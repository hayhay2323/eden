use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use eden::ontology::domain::{DerivedSignal, Event};
use eden::ontology::objects::Symbol;
use eden::ontology::reasoning::{Hypothesis, TacticalSetup};

use crate::graph::decision::UsMarketRegimeBias;
use crate::graph::propagation::CrossMarketSignal;
use crate::pipeline::signals::{UsDerivedSignalRecord, UsEventRecord};

/// Per-symbol US signals captured at one tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsSymbolSignals {
    pub mark_price: Option<Decimal>,
    /// Mean of the 5 dimension values.
    pub composite: Decimal,
    pub capital_flow_direction: Decimal,
    pub price_momentum: Decimal,
    pub volume_profile: Decimal,
    pub pre_post_market_anomaly: Decimal,
    pub valuation: Decimal,
    /// Change in pre_post_market_anomaly vs previous tick (unique to US).
    pub pre_market_delta: Decimal,
}

/// Compact snapshot of one US pipeline tick's key signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsTickRecord {
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub signals: HashMap<Symbol, UsSymbolSignals>,
    pub cross_market_signals: Vec<CrossMarketSignal>,
    pub events: Vec<Event<UsEventRecord>>,
    pub derived_signals: Vec<DerivedSignal<UsDerivedSignalRecord>>,
    pub hypotheses: Vec<Hypothesis>,
    pub tactical_setups: Vec<TacticalSetup>,
    pub market_regime: UsMarketRegimeBias,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_record(tick: u64) -> UsTickRecord {
        let mut signals = HashMap::new();
        signals.insert(
            Symbol("AAPL.US".into()),
            UsSymbolSignals {
                mark_price: Some(dec!(180)),
                composite: dec!(0.3),
                capital_flow_direction: dec!(0.1),
                price_momentum: dec!(0.4),
                volume_profile: dec!(0.2),
                pre_post_market_anomaly: dec!(0.5),
                valuation: dec!(0.3),
                pre_market_delta: dec!(0.05),
            },
        );
        UsTickRecord {
            tick_number: tick,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: UsMarketRegimeBias::Neutral,
        }
    }

    #[test]
    fn record_fields_accessible() {
        let r = make_record(1);
        assert_eq!(r.tick_number, 1);
        let sig = &r.signals[&Symbol("AAPL.US".into())];
        assert_eq!(sig.composite, dec!(0.3));
        assert_eq!(sig.pre_market_delta, dec!(0.05));
    }

    #[test]
    fn record_serde_roundtrip() {
        let r = make_record(42);
        let json = serde_json::to_string(&r).unwrap();
        let parsed: UsTickRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.tick_number, 42);
        assert_eq!(
            parsed.signals[&Symbol("AAPL.US".into())].composite,
            dec!(0.3)
        );
    }

    #[test]
    fn record_clone() {
        let r = make_record(1);
        let r2 = r.clone();
        assert_eq!(r.tick_number, r2.tick_number);
        assert_eq!(r.signals.len(), r2.signals.len());
    }

    #[test]
    fn record_empty_signals() {
        let r = UsTickRecord {
            tick_number: 0,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: HashMap::new(),
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: UsMarketRegimeBias::Neutral,
        };
        assert!(r.signals.is_empty());
        assert!(r.cross_market_signals.is_empty());
    }
}
