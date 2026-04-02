use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::temporal::buffer::TickHistory;

use super::{compute_lineage_stats, CaseRealizedOutcome, FamilyContextLineageOutcome};

#[path = "outcomes/aggregation.rs"]
mod aggregation;
#[path = "outcomes/evaluation.rs"]
mod evaluation;
#[path = "outcomes/filters.rs"]
mod filters;

pub(super) use aggregation::{
    top_context_outcomes, top_counts, top_family_context_outcomes, top_outcomes,
    update_context_outcome, update_family_context_outcome, update_outcome,
    ContextualOutcomeAccumulator, FamilyContextAccumulator, OutcomeAccumulator,
};
pub(super) use evaluation::{
    evaluate_setup_outcome, setup_context, EvaluatedOutcome, SetupOutcomeContext,
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
            evaluate_at_horizon(
                &context,
                resolution_lag,
                current_tick,
                &window,
                &window_by_tick,
            )
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

            let symbol = context.symbol.as_ref()?;
            let entry_price = context.entry_price?;
            if entry_price <= Decimal::ZERO {
                return None;
            }
            let resolved_tick = adaptive_peak_tick_from_prices(
                context.entry_tick,
                entry_price,
                context.direction,
                min_lag,
                future_records.iter().filter_map(|record| {
                    let price = record
                        .signals
                        .get(symbol)
                        .and_then(|signal| signal.mark_price)
                        .filter(|price| *price > Decimal::ZERO)?;
                    Some((record.tick_number, price))
                }),
            )?;
            let bounded_records = future_records
                .iter()
                .copied()
                .filter(|record| record.tick_number <= resolved_tick)
                .collect::<Vec<_>>();
            let outcome = evaluate_setup_outcome(&context, &bounded_records, &window_by_tick)?;
            let resolved_at = window_by_tick
                .get(&resolved_tick)
                .copied()
                .map(|record| record.timestamp)
                .or_else(|| future_records.last().map(|record| record.timestamp))?;
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

fn adaptive_peak_tick_from_prices<I>(
    entry_tick: u64,
    entry_price: Decimal,
    direction: i8,
    min_lag: u64,
    future_prices: I,
) -> Option<u64>
where
    I: IntoIterator<Item = (u64, Decimal)>,
{
    let mut best: Option<(u64, Decimal)> = None;

    for (tick_number, price) in future_prices {
        if tick_number < entry_tick + min_lag || price <= Decimal::ZERO {
            continue;
        }

        let ret = if direction >= 0 {
            (price - entry_price) / entry_price
        } else {
            (entry_price - price) / entry_price
        };

        match best {
            Some((_, best_ret)) if ret <= best_ret => {}
            _ => best = Some((tick_number, ret)),
        }
    }

    best.map(|(tick_number, _)| tick_number)
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

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::adaptive_peak_tick_from_prices;

    #[test]
    fn adaptive_peak_tick_prefers_best_long_return() {
        let resolved = adaptive_peak_tick_from_prices(
            10,
            dec!(100),
            1,
            2,
            vec![
                (11, dec!(98)),
                (12, dec!(103)),
                (13, dec!(101)),
                (14, dec!(105)),
            ],
        );

        assert_eq!(resolved, Some(14));
    }

    #[test]
    fn adaptive_peak_tick_prefers_least_negative_return_when_long_never_recovers() {
        let resolved = adaptive_peak_tick_from_prices(
            10,
            dec!(100),
            1,
            2,
            vec![(12, dec!(97)), (13, dec!(96)), (14, dec!(98))],
        );

        assert_eq!(resolved, Some(14));
    }

    #[test]
    fn adaptive_peak_tick_respects_short_direction() {
        let resolved = adaptive_peak_tick_from_prices(
            10,
            dec!(100),
            -1,
            2,
            vec![(12, dec!(97)), (13, dec!(94)), (14, dec!(96))],
        );

        assert_eq!(resolved, Some(13));
    }
}

pub fn compute_family_context_outcomes(
    history: &TickHistory,
    limit: usize,
) -> Vec<FamilyContextLineageOutcome> {
    compute_lineage_stats(history, limit).family_contexts
}
