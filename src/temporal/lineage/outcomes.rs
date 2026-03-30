use std::collections::HashMap;

use rust_decimal::Decimal;

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

/// Evaluate outcomes at a single resolution lag.
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
    let seen_setups = collect_setup_contexts(&window);

    let mut outcomes = seen_setups
        .into_values()
        .filter_map(|context| {
            evaluate_at_horizon(&context, resolution_lag, current_tick, &window, &window_by_tick)
        })
        .collect::<Vec<_>>();

    outcomes.sort_by(|left, right| right.resolved_tick.cmp(&left.resolved_tick));
    outcomes
}

/// Continuous horizon evaluation: scan every available future tick for each setup,
/// find the peak (best net_return), and resolve at that natural horizon.
/// No hardcoded horizons. The system finds its own optimal evaluation point.
///
/// Minimum lag: 10 ticks (avoid resolving on noise).
/// The "resolved_tick" in the output reflects where the peak actually occurred.
pub fn compute_case_realized_outcomes_adaptive(
    history: &TickHistory,
    limit: usize,
) -> Vec<CaseRealizedOutcome> {
    let window = history.latest_n(limit);
    if window.is_empty() {
        return Vec::new();
    }

    let current_tick = window
        .last()
        .map(|record| record.tick_number)
        .unwrap_or_default();
    let window_by_tick: HashMap<u64, &crate::temporal::record::TickRecord> = window
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect();
    let seen_setups = collect_setup_contexts(&window);
    let min_lag: u64 = 10;

    let mut outcomes = seen_setups
        .into_values()
        .filter_map(|context| {
            // Need at least min_lag ticks of future data
            if current_tick < context.entry_tick + min_lag {
                return None;
            }

            // Collect all future records after entry
            let future_records: Vec<_> = window
                .iter()
                .copied()
                .filter(|record| record.tick_number > context.entry_tick)
                .collect();

            if future_records.len() < min_lag as usize {
                return None;
            }

            // Evaluate the full path
            let outcome = evaluate_setup_outcome(&context, &future_records, &window_by_tick)?;

            // Now find the peak horizon by scanning tick by tick
            let symbol = context.symbol.as_ref()?;
            let entry_price = context.entry_price?;
            if entry_price <= Decimal::ZERO {
                return None;
            }

            let mut peak_return = Decimal::ZERO;
            let mut peak_tick = context.entry_tick + min_lag;
            let mut ticks_since_entry: u64 = 0;

            for record in &future_records {
                ticks_since_entry += 1;
                if ticks_since_entry < min_lag {
                    continue;
                }
                if let Some(signal) = record.signals.get(symbol) {
                    if let Some(mark_price) = signal.mark_price {
                        if mark_price > Decimal::ZERO {
                            let ret = if context.direction >= 0 {
                                (mark_price - entry_price) / entry_price
                            } else {
                                (entry_price - mark_price) / entry_price
                            };
                            if ret > peak_return {
                                peak_return = ret;
                                peak_tick = record.tick_number;
                            }
                        }
                    }
                }
            }

            let resolved_tick = peak_tick;
            let resolved_at = window_by_tick
                .get(&resolved_tick)
                .copied()
                .map(|record| record.timestamp)
                .or_else(|| {
                    future_records.last().map(|record| record.timestamp)
                })?;
            Some(CaseRealizedOutcome {
                setup_id: context.setup_id.clone(),
                workflow_id: context.workflow_id.clone(),
                symbol: context.symbol.as_ref().map(|s| s.0.clone()),
                entry_tick: context.entry_tick,
                entry_timestamp: context.entry_timestamp,
                resolved_tick,
                resolved_at,
                family: context.family.clone(),
                session: context.session.clone(),
                market_regime: context.market_regime.clone(),
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

fn collect_setup_contexts(
    window: &[&crate::temporal::record::TickRecord],
) -> HashMap<String, SetupOutcomeContext> {
    let mut seen = HashMap::new();
    for record in window {
        for setup in &record.tactical_setups {
            seen.entry(setup.setup_id.clone())
                .or_insert_with(|| setup_context(record, setup));
        }
    }
    seen
}

fn evaluate_at_horizon(
    context: &SetupOutcomeContext,
    resolution_lag: u64,
    current_tick: u64,
    window: &[&crate::temporal::record::TickRecord],
    window_by_tick: &HashMap<u64, &crate::temporal::record::TickRecord>,
) -> Option<CaseRealizedOutcome> {
    let resolved_tick = context.entry_tick + resolution_lag;
    if current_tick < resolved_tick {
        return None;
    }

    let future_records: Vec<_> = window
        .iter()
        .copied()
        .filter(|record| {
            record.tick_number > context.entry_tick && record.tick_number <= resolved_tick
        })
        .collect();
    let outcome = evaluate_setup_outcome(context, &future_records, window_by_tick)?;
    let resolved_at = window_by_tick
        .get(&resolved_tick)
        .copied()
        .map(|record| record.timestamp)?;

    Some(CaseRealizedOutcome {
        setup_id: context.setup_id.clone(),
        workflow_id: context.workflow_id.clone(),
        symbol: context.symbol.as_ref().map(|symbol| symbol.0.clone()),
        entry_tick: context.entry_tick,
        entry_timestamp: context.entry_timestamp,
        resolved_tick,
        resolved_at,
        family: context.family.clone(),
        session: context.session.clone(),
        market_regime: context.market_regime.clone(),
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
}

pub fn compute_family_context_outcomes(
    history: &TickHistory,
    limit: usize,
) -> Vec<FamilyContextLineageOutcome> {
    compute_lineage_stats(history, limit).family_contexts
}
