use std::collections::HashSet;

use crate::ontology::reasoning::{Hypothesis, PropagationPath};
use crate::pipeline::attention_budget::{AttentionBudgetAllocator, AttentionLevel};
use crate::us::pipeline::signals::UsSignalScope;

use super::*;

#[derive(Debug, Clone, Default)]
pub(in crate::us::runtime) struct UsVortexAttention {
    pub(in crate::us::runtime) deep_symbols: HashSet<Symbol>,
    pub(in crate::us::runtime) standard_symbols: HashSet<Symbol>,
}

#[derive(Debug, Clone)]
pub(in crate::us::runtime) struct UsReasoningAttentionPlan {
    pub(in crate::us::runtime) deep_symbols: HashSet<Symbol>,
    pub(in crate::us::runtime) standard_symbols: HashSet<Symbol>,
}

impl UsReasoningAttentionPlan {
    pub(in crate::us::runtime) fn active_symbols(&self) -> HashSet<Symbol> {
        self.deep_symbols
            .iter()
            .cloned()
            .chain(self.standard_symbols.iter().cloned())
            .collect()
    }
}

pub(in crate::us::runtime) fn attention_reasoning_plan(
    stock_nodes: impl Iterator<Item = Symbol>,
    attention: &AttentionBudgetAllocator,
    previous_setups: &[TacticalSetup],
    previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
    vortex_attention: &UsVortexAttention,
) -> UsReasoningAttentionPlan {
    let stock_nodes = stock_nodes.collect::<Vec<_>>();
    let previously_active = previous_setups
        .iter()
        .filter_map(|setup| match &setup.scope {
            crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        })
        .chain(
            previous_tracks
                .iter()
                .filter_map(|track| match &track.scope {
                    crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => {
                        Some(symbol.clone())
                    }
                    _ => None,
                }),
        )
        .collect::<HashSet<_>>();

    let mut deep_symbols = HashSet::new();
    let mut standard_symbols = HashSet::new();

    for symbol in &stock_nodes {
        if vortex_attention.deep_symbols.contains(symbol) {
            deep_symbols.insert(symbol.clone());
            continue;
        }

        let base_attention = attention.attention_for(&symbol.0);
        match base_attention {
            AttentionLevel::Deep => {
                deep_symbols.insert(symbol.clone());
            }
            AttentionLevel::Standard
                if previously_active.contains(symbol)
                    || vortex_attention.standard_symbols.contains(symbol) =>
            {
                standard_symbols.insert(symbol.clone());
            }
            AttentionLevel::Standard | AttentionLevel::Scan | AttentionLevel::Skip => {
                if vortex_attention.standard_symbols.contains(symbol) {
                    standard_symbols.insert(symbol.clone());
                }
            }
        }
    }

    if deep_symbols.is_empty() && standard_symbols.is_empty() {
        deep_symbols.extend(stock_nodes);
    }

    UsReasoningAttentionPlan {
        deep_symbols,
        standard_symbols,
    }
}

pub(in crate::us::runtime) fn filter_us_event_snapshot_for_reasoning(
    snapshot: &UsEventSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> UsEventSnapshot {
    let events = snapshot
        .events
        .iter()
        .filter(|event| match &event.value.scope {
            UsSignalScope::Symbol(symbol) => active_symbols.contains(symbol),
            _ => true,
        })
        .cloned()
        .collect();
    UsEventSnapshot {
        timestamp: snapshot.timestamp,
        events,
    }
}

pub(in crate::us::runtime) fn filter_us_derived_signal_snapshot_for_reasoning(
    snapshot: &UsDerivedSignalSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> UsDerivedSignalSnapshot {
    let signals = snapshot
        .signals
        .iter()
        .filter(|signal| match &signal.value.scope {
            UsSignalScope::Symbol(symbol) => active_symbols.contains(symbol),
            _ => true,
        })
        .cloned()
        .collect();
    UsDerivedSignalSnapshot {
        timestamp: snapshot.timestamp,
        signals,
    }
}

pub(in crate::us::runtime) fn filter_us_decision_for_reasoning(
    decision: &crate::us::graph::decision::UsDecisionSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> crate::us::graph::decision::UsDecisionSnapshot {
    let mut filtered = decision.clone();
    filtered
        .convergence_scores
        .retain(|symbol, _| active_symbols.contains(symbol));
    filtered
        .order_suggestions
        .retain(|suggestion| active_symbols.contains(&suggestion.symbol));
    filtered
}

pub(in crate::us::runtime) fn merge_us_standard_attention_maintenance(
    reasoning_snapshot: &mut UsReasoningSnapshot,
    previous_tick: Option<&UsTickRecord>,
    standard_symbols: &HashSet<Symbol>,
    previous_setups: &[TacticalSetup],
    _previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
    _timestamp: time::OffsetDateTime,
) {
    if standard_symbols.is_empty() {
        return;
    }

    let mut seen_hypotheses = reasoning_snapshot
        .hypotheses
        .iter()
        .map(|item| item.hypothesis_id.clone())
        .collect::<HashSet<_>>();
    if let Some(previous_tick) = previous_tick {
        for hypothesis in &previous_tick.hypotheses {
            let carries_symbol_scope = matches!(
                &hypothesis.scope,
                crate::ontology::reasoning::ReasoningScope::Symbol(symbol)
                    if standard_symbols.contains(symbol)
            );
            if carries_symbol_scope && seen_hypotheses.insert(hypothesis.hypothesis_id.clone()) {
                reasoning_snapshot.hypotheses.push(hypothesis.clone());
            }
        }
    }

    let mut seen_setups = reasoning_snapshot
        .tactical_setups
        .iter()
        .map(|item| item.setup_id.clone())
        .collect::<HashSet<_>>();
    for setup in previous_setups {
        let carries_symbol_scope = matches!(
            &setup.scope,
            crate::ontology::reasoning::ReasoningScope::Symbol(symbol)
                if standard_symbols.contains(symbol)
        );
        if carries_symbol_scope && seen_setups.insert(setup.setup_id.clone()) {
            reasoning_snapshot.tactical_setups.push(
                crate::ontology::reasoning::sanitize_carried_tactical_setup(setup),
            );
        }
    }

    reasoning_snapshot.hypothesis_tracks = Vec::new();
}

pub(in crate::us::runtime) fn derive_us_vortex_attention(
    hypotheses: &[Hypothesis],
    propagation_paths: &[PropagationPath],
) -> UsVortexAttention {
    let mut deep_symbols = HashSet::new();
    let mut standard_symbols = HashSet::new();

    for hypothesis in hypotheses.iter().filter(|item| {
        matches!(
            item.kind,
            Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis)
        )
    }) {
        if let crate::ontology::reasoning::ReasoningScope::Symbol(symbol) = &hypothesis.scope {
            deep_symbols.insert(symbol.clone());
        }

        let path_id_set = hypothesis
            .propagation_path_ids
            .iter()
            .collect::<HashSet<_>>();
        for path in propagation_paths
            .iter()
            .filter(|path| path_id_set.contains(&path.path_id))
        {
            for step in &path.steps {
                for scope in [&step.from, &step.to] {
                    if let crate::ontology::reasoning::ReasoningScope::Symbol(symbol) = scope {
                        if !matches!(
                            &hypothesis.scope,
                            crate::ontology::reasoning::ReasoningScope::Symbol(center)
                                if center == symbol
                        ) {
                            standard_symbols.insert(symbol.clone());
                        }
                    }
                }
            }
        }
    }

    standard_symbols.retain(|symbol| !deep_symbols.contains(symbol));
    UsVortexAttention {
        deep_symbols,
        standard_symbols,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::reasoning::{
        default_case_horizon, DecisionLineage, HypothesisTrackStatus, PropagationStep,
    };
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn symbol(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn prov(tag: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, time::OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(tag)
            .with_inputs([tag.to_string()])
    }

    #[test]
    fn us_attention_plan_falls_back_to_all_symbols_when_unseeded() {
        let allocator = AttentionBudgetAllocator::from_universe_size(50);
        let plan = attention_reasoning_plan(
            [symbol("AAPL.US"), symbol("MSFT.US")].into_iter(),
            &allocator,
            &[],
            &[],
            &UsVortexAttention::default(),
        );

        assert!(plan.deep_symbols.contains(&symbol("AAPL.US")));
        assert!(plan.deep_symbols.contains(&symbol("MSFT.US")));
    }

    #[test]
    fn us_attention_plan_promotes_vortex_center_and_edges() {
        let mut allocator = AttentionBudgetAllocator::from_universe_size(100);
        for (name, change_pct) in [
            ("NVDA.US", 20.0),
            ("TSLA.US", 18.0),
            ("META.US", 16.0),
            ("MSFT.US", 14.0),
            ("AAPL.US", 12.0),
        ] {
            allocator.update_activity(name, true, true, change_pct, 1, true);
        }
        allocator.update_activity("BABA.US", false, false, 0.02, 0, false);
        allocator.update_activity("PDD.US", false, false, 0.02, 0, false);

        let plan = attention_reasoning_plan(
            [symbol("BABA.US"), symbol("PDD.US")].into_iter(),
            &allocator,
            &[],
            &[],
            &UsVortexAttention {
                deep_symbols: HashSet::from([symbol("BABA.US")]),
                standard_symbols: HashSet::from([symbol("PDD.US")]),
            },
        );

        assert!(plan.deep_symbols.contains(&symbol("BABA.US")));
        // With only 7 symbols and 10 deep slots, allocator assigns Deep to all,
        // so PDD.US lands in deep_symbols even though vortex only marks it standard.
        assert!(plan.deep_symbols.contains(&symbol("PDD.US")));
    }

    #[test]
    fn derive_us_vortex_attention_extracts_center_and_edges() {
        let hypotheses = vec![Hypothesis {
            hypothesis_id: "hyp:BABA.US:convergence_hypothesis".into(),
            kind: Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis),
            family_label: "Convergence Hypothesis".into(),
            provenance: prov("hyp:BABA.US"),
            scope: crate::ontology::reasoning::ReasoningScope::Symbol(symbol("BABA.US")),
            statement: "BABA.US shows an emergent convergence vortex".into(),
            confidence: dec!(0.74),
            local_support_weight: dec!(0.6),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.4),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:cm".into()],
            expected_observations: vec![],
        }];
        let paths = vec![PropagationPath {
            path_id: "path:cm".into(),
            summary: "9988.HK may diffuse into BABA.US".into(),
            confidence: dec!(0.55),
            steps: vec![
                PropagationStep {
                    from: crate::ontology::reasoning::ReasoningScope::Symbol(symbol("9988.HK")),
                    to: crate::ontology::reasoning::ReasoningScope::Symbol(symbol("BABA.US")),
                    mechanism: "cross-market diffusion".into(),
                    confidence: dec!(0.55),
                    polarity: 1,
                    references: vec![],
                },
                PropagationStep {
                    from: crate::ontology::reasoning::ReasoningScope::Symbol(symbol("BABA.US")),
                    to: crate::ontology::reasoning::ReasoningScope::Symbol(symbol("PDD.US")),
                    mechanism: "stock diffusion".into(),
                    confidence: dec!(0.50),
                    polarity: 1,
                    references: vec![],
                },
            ],
        }];

        let attention = derive_us_vortex_attention(&hypotheses, &paths);

        assert!(attention.deep_symbols.contains(&symbol("BABA.US")));
        assert!(attention.standard_symbols.contains(&symbol("9988.HK")));
        assert!(attention.standard_symbols.contains(&symbol("PDD.US")));
    }

    #[test]
    fn merge_us_standard_attention_maintenance_preserves_previous_symbol_state() {
        let standard_symbol = symbol("BABA.US");
        let previous_setup = TacticalSetup {
            setup_id: "setup:BABA.US:review".into(),
            hypothesis_id: "hyp:BABA.US:momentum_continuation".into(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:BABA.US"),
            lineage: DecisionLineage::default(),
            scope: crate::ontology::reasoning::ReasoningScope::Symbol(standard_symbol.clone()),
            title: "Long BABA.US".into(),
            action: "review".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.62),
            confidence_gap: dec!(0.10),
            heuristic_edge: dec!(0.08),
            convergence_score: Some(dec!(0.45)),
            convergence_detail: None,
            workflow_id: Some("order:BABA.US:buy".into()),
            entry_rationale: "carry".into(),
            causal_narrative: None,
            risk_notes: vec![
                "family=Momentum Continuation".into(),
                "phase=Growing".into(),
                "velocity=0.1234".into(),
                "acceleration=0.4321".into(),
            ],
            review_reason_code: None,
            policy_verdict: None,
        };
        let previous_hypothesis = Hypothesis {
            hypothesis_id: previous_setup.hypothesis_id.clone(),
            kind: None,
            family_label: "Momentum Continuation".into(),
            provenance: prov("hyp:BABA.US"),
            scope: crate::ontology::reasoning::ReasoningScope::Symbol(standard_symbol.clone()),
            statement: "BABA.US momentum persists".into(),
            confidence: dec!(0.62),
            local_support_weight: dec!(0.4),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        };
        let previous_track = crate::ontology::reasoning::HypothesisTrack {
            track_id: "track:BABA.US".into(),
            setup_id: previous_setup.setup_id.clone(),
            hypothesis_id: previous_setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: None,
            scope: crate::ontology::reasoning::ReasoningScope::Symbol(standard_symbol.clone()),
            title: previous_setup.title.clone(),
            action: previous_setup.action.to_string(),
            status: HypothesisTrackStatus::Stable,
            age_ticks: 3,
            status_streak: 2,
            confidence: previous_setup.confidence,
            previous_confidence: Some(previous_setup.confidence),
            confidence_change: Decimal::ZERO,
            confidence_gap: previous_setup.confidence_gap,
            previous_confidence_gap: Some(previous_setup.confidence_gap),
            confidence_gap_change: Decimal::ZERO,
            heuristic_edge: previous_setup.heuristic_edge,
            policy_reason: "maintain".into(),
            transition_reason: None,
            first_seen_at: time::OffsetDateTime::UNIX_EPOCH,
            last_updated_at: time::OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let previous_tick = UsTickRecord {
            tick_number: 1,
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            signals: HashMap::new(),
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![previous_hypothesis.clone()],
            tactical_setups: vec![previous_setup.clone()],
            market_regime: crate::us::graph::decision::UsMarketRegimeBias::Neutral,
        };
        let mut reasoning_snapshot = UsReasoningSnapshot {
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![],
            propagation_paths: vec![],
            investigation_selections: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
        };

        merge_us_standard_attention_maintenance(
            &mut reasoning_snapshot,
            Some(&previous_tick),
            &HashSet::from([standard_symbol.clone()]),
            std::slice::from_ref(&previous_setup),
            std::slice::from_ref(&previous_track),
            time::OffsetDateTime::UNIX_EPOCH,
        );

        assert!(reasoning_snapshot
            .hypotheses
            .iter()
            .any(|item| item.hypothesis_id == previous_hypothesis.hypothesis_id));
        assert!(reasoning_snapshot
            .tactical_setups
            .iter()
            .any(|item| item.setup_id == previous_setup.setup_id));
        let carried = reasoning_snapshot
            .tactical_setups
            .iter()
            .find(|item| item.setup_id == previous_setup.setup_id)
            .expect("carried setup");
        assert_eq!(
            carried.review_reason_code,
            Some(crate::ontology::reasoning::ReviewReasonCode::StaleSymbolConfirmation)
        );
        assert!(carried
            .risk_notes
            .iter()
            .all(|note| !note.starts_with("velocity=") && !note.starts_with("acceleration=")));
        // hypothesis_tracks are no longer generated by the reasoning pipeline
        // (pressure field redesign removed template-based track derivation)
        assert!(reasoning_snapshot.hypothesis_tracks.is_empty());
    }
}
