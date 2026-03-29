use super::*;

pub(crate) fn stabilize_cross_market_signals(
    signals: Vec<crate::bridges::hk_to_us::CrossMarketSignal>,
    dims: &UsDimensionSnapshot,
) -> Vec<crate::bridges::hk_to_us::CrossMarketSignal> {
    signals
        .into_iter()
        .map(|mut signal| {
            let Some(target_dims) = dims.dimensions.get(&signal.us_symbol) else {
                return signal;
            };

            let anchor_confidence =
                (signal.hk_composite.abs() * Decimal::new(35, 2)).min(Decimal::ONE);
            let us_response = target_dims
                .price_momentum
                .abs()
                .max(target_dims.capital_flow_direction.abs())
                .max(target_dims.volume_profile.abs());
            let signal_direction = signal.propagation_confidence.signum();
            let us_direction = if target_dims.price_momentum != Decimal::ZERO {
                target_dims.price_momentum.signum()
            } else {
                target_dims.capital_flow_direction.signum()
            };

            let should_hold_anchor = us_response < Decimal::new(15, 2)
                || us_direction == Decimal::ZERO
                || us_direction != signal_direction;

            if should_hold_anchor {
                let magnitude = signal.propagation_confidence.abs().max(anchor_confidence);
                signal.propagation_confidence = signal_direction * magnitude;
            }

            signal
        })
        .collect()
}

pub(crate) fn market_status_from_trade_status(status: TradeStatus) -> MarketStatus {
    #[allow(unreachable_patterns)]
    match status {
        TradeStatus::Normal => MarketStatus::Normal,
        TradeStatus::Halted => MarketStatus::Halted,
        TradeStatus::SuspendTrade => MarketStatus::SuspendTrade,
        TradeStatus::ToBeOpened => MarketStatus::ToBeOpened,
        _ => MarketStatus::Other,
    }
}

pub(crate) fn build_quotes(raw: &HashMap<Symbol, SecurityQuote>) -> Vec<QuoteObservation> {
    raw.iter()
        .filter_map(|(symbol, q)| {
            if q.prev_close == Decimal::ZERO {
                return None;
            }
            Some(QuoteObservation {
                symbol: symbol.clone(),
                last_done: q.last_done,
                prev_close: q.prev_close,
                open: q.open,
                high: q.high,
                low: q.low,
                volume: q.volume,
                turnover: q.turnover,
                timestamp: q.timestamp,
                market_status: market_status_from_trade_status(q.trade_status),
                pre_market: q
                    .pre_market_quote
                    .as_ref()
                    .map(crate::ontology::links::convert_pre_post_quote),
                post_market: q
                    .post_market_quote
                    .as_ref()
                    .map(crate::ontology::links::convert_pre_post_quote),
            })
        })
        .collect()
}

pub(crate) fn build_capital_flows(
    raw: &HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
) -> Vec<CapitalFlow> {
    raw.iter()
        .filter_map(|(symbol, lines)| {
            lines.last().map(|line| CapitalFlow {
                symbol: symbol.clone(),
                net_inflow: YuanAmount::from_ten_thousands(line.inflow),
            })
        })
        .collect()
}

pub(crate) fn build_calc_indexes(
    raw: &HashMap<Symbol, SecurityCalcIndex>,
) -> Vec<CalcIndexObservation> {
    raw.iter()
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

pub(crate) fn build_candlesticks(
    raw: &HashMap<Symbol, Vec<longport::quote::Candlestick>>,
) -> Vec<CandlestickObservation> {
    raw.iter()
        .filter_map(|(symbol, candles)| {
            let latest = candles.last()?;
            let first = candles
                .iter()
                .rev()
                .take(5)
                .last()
                .copied()
                .unwrap_or(*latest);

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

pub(crate) fn us_sector_name(store: &Arc<ObjectStore>, symbol: &Symbol) -> Option<String> {
    store.sector_name_for_symbol(symbol).map(str::to_string)
}
