use std::collections::{HashMap, HashSet};

use crate::core::market_snapshot::CanonicalMarketSnapshot;
use crate::math::{clamp_signed_unit_interval, median, normalized_ratio};
use crate::ontology::links::{
    CalcIndexObservation, CandlestickObservation, CapitalFlow, QuoteObservation,
};
use crate::ontology::objects::Symbol;
use crate::ontology::store::ObjectStore;
use rust_decimal::Decimal;
use time::OffsetDateTime;

// For liquid US large caps, a 5% session move is already a strong trend day.
// We saturate there so normal momentum does not get overshadowed by rare gap/meme moves.
fn price_momentum_normalizer() -> Decimal {
    Decimal::new(5, 2)
}

// Pre/post-market gaps are usually smaller than regular-session trends.
// A 3% overnight move is large enough to count as a full anomaly signal.
fn pre_post_market_anomaly_normalizer() -> Decimal {
    Decimal::new(3, 2)
}

/// Per-symbol US dimension vector. Each value in [-1, +1].
///
/// US markets lack broker queue and 10-level depth from Longport,
/// so we drop order_book_pressure, depth_structure_imbalance,
/// institutional_direction, and capital_size_divergence.
/// We add pre_post_market_anomaly (unique to US extended hours).
#[derive(Debug, Clone, Default)]
pub struct UsSymbolDimensions {
    pub capital_flow_direction: Decimal,  // net inflow / turnover
    pub price_momentum: Decimal,          // last vs prev_close + pre/post delta
    pub volume_profile: Decimal,          // OHLCV conviction from candlesticks
    pub pre_post_market_anomaly: Decimal, // extended-hours price deviation
    pub valuation: Decimal,               // PE/PB/dividend vs peer median
    pub multi_horizon_momentum: Decimal,  // 5d/10d/ytd trend alignment
}

/// Market-wide US dimension snapshot.
#[derive(Debug)]
pub struct UsDimensionSnapshot {
    pub timestamp: OffsetDateTime,
    pub dimensions: HashMap<Symbol, UsSymbolDimensions>,
}

impl UsDimensionSnapshot {
    pub fn compute_from_canonical(snapshot: &CanonicalMarketSnapshot, store: &ObjectStore) -> Self {
        let quotes = snapshot
            .quotes
            .iter()
            .filter_map(|(symbol, quote)| {
                if quote.prev_close == Decimal::ZERO {
                    return None;
                }
                Some(QuoteObservation {
                    symbol: symbol.clone(),
                    last_done: quote.last_done,
                    prev_close: quote.prev_close,
                    open: quote.open,
                    high: quote.high,
                    low: quote.low,
                    volume: quote.volume,
                    turnover: quote.turnover,
                    timestamp: quote.timestamp,
                    market_status: match quote.market_status {
                        crate::core::market_snapshot::CanonicalMarketStatus::Normal => {
                            crate::ontology::links::MarketStatus::Normal
                        }
                        crate::core::market_snapshot::CanonicalMarketStatus::Halted => {
                            crate::ontology::links::MarketStatus::Halted
                        }
                        crate::core::market_snapshot::CanonicalMarketStatus::SuspendTrade => {
                            crate::ontology::links::MarketStatus::SuspendTrade
                        }
                        crate::core::market_snapshot::CanonicalMarketStatus::ToBeOpened => {
                            crate::ontology::links::MarketStatus::ToBeOpened
                        }
                        crate::core::market_snapshot::CanonicalMarketStatus::Other => {
                            crate::ontology::links::MarketStatus::Other
                        }
                    },
                    pre_market: quote.pre_market.as_ref().map(|session| {
                        crate::ontology::links::ExtendedSessionQuote {
                            last_done: session.last_done,
                            timestamp: session.timestamp,
                            volume: session.volume,
                            turnover: session.turnover,
                            high: session.high,
                            low: session.low,
                            prev_close: session.prev_close,
                        }
                    }),
                    post_market: quote.post_market.as_ref().map(|session| {
                        crate::ontology::links::ExtendedSessionQuote {
                            last_done: session.last_done,
                            timestamp: session.timestamp,
                            volume: session.volume,
                            turnover: session.turnover,
                            high: session.high,
                            low: session.low,
                            prev_close: session.prev_close,
                        }
                    }),
                })
            })
            .collect::<Vec<_>>();
        let capital_flows = snapshot
            .capital_flow_series
            .iter()
            .filter_map(|(symbol, lines)| {
                lines.last().map(|line| CapitalFlow {
                    symbol: symbol.clone(),
                    net_inflow: crate::ontology::links::YuanAmount::from_ten_thousands(line.inflow),
                })
            })
            .collect::<Vec<_>>();
        let calc_indexes = snapshot
            .calc_indexes
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
                ytd_change_rate: idx.ytd_change_rate,
                five_day_change_rate: idx.five_day_change_rate,
                ten_day_change_rate: idx.ten_day_change_rate,
                half_year_change_rate: idx.half_year_change_rate,
                total_market_value: idx.total_market_value,
                capital_flow: idx.capital_flow,
                change_rate: idx.change_rate,
            })
            .collect::<Vec<_>>();
        let candlesticks = snapshot
            .candlesticks
            .iter()
            .filter_map(|(symbol, candles)| {
                let latest = candles.last()?;
                let first = candles
                    .iter()
                    .rev()
                    .take(5)
                    .last()
                    .cloned()
                    .unwrap_or_else(|| latest.clone());
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
                        (window_high - window_low) / first.open / Decimal::new(8, 2),
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
            .collect::<Vec<_>>();
        let intraday = snapshot
            .intraday
            .iter()
            .filter_map(|(symbol, lines)| {
                let last = lines.last()?;
                if last.avg_price <= Decimal::ZERO {
                    return None;
                }
                Some(crate::ontology::links::IntradayObservation {
                    symbol: symbol.clone(),
                    avg_price: last.avg_price,
                    last_price: last.price,
                    vwap_deviation: (last.price - last.avg_price) / last.avg_price,
                    point_count: lines.len(),
                })
            })
            .collect::<Vec<_>>();
        Self::compute_with_intraday(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            store,
            snapshot.timestamp,
            &intraday,
        )
    }

    /// Pure synchronous function -- compute all US dimensions from available data.
    pub fn compute(
        quotes: &[QuoteObservation],
        capital_flows: &[CapitalFlow],
        calc_indexes: &[CalcIndexObservation],
        candlesticks: &[CandlestickObservation],
        store: &ObjectStore,
        timestamp: OffsetDateTime,
    ) -> Self {
        Self::compute_with_intraday(
            quotes,
            capital_flows,
            calc_indexes,
            candlesticks,
            store,
            timestamp,
            &[],
        )
    }

    /// Compute dimensions with optional intraday VWAP data.
    pub fn compute_with_intraday(
        quotes: &[QuoteObservation],
        capital_flows: &[CapitalFlow],
        calc_indexes: &[CalcIndexObservation],
        candlesticks: &[CandlestickObservation],
        store: &ObjectStore,
        timestamp: OffsetDateTime,
        intraday: &[crate::ontology::links::IntradayObservation],
    ) -> Self {
        let mut flow_dir = compute_capital_flow_direction(quotes, capital_flows);
        for (symbol, fallback) in compute_capital_flow_direction_from_indexes(quotes, calc_indexes)
        {
            flow_dir.entry(symbol).or_insert(fallback);
        }
        let mut momentum = compute_price_momentum(quotes);
        // Apply VWAP confirmation: if price deviates from VWAP in same direction as momentum, boost
        for obs in intraday {
            if let Some(mom) = momentum.get_mut(&obs.symbol) {
                let vwap_factor = if (*mom > Decimal::ZERO) == (obs.vwap_deviation > Decimal::ZERO)
                {
                    Decimal::ONE + obs.vwap_deviation.abs().min(Decimal::new(3, 1))
                } else {
                    Decimal::ONE - obs.vwap_deviation.abs().min(Decimal::new(2, 1))
                };
                *mom = clamp_signed_unit_interval(*mom * vwap_factor);
            }
        }
        let volume = compute_volume_profile(candlesticks);
        let prepost = compute_pre_post_market_anomaly(quotes);
        let val = compute_valuation(calc_indexes, quotes, store);
        let multi_horizon = compute_us_multi_horizon_momentum(calc_indexes);

        let mut all_symbols: HashSet<Symbol> = HashSet::new();
        for s in flow_dir.keys() {
            all_symbols.insert(s.clone());
        }
        for s in momentum.keys() {
            all_symbols.insert(s.clone());
        }
        for s in volume.keys() {
            all_symbols.insert(s.clone());
        }
        for s in prepost.keys() {
            all_symbols.insert(s.clone());
        }
        for s in val.keys() {
            all_symbols.insert(s.clone());
        }
        for s in multi_horizon.keys() {
            all_symbols.insert(s.clone());
        }

        let zero = Decimal::ZERO;
        let dimensions = all_symbols
            .into_iter()
            .map(|sym| {
                let dims = UsSymbolDimensions {
                    capital_flow_direction: flow_dir.get(&sym).copied().unwrap_or(zero),
                    price_momentum: momentum.get(&sym).copied().unwrap_or(zero),
                    volume_profile: volume.get(&sym).copied().unwrap_or(zero),
                    pre_post_market_anomaly: prepost.get(&sym).copied().unwrap_or(zero),
                    valuation: val.get(&sym).copied().unwrap_or(zero),
                    multi_horizon_momentum: multi_horizon.get(&sym).copied().unwrap_or(zero),
                };
                (sym, dims)
            })
            .collect();

        UsDimensionSnapshot {
            timestamp,
            dimensions,
        }
    }
}

// ── Helpers ──

fn positive_part(value: Decimal) -> Decimal {
    if value > Decimal::ZERO {
        value
    } else {
        Decimal::ZERO
    }
}

fn average(values: impl IntoIterator<Item = Decimal>) -> Decimal {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        Decimal::ZERO
    } else {
        values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
    }
}

// ── Dimension 1: Capital Flow Direction ──
// net_inflow / turnover, clamped to [-1, +1].
// Identical logic to HK but sourced from US capital_flow API.

fn compute_capital_flow_direction(
    quotes: &[QuoteObservation],
    flows: &[CapitalFlow],
) -> HashMap<Symbol, Decimal> {
    let turnover: HashMap<&Symbol, Decimal> =
        quotes.iter().map(|q| (&q.symbol, q.turnover)).collect();

    flows
        .iter()
        .map(|cf| {
            let t = turnover.get(&cf.symbol).copied().unwrap_or(Decimal::ZERO);
            if t == Decimal::ZERO {
                (cf.symbol.clone(), Decimal::ZERO)
            } else {
                let ratio = cf.net_inflow.as_yuan() / t;
                (cf.symbol.clone(), clamp_signed_unit_interval(ratio))
            }
        })
        .collect()
}

fn compute_capital_flow_direction_from_indexes(
    quotes: &[QuoteObservation],
    calc_indexes: &[CalcIndexObservation],
) -> HashMap<Symbol, Decimal> {
    let turnover: HashMap<&Symbol, Decimal> =
        quotes.iter().map(|q| (&q.symbol, q.turnover)).collect();

    calc_indexes
        .iter()
        .filter_map(|idx| {
            let turnover = turnover.get(&idx.symbol).copied().unwrap_or(Decimal::ZERO);
            let capital_flow = idx.capital_flow?;
            if turnover == Decimal::ZERO {
                return Some((idx.symbol.clone(), Decimal::ZERO));
            }
            Some((
                idx.symbol.clone(),
                clamp_signed_unit_interval(capital_flow / turnover),
            ))
        })
        .collect()
}

// ── Dimension 2: Price Momentum ──
// Combines intraday return (last vs prev_close) with pre/post market signals.
// For US stocks, Longport provides pre_market_quote and post_market_quote on SecurityQuote.
// We use the last_done vs prev_close change rate, normalized by a 5% cap.

fn compute_price_momentum(quotes: &[QuoteObservation]) -> HashMap<Symbol, Decimal> {
    quotes
        .iter()
        .filter_map(|q| {
            if q.prev_close == Decimal::ZERO {
                return Some((q.symbol.clone(), Decimal::ZERO));
            }
            let change_rate = (q.last_done - q.prev_close) / q.prev_close;
            // Normalize: 5% move = full signal
            let normalized = clamp_signed_unit_interval(change_rate / price_momentum_normalizer());
            Some((q.symbol.clone(), normalized))
        })
        .collect()
}

// ── Dimension 3: Volume Profile (Candlestick Conviction) ──
// Same logic as HK candlestick_conviction: directional * (1 + confirmation) / 2.

fn compute_volume_profile(candlesticks: &[CandlestickObservation]) -> HashMap<Symbol, Decimal> {
    candlesticks
        .iter()
        .map(|candle| {
            let directional = average([candle.window_return, candle.body_bias]);
            let volume_confirmation = positive_part(clamp_signed_unit_interval(
                (candle.volume_ratio - Decimal::ONE) / Decimal::TWO,
            ));
            let confirmation = average([volume_confirmation, candle.range_ratio]);
            let value = directional * ((Decimal::ONE + confirmation) / Decimal::TWO);
            (candle.symbol.clone(), clamp_signed_unit_interval(value))
        })
        .collect()
}

// ── Dimension 4: Pre/Post Market Anomaly ──
// Detects institutional pre-market positioning.
// Signal = (open - prev_close) / prev_close, normalized by 3% cap.
// The gap between prev_close and open reflects overnight/pre-market institutional activity.

fn compute_pre_post_market_anomaly(quotes: &[QuoteObservation]) -> HashMap<Symbol, Decimal> {
    quotes
        .iter()
        .filter_map(|q| {
            if q.prev_close == Decimal::ZERO || q.open == Decimal::ZERO {
                return Some((q.symbol.clone(), Decimal::ZERO));
            }
            let gap = (q.open - q.prev_close) / q.prev_close;
            // 3% gap = full signal (institutional moves happen in pre-market)
            let normalized = clamp_signed_unit_interval(gap / pre_post_market_anomaly_normalizer());
            Some((q.symbol.clone(), normalized))
        })
        .collect()
}

// ── Dimension 6: Multi-horizon momentum ──

fn compute_us_multi_horizon_momentum(
    calc_indexes: &[CalcIndexObservation],
) -> HashMap<Symbol, Decimal> {
    calc_indexes
        .iter()
        .filter_map(|idx| {
            let five_d = idx.five_day_change_rate?;
            let ten_d = idx.ten_day_change_rate.unwrap_or(five_d);
            let ytd = idx.ytd_change_rate.unwrap_or(Decimal::ZERO);

            let short = clamp_signed_unit_interval(five_d / Decimal::new(10, 2));
            let mid = clamp_signed_unit_interval(ten_d / Decimal::new(15, 2));
            let long = clamp_signed_unit_interval(ytd / Decimal::new(30, 2));

            let aligned =
                short * Decimal::new(5, 1) + mid * Decimal::new(3, 1) + long * Decimal::new(2, 1);
            Some((idx.symbol.clone(), clamp_signed_unit_interval(aligned)))
        })
        .collect()
}

// ── Dimension 5: Valuation ──
// PE/PB/dividend compared to cross-sectional median.
// Lower PE/PB = positive, higher dividend = positive.

fn compute_valuation(
    calc_indexes: &[CalcIndexObservation],
    quotes: &[QuoteObservation],
    store: &ObjectStore,
) -> HashMap<Symbol, Decimal> {
    let last_prices: HashMap<&Symbol, Decimal> =
        quotes.iter().map(|q| (&q.symbol, q.last_done)).collect();
    let calc_lookup: HashMap<&Symbol, _> =
        calc_indexes.iter().map(|idx| (&idx.symbol, idx)).collect();

    let mut pe_values = Vec::new();
    let mut pb_values = Vec::new();
    let mut dividend_values = Vec::new();
    let mut derived: HashMap<Symbol, (Option<Decimal>, Option<Decimal>, Option<Decimal>)> =
        HashMap::new();

    for (symbol, stock) in &store.stocks {
        let idx = calc_lookup.get(symbol);
        let price = last_prices.get(symbol).copied().unwrap_or(Decimal::ZERO);

        let pe = idx
            .and_then(|idx| idx.pe_ttm_ratio)
            .or_else(|| {
                if stock.eps_ttm > Decimal::ZERO && price > Decimal::ZERO {
                    Some(price / stock.eps_ttm)
                } else {
                    None
                }
            })
            .filter(|value| *value > Decimal::ZERO);

        let pb = idx
            .and_then(|idx| idx.pb_ratio)
            .or_else(|| {
                if stock.bps > Decimal::ZERO && price > Decimal::ZERO {
                    Some(price / stock.bps)
                } else {
                    None
                }
            })
            .filter(|value| *value > Decimal::ZERO);

        let dividend = idx
            .and_then(|idx| idx.dividend_ratio_ttm)
            .or_else(|| {
                if stock.dividend_yield > Decimal::ZERO {
                    Some(stock.dividend_yield)
                } else {
                    None
                }
            })
            .filter(|value| *value > Decimal::ZERO);

        if let Some(value) = pe {
            pe_values.push(value);
        }
        if let Some(value) = pb {
            pb_values.push(value);
        }
        if let Some(value) = dividend {
            dividend_values.push(value);
        }

        derived.insert(symbol.clone(), (pe, pb, dividend));
    }

    let pe_median = median(pe_values);
    let pb_median = median(pb_values);
    let dividend_median = median(dividend_values);

    derived
        .into_iter()
        .filter_map(|(symbol, (pe, pb, dividend))| {
            let mut components = Vec::new();

            // Lower PE = positive (cheaper)
            if let (Some(med), Some(value)) = (pe_median, pe) {
                components.push(normalized_ratio(med, value));
            }
            // Lower PB = positive (cheaper)
            if let (Some(med), Some(value)) = (pb_median, pb) {
                components.push(normalized_ratio(med, value));
            }
            // Higher dividend = positive
            if let (Some(med), Some(value)) = (dividend_median, dividend) {
                components.push(normalized_ratio(value, med));
            }

            if components.is_empty() {
                None
            } else {
                Some((symbol, average(components)))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::links::MarketStatus;
    use crate::ontology::objects::Stock;
    use crate::ontology::store::ObjectStore;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_quote(
        symbol: &str,
        last_done: Decimal,
        prev_close: Decimal,
        open: Decimal,
    ) -> QuoteObservation {
        QuoteObservation {
            symbol: sym(symbol),
            last_done,
            prev_close,
            open,
            high: last_done + dec!(1),
            low: prev_close - dec!(1),
            volume: 1_000_000,
            turnover: dec!(10000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        }
    }

    fn make_stock(symbol: &str, eps_ttm: Decimal, bps: Decimal, dividend_yield: Decimal) -> Stock {
        let symbol_id = sym(symbol);
        Stock {
            market: symbol_id.market(),
            symbol: symbol_id,
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "NASDAQ".into(),
            lot_size: 1,
            sector_id: None,
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm,
            bps,
            dividend_yield,
        }
    }

    fn make_store(stocks: Vec<Stock>) -> ObjectStore {
        let stock_map = stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: stock_map,
            sectors: HashMap::new(),
            broker_to_institution: HashMap::new(),
            knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
        }
    }

    // ── capital_flow_direction ──

    #[test]
    fn capital_flow_inflow() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(1000)),
        }];
        let result = compute_capital_flow_direction(&quotes, &flows);
        // 1000 / 10000 = 0.1
        assert_eq!(result[&sym("AAPL.US")], dec!(0.1));
    }

    #[test]
    fn capital_flow_outflow() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(-5000)),
        }];
        let result = compute_capital_flow_direction(&quotes, &flows);
        assert_eq!(result[&sym("AAPL.US")], dec!(-0.5));
    }

    #[test]
    fn capital_flow_clamped() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(99999)),
        }];
        let result = compute_capital_flow_direction(&quotes, &flows);
        assert_eq!(result[&sym("AAPL.US")], dec!(1));
    }

    #[test]
    fn capital_flow_zero_turnover() {
        let mut quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        quotes[0].turnover = dec!(0);
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(100)),
        }];
        let result = compute_capital_flow_direction(&quotes, &flows);
        assert_eq!(result[&sym("AAPL.US")], dec!(0));
    }

    #[test]
    fn capital_flow_falls_back_to_calc_index_when_series_missing() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let calc_indexes = vec![CalcIndexObservation {
            symbol: sym("AAPL.US"),
            turnover_rate: None,
            volume_ratio: None,
            pe_ttm_ratio: None,
            pb_ratio: None,
            dividend_ratio_ttm: None,
            amplitude: None,
            five_minutes_change_rate: None,
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: Some(dec!(1000)),
            change_rate: None,
        }];
        let result = compute_capital_flow_direction_from_indexes(&quotes, &calc_indexes);
        assert_eq!(result[&sym("AAPL.US")], dec!(0.1));
    }

    // ── price_momentum ──

    #[test]
    fn momentum_positive() {
        // 2% gain: 178 -> 181.56  (181.56-178)/178 = 0.02 => 0.02/0.05 = 0.4
        let quotes = vec![make_quote("NVDA.US", dec!(181.56), dec!(178), dec!(179))];
        let result = compute_price_momentum(&quotes);
        let v = result[&sym("NVDA.US")];
        assert!(v > dec!(0.3) && v < dec!(0.5));
    }

    #[test]
    fn momentum_negative() {
        // -3% drop
        let quotes = vec![make_quote("NVDA.US", dec!(172.66), dec!(178), dec!(177))];
        let result = compute_price_momentum(&quotes);
        let v = result[&sym("NVDA.US")];
        assert!(v < dec!(-0.5));
    }

    #[test]
    fn momentum_zero_prev_close() {
        let quotes = vec![make_quote("NVDA.US", dec!(100), dec!(0), dec!(100))];
        let result = compute_price_momentum(&quotes);
        assert_eq!(result[&sym("NVDA.US")], dec!(0));
    }

    #[test]
    fn momentum_clamped_at_limit() {
        // 10% move should clamp to 1.0
        let quotes = vec![make_quote("NVDA.US", dec!(198), dec!(180), dec!(181))];
        let result = compute_price_momentum(&quotes);
        assert_eq!(result[&sym("NVDA.US")], dec!(1));
    }

    // ── volume_profile ──

    #[test]
    fn volume_profile_bullish() {
        let candles = vec![CandlestickObservation {
            symbol: sym("TSLA.US"),
            candle_count: 5,
            window_return: dec!(0.5),
            body_bias: dec!(0.6),
            volume_ratio: dec!(2.5),
            range_ratio: dec!(0.4),
        }];
        let result = compute_volume_profile(&candles);
        assert!(result[&sym("TSLA.US")] > dec!(0.3));
    }

    #[test]
    fn volume_profile_bearish() {
        let candles = vec![CandlestickObservation {
            symbol: sym("TSLA.US"),
            candle_count: 5,
            window_return: dec!(-0.5),
            body_bias: dec!(-0.6),
            volume_ratio: dec!(2.0),
            range_ratio: dec!(0.3),
        }];
        let result = compute_volume_profile(&candles);
        assert!(result[&sym("TSLA.US")] < dec!(-0.2));
    }

    #[test]
    fn volume_profile_empty() {
        let result = compute_volume_profile(&[]);
        assert!(result.is_empty());
    }

    // ── pre_post_market_anomaly ──

    #[test]
    fn prepost_gap_up() {
        // prev_close=100, open=102 => 2% gap => 0.02/0.03 = 0.666...
        let quotes = vec![make_quote("BABA.US", dec!(103), dec!(100), dec!(102))];
        let result = compute_pre_post_market_anomaly(&quotes);
        let v = result[&sym("BABA.US")];
        assert!(v > dec!(0.6) && v < dec!(0.7));
    }

    #[test]
    fn prepost_gap_down() {
        // prev_close=100, open=97 => -3% gap => clamped to -1
        let quotes = vec![make_quote("BABA.US", dec!(96), dec!(100), dec!(97))];
        let result = compute_pre_post_market_anomaly(&quotes);
        assert_eq!(result[&sym("BABA.US")], dec!(-1));
    }

    #[test]
    fn prepost_no_gap() {
        let quotes = vec![make_quote("BABA.US", dec!(101), dec!(100), dec!(100))];
        let result = compute_pre_post_market_anomaly(&quotes);
        assert_eq!(result[&sym("BABA.US")], dec!(0));
    }

    #[test]
    fn prepost_zero_prev_close() {
        let quotes = vec![make_quote("BABA.US", dec!(100), dec!(0), dec!(100))];
        let result = compute_pre_post_market_anomaly(&quotes);
        assert_eq!(result[&sym("BABA.US")], dec!(0));
    }

    // ── valuation ──

    #[test]
    fn valuation_cheaper_stock_scores_positive() {
        let quotes = vec![
            make_quote("AAPL.US", dec!(100), dec!(100), dec!(100)),
            make_quote("MSFT.US", dec!(50), dec!(50), dec!(50)),
        ];
        let calc_indexes = vec![
            CalcIndexObservation {
                symbol: sym("AAPL.US"),
                turnover_rate: None,
                volume_ratio: None,
                pe_ttm_ratio: Some(dec!(30)),
                pb_ratio: Some(dec!(5)),
                dividend_ratio_ttm: Some(dec!(0.01)),
                amplitude: None,
                five_minutes_change_rate: None,
                ytd_change_rate: None,
                five_day_change_rate: None,
                ten_day_change_rate: None,
                half_year_change_rate: None,
                total_market_value: None,
                capital_flow: None,
                change_rate: None,
            },
            CalcIndexObservation {
                symbol: sym("MSFT.US"),
                turnover_rate: None,
                volume_ratio: None,
                pe_ttm_ratio: Some(dec!(15)),
                pb_ratio: Some(dec!(2)),
                dividend_ratio_ttm: Some(dec!(0.03)),
                amplitude: None,
                five_minutes_change_rate: None,
                ytd_change_rate: None,
                five_day_change_rate: None,
                ten_day_change_rate: None,
                half_year_change_rate: None,
                total_market_value: None,
                capital_flow: None,
                change_rate: None,
            },
        ];
        let store = make_store(vec![
            make_stock("AAPL.US", dec!(3.33), dec!(20), dec!(0.01)),
            make_stock("MSFT.US", dec!(3.33), dec!(25), dec!(0.03)),
        ]);

        let result = compute_valuation(&calc_indexes, &quotes, &store);
        // MSFT has lower PE, lower PB, higher dividend => should score positive
        assert!(result[&sym("MSFT.US")] > Decimal::ZERO);
        // AAPL is the expensive one => negative
        assert!(result[&sym("AAPL.US")] < Decimal::ZERO);
    }

    #[test]
    fn valuation_fallback_to_static_info() {
        let quotes = vec![make_quote("AAPL.US", dec!(100), dec!(100), dec!(100))];
        // No calc_indexes at all -- should fall back to stock.eps_ttm / bps
        let store = make_store(vec![make_stock("AAPL.US", dec!(5), dec!(20), dec!(0.02))]);
        let result = compute_valuation(&[], &quotes, &store);
        // Single stock: with only one data point, median = itself, so score = 0
        assert!(result.contains_key(&sym("AAPL.US")));
    }

    #[test]
    fn valuation_empty() {
        let result = compute_valuation(&[], &[], &make_store(vec![]));
        assert!(result.is_empty());
    }

    // ── full snapshot ──

    #[test]
    fn full_snapshot_assembles_all_dimensions() {
        let quotes = vec![make_quote("NVDA.US", dec!(120), dec!(100), dec!(105))];
        let flows = vec![CapitalFlow {
            symbol: sym("NVDA.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(500)),
        }];
        let calc_indexes = vec![CalcIndexObservation {
            symbol: sym("NVDA.US"),
            turnover_rate: None,
            volume_ratio: None,
            pe_ttm_ratio: Some(dec!(25)),
            pb_ratio: Some(dec!(8)),
            dividend_ratio_ttm: Some(dec!(0.005)),
            amplitude: None,
            five_minutes_change_rate: None,
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: None,
            change_rate: None,
        }];
        let candles = vec![CandlestickObservation {
            symbol: sym("NVDA.US"),
            candle_count: 5,
            window_return: dec!(0.3),
            body_bias: dec!(0.4),
            volume_ratio: dec!(1.5),
            range_ratio: dec!(0.2),
        }];
        let store = make_store(vec![make_stock(
            "NVDA.US",
            dec!(4.8),
            dec!(15),
            dec!(0.005),
        )]);

        let snap = UsDimensionSnapshot::compute(
            &quotes,
            &flows,
            &calc_indexes,
            &candles,
            &store,
            OffsetDateTime::UNIX_EPOCH,
        );

        let dims = &snap.dimensions[&sym("NVDA.US")];
        // capital_flow: 500/10000 = 0.05
        assert_eq!(dims.capital_flow_direction, dec!(0.05));
        // momentum: (120-100)/100 = 20% => clamped to 1.0
        assert_eq!(dims.price_momentum, dec!(1));
        // pre_post: (105-100)/100 = 5% / 3% = 1.666 => clamped to 1.0
        assert_eq!(dims.pre_post_market_anomaly, dec!(1));
        // volume_profile: positive (bullish candles)
        assert!(dims.volume_profile > dec!(0));
    }
}
