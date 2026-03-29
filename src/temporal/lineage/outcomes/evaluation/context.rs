use rust_decimal::Decimal;
use time::{OffsetDateTime, UtcOffset};

use crate::ontology::{ReasoningScope, Symbol};

pub(in super::super::super) struct SetupOutcomeContext {
    pub(in super::super::super) setup_id: String,
    pub(in super::super::super) workflow_id: Option<String>,
    pub(in super::super::super) symbol: Option<Symbol>,
    pub(in super::super::super) hypothesis_id: String,
    pub(in super::super::super) entry_tick: u64,
    pub(in super::super::super) entry_timestamp: OffsetDateTime,
    pub(in super::super::super) entry_price: Option<Decimal>,
    pub(in super::super::super) entry_composite: Option<Decimal>,
    pub(in super::super::super) direction: i8,
    pub(in super::super::super) estimated_cost: Decimal,
    pub(in super::super::super) convergence_score: Option<Decimal>,
    pub(in super::super::super) external_support_slug: Option<String>,
    pub(in super::super::super) external_support_probability: Option<Decimal>,
    pub(in super::super::super) external_conflict_slug: Option<String>,
    pub(in super::super::super) external_conflict_probability: Option<Decimal>,
    pub(in super::super::super) family: String,
    pub(in super::super::super) session: String,
    pub(in super::super::super) market_regime: String,
    pub(in super::super::super) promoted_by: Vec<String>,
    pub(in super::super::super) blocked_by: Vec<String>,
    pub(in super::super::super) falsified_by: Vec<String>,
}

pub(in super::super::super) fn setup_context(
    record: &crate::temporal::record::TickRecord,
    setup: &crate::ontology::TacticalSetup,
) -> SetupOutcomeContext {
    let symbol = match &setup.scope {
        ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
        _ => None,
    };
    let entry_price = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .and_then(super::outcome::effective_price);
    let entry_composite = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .map(|signal| signal.composite);
    let family = record
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.hypothesis_id == setup.hypothesis_id)
        .map(|hypothesis| hypothesis.family_label.clone())
        .unwrap_or_else(|| "Unknown".into());
    let market_regime = record
        .world_state
        .entities
        .iter()
        .find(|entity| matches!(entity.scope, ReasoningScope::Market(_)))
        .map(|entity| entity.regime.clone())
        .unwrap_or_else(|| "unknown".into());

    SetupOutcomeContext {
        setup_id: setup.setup_id.clone(),
        workflow_id: setup.workflow_id.clone(),
        symbol,
        hypothesis_id: setup.hypothesis_id.clone(),
        entry_tick: record.tick_number,
        entry_timestamp: record.timestamp,
        entry_price,
        entry_composite,
        direction: setup_direction(setup, entry_composite),
        estimated_cost: estimated_execution_cost(setup),
        convergence_score: setup_note_decimal(setup, "convergence_score"),
        external_support_slug: setup_note_value(setup, "external_support_slug"),
        external_support_probability: setup_note_decimal(setup, "external_support_probability"),
        external_conflict_slug: setup_note_value(setup, "external_conflict_slug"),
        external_conflict_probability: setup_note_decimal(setup, "external_conflict_probability"),
        family,
        session: classify_session(record.timestamp),
        market_regime,
        promoted_by: setup.lineage.promoted_by.clone(),
        blocked_by: setup.lineage.blocked_by.clone(),
        falsified_by: setup.lineage.falsified_by.clone(),
    }
}

pub(super) fn estimated_execution_cost(setup: &crate::ontology::TacticalSetup) -> Decimal {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("estimated execution cost="))
        .and_then(|value| value.parse::<Decimal>().ok())
        .unwrap_or(Decimal::ZERO)
}

pub(super) fn setup_note_value(setup: &crate::ontology::TacticalSetup, key: &str) -> Option<String> {
    setup.risk_notes.iter().find_map(|note| {
        note.strip_prefix(&format!("{}=", key))
            .filter(|value| !value.is_empty())
            .map(std::borrow::ToOwned::to_owned)
    })
}

pub(super) fn setup_note_decimal(setup: &crate::ontology::TacticalSetup, key: &str) -> Option<Decimal> {
    setup_note_value(setup, key).and_then(|value| value.parse::<Decimal>().ok())
}

pub(in super::super::super) fn setup_direction(
    setup: &crate::ontology::TacticalSetup,
    entry_composite: Option<Decimal>,
) -> i8 {
    if let Some(workflow_id) = setup.workflow_id.as_deref() {
        if workflow_id.ends_with(":sell") {
            return -1;
        }
        if workflow_id.ends_with(":buy") {
            return 1;
        }
    }

    if setup.title.starts_with("Short ") {
        -1
    } else if setup.title.starts_with("Long ") {
        1
    } else if let Some(composite) = entry_composite {
        if composite < Decimal::ZERO {
            -1
        } else {
            1
        }
    } else {
        1
    }
}

fn classify_session(timestamp: time::OffsetDateTime) -> String {
    let hk = timestamp.to_offset(UtcOffset::from_hms(8, 0, 0).expect("valid hk offset"));
    let minutes = u16::from(hk.hour()) * 60 + u16::from(hk.minute());
    match minutes as u16 {
        570..=630 => "opening".into(),
        631..=870 => "midday".into(),
        871..=970 => "closing".into(),
        _ => "offhours".into(),
    }
}
