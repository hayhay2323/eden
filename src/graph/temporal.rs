use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::links::{BrokerQueueEntry, Side};
use crate::ontology::objects::{BrokerId, InstitutionId, SectorId, Symbol};
use crate::ontology::{institution_numeric_node_id, sector_node_id, symbol_node_id};

use super::graph::{BrainGraph, EdgeKind, NodeKind};

#[path = "temporal/broker.rs"]
mod broker;
#[path = "temporal/edge.rs"]
mod edge;
#[path = "temporal/node.rs"]
mod node;
#[path = "temporal/shared.rs"]
mod shared;

pub use broker::*;
pub use edge::*;
pub use node::*;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rust_decimal_macros::dec;
    use time::Duration;

    use super::*;
    use crate::action::narrative::{NarrativeSnapshot, Regime, SymbolNarrative};
    use crate::graph::graph::BrainGraph;
    use crate::ontology::links::{CrossStockPresence, InstitutionActivity, LinkSnapshot};
    use crate::ontology::store::ObjectStore;
    use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn stock(symbol: &str) -> crate::ontology::objects::Stock {
        let symbol_id = sym(symbol);
        crate::ontology::objects::Stock {
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
            eps_ttm: Decimal::ZERO,
            bps: Decimal::ZERO,
            dividend_yield: Decimal::ZERO,
        }
    }

    fn store() -> ObjectStore {
        let mut stocks = HashMap::new();
        stocks.insert(sym("700.HK"), stock("700.HK"));
        stocks.insert(sym("9988.HK"), stock("9988.HK"));
        stocks.insert(sym("3690.HK"), stock("3690.HK"));
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks,
            sectors: HashMap::new(),
            broker_to_institution: HashMap::new(),
            knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
        }
    }

    fn narrative_snapshot(timestamp: OffsetDateTime) -> NarrativeSnapshot {
        NarrativeSnapshot {
            timestamp,
            narratives: HashMap::from([
                (
                    sym("700.HK"),
                    SymbolNarrative {
                        regime: Regime::CoherentBullish,
                        coherence: dec!(0.6),
                        mean_direction: dec!(0.4),
                        readings: vec![],
                        agreements: vec![],
                        contradictions: vec![],
                    },
                ),
                (
                    sym("9988.HK"),
                    SymbolNarrative {
                        regime: Regime::CoherentBullish,
                        coherence: dec!(0.5),
                        mean_direction: dec!(0.3),
                        readings: vec![],
                        agreements: vec![],
                        contradictions: vec![],
                    },
                ),
                (
                    sym("3690.HK"),
                    SymbolNarrative {
                        regime: Regime::CoherentNeutral,
                        coherence: dec!(0.4),
                        mean_direction: Decimal::ZERO,
                        readings: vec![],
                        agreements: vec![],
                        contradictions: vec![],
                    },
                ),
            ]),
        }
    }

    fn dimension_snapshot(timestamp: OffsetDateTime) -> DimensionSnapshot {
        DimensionSnapshot {
            timestamp,
            dimensions: HashMap::from([
                (
                    sym("700.HK"),
                    SymbolDimensions {
                        order_book_pressure: dec!(0.4),
                        capital_flow_direction: dec!(0.4),
                        capital_size_divergence: dec!(0.4),
                        institutional_direction: dec!(0.4),
                        ..Default::default()
                    },
                ),
                (
                    sym("9988.HK"),
                    SymbolDimensions {
                        order_book_pressure: dec!(0.4),
                        capital_flow_direction: dec!(0.4),
                        capital_size_divergence: dec!(0.4),
                        institutional_direction: dec!(0.4),
                        ..Default::default()
                    },
                ),
                (
                    sym("3690.HK"),
                    SymbolDimensions {
                        order_book_pressure: Decimal::ZERO,
                        capital_flow_direction: dec!(0.4),
                        capital_size_divergence: Decimal::ZERO,
                        institutional_direction: Decimal::ZERO,
                        ..Default::default()
                    },
                ),
            ]),
        }
    }

    fn links_with_institution_edge(timestamp: OffsetDateTime) -> LinkSnapshot {
        LinkSnapshot {
            timestamp,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![InstitutionActivity {
                symbol: sym("700.HK"),
                institution_id: InstitutionId(100),
                ask_positions: vec![],
                bid_positions: vec![1, 2],
                seat_count: 2,
            }],
            cross_stock_presences: vec![CrossStockPresence {
                institution_id: InstitutionId(100),
                symbols: vec![sym("700.HK"), sym("9988.HK")],
                ask_symbols: vec![],
                bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
            }],
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

    fn links_without_edges(timestamp: OffsetDateTime) -> LinkSnapshot {
        LinkSnapshot {
            timestamp,
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

    fn build_brain(timestamp: OffsetDateTime, links: LinkSnapshot) -> BrainGraph {
        let store = store();
        BrainGraph::compute(
            &narrative_snapshot(timestamp),
            &dimension_snapshot(timestamp),
            &links,
            &store,
        )
    }

    #[test]
    fn tracks_edge_appearance_disappearance_and_reappearance() {
        let mut registry = TemporalEdgeRegistry::new();
        let timestamp = OffsetDateTime::UNIX_EPOCH;
        let edge_id = GraphEdgeId::institution_to_stock(InstitutionId(100), &sym("700.HK"));

        let first = registry.update(
            &build_brain(timestamp, links_with_institution_edge(timestamp)),
            1,
        );
        assert_eq!(first.active_edge_count, 2);
        assert_eq!(first.transitions.len(), 2);
        assert!(first.transitions.iter().any(|item| {
            item.edge_id == edge_id && item.kind == GraphEdgeTransitionKind::Appeared
        }));

        let second = registry.update(
            &build_brain(
                timestamp + Duration::seconds(1),
                links_with_institution_edge(timestamp + Duration::seconds(1)),
            ),
            2,
        );
        assert!(second.transitions.is_empty());
        let state = registry.edge_state(&edge_id).unwrap();
        assert!(state.active);
        assert_eq!(state.first_seen_tick, 1);
        assert_eq!(state.last_seen_tick, 2);

        let third = registry.update(
            &build_brain(
                timestamp + Duration::seconds(2),
                links_without_edges(timestamp + Duration::seconds(2)),
            ),
            3,
        );
        assert!(third.transitions.iter().any(|item| {
            item.edge_id == edge_id && item.kind == GraphEdgeTransitionKind::Disappeared
        }));
        let state = registry.edge_state(&edge_id).unwrap();
        assert!(!state.active);
        assert_eq!(state.last_disappeared_tick, Some(3));

        let fourth = registry.update(
            &build_brain(
                timestamp + Duration::seconds(3),
                links_with_institution_edge(timestamp + Duration::seconds(3)),
            ),
            4,
        );
        assert!(fourth.transitions.iter().any(|item| {
            item.edge_id == edge_id && item.kind == GraphEdgeTransitionKind::Reappeared
        }));
        let state = registry.edge_state(&edge_id).unwrap();
        assert!(state.active);
        assert_eq!(state.last_appeared_tick, 4);
    }

    #[test]
    fn canonicalizes_undirected_similarity_edges() {
        let mut registry = TemporalEdgeRegistry::new();
        let timestamp = OffsetDateTime::UNIX_EPOCH;

        let delta = registry.update(&build_brain(timestamp, links_without_edges(timestamp)), 1);
        let stock_to_stock = delta
            .transitions
            .iter()
            .filter(|item| item.edge_id.kind == GraphEdgeKind::StockToStock)
            .collect::<Vec<_>>();
        assert_eq!(stock_to_stock.len(), 1);
        assert_eq!(registry.active_edges().len(), 1);
    }
}
