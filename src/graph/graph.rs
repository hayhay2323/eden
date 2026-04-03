use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::action::narrative::{NarrativeSnapshot, Regime};
use crate::math::{cosine_similarity, jaccard, median, normalized_ratio};
use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::links::{LinkSnapshot, MarketTemperatureObservation};
use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::ontology::store::ObjectStore;
use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};

// ── Node types ──

#[derive(Debug, Clone)]
pub struct StockNode {
    pub symbol: Symbol,
    pub regime: Regime,
    pub coherence: Decimal,
    pub mean_direction: Decimal,
    pub dimensions: SymbolDimensions,
}

#[derive(Debug, Clone)]
pub struct InstitutionNode {
    pub institution_id: InstitutionId,
    pub stock_count: usize,
    pub bid_stock_count: usize,
    pub ask_stock_count: usize,
    pub net_direction: Decimal,
}

#[derive(Debug, Clone)]
pub struct SectorNode {
    pub sector_id: SectorId,
    pub stock_count: usize,
    pub mean_coherence: Decimal,
    pub mean_direction: Decimal,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Stock(StockNode),
    Institution(InstitutionNode),
    Sector(SectorNode),
}

// ── Edge types ──

#[derive(Debug, Clone)]
pub struct InstitutionToStock {
    pub direction: Decimal,
    pub seat_count: usize,
    pub timestamp: OffsetDateTime,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone)]
pub struct StockToSector {
    /// Relative weight of this stock within its sector (e.g. by turnover).
    /// Used to weight the stock's contribution to sector-level aggregates.
    pub weight: Decimal,
    pub timestamp: OffsetDateTime,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone)]
pub struct StockToStock {
    pub similarity: Decimal,
    pub timestamp: OffsetDateTime,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone)]
pub struct InstitutionToInstitution {
    pub jaccard: Decimal,
    pub timestamp: OffsetDateTime,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone)]
pub enum EdgeKind {
    InstitutionToStock(InstitutionToStock),
    StockToSector(StockToSector),
    StockToStock(StockToStock),
    InstitutionToInstitution(InstitutionToInstitution),
}

// ── BrainGraph ──

#[derive(Debug)]
pub struct BrainGraph {
    pub timestamp: OffsetDateTime,
    pub graph: DiGraph<NodeKind, EdgeKind>,
    pub market_temperature: Option<MarketTemperatureObservation>,
    pub stock_nodes: HashMap<Symbol, NodeIndex>,
    pub institution_nodes: HashMap<InstitutionId, NodeIndex>,
    pub sector_nodes: HashMap<SectorId, NodeIndex>,
}

impl BrainGraph {
    /// Build the full multi-entity graph from narrative, dimension, and link data.
    pub fn compute(
        narrative: &NarrativeSnapshot,
        dimensions: &DimensionSnapshot,
        links: &LinkSnapshot,
        store: &ObjectStore,
    ) -> Self {
        let mut graph = DiGraph::new();
        let mut stock_nodes = HashMap::new();
        let mut institution_nodes = HashMap::new();
        let mut sector_nodes = HashMap::new();

        // 1. Create stock nodes from NarrativeSnapshot + DimensionSnapshot
        for (sym, narr) in &narrative.narratives {
            let dims = dimensions.dimensions.get(sym).cloned().unwrap_or_default();
            let node = StockNode {
                symbol: sym.clone(),
                regime: narr.regime,
                coherence: narr.coherence,
                mean_direction: narr.mean_direction,
                dimensions: dims,
            };
            let idx = graph.add_node(NodeKind::Stock(node));
            stock_nodes.insert(sym.clone(), idx);
        }

        // 2. Create sector nodes, compute mean_coherence/mean_direction, add stock→sector edges
        for sector in store.sectors.values() {
            let member_stocks: Vec<&Symbol> = stock_nodes
                .keys()
                .filter(|sym| {
                    store.stocks.get(*sym).and_then(|s| s.sector_id.as_ref()) == Some(&sector.id)
                })
                .collect();

            if member_stocks.is_empty() {
                continue;
            }

            let stock_count = member_stocks.len();
            let mut total_coherence = Decimal::ZERO;
            let mut total_direction = Decimal::ZERO;
            for sym in &member_stocks {
                if let Some(narr) = narrative.narratives.get(*sym) {
                    total_coherence += narr.coherence;
                    total_direction += narr.mean_direction;
                }
            }
            let count_dec = Decimal::from(stock_count as i64);
            let sector_node = SectorNode {
                sector_id: sector.id.clone(),
                stock_count,
                mean_coherence: total_coherence / count_dec,
                mean_direction: total_direction / count_dec,
            };
            let sector_idx = graph.add_node(NodeKind::Sector(sector_node));
            sector_nodes.insert(sector.id.clone(), sector_idx);

            // Add stock→sector edges, weighted by relative turnover within the sector.
            let turnovers: Vec<_> = member_stocks
                .iter()
                .filter_map(|sym| {
                    stock_nodes.get(*sym)?;
                    let t = links
                        .quotes
                        .iter()
                        .find(|q| &q.symbol == *sym)
                        .map(|q| q.turnover)
                        .unwrap_or(Decimal::ZERO);
                    Some((*sym, t))
                })
                .collect();
            let total_turnover: Decimal = turnovers.iter().map(|(_, t)| *t).sum();
            for (sym, t) in &turnovers {
                if let Some(&stock_idx) = stock_nodes.get(*sym) {
                    let weight = if total_turnover > Decimal::ZERO {
                        *t / total_turnover
                    } else {
                        Decimal::ONE / Decimal::from(turnovers.len().max(1) as i64)
                    };
                    graph.add_edge(
                        stock_idx,
                        sector_idx,
                        EdgeKind::StockToSector(StockToSector {
                            weight,
                            timestamp: links.timestamp,
                            provenance: computed_edge_provenance(
                                links.timestamp,
                                Decimal::ONE,
                                [format!("sector_membership:{}", sym)],
                            ),
                        }),
                    );
                }
            }
        }

        // 3. Create institution nodes from CrossStockPresence (institutions in ≥2 stocks)
        // Build institution → stock sets for Jaccard later
        let mut inst_stock_sets: HashMap<InstitutionId, HashSet<Symbol>> = HashMap::new();
        for csp in &links.cross_stock_presences {
            let stock_set: HashSet<Symbol> = csp.symbols.iter().cloned().collect();
            let node = InstitutionNode {
                institution_id: csp.institution_id,
                stock_count: csp.symbols.len(),
                bid_stock_count: csp.bid_symbols.len(),
                ask_stock_count: csp.ask_symbols.len(),
                net_direction: normalized_ratio(
                    Decimal::from(csp.bid_symbols.len() as i64),
                    Decimal::from(csp.ask_symbols.len() as i64),
                ),
            };
            let idx = graph.add_node(NodeKind::Institution(node));
            institution_nodes.insert(csp.institution_id, idx);
            inst_stock_sets.insert(csp.institution_id, stock_set);
        }

        // 4. Add institution→stock edges from InstitutionActivity
        let knowledge = store.knowledge_read();
        for act in &links.institution_activities {
            if let (Some(&inst_idx), Some(&stock_idx)) = (
                institution_nodes.get(&act.institution_id),
                stock_nodes.get(&act.symbol),
            ) {
                let bid = Decimal::from(act.bid_positions.len() as i64);
                let ask = Decimal::from(act.ask_positions.len() as i64);
                let direction = normalized_ratio(bid, ask);
                let history_bonus =
                    knowledge.institution_history_bonus(&act.institution_id, &act.symbol);
                let base_confidence = direction.abs();
                let adjusted_confidence =
                    crate::math::clamp_unit_interval(base_confidence + history_bonus);
                graph.add_edge(
                    inst_idx,
                    stock_idx,
                    EdgeKind::InstitutionToStock(InstitutionToStock {
                        direction,
                        seat_count: act.seat_count,
                        timestamp: links.timestamp,
                        provenance: computed_edge_provenance(
                            links.timestamp,
                            adjusted_confidence,
                            [
                                format!("institution_activity:{}", act.symbol),
                                format!("institution:{}", act.institution_id),
                            ],
                        ),
                    }),
                );
            }
        }
        drop(knowledge);

        // 5. Add stock↔stock edges (cosine similarity, filtered by median)
        let stock_syms: Vec<Symbol> = stock_nodes.keys().cloned().collect();

        // First pass: compute all similarities
        let mut all_pairs: Vec<(usize, usize, Decimal)> = Vec::new();
        for i in 0..stock_syms.len() {
            for j in (i + 1)..stock_syms.len() {
                let sym_a = &stock_syms[i];
                let sym_b = &stock_syms[j];
                if let (Some(dims_a), Some(dims_b)) = (
                    dimensions.dimensions.get(sym_a),
                    dimensions.dimensions.get(sym_b),
                ) {
                    let vec_a = dims_to_array(dims_a);
                    let vec_b = dims_to_array(dims_b);
                    if vec_a.iter().all(|v| *v == Decimal::ZERO)
                        || vec_b.iter().all(|v| *v == Decimal::ZERO)
                    {
                        continue;
                    }
                    let similarity = cosine_similarity(vec_a, vec_b);
                    all_pairs.push((i, j, similarity));
                }
            }
        }

        // Compute median absolute similarity as data-derived cutoff.
        // Keep only pairs strictly above the median to align with the graph insights filters.
        let median_cutoff =
            median(all_pairs.iter().map(|(_, _, s)| s.abs()).collect()).unwrap_or(Decimal::ZERO);

        // Second pass: only create edges above the median
        for (i, j, similarity) in &all_pairs {
            if similarity.abs() <= median_cutoff {
                continue;
            }
            let &idx_a = stock_nodes.get(&stock_syms[*i]).unwrap();
            let &idx_b = stock_nodes.get(&stock_syms[*j]).unwrap();
            graph.add_edge(
                idx_a,
                idx_b,
                EdgeKind::StockToStock(StockToStock {
                    similarity: *similarity,
                    timestamp: links.timestamp,
                    provenance: computed_edge_provenance(
                        links.timestamp,
                        similarity.abs(),
                        [
                            format!("dimension_similarity:{}", stock_syms[*i]),
                            format!("dimension_similarity:{}", stock_syms[*j]),
                        ],
                    ),
                }),
            );
            graph.add_edge(
                idx_b,
                idx_a,
                EdgeKind::StockToStock(StockToStock {
                    similarity: *similarity,
                    timestamp: links.timestamp,
                    provenance: computed_edge_provenance(
                        links.timestamp,
                        similarity.abs(),
                        [
                            format!("dimension_similarity:{}", stock_syms[*j]),
                            format!("dimension_similarity:{}", stock_syms[*i]),
                        ],
                    ),
                }),
            );
        }

        // 6. Add institution↔institution edges (Jaccard of stock sets, skip disjoint)
        let inst_ids: Vec<InstitutionId> = institution_nodes.keys().copied().collect();
        for i in 0..inst_ids.len() {
            for j in (i + 1)..inst_ids.len() {
                let id_a = &inst_ids[i];
                let id_b = &inst_ids[j];
                if let (Some(set_a), Some(set_b)) =
                    (inst_stock_sets.get(id_a), inst_stock_sets.get(id_b))
                {
                    let j_coeff = jaccard(set_a, set_b);
                    if j_coeff == Decimal::ZERO {
                        continue;
                    }
                    let &idx_a = institution_nodes.get(id_a).unwrap();
                    let &idx_b = institution_nodes.get(id_b).unwrap();
                    graph.add_edge(
                        idx_a,
                        idx_b,
                        EdgeKind::InstitutionToInstitution(InstitutionToInstitution {
                            jaccard: j_coeff,
                            timestamp: links.timestamp,
                            provenance: computed_edge_provenance(
                                links.timestamp,
                                j_coeff,
                                [
                                    format!("institution_overlap:{}", id_a),
                                    format!("institution_overlap:{}", id_b),
                                ],
                            ),
                        }),
                    );
                    graph.add_edge(
                        idx_b,
                        idx_a,
                        EdgeKind::InstitutionToInstitution(InstitutionToInstitution {
                            jaccard: j_coeff,
                            timestamp: links.timestamp,
                            provenance: computed_edge_provenance(
                                links.timestamp,
                                j_coeff,
                                [
                                    format!("institution_overlap:{}", id_b),
                                    format!("institution_overlap:{}", id_a),
                                ],
                            ),
                        }),
                    );
                }
            }
        }

        BrainGraph {
            timestamp: narrative.timestamp,
            graph,
            market_temperature: links.market_temperature.clone(),
            stock_nodes,
            institution_nodes,
            sector_nodes,
        }
    }
}

fn computed_edge_provenance<I, S>(
    observed_at: OffsetDateTime,
    confidence: Decimal,
    inputs: I,
) -> ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at)
        .with_confidence(confidence.clamp(Decimal::ZERO, Decimal::ONE))
        .with_inputs(inputs)
}

/// Convert SymbolDimensions to an array for cosine similarity.
pub fn dims_to_array(d: &SymbolDimensions) -> [Decimal; 8] {
    [
        d.order_book_pressure,
        d.capital_flow_direction,
        d.capital_size_divergence,
        d.institutional_direction,
        d.depth_structure_imbalance,
        d.valuation_support,
        d.activity_momentum,
        d.candlestick_conviction,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::narrative::{
        DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
    };
    use crate::logic::tension::Dimension;
    use crate::ontology::links::*;
    use crate::ontology::objects::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_store(stocks: Vec<Stock>, sectors: Vec<Sector>) -> ObjectStore {
        let mut stock_map = HashMap::new();
        for s in stocks {
            stock_map.insert(s.symbol.clone(), s);
        }
        let mut sector_map = HashMap::new();
        for s in sectors {
            sector_map.insert(s.id.clone(), s);
        }
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: HashMap::new(),
            knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
        }
    }

    fn make_stock(symbol: &str, sector: Option<&str>) -> Stock {
        let symbol_id = sym(symbol);
        Stock {
            market: symbol_id.market(),
            symbol: symbol_id,
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: sector.map(|s| SectorId(s.into())),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: rust_decimal::Decimal::ZERO,
            bps: rust_decimal::Decimal::ZERO,
            dividend_yield: rust_decimal::Decimal::ZERO,
        }
    }

    fn make_narrative(coherence: Decimal, mean_direction: Decimal) -> SymbolNarrative {
        SymbolNarrative {
            regime: Regime::classify(coherence, mean_direction),
            coherence,
            mean_direction,
            readings: vec![DimensionReading {
                dimension: Dimension::OrderBookPressure,
                value: mean_direction,
                direction: Direction::from_value(mean_direction),
            }],
            agreements: vec![],
            contradictions: vec![],
        }
    }

    fn make_dims(obp: Decimal, cfd: Decimal, csd: Decimal, id: Decimal) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: obp,
            capital_flow_direction: cfd,
            capital_size_divergence: csd,
            institutional_direction: id,
            ..Default::default()
        }
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
        }
    }

    // ── Tests ──

    #[test]
    fn empty_graph() {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives: HashMap::new(),
        };
        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
        assert_eq!(brain.graph.node_count(), 0);
        assert_eq!(brain.graph.edge_count(), 0);
    }

    #[test]
    fn stock_node_creation() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
        assert_eq!(brain.stock_nodes.len(), 1);
        assert!(brain.stock_nodes.contains_key(&sym("700.HK")));

        let idx = brain.stock_nodes[&sym("700.HK")];
        match &brain.graph[idx] {
            NodeKind::Stock(s) => {
                assert_eq!(s.symbol, sym("700.HK"));
                assert_eq!(s.regime, Regime::CoherentBullish);
            }
            _ => panic!("expected StockNode"),
        }
    }

    #[test]
    fn sector_aggregation() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.6), dec!(0.4)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.2), dec!(0.2)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        let links = empty_links();
        let store = make_store(
            vec![
                make_stock("700.HK", Some("tech")),
                make_stock("9988.HK", Some("tech")),
            ],
            vec![Sector {
                id: SectorId("tech".into()),
                name: "Technology".into(),
            }],
        );

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
        assert_eq!(brain.sector_nodes.len(), 1);

        let sector_idx = brain.sector_nodes[&SectorId("tech".into())];
        match &brain.graph[sector_idx] {
            NodeKind::Sector(s) => {
                assert_eq!(s.stock_count, 2);
                // mean_coherence = (0.6 + 0.2) / 2 = 0.4
                assert_eq!(s.mean_coherence, dec!(0.4));
                // mean_direction = (0.4 + 0.2) / 2 = 0.3
                assert_eq!(s.mean_direction, dec!(0.3));
            }
            _ => panic!("expected SectorNode"),
        }

        // Should have 2 stock→sector edges
        let edge_count = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::StockToSector(_)))
            .count();
        assert_eq!(edge_count, 2);
        for edge_idx in brain.graph.edge_indices() {
            if let EdgeKind::StockToSector(edge) = &brain.graph[edge_idx] {
                assert_eq!(edge.timestamp, OffsetDateTime::UNIX_EPOCH);
            }
        }
    }

    #[test]
    fn institution_edges() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![sym("700.HK")],
            bid_symbols: vec![sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![1],
            seat_count: 3,
        });

        let store = make_store(vec![], vec![]);
        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        assert_eq!(brain.institution_nodes.len(), 1);

        // Should have institution→stock edge for 700.HK
        let inst_stock_edges: Vec<_> = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::InstitutionToStock(_)))
            .collect();
        assert_eq!(inst_stock_edges.len(), 1);

        // Direction: bid=1, ask=2 → (1-2)/(1+2) = -1/3
        match &brain.graph[inst_stock_edges[0]] {
            EdgeKind::InstitutionToStock(e) => {
                let expected = Decimal::from(-1) / Decimal::from(3);
                assert_eq!(e.direction.round_dp(10), expected.round_dp(10));
                assert_eq!(e.seat_count, 3);
                assert_eq!(e.timestamp, OffsetDateTime::UNIX_EPOCH);
            }
            _ => panic!("expected InstitutionToStock"),
        }
    }

    #[test]
    fn cosine_similarity_edges() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));
        narratives.insert(sym("388.HK"), make_narrative(dec!(0.2), dec!(-0.1)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        // Similar vectors
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );
        dimensions.insert(
            sym("388.HK"),
            make_dims(dec!(0.5), dec!(-0.5), dec!(0.5), dec!(-0.5)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        // Should have 2 stock↔stock edges (bidirectional)
        let ss_edges: Vec<_> = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::StockToStock(_)))
            .collect();
        assert_eq!(ss_edges.len(), 2);

        // Parallel vectors → similarity ≈ 1.0
        match &brain.graph[ss_edges[0]] {
            EdgeKind::StockToStock(e) => {
                assert!((e.similarity - Decimal::ONE).abs() < dec!(0.001));
            }
            _ => panic!("expected StockToStock"),
        }
    }

    #[test]
    fn jaccard_edges() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));
        narratives.insert(sym("3690.HK"), make_narrative(dec!(0.4), dec!(0.2)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );
        dimensions.insert(
            sym("3690.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };

        let mut links = empty_links();
        // Institution A: in 700, 9988
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        // Institution B: in 9988, 3690
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(200),
            symbols: vec![sym("9988.HK"), sym("3690.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("9988.HK"), sym("3690.HK")],
        });

        let store = make_store(vec![], vec![]);
        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        // Jaccard({700,9988}, {9988,3690}) = 1/3
        let ii_edges: Vec<_> = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::InstitutionToInstitution(_)))
            .collect();
        assert_eq!(ii_edges.len(), 2); // bidirectional

        match &brain.graph[ii_edges[0]] {
            EdgeKind::InstitutionToInstitution(e) => {
                let expected = Decimal::ONE / Decimal::from(3);
                assert_eq!(e.jaccard.round_dp(10), expected.round_dp(10));
            }
            _ => panic!("expected InstitutionToInstitution"),
        }
    }

    #[test]
    fn no_self_edges() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        // No stock↔stock self-edges (only 1 stock, so no pairs)
        let ss_edges = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::StockToStock(_)))
            .count();
        assert_eq!(ss_edges, 0);
    }

    #[test]
    fn skip_all_zero_vectors() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        // All-zero vector
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0), dec!(0), dec!(0), dec!(0)),
        );

        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);

        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        let ss_edges = brain
            .graph
            .edge_indices()
            .filter(|e| matches!(brain.graph[*e], EdgeKind::StockToStock(_)))
            .count();
        assert_eq!(ss_edges, 0);
    }
}
