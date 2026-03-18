use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::Symbol;

/// Per-symbol dimension vector. Each value in [-1, +1].
#[derive(Debug, Clone)]
pub struct SymbolDimensions {
    pub order_book_pressure: Decimal,     // bid-dominant = positive
    pub capital_flow_direction: Decimal,  // net inflow = positive
    pub capital_size_divergence: Decimal, // large-order-dominant = positive
    pub institutional_direction: Decimal, // institutional bid-bias = positive
}

/// Market-wide dimension snapshot.
#[derive(Debug)]
pub struct DimensionSnapshot {
    pub timestamp: OffsetDateTime,
    pub dimensions: HashMap<Symbol, SymbolDimensions>,
}

impl DimensionSnapshot {
    /// Pure synchronous function — compute all dimensions from a LinkSnapshot.
    pub fn compute(links: &LinkSnapshot) -> Self {
        let book_pressure = compute_order_book_pressure(links);
        let flow_direction = compute_capital_flow_direction(links);
        let size_divergence = compute_capital_size_divergence(links);
        let inst_direction = compute_institutional_direction(links);

        // Collect all symbols that appear in any dimension.
        let mut all_symbols: std::collections::HashSet<Symbol> = std::collections::HashSet::new();
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

        let zero = Decimal::ZERO;
        let dimensions = all_symbols
            .into_iter()
            .map(|sym| {
                let dims = SymbolDimensions {
                    order_book_pressure: book_pressure.get(&sym).copied().unwrap_or(zero),
                    capital_flow_direction: flow_direction.get(&sym).copied().unwrap_or(zero),
                    capital_size_divergence: size_divergence.get(&sym).copied().unwrap_or(zero),
                    institutional_direction: inst_direction.get(&sym).copied().unwrap_or(zero),
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

/// (A - B) / (A + B). Returns 0 when denominator is 0. Arithmetic identity, not a threshold.
fn normalized_ratio(a: Decimal, b: Decimal) -> Decimal {
    let sum = a + b;
    if sum == Decimal::ZERO {
        Decimal::ZERO
    } else {
        (a - b) / sum
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
            let ratio = cf.net_inflow / t;
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

    let mut all_symbols: std::collections::HashSet<Symbol> = std::collections::HashSet::new();
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
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            order_books: vec![],
            quotes: vec![],
        }
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
            net_inflow: dec!(100),
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
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(0.1));
    }

    #[test]
    fn capital_flow_outflow() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: dec!(-500),
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
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(-0.5));
    }

    #[test]
    fn capital_flow_clamp() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: dec!(2000),
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
        });

        let result = compute_capital_flow_direction(&links);
        assert_eq!(result[&sym("700.HK")], dec!(1));
    }

    #[test]
    fn capital_flow_zero_turnover() {
        let mut links = empty_links();
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: dec!(100),
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

    // ── full snapshot ──

    #[test]
    fn full_dimension_snapshot() {
        let mut links = empty_links();

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
        });

        // Capital flow for 700.HK
        links.capital_flows.push(CapitalFlow {
            symbol: sym("700.HK"),
            net_inflow: dec!(500),
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

        let snapshot = DimensionSnapshot::compute(&links);
        let dims = &snapshot.dimensions[&sym("700.HK")];

        // order_book_pressure: (600-400)/(600+400) = 0.2
        assert_eq!(dims.order_book_pressure, dec!(0.2));
        // capital_flow_direction: 500/10000 = 0.05
        assert_eq!(dims.capital_flow_direction, dec!(0.05));
        // capital_size_divergence: 200/(200+50+30) = 200/280
        assert!(dims.capital_size_divergence > dec!(0));
        // institutional_direction: bid=2, ask=1 → (2-1)/(2+1) = 1/3
        let one_third = Decimal::ONE / Decimal::from(3);
        assert_eq!(dims.institutional_direction.round_dp(10), one_third.round_dp(10));
    }
}
