use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::ReasoningScope;
use crate::temporal::record::SymbolSignals;

use super::context::SetupOutcomeContext;

#[derive(Clone, Copy)]
pub(in super::super::super) struct EvaluatedOutcome {
    pub(in super::super::super) return_pct: Decimal,
    pub(in super::super::super) net_return: Decimal,
    pub(in super::super::super) fade_return: Decimal,
    pub(in super::super::super) max_favorable_excursion: Decimal,
    pub(in super::super::super) max_adverse_excursion: Decimal,
    pub(in super::super::super) followed_through: bool,
    pub(in super::super::super) invalidated: bool,
    pub(in super::super::super) structure_retained: bool,
    pub(in super::super::super) convergence_score: Decimal,
    pub(in super::super::super) external_delta: Decimal,
    pub(in super::super::super) external_follow_through: bool,
}

pub(in super::super::super) fn evaluate_setup_outcome(
    context: &SetupOutcomeContext,
    future_records: &[&crate::temporal::record::TickRecord],
    records_by_tick: &HashMap<u64, &crate::temporal::record::TickRecord>,
) -> Option<EvaluatedOutcome> {
    let symbol = context.symbol.as_ref()?;
    let entry_price = context.entry_price?;
    if entry_price <= Decimal::ZERO {
        return None;
    }

    let mut path_returns = Vec::new();
    let mut latest_signal: Option<&SymbolSignals> = None;
    let mut invalidated = false;

    for record in future_records {
        if let Some(signal) = record.signals.get(symbol) {
            latest_signal = Some(signal);
            if let Some(mark_price) = effective_price(signal) {
                path_returns.push(signed_return(entry_price, mark_price, context.direction));
            }
        }
        if matching_track_invalidated(record, context) {
            invalidated = true;
        }
    }

    let latest_return = path_returns.last().copied()?;
    let max_favorable_excursion = path_returns.iter().copied().max().unwrap_or(Decimal::ZERO);
    let max_adverse_excursion = path_returns.iter().copied().min().unwrap_or(Decimal::ZERO);
    let round_trip_cost = context.estimated_cost * Decimal::TWO;
    let material_move = round_trip_cost.max(Decimal::new(3, 3));
    let entry_record = records_by_tick.get(&context.entry_tick).copied();
    let external_delta = external_alignment_delta(context, future_records);
    let followed_through = max_favorable_excursion > material_move;
    let structure_retained = latest_signal
        .map(|signal| structure_retained(entry_record, context, signal))
        .unwrap_or(false);

    Some(EvaluatedOutcome {
        return_pct: latest_return,
        net_return: latest_return - round_trip_cost,
        fade_return: fade_return(
            latest_return,
            max_adverse_excursion,
            round_trip_cost,
            material_move,
            invalidated,
            followed_through,
            structure_retained,
        ),
        max_favorable_excursion,
        max_adverse_excursion,
        followed_through,
        invalidated,
        structure_retained,
        convergence_score: context.convergence_score.unwrap_or(Decimal::ZERO),
        external_delta,
        external_follow_through: external_delta > Decimal::ZERO,
    })
}

pub(super) fn structure_retained(
    entry_record: Option<&crate::temporal::record::TickRecord>,
    context: &SetupOutcomeContext,
    latest_signal: &SymbolSignals,
) -> bool {
    let Some(entry_composite) = context.entry_composite else {
        return false;
    };
    let entry_mark = entry_record
        .and_then(|record| {
            context
                .symbol
                .as_ref()
                .and_then(|symbol| record.signals.get(symbol))
        })
        .and_then(effective_price);
    let latest_mark = effective_price(latest_signal);
    let price_not_broken = match (entry_mark, latest_mark) {
        (Some(entry_mark), Some(latest_mark)) => {
            signed_return(entry_mark, latest_mark, context.direction) > Decimal::new(-8, 3)
        }
        _ => true,
    };

    have_same_sign(entry_composite, latest_signal.composite)
        && latest_signal.composite.abs() >= entry_composite.abs() / Decimal::TWO
        && latest_signal.composite_degradation.unwrap_or(Decimal::ZERO) < Decimal::new(45, 2)
        && price_not_broken
}

pub(super) fn matching_track_invalidated(
    record: &crate::temporal::record::TickRecord,
    context: &SetupOutcomeContext,
) -> bool {
    let Some(symbol) = &context.symbol else {
        return false;
    };
    record.hypothesis_tracks.iter().any(|track| {
        matches!(
            &track.scope,
            ReasoningScope::Symbol(track_symbol) if track_symbol == symbol
        ) && track.hypothesis_id == context.hypothesis_id
            && (track.invalidated_at.is_some() || track.status.as_str() == "invalidated")
    })
}

pub(super) fn effective_price(signal: &SymbolSignals) -> Option<Decimal> {
    signal
        .mark_price
        .filter(|price| *price > Decimal::ZERO)
        .or_else(|| signal.vwap.filter(|price| *price > Decimal::ZERO))
}

pub(super) fn external_alignment_delta(
    context: &SetupOutcomeContext,
    future_records: &[&crate::temporal::record::TickRecord],
) -> Decimal {
    let support_delta = context
        .external_support_slug
        .as_ref()
        .and_then(|slug| {
            let latest = latest_polymarket_probability(future_records, slug)?;
            let entry = context.external_support_probability?;
            Some(latest - entry)
        })
        .unwrap_or(Decimal::ZERO);
    let conflict_delta = context
        .external_conflict_slug
        .as_ref()
        .and_then(|slug| {
            let latest = latest_polymarket_probability(future_records, slug)?;
            let entry = context.external_conflict_probability?;
            Some(latest - entry)
        })
        .unwrap_or(Decimal::ZERO);

    support_delta - conflict_delta
}

pub(super) fn latest_polymarket_probability(
    future_records: &[&crate::temporal::record::TickRecord],
    slug: &str,
) -> Option<Decimal> {
    future_records.iter().rev().find_map(|record| {
        record
            .polymarket_priors
            .iter()
            .find(|prior| prior.slug == slug)
            .map(|prior| prior.probability)
    })
}

pub(in super::super::super) fn fade_return(
    latest_return: Decimal,
    max_adverse_excursion: Decimal,
    round_trip_cost: Decimal,
    material_move: Decimal,
    invalidated: bool,
    followed_through: bool,
    structure_retained: bool,
) -> Decimal {
    let reversal_capture = (-max_adverse_excursion).max(-latest_return);
    let reversal_is_material = reversal_capture > material_move;
    let realized_fade =
        if (invalidated || !followed_through || !structure_retained) && reversal_is_material {
            reversal_capture
        } else {
            -latest_return
        };
    realized_fade - round_trip_cost
}

fn signed_return(entry_price: Decimal, exit_price: Decimal, direction: i8) -> Decimal {
    let raw_return = (exit_price - entry_price) / entry_price;
    if direction >= 0 {
        raw_return
    } else {
        -raw_return
    }
}

fn have_same_sign(left: Decimal, right: Decimal) -> bool {
    (left > Decimal::ZERO && right > Decimal::ZERO)
        || (left < Decimal::ZERO && right < Decimal::ZERO)
}
