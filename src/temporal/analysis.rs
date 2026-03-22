use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

use super::buffer::TickHistory;
use super::record::SymbolSignals;

/// Temporal analysis for a single symbol: how its signals are changing.
#[derive(Debug, Clone)]
pub struct SignalDynamics {
    pub symbol: Symbol,
    pub composite_delta: Decimal,
    pub convergence_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub composite_duration: u64,
    pub inst_alignment_delta: Decimal,
    pub bid_wall_delta: Decimal,
    pub ask_wall_delta: Decimal,
    pub buy_ratio_trend: Decimal,
}

#[derive(Debug, Clone)]
pub struct PolymarketDynamics {
    pub slug: String,
    pub label: String,
    pub probability_delta: Decimal,
    pub probability_acceleration: Decimal,
    pub current_probability: Decimal,
}

/// Compute temporal dynamics for all symbols in the history.
pub fn compute_dynamics(history: &TickHistory) -> HashMap<Symbol, SignalDynamics> {
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
        let series: Vec<&SymbolSignals> = records
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

        let composite_delta = prev
            .map(|p| current.composite - p.composite)
            .unwrap_or(Decimal::ZERO);
        let convergence_delta = prev
            .map(|p| {
                current.convergence_score.unwrap_or(Decimal::ZERO)
                    - p.convergence_score.unwrap_or(Decimal::ZERO)
            })
            .unwrap_or(Decimal::ZERO);
        let inst_alignment_delta = prev
            .map(|p| current.institutional_alignment - p.institutional_alignment)
            .unwrap_or(Decimal::ZERO);
        let bid_wall_delta = prev
            .map(|p| current.bid_top3_ratio - p.bid_top3_ratio)
            .unwrap_or(Decimal::ZERO);
        let ask_wall_delta = prev
            .map(|p| current.ask_top3_ratio - p.ask_top3_ratio)
            .unwrap_or(Decimal::ZERO);

        let prev_delta = match (prev, prev_prev) {
            (Some(p), Some(pp)) => p.composite - pp.composite,
            _ => Decimal::ZERO,
        };
        let composite_acceleration = if prev.is_some() && prev_prev.is_some() {
            composite_delta - prev_delta
        } else {
            Decimal::ZERO
        };

        let current_sign = current.composite.signum();
        let mut composite_duration: u64 = 0;
        for s in series.iter().rev() {
            if s.composite.signum() == current_sign {
                composite_duration += 1;
            } else {
                break;
            }
        }

        let total_buy: i64 = series.iter().map(|s| s.buy_volume).sum();
        let total_vol: i64 = series.iter().map(|s| s.trade_volume).sum();
        let buy_ratio_trend = if total_vol > 0 {
            Decimal::from(total_buy) / Decimal::from(total_vol)
        } else {
            Decimal::ZERO
        };

        result.insert(
            symbol.clone(),
            SignalDynamics {
                symbol: symbol.clone(),
                composite_delta,
                convergence_delta,
                composite_acceleration,
                composite_duration,
                inst_alignment_delta,
                bid_wall_delta,
                ask_wall_delta,
                buy_ratio_trend,
            },
        );
    }

    result
}

pub fn compute_polymarket_dynamics(history: &TickHistory) -> Vec<PolymarketDynamics> {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return vec![];
    }

    let mut series_by_slug = HashMap::<String, Vec<(String, Decimal)>>::new();
    for record in &records {
        for prior in &record.polymarket_priors {
            series_by_slug
                .entry(prior.slug.clone())
                .or_default()
                .push((prior.label.clone(), prior.probability));
        }
    }

    let mut dynamics = series_by_slug
        .into_iter()
        .filter_map(|(slug, series)| {
            let (label, current_probability) = series.last()?.clone();
            let prev = series
                .len()
                .checked_sub(2)
                .and_then(|index| series.get(index))
                .map(|(_, probability)| *probability)
                .unwrap_or(Decimal::ZERO);
            let prev_prev = series
                .len()
                .checked_sub(3)
                .and_then(|index| series.get(index))
                .map(|(_, probability)| *probability)
                .unwrap_or(Decimal::ZERO);
            let probability_delta = current_probability - prev;
            let prev_delta = if series.len() >= 3 {
                prev - prev_prev
            } else {
                Decimal::ZERO
            };

            Some(PolymarketDynamics {
                slug,
                label,
                probability_delta,
                probability_acceleration: probability_delta - prev_delta,
                current_probability,
            })
        })
        .collect::<Vec<_>>();

    dynamics.sort_by(|a, b| b.probability_delta.abs().cmp(&a.probability_delta.abs()));
    dynamics
}

#[cfg(test)]
mod tests {
    use super::super::record::TickRecord;
    use super::*;
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn make_signal_full(
        composite: Decimal,
        inst: Decimal,
        bid_top3: Decimal,
        ask_top3: Decimal,
        buy_vol: i64,
        sell_vol: i64,
    ) -> SymbolSignals {
        SymbolSignals {
            mark_price: None,
            composite,
            institutional_alignment: inst,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: bid_top3,
            ask_top3_ratio: ask_top3,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: buy_vol + sell_vol,
            buy_volume: buy_vol,
            sell_volume: sell_vol,
            vwap: None,
            convergence_score: None,
            composite_degradation: None,
            institution_retention: None,
        }
    }

    fn make_tick(tick: u64, sym: &str, sig: SymbolSignals) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), sig);
        TickRecord {
            tick_number: tick,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
        }
    }

    #[test]
    fn delta_from_two_ticks() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0.05), dec!(0.1), dec!(0.3), dec!(0.4), 100, 50),
        ));
        h.push(make_tick(
            2,
            "700.HK",
            make_signal_full(dec!(0.08), dec!(0.15), dec!(0.35), dec!(0.38), 200, 80),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_delta, dec!(0.03));
        assert_eq!(d.inst_alignment_delta, dec!(0.05));
        assert_eq!(d.bid_wall_delta, dec!(0.05));
        assert_eq!(d.ask_wall_delta, dec!(-0.02));
    }

    #[test]
    fn acceleration_from_three_ticks() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0.01), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            2,
            "700.HK",
            make_signal_full(dec!(0.03), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            3,
            "700.HK",
            make_signal_full(dec!(0.06), dec!(0), dec!(0), dec!(0), 0, 0),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_delta, dec!(0.03));
        assert_eq!(d.composite_acceleration, dec!(0.01));
    }

    #[test]
    fn duration_same_sign() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0.01), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            2,
            "700.HK",
            make_signal_full(dec!(0.03), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            3,
            "700.HK",
            make_signal_full(dec!(0.05), dec!(0), dec!(0), dec!(0), 0, 0),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_duration, 3);
    }

    #[test]
    fn duration_resets_on_sign_change() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0.05), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            2,
            "700.HK",
            make_signal_full(dec!(-0.02), dec!(0), dec!(0), dec!(0), 0, 0),
        ));
        h.push(make_tick(
            3,
            "700.HK",
            make_signal_full(dec!(-0.04), dec!(0), dec!(0), dec!(0), 0, 0),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_duration, 2);
    }

    #[test]
    fn buy_ratio_trend() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 100, 100),
        ));
        h.push(make_tick(
            2,
            "700.HK",
            make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 150, 50),
        ));
        h.push(make_tick(
            3,
            "700.HK",
            make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 200, 50),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert!(d.buy_ratio_trend > dec!(0.69));
        assert!(d.buy_ratio_trend < dec!(0.70));
    }

    #[test]
    fn single_tick_zeroed_deltas() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(
            1,
            "700.HK",
            make_signal_full(dec!(0.05), dec!(0.1), dec!(0.3), dec!(0.4), 100, 50),
        ));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_delta, Decimal::ZERO);
        assert_eq!(d.composite_acceleration, Decimal::ZERO);
        assert_eq!(d.composite_duration, 1);
    }

    #[test]
    fn empty_history() {
        let h = TickHistory::new(10);
        let dynamics = compute_dynamics(&h);
        assert!(dynamics.is_empty());
    }
}
