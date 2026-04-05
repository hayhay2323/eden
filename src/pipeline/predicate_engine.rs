use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveMarketRegime, LivePressure, LiveSignal, LiveStressSnapshot,
    LiveTacticalCase,
};
use crate::math::clamp_unit_interval;
use crate::ontology::{ActionDirection, ActionNode, AtomicPredicate, AtomicPredicateKind};

#[path = "predicate_engine/contextual.rs"]
mod contextual;
#[path = "predicate_engine/market_structure.rs"]
mod market_structure;
#[path = "predicate_engine/position.rs"]
mod position;
#[path = "predicate_engine/review.rs"]
mod review;
#[path = "predicate_engine/signal_context.rs"]
mod signal_context;
#[path = "predicate_engine/state_dynamics.rs"]
mod state_dynamics;
use contextual::{
    counterevidence_present, cross_market_dislocation, leader_flip_detected,
    sector_rotation_pressure,
};
use market_structure::{
    broker_cluster_aligned, broker_concentration_risk, broker_replenish_active,
    consolidation_before_breakout, regime_stability,
};
use position::{
    concentration_risk, exit_condition_forming, position_conflict, position_reinforcement,
};
pub(crate) use review::derive_human_review_context;
use review::human_rejected;
use signal_context::{
    cross_market_link_active, cross_scope_propagation, event_catalyst_active, liquidity_imbalance,
    mean_reversion_pressure, price_reasoning_divergence, source_concentrated,
};
use state_dynamics::{
    confidence_builds, pressure_persists, signal_recurs, stress_accelerating,
    structural_degradation,
};

pub struct PredicateInputs<'a> {
    pub tactical_case: &'a LiveTacticalCase,
    pub active_positions: &'a [ActionNode],
    pub chain: Option<&'a LiveBackwardChain>,
    pub pressure: Option<&'a LivePressure>,
    pub signal: Option<&'a LiveSignal>,
    pub causal: Option<&'a LiveCausalLeader>,
    pub track: Option<&'a LiveHypothesisTrack>,
    pub stress: &'a LiveStressSnapshot,
    pub market_regime: &'a LiveMarketRegime,
    pub all_signals: &'a [LiveSignal],
    pub all_pressures: &'a [LivePressure],
    pub events: &'a [LiveEvent],
    pub cross_market_signals: &'a [LiveCrossMarketSignal],
    pub cross_market_anomalies: &'a [LiveCrossMarketAnomaly],
}

impl PredicateInputs<'_> {
    fn events_for_symbol(&self) -> Vec<&LiveEvent> {
        let symbol = self.tactical_case.symbol.trim();
        if symbol.is_empty() {
            return Vec::new();
        }

        let symbol_upper = symbol.to_uppercase();
        self.events
            .iter()
            .filter(|event| {
                // Prefer structured symbol match when available; fall back to summary
                // containment only for events without a symbol field.
                if let Some(ref event_symbol) = event.symbol {
                    event_symbol.to_uppercase() == symbol_upper
                } else {
                    // Fallback: require the symbol appears as a distinct token in the summary
                    // to avoid false matches for short symbols like "C.US".
                    let summary = event.summary.to_uppercase();
                    summary
                        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
                        .any(|token| token == symbol_upper)
                }
            })
            .collect()
    }
}

pub fn derive_atomic_predicates(inputs: &PredicateInputs<'_>) -> Vec<AtomicPredicate> {
    let mut predicates = vec![
        signal_recurs(inputs),
        confidence_builds(inputs),
        pressure_persists(inputs),
        cross_scope_propagation(inputs),
        cross_market_link_active(inputs),
        source_concentrated(inputs),
        structural_degradation(inputs),
        stress_accelerating(inputs),
        price_reasoning_divergence(inputs),
        event_catalyst_active(inputs),
        liquidity_imbalance(inputs),
        mean_reversion_pressure(inputs),
        cross_market_dislocation(inputs),
        sector_rotation_pressure(inputs),
        leader_flip_detected(inputs),
        counterevidence_present(inputs),
        position_conflict(inputs),
        position_reinforcement(inputs),
        concentration_risk(inputs),
        exit_condition_forming(inputs),
        regime_stability(inputs),
        consolidation_before_breakout(inputs),
        broker_replenish_active(inputs),
        broker_cluster_aligned(inputs),
        broker_concentration_risk(inputs),
    ];

    predicates.retain(|predicate| predicate.score > dec!(0.15));
    predicates.sort_by(|left, right| right.score.cmp(&left.score));
    predicates
}

pub fn augment_predicates_with_workflow(
    predicates: &[AtomicPredicate],
    workflow_state: &str,
    workflow_note: Option<&str>,
) -> Vec<AtomicPredicate> {
    let human_review = derive_human_review_context(workflow_state, workflow_note);
    let mut next = predicates
        .iter()
        .filter(|predicate| predicate.kind != AtomicPredicateKind::HumanRejected)
        .cloned()
        .collect::<Vec<_>>();

    if let Some(predicate) = human_review.as_ref().and_then(human_rejected) {
        next.push(predicate);
    }

    next.sort_by(|left, right| right.score.cmp(&left.score));
    next
}

pub(super) fn predicate(
    kind: AtomicPredicateKind,
    score: Decimal,
    summary: &str,
    evidence: Vec<String>,
) -> AtomicPredicate {
    AtomicPredicate {
        kind,
        label: kind.label().to_string(),
        law: kind.law(),
        score: clamp_unit_interval(score),
        summary: summary.to_string(),
        evidence,
    }
}

pub(super) fn evidence_concentration(items: &[crate::live_snapshot::LiveEvidence]) -> Decimal {
    let total = items
        .iter()
        .fold(Decimal::ZERO, |acc, item| acc + item.weight.abs());
    if total <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let peak = items
        .iter()
        .map(|item| item.weight.abs())
        .max()
        .unwrap_or(Decimal::ZERO);
    clamp_unit_interval(peak / total)
}

pub(super) fn active_positions_for_symbol<'a>(
    inputs: &'a PredicateInputs<'_>,
) -> Vec<&'a ActionNode> {
    let symbol = inputs.tactical_case.symbol.trim();
    if symbol.is_empty() {
        return Vec::new();
    }
    inputs
        .active_positions
        .iter()
        .filter(|position| position.symbol.0 == symbol)
        .collect()
}

pub(super) fn case_direction(inputs: &PredicateInputs<'_>) -> ActionDirection {
    if let Some(signal) = inputs.signal {
        if signal.composite > Decimal::ZERO {
            return ActionDirection::Long;
        }
        if signal.composite < Decimal::ZERO {
            return ActionDirection::Short;
        }
        let signed_flow = signal.capital_flow_direction + signal.price_momentum;
        if signed_flow > Decimal::ZERO {
            return ActionDirection::Long;
        }
        if signed_flow < Decimal::ZERO {
            return ActionDirection::Short;
        }
    }

    if let Some(pressure) = inputs.pressure {
        if pressure.capital_flow_pressure > Decimal::ZERO {
            return ActionDirection::Long;
        }
        if pressure.capital_flow_pressure < Decimal::ZERO {
            return ActionDirection::Short;
        }
    }

    if inputs.tactical_case.title.starts_with("Long ") {
        ActionDirection::Long
    } else if inputs.tactical_case.title.starts_with("Short ") {
        ActionDirection::Short
    } else {
        ActionDirection::Neutral
    }
}

pub(super) fn directions_conflict(
    case_direction: ActionDirection,
    active_direction: ActionDirection,
) -> bool {
    matches!(
        (case_direction, active_direction),
        (ActionDirection::Long, ActionDirection::Short)
            | (ActionDirection::Short, ActionDirection::Long)
    )
}

pub(super) fn directions_align(
    case_direction: ActionDirection,
    active_direction: ActionDirection,
) -> bool {
    matches!(
        (case_direction, active_direction),
        (ActionDirection::Long, ActionDirection::Long)
            | (ActionDirection::Short, ActionDirection::Short)
    )
}

pub(super) fn direction_label(direction: ActionDirection) -> &'static str {
    match direction {
        ActionDirection::Long => "long",
        ActionDirection::Short => "short",
        ActionDirection::Neutral => "neutral",
    }
}

pub(super) fn stage_label(position: &ActionNode) -> &'static str {
    match position.stage {
        crate::ontology::ActionNodeStage::Suggested => "suggested",
        crate::ontology::ActionNodeStage::Confirmed => "confirmed",
        crate::ontology::ActionNodeStage::Executed => "executed",
        crate::ontology::ActionNodeStage::Monitoring => "monitoring",
        crate::ontology::ActionNodeStage::Reviewed => "reviewed",
    }
}

pub(super) fn case_sector(inputs: &PredicateInputs<'_>) -> Option<String> {
    inputs
        .signal
        .and_then(|signal| signal.sector.clone())
        .or_else(|| inputs.pressure.and_then(|pressure| pressure.sector.clone()))
        .map(|sector| sector.trim().to_string())
        .filter(|sector| !sector.is_empty())
}

pub(super) fn weighted_sum(items: &[(Decimal, Decimal)]) -> Decimal {
    clamp_unit_interval(items.iter().fold(Decimal::ZERO, |acc, (value, weight)| {
        acc + clamp_unit_interval(*value) * *weight
    }))
}

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    clamp_unit_interval(
        values.iter().fold(Decimal::ZERO, |acc, value| acc + *value)
            / Decimal::from(values.len() as i64),
    )
}

pub(super) fn normalize_count(count: usize, max: usize) -> Decimal {
    if max == 0 {
        return Decimal::ZERO;
    }
    clamp_unit_interval(Decimal::from(count as i64) / Decimal::from(max as i64))
}

fn normalize_ratio(value: Decimal, max: Decimal) -> Decimal {
    if max <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    clamp_unit_interval(value / max)
}

#[cfg(test)]
#[path = "predicate_engine_tests.rs"]
mod tests;
