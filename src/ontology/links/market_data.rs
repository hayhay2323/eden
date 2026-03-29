use super::*;
use crate::math::clamp_signed_unit_interval;

pub(super) fn compute_capital_flows(raw: &RawSnapshot) -> Vec<CapitalFlow> {
    raw.capital_flows
        .iter()
        .filter_map(|(symbol, lines)| {
            lines.last().map(|line| CapitalFlow {
                symbol: symbol.clone(),
                net_inflow: YuanAmount::from_ten_thousands(line.inflow),
            })
        })
        .collect()
}

pub(super) fn compute_capital_flow_series(raw: &RawSnapshot) -> Vec<CapitalFlowTimeSeries> {
    raw.capital_flows
        .iter()
        .filter_map(|(symbol, lines)| {
            if lines.is_empty() {
                return None;
            }

            let points: Vec<CapitalFlowPoint> = lines
                .iter()
                .map(|line| CapitalFlowPoint {
                    timestamp: line.timestamp,
                    inflow: line.inflow,
                })
                .collect();

            let last = lines.last().unwrap();
            let latest_inflow = YuanAmount::from_ten_thousands(last.inflow);

            let velocity = if lines.len() >= 2 {
                let prev = &lines[lines.len() - 2];
                let curr = last;
                let dt_seconds = (curr.timestamp - prev.timestamp).whole_seconds();
                if dt_seconds > 0 {
                    let dt_minutes = Decimal::from(dt_seconds) / Decimal::from(60);
                    (curr.inflow - prev.inflow) / dt_minutes
                } else {
                    Decimal::ZERO
                }
            } else {
                Decimal::ZERO
            };

            Some(CapitalFlowTimeSeries {
                symbol: symbol.clone(),
                points,
                latest_inflow,
                velocity,
            })
        })
        .collect()
}

pub(super) fn compute_calc_indexes(raw: &RawSnapshot) -> Vec<CalcIndexObservation> {
    raw.calc_indexes
        .iter()
        .map(|(symbol, idx)| CalcIndexObservation {
            symbol: symbol.clone(),
            turnover_rate: idx.turnover_rate,
            volume_ratio: idx.volume_ratio,
            pe_ttm_ratio: idx.pe_ttm_ratio,
            pb_ratio: idx.pb_ratio,
            dividend_ratio_ttm: idx.dividend_ratio_ttm,
            amplitude: idx.amplitude,
            five_minutes_change_rate: idx.five_minutes_change_rate,
        })
        .collect()
}

pub(super) fn compute_capital_breakdowns(raw: &RawSnapshot) -> Vec<CapitalBreakdown> {
    raw.capital_distributions
        .iter()
        .map(|(symbol, dist)| CapitalBreakdown {
            symbol: symbol.clone(),
            large_net: dist.capital_in.large - dist.capital_out.large,
            medium_net: dist.capital_in.medium - dist.capital_out.medium,
            small_net: dist.capital_in.small - dist.capital_out.small,
        })
        .collect()
}

pub(super) fn compute_candlesticks(raw: &RawSnapshot) -> Vec<CandlestickObservation> {
    raw.candlesticks
        .iter()
        .filter_map(|(symbol, candles)| {
            let latest = candles.last()?;
            let first = candles
                .iter()
                .rev()
                .take(5)
                .last()
                .copied()
                .unwrap_or(*latest);

            let window_high = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.high)
                .max()
                .unwrap_or(latest.high);
            let window_low = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.low)
                .min()
                .unwrap_or(latest.low);

            let window_return = if first.open > Decimal::ZERO {
                clamp_signed_unit_interval(
                    (latest.close - first.open) / first.open / Decimal::new(2, 2),
                )
            } else {
                Decimal::ZERO
            };

            let latest_range = latest.high - latest.low;
            let body_bias = if latest_range > Decimal::ZERO {
                clamp_signed_unit_interval((latest.close - latest.open) / latest_range)
            } else {
                Decimal::ZERO
            };

            let recent = candles.iter().rev().take(5).collect::<Vec<_>>();
            let average_volume = if recent.is_empty() {
                Decimal::ZERO
            } else {
                Decimal::from(recent.iter().map(|c| c.volume).sum::<i64>())
                    / Decimal::from(recent.len() as i64)
            };
            let volume_ratio = if average_volume > Decimal::ZERO {
                Decimal::from(latest.volume) / average_volume
            } else {
                Decimal::ZERO
            };

            let range_ratio = if first.open > Decimal::ZERO {
                clamp_signed_unit_interval(
                    (window_high - window_low) / first.open / candle_range_normalizer(),
                )
            } else {
                Decimal::ZERO
            };

            Some(CandlestickObservation {
                symbol: symbol.clone(),
                candle_count: candles.len(),
                window_return,
                body_bias,
                volume_ratio,
                range_ratio,
            })
        })
        .collect()
}

pub(super) fn compute_depth_profile(levels: &[DepthLevel], best_price: Option<Decimal>) -> DepthProfile {
    let active_levels = levels.iter().filter(|l| l.volume > 0).count();
    let total_vol: i64 = levels.iter().map(|l| l.volume).sum();

    if total_vol == 0 || levels.is_empty() {
        return DepthProfile {
            top3_volume_ratio: Decimal::ZERO,
            volume_weighted_distance: Decimal::ZERO,
            best_level_ratio: Decimal::ZERO,
            active_levels,
        };
    }

    let total_dec = Decimal::from(total_vol);
    let top3_vol: i64 = levels.iter().take(3).map(|l| l.volume).sum();
    let top3_volume_ratio = Decimal::from(top3_vol) / total_dec;
    let best_vol = levels.first().map(|l| l.volume).unwrap_or(0);
    let best_level_ratio = Decimal::from(best_vol) / total_dec;

    let volume_weighted_distance = if let Some(bp) = best_price {
        let mut weighted_sum = Decimal::ZERO;
        for l in levels {
            if let Some(price) = l.price {
                let dist = (price - bp).abs();
                weighted_sum += dist * Decimal::from(l.volume);
            }
        }
        weighted_sum / total_dec
    } else {
        Decimal::ZERO
    };

    DepthProfile {
        top3_volume_ratio,
        volume_weighted_distance,
        best_level_ratio,
        active_levels,
    }
}

pub(super) fn compute_market_temperature(raw: &RawSnapshot) -> Option<MarketTemperatureObservation> {
    raw.market_temperature
        .as_ref()
        .map(|temp| MarketTemperatureObservation {
            temperature: Decimal::from(temp.temperature),
            valuation: Decimal::from(temp.valuation),
            sentiment: Decimal::from(temp.sentiment),
            description: temp.description.clone(),
            timestamp: temp.timestamp,
        })
}
