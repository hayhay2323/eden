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

    // capital_flow_direction: volume-weighted price impact.
    // Real Longport capital_flow has mean≈-0.1, std≈0.26, range [-1, +0.06].
    // We approximate with dollar-volume-weighted return per bar, scaled to match
    // the real distribution.
    let total_turnover: f64 = window.iter().map(|b| b.turnover).sum();
    let capital_flow: f64 = if total_turnover > 0.0 {
        let weighted_return: f64 = window.iter().map(|b| {
            let bar_return = if b.open != 0.0 { (b.close - b.open) / b.open } else { 0.0 };
            bar_return * b.turnover
        }).sum::<f64>() / total_turnover;
        // Scale: real data std≈0.26. Raw weighted return over 30 bars is tiny (~0.001).
        // Multiply by 200 to get into the right range.
        clamp(weighted_return * 200.0)
    } else {
        0.0
    };

    // price_momentum: return over window.
    // Real momentum has mean≈+0.2, std≈0.67, range [-0.85, +1.0].
    // A 30-bar (30-min) return is typically 0.1-0.5%. Scale by 150 to match.
    let first_open = window.first().unwrap().open;
    let last_close = window.last().unwrap().close;
    let raw_momentum = if first_open != 0.0 {
        (last_close - first_open) / first_open
    } else {
        0.0
    };
    let momentum = clamp(raw_momentum * 150.0);

    // volume_profile: recent 5 bars vs full window.
    // Real volume_profile is often near zero. Keep the scaling modest.
    let total_vol: f64 = window.iter().map(|b| b.volume as f64).sum();
    let window_avg_vol = total_vol / window.len() as f64;
    let recent_5 = &window[window.len().saturating_sub(5)..];
    let recent_avg_vol =
        recent_5.iter().map(|b| b.volume as f64).sum::<f64>() / recent_5.len() as f64;
    let volume_profile = if window_avg_vol > 0.0 {
        clamp((recent_avg_vol / window_avg_vol - 1.0) * 2.0)
    } else {
        0.0
    };

    // composite: match real distribution (mean≈0, std≈0.2, range [-0.3, +0.3])
    let composite = clamp(capital_flow * 0.4 + momentum * 0.4 + volume_profile * 0.2);

    // ── pressure ─────────────────────────────────────────────────────────────

    // pressure_duration: count of consecutive bars at end with same capital_flow sign.
    // Real duration is in ticks (mean≈239, max≈265). The predicate engine uses
    // normalize_count(duration, 8) so we scale bar-count × 30 to approximate ticks.
    let last_dir_up = capital_flow >= 0.0;
    let consecutive_bars = window
        .iter()
        .rev()
        .take_while(|b| (b.close >= b.open) == last_dir_up)
        .count() as u64;
    // Scale to tick-equivalent: each bar ≈ 8 ticks of real-time data at 8s push interval
    let pressure_duration = consecutive_bars * 8;

    // pressure_delta: difference between current and earlier capital_flow
    let half = window.len() / 2;
    let early_half = &window[..half];
    let early_turnover: f64 = early_half.iter().map(|b| b.turnover).sum();
    let early_cf = if early_turnover > 0.0 {
        let wr: f64 = early_half.iter().map(|b| {
            let r = if b.open != 0.0 { (b.close - b.open) / b.open } else { 0.0 };
            r * b.turnover
        }).sum::<f64>() / early_turnover;
        clamp(wr * 200.0)
    } else {
        0.0
    };
    let pressure_delta = capital_flow - early_cf;

    // accelerating: only when delta is meaningful and same-signed
    let accelerating = pressure_delta.abs() > 0.05
        && pressure_delta.signum() == capital_flow.signum()
        && capital_flow.abs() > 0.15;

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

    // Direction threshold: real composite has std≈0.2.
    // Use 0.05 (~0.25 std) as the neutral zone.
    let direction: i8 = if composite > 0.05 {
        1
    } else if composite < -0.05 {
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
