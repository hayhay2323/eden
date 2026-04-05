use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::Symbol;
use crate::ontology::store::ObjectStore;

/// Per-symbol dimension vector. Each value in [-1, +1].
#[derive(Debug, Clone, Default)]
pub struct SymbolDimensions {
    pub order_book_pressure: Decimal,       // bid-dominant = positive
    pub capital_flow_direction: Decimal,    // net inflow = positive
    pub capital_size_divergence: Decimal,   // large-order-dominant = positive
    pub institutional_direction: Decimal,   // institutional bid-bias = positive
    pub depth_structure_imbalance: Decimal, // bid wall concentration > ask = positive
    pub valuation_support: Decimal,         // cheaper / higher-yielding than peers = positive
    pub activity_momentum: Decimal,         // active tape + positive short-term move = positive
    pub candlestick_conviction: Decimal,    // recent OHLCV confirms upside = positive
    pub multi_horizon_momentum: Decimal,    // 5d/10d/ytd trend alignment = positive
}

/// Market-wide dimension snapshot.
#[derive(Debug)]
pub struct DimensionSnapshot {
    pub timestamp: OffsetDateTime,
    pub dimensions: HashMap<Symbol, SymbolDimensions>,
}

impl DimensionSnapshot {
    /// Pure synchronous function — compute all dimensions from a LinkSnapshot.
    pub fn compute(links: &LinkSnapshot, store: &ObjectStore) -> Self {
        let book_pressure = compute_order_book_pressure(links);
        let flow_direction = compute_capital_flow_direction(links);
        let size_divergence = compute_capital_size_divergence(links);
        let inst_direction = compute_institutional_direction(links);
        let depth_structure = compute_depth_structure_imbalance(links);
        let valuation_support = compute_valuation_support(links, store);
        let activity_momentum = compute_activity_momentum(links);
        let candlestick_conviction = compute_candlestick_conviction(links);
        let multi_horizon = compute_multi_horizon_momentum(links);

        let mut all_symbols: HashSet<Symbol> = HashSet::new();
        for s in book_pressure.keys() {
            all_symbols.insert(s.clone());
        }
        for s in flow_direction.keys() {
            all_symbols.insert(s.clone());
        }
        for s in size_divergence.keys() {
            all_symbols.insert(s.clone());
        }
        for s in inst_direction.keys() {
            all_symbols.insert(s.clone());
        }
        for s in depth_structure.keys() {
            all_symbols.insert(s.clone());
        }
        for s in valuation_support.keys() {
            all_symbols.insert(s.clone());
        }
        for s in activity_momentum.keys() {
            all_symbols.insert(s.clone());
        }
        for s in candlestick_conviction.keys() {
            all_symbols.insert(s.clone());
        }
        for s in multi_horizon.keys() {
            all_symbols.insert(s.clone());
        }

        let zero = Decimal::ZERO;
        let dimensions = all_symbols
            .into_iter()
            .map(|sym| {
                let dims = SymbolDimensions {
                    order_book_pressure: book_pressure.get(&sym).copied().unwrap_or(zero),
                    capital_flow_direction: flow_direction.get(&sym).copied().unwrap_or(zero),
                    capital_size_divergence: size_divergence.get(&sym).copied().unwrap_or(zero),
                    institutional_direction: inst_direction.get(&sym).copied().unwrap_or(zero),
                    depth_structure_imbalance: depth_structure.get(&sym).copied().unwrap_or(zero),
                    valuation_support: valuation_support.get(&sym).copied().unwrap_or(zero),
                    activity_momentum: activity_momentum.get(&sym).copied().unwrap_or(zero),
                    candlestick_conviction: candlestick_conviction
                        .get(&sym)
                        .copied()
                        .unwrap_or(zero),
                    multi_horizon_momentum: multi_horizon.get(&sym).copied().unwrap_or(zero),
                };
                (sym, dims)
            })
            .collect();

        DimensionSnapshot {
            timestamp: links.timestamp,
            dimensions,
        }
    }
}

use crate::math::{clamp_signed_unit_interval, median, normalized_ratio};

// A 3% five-minute move is already an unusually large intraday impulse for HK names,
// so we treat it as "full" directional activity strength.
fn activity_return_normalizer() -> Decimal {
    Decimal::new(3, 2)
}

// 8% turnover-rate or amplitude readings are already extreme on this watchlist.
// Past that point we cap the boost instead of letting a few tape outliers dominate.
fn activity_boost_normalizer() -> Decimal {
    Decimal::new(8, 2)
}

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

fn compute_order_book_pressure(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .order_books
        .iter()
        .map(|ob| {
            let bid = Decimal::from(ob.total_bid_volume);
            let ask = Decimal::from(ob.total_ask_volume);
            (ob.symbol.clone(), normalized_ratio(bid, ask))
        })
        .collect()
}

/// Depth structure imbalance: compares bid vs ask wall concentration.
/// Bid wall stronger than ask wall → positive (bullish structural support).
/// Uses top3_volume_ratio and best_level_ratio as wall indicators.
fn compute_depth_structure_imbalance(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .order_books
        .iter()
        .map(|ob| {
            // Wall strength = average of top3 concentration and best-level concentration
            let bid_wall =
                (ob.bid_profile.top3_volume_ratio + ob.bid_profile.best_level_ratio) / Decimal::TWO;
            let ask_wall =
                (ob.ask_profile.top3_volume_ratio + ob.ask_profile.best_level_ratio) / Decimal::TWO;
            (ob.symbol.clone(), normalized_ratio(bid_wall, ask_wall))
        })
        .collect()
}

fn compute_capital_flow_direction(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    // Build turnover lookup from quotes.
    let turnover: HashMap<&Symbol, Decimal> = links
        .quotes
        .iter()
        .map(|q| (&q.symbol, q.turnover))
        .collect();

    links
        .capital_flows
        .iter()
        .filter_map(|cf| {
            let t = turnover.get(&cf.symbol).copied().unwrap_or(Decimal::ZERO);
            if t == Decimal::ZERO {
                return Some((cf.symbol.clone(), Decimal::ZERO));
            }
            let ratio = cf.net_inflow.as_yuan() / t;
            // Clamp to [-1, +1]
            let one = Decimal::ONE;
            let clamped = if ratio > one {
                one
            } else if ratio < -one {
                -one
            } else {
                ratio
            };
            Some((cf.symbol.clone(), clamped))
        })
        .collect()
}

fn compute_capital_size_divergence(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .capital_breakdowns
        .iter()
        .map(|cb| {
            let abs_large = cb.large_net.abs();
            let abs_medium = cb.medium_net.abs();
            let abs_small = cb.small_net.abs();
            let denom = abs_large + abs_medium + abs_small;
            let value = if denom == Decimal::ZERO {
                Decimal::ZERO
            } else {
                cb.large_net / denom
            };
            (cb.symbol.clone(), value)
        })
        .collect()
}

fn compute_institutional_direction(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    // Aggregate bid_positions count vs ask_positions count per symbol across all institutions.
    let mut bid_seats: HashMap<Symbol, i64> = HashMap::new();
    let mut ask_seats: HashMap<Symbol, i64> = HashMap::new();

    for act in &links.institution_activities {
        *bid_seats.entry(act.symbol.clone()).or_default() += act.bid_positions.len() as i64;
        *ask_seats.entry(act.symbol.clone()).or_default() += act.ask_positions.len() as i64;
    }

    let mut all_symbols: HashSet<Symbol> = HashSet::new();
    for s in bid_seats.keys() {
        all_symbols.insert(s.clone());
    }
    for s in ask_seats.keys() {
        all_symbols.insert(s.clone());
    }

    all_symbols
        .into_iter()
        .map(|sym| {
            let b = Decimal::from(*bid_seats.get(&sym).unwrap_or(&0));
            let a = Decimal::from(*ask_seats.get(&sym).unwrap_or(&0));
            (sym, normalized_ratio(b, a))
        })
        .collect()
}

fn compute_valuation_support(
    links: &LinkSnapshot,
    store: &ObjectStore,
) -> HashMap<Symbol, Decimal> {
    let last_prices: HashMap<&Symbol, Decimal> = links
        .quotes
        .iter()
        .map(|q| (&q.symbol, q.last_done))
        .collect();
    let calc_lookup: HashMap<&Symbol, _> = links
        .calc_indexes
        .iter()
        .map(|idx| (&idx.symbol, idx))
        .collect();

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

            if let (Some(median), Some(value)) = (pe_median, pe) {
                components.push(normalized_ratio(median, value));
            }
            if let (Some(median), Some(value)) = (pb_median, pb) {
                components.push(normalized_ratio(median, value));
            }
            if let (Some(median), Some(value)) = (dividend_median, dividend) {
                components.push(normalized_ratio(value, median));
            }

            if components.is_empty() {
                None
            } else {
                Some((symbol, average(components)))
            }
        })
        .collect()
}

fn compute_activity_momentum(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .calc_indexes
        .iter()
        .map(|idx| {
            let direction = idx
                .five_minutes_change_rate
                .map(|value| clamp_signed_unit_interval(value / activity_return_normalizer()))
                .unwrap_or(Decimal::ZERO);
            let volume_boost = idx
                .volume_ratio
                .map(|value| {
                    positive_part(clamp_signed_unit_interval(
                        (value - Decimal::ONE) / Decimal::TWO,
                    ))
                })
                .unwrap_or(Decimal::ZERO);
            let turnover_boost = idx
                .turnover_rate
                .map(|value| {
                    positive_part(clamp_signed_unit_interval(
                        value / activity_boost_normalizer(),
                    ))
                })
                .unwrap_or(Decimal::ZERO);
            let amplitude_boost = idx
                .amplitude
                .map(|value| {
                    positive_part(clamp_signed_unit_interval(
                        value / activity_boost_normalizer(),
                    ))
                })
                .unwrap_or(Decimal::ZERO);

            // VWAP confirmation: price above/below avg_price strengthens directional signal
            let vwap_boost = links
                .intraday
                .iter()
                .find(|obs| obs.symbol == idx.symbol)
                .map(|obs| clamp_signed_unit_interval(obs.vwap_deviation * Decimal::from(5)))
                .unwrap_or(Decimal::ZERO);

            let activity_level = average([volume_boost, turnover_boost, amplitude_boost]);
            // VWAP confirms direction: if both point same way, boost; if they conflict, dampen
            let vwap_factor = if (direction > Decimal::ZERO) == (vwap_boost > Decimal::ZERO) {
                Decimal::ONE + vwap_boost.abs() * Decimal::new(3, 1) // up to +30% boost
            } else {
                Decimal::ONE - vwap_boost.abs() * Decimal::new(2, 1) // up to -20% dampening
            };
            let value = direction * ((Decimal::ONE + activity_level) / Decimal::TWO) * vwap_factor;
            (idx.symbol.clone(), clamp_signed_unit_interval(value))
        })
        .collect()
}

fn compute_multi_horizon_momentum(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .calc_indexes
        .iter()
        .filter_map(|idx| {
            let five_d = idx.five_day_change_rate?;
            let ten_d = idx.ten_day_change_rate.unwrap_or(five_d);
            let ytd = idx.ytd_change_rate.unwrap_or(Decimal::ZERO);

            let short = clamp_signed_unit_interval(five_d / Decimal::new(10, 2));
            let mid = clamp_signed_unit_interval(ten_d / Decimal::new(15, 2));
            let long = clamp_signed_unit_interval(ytd / Decimal::new(30, 2));

            let aligned = short * Decimal::new(5, 1)
                + mid * Decimal::new(3, 1)
                + long * Decimal::new(2, 1);
            Some((idx.symbol.clone(), clamp_signed_unit_interval(aligned)))
        })
        .collect()
}

fn compute_candlestick_conviction(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    links
        .candlesticks
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::links::*;
    use crate::ontology::objects::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn empty_links() -> LinkSnapshot {
        LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_flow_series: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![],
            quotes: vec![],
            trade_activities: vec![],
            intraday: vec![],
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
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: None,
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm,
            bps,
            dividend_yield,
        }
    }

    fn make_store(stocks: Vec<Stock>) -> ObjectStore {
        ObjectStore::from_parts(vec![], stocks, vec![])
    }

    // ── normalized_ratio ──

    #[test]
    fn normalized_ratio_basic() {
        assert_eq!(normalized_ratio(dec!(3), dec!(1)), dec!(0.5));
    }

    #[test]
    fn normalized_ratio_zero_denominator() {
        assert_eq!(normalized_ratio(dec!(0), dec!(0)), dec!(0));
    }

    #[test]
    fn normalized_ratio_equal() {
        assert_eq!(normalized_ratio(dec!(5), dec!(5)), dec!(0));
    }

    #[test]
    fn normalized_ratio_negative() {
        assert_eq!(normalized_ratio(dec!(1), dec!(3)), dec!(-0.5));
    }

    // ── order_book_pressure ──

    #[test]
    fn order_book_pressure_bid_dominant() {
        let mut links = empty_links();
        links.order_books.push(OrderBookObservation {
            symbol: sym("700.HK"),
            ask_levels: vec![],
            bid_levels: vec![],
            total_ask_volume: 200,
            total_bid_volume: 800,
            total_ask_orders: 0,
            total_bid_orders: 0,
            spread: None,
            ask_level_count: 0,
            bid_level_count: 0,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        });

        let result = compute_order_book_pressure(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0.6));
    }

    #[test]
    fn order_book_pressure_balanced() {
        let mut links = empty_links();
        links.order_books.push(OrderBookObservation {
            symbol: sym("700.HK"),
            ask_levels: vec![],
            bid_levels: vec![],
            total_ask_volume: 500,
            total_bid_volume: 500,
            total_ask_orders: 0,
            total_bid_orders: 0,
            spread: None,
            ask_level_count: 0,
            bid_level_count: 0,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        });

        let result = compute_order_book_pressure(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0));
    }

    #[test]
    fn order_book_pressure_empty() {
        let links = empty_links();
        let result = compute_order_book_pressure(&links);
        assert!(result.is_empty());
    }

    // ── capital_flow_direction ──

    #[test]
    fn capital_flow_inflow() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(100)),
        });
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(350),
            prev_close: dec!(348),
            open: dec!(349),
            high: dec!(352),
            low: dec!(347),
            volume: 1_000_000,
            turnover: dec!(1000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0.1));
    }

    #[test]
    fn capital_flow_outflow() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(-500)),
        });
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(350),
            prev_close: dec!(348),
            open: dec!(349),
            high: dec!(352),
            low: dec!(347),
            volume: 1_000_000,
            turnover: dec!(1000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(-0.5));
    }

    #[test]
    fn capital_flow_clamp() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(2000)),
        });
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(350),
            prev_close: dec!(348),
            open: dec!(349),
            high: dec!(352),
            low: dec!(347),
            volume: 1_000_000,
            turnover: dec!(1000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(1));
    }

    #[test]
    fn capital_flow_zero_turnover() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(100)),
        });
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(350),
            prev_close: dec!(348),
            open: dec!(349),
            high: dec!(352),
            low: dec!(347),
            volume: 0,
            turnover: dec!(0),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0));
    }

    // ── capital_size_divergence ──

    #[test]
    fn capital_size_large_dominant() {
        let mut links = empty_links();
        links.capital_breakdowns.push(CapitalBreakdown {
            symbol: sym("700.HK"),
            large_net: dec!(100),
            medium_net: dec!(10),
            small_net: dec!(5),
        });

        let result = compute_capital_size_divergence(&links);
        let v = result[&sym("700.HK")];
        // 100 / (100 + 10 + 5) = 100/115
        assert!(v > dec!(0));
        assert!(v < dec!(1));
    }

    #[test]
    fn capital_size_small_dominant() {
        let mut links = empty_links();
        links.capital_breakdowns.push(CapitalBreakdown {
            symbol: sym("700.HK"),
            large_net: dec!(-100),
            medium_net: dec!(10),
            small_net: dec!(90),
        });

        let result = compute_capital_size_divergence(&links);
        let v = result[&sym("700.HK")];
        // -100 / (100 + 10 + 90) = -100/200 = -0.5
        assert_eq!(v, dec!(-0.5));
    }

    #[test]
    fn capital_size_no_flow() {
        let mut links = empty_links();
        links.capital_breakdowns.push(CapitalBreakdown {
            symbol: sym("700.HK"),
            large_net: dec!(0),
            medium_net: dec!(0),
            small_net: dec!(0),
        });

        let result = compute_capital_size_divergence(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0));
    }

    // ── institutional_direction ──

    #[test]
    fn institutional_bid_heavy() {
        let mut links = empty_links();
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1],
            bid_positions: vec![1, 2, 3],
            seat_count: 4,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(200),
            ask_positions: vec![],
            bid_positions: vec![2],
            seat_count: 1,
        });

        let result = compute_institutional_direction(&links);
        let v = result[&sym("700.HK")];
        // total bid seats = 4, total ask seats = 1 → (4-1)/(4+1) = 0.6
        assert_eq!(v, dec!(0.6));
    }

    #[test]
    fn institutional_ask_heavy() {
        let mut links = empty_links();
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2, 3],
            bid_positions: vec![1],
            seat_count: 4,
        });

        let result = compute_institutional_direction(&links);
        let v = result[&sym("700.HK")];
        // bid=1, ask=3 → (1-3)/(1+3) = -0.5
        assert_eq!(v, dec!(-0.5));
    }

    #[test]
    fn valuation_support_reads_calc_indexes_and_static_fallbacks() {
        let mut links = empty_links();
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(100),
            prev_close: dec!(100),
            open: dec!(100),
            high: dec!(101),
            low: dec!(99),
            volume: 1_000,
            turnover: dec!(100000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });
        links.quotes.push(QuoteObservation {
            symbol: sym("5.HK"),
            last_done: dec!(50),
            prev_close: dec!(50),
            open: dec!(50),
            high: dec!(50),
            low: dec!(50),
            volume: 1_000,
            turnover: dec!(50000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });
        links.calc_indexes.push(CalcIndexObservation {
            symbol: sym("700.HK"),
            turnover_rate: None,
            volume_ratio: None,
            pe_ttm_ratio: Some(dec!(20)),
            pb_ratio: Some(dec!(2)),
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
        });

        let store = make_store(vec![
            make_stock("700.HK", dec!(5), dec!(40), dec!(0.01)),
            make_stock("5.HK", dec!(10), dec!(10), dec!(0.05)),
        ]);

        let valuations = compute_valuation_support(&links, &store);
        assert!(valuations[&sym("5.HK")] > Decimal::ZERO);
        assert!(valuations[&sym("700.HK")] < Decimal::ZERO);
    }

    #[test]
    fn activity_momentum_uses_direction_and_activity_level() {
        let mut links = empty_links();
        links.calc_indexes.push(CalcIndexObservation {
            symbol: sym("700.HK"),
            turnover_rate: Some(dec!(0.04)),
            volume_ratio: Some(dec!(3)),
            pe_ttm_ratio: None,
            pb_ratio: None,
            dividend_ratio_ttm: None,
            amplitude: Some(dec!(0.06)),
            five_minutes_change_rate: Some(dec!(0.02)),
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: None,
            change_rate: None,
        });

        let activity = compute_activity_momentum(&links);
        assert!(activity[&sym("700.HK")] > Decimal::ZERO);
    }

    #[test]
    fn candlestick_conviction_uses_recent_ohlcv() {
        let mut links = empty_links();
        links.candlesticks.push(CandlestickObservation {
            symbol: sym("700.HK"),
            candle_count: 5,
            window_return: dec!(0.5),
            body_bias: dec!(0.6),
            volume_ratio: dec!(2.5),
            range_ratio: dec!(0.4),
        });

        let conviction = compute_candlestick_conviction(&links);
        assert!(conviction[&sym("700.HK")] > dec!(0.3));
    }

    // ── full snapshot ──

    #[test]
    fn full_dimension_snapshot() {
        let mut links = empty_links();
        let store = make_store(vec![make_stock("700.HK", dec!(10), dec!(20), dec!(0.03))]);

        // Order book for 700.HK
        links.order_books.push(OrderBookObservation {
            symbol: sym("700.HK"),
            ask_levels: vec![],
            bid_levels: vec![],
            total_ask_volume: 400,
            total_bid_volume: 600,
            total_ask_orders: 0,
            total_bid_orders: 0,
            spread: None,
            ask_level_count: 0,
            bid_level_count: 0,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        });

        // Quote for 700.HK
        links.quotes.push(QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(350),
            prev_close: dec!(348),
            open: dec!(349),
            high: dec!(352),
            low: dec!(347),
            volume: 1_000_000,
            turnover: dec!(10000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        });

        // Capital flow for 700.HK
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(500)),
        });

        // Capital breakdown for 700.HK
        links.capital_breakdowns.push(CapitalBreakdown {
            symbol: sym("700.HK"),
            large_net: dec!(200),
            medium_net: dec!(50),
            small_net: dec!(30),
        });

        // Institution activity for 700.HK
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1],
            bid_positions: vec![1, 2],
            seat_count: 3,
        });

        let snapshot = DimensionSnapshot::compute(&links, &store);
        let dims = &snapshot.dimensions[&sym("700.HK")];

        // order_book_pressure: (600-400)/(600+400) = 0.2
        assert_eq!(dims.order_book_pressure, dec!(0.2));
        // capital_flow_direction: 500/10000 = 0.05
        assert_eq!(dims.capital_flow_direction, dec!(0.05));
        // capital_size_divergence: 200/(200+50+30) = 200/280
        assert!(dims.capital_size_divergence > dec!(0));
        // institutional_direction: bid=2, ask=1 → (2-1)/(2+1) = 1/3
        let one_third = Decimal::ONE / Decimal::from(3);
        assert_eq!(
            dims.institutional_direction.round_dp(10),
            one_third.round_dp(10)
        );
    }
}
