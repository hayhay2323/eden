use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::scope_node_id;
use crate::ontology::world::{BackwardInvestigation, CausalContestState};
use crate::ontology::ReasoningScope;

use super::buffer::TickHistory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CausalFlipStyle {
    Sudden,
    ErosionDriven,
}

impl CausalFlipStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sudden => "sudden",
            Self::ErosionDriven => "erosion-driven",
        }
    }
}

impl std::fmt::Display for CausalFlipStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalTimelinePoint {
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub contest_state: CausalContestState,
    pub leading_cause_id: Option<String>,
    pub leading_explanation: Option<String>,
    pub cause_gap: Option<Decimal>,
    pub leading_support_delta: Option<Decimal>,
    pub leading_contradict_delta: Option<Decimal>,
    pub leader_transition_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalFlipEvent {
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub from_cause_id: String,
    pub from_explanation: String,
    pub to_cause_id: String,
    pub to_explanation: String,
    pub style: CausalFlipStyle,
    pub cause_gap: Option<Decimal>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalTimeline {
    pub leaf_scope_key: String,
    pub leaf_label: String,
    pub points: Vec<CausalTimelinePoint>,
    pub flip_events: Vec<CausalFlipEvent>,
}

impl CausalTimeline {
    pub fn latest_point(&self) -> Option<&CausalTimelinePoint> {
        self.points.last()
    }

    pub fn latest_flip(&self) -> Option<&CausalFlipEvent> {
        self.flip_events.last()
    }

    pub fn latest_flip_style(&self) -> Option<CausalFlipStyle> {
        self.latest_flip().map(|flip| flip.style)
    }

    pub fn recent_leader_sequence(&self, limit: usize) -> Vec<String> {
        let mut sequence = Vec::new();
        for point in self.points.iter().rev() {
            let Some(explanation) = point.leading_explanation.as_ref() else {
                continue;
            };
            if sequence.last() != Some(explanation) {
                sequence.push(explanation.clone());
            }
            if sequence.len() >= limit {
                break;
            }
        }
        sequence.reverse();
        sequence
    }
}

pub fn compute_causal_timelines(history: &TickHistory) -> HashMap<String, CausalTimeline> {
    let records = history.latest_n(history.len());
    let mut grouped: HashMap<String, Vec<(u64, OffsetDateTime, &BackwardInvestigation)>> =
        HashMap::new();

    for record in records {
        for investigation in &record.backward_reasoning.investigations {
            grouped
                .entry(scope_key(&investigation.leaf_scope))
                .or_default()
                .push((record.tick_number, record.timestamp, investigation));
        }
    }

    grouped
        .into_iter()
        .map(|(leaf_scope_key, observations)| {
            let mut points: Vec<CausalTimelinePoint> = Vec::new();
            let mut flip_events: Vec<CausalFlipEvent> = Vec::new();

            for (index, (tick_number, timestamp, investigation)) in observations.iter().enumerate()
            {
                let point = CausalTimelinePoint {
                    tick_number: *tick_number,
                    timestamp: *timestamp,
                    contest_state: investigation.contest_state,
                    leading_cause_id: investigation
                        .leading_cause
                        .as_ref()
                        .map(|cause| cause.cause_id.clone()),
                    leading_explanation: investigation
                        .leading_cause
                        .as_ref()
                        .map(|cause| cause.explanation.clone()),
                    cause_gap: investigation.cause_gap,
                    leading_support_delta: investigation.leading_support_delta,
                    leading_contradict_delta: investigation.leading_contradict_delta,
                    leader_transition_summary: investigation.leader_transition_summary.clone(),
                };

                if index > 0 {
                    let previous = &points[index - 1];
                    if let (Some(prev_id), Some(curr_id)) = (
                        previous.leading_cause_id.as_ref(),
                        point.leading_cause_id.as_ref(),
                    ) {
                        if prev_id != curr_id {
                            let style = classify_flip_style(&points, index - 1, &point);
                            flip_events.push(CausalFlipEvent {
                                tick_number: point.tick_number,
                                timestamp: point.timestamp,
                                from_cause_id: prev_id.clone(),
                                from_explanation: previous
                                    .leading_explanation
                                    .clone()
                                    .unwrap_or_else(|| prev_id.clone()),
                                to_cause_id: curr_id.clone(),
                                to_explanation: point
                                    .leading_explanation
                                    .clone()
                                    .unwrap_or_else(|| curr_id.clone()),
                                style,
                                cause_gap: point.cause_gap,
                                summary: point.leader_transition_summary.clone().unwrap_or_else(
                                    || format!("leader flipped from {} to {}", prev_id, curr_id),
                                ),
                            });
                        }
                    }
                }

                points.push(point);
            }

            let leaf_label = observations
                .last()
                .map(|(_, _, investigation)| investigation.leaf_label.clone())
                .unwrap_or_else(|| leaf_scope_key.clone());

            (
                leaf_scope_key.clone(),
                CausalTimeline {
                    leaf_scope_key,
                    leaf_label,
                    points,
                    flip_events,
                },
            )
        })
        .collect()
}

fn classify_flip_style(
    points: &[CausalTimelinePoint],
    previous_index: usize,
    current_point: &CausalTimelinePoint,
) -> CausalFlipStyle {
    let lookback = points
        .iter()
        .take(previous_index + 1)
        .rev()
        .take_while(|point| point.leading_cause_id == points[previous_index].leading_cause_id)
        .collect::<Vec<_>>();
    let had_erosion_signal = lookback.iter().any(|point| {
        point.contest_state == CausalContestState::Eroding
            || point.contest_state == CausalContestState::Contested
            || point.leading_contradict_delta.unwrap_or(Decimal::ZERO) > Decimal::ZERO
    });

    if had_erosion_signal || current_point.contest_state == CausalContestState::Flipped {
        CausalFlipStyle::ErosionDriven
    } else {
        CausalFlipStyle::Sudden
    }
}

fn scope_key(scope: &ReasoningScope) -> String {
    scope_node_id(scope)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::ontology::world::{
        BackwardCause, BackwardReasoningSnapshot, CausalContestState, WorldStateSnapshot,
    };
    use crate::ontology::Symbol;
    use crate::temporal::record::{SymbolSignals, TickRecord};

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn empty_signal() -> SymbolSignals {
        SymbolSignals {
            mark_price: None,
            composite: Decimal::ZERO,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: None,
            convergence_score: None,
            composite_degradation: None,
            institution_retention: None,
            edge_stability: None,
            temporal_weight: None,
            microstructure_confirmation: None,
            component_spread: None,
            institutional_edge_age: None,
        }
    }

    fn make_tick(
        tick_number: u64,
        contest_state: CausalContestState,
        leading_id: &str,
        leading_explanation: &str,
        cause_gap: Decimal,
        contradict_delta: Decimal,
    ) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(sym("700.HK"), empty_signal());
        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick_number as i64),
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
                perceptual_states: vec![],
                vortices: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![BackwardInvestigation {
                    investigation_id: "backward:700.HK".into(),
                    leaf_scope: ReasoningScope::Symbol(sym("700.HK")),
                    leaf_label: "Long 700.HK".into(),
                    leaf_regime: "review".into(),
                    contest_state,
                    leading_cause_streak: 1,
                    previous_leading_cause_id: None,
                    leading_cause: Some(BackwardCause {
                        cause_id: leading_id.into(),
                        scope: ReasoningScope::market(),
                        layer: crate::ontology::WorldLayer::Forest,
                        depth: 1,
                        provenance: crate::ontology::ProvenanceMetadata::new(
                            crate::ontology::ProvenanceSource::Computed,
                            OffsetDateTime::UNIX_EPOCH,
                        )
                        .with_trace_id(leading_id)
                        .with_inputs([leading_explanation]),
                        explanation: leading_explanation.into(),
                        chain_summary: None,
                        confidence: dec!(0.6),
                        support_weight: dec!(0.6),
                        contradict_weight: Decimal::ZERO,
                        net_conviction: dec!(0.6),
                        competitive_score: dec!(0.7),
                        falsifier: None,
                        supporting_evidence: vec![],
                        contradicting_evidence: vec![],
                        references: vec![],
                    }),
                    runner_up_cause: None,
                    cause_gap: Some(cause_gap),
                    leading_support_delta: Some(Decimal::ZERO),
                    leading_contradict_delta: Some(contradict_delta),
                    leader_transition_summary: Some("transition".into()),
                    leading_falsifier: None,
                    candidate_causes: vec![],
                }],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        }
    }

    #[test]
    fn compute_causal_timelines_detects_erosion_driven_flip() {
        let mut history = TickHistory::new(10);
        history.push(make_tick(
            1,
            CausalContestState::Stable,
            "cause:market:700.HK",
            "market dominates",
            dec!(0.14),
            Decimal::ZERO,
        ));
        history.push(make_tick(
            2,
            CausalContestState::Eroding,
            "cause:market:700.HK",
            "market dominates",
            dec!(0.04),
            dec!(0.08),
        ));
        history.push(make_tick(
            3,
            CausalContestState::Flipped,
            "cause:sector:700.HK:sector:tech",
            "sector takes over",
            dec!(0.09),
            Decimal::ZERO,
        ));

        let timelines = compute_causal_timelines(&history);
        let timeline = timelines.get("symbol:700.hk").expect("timeline");
        assert_eq!(timeline.points.len(), 3);
        assert_eq!(timeline.flip_events.len(), 1);
        assert_eq!(
            timeline.latest_flip_style(),
            Some(CausalFlipStyle::ErosionDriven)
        );
        assert_eq!(
            timeline.recent_leader_sequence(3),
            vec![
                "market dominates".to_string(),
                "sector takes over".to_string()
            ]
        );
    }
}
