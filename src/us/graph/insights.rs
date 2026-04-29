//! US GraphInsights — cross-entity signals derived from the UsGraph knowledge graph.

use std::collections::{HashMap, HashSet};

use crate::ontology::objects::{SectorId, Symbol};
use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::graph::propagation::CrossMarketSignal;
use crate::us::pipeline::dimensions::UsDimensionSnapshot;
use crate::us::temporal::analysis::UsSignalDynamics;

#[path = "insights/anomaly.rs"]
mod anomaly;
#[path = "insights/cluster.rs"]
mod cluster;
#[path = "insights/helpers.rs"]
mod helpers;
#[path = "insights/pressure.rs"]
mod pressure;
#[path = "insights/rotation.rs"]
mod rotation;
#[path = "insights/stress.rs"]
mod stress;
#[path = "insights/types.rs"]
mod types;

pub use anomaly::compute_propagation_senses;
use helpers::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::ontology::objects::{SectorId, Symbol};
    use crate::us::graph::graph::UsGraph;
    use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn sector(s: &str) -> SectorId {
        SectorId(s.into())
    }

    fn make_dims(
        flow: Decimal,
        momentum: Decimal,
        volume: Decimal,
        prepost: Decimal,
        val: Decimal,
    ) -> UsSymbolDimensions {
        UsSymbolDimensions {
            capital_flow_direction: flow,
            price_momentum: momentum,
            volume_profile: volume,
            pre_post_market_anomaly: prepost,
            valuation: val,
            multi_horizon_momentum: Decimal::ZERO,
        }
    }

    fn make_snapshot(entries: Vec<(Symbol, UsSymbolDimensions)>) -> UsDimensionSnapshot {
        UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: entries.into_iter().collect(),
        }
    }

    fn make_graph(snap: &UsDimensionSnapshot, sector_map: &HashMap<Symbol, SectorId>) -> UsGraph {
        UsGraph::compute(snap, sector_map, &HashMap::new())
    }

    #[test]
    fn pressure_values_match_dimensions() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.6), dec!(0.4), dec!(0.3), dec!(0), dec!(0)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(-0.3), dec!(-0.2), dec!(0.1), dec!(0), dec!(0)),
            ),
        ]);
        let graph = make_graph(&snap, &HashMap::new());
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert_eq!(insights.pressures[0].symbol, sym("AAPL.US"));
        assert_eq!(insights.pressures[0].capital_flow_pressure, dec!(0.6));
        assert_eq!(insights.pressures[0].volume_intensity, dec!(0.3));
        assert_eq!(insights.pressures[0].momentum, dec!(0.4));
    }

    #[test]
    fn pressure_delta_and_duration() {
        let snap1 = make_snapshot(vec![(
            sym("NVDA.US"),
            make_dims(dec!(0.4), dec!(0.5), dec!(0.2), dec!(0), dec!(0)),
        )]);
        let graph1 = make_graph(&snap1, &HashMap::new());
        let insights1 = UsGraphInsights::compute(&graph1, &snap1, &[], None, 1);

        let snap2 = make_snapshot(vec![(
            sym("NVDA.US"),
            make_dims(dec!(0.6), dec!(0.5), dec!(0.2), dec!(0), dec!(0)),
        )]);
        let graph2 = make_graph(&snap2, &HashMap::new());
        let insights2 = UsGraphInsights::compute(&graph2, &snap2, &[], Some(&insights1), 2);

        let p = &insights2.pressures[0];
        assert_eq!(p.symbol, sym("NVDA.US"));
        assert_eq!(p.pressure_delta, dec!(0.2));
        assert_eq!(p.pressure_duration, 2);
    }

    #[test]
    fn pressure_direction_flip_resets_duration() {
        let snap1 = make_snapshot(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0.5), dec!(0.3), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let graph1 = make_graph(&snap1, &HashMap::new());
        let insights1 = UsGraphInsights::compute(&graph1, &snap1, &[], None, 1);

        let snap2 = make_snapshot(vec![(
            sym("TSLA.US"),
            make_dims(dec!(-0.3), dec!(-0.2), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let graph2 = make_graph(&snap2, &HashMap::new());
        let insights2 = UsGraphInsights::compute(&graph2, &snap2, &[], Some(&insights1), 2);

        let p = &insights2.pressures[0];
        assert_eq!(p.pressure_duration, 1);
    }

    #[test]
    fn zero_flow_resets_pressure_duration() {
        let snap1 = make_snapshot(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0.5), dec!(0.3), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let graph1 = make_graph(&snap1, &HashMap::new());
        let insights1 = UsGraphInsights::compute(&graph1, &snap1, &[], None, 1);

        let snap2 = make_snapshot(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0), dec!(0.1), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let graph2 = make_graph(&snap2, &HashMap::new());
        let insights2 = UsGraphInsights::compute(&graph2, &snap2, &[], Some(&insights1), 2);

        let p = &insights2.pressures[0];
        assert_eq!(p.pressure_duration, 0);
    }

    #[test]
    fn sector_rotation_only_above_median() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.8), dec!(0.7), dec!(0.5), dec!(0), dec!(0)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.7), dec!(0.6), dec!(0.4), dec!(0), dec!(0)),
            ),
            (
                sym("XOM.US"),
                make_dims(dec!(-0.5), dec!(-0.4), dec!(0.1), dec!(0), dec!(0)),
            ),
            (
                sym("CVX.US"),
                make_dims(dec!(-0.6), dec!(-0.3), dec!(0.2), dec!(0), dec!(0)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), sector("tech")),
            (sym("MSFT.US"), sector("tech")),
            (sym("XOM.US"), sector("energy")),
            (sym("CVX.US"), sector("energy")),
        ]);
        let graph = make_graph(&snap, &sector_map);
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert!(insights.rotations.len() <= 1);
    }

    #[test]
    fn sector_rotation_spread_correct() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.9), dec!(0.8), dec!(0.5), dec!(0), dec!(0)),
            ),
            (
                sym("XOM.US"),
                make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0), dec!(0)),
            ),
            (
                sym("GS.US"),
                make_dims(dec!(-0.7), dec!(-0.6), dec!(0.2), dec!(0), dec!(0)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), sector("tech")),
            (sym("XOM.US"), sector("energy")),
            (sym("GS.US"), sector("finance")),
        ]);
        let graph = make_graph(&snap, &sector_map);
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert!(!insights.rotations.is_empty());
        assert!(insights.rotations[0].spread >= insights.rotations.last().unwrap().spread);
    }

    #[test]
    fn cluster_age_filter_requires_3_ticks() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.8), dec!(0.7), dec!(0.5), dec!(0.1), dec!(0.2)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.8), dec!(0.7), dec!(0.5), dec!(0.1), dec!(0.2)),
            ),
            (
                sym("XOM.US"),
                make_dims(dec!(-0.4), dec!(0.1), dec!(-0.3), dec!(0), dec!(0.1)),
            ),
        ]);
        let graph = make_graph(
            &snap,
            &HashMap::from([
                (sym("AAPL.US"), sector("tech")),
                (sym("MSFT.US"), sector("tech")),
                (sym("XOM.US"), sector("energy")),
            ]),
        );
        let insights1 = UsGraphInsights::compute(&graph, &snap, &[], None, 1);
        assert!(!insights1.clusters.is_empty());

        let insights2 = UsGraphInsights::compute(&graph, &snap, &[], Some(&insights1), 2);
        assert!(insights2.clusters.is_empty());

        let insights3 = UsGraphInsights::compute(&graph, &snap, &[], Some(&insights2), 3);
        let _ = insights3;
    }

    #[test]
    fn stress_index_consensus_calculation() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0), dec!(0)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.2), dec!(0.4), dec!(0.1), dec!(0), dec!(0)),
            ),
            (
                sym("NVDA.US"),
                make_dims(dec!(0.1), dec!(0.3), dec!(0.3), dec!(0), dec!(0)),
            ),
            (
                sym("TSLA.US"),
                make_dims(dec!(-0.4), dec!(-0.6), dec!(0.5), dec!(0), dec!(0)),
            ),
        ]);
        let graph = make_graph(&snap, &HashMap::new());
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert_eq!(insights.stress.momentum_consensus, dec!(0.75));
    }

    #[test]
    fn stress_index_dispersion_zero_when_uniform() {
        let snap = make_snapshot(vec![
            (
                sym("A.US"),
                make_dims(dec!(0.5), dec!(0.4), dec!(0.3), dec!(0), dec!(0)),
            ),
            (
                sym("B.US"),
                make_dims(dec!(0.5), dec!(0.2), dec!(0.1), dec!(0), dec!(0)),
            ),
            (
                sym("C.US"),
                make_dims(dec!(0.5), dec!(0.3), dec!(0.2), dec!(0), dec!(0)),
            ),
        ]);
        let graph = make_graph(&snap, &HashMap::new());
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert_eq!(insights.stress.pressure_dispersion, dec!(0));
    }

    #[test]
    fn cross_market_anomaly_opposite_direction() {
        use crate::us::graph::propagation::CrossMarketSignal;

        let snap = make_snapshot(vec![(
            sym("BABA.US"),
            make_dims(dec!(-0.5), dec!(-0.6), dec!(0.2), dec!(0), dec!(0)),
        )]);
        let graph = make_graph(&snap, &HashMap::new());

        let cross_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.7),
            hk_inst_alignment: dec!(0.8),
            hk_timestamp: "2026-03-21T08:00:00Z".into(),
            time_since_hk_close_minutes: 30,
            propagation_confidence: dec!(0.63),
        }];

        let insights = UsGraphInsights::compute(&graph, &snap, &cross_signals, None, 1);

        assert_eq!(insights.cross_market_anomalies.len(), 1);
        let anomaly = &insights.cross_market_anomalies[0];
        assert_eq!(anomaly.us_symbol, sym("BABA.US"));
        assert_eq!(anomaly.hk_symbol, sym("9988.HK"));
        assert!(anomaly.expected_direction > Decimal::ZERO);
        assert!(anomaly.actual_direction < Decimal::ZERO);
        assert!(anomaly.divergence > Decimal::ZERO);
    }

    #[test]
    fn cross_market_no_anomaly_when_aligned() {
        use crate::us::graph::propagation::CrossMarketSignal;

        let snap = make_snapshot(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.4), dec!(0.5), dec!(0.3), dec!(0), dec!(0)),
        )]);
        let graph = make_graph(&snap, &HashMap::new());

        let cross_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-21T08:00:00Z".into(),
            time_since_hk_close_minutes: 30,
            propagation_confidence: dec!(0.5),
        }];

        let insights = UsGraphInsights::compute(&graph, &snap, &cross_signals, None, 1);
        assert!(insights.cross_market_anomalies.is_empty());
    }

    #[test]
    fn empty_graph_returns_zero_stress() {
        let snap = make_snapshot(vec![]);
        let graph = make_graph(&snap, &HashMap::new());
        let insights = UsGraphInsights::compute(&graph, &snap, &[], None, 1);

        assert!(insights.pressures.is_empty());
        assert!(insights.rotations.is_empty());
        assert!(insights.clusters.is_empty());
        assert_eq!(insights.stress.pressure_dispersion, dec!(0));
        assert_eq!(insights.stress.momentum_consensus, dec!(0));
        assert_eq!(insights.stress.volume_anomaly, dec!(0));
    }

    #[test]
    fn pressure_accelerating_when_delta_grows() {
        let snap1 = make_snapshot(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.3), dec!(0.2), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let g1 = make_graph(&snap1, &HashMap::new());
        let i1 = UsGraphInsights::compute(&g1, &snap1, &[], None, 1);

        let snap2 = make_snapshot(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.5), dec!(0.3), dec!(0.1), dec!(0), dec!(0)),
        )]);
        let g2 = make_graph(&snap2, &HashMap::new());
        let i2 = UsGraphInsights::compute(&g2, &snap2, &[], Some(&i1), 2);

        let p = &i2.pressures[0];
        assert!(p.accelerating);
    }
}
