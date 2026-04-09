use std::collections::{HashMap, VecDeque};

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;

/// Per-symbol aggregated signal history across multiple time horizons.
/// Runtime holds this across ticks, reasoning consults it for stability.
#[derive(Debug, Clone, Default)]
pub struct SignalAggregator {
    entries: HashMap<Symbol, SymbolSignalHistory>,
}

#[derive(Debug, Clone)]
struct SignalEntry {
    prio: Decimal,
    is_enter: bool,
    timestamp: OffsetDateTime,
}

#[derive(Debug, Clone, Default)]
struct SymbolSignalHistory {
    entries: VecDeque<SignalEntry>,
}

/// Aggregated signal stats for a symbol over a time window.
#[derive(Debug, Clone)]
pub struct AggregatedSignal {
    pub symbol: Symbol,
    pub count: usize,
    pub avg_prio: Decimal,
    pub max_prio: Decimal,
    pub enter_count: usize,
    pub trend: SignalTrend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalTrend {
    Rising,
    Falling,
    Stable,
}

impl SignalAggregator {
    /// Record a convergence hypothesis signal for a symbol this tick.
    pub fn record(&mut self, symbol: &Symbol, prio: Decimal, is_enter: bool, now: OffsetDateTime) {
        let history = self.entries.entry(symbol.clone()).or_default();
        history.entries.push_back(SignalEntry {
            prio,
            is_enter,
            timestamp: now,
        });
        // Keep max 500 entries per symbol (~30 min at 1 tick/3s)
        while history.entries.len() > 500 {
            history.entries.pop_front();
        }
    }

    /// Get aggregated signal for a symbol over the last `window` minutes.
    pub fn aggregate(
        &self,
        symbol: &Symbol,
        window_minutes: i64,
        now: OffsetDateTime,
    ) -> Option<AggregatedSignal> {
        let history = self.entries.get(symbol)?;
        let cutoff = now - time::Duration::minutes(window_minutes);
        let recent: Vec<_> = history
            .entries
            .iter()
            .filter(|e| e.timestamp >= cutoff)
            .collect();

        if recent.is_empty() {
            return None;
        }

        let count = recent.len();
        let sum_prio: Decimal = recent.iter().map(|e| e.prio).sum();
        let avg_prio = sum_prio / Decimal::from(count as i64);
        let max_prio = recent.iter().map(|e| e.prio).max().unwrap_or(Decimal::ZERO);
        let enter_count = recent.iter().filter(|e| e.is_enter).count();

        // Trend: compare first half avg vs second half avg
        let mid = count / 2;
        let trend = if count >= 4 {
            let first_half_avg: Decimal = recent[..mid].iter().map(|e| e.prio).sum::<Decimal>()
                / Decimal::from(mid.max(1) as i64);
            let second_half_avg: Decimal = recent[mid..].iter().map(|e| e.prio).sum::<Decimal>()
                / Decimal::from((count - mid).max(1) as i64);
            let delta = second_half_avg - first_half_avg;
            if delta > Decimal::new(5, 2) {
                SignalTrend::Rising
            } else if delta < Decimal::new(-5, 2) {
                SignalTrend::Falling
            } else {
                SignalTrend::Stable
            }
        } else {
            SignalTrend::Stable
        };

        Some(AggregatedSignal {
            symbol: symbol.clone(),
            count,
            avg_prio,
            max_prio,
            enter_count,
            trend,
        })
    }

    /// Get all symbols with signals in the window, sorted by avg_prio descending.
    pub fn top_signals(&self, window_minutes: i64, now: OffsetDateTime) -> Vec<AggregatedSignal> {
        let mut signals: Vec<_> = self
            .entries
            .keys()
            .filter_map(|sym| self.aggregate(sym, window_minutes, now))
            .collect();
        signals.sort_by(|a, b| {
            b.avg_prio
                .partial_cmp(&a.avg_prio)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        signals
    }

    /// Check if a symbol should be considered for entry.
    /// Requires: count >= min_count, avg_prio >= min_avg, enter_count >= min_enters.
    pub fn qualifies_for_entry(
        &self,
        symbol: &Symbol,
        window_minutes: i64,
        now: OffsetDateTime,
        min_count: usize,
        min_avg_prio: Decimal,
        min_enters: usize,
    ) -> bool {
        self.aggregate(symbol, window_minutes, now)
            .map(|agg| {
                agg.count >= min_count
                    && agg.avg_prio >= min_avg_prio
                    && agg.enter_count >= min_enters
            })
            .unwrap_or(false)
    }

    /// Check if a symbol's signal has deteriorated enough to exit.
    /// Returns true if: no signals in window, or avg_prio dropped > threshold from reference.
    pub fn should_exit(
        &self,
        symbol: &Symbol,
        window_minutes: i64,
        now: OffsetDateTime,
        reference_prio: Decimal,
        drop_threshold: Decimal,
    ) -> bool {
        match self.aggregate(symbol, window_minutes, now) {
            None => true, // No signals = Eden lost interest
            Some(agg) => {
                // Avg prio dropped more than threshold from entry reference
                reference_prio > Decimal::ZERO
                    && agg.avg_prio < reference_prio * (Decimal::ONE - drop_threshold)
            }
        }
    }

    /// Remove symbols with no recent activity.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::hours(1);
        for history in self.entries.values_mut() {
            history.entries.retain(|e| e.timestamp >= cutoff);
        }
        self.entries.retain(|_, h| !h.entries.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn aggregate_computes_stats() {
        let mut agg = SignalAggregator::default();
        let sym = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();

        agg.record(&sym, dec!(1.1), true, now);
        agg.record(&sym, dec!(1.2), true, now);
        agg.record(&sym, dec!(0.9), false, now);

        let result = agg.aggregate(&sym, 30, now).unwrap();
        assert_eq!(result.count, 3);
        assert_eq!(result.enter_count, 2);
        assert!(result.avg_prio > dec!(1.0));
        assert_eq!(result.max_prio, dec!(1.2));
    }

    #[test]
    fn qualifies_for_entry_checks_all_conditions() {
        let mut agg = SignalAggregator::default();
        let sym = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();

        for _ in 0..5 {
            agg.record(&sym, dec!(1.1), true, now);
        }

        assert!(agg.qualifies_for_entry(&sym, 30, now, 5, dec!(0.9), 3));
        assert!(!agg.qualifies_for_entry(&sym, 30, now, 10, dec!(0.9), 3)); // count too low
        assert!(!agg.qualifies_for_entry(&sym, 30, now, 5, dec!(1.5), 3)); // avg too low
    }

    #[test]
    fn should_exit_on_no_signals() {
        let agg = SignalAggregator::default();
        let sym = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();

        assert!(agg.should_exit(&sym, 30, now, dec!(1.0), dec!(0.3)));
    }

    #[test]
    fn should_exit_on_prio_drop() {
        let mut agg = SignalAggregator::default();
        let sym = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();

        agg.record(&sym, dec!(0.5), false, now);

        // Reference was 1.0, current avg is 0.5, drop > 30%
        assert!(agg.should_exit(&sym, 30, now, dec!(1.0), dec!(0.3)));
    }

    #[test]
    fn trend_detects_rising() {
        let mut agg = SignalAggregator::default();
        let sym = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();

        // First half low, second half high
        agg.record(&sym, dec!(0.5), false, now);
        agg.record(&sym, dec!(0.6), false, now);
        agg.record(&sym, dec!(0.9), true, now);
        agg.record(&sym, dec!(1.1), true, now);

        let result = agg.aggregate(&sym, 30, now).unwrap();
        assert_eq!(result.trend, SignalTrend::Rising);
    }
}
