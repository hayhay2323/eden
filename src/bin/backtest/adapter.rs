use rust_decimal::Decimal;

use eden::live_snapshot::{
    LiveMarketRegime, LivePressure, LiveSignal, LiveStressSnapshot, LiveTacticalCase,
};

use super::loader::Bar;

// ── helpers ──────────────────────────────────────────────────────────────────

fn clamp(v: f64) -> f64 {
    v.max(-1.0).min(1.0)
}

fn to_dec(v: f64) -> Decimal {
    Decimal::try_from(v).unwrap_or(Decimal::ZERO)
}

// ── public types ─────────────────────────────────────────────────────────────

pub struct SyntheticTick {
    pub case: LiveTacticalCase,
    pub signal: LiveSignal,
    pub pressure: LivePressure,
    pub stress: LiveStressSnapshot,
    pub regime: LiveMarketRegime,
    pub direction: i8,  // +1 bullish, -1 bearish, 0 neutral
    pub timestamp: i64,
}

// ── main function ─────────────────────────────────────────────────────────────

pub fn build_synthetic_tick(
    symbol: &str,
    sector: &str,
    window: &[Bar],
    all_symbols_bars: &[(String, &[Bar])],
) -> Option<SyntheticTick> {
    if window.len() < 5 {
        return None;
    }

    let timestamp = window.last().unwrap().ts;

    // ── signal dimensions ────────────────────────────────────────────────────

    // capital_flow_direction: volume-weighted up vs down
    let (up_vol, down_vol): (f64, f64) = window.iter().fold((0.0, 0.0), |(u, d), bar| {
        let vol = bar.volume as f64;
        if bar.close >= bar.open {
            (u + vol, d)
        } else {
            (u, d + vol)
        }
    });
    let total_vol = up_vol + down_vol;
    let capital_flow: f64 = if total_vol > 0.0 {
        (up_vol - down_vol) / total_vol
    } else {
        0.0
    };

    // price_momentum: return over window, scaled so 5% = 1.0
    let first_open = window.first().unwrap().open;
    let last_close = window.last().unwrap().close;
    let raw_momentum = if first_open != 0.0 {
        (last_close - first_open) / first_open
    } else {
        0.0
    };
    let momentum = clamp(raw_momentum * 20.0);

    // volume_profile: recent 5 bars vs full window
    let window_avg_vol = window.iter().map(|b| b.volume as f64).sum::<f64>() / window.len() as f64;
    let recent_5 = &window[window.len().saturating_sub(5)..];
    let recent_avg_vol =
        recent_5.iter().map(|b| b.volume as f64).sum::<f64>() / recent_5.len() as f64;
    let volume_profile = if window_avg_vol > 0.0 {
        clamp((recent_avg_vol / window_avg_vol - 1.0) * 2.0)
    } else {
        0.0
    };

    // composite
    let composite = capital_flow * 0.4 + momentum * 0.4 + volume_profile * 0.2;

    // ── pressure ─────────────────────────────────────────────────────────────

    // pressure_duration: count of consecutive bars at end with same direction
    let last_dir_up = window.last().map(|b| b.close >= b.open).unwrap_or(true);
    let pressure_duration = window
        .iter()
        .rev()
        .take_while(|b| (b.close >= b.open) == last_dir_up)
        .count() as u64;

    // capital_flow of the earlier half of the window
    let half = window.len() / 2;
    let early_half = &window[..half];
    let (eu, ed): (f64, f64) = early_half.iter().fold((0.0, 0.0), |(u, d), bar| {
        let vol = bar.volume as f64;
        if bar.close >= bar.open {
            (u + vol, d)
        } else {
            (u, d + vol)
        }
    });
    let early_total = eu + ed;
    let early_cf = if early_total > 0.0 {
        (eu - ed) / early_total
    } else {
        0.0
    };
    let pressure_delta = capital_flow - early_cf;

    let accelerating = pressure_delta > 0.1 && pressure_delta.signum() == capital_flow.signum();

    // ── stress (cross-sectional) ─────────────────────────────────────────────

    let composite_stress: f64 = if all_symbols_bars.is_empty() {
        0.0
    } else {
        let mut negative_count = 0usize;
        let mut total_magnitude = 0.0f64;
        let mut count = 0usize;

        for (_sym, bars) in all_symbols_bars {
            // bars within 120 seconds of current timestamp
            let nearby: Vec<&Bar> = bars
                .iter()
                .filter(|b| (b.ts - timestamp).abs() <= 120)
                .collect();

            if nearby.len() < 2 {
                continue;
            }

            let first_close = nearby.first().unwrap().close;
            let last_c = nearby.last().unwrap().close;
            let ret = if first_close != 0.0 {
                (last_c - first_close) / first_close
            } else {
                0.0
            };

            total_magnitude += ret.abs();
            if ret < 0.0 {
                negative_count += 1;
            }
            count += 1;
        }

        if count == 0 {
            0.0
        } else {
            let prop_negative = negative_count as f64 / count as f64;
            let avg_magnitude = total_magnitude / count as f64;
            clamp(prop_negative * avg_magnitude * 20.0) // scale similarly to momentum
        }
    };

    // ── direction ────────────────────────────────────────────────────────────

    let direction: i8 = if composite > 0.1 {
        1
    } else if composite < -0.1 {
        -1
    } else {
        0
    };

    // ── construct Live* types ─────────────────────────────────────────────────

    let confidence = composite.abs();
    let confidence_gap = confidence * 0.2;
    let heuristic_edge = confidence * 0.15;

    let case = LiveTacticalCase {
        setup_id: format!("bt:{}:{}", symbol, timestamp),
        symbol: symbol.to_string(),
        title: format!("Backtest tick {} @ {}", symbol, timestamp),
        action: "enter".to_string(),
        confidence: to_dec(confidence),
        confidence_gap: to_dec(confidence_gap),
        heuristic_edge: to_dec(heuristic_edge),
        entry_rationale: format!(
            "cf={:.3} mom={:.3} vol={:.3} composite={:.3}",
            capital_flow, momentum, volume_profile, composite
        ),
        family_label: None,
        counter_label: None,
    };

    let signal = LiveSignal {
        symbol: symbol.to_string(),
        sector: Some(sector.to_string()),
        composite: to_dec(composite),
        mark_price: Some(to_dec(last_close)),
        dimension_composite: Some(to_dec(composite)),
        capital_flow_direction: to_dec(capital_flow),
        price_momentum: to_dec(momentum),
        volume_profile: to_dec(volume_profile),
        pre_post_market_anomaly: Decimal::ZERO,
        valuation: Decimal::ZERO,
        cross_stock_correlation: None,
        sector_coherence: None,
        cross_market_propagation: None,
    };

    let pressure = LivePressure {
        symbol: symbol.to_string(),
        sector: Some(sector.to_string()),
        capital_flow_pressure: to_dec(capital_flow),
        momentum: to_dec(momentum),
        pressure_delta: to_dec(pressure_delta),
        pressure_duration,
        accelerating,
    };

    let stress = LiveStressSnapshot {
        composite_stress: to_dec(composite_stress),
        sector_synchrony: None,
        pressure_consensus: None,
        momentum_consensus: None,
        pressure_dispersion: None,
        volume_anomaly: None,
    };

    let regime = LiveMarketRegime {
        bias: "neutral".to_string(),
        confidence: Decimal::ZERO,
        breadth_up: Decimal::ZERO,
        breadth_down: Decimal::ZERO,
        average_return: Decimal::ZERO,
        directional_consensus: None,
        pre_market_sentiment: None,
    };

    Some(SyntheticTick {
        case,
        signal,
        pressure,
        stress,
        regime,
        direction,
        timestamp,
    })
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bars(prices: &[(f64, f64)], base_volume: u64) -> Vec<Bar> {
        prices
            .iter()
            .enumerate()
            .map(|(i, (open, close))| Bar {
                symbol: "700.HK".into(),
                ts: 1700000000 + (i as i64) * 60,
                open: *open,
                high: close.max(*open) + 0.5,
                low: close.min(*open) - 0.5,
                close: *close,
                volume: base_volume,
                turnover: base_volume as f64 * close,
            })
            .collect()
    }

    #[test]
    fn uptrend_produces_positive_direction() {
        let bars = make_bars(
            &[
                (100.0, 101.0),
                (101.0, 102.0),
                (102.0, 103.0),
                (103.0, 104.0),
                (104.0, 105.0),
                (105.0, 106.0),
            ],
            100000,
        );
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, 1, "uptrend should be bullish");
    }

    #[test]
    fn downtrend_produces_negative_direction() {
        let bars = make_bars(
            &[
                (106.0, 105.0),
                (105.0, 104.0),
                (104.0, 103.0),
                (103.0, 102.0),
                (102.0, 101.0),
                (101.0, 100.0),
            ],
            100000,
        );
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, -1, "downtrend should be bearish");
    }

    #[test]
    fn flat_produces_neutral() {
        let bars = make_bars(
            &[
                (100.0, 100.05),
                (100.05, 100.0),
                (100.0, 100.05),
                (100.05, 100.0),
                (100.0, 100.05),
                (100.05, 100.0),
            ],
            100000,
        );
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, 0, "flat should be neutral");
    }

    #[test]
    fn too_few_bars_returns_none() {
        let bars = make_bars(&[(100.0, 101.0), (101.0, 102.0)], 100000);
        assert!(build_synthetic_tick("700.HK", "tech", &bars, &[]).is_none());
    }
}
