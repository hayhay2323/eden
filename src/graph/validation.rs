use rust_decimal::Decimal;
use std::collections::VecDeque;

use crate::ontology::objects::Symbol;

/// Type of signal that was emitted.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalType {
    OrderBuy,
    OrderSell,
    PressureBullish,
    PressureBearish,
}

/// A recorded signal event with the price at emission time.
#[derive(Debug, Clone)]
pub struct SignalEvent {
    pub tick: u64,
    pub symbol: Symbol,
    pub signal_type: SignalType,
    pub strength: Decimal,
    pub price_at_emission: Decimal,
    pub resolved: bool,
    pub price_at_resolution: Option<Decimal>,
    pub return_pct: Option<Decimal>,
}

/// Aggregated scorecard statistics for a signal type.
#[derive(Debug, Clone)]
pub struct SignalStats {
    pub signal_type: SignalType,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

/// Tracks signal emissions and validates them against future prices.
pub struct SignalScorecard {
    events: VecDeque<SignalEvent>,
    capacity: usize,
    /// How many ticks to wait before resolving a signal
    pub resolution_lag: u64,
}

impl SignalScorecard {
    pub fn new(capacity: usize, resolution_lag: u64) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
            resolution_lag,
        }
    }

    /// Record a new signal event.
    pub fn record(
        &mut self,
        tick: u64,
        symbol: Symbol,
        signal_type: SignalType,
        strength: Decimal,
        price: Decimal,
    ) {
        if price == Decimal::ZERO {
            return; // Can't validate without a price
        }
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(SignalEvent {
            tick,
            symbol,
            signal_type,
            strength,
            price_at_emission: price,
            resolved: false,
            price_at_resolution: None,
            return_pct: None,
        });
    }

    /// Resolve pending events whose resolution_lag has elapsed.
    /// `current_prices` maps Symbol → current last_done price.
    /// `current_tick` is the current tick number.
    pub fn resolve(
        &mut self,
        current_tick: u64,
        current_prices: &std::collections::HashMap<Symbol, Decimal>,
    ) {
        for event in self.events.iter_mut() {
            if event.resolved {
                continue;
            }
            if current_tick < event.tick + self.resolution_lag {
                continue;
            }
            if let Some(&price_now) = current_prices.get(&event.symbol) {
                if price_now == Decimal::ZERO || event.price_at_emission == Decimal::ZERO {
                    continue;
                }
                let return_pct = (price_now - event.price_at_emission) / event.price_at_emission;
                event.price_at_resolution = Some(price_now);
                event.return_pct = Some(return_pct);
                event.resolved = true;
            }
        }
    }

    /// Compute statistics per signal type.
    pub fn stats(&self) -> Vec<SignalStats> {
        let types = [
            SignalType::OrderBuy,
            SignalType::OrderSell,
            SignalType::PressureBullish,
            SignalType::PressureBearish,
        ];

        types
            .into_iter()
            .filter_map(|st| {
                let matching: Vec<&SignalEvent> =
                    self.events.iter().filter(|e| e.signal_type == st).collect();
                let total = matching.len();
                if total == 0 {
                    return None;
                }
                let resolved: Vec<&SignalEvent> =
                    matching.iter().filter(|e| e.resolved).copied().collect();
                let resolved_count = resolved.len();
                if resolved_count == 0 {
                    return Some(SignalStats {
                        signal_type: st,
                        total,
                        resolved: 0,
                        hits: 0,
                        hit_rate: Decimal::ZERO,
                        mean_return: Decimal::ZERO,
                    });
                }

                let hits = resolved
                    .iter()
                    .filter(|e| {
                        let r = e.return_pct.unwrap_or(Decimal::ZERO);
                        match e.signal_type {
                            SignalType::OrderBuy | SignalType::PressureBullish => r > Decimal::ZERO,
                            SignalType::OrderSell | SignalType::PressureBearish => {
                                r < Decimal::ZERO
                            }
                        }
                    })
                    .count();

                let total_return: Decimal = resolved
                    .iter()
                    .map(|e| {
                        let r = e.return_pct.unwrap_or(Decimal::ZERO);
                        match e.signal_type {
                            SignalType::OrderBuy | SignalType::PressureBullish => r,
                            SignalType::OrderSell | SignalType::PressureBearish => -r,
                        }
                    })
                    .sum();

                let hit_rate = Decimal::from(hits as i64) / Decimal::from(resolved_count as i64);
                let mean_return = total_return / Decimal::from(resolved_count as i64);

                Some(SignalStats {
                    signal_type: st,
                    total,
                    resolved: resolved_count,
                    hits,
                    hit_rate,
                    mean_return,
                })
            })
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.events.iter().filter(|e| !e.resolved).count()
    }

    pub fn total_count(&self) -> usize {
        self.events.len()
    }
}

impl std::fmt::Display for SignalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalType::OrderBuy => write!(f, "BUY"),
            SignalType::OrderSell => write!(f, "SELL"),
            SignalType::PressureBullish => write!(f, "PRESS+"),
            SignalType::PressureBearish => write!(f, "PRESS-"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    #[test]
    fn record_and_resolve_buy_signal() {
        let mut sc = SignalScorecard::new(100, 5);
        sc.record(1, sym("700.HK"), SignalType::OrderBuy, dec!(0.1), dec!(100));
        assert_eq!(sc.total_count(), 1);
        assert_eq!(sc.pending_count(), 1);

        // Tick 3: too early to resolve (lag=5)
        let mut prices = HashMap::new();
        prices.insert(sym("700.HK"), dec!(105));
        sc.resolve(3, &prices);
        assert_eq!(sc.pending_count(), 1); // still pending

        // Tick 6: resolve (1 + 5 = 6)
        prices.insert(sym("700.HK"), dec!(110));
        sc.resolve(6, &prices);
        assert_eq!(sc.pending_count(), 0);

        let stats = sc.stats();
        let buy_stats = stats
            .iter()
            .find(|s| s.signal_type == SignalType::OrderBuy)
            .unwrap();
        assert_eq!(buy_stats.total, 1);
        assert_eq!(buy_stats.resolved, 1);
        assert_eq!(buy_stats.hits, 1); // price went up → hit for BUY
        assert_eq!(buy_stats.hit_rate, dec!(1));
        assert!(buy_stats.mean_return > Decimal::ZERO);
    }

    #[test]
    fn sell_signal_hit_when_price_drops() {
        let mut sc = SignalScorecard::new(100, 1);
        sc.record(
            1,
            sym("700.HK"),
            SignalType::OrderSell,
            dec!(0.05),
            dec!(100),
        );

        let mut prices = HashMap::new();
        prices.insert(sym("700.HK"), dec!(95)); // dropped 5%
        sc.resolve(2, &prices);

        let stats = sc.stats();
        let sell_stats = stats
            .iter()
            .find(|s| s.signal_type == SignalType::OrderSell)
            .unwrap();
        assert_eq!(sell_stats.hits, 1); // price dropped → hit for SELL
        assert!(sell_stats.mean_return > Decimal::ZERO); // return is positive (sold before drop)
    }

    #[test]
    fn buy_signal_miss_when_price_drops() {
        let mut sc = SignalScorecard::new(100, 1);
        sc.record(1, sym("700.HK"), SignalType::OrderBuy, dec!(0.1), dec!(100));

        let mut prices = HashMap::new();
        prices.insert(sym("700.HK"), dec!(90)); // dropped 10%
        sc.resolve(2, &prices);

        let stats = sc.stats();
        let buy_stats = stats
            .iter()
            .find(|s| s.signal_type == SignalType::OrderBuy)
            .unwrap();
        assert_eq!(buy_stats.hits, 0); // price dropped → miss for BUY
        assert!(buy_stats.mean_return < Decimal::ZERO);
    }

    #[test]
    fn capacity_evicts_oldest() {
        let mut sc = SignalScorecard::new(2, 1);
        sc.record(1, sym("A"), SignalType::OrderBuy, dec!(0.1), dec!(100));
        sc.record(2, sym("B"), SignalType::OrderBuy, dec!(0.1), dec!(100));
        sc.record(3, sym("C"), SignalType::OrderBuy, dec!(0.1), dec!(100)); // evicts A
        assert_eq!(sc.total_count(), 2);
    }

    #[test]
    fn zero_price_ignored() {
        let mut sc = SignalScorecard::new(100, 1);
        sc.record(
            1,
            sym("700.HK"),
            SignalType::OrderBuy,
            dec!(0.1),
            Decimal::ZERO,
        );
        assert_eq!(sc.total_count(), 0); // not recorded
    }

    #[test]
    fn no_events_no_stats() {
        let sc = SignalScorecard::new(100, 1);
        let stats = sc.stats();
        assert!(stats.is_empty());
    }

    #[test]
    fn mixed_signals_independent_stats() {
        let mut sc = SignalScorecard::new(100, 1);
        sc.record(1, sym("A"), SignalType::OrderBuy, dec!(0.1), dec!(100));
        sc.record(1, sym("B"), SignalType::OrderSell, dec!(0.05), dec!(100));

        let mut prices = HashMap::new();
        prices.insert(sym("A"), dec!(110)); // up
        prices.insert(sym("B"), dec!(90)); // down
        sc.resolve(2, &prices);

        let stats = sc.stats();
        assert_eq!(stats.len(), 2);
        let buy = stats
            .iter()
            .find(|s| s.signal_type == SignalType::OrderBuy)
            .unwrap();
        let sell = stats
            .iter()
            .find(|s| s.signal_type == SignalType::OrderSell)
            .unwrap();
        assert_eq!(buy.hits, 1);
        assert_eq!(sell.hits, 1);
    }
}
