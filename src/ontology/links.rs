use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::microstructure::TradeSession;

use super::objects::{BrokerId, InstitutionId, Symbol};
use super::snapshot::RawSnapshot;
use super::store::ObjectStore;

#[path = "links/broker.rs"]
mod broker;
#[path = "links/market_data.rs"]
mod market_data;
#[path = "links/quote_trade.rs"]
mod quote_trade;
#[path = "links/replay.rs"]
mod replay;
#[path = "links/types.rs"]
mod types;

pub use quote_trade::convert_pre_post_quote;
pub use types::*;

#[cfg(test)]
use broker::{
    compute_broker_queues, compute_cross_stock_presences, compute_institution_activities,
};
#[cfg(test)]
use market_data::{compute_capital_breakdowns, compute_capital_flows};
#[cfg(test)]
use quote_trade::compute_order_books;
#[cfg(test)]
use quote_trade::compute_quotes;

impl LinkSnapshot {
    pub fn compute(raw: &RawSnapshot, store: &ObjectStore) -> Self {
        let broker_queues = broker::compute_broker_queues(raw);
        let calc_indexes = market_data::compute_calc_indexes(raw);
        let candlesticks = market_data::compute_candlesticks(raw);
        let institution_activities = broker::compute_institution_activities(&broker_queues, store);
        let cross_stock_presences = broker::compute_cross_stock_presences(&institution_activities);
        let capital_flows = market_data::compute_capital_flows(raw);
        let capital_flow_series = market_data::compute_capital_flow_series(raw);
        let capital_breakdowns = market_data::compute_capital_breakdowns(raw);
        let market_temperature = market_data::compute_market_temperature(raw);
        let order_books = quote_trade::compute_order_books(raw);
        let quotes = quote_trade::compute_quotes(raw);
        let trade_activities = quote_trade::compute_trade_activities(raw);

        LinkSnapshot {
            timestamp: raw.timestamp,
            broker_queues,
            calc_indexes,
            candlesticks,
            institution_activities,
            cross_stock_presences,
            capital_flows,
            capital_flow_series,
            capital_breakdowns,
            market_temperature,
            order_books,
            quotes,
            trade_activities,
            intraday: vec![],
        }
    }

    /// Attach intraday observations (from REST fetch, not part of RawSnapshot).
    pub fn with_intraday(mut self, intraday: Vec<IntradayObservation>) -> Self {
        self.intraday = intraday;
        self
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::super::objects::{Institution, InstitutionClass};
    use super::*;
    use longport::quote::{
        Brokers, CapitalDistribution, CapitalDistributionResponse, CapitalFlowLine,
        Depth as LPDepth, SecurityBrokers, SecurityDepth, SecurityQuote, TradeStatus,
    };

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_raw_with_brokers(data: Vec<(Symbol, SecurityBrokers)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: data.into_iter().collect(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        }
    }

    fn make_store_with_institutions(institutions: Vec<(i32, &[i32])>) -> ObjectStore {
        let insts: Vec<Institution> = institutions
            .into_iter()
            .map(|(min_id, broker_ids)| Institution {
                id: InstitutionId(min_id),
                name_en: format!("Inst{}", min_id),
                name_cn: String::new(),
                name_hk: String::new(),
                broker_ids: broker_ids.iter().map(|&i| BrokerId(i)).collect(),
                class: InstitutionClass::Unknown,
            })
            .collect();

        ObjectStore::from_parts(insts, vec![], vec![])
    }

    // ── broker_queue tests ──

    #[test]
    fn broker_queue_basic() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].broker_id, BrokerId(100));
        assert_eq!(entries[0].side, Side::Ask);
        assert_eq!(entries[0].position, 1);
        assert_eq!(entries[0].symbol, sym("700.HK"));
    }

    #[test]
    fn broker_queue_multiple_at_same_position() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100, 200, 300],
                }],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 3);
        assert!(entries
            .iter()
            .all(|e| e.position == 1 && e.side == Side::Ask));
    }

    #[test]
    fn broker_queue_both_sides() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![Brokers {
                    position: 2,
                    broker_ids: vec![100],
                }],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 2);
        let ask = entries.iter().find(|e| e.side == Side::Ask).unwrap();
        let bid = entries.iter().find(|e| e.side == Side::Bid).unwrap();
        assert_eq!(ask.broker_id, BrokerId(100));
        assert_eq!(bid.broker_id, BrokerId(100));
        assert_eq!(ask.position, 1);
        assert_eq!(bid.position, 2);
    }

    #[test]
    fn broker_queue_empty() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert!(entries.is_empty());
    }

    // ── institution_activity tests ──

    #[test]
    fn institution_activity_aggregation() {
        // Institution 100 has 3 seats: 100, 101, 102
        let store = make_store_with_institutions(vec![(100, &[100, 101, 102])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![
                    Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    },
                    Brokers {
                        position: 3,
                        broker_ids: vec![101],
                    },
                ],
                bid_brokers: vec![Brokers {
                    position: 2,
                    broker_ids: vec![102],
                }],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        assert_eq!(activities.len(), 1);

        let act = &activities[0];
        assert_eq!(act.institution_id, InstitutionId(100));
        assert_eq!(act.seat_count, 3);
        assert_eq!(act.ask_positions.len(), 2);
        assert_eq!(act.bid_positions.len(), 1);
    }

    #[test]
    fn institution_activity_unknown_broker() {
        // Store only knows broker 100, not 999
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100, 999],
                }],
                bid_brokers: vec![],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        // Only broker 100 is recognized → 1 activity with seat_count=1
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].seat_count, 1);
    }

    // ── cross_stock tests ──

    #[test]
    fn cross_stock_two_stocks() {
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![
            (
                sym("700.HK"),
                SecurityBrokers {
                    ask_brokers: vec![Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    }],
                    bid_brokers: vec![],
                },
            ),
            (
                sym("9988.HK"),
                SecurityBrokers {
                    ask_brokers: vec![],
                    bid_brokers: vec![Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    }],
                },
            ),
        ]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        let cross = compute_cross_stock_presences(&activities);
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].institution_id, InstitutionId(100));
        assert_eq!(cross[0].symbols.len(), 2);
    }

    #[test]
    fn cross_stock_single_stock() {
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        let cross = compute_cross_stock_presences(&activities);
        assert!(cross.is_empty());
    }

    // ── capital_flow tests ──

    #[test]
    fn capital_flow_latest() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([(
                sym("700.HK"),
                vec![
                    CapitalFlowLine {
                        inflow: Decimal::new(100, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                    CapitalFlowLine {
                        inflow: Decimal::new(200, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                    CapitalFlowLine {
                        inflow: Decimal::new(300, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                ],
            )]),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let flows = compute_capital_flows(&raw);
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].net_inflow.as_yuan(), Decimal::new(3_000_000, 0));
    }

    #[test]
    fn capital_flow_empty() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([(sym("700.HK"), vec![])]),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let flows = compute_capital_flows(&raw);
        assert!(flows.is_empty());
    }

    // ── capital_breakdown tests ──

    #[test]
    fn capital_breakdown_net() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::from([(
                sym("700.HK"),
                CapitalDistributionResponse {
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    capital_in: CapitalDistribution {
                        large: Decimal::new(100, 0),
                        medium: Decimal::new(50, 0),
                        small: Decimal::new(20, 0),
                    },
                    capital_out: CapitalDistribution {
                        large: Decimal::new(30, 0),
                        medium: Decimal::new(10, 0),
                        small: Decimal::new(5, 0),
                    },
                },
            )]),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let breakdowns = compute_capital_breakdowns(&raw);
        assert_eq!(breakdowns.len(), 1);
        assert_eq!(breakdowns[0].large_net, Decimal::new(70, 0));
        assert_eq!(breakdowns[0].medium_net, Decimal::new(40, 0));
        assert_eq!(breakdowns[0].small_net, Decimal::new(15, 0));
    }

    // ── full integration ──

    #[test]
    fn full_snapshot_integration() {
        let store = make_store_with_institutions(vec![(100, &[100, 101]), (200, &[200])]);

        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::from([
                (
                    sym("700.HK"),
                    SecurityBrokers {
                        ask_brokers: vec![Brokers {
                            position: 1,
                            broker_ids: vec![100, 200],
                        }],
                        bid_brokers: vec![Brokers {
                            position: 1,
                            broker_ids: vec![101],
                        }],
                    },
                ),
                (
                    sym("9988.HK"),
                    SecurityBrokers {
                        ask_brokers: vec![Brokers {
                            position: 2,
                            broker_ids: vec![100],
                        }],
                        bid_brokers: vec![],
                    },
                ),
            ]),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([
                (
                    sym("700.HK"),
                    vec![CapitalFlowLine {
                        inflow: Decimal::new(500, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    }],
                ),
                (
                    sym("9988.HK"),
                    vec![CapitalFlowLine {
                        inflow: Decimal::new(-200, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    }],
                ),
            ]),
            capital_distributions: HashMap::from([(
                sym("700.HK"),
                CapitalDistributionResponse {
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    capital_in: CapitalDistribution {
                        large: Decimal::new(1000, 0),
                        medium: Decimal::new(500, 0),
                        small: Decimal::new(200, 0),
                    },
                    capital_out: CapitalDistribution {
                        large: Decimal::new(400, 0),
                        medium: Decimal::new(200, 0),
                        small: Decimal::new(100, 0),
                    },
                },
            )]),
            depths: HashMap::from([(
                sym("700.HK"),
                SecurityDepth {
                    asks: vec![LPDepth {
                        position: 1,
                        price: Some(Decimal::new(35000, 2)),
                        volume: 1000,
                        order_num: 5,
                    }],
                    bids: vec![LPDepth {
                        position: 1,
                        price: Some(Decimal::new(34980, 2)),
                        volume: 800,
                        order_num: 3,
                    }],
                },
            )]),
            market_temperature: None,
            quotes: HashMap::from([(
                sym("700.HK"),
                SecurityQuote {
                    symbol: "700.HK".into(),
                    last_done: Decimal::new(35000, 2),
                    prev_close: Decimal::new(34800, 2),
                    open: Decimal::new(34900, 2),
                    high: Decimal::new(35200, 2),
                    low: Decimal::new(34700, 2),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    volume: 10_000_000,
                    turnover: Decimal::new(35_000_000_00, 2),
                    trade_status: TradeStatus::Normal,
                    pre_market_quote: None,
                    post_market_quote: None,
                    overnight_quote: None,
                },
            )]),
            trades: HashMap::new(),
        };

        let snapshot = LinkSnapshot::compute(&raw, &store);

        // Broker queues: 700.HK has 3 entries (100 ask, 200 ask, 101 bid) + 9988.HK has 1 (100 ask) = 4
        assert_eq!(snapshot.broker_queues.len(), 4);

        // Institution activities: inst 100 in 700.HK + 9988.HK, inst 200 in 700.HK = 3
        assert_eq!(snapshot.institution_activities.len(), 3);

        // Cross-stock: inst 100 appears in 2 stocks
        assert_eq!(snapshot.cross_stock_presences.len(), 1);
        assert_eq!(
            snapshot.cross_stock_presences[0].institution_id,
            InstitutionId(100)
        );

        // Capital flows: 2 symbols
        assert_eq!(snapshot.capital_flows.len(), 2);

        // Capital breakdowns: 1 symbol (only 700.HK has distribution data)
        assert_eq!(snapshot.capital_breakdowns.len(), 1);
        assert_eq!(
            snapshot.capital_breakdowns[0].large_net,
            Decimal::new(600, 0)
        );

        // Order books: 1 symbol with depth data
        assert_eq!(snapshot.order_books.len(), 1);
        assert_eq!(snapshot.order_books[0].spread, Some(Decimal::new(20, 2)));

        // Quotes: 1 symbol
        assert_eq!(snapshot.quotes.len(), 1);
        assert_eq!(snapshot.quotes[0].market_status, MarketStatus::Normal);
        assert_eq!(snapshot.quotes[0].last_done, Decimal::new(35000, 2));
    }

    // ── order_book tests ──

    fn make_raw_with_depths(data: Vec<(Symbol, SecurityDepth)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: data.into_iter().collect(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        }
    }

    fn make_raw_with_quotes(data: Vec<(Symbol, SecurityQuote)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: data.into_iter().collect(),
            trades: HashMap::new(),
        }
    }

    fn make_quote(symbol: &str, trade_status: TradeStatus) -> SecurityQuote {
        SecurityQuote {
            symbol: symbol.into(),
            last_done: Decimal::new(35000, 2),
            prev_close: Decimal::new(34800, 2),
            open: Decimal::new(34900, 2),
            high: Decimal::new(35200, 2),
            low: Decimal::new(34700, 2),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            volume: 10_000_000,
            turnover: Decimal::new(35_000_000_00, 2),
            trade_status,
            pre_market_quote: None,
            post_market_quote: None,
            overnight_quote: None,
        }
    }

    #[test]
    fn order_book_basic_spread() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(35000, 2)),
                    volume: 500,
                    order_num: 3,
                }],
                bids: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(34980, 2)),
                    volume: 400,
                    order_num: 2,
                }],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].spread, Some(Decimal::new(20, 2)));
        assert_eq!(books[0].total_ask_volume, 500);
        assert_eq!(books[0].total_bid_volume, 400);
    }

    #[test]
    fn order_book_multiple_levels() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![
                    LPDepth {
                        position: 1,
                        price: Some(Decimal::new(35000, 2)),
                        volume: 100,
                        order_num: 1,
                    },
                    LPDepth {
                        position: 2,
                        price: Some(Decimal::new(35020, 2)),
                        volume: 200,
                        order_num: 2,
                    },
                    LPDepth {
                        position: 3,
                        price: Some(Decimal::new(35040, 2)),
                        volume: 300,
                        order_num: 3,
                    },
                ],
                bids: vec![
                    LPDepth {
                        position: 1,
                        price: Some(Decimal::new(34980, 2)),
                        volume: 150,
                        order_num: 1,
                    },
                    LPDepth {
                        position: 2,
                        price: Some(Decimal::new(34960, 2)),
                        volume: 250,
                        order_num: 4,
                    },
                ],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].total_ask_volume, 600);
        assert_eq!(books[0].total_bid_volume, 400);
        assert_eq!(books[0].total_ask_orders, 6);
        assert_eq!(books[0].total_bid_orders, 5);
        assert_eq!(books[0].ask_level_count, 3);
        assert_eq!(books[0].bid_level_count, 2);
    }

    #[test]
    fn order_book_empty_one_side() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(35000, 2)),
                    volume: 100,
                    order_num: 1,
                }],
                bids: vec![],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].spread, None);
        assert_eq!(books[0].total_bid_volume, 0);
    }

    #[test]
    fn order_book_empty_depth() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![],
                bids: vec![],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].total_ask_volume, 0);
        assert_eq!(books[0].total_bid_volume, 0);
        assert_eq!(books[0].spread, None);
    }

    #[test]
    fn order_book_no_symbols() {
        let raw = make_raw_with_depths(vec![]);
        let books = compute_order_books(&raw);
        assert!(books.is_empty());
    }

    #[test]
    fn quote_basic() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Normal),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].last_done, Decimal::new(35000, 2));
        assert_eq!(quotes[0].prev_close, Decimal::new(34800, 2));
        assert_eq!(quotes[0].market_status, MarketStatus::Normal);
    }

    #[test]
    fn quote_halted_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Halted),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::Halted);
    }

    #[test]
    fn quote_suspended_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::SuspendTrade),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::SuspendTrade);
    }

    #[test]
    fn quote_unknown_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Expired),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::Other);
    }

    #[test]
    fn quote_multiple_symbols() {
        let raw = make_raw_with_quotes(vec![
            (sym("700.HK"), make_quote("700.HK", TradeStatus::Normal)),
            (sym("9988.HK"), make_quote("9988.HK", TradeStatus::Normal)),
        ]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes.len(), 2);
    }

    #[test]
    fn quote_empty() {
        let raw = make_raw_with_quotes(vec![]);
        let quotes = compute_quotes(&raw);
        assert!(quotes.is_empty());
    }
}
