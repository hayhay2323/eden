use super::*;

pub(crate) fn build_hk_backward_chains(
    snapshot: &eden::BackwardReasoningSnapshot,
) -> Vec<LiveBackwardChain> {
    snapshot
        .investigations
        .iter()
        .filter_map(|item| {
            let symbol = extract_symbol_scope(&item.leaf_scope)?;
            let leading = item
                .leading_cause
                .as_ref()
                .or_else(|| item.candidate_causes.first())?;

            let mut evidence = leading
                .supporting_evidence
                .iter()
                .map(|e| LiveEvidence {
                    source: e.channel.clone(),
                    description: e.statement.clone(),
                    weight: e.weight,
                    direction: e.weight,
                })
                .collect::<Vec<_>>();
            evidence.extend(leading.contradicting_evidence.iter().map(|e| LiveEvidence {
                source: e.channel.clone(),
                description: e.statement.clone(),
                weight: e.weight,
                direction: -e.weight,
            }));

            Some(LiveBackwardChain {
                symbol: symbol.0.clone(),
                conclusion: format!("{} — 主因: {}", item.leaf_label, leading.explanation),
                primary_driver: leading.explanation.clone(),
                confidence: leading.confidence,
                evidence,
            })
        })
        .take(10)
        .collect()
}

pub(crate) fn build_hk_structural_deltas(
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    dynamics: &std::collections::HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
) -> Vec<eden::live_snapshot::LiveStructuralDelta> {
    let mut items = dynamics
        .values()
        .filter(|item| {
            item.composite_delta.abs() >= Decimal::new(2, 2)
                || item.composite_acceleration.abs() >= Decimal::new(1, 2)
        })
        .map(|item| eden::live_snapshot::LiveStructuralDelta {
            symbol: item.symbol.0.clone(),
            sector: sector_name_for_symbol(store, &item.symbol),
            composite_delta: item.composite_delta,
            composite_acceleration: item.composite_acceleration,
            capital_flow_delta: item.inst_alignment_delta,
            flow_persistence: item.composite_duration,
            flow_reversal: item.composite_delta.signum() != Decimal::ZERO
                && item.composite_acceleration.signum() != Decimal::ZERO
                && item.composite_delta.signum() != item.composite_acceleration.signum(),
            pre_market_trend: Decimal::ZERO,
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.composite_delta
            .abs()
            .cmp(&a.composite_delta.abs())
            .then_with(|| {
                b.composite_acceleration
                    .abs()
                    .cmp(&a.composite_acceleration.abs())
            })
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    items.truncate(64);
    items
}

pub(crate) fn build_hk_propagation_senses(
    reasoning_snapshot: &ReasoningSnapshot,
    dynamics: &std::collections::HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
) -> Vec<eden::live_snapshot::LivePropagationSense> {
    let mut items = reasoning_snapshot
        .propagation_paths
        .iter()
        .filter_map(|path| {
            let first = path.steps.first()?;
            let last = path.steps.last()?;
            let eden::ReasoningScope::Symbol(target_symbol) = &last.to else {
                return None;
            };
            let source_label = hk_scope_label(&first.from);
            if source_label == target_symbol.0 {
                return None;
            }

            let source_delta = match &first.from {
                eden::ReasoningScope::Symbol(source_symbol) => dynamics
                    .get(source_symbol)
                    .map(|item| item.composite_delta)
                    .unwrap_or(path.confidence),
                _ => path.confidence,
            };
            let target_delta = dynamics
                .get(target_symbol)
                .map(|item| item.composite_delta)
                .unwrap_or(Decimal::ZERO);
            let lag_gap = (source_delta.abs() - target_delta.abs()).max(Decimal::ZERO);
            if path.confidence <= Decimal::ZERO || lag_gap <= Decimal::ZERO {
                return None;
            }

            Some(eden::live_snapshot::LivePropagationSense {
                source_symbol: source_label,
                target_symbol: target_symbol.0.clone(),
                channel: first.mechanism.clone(),
                propagation_strength: path.confidence,
                target_momentum: target_delta,
                lag_gap,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.propagation_strength
            .cmp(&a.propagation_strength)
            .then_with(|| b.lag_gap.cmp(&a.lag_gap))
            .then_with(|| a.source_symbol.cmp(&b.source_symbol))
            .then_with(|| a.target_symbol.cmp(&b.target_symbol))
    });
    items.dedup_by(|a, b| {
        a.source_symbol == b.source_symbol
            && a.target_symbol == b.target_symbol
            && a.channel == b.channel
    });
    items.truncate(32);
    items
}

pub(crate) fn hk_causal_leader_streak(timeline: &CausalTimeline) -> u64 {
    let Some(latest) = timeline.points.last() else {
        return 0;
    };
    let latest_id = latest.leading_cause_id.as_deref();
    timeline
        .points
        .iter()
        .rev()
        .take_while(|point| point.leading_cause_id.as_deref() == latest_id)
        .count() as u64
}

pub(crate) fn build_hk_causal_leaders(
    timelines: &std::collections::HashMap<String, CausalTimeline>,
) -> Vec<LiveCausalLeader> {
    let mut items = timelines
        .values()
        .filter(|timeline| timeline.leaf_scope_key.ends_with(".HK"))
        .filter_map(|timeline| {
            let current_leader = timeline.latest_point()?.leading_explanation.clone()?;
            Some(LiveCausalLeader {
                symbol: timeline.leaf_scope_key.clone(),
                current_leader,
                leader_streak: hk_causal_leader_streak(timeline),
                flips: timeline.flip_events.len(),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.leader_streak.cmp(&a.leader_streak));
    items.truncate(10);
    items
}
