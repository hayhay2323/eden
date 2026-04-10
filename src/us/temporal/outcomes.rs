//! US case realized outcome evaluation — parallel to `temporal::lineage::outcomes`.
//!
//! Operates on `UsTickHistory`/`UsTickRecord` instead of HK's `TickHistory`/`TickRecord`.
//! Core evaluation logic (oriented return, MFE/MAE, follow-through) is identical.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{ReasoningScope, TacticalSetup};
use crate::temporal::lineage::CaseRealizedOutcome;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};

// ── Outcome context ──

struct UsSetupContext {
    setup_id: String,
    workflow_id: Option<String>,
    symbol: Option<Symbol>,
    entry_tick: u64,
    entry_timestamp: OffsetDateTime,
    family: String,
    session: String,
    market_regime: String,
    direction: i8,
    entry_price: Option<Decimal>,
    convergence_score: Decimal,
}

fn us_setup_context(record: &UsTickRecord, setup: &TacticalSetup) -> UsSetupContext {
    let symbol = match &setup.scope {
        ReasoningScope::Symbol(s) => Some(s.clone()),
        _ => None,
    };
    let hypothesis = record
        .hypotheses
        .iter()
        .find(|h| h.hypothesis_id == setup.hypothesis_id);
    let entry_price = symbol
        .as_ref()
        .and_then(|s| record.signals.get(s))
        .and_then(|sig| sig.mark_price)
        .filter(|p| *p > Decimal::ZERO);
    let entry_composite = symbol
        .as_ref()
        .and_then(|s| record.signals.get(s))
        .map(|sig| sig.composite);

    UsSetupContext {
        setup_id: setup.setup_id.clone(),
        workflow_id: setup.workflow_id.clone(),
        symbol,
        entry_tick: record.tick_number,
        entry_timestamp: record.timestamp,
        family: hypothesis
            .map(|h| h.family_label.clone())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "Unknown".into()),
        session: us_session_label(record.timestamp).into(),
        market_regime: format!("{:?}", record.market_regime).to_ascii_lowercase(),
        direction: setup_direction(setup, entry_composite),
        entry_price,
        convergence_score: setup.convergence_score.unwrap_or(Decimal::ZERO),
    }
}

fn setup_direction(setup: &TacticalSetup, entry_composite: Option<Decimal>) -> i8 {
    let title = setup.title.trim().to_ascii_lowercase();
    if title.starts_with("short ") {
        return -1;
    }
    if title.starts_with("long ") {
        return 1;
    }
    match entry_composite {
        Some(v) if v < Decimal::ZERO => -1,
        _ => 1,
    }
}

fn us_session_label(timestamp: OffsetDateTime) -> &'static str {
    // US market: 09:30-16:00 ET → 13:30-20:00 UTC (EST) or 13:30-20:00 UTC (EDT)
    let hour = timestamp.hour();
    match hour {
        0..=14 => "pre_market",
        15..=16 => "morning",
        17..=18 => "midday",
        19..=20 => "afternoon",
        _ => "after_hours",
    }
}

// ── Core evaluation ──

fn oriented_return(entry: Decimal, exit: Decimal, direction: i8) -> Decimal {
    if entry <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    if direction >= 0 {
        (exit - entry) / entry
    } else {
        (entry - exit) / entry
    }
}

fn us_effective_price(signals: &UsSymbolSignals) -> Option<Decimal> {
    signals.mark_price.filter(|p| *p > Decimal::ZERO)
}

fn evaluate_us_setup_outcome(
    context: &UsSetupContext,
    future_records: &[&UsTickRecord],
) -> Option<CaseRealizedOutcome> {
    let symbol = context.symbol.as_ref()?;
    let entry_price = context.entry_price?;
    if entry_price <= Decimal::ZERO {
        return None;
    }

    let path_returns: Vec<Decimal> = future_records
        .iter()
        .filter_map(|record| {
            let exit_price = record
                .signals
                .get(symbol)
                .and_then(us_effective_price)?;
            Some(oriented_return(entry_price, exit_price, context.direction))
        })
        .collect();

    if path_returns.is_empty() {
        return None;
    }

    let resolved_record = future_records.last()?;
    let resolved_price = resolved_record
        .signals
        .get(symbol)
        .and_then(us_effective_price)?;
    let return_pct = oriented_return(entry_price, resolved_price, context.direction);
    let net_return = return_pct - dec!(0.001); // estimated execution cost
    let max_favorable_excursion = path_returns.iter().copied().max().unwrap_or(Decimal::ZERO);
    let max_adverse_excursion = path_returns.iter().copied().min().unwrap_or(Decimal::ZERO);
    let followed_through = max_favorable_excursion > dec!(0.003);
    let invalidated = max_adverse_excursion < dec!(-0.003);
    let structure_retained = followed_through && !invalidated;

    Some(CaseRealizedOutcome {
        setup_id: context.setup_id.clone(),
        workflow_id: context.workflow_id.clone(),
        symbol: context.symbol.as_ref().map(|s| s.0.clone()),
        entry_tick: context.entry_tick,
        entry_timestamp: context.entry_timestamp,
        resolved_tick: resolved_record.tick_number,
        resolved_at: resolved_record.timestamp,
        family: context.family.clone(),
        session: context.session.clone(),
        market_regime: context.market_regime.clone(),
        direction: context.direction,
        return_pct,
        net_return,
        max_favorable_excursion,
        max_adverse_excursion,
        followed_through,
        invalidated,
        structure_retained,
        convergence_score: context.convergence_score,
    })
}

// ── Public API ──

/// Adaptive peak evaluation for US setups — mirrors HK's
/// `compute_case_realized_outcomes_adaptive`.
pub fn compute_us_case_realized_outcomes_adaptive(
    history: &UsTickHistory,
    limit: usize,
) -> Vec<CaseRealizedOutcome> {
    let window = history.latest_n(limit);
    if window.is_empty() {
        return Vec::new();
    }

    let current_tick = window
        .last()
        .map(|r| r.tick_number)
        .unwrap_or_default();

    let mut seen: HashMap<String, UsSetupContext> = HashMap::new();
    for record in &window {
        for setup in &record.tactical_setups {
            seen.entry(setup.setup_id.clone())
                .or_insert_with(|| us_setup_context(record, setup));
        }
    }

    let min_lag: u64 = 10;

    let mut outcomes: Vec<CaseRealizedOutcome> = seen
        .into_values()
        .filter_map(|context| {
            if current_tick < context.entry_tick + min_lag {
                return None;
            }

            let future_records: Vec<&UsTickRecord> = window
                .iter()
                .filter(|r| r.tick_number > context.entry_tick)
                .copied()
                .collect();

            if future_records.len() < min_lag as usize {
                return None;
            }

            let symbol = context.symbol.as_ref()?;
            let entry_price = context.entry_price?;
            if entry_price <= Decimal::ZERO {
                return None;
            }

            // Find adaptive peak tick
            let resolved_tick = adaptive_peak_tick(
                context.entry_tick,
                entry_price,
                context.direction,
                min_lag,
                future_records.iter().filter_map(|r| {
                    let price = r
                        .signals
                        .get(symbol)
                        .and_then(us_effective_price)?;
                    Some((r.tick_number, price))
                }),
            )?;

            let bounded: Vec<&UsTickRecord> = future_records
                .iter()
                .copied()
                .filter(|r| r.tick_number <= resolved_tick)
                .collect();

            evaluate_us_setup_outcome(&context, &bounded)
        })
        .collect();

    outcomes.sort_by(|a, b| b.resolved_tick.cmp(&a.resolved_tick));
    outcomes
}

fn adaptive_peak_tick<I>(
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
    for (tick, price) in future_prices {
        if tick < entry_tick + min_lag || price <= Decimal::ZERO {
            continue;
        }
        let ret = oriented_return(entry_price, price, direction);
        match best {
            Some((_, best_ret)) if ret <= best_ret => {}
            _ => best = Some((tick, ret)),
        }
    }
    best.map(|(tick, _)| tick)
}

