use std::collections::HashMap;

use crate::temporal::buffer::TickHistory;

use super::{CaseRealizedOutcome, FamilyContextLineageOutcome, compute_lineage_stats};

#[path = "outcomes/aggregation.rs"]
mod aggregation;
#[path = "outcomes/evaluation.rs"]
mod evaluation;
#[path = "outcomes/filters.rs"]
mod filters;

pub(super) use aggregation::{
    ContextualOutcomeAccumulator, FamilyContextAccumulator, OutcomeAccumulator,
    top_context_outcomes, top_counts, top_family_context_outcomes, top_outcomes,
    update_context_outcome, update_family_context_outcome, update_outcome,
};
pub(super) use evaluation::{
    EvaluatedOutcome, SetupOutcomeContext, evaluate_setup_outcome, setup_context,
};
#[cfg(test)]
pub(super) use evaluation::{fade_return, setup_direction};
pub(super) use filters::{
    filter_context_outcomes, filter_contexts_by_alignment, filter_count_list,
    filter_family_context_outcomes, filter_family_contexts_by_alignment, filter_outcomes,
    filter_outcomes_by_alignment,
};

pub fn compute_case_realized_outcomes(
    history: &TickHistory,
    limit: usize,
    resolution_lag: u64,
) -> Vec<CaseRealizedOutcome> {
    let window = history.latest_n(limit);
    if window.is_empty() {
        return Vec::new();
    }

    let current_tick = window
        .last()
        .map(|record| record.tick_number)
        .unwrap_or_default();
    let window_by_tick = window
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect::<HashMap<_, _>>();
    let mut seen_setups = HashMap::<String, SetupOutcomeContext>::new();

    for record in &window {
        for setup in &record.tactical_setups {
            seen_setups
                .entry(setup.setup_id.clone())
                .or_insert_with(|| setup_context(record, setup));
        }
    }

    let mut outcomes = seen_setups
        .into_values()
        .filter_map(|context| {
            let resolved_tick = context.entry_tick + resolution_lag;
            if current_tick < resolved_tick {
                return None;
            }

            let future_records = window
                .iter()
                .copied()
                .filter(|record| {
                    record.tick_number > context.entry_tick && record.tick_number <= resolved_tick
                })
                .collect::<Vec<_>>();
            let outcome = evaluate_setup_outcome(&context, &future_records, &window_by_tick)?;
            let resolved_at = window_by_tick
                .get(&resolved_tick)
                .copied()
                .map(|record| record.timestamp)?;

            Some(CaseRealizedOutcome {
                setup_id: context.setup_id,
                workflow_id: context.workflow_id,
                symbol: context.symbol.map(|symbol| symbol.0),
                entry_tick: context.entry_tick,
                entry_timestamp: context.entry_timestamp,
                resolved_tick,
                resolved_at,
                family: context.family,
                session: context.session,
                market_regime: context.market_regime,
                direction: context.direction,
                return_pct: outcome.return_pct,
                net_return: outcome.net_return,
                max_favorable_excursion: outcome.max_favorable_excursion,
                max_adverse_excursion: outcome.max_adverse_excursion,
                followed_through: outcome.followed_through,
                invalidated: outcome.invalidated,
                structure_retained: outcome.structure_retained,
                convergence_score: outcome.convergence_score,
            })
        })
        .collect::<Vec<_>>();

    outcomes.sort_by(|left, right| right.resolved_tick.cmp(&left.resolved_tick));
    outcomes
}

pub fn compute_family_context_outcomes(
    history: &TickHistory,
    limit: usize,
) -> Vec<FamilyContextLineageOutcome> {
    compute_lineage_stats(history, limit).family_contexts
}
