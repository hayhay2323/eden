use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

use super::buffer::UsTickHistory;
use super::record::UsSymbolSignals;

/// Temporal analysis for a single US symbol: how its signals are changing.
#[derive(Debug, Clone)]
pub struct UsSignalDynamics {
    pub symbol: Symbol,
    pub composite_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub composite_duration: u64,
    /// Trend in pre_market_anomaly changes over recent ticks (unique to US).
    /// Positive = pre-market anomaly is increasing, negative = decreasing.
    pub pre_market_trend: Decimal,
}

/// Compute temporal dynamics for all symbols in the US history.
pub fn compute_us_dynamics(history: &UsTickHistory) -> HashMap<Symbol, UsSignalDynamics> {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return HashMap::new();
    }

    let latest = match records.last() {
        Some(r) => r,
        None => return HashMap::new(),
    };

    let mut result = HashMap::new();

    for symbol in latest.signals.keys() {
        let series: Vec<&UsSymbolSignals> = records
            .iter()
            .filter_map(|r| r.signals.get(symbol))
            .collect();

        if series.is_empty() {
            continue;
        }

        let current = series.last().unwrap();
        let prev = if series.len() >= 2 {
            Some(series[series.len() - 2])
        } else {
            None
        };
        let prev_prev = if series.len() >= 3 {
            Some(series[series.len() - 3])
        } else {
            None
        };

        // composite_delta: change from previous tick
        let composite_delta = prev
            .map(|p| current.composite - p.composite)
            .unwrap_or(Decimal::ZERO);

        // composite_acceleration: change in delta
        let prev_delta = match (prev, prev_prev) {
            (Some(p), Some(pp)) => p.composite - pp.composite,
            _ => Decimal::ZERO,
        };
        let composite_acceleration = if prev.is_some() && prev_prev.is_some() {
            composite_delta - prev_delta
        } else {
            Decimal::ZERO
        };

        // composite_duration: consecutive ticks with same sign
        let current_sign = current.composite.signum();
        let mut composite_duration: u64 = 0;
        for s in series.iter().rev() {
            if s.composite.signum() == current_sign {
                composite_duration += 1;
            } else {
                break;
            }
        }

        // pre_market_trend: mean of pre_market_delta over available ticks.
        // pre_market_delta on each tick record tracks (current - previous) pre_post_market_anomaly.
        let pre_market_deltas: Vec<Decimal> = series.iter().map(|s| s.pre_market_delta).collect();
        let pre_market_trend = if pre_market_deltas.is_empty() {
            Decimal::ZERO
        } else {
            let sum: Decimal = pre_market_deltas.iter().copied().sum();
            sum / Decimal::from(pre_market_deltas.len() as i64)
        };

        result.insert(
            symbol.clone(),
            UsSignalDynamics {
                symbol: symbol.clone(),
                composite_delta,
                composite_acceleration,
                composite_duration,
                pre_market_trend,
            },
        );
    }

    result
}

#[cfg(test)]
mod tests {
    use super::super::record::UsTickRecord;
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn make_signal(
        composite: Decimal,
        pre_post: Decimal,
        pre_market_delta: Decimal,
    ) -> UsSymbolSignals {
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
            pre_post_market_anomaly: pre_post,
            valuation: Decimal::ZERO,
            pre_market_delta,
        }
    }

    fn make_tick(tick: u64, sym: &str, sig: UsSymbolSignals) -> UsTickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), sig);
        UsTickRecord {
            tick_number: tick,
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
    fn delta_from_two_ticks() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "AAPL.US",
            make_signal(dec!(0.05), dec!(0.1), dec!(0)),
        ));
        h.push(make_tick(
            2,
            "AAPL.US",
            make_signal(dec!(0.08), dec!(0.15), dec!(0.05)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("AAPL.US".into())];
        assert_eq!(d.composite_delta, dec!(0.03));
    }

    #[test]
    fn acceleration_from_three_ticks() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "AAPL.US",
            make_signal(dec!(0.01), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            2,
            "AAPL.US",
            make_signal(dec!(0.03), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            3,
            "AAPL.US",
            make_signal(dec!(0.06), dec!(0), dec!(0)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("AAPL.US".into())];
        // delta: 0.06 - 0.03 = 0.03
        // prev_delta: 0.03 - 0.01 = 0.02
        // accel: 0.03 - 0.02 = 0.01
        assert_eq!(d.composite_delta, dec!(0.03));
        assert_eq!(d.composite_acceleration, dec!(0.01));
    }

    #[test]
    fn duration_same_sign() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "AAPL.US",
            make_signal(dec!(0.01), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            2,
            "AAPL.US",
            make_signal(dec!(0.03), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            3,
            "AAPL.US",
            make_signal(dec!(0.05), dec!(0), dec!(0)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("AAPL.US".into())];
        assert_eq!(d.composite_duration, 3);
    }

    #[test]
    fn duration_resets_on_sign_change() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "AAPL.US",
            make_signal(dec!(0.05), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            2,
            "AAPL.US",
            make_signal(dec!(-0.02), dec!(0), dec!(0)),
        ));
        h.push(make_tick(
            3,
            "AAPL.US",
            make_signal(dec!(-0.04), dec!(0), dec!(0)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("AAPL.US".into())];
        assert_eq!(d.composite_duration, 2);
    }

    #[test]
    fn pre_market_trend_positive() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "BABA.US",
            make_signal(dec!(0), dec!(0.1), dec!(0.05)),
        ));
        h.push(make_tick(
            2,
            "BABA.US",
            make_signal(dec!(0), dec!(0.2), dec!(0.10)),
        ));
        h.push(make_tick(
            3,
            "BABA.US",
            make_signal(dec!(0), dec!(0.35), dec!(0.15)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("BABA.US".into())];
        // mean of [0.05, 0.10, 0.15] = 0.30 / 3 = 0.1
        assert_eq!(d.pre_market_trend, dec!(0.1));
    }

    #[test]
    fn pre_market_trend_negative() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "BABA.US",
            make_signal(dec!(0), dec!(0.5), dec!(-0.1)),
        ));
        h.push(make_tick(
            2,
            "BABA.US",
            make_signal(dec!(0), dec!(0.3), dec!(-0.2)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("BABA.US".into())];
        // mean of [-0.1, -0.2] = -0.3 / 2 = -0.15
        assert_eq!(d.pre_market_trend, dec!(-0.15));
    }

    #[test]
    fn single_tick_zeroed_deltas() {
        let mut h = UsTickHistory::new(10);
        h.push(make_tick(
            1,
            "NVDA.US",
            make_signal(dec!(0.05), dec!(0.1), dec!(0.03)),
        ));

        let dynamics = compute_us_dynamics(&h);
        let d = &dynamics[&Symbol("NVDA.US".into())];
        assert_eq!(d.composite_delta, Decimal::ZERO);
        assert_eq!(d.composite_acceleration, Decimal::ZERO);
        assert_eq!(d.composite_duration, 1);
        assert_eq!(d.pre_market_trend, dec!(0.03));
    }

    #[test]
    fn empty_history() {
        let h = UsTickHistory::new(10);
        let dynamics = compute_us_dynamics(&h);
        assert!(dynamics.is_empty());
    }

    #[test]
    fn multiple_symbols() {
        let mut h = UsTickHistory::new(10);
        let mut signals = HashMap::new();
        signals.insert(
            Symbol("AAPL.US".into()),
            make_signal(dec!(0.1), dec!(0), dec!(0)),
        );
        signals.insert(
            Symbol("NVDA.US".into()),
            make_signal(dec!(-0.2), dec!(0), dec!(0)),
        );
        h.push(UsTickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: crate::us::graph::decision::UsMarketRegimeBias::Neutral,
        });

        let dynamics = compute_us_dynamics(&h);
        assert_eq!(dynamics.len(), 2);
        assert!(dynamics.contains_key(&Symbol("AAPL.US".into())));
        assert!(dynamics.contains_key(&Symbol("NVDA.US".into())));
    }
}
