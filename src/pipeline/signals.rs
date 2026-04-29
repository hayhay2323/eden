use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::decision::DecisionSnapshot;
use crate::graph::insights::GraphInsights;
use crate::math::{median, normalized_ratio};
use crate::ontology::domain::{
    DerivedSignal, Event, Observation, ProvenanceMetadata, ProvenanceSource,
};
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::{InstitutionId, SectorId, Symbol, ThemeId};
use crate::temporal::buffer::TickHistory;

use super::dimensions::{DimensionSnapshot, SymbolDimensions};

#[path = "signals/derived.rs"]
mod derived;
#[path = "signals/events.rs"]
mod events;
#[path = "signals/helpers.rs"]
mod helpers;
#[path = "signals/observations.rs"]
mod observations;
#[path = "signals/types.rs"]
mod types;

// Re-export attribution types from types.rs (previously in events.rs, now rebuilt)
pub use events::broker_events_from_delta;
pub use events::catalyst_events_from_macro_events;
pub(crate) use events::detect_propagation_absences;
pub(crate) use events::enrich_attribution_with_evidence;
use helpers::*;
pub use types::*;
pub use types::{
    event_driver_kind, event_propagation_scope, EventDriverKind, EventPropagationScope,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::decision::{ConvergenceScore, MarketRegimeFilter};
    use crate::graph::insights::MarketStressIndex;
    use crate::graph::insights::StockPressure;
    use crate::ontology::links::{
        CalcIndexObservation, DepthLevel, DepthProfile, LinkSnapshot, MarketStatus,
        MarketTemperatureObservation, OrderBookObservation, QuoteObservation,
    };
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use crate::temporal::record::{SymbolSignals, TickRecord};
    use rust_decimal_macros::dec;

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn empty_history() -> TickHistory {
        TickHistory::new(10)
    }

    fn history_tick(
        tick_number: u64,
        symbol: &str,
        composite: Decimal,
        institutional_alignment: Decimal,
        market_stress: Decimal,
    ) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(
            sym(symbol),
            SymbolSignals {
                mark_price: None,
                composite,
                institutional_alignment,
                sector_coherence: None,
                cross_stock_correlation: Decimal::ZERO,
                order_book_pressure: Decimal::ZERO,
                capital_flow_direction: Decimal::ZERO,
                capital_size_divergence: Decimal::ZERO,
                institutional_direction: Decimal::ZERO,
                depth_structure_imbalance: Decimal::ZERO,
                bid_top3_ratio: Decimal::ZERO,
                ask_top3_ratio: Decimal::ZERO,
                bid_best_ratio: Decimal::ZERO,
                ask_best_ratio: Decimal::ZERO,
                spread: None,
                trade_count: 0,
                trade_volume: 0,
                buy_volume: 0,
                sell_volume: 0,
                vwap: None,
                convergence_score: None,
                composite_degradation: None,
                institution_retention: None,
                edge_stability: None,
                temporal_weight: None,
                microstructure_confirmation: None,
                component_spread: None,
                institutional_edge_age: None,
            },
        );
        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick_number as i64),
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Market,
                    kind: DerivedSignalKind::MarketStress,
                    strength: market_stress,
                    summary: "prev stress".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
            )],
            action_workflows: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
                perceptual_states: vec![],
                vortices: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        }
    }

    fn order_book(
        symbol: &str,
        total_bid_volume: i64,
        total_ask_volume: i64,
    ) -> OrderBookObservation {
        OrderBookObservation {
            symbol: sym(symbol),
            ask_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(10.1)),
                volume: total_ask_volume,
                order_num: 1,
            }],
            bid_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(10.0)),
                volume: total_bid_volume,
                order_num: 1,
            }],
            total_ask_volume,
            total_bid_volume,
            total_ask_orders: 1,
            total_bid_orders: 1,
            spread: Some(dec!(0.1)),
            ask_level_count: 1,
            bid_level_count: 1,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        }
    }

    #[test]
    fn provenance_includes_confidence_and_inputs() {
        let provenance = provenance(
            ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
            Some(dec!(0.6)),
            ["a", "b"],
        );
        assert_eq!(provenance.confidence, Some(dec!(0.6)));
        assert_eq!(provenance.inputs.len(), 2);
    }

    #[test]
    fn observation_snapshot_collects_market_temperature() {
        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_flow_series: vec![],
            capital_breakdowns: vec![],
            market_temperature: Some(MarketTemperatureObservation {
                temperature: dec!(70),
                valuation: dec!(60),
                sentiment: dec!(80),
                description: "warm".into(),
                timestamp: OffsetDateTime::UNIX_EPOCH,
            }),
            order_books: vec![],
            quotes: vec![QuoteObservation {
                symbol: sym("700.HK"),
                last_done: dec!(350),
                prev_close: dec!(348),
                open: dec!(349),
                high: dec!(352),
                low: dec!(347),
                volume: 100,
                turnover: dec!(35000),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
                pre_market: None,
                post_market: None,
            }],
            trade_activities: vec![],
            intraday: vec![],
        };

        let snapshot = ObservationSnapshot::from_links(&links);
        assert!(snapshot.observations.len() >= 2);
    }

    #[test]
    fn derived_signal_snapshot_emits_market_stress() {
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("700.HK"),
                SymbolDimensions {
                    valuation_support: dec!(0.4),
                    activity_momentum: dec!(0.5),
                    candlestick_conviction: dec!(0.6),
                    ..Default::default()
                },
            )]),
        };
        let insights = GraphInsights {
            pressures: vec![StockPressure {
                symbol: sym("700.HK"),
                net_pressure: dec!(0.7),
                institution_count: 1,
                buy_inst_count: 1,
                sell_inst_count: 0,
                pressure_delta: dec!(0.1),
                pressure_duration: 1,
                accelerating: true,
            }],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.4),
                pressure_consensus: dec!(0.5),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.8),
                composite_stress: dec!(0.6),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.4),
                    sector_coherence: Some(dec!(0.3)),
                    cross_stock_correlation: dec!(0.2),
                    composite: dec!(0.5),
                    edge_stability: None,
                    institutional_edge_age: None,
                    new_edge_fraction: None,
                    microstructure_confirmation: None,
                    component_spread: None,
                    temporal_weight: None,
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };
        let events = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![],
        };

        let snapshot = DerivedSignalSnapshot::compute(&dimensions, &insights, &decision, &events);
        assert!(snapshot
            .signals
            .iter()
            .any(|signal| matches!(signal.value.kind, DerivedSignalKind::MarketStress)));
    }

    #[test]
    fn event_snapshot_detects_temporal_transitions() {
        let mut history = empty_history();
        history.push(history_tick(1, "700.HK", dec!(0.1), dec!(-0.4), dec!(0.1)));

        let links = LinkSnapshot {
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
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.1),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.3),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.5),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.35),
                    edge_stability: None,
                    institutional_edge_age: None,
                    new_edge_fraction: None,
                    microstructure_confirmation: None,
                    component_spread: None,
                    temporal_weight: None,
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot =
            EventSnapshot::detect(&history, 2, &links, &dimensions, &insights, &decision);
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::CompositeAcceleration)));
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::InstitutionalFlip)));
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::StressRegimeShift)));
    }

    #[test]
    fn event_snapshot_ignores_current_tick_when_history_already_contains_it() {
        let mut history = empty_history();
        history.push(history_tick(1, "700.HK", dec!(0.1), dec!(-0.4), dec!(0.1)));
        history.push(history_tick(2, "700.HK", dec!(0.35), dec!(0.5), dec!(0.4)));

        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(2),
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
        };
        let dimensions = DimensionSnapshot {
            timestamp: links.timestamp,
            dimensions: HashMap::new(),
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.1),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.3),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: links.timestamp,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.5),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.35),
                    edge_stability: None,
                    institutional_edge_age: None,
                    new_edge_fraction: None,
                    microstructure_confirmation: None,
                    component_spread: None,
                    temporal_weight: None,
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot =
            EventSnapshot::detect(&history, 2, &links, &dimensions, &insights, &decision);
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::CompositeAcceleration)));
    }

    #[test]
    fn event_snapshot_uses_sample_derived_cutoffs() {
        let mut history = empty_history();
        history.push(history_tick(
            1,
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.2),
        ));
        history.push(history_tick(
            2,
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.4),
        ));
        history.push(history_tick(
            3,
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.6),
        ));

        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![
                CalcIndexObservation {
                    symbol: sym("A.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(1.2)),
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
                    capital_flow: None,
                    change_rate: None,
                },
                CalcIndexObservation {
                    symbol: sym("B.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(1.5)),
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
                    capital_flow: None,
                    change_rate: None,
                },
                CalcIndexObservation {
                    symbol: sym("C.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(4)),
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
                    capital_flow: None,
                    change_rate: None,
                },
            ],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_flow_series: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![
                order_book("A.HK", 55, 45),
                order_book("B.HK", 65, 35),
                order_book("C.HK", 95, 5),
            ],
            quotes: vec![],
            trade_activities: vec![],
            intraday: vec![],
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (
                    sym("A.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.2),
                        ..Default::default()
                    },
                ),
                (
                    sym("B.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.4),
                        ..Default::default()
                    },
                ),
                (
                    sym("C.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.8),
                        ..Default::default()
                    },
                ),
            ]),
        };
        let insights = GraphInsights {
            pressures: vec![
                StockPressure {
                    symbol: sym("A.HK"),
                    net_pressure: dec!(0.1),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("B.HK"),
                    net_pressure: dec!(0.4),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("C.HK"),
                    net_pressure: dec!(0.7),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
            ],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.3),
                pressure_consensus: dec!(0.4),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.5),
                composite_stress: dec!(0.7),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::new(),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot =
            EventSnapshot::detect(&history, 4, &links, &dimensions, &insights, &decision);

        let order_book_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::OrderBookDislocation))
            .collect();
        assert_eq!(order_book_events.len(), 1);
        assert!(matches!(
            &order_book_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let volume_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::VolumeDislocation))
            .collect();
        assert_eq!(volume_events.len(), 1);
        assert!(matches!(
            &volume_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let breakout_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::CandlestickBreakout))
            .collect();
        assert_eq!(breakout_events.len(), 1);
        assert!(matches!(
            &breakout_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let pressure_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::SmartMoneyPressure))
            .collect();
        assert_eq!(pressure_events.len(), 1);
        assert!(matches!(
            &pressure_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::MarketStressElevated)));
    }

    #[test]
    fn isolated_symbol_pressure_does_not_force_sector_propagation() {
        let history = empty_history();
        let links = LinkSnapshot {
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
            quotes: vec![QuoteObservation {
                symbol: sym("700.HK"),
                last_done: dec!(350),
                prev_close: dec!(348),
                open: dec!(349),
                high: dec!(351),
                low: dec!(347),
                volume: 100,
                turnover: dec!(35000),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
                pre_market: None,
                post_market: None,
            }],
            trade_activities: vec![],
            intraday: vec![],
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let insights = GraphInsights {
            pressures: vec![StockPressure {
                symbol: sym("700.HK"),
                net_pressure: dec!(0.7),
                institution_count: 1,
                buy_inst_count: 1,
                sell_inst_count: 0,
                pressure_delta: Decimal::ZERO,
                pressure_duration: 1,
                accelerating: false,
            }],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::new(),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot =
            EventSnapshot::detect(&history, 1, &links, &dimensions, &insights, &decision);

        assert!(!snapshot.events.iter().any(|event| {
            matches!(event.value.kind, MarketEventKind::CompositeAcceleration)
                && matches!(event.value.scope, SignalScope::Sector(_))
        }));
    }

    #[test]
    fn corroborated_symbol_pressure_emits_sector_propagation() {
        let history = empty_history();
        let links = LinkSnapshot {
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
            quotes: vec![
                QuoteObservation {
                    symbol: sym("700.HK"),
                    last_done: dec!(350),
                    prev_close: dec!(348),
                    open: dec!(349),
                    high: dec!(351),
                    low: dec!(347),
                    volume: 100,
                    turnover: dec!(35000),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    market_status: MarketStatus::Normal,
                    pre_market: None,
                    post_market: None,
                },
                QuoteObservation {
                    symbol: sym("9988.HK"),
                    last_done: dec!(90),
                    prev_close: dec!(88),
                    open: dec!(89),
                    high: dec!(91),
                    low: dec!(87),
                    volume: 100,
                    turnover: dec!(9000),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    market_status: MarketStatus::Normal,
                    pre_market: None,
                    post_market: None,
                },
                QuoteObservation {
                    symbol: sym("5.HK"),
                    last_done: dec!(70),
                    prev_close: dec!(69),
                    open: dec!(69),
                    high: dec!(71),
                    low: dec!(68),
                    volume: 100,
                    turnover: dec!(7000),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    market_status: MarketStatus::Normal,
                    pre_market: None,
                    post_market: None,
                },
                QuoteObservation {
                    symbol: sym("939.HK"),
                    last_done: dec!(40),
                    prev_close: dec!(39),
                    open: dec!(39),
                    high: dec!(41),
                    low: dec!(38),
                    volume: 100,
                    turnover: dec!(4000),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    market_status: MarketStatus::Normal,
                    pre_market: None,
                    post_market: None,
                },
            ],
            trade_activities: vec![],
            intraday: vec![],
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let insights = GraphInsights {
            pressures: vec![
                StockPressure {
                    symbol: sym("700.HK"),
                    net_pressure: dec!(0.7),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("9988.HK"),
                    net_pressure: dec!(0.68),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("5.HK"),
                    net_pressure: dec!(0.2),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("939.HK"),
                    net_pressure: dec!(0.18),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
            ],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::new(),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot =
            EventSnapshot::detect(&history, 1, &links, &dimensions, &insights, &decision);

        // Corroborated pressures (700.HK and 9988.HK above median) emit
        // SmartMoneyPressure events at the symbol level.
        let pressure_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::SmartMoneyPressure))
            .collect();
        assert!(
            pressure_events.len() >= 2,
            "expected at least 2 SmartMoneyPressure events for the corroborated symbols, got {}",
            pressure_events.len()
        );
    }
}
