use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use time::{OffsetDateTime, UtcOffset};

use crate::ontology::world::WorldStateSnapshot;
use crate::ontology::{ReasoningScope, Symbol, TacticalSetup};
use crate::temporal::record::{SymbolSignals, TickRecord};

#[derive(Debug, Clone)]
pub(crate) struct SetupOutcomeContext {
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub symbol: Option<Symbol>,
    pub entry_tick: u64,
    pub entry_timestamp: OffsetDateTime,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub direction: i8,
    pub entry_price: Option<Decimal>,
    pub convergence_score: Decimal,
    pub promoted_by: Vec<String>,
    pub blocked_by: Vec<String>,
    pub falsified_by: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct EvaluatedOutcome {
    pub return_pct: Decimal,
    pub net_return: Decimal,
    pub max_favorable_excursion: Decimal,
    pub max_adverse_excursion: Decimal,
    pub followed_through: bool,
    pub invalidated: bool,
    pub structure_retained: bool,
    pub convergence_score: Decimal,
    pub external_delta: Decimal,
    pub external_follow_through: bool,
    pub follow_expectancy: Decimal,
    pub fade_expectancy: Decimal,
    pub wait_expectancy: Decimal,
}

pub(crate) fn setup_context(record: &TickRecord, setup: &TacticalSetup) -> SetupOutcomeContext {
    let symbol = match &setup.scope {
        ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
        _ => None,
    };
    let hypothesis = record
        .hypotheses
        .iter()
        .find(|item| item.hypothesis_id == setup.hypothesis_id);
    let entry_price = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .and_then(effective_price);
    let entry_composite = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .map(|signal| signal.composite);

    SetupOutcomeContext {
        setup_id: setup.setup_id.clone(),
        workflow_id: setup.workflow_id.clone(),
        symbol,
        entry_tick: record.tick_number,
        entry_timestamp: record.timestamp,
        family: hypothesis
            .map(|item| item.family_label.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Unknown".into()),
        session: hk_session_label(record.timestamp).into(),
        market_regime: current_market_regime(&record.world_state).into(),
        direction: setup_direction(setup, entry_composite),
        entry_price,
        convergence_score: setup.convergence_score.unwrap_or(Decimal::ZERO),
        promoted_by: setup.lineage.promoted_by.clone(),
        blocked_by: setup.lineage.blocked_by.clone(),
        falsified_by: setup.lineage.falsified_by.clone(),
    }
}

pub(crate) fn evaluate_setup_outcome(
    context: &SetupOutcomeContext,
    future_records: &[&TickRecord],
    window_by_tick: &HashMap<u64, &TickRecord>,
) -> Option<EvaluatedOutcome> {
    let symbol = context.symbol.as_ref()?;
    let entry_price = context.entry_price?;
    if entry_price <= Decimal::ZERO {
        return None;
    }

    let path_returns = future_records
        .iter()
        .filter_map(|record| {
            let exit_price = record.signals.get(symbol).and_then(effective_price)?;
            Some(oriented_return(entry_price, exit_price, context.direction))
        })
        .collect::<Vec<_>>();

    if path_returns.is_empty() {
        return None;
    }

    let resolved_record = future_records.last().copied().or_else(|| window_by_tick.get(&context.entry_tick).copied())?;
    let resolved_price = resolved_record.signals.get(symbol).and_then(effective_price)?;
    let return_pct = oriented_return(entry_price, resolved_price, context.direction);
    let execution_cost = estimated_execution_cost(symbol, context, window_by_tick);
    let net_return = return_pct - execution_cost;
    let max_favorable_excursion = path_returns
        .iter()
        .copied()
        .max()
        .unwrap_or(Decimal::ZERO);
    let max_adverse_excursion = path_returns
        .iter()
        .copied()
        .min()
        .unwrap_or(Decimal::ZERO);
    let followed_through = max_favorable_excursion > dec!(0.003);
    let invalidated = max_adverse_excursion < dec!(-0.003);
    let structure_retained = followed_through && !invalidated;
    let fade_expectancy = fade_return(
        return_pct,
        max_adverse_excursion,
        execution_cost,
        dec!(0.003),
        !structure_retained,
        invalidated,
        followed_through,
    );
    let wait_expectancy = if followed_through {
        max_favorable_excursion.max(Decimal::ZERO)
    } else {
        Decimal::ZERO
    };

    Some(EvaluatedOutcome {
        return_pct,
        net_return,
        max_favorable_excursion,
        max_adverse_excursion,
        followed_through,
        invalidated,
        structure_retained,
        convergence_score: context.convergence_score,
        external_delta: Decimal::ZERO,
        external_follow_through: false,
        follow_expectancy: net_return,
        fade_expectancy,
        wait_expectancy,
    })
}

pub(crate) fn setup_direction(setup: &TacticalSetup, entry_composite: Option<Decimal>) -> i8 {
    let title = setup.title.trim().to_ascii_lowercase();
    if title.starts_with("short ") {
        return -1;
    }
    if title.starts_with("long ") {
        return 1;
    }

    match entry_composite {
        Some(value) if value < Decimal::ZERO => -1,
        _ => 1,
    }
}

pub(crate) fn fade_return(
    realized_return: Decimal,
    max_adverse_excursion: Decimal,
    estimated_execution_cost: Decimal,
    material_move: Decimal,
    structure_failed: bool,
    invalidated: bool,
    followed_through: bool,
) -> Decimal {
    if structure_failed || invalidated || !followed_through {
        let reversal_capture = (-max_adverse_excursion - estimated_execution_cost).max(Decimal::ZERO);
        if reversal_capture > material_move {
            reversal_capture
        } else {
            -realized_return - estimated_execution_cost
        }
    } else {
        -realized_return - estimated_execution_cost
    }
}

fn oriented_return(entry_price: Decimal, exit_price: Decimal, direction: i8) -> Decimal {
    if entry_price <= Decimal::ZERO {
        return Decimal::ZERO;
    }

    let raw = (exit_price - entry_price) / entry_price;
    if direction < 0 { -raw } else { raw }
}

fn effective_price(signal: &SymbolSignals) -> Option<Decimal> {
    signal
        .mark_price
        .filter(|price| *price > Decimal::ZERO)
        .or(signal.vwap.filter(|price| *price > Decimal::ZERO))
}

fn current_market_regime(world_state: &WorldStateSnapshot) -> &str {
    world_state
        .entities
        .iter()
        .find(|entity| matches!(entity.scope, ReasoningScope::Market(_)))
        .map(|entity| entity.regime.as_str())
        .unwrap_or("unknown")
}

fn hk_session_label(timestamp: OffsetDateTime) -> &'static str {
    let hk = timestamp.to_offset(UtcOffset::from_hms(8, 0, 0).expect("valid hk offset"));
    let minutes = u16::from(hk.hour()) * 60 + u16::from(hk.minute());
    match minutes {
        570..=630 => "opening",
        631..=870 => "midday",
        871..=970 => "closing",
        _ => "offhours",
    }
}

fn estimated_execution_cost(
    symbol: &Symbol,
    context: &SetupOutcomeContext,
    window_by_tick: &HashMap<u64, &TickRecord>,
) -> Decimal {
    window_by_tick
        .get(&context.entry_tick)
        .and_then(|record| {
            record
                .tactical_setups
                .iter()
                .find(|setup| setup.setup_id == context.setup_id)
        })
        .and_then(|setup| {
            setup.risk_notes.iter().find_map(|note| {
                note.strip_prefix("estimated execution cost=")
                    .and_then(|value| value.parse::<Decimal>().ok())
            })
        })
        .or_else(|| {
            window_by_tick
                .get(&context.entry_tick)
                .and_then(|record| record.signals.get(symbol))
                .and_then(|signal| signal.spread)
                .map(|spread| spread / dec!(2))
        })
        .unwrap_or(dec!(0.002))
}
