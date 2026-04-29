use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use crate::us::common::{dimension_composite, SIGNAL_RESOLUTION_LAG};
use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::graph::propagation::CrossMarketSignal;

#[path = "decision/convergence.rs"]
mod convergence;
#[path = "decision/regime.rs"]
mod regime;
#[path = "decision/scorecard.rs"]
mod scorecard;
#[path = "decision/snapshot.rs"]
mod snapshot;

pub use convergence::UsConvergenceScore;
pub use regime::{UsMarketRegimeBias, UsMarketRegimeFilter};
pub use scorecard::{
    ProvenanceMetadata, UsOrderDirection, UsOrderSuggestion, UsSignalRecord, UsSignalScorecard,
    UsSignalScorecardAccumulator,
};
pub use snapshot::UsDecisionSnapshot;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::edge_learning::{EdgeKey, EdgeLearningLedger};
    use crate::ontology::objects::SectorId;
    use crate::pipeline::reasoning::ConvergenceDetail;
    use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
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

    fn make_graph(entries: Vec<(Symbol, UsSymbolDimensions)>) -> UsGraph {
        let snap = make_snapshot(entries);
        UsGraph::compute(&snap, &HashMap::new(), &HashMap::new())
    }

    #[test]
    fn convergence_basic_positive() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
        )]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], None).unwrap();
        assert!(score.dimension_composite > Decimal::ZERO);
        assert!(score.composite > Decimal::ZERO);
        assert!(score.cross_market_propagation.is_none());
    }

    #[test]
    fn convergence_with_cross_market() {
        let g = make_graph(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(0.2), dec!(0.1), dec!(0.05), dec!(0)),
        )]);
        let cm_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.6),
        }];
        let score = UsConvergenceScore::compute(&sym("BABA.US"), &g, &cm_signals, None).unwrap();
        assert_eq!(score.cross_market_propagation, Some(dec!(0.6)));
        assert!(score.composite > score.dimension_composite);
    }

    #[test]
    fn convergence_no_cross_market_for_non_dual() {
        let g = make_graph(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0), dec!(0.1)),
        )]);
        let cm_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.6),
        }];
        let score = UsConvergenceScore::compute(&sym("TSLA.US"), &g, &cm_signals, None).unwrap();
        assert!(score.cross_market_propagation.is_none());
    }

    #[test]
    fn convergence_with_sector() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.2), dec!(0.4), dec!(0.1), dec!(0.05), dec!(0.3)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), SectorId("tech".into())),
            (sym("MSFT.US"), SectorId("tech".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], None).unwrap();
        assert!(score.sector_coherence.is_some());
    }

    #[test]
    fn convergence_zero_dims() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0)),
        )]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], None).unwrap();
        assert_eq!(score.composite, Decimal::ZERO);
    }

    #[test]
    fn convergence_missing_symbol() {
        let g = make_graph(vec![]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], None);
        assert!(score.is_none());
    }

    #[test]
    fn convergence_respects_learned_stock_edge_multiplier() {
        // A sector_map is required so the graph creates stock-to-stock edges.
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.1), dec!(0.2), dec!(0.2), dec!(0.0), dec!(0.1)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.8), dec!(0.8), dec!(0.7), dec!(0.1), dec!(0.2)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), SectorId("tech".into())),
            (sym("MSFT.US"), SectorId("tech".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        let baseline = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], None)
            .expect("baseline convergence");

        let mut ledger = EdgeLearningLedger::default();
        let edge_key = EdgeKey::StockToStock {
            a: sym("AAPL.US"),
            b: sym("MSFT.US"),
        };
        ledger.credit_from_outcome(
            &sym("AAPL.US"),
            dec!(0.05),
            &ConvergenceDetail {
                institutional_alignment: dec!(0.1),
                sector_coherence: Some(dec!(0.1)),
                cross_stock_correlation: dec!(0.8),
                component_spread: None,
                edge_stability: None,
            },
            OffsetDateTime::UNIX_EPOCH,
            &[],
            &[edge_key],
            None,
        );

        let learned = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[], Some(&ledger))
            .expect("learned convergence");
        assert!(learned.cross_stock_correlation > baseline.cross_stock_correlation);
        assert!(learned.composite > baseline.composite);
    }

    #[test]
    fn regime_neutral_on_mixed_market() {
        let dims = HashMap::from([
            (
                sym("AAPL.US"),
                make_dims(dec!(0.1), dec!(0.2), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("NVDA.US"),
                make_dims(dec!(-0.1), dec!(-0.2), dec!(0), dec!(0), dec!(0)),
            ),
        ]);
        let macro_syms = vec![sym("SPY.US")];
        let regime = UsMarketRegimeFilter::compute(&dims, &macro_syms);
        assert_eq!(regime.bias, UsMarketRegimeBias::Neutral);
    }

    #[test]
    fn regime_risk_off_on_broad_selloff() {
        let mut dims = HashMap::new();
        for i in 0..20 {
            dims.insert(
                Symbol(format!("STOCK{}.US", i)),
                make_dims(dec!(-0.5), dec!(-0.8), dec!(-0.3), dec!(-0.4), dec!(0)),
            );
        }
        for i in 20..22 {
            dims.insert(
                Symbol(format!("STOCK{}.US", i)),
                make_dims(dec!(0.1), dec!(0.1), dec!(0), dec!(0), dec!(0)),
            );
        }
        dims.insert(
            sym("SPY.US"),
            make_dims(dec!(-0.5), dec!(-0.7), dec!(-0.3), dec!(-0.3), dec!(0)),
        );
        let macro_syms = vec![sym("SPY.US")];
        let regime = UsMarketRegimeFilter::compute(&dims, &macro_syms);
        assert_eq!(regime.bias, UsMarketRegimeBias::RiskOff);
        assert!(regime.blocks(UsOrderDirection::Buy));
        assert!(!regime.blocks(UsOrderDirection::Sell));
    }

    #[test]
    fn regime_empty_dims() {
        let regime = UsMarketRegimeFilter::compute(&HashMap::new(), &[]);
        assert_eq!(regime.bias, UsMarketRegimeBias::Neutral);
    }

    #[test]
    fn suggestions_generated_for_nonzero_composite() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 1, None);
        assert_eq!(snap.order_suggestions.len(), 1);
        let s = &snap.order_suggestions[0];
        assert_eq!(s.symbol, sym("AAPL.US"));
        assert_eq!(s.direction, UsOrderDirection::Buy);
        assert!(s.heuristic_edge > Decimal::ZERO);
        assert!(s.provenance.trace_id.contains("us-t1-AAPL.US"));
        assert!(!s.provenance.inputs.is_empty());
    }

    #[test]
    fn no_suggestion_for_zero_composite() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 1, None);
        assert!(snap.order_suggestions.is_empty());
    }

    #[test]
    fn sell_suggestion_for_negative_composite() {
        let g = make_graph(vec![(
            sym("NVDA.US"),
            make_dims(dec!(-0.4), dec!(-0.6), dec!(-0.3), dec!(-0.2), dec!(-0.1)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 5, None);
        let s = &snap.order_suggestions[0];
        assert_eq!(s.direction, UsOrderDirection::Sell);
    }

    #[test]
    fn provenance_includes_cross_market() {
        let g = make_graph(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(0.2), dec!(0.1), dec!(0.05), dec!(0)),
        )]);
        let cm = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.5),
        }];
        let snap = UsDecisionSnapshot::compute(&g, &cm, 10, None);
        let s = snap
            .order_suggestions
            .iter()
            .find(|s| s.symbol == sym("BABA.US"))
            .unwrap();
        assert!(s
            .provenance
            .inputs
            .iter()
            .any(|i| i.contains("hk_propagation")));
    }

    #[test]
    fn scorecard_empty() {
        let sc = UsSignalScorecard::compute(&[]);
        assert_eq!(sc.total_signals, 0);
        assert_eq!(sc.hit_rate, Decimal::ZERO);
    }

    #[test]
    fn scorecard_unresolved() {
        let records = vec![UsSignalRecord {
            setup_id: "setup:AAPL.US:1".into(),
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: false,
        }];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.total_signals, 1);
        assert_eq!(sc.resolved_signals, 0);
    }

    #[test]
    fn scorecard_resolved_hit() {
        let records = vec![UsSignalRecord {
            setup_id: "setup:AAPL.US:1".into(),
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: true,
            price_at_resolution: Some(dec!(185)),
            hit: Some(true),
            realized_return: Some(dec!(0.0278)),
            is_actionable_tier: false,
        }];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.hits, 1);
        assert_eq!(sc.misses, 0);
        assert_eq!(sc.hit_rate, Decimal::ONE);
        assert!(sc.mean_return > Decimal::ZERO);
    }

    #[test]
    fn scorecard_mixed() {
        let records = vec![
            UsSignalRecord {
                setup_id: "setup:AAPL.US:1".into(),
                symbol: sym("AAPL.US"),
                tick_emitted: 1,
                direction: UsOrderDirection::Buy,
                composite_at_emission: dec!(0.5),
                price_at_emission: Some(dec!(180)),
                resolved: true,
                price_at_resolution: Some(dec!(185)),
                hit: Some(true),
                realized_return: Some(dec!(0.028)),
                is_actionable_tier: false,
            },
            UsSignalRecord {
                setup_id: "setup:NVDA.US:2".into(),
                symbol: sym("NVDA.US"),
                tick_emitted: 2,
                direction: UsOrderDirection::Sell,
                composite_at_emission: dec!(-0.4),
                price_at_emission: Some(dec!(900)),
                resolved: true,
                price_at_resolution: Some(dec!(910)),
                hit: Some(false),
                realized_return: Some(dec!(-0.011)),
                is_actionable_tier: false,
            },
        ];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.hits, 1);
        assert_eq!(sc.misses, 1);
        assert_eq!(sc.hit_rate, dec!(0.5));
    }

    #[test]
    fn try_resolve_before_lag() {
        let mut record = UsSignalRecord {
            setup_id: "setup:AAPL.US:10".into(),
            symbol: sym("AAPL.US"),
            tick_emitted: 10,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: true,
        };
        UsSignalScorecard::try_resolve(&mut record, 20, Some(dec!(185)));
        assert!(!record.resolved);
    }

    #[test]
    fn try_resolve_after_lag() {
        let mut record = UsSignalRecord {
            setup_id: "setup:AAPL.US:10".into(),
            symbol: sym("AAPL.US"),
            tick_emitted: 10,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: true,
        };
        UsSignalScorecard::try_resolve(&mut record, 60, Some(dec!(185)));
        assert!(record.resolved);
        assert_eq!(record.hit, Some(true));
        assert!(record.realized_return.unwrap() > Decimal::ZERO);
    }

    #[test]
    fn try_resolve_sell_direction() {
        let mut record = UsSignalRecord {
            setup_id: "setup:NVDA.US:5".into(),
            symbol: sym("NVDA.US"),
            tick_emitted: 5,
            direction: UsOrderDirection::Sell,
            composite_at_emission: dec!(-0.4),
            price_at_emission: Some(dec!(900)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: true,
        };
        UsSignalScorecard::try_resolve(&mut record, 60, Some(dec!(880)));
        assert!(record.resolved);
        assert_eq!(record.hit, Some(true));
        assert!(record.realized_return.unwrap() > Decimal::ZERO);
    }

    #[test]
    fn try_resolve_already_resolved() {
        let mut record = UsSignalRecord {
            setup_id: "setup:AAPL.US:1".into(),
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: true,
            price_at_resolution: Some(dec!(185)),
            hit: Some(true),
            realized_return: Some(dec!(0.028)),
            is_actionable_tier: true,
        };
        UsSignalScorecard::try_resolve(&mut record, 100, Some(dec!(170)));
        assert_eq!(record.price_at_resolution, Some(dec!(185)));
        assert_eq!(record.hit, Some(true));
    }
}
