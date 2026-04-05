use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalScope {
    Market,
    Symbol(Symbol),
    Institution(InstitutionId),
    Sector(SectorId),
    Theme(ThemeId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObservationRecord {
    Quote {
        symbol: Symbol,
        last_done: Decimal,
        turnover: Decimal,
        market_status: String,
        pre_market_last: Option<Decimal>,
        post_market_last: Option<Decimal>,
    },
    OrderBook {
        symbol: Symbol,
        total_bid_volume: i64,
        total_ask_volume: i64,
        spread: Option<Decimal>,
    },
    CapitalFlow {
        symbol: Symbol,
        net_inflow: Decimal,
    },
    CapitalFlowSeries {
        symbol: Symbol,
        point_count: usize,
        latest_inflow: Decimal,
        velocity: Decimal,
    },
    CapitalBreakdown {
        symbol: Symbol,
        large_net: Decimal,
        medium_net: Decimal,
        small_net: Decimal,
    },
    CalcIndex {
        symbol: Symbol,
        turnover_rate: Option<Decimal>,
        volume_ratio: Option<Decimal>,
        pe_ttm_ratio: Option<Decimal>,
        pb_ratio: Option<Decimal>,
        dividend_ratio_ttm: Option<Decimal>,
        amplitude: Option<Decimal>,
        five_minutes_change_rate: Option<Decimal>,
        ytd_change_rate: Option<Decimal>,
        five_day_change_rate: Option<Decimal>,
        ten_day_change_rate: Option<Decimal>,
        half_year_change_rate: Option<Decimal>,
        total_market_value: Option<Decimal>,
        change_rate: Option<Decimal>,
    },
    Candlestick {
        symbol: Symbol,
        candle_count: usize,
        window_return: Decimal,
        body_bias: Decimal,
        volume_ratio: Decimal,
        range_ratio: Decimal,
    },
    InstitutionActivity {
        symbol: Symbol,
        institution_id: String,
        seat_count: usize,
    },
    TradeActivity {
        symbol: Symbol,
        trade_count: usize,
        total_volume: i64,
        buy_volume: i64,
        sell_volume: i64,
        vwap: Decimal,
        pre_market_volume: i64,
        post_market_volume: i64,
    },
    MarketTemperature {
        temperature: Decimal,
        valuation: Decimal,
        sentiment: Decimal,
        description: String,
    },
    BrokerActivity {
        symbol: Symbol,
        broker_id: i32,
        institution_id: Option<i32>,
        side: String,
        position: i32,
        duration_ticks: u64,
        replenish_count: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketEventKind {
    OrderBookDislocation,
    VolumeDislocation,
    SmartMoneyPressure,
    CandlestickBreakout,
    CompositeAcceleration,
    InstitutionalFlip,
    MarketStressElevated,
    StressRegimeShift,
    ManualReviewRequired,
    SharedHolderAnomaly,
    CatalystActivation,
    IcebergDetected,
    BrokerClusterFormation,
    BrokerSideFlip,
    PropagationAbsence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEventRecord {
    pub scope: SignalScope,
    pub kind: MarketEventKind,
    pub magnitude: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DerivedSignalKind {
    StructuralComposite,
    Convergence,
    ValuationSupport,
    ActivityMomentum,
    CandlestickConviction,
    SmartMoneyPressure,
    MarketStress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalRecord {
    pub scope: SignalScope,
    pub kind: DerivedSignalKind,
    pub strength: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationSnapshot {
    pub timestamp: OffsetDateTime,
    pub observations: Vec<Observation<ObservationRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSnapshot {
    pub timestamp: OffsetDateTime,
    pub events: Vec<Event<MarketEventRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalSnapshot {
    pub timestamp: OffsetDateTime,
    pub signals: Vec<DerivedSignal<DerivedSignalRecord>>,
}

// ── Event Attribution ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPropagationScope {
    Local,
    Sector,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDriverKind {
    CompanySpecific,
    SectorWide,
    MacroWide,
}

const ATTR_SCOPE_PREFIX: &str = "attr:scope=";
const ATTR_DRIVER_PREFIX: &str = "attr:driver=";

/// Static mapping from MarketEventKind to (driver, scope) attribution.
/// Replaces scattered `attribution_inputs("company_specific", "local")` calls
/// throughout events.rs with a single declarative table.
static EVENT_ATTRIBUTION: &[(MarketEventKind, EventDriverKind, EventPropagationScope)] = &[
    // Company-specific, local scope
    (MarketEventKind::OrderBookDislocation, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::VolumeDislocation, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::SmartMoneyPressure, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::CandlestickBreakout, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::IcebergDetected, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::BrokerSideFlip, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    (MarketEventKind::BrokerClusterFormation, EventDriverKind::CompanySpecific, EventPropagationScope::Local),
    // Sector-wide, sector scope
    (MarketEventKind::SharedHolderAnomaly, EventDriverKind::SectorWide, EventPropagationScope::Sector),
    (MarketEventKind::InstitutionalFlip, EventDriverKind::SectorWide, EventPropagationScope::Sector),
    (MarketEventKind::CompositeAcceleration, EventDriverKind::SectorWide, EventPropagationScope::Sector),
    (MarketEventKind::CatalystActivation, EventDriverKind::SectorWide, EventPropagationScope::Sector),
    // Macro-wide, market scope
    (MarketEventKind::MarketStressElevated, EventDriverKind::MacroWide, EventPropagationScope::Market),
    (MarketEventKind::StressRegimeShift, EventDriverKind::MacroWide, EventPropagationScope::Market),
    (MarketEventKind::ManualReviewRequired, EventDriverKind::MacroWide, EventPropagationScope::Market),
    // PropagationAbsence has no attribution (intentionally omitted)
];

/// Look up the attribution (driver, scope) for a given event kind.
/// Returns None for event kinds without attribution (e.g. PropagationAbsence).
pub fn attribution_for_event_kind(kind: &MarketEventKind) -> Option<(EventDriverKind, EventPropagationScope)> {
    EVENT_ATTRIBUTION
        .iter()
        .find(|(k, _, _)| k == kind)
        .map(|(_, driver, scope)| (*driver, *scope))
}

/// Build provenance input strings for a given event kind's attribution.
pub fn attribution_inputs_for_kind(kind: &MarketEventKind) -> Vec<String> {
    attribution_for_event_kind(kind)
        .map(|(driver, scope)| {
            let driver_str = match driver {
                EventDriverKind::CompanySpecific => "company_specific",
                EventDriverKind::SectorWide => "sector_wide",
                EventDriverKind::MacroWide => "macro_wide",
            };
            let scope_str = match scope {
                EventPropagationScope::Local => "local",
                EventPropagationScope::Sector => "sector",
                EventPropagationScope::Market => "market",
            };
            vec![
                format!("{}{}", ATTR_DRIVER_PREFIX, driver_str),
                format!("{}{}", ATTR_SCOPE_PREFIX, scope_str),
            ]
        })
        .unwrap_or_default()
}

pub fn event_propagation_scope(
    event: &crate::ontology::domain::Event<MarketEventRecord>,
) -> Option<EventPropagationScope> {
    event.provenance.inputs.iter().find_map(|input| {
        input
            .strip_prefix(ATTR_SCOPE_PREFIX)
            .and_then(|value| match value {
                "local" => Some(EventPropagationScope::Local),
                "sector" => Some(EventPropagationScope::Sector),
                "market" => Some(EventPropagationScope::Market),
                _ => None,
            })
    })
}

pub fn event_driver_kind(
    event: &crate::ontology::domain::Event<MarketEventRecord>,
) -> Option<EventDriverKind> {
    event.provenance.inputs.iter().find_map(|input| {
        input
            .strip_prefix(ATTR_DRIVER_PREFIX)
            .and_then(|value| match value {
                "company_specific" => Some(EventDriverKind::CompanySpecific),
                "sector_wide" => Some(EventDriverKind::SectorWide),
                "macro_wide" => Some(EventDriverKind::MacroWide),
                _ => None,
            })
    })
}

#[cfg(test)]
mod attribution_tests {
    use super::*;
    use crate::ontology::domain::{Event, ProvenanceMetadata, ProvenanceSource};
    use time::OffsetDateTime;

    fn event_with_provenance(inputs: Vec<&str>) -> Event<MarketEventRecord> {
        Event {
            value: MarketEventRecord {
                scope: SignalScope::Symbol(crate::ontology::objects::Symbol("700.HK".into())),
                kind: MarketEventKind::SmartMoneyPressure,
                magnitude: rust_decimal::Decimal::ONE,
                summary: "test".into(),
            },
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_inputs(inputs.into_iter().map(String::from).collect::<Vec<_>>()),
        }
    }

    #[test]
    fn hk_event_propagation_scope_from_provenance() {
        let event = event_with_provenance(vec!["attr:scope=sector"]);
        assert_eq!(
            event_propagation_scope(&event),
            Some(EventPropagationScope::Sector)
        );
    }

    #[test]
    fn hk_event_propagation_scope_none_when_missing() {
        let event = event_with_provenance(vec!["other:data"]);
        assert_eq!(event_propagation_scope(&event), None);
    }

    #[test]
    fn hk_event_driver_kind_from_provenance() {
        let event = event_with_provenance(vec!["attr:driver=company_specific"]);
        assert_eq!(
            event_driver_kind(&event),
            Some(EventDriverKind::CompanySpecific)
        );
    }

    #[test]
    fn hk_event_driver_kind_none_when_missing() {
        let event = event_with_provenance(vec![]);
        assert_eq!(event_driver_kind(&event), None);
    }
}
