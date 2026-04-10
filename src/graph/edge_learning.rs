use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::pipeline::reasoning::ConvergenceDetail;

/// Typed edge key — avoids per-tick format!() string allocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EdgeKey {
    InstitutionToStock {
        institution_id: InstitutionId,
        symbol: Symbol,
    },
    StockToStock {
        a: Symbol,
        b: Symbol,
    }, // a < b (sorted)
    StockToSector {
        symbol: Symbol,
        sector_id: SectorId,
    },
}

/// Per-edge accumulated learning signal from historical outcomes.
/// Runtime持有，跨tick累積。Profitable edges get amplified, losing edges dampened.
#[derive(Debug, Clone, Default)]
pub struct EdgeLearningLedger {
    entries: HashMap<EdgeKey, EdgeCredit>,
}

#[derive(Debug, Clone)]
pub struct EdgeCredit {
    pub total_credit: Decimal,
    pub sample_count: u32,
    pub mean_credit: Decimal,
    pub last_updated: OffsetDateTime,
}

impl EdgeLearningLedger {
    /// Get the weight multiplier for a given edge. Range: [0.5, 1.5].
    /// Returns 1.0 (neutral) if no learning data exists.
    pub fn weight_multiplier(&self, key: &EdgeKey) -> Decimal {
        self.entries
            .get(key)
            .map(|credit| {
                Decimal::ONE
                    + credit
                        .mean_credit
                        .clamp(Decimal::new(-5, 1), Decimal::new(5, 1))
            })
            .unwrap_or(Decimal::ONE)
    }

    /// Get or create a mutable reference to an edge credit entry.
    pub fn entry_mut_or_insert(&mut self, key: &EdgeKey, now: OffsetDateTime) -> &mut EdgeCredit {
        self.entries.entry(key.clone()).or_insert(EdgeCredit {
            total_credit: Decimal::ZERO,
            sample_count: 0,
            mean_credit: Decimal::ZERO,
            last_updated: now,
        })
    }

    /// Credit edges based on a resolved outcome's convergence detail.
    ///
    /// Identifies the dominant convergence component and distributes credit
    /// to the corresponding edge type.
    pub fn credit_from_outcome(
        &mut self,
        _symbol: &Symbol,
        net_return: Decimal,
        detail: &ConvergenceDetail,
        now: OffsetDateTime,
        inst_edge_keys: &[EdgeKey],
        stock_edge_keys: &[EdgeKey],
        sector_edge_key: Option<&EdgeKey>,
    ) {
        let inst_abs = detail.institutional_alignment.abs();
        let sector_abs = detail
            .sector_coherence
            .map(|v| v.abs())
            .unwrap_or(Decimal::ZERO);
        let cross_abs = detail.cross_stock_correlation.abs();
        let total_abs = inst_abs + sector_abs + cross_abs;

        if total_abs == Decimal::ZERO {
            return;
        }

        let (target_keys, contribution_ratio): (Vec<EdgeKey>, _) =
            if inst_abs >= sector_abs && inst_abs >= cross_abs {
                (inst_edge_keys.to_vec(), inst_abs / total_abs)
            } else if cross_abs >= inst_abs && cross_abs >= sector_abs {
                (stock_edge_keys.to_vec(), cross_abs / total_abs)
            } else {
                (
                    sector_edge_key.cloned().into_iter().collect(),
                    sector_abs / total_abs,
                )
            };

        let credit = net_return * contribution_ratio;
        for key in target_keys {
            let entry = self.entries.entry(key).or_insert(EdgeCredit {
                total_credit: Decimal::ZERO,
                sample_count: 0,
                mean_credit: Decimal::ZERO,
                last_updated: now,
            });
            entry.total_credit += credit;
            entry.sample_count += 1;
            entry.mean_credit = entry.total_credit / Decimal::from(entry.sample_count);
            entry.last_updated = now;
        }
    }

    const MAX_ENTRIES: usize = 50_000;

    /// Decay stale entries. Entries older than 7 days get credit reduced by 5%.
    /// Entries with negligible credit are removed. Hard-capped at 50k entries.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::days(7);
        for entry in self.entries.values_mut() {
            if entry.last_updated < cutoff {
                entry.total_credit *= Decimal::new(95, 2);
                entry.mean_credit = if entry.sample_count > 0 {
                    entry.total_credit / Decimal::from(entry.sample_count)
                } else {
                    Decimal::ZERO
                };
            }
        }
        self.entries
            .retain(|_, entry| entry.total_credit.abs() >= Decimal::new(1, 3));

        // Hard cap: evict lowest-credit entries if over limit
        if self.entries.len() > Self::MAX_ENTRIES {
            let mut entries: Vec<_> = self.entries.drain().collect();
            entries.sort_by(|a, b| {
                b.1.total_credit
                    .abs()
                    .partial_cmp(&a.1.total_credit.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            entries.truncate(Self::MAX_ENTRIES);
            self.entries = entries.into_iter().collect();
        }
    }

    /// Number of edges with learning data.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Build edge keys for a symbol's edges in the BrainGraph.
pub fn edge_keys_for_symbol(
    symbol: &Symbol,
    brain: &crate::graph::graph::BrainGraph,
) -> (Vec<EdgeKey>, Vec<EdgeKey>, Option<EdgeKey>) {
    use crate::graph::graph::{EdgeKind, NodeKind};
    use petgraph::visit::EdgeRef;
    use petgraph::Direction as GraphDirection;

    let Some(&stock_idx) = brain.stock_nodes.get(symbol) else {
        return (vec![], vec![], None);
    };

    let mut inst_keys = Vec::new();
    let mut stock_keys = Vec::new();
    let mut sector_key = None;

    for edge in brain
        .graph
        .edges_directed(stock_idx, GraphDirection::Incoming)
    {
        if let EdgeKind::InstitutionToStock(_) = edge.weight() {
            let source = edge.source();
            if let NodeKind::Institution(inst) = &brain.graph[source] {
                inst_keys.push(EdgeKey::InstitutionToStock {
                    institution_id: inst.institution_id,
                    symbol: symbol.clone(),
                });
            }
        }
    }

    for edge in brain
        .graph
        .edges_directed(stock_idx, GraphDirection::Outgoing)
    {
        match edge.weight() {
            EdgeKind::StockToStock(_) => {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    let (a, b) = if symbol.0 < neighbor.symbol.0 {
                        (symbol.clone(), neighbor.symbol.clone())
                    } else {
                        (neighbor.symbol.clone(), symbol.clone())
                    };
                    stock_keys.push(EdgeKey::StockToStock { a, b });
                }
            }
            EdgeKind::StockToSector(_) => {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_key = Some(EdgeKey::StockToSector {
                        symbol: symbol.clone(),
                        sector_id: s.sector_id.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    (inst_keys, stock_keys, sector_key)
}

/// Build edge keys for a symbol's edges in the US graph.
/// US currently learns only stock↔stock and stock→sector topology.
pub fn us_edge_keys_for_symbol(
    symbol: &Symbol,
    graph: &crate::us::graph::graph::UsGraph,
) -> (Vec<EdgeKey>, Option<EdgeKey>) {
    use crate::us::graph::graph::{UsEdgeKind, UsNodeKind};
    use petgraph::visit::EdgeRef;
    use petgraph::Direction as GraphDirection;

    let Some(&stock_idx) = graph.stock_nodes.get(symbol) else {
        return (vec![], None);
    };

    let mut stock_keys = std::collections::HashSet::new();
    let mut sector_key = None;

    for edge in graph
        .graph
        .edges_directed(stock_idx, GraphDirection::Outgoing)
    {
        match edge.weight() {
            UsEdgeKind::StockToStock(_) => {
                let target = edge.target();
                if let UsNodeKind::Stock(neighbor) = &graph.graph[target] {
                    let (a, b) = if symbol.0 < neighbor.symbol.0 {
                        (symbol.clone(), neighbor.symbol.clone())
                    } else {
                        (neighbor.symbol.clone(), symbol.clone())
                    };
                    stock_keys.insert(EdgeKey::StockToStock { a, b });
                }
            }
            UsEdgeKind::StockToSector(_) => {
                let target = edge.target();
                if let UsNodeKind::Sector(sector) = &graph.graph[target] {
                    sector_key = Some(EdgeKey::StockToSector {
                        symbol: symbol.clone(),
                        sector_id: sector.sector_id.clone(),
                    });
                }
            }
            UsEdgeKind::CrossMarket(_) => {}
        }
    }

    (stock_keys.into_iter().collect(), sector_key)
}

/// Replay resolved US topology outcomes from tick history into the edge ledger.
/// Returns the number of newly credited setups.
pub fn ingest_us_topology_outcomes(
    ledger: &mut EdgeLearningLedger,
    seen_setup_ids: &mut std::collections::HashSet<String>,
    history: &crate::us::temporal::buffer::UsTickHistory,
    graph: &crate::us::graph::graph::UsGraph,
    resolution_lag: u64,
    now: OffsetDateTime,
) -> usize {
    let mut credited = 0usize;
    for outcome in
        crate::us::temporal::lineage::compute_us_resolved_topology_outcomes(history, resolution_lag)
    {
        if !seen_setup_ids.insert(outcome.setup_id.clone()) {
            continue;
        }
        let (stock_edge_keys, sector_edge_key) = us_edge_keys_for_symbol(&outcome.symbol, graph);
        let mut learning_detail = outcome.convergence_detail.clone();
        // US currently learns stock/sector topology only; there is no institution-edge analogue.
        learning_detail.institutional_alignment = Decimal::ZERO;
        ledger.credit_from_outcome(
            &outcome.symbol,
            outcome.net_return,
            &learning_detail,
            now,
            &[],
            &stock_edge_keys,
            sector_edge_key.as_ref(),
        );
        credited += 1;
    }
    credited
}

/// Replay resolved HK topology outcomes from tick history into the edge ledger.
/// Returns the number of newly credited setups.
pub fn ingest_hk_topology_outcomes(
    ledger: &mut EdgeLearningLedger,
    seen_setup_ids: &mut std::collections::HashSet<String>,
    history: &crate::temporal::buffer::TickHistory,
    brain: &crate::graph::graph::BrainGraph,
    resolution_lag: u64,
    now: OffsetDateTime,
) -> usize {
    let mut credited = 0usize;
    for outcome in crate::temporal::lineage::compute_resolved_topology_outcomes(history, resolution_lag)
    {
        if !seen_setup_ids.insert(outcome.setup_id.clone()) {
            continue;
        }
        let (inst_edge_keys, stock_edge_keys, sector_edge_key) =
            edge_keys_for_symbol(&outcome.symbol, brain);
        ledger.credit_from_outcome(
            &outcome.symbol,
            outcome.net_return,
            &outcome.convergence_detail,
            now,
            &inst_edge_keys,
            &stock_edge_keys,
            sector_edge_key.as_ref(),
        );
        credited += 1;
    }
    credited
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn make_detail(inst: Decimal, sector: Decimal, cross: Decimal) -> ConvergenceDetail {
        ConvergenceDetail {
            institutional_alignment: inst,
            sector_coherence: Some(sector),
            cross_stock_correlation: cross,
            component_spread: None,
            edge_stability: None,
        }
    }

    fn test_inst_key() -> EdgeKey {
        EdgeKey::InstitutionToStock {
            institution_id: crate::ontology::objects::InstitutionId(1),
            symbol: Symbol("700.HK".into()),
        }
    }

    fn test_stock_key() -> EdgeKey {
        EdgeKey::StockToStock {
            a: Symbol("388.HK".into()),
            b: Symbol("700.HK".into()),
        }
    }

    fn test_sector_key() -> EdgeKey {
        EdgeKey::StockToSector {
            symbol: Symbol("700.HK".into()),
            sector_id: crate::ontology::objects::SectorId("tech".into()),
        }
    }

    #[test]
    fn credit_attribution_selects_dominant_component() {
        let mut ledger = EdgeLearningLedger::default();
        let symbol = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();
        let detail = make_detail(dec!(0.6), dec!(0.2), dec!(0.1));
        ledger.credit_from_outcome(
            &symbol,
            dec!(0.05),
            &detail,
            now,
            &[test_inst_key()],
            &[test_stock_key()],
            Some(&test_sector_key()),
        );
        assert!(ledger.weight_multiplier(&test_inst_key()) > Decimal::ONE);
        assert_eq!(ledger.weight_multiplier(&test_stock_key()), Decimal::ONE);
        assert_eq!(ledger.weight_multiplier(&test_sector_key()), Decimal::ONE);
    }

    #[test]
    fn weight_multiplier_positive_credit_amplifies() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            test_inst_key(),
            EdgeCredit {
                total_credit: dec!(0.3),
                sample_count: 1,
                mean_credit: dec!(0.3),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier(&test_inst_key()), dec!(1.3));
    }

    #[test]
    fn weight_multiplier_negative_credit_dampens() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            test_inst_key(),
            EdgeCredit {
                total_credit: dec!(-0.3),
                sample_count: 1,
                mean_credit: dec!(-0.3),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier(&test_inst_key()), dec!(0.7));
    }

    #[test]
    fn weight_multiplier_capped_at_50_pct() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            test_inst_key(),
            EdgeCredit {
                total_credit: dec!(0.9),
                sample_count: 1,
                mean_credit: dec!(0.9),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier(&test_inst_key()), dec!(1.5));
    }

    #[test]
    fn decay_reduces_stale_entries() {
        let mut ledger = EdgeLearningLedger::default();
        let now = OffsetDateTime::now_utc();
        let old = now - time::Duration::days(8);
        ledger.entries.insert(
            test_inst_key(),
            EdgeCredit {
                total_credit: dec!(0.10),
                sample_count: 1,
                mean_credit: dec!(0.10),
                last_updated: old,
            },
        );
        ledger.decay(now);
        assert!(ledger.weight_multiplier(&test_inst_key()) < dec!(1.10));
    }

    #[test]
    fn no_learning_data_returns_neutral() {
        let ledger = EdgeLearningLedger::default();
        assert_eq!(ledger.weight_multiplier(&test_inst_key()), Decimal::ONE);
    }

    #[test]
    fn ingest_us_topology_outcomes_credits_sector_edge() {
        use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
        use crate::ontology::reasoning::{DecisionLineage, ReasoningScope, TacticalSetup};
        use crate::us::graph::decision::UsMarketRegimeBias;
        use crate::us::graph::graph::UsGraph;
        use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
        use crate::us::temporal::buffer::UsTickHistory;
        use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};

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

        fn make_signal(mark_price: Decimal) -> UsSymbolSignals {
            UsSymbolSignals {
                mark_price: Some(mark_price),
                composite: dec!(0.3),
                composite_delta: Decimal::ZERO,
                composite_acceleration: Decimal::ZERO,
                capital_flow_direction: Decimal::ZERO,
                capital_flow_delta: Decimal::ZERO,
                flow_persistence: 0,
                flow_reversal: false,
                price_momentum: Decimal::ZERO,
                volume_profile: Decimal::ZERO,
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                pre_market_delta: Decimal::ZERO,
            }
        }

        let timestamp = OffsetDateTime::UNIX_EPOCH;
        let dims = UsDimensionSnapshot {
            timestamp,
            dimensions: HashMap::from([(
                Symbol("AAPL.US".into()),
                make_dims(dec!(0.2), dec!(0.3), dec!(0.1), dec!(0.0), dec!(0.1)),
            )]),
        };
        let sector_map = HashMap::from([(
            Symbol("AAPL.US".into()),
            crate::ontology::objects::SectorId("tech".into()),
        )]);
        let graph = UsGraph::compute(&dims, &sector_map, &HashMap::new());

        let setup = TacticalSetup {
            setup_id: "setup:AAPL.US:review".into(),
            hypothesis_id: "hyp:AAPL.US:latent_vortex".into(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("AAPL.US".into())),
            title: "Long AAPL.US".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.5),
            confidence_gap: dec!(0.1),
            heuristic_edge: dec!(0.2),
            convergence_score: Some(dec!(0.3)),
            convergence_detail: Some(ConvergenceDetail {
                institutional_alignment: dec!(0.3),
                sector_coherence: Some(dec!(0.4)),
                cross_stock_correlation: dec!(0.1),
                component_spread: None,
                edge_stability: None,
            }),
            workflow_id: None,
            entry_rationale: "test".into(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        };

        let mut history = UsTickHistory::new(10);
        history.push(UsTickRecord {
            tick_number: 1,
            timestamp,
            signals: HashMap::from([(Symbol("AAPL.US".into()), make_signal(dec!(100)))]),
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![setup],
            market_regime: UsMarketRegimeBias::Neutral,
        });
        history.push(UsTickRecord {
            tick_number: 6,
            timestamp,
            signals: HashMap::from([(Symbol("AAPL.US".into()), make_signal(dec!(110)))]),
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: UsMarketRegimeBias::Neutral,
        });

        let mut ledger = EdgeLearningLedger::default();
        let mut seen = std::collections::HashSet::new();
        let credited =
            ingest_us_topology_outcomes(&mut ledger, &mut seen, &history, &graph, 5, timestamp);
        assert_eq!(credited, 1);
        let sector_key = EdgeKey::StockToSector {
            symbol: Symbol("AAPL.US".into()),
            sector_id: crate::ontology::objects::SectorId("tech".into()),
        };
        assert!(ledger.weight_multiplier(&sector_key) > Decimal::ONE);
    }

}
