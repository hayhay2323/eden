use std::collections::VecDeque;

use crate::ontology::objects::Symbol;
use rust_decimal::Decimal;

use super::record::{UsSymbolSignals, UsTickRecord};

/// Ring buffer of recent US tick records.
/// Capacity is fixed at creation; oldest ticks are evicted when full.
pub struct UsTickHistory {
    records: VecDeque<UsTickRecord>,
    capacity: usize,
}

impl UsTickHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, record: UsTickRecord) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn latest(&self) -> Option<&UsTickRecord> {
        self.records.back()
    }

    pub fn oldest(&self) -> Option<&UsTickRecord> {
        self.records.front()
    }

    /// Return the last N records in chronological order.
    pub fn latest_n(&self, n: usize) -> Vec<&UsTickRecord> {
        let skip = self.records.len().saturating_sub(n);
        self.records.iter().skip(skip).collect()
    }

    /// Extract a time series of a specific field for a symbol.
    /// Returns values in chronological order, skipping ticks where the symbol is absent.
    pub fn signal_series<F>(&self, symbol: &Symbol, extractor: F) -> Vec<Decimal>
    where
        F: Fn(&UsSymbolSignals) -> Decimal,
    {
        self.records
            .iter()
            .filter_map(|r| r.signals.get(symbol).map(|s| extractor(s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn make_signal(composite: Decimal) -> UsSymbolSignals {
        UsSymbolSignals {
            mark_price: None,
            composite,
            composite_delta: Decimal::ZERO,
            composite_acceleration: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_flow_delta: Decimal::ZERO,
            flow_persistence: 0,
            flow_reversal: false,
            price_momentum: Decimal::ZERO,
            volume_profile: Decimal::ZERO,
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            pre_market_delta: Decimal::ZERO,
        }
    }

    fn make_tick(tick_number: u64, sym: &str, composite: Decimal) -> UsTickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), make_signal(composite));
        UsTickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: crate::us::graph::decision::UsMarketRegimeBias::Neutral,
        }
    }

    #[test]
    fn push_and_len() {
        let mut h = UsTickHistory::new(10);
        assert_eq!(h.len(), 0);
        h.push(make_tick(1, "AAPL.US", dec!(0.1)));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut h = UsTickHistory::new(3);
        h.push(make_tick(1, "AAPL.US", dec!(0.1)));
        h.push(make_tick(2, "AAPL.US", dec!(0.2)));
        h.push(make_tick(3, "AAPL.US", dec!(0.3)));
        h.push(make_tick(4, "AAPL.US", dec!(0.4)));
        assert_eq!(h.len(), 3);
        assert_eq!(h.oldest().unwrap().tick_number, 2);
        assert_eq!(h.latest().unwrap().tick_number, 4);
    }

    #[test]
    fn latest_n() {
        let mut h = UsTickHistory::new(10);
        for i in 1..=5 {
            h.push(make_tick(i, "AAPL.US", Decimal::from(i)));
        }
        let last3 = h.latest_n(3);
        assert_eq!(last3.len(), 3);
        assert_eq!(last3[0].tick_number, 3);
        assert_eq!(last3[2].tick_number, 5);
    }

    #[test]
    fn signal_series() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(1, "AAPL.US", dec!(0.1)));
        h.push(make_tick(2, "AAPL.US", dec!(0.3)));
        h.push(make_tick(3, "AAPL.US", dec!(0.5)));

        let series = h.signal_series(&Symbol("AAPL.US".into()), |s| s.composite);
        assert_eq!(series, vec![dec!(0.1), dec!(0.3), dec!(0.5)]);
    }

    #[test]
    fn signal_series_missing_symbol() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(1, "AAPL.US", dec!(0.1)));

        let series = h.signal_series(&Symbol("NVDA.US".into()), |s| s.composite);
        assert!(series.is_empty());
    }

    #[test]
    fn empty_buffer() {
        let h = UsTickHistory::new(10);
        assert!(h.latest().is_none());
        assert!(h.oldest().is_none());
        assert!(h.latest_n(5).is_empty());
    }

    #[test]
    fn latest_n_more_than_available() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(1, "AAPL.US", dec!(0.1)));
        h.push(make_tick(2, "AAPL.US", dec!(0.2)));
        let all = h.latest_n(100);
        assert_eq!(all.len(), 2);
    }
}
