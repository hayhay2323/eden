use std::collections::HashMap;

use crate::math::{cosine_similarity, median};
use crate::ontology::objects::{SectorId, Symbol};
use crate::us::common::dimension_composite;
use petgraph::graph::{DiGraph, NodeIndex};
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
use crate::us::watchlist::CROSS_MARKET_PAIRS;

// ── Node types ──

#[derive(Debug, Clone)]
pub struct UsStockNode {
    pub symbol: Symbol,
    pub mean_direction: Decimal,
    pub dimensions: UsSymbolDimensions,
}

#[derive(Debug, Clone)]
pub struct UsSectorNode {
    pub sector_id: SectorId,
    pub stock_count: usize,
    pub mean_direction: Decimal,
}

#[derive(Debug, Clone)]
pub struct CrossMarketNode {
    pub us_symbol: Symbol,
    pub hk_symbol: Symbol,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum UsNodeKind {
    Stock(UsStockNode),
    Sector(UsSectorNode),
    CrossMarket(CrossMarketNode),
}

// ── Edge types ──

#[derive(Debug, Clone)]
pub struct UsStockToStock {
    pub similarity: Decimal,
    pub timestamp: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct UsStockToSector {
    pub timestamp: OffsetDateTime,
}

/// Edge connecting a US stock to its HK dual-listed counterpart.
/// Weight represents the signal propagation strength.
#[derive(Debug, Clone)]
pub struct CrossMarketEdge {
    pub us_symbol: Symbol,
    pub hk_symbol: Symbol,
    /// Propagation strength from HK to US (0 = no signal, 1 = full propagation).
    pub propagation_strength: Decimal,
    /// Directional signal from HK counterpart (positive = bullish, negative = bearish).
    pub direction: Decimal,
    /// Confidence in the cross-market relationship.
    pub confidence: Decimal,
    pub timestamp: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub enum UsEdgeKind {
    StockToStock(UsStockToStock),
    StockToSector(UsStockToSector),
    CrossMarket(CrossMarketEdge),
}

// ── US BrainGraph ──

#[derive(Debug)]
pub struct UsGraph {
    pub timestamp: OffsetDateTime,
    pub graph: DiGraph<UsNodeKind, UsEdgeKind>,
    pub stock_nodes: HashMap<Symbol, NodeIndex>,
    pub sector_nodes: HashMap<SectorId, NodeIndex>,
    pub cross_market_nodes: HashMap<Symbol, NodeIndex>, // keyed by US symbol
}

fn minimum_stock_edge_similarity() -> Decimal {
    Decimal::new(55, 2)
}

fn stock_relation_group(symbol: &Symbol, sector_map: &HashMap<Symbol, SectorId>) -> Option<String> {
    let sector = sector_map.get(symbol)?.0.as_str();
    if sector == "etf" {
        return Some(etf_relation_group(symbol).to_string());
    }
    Some(sector.to_string())
}

fn etf_relation_group(symbol: &Symbol) -> &'static str {
    match symbol.0.as_str() {
        "SPY.US" | "QQQ.US" | "IWM.US" | "DIA.US" => "etf_macro_us_equity",
        "EEM.US" | "EWJ.US" | "EFA.US" => "etf_macro_international",
        "TLT.US" | "HYG.US" | "LQD.US" | "VXX.US" => "etf_macro_rates_credit_vol",
        "GLD.US" | "SLV.US" | "USO.US" | "UNG.US" => "etf_macro_commodities",
        "XLF.US" => "etf_sector_finance",
        "XLK.US" => "etf_sector_tech",
        "XLE.US" => "etf_sector_energy",
        "XLV.US" => "etf_sector_healthcare",
        "XLI.US" => "etf_sector_industrial",
        "XLP.US" => "etf_sector_consumer_staples",
        "XLU.US" => "etf_sector_utilities",
        "XLY.US" => "etf_sector_consumer_discretionary",
        "XLB.US" => "etf_sector_materials",
        "XLRE.US" => "etf_sector_real_estate",
        "XLC.US" => "etf_sector_telecom_media",
        "SOXX.US" | "SMH.US" => "etf_theme_semiconductors",
        "KWEB.US" | "FXI.US" => "etf_theme_china",
        "ARKK.US" | "BOTZ.US" => "etf_theme_innovation",
        "XBI.US" => "etf_theme_biotech",
        "HACK.US" => "etf_theme_cybersecurity",
        "TAN.US" | "ICLN.US" => "etf_theme_clean_energy",
        "IBIT.US" | "BITO.US" => "etf_theme_crypto",
        _ => "etf_other",
    }
}

impl UsGraph {
    /// Build the US knowledge graph from dimension data and sector assignments.
    ///
    /// `sector_map` maps Symbol -> SectorId for stocks that have sector assignments.
    /// Cross-market edges are created for all dual-listed pairs present in stock_nodes.
    pub fn compute(
        dimensions: &UsDimensionSnapshot,
        sector_map: &HashMap<Symbol, SectorId>,
        sector_names: &HashMap<SectorId, String>,
    ) -> Self {
        let mut graph = DiGraph::new();
        let mut stock_nodes = HashMap::new();
        let mut sector_nodes = HashMap::new();
        let mut cross_market_nodes = HashMap::new();

        // 1. Create stock nodes
        for (sym, dims) in &dimensions.dimensions {
            let mean_direction = dimension_composite(dims);
            let node = UsStockNode {
                symbol: sym.clone(),
                mean_direction,
                dimensions: dims.clone(),
            };
            let idx = graph.add_node(UsNodeKind::Stock(node));
            stock_nodes.insert(sym.clone(), idx);
        }

        // 2. Create sector nodes + stock->sector edges
        let mut sector_members: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        for (sym, sector_id) in sector_map {
            if stock_nodes.contains_key(sym) {
                sector_members
                    .entry(sector_id.clone())
                    .or_default()
                    .push(sym.clone());
            }
        }

        for (sector_id, members) in &sector_members {
            let stock_count = members.len();
            let total_direction: Decimal = members
                .iter()
                .filter_map(|s| dimensions.dimensions.get(s))
                .map(dimension_composite)
                .sum();
            let mean_direction = total_direction / Decimal::from(stock_count.max(1) as i64);

            let name = sector_names.get(sector_id).cloned().unwrap_or_default();
            let _ = name; // name used in display, not stored in node

            let sector_node = UsSectorNode {
                sector_id: sector_id.clone(),
                stock_count,
                mean_direction,
            };
            let sector_idx = graph.add_node(UsNodeKind::Sector(sector_node));
            sector_nodes.insert(sector_id.clone(), sector_idx);

            for sym in members {
                if let Some(&stock_idx) = stock_nodes.get(sym) {
                    graph.add_edge(
                        stock_idx,
                        sector_idx,
                        UsEdgeKind::StockToSector(UsStockToSector {
                            timestamp: dimensions.timestamp,
                        }),
                    );
                }
            }
        }

        // 3. Stock-to-stock edges. We only connect symbols inside the same
        // economic relation group first, then keep the stronger positive
        // similarities within that group. Pure cross-market cosine matches
        // without a plausible relation are treated as noise.
        let stock_syms: Vec<Symbol> = stock_nodes.keys().cloned().collect();
        let mut grouped_pairs: HashMap<String, Vec<(usize, usize, Decimal)>> = HashMap::new();

        for i in 0..stock_syms.len() {
            for j in (i + 1)..stock_syms.len() {
                let Some(group_a) = stock_relation_group(&stock_syms[i], sector_map) else {
                    continue;
                };
                let Some(group_b) = stock_relation_group(&stock_syms[j], sector_map) else {
                    continue;
                };
                if group_a != group_b {
                    continue;
                }
                if let (Some(dims_a), Some(dims_b)) = (
                    dimensions.dimensions.get(&stock_syms[i]),
                    dimensions.dimensions.get(&stock_syms[j]),
                ) {
                    let vec_a = dims_to_array(dims_a);
                    let vec_b = dims_to_array(dims_b);
                    if vec_a.iter().all(|v| *v == Decimal::ZERO)
                        || vec_b.iter().all(|v| *v == Decimal::ZERO)
                    {
                        continue;
                    }
                    let similarity = cosine_similarity(vec_a, vec_b);
                    if similarity <= Decimal::ZERO {
                        continue;
                    }
                    grouped_pairs
                        .entry(group_a)
                        .or_default()
                        .push((i, j, similarity));
                }
            }
        }

        for pairs in grouped_pairs.values() {
            let median_cutoff =
                median(pairs.iter().map(|(_, _, s)| *s).collect()).unwrap_or(Decimal::ZERO);
            let cutoff = median_cutoff.max(minimum_stock_edge_similarity());

            for (i, j, similarity) in pairs {
                if *similarity < cutoff {
                    continue;
                }
                let &idx_a = stock_nodes.get(&stock_syms[*i]).unwrap();
                let &idx_b = stock_nodes.get(&stock_syms[*j]).unwrap();
                let edge = UsStockToStock {
                    similarity: *similarity,
                    timestamp: dimensions.timestamp,
                };
                graph.add_edge(idx_a, idx_b, UsEdgeKind::StockToStock(edge.clone()));
                graph.add_edge(idx_b, idx_a, UsEdgeKind::StockToStock(edge));
            }
        }

        // 4. Cross-market nodes + edges for dual-listed pairs
        for pair in CROSS_MARKET_PAIRS {
            let us_sym = Symbol(pair.us_symbol.to_string());
            if !stock_nodes.contains_key(&us_sym) {
                continue;
            }

            let hk_sym = Symbol(pair.hk_symbol.to_string());
            let cm_node = CrossMarketNode {
                us_symbol: us_sym.clone(),
                hk_symbol: hk_sym.clone(),
                name: pair.name.to_string(),
            };
            let cm_idx = graph.add_node(UsNodeKind::CrossMarket(cm_node));
            cross_market_nodes.insert(us_sym.clone(), cm_idx);

            // Bidirectional edge: US stock <-> cross-market node
            let &stock_idx = stock_nodes.get(&us_sym).unwrap();
            let edge = CrossMarketEdge {
                us_symbol: us_sym.clone(),
                hk_symbol: hk_sym.clone(),
                propagation_strength: Decimal::ZERO,
                direction: Decimal::ZERO,
                confidence: Decimal::ZERO,
                timestamp: dimensions.timestamp,
            };
            graph.add_edge(stock_idx, cm_idx, UsEdgeKind::CrossMarket(edge.clone()));
            graph.add_edge(cm_idx, stock_idx, UsEdgeKind::CrossMarket(edge));
        }

        UsGraph {
            timestamp: dimensions.timestamp,
            graph,
            stock_nodes,
            sector_nodes,
            cross_market_nodes,
        }
    }

    /// Find which HK symbol is linked to a US stock, if any.
    pub fn hk_counterpart(&self, us_symbol: &Symbol) -> Option<Symbol> {
        self.cross_market_nodes
            .get(us_symbol)
            .and_then(|&idx| match &self.graph[idx] {
                UsNodeKind::CrossMarket(cm) => Some(cm.hk_symbol.clone()),
                _ => None,
            })
    }
}

// ── Helpers ──

fn dims_to_array(dims: &UsSymbolDimensions) -> [Decimal; 5] {
    [
        dims.capital_flow_direction,
        dims.price_momentum,
        dims.volume_profile,
        dims.pre_post_market_anomaly,
        dims.valuation,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

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

    #[test]
    fn graph_creates_stock_nodes() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0), dec!(-0.1)),
            ),
            (
                sym("NVDA.US"),
                make_dims(dec!(0.5), dec!(0.8), dec!(0.4), dec!(0.1), dec!(0.2)),
            ),
        ]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        assert_eq!(g.stock_nodes.len(), 2);
        assert!(g.stock_nodes.contains_key(&sym("AAPL.US")));
        assert!(g.stock_nodes.contains_key(&sym("NVDA.US")));
    }

    #[test]
    fn graph_creates_sector_edges() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0), dec!(0)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.2), dec!(0.3), dec!(0.1), dec!(0), dec!(0)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), SectorId("tech".into())),
            (sym("MSFT.US"), SectorId("tech".into())),
        ]);
        let sector_names = HashMap::from([(SectorId("tech".into()), "Technology".into())]);
        let g = UsGraph::compute(&snap, &sector_map, &sector_names);

        assert_eq!(g.sector_nodes.len(), 1);
        assert!(g.sector_nodes.contains_key(&SectorId("tech".into())));
    }

    #[test]
    fn graph_stock_to_stock_edges() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0.1), dec!(0.2)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0.1), dec!(0.2)),
            ),
            (
                sym("XOM.US"),
                make_dims(dec!(-0.5), dec!(0.1), dec!(-0.2), dec!(0), dec!(0.1)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), SectorId("tech".into())),
            (sym("MSFT.US"), SectorId("tech".into())),
            (sym("XOM.US"), SectorId("energy".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        // One highly similar pair plus a dissimilar third stock ensures
        // the top pair stays strictly above the median cutoff.
        assert!(g.graph.edge_count() > 0);
    }

    #[test]
    fn graph_rejects_cross_sector_similarity_noise() {
        let snap = make_snapshot(vec![
            (
                sym("IQ.US"),
                make_dims(dec!(0.6), dec!(0.5), dec!(0.1), dec!(0), dec!(-0.1)),
            ),
            (
                sym("JPM.US"),
                make_dims(dec!(0.6), dec!(0.5), dec!(0.1), dec!(0), dec!(-0.1)),
            ),
            (
                sym("BAC.US"),
                make_dims(dec!(0.6), dec!(0.5), dec!(0.1), dec!(0), dec!(-0.1)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("IQ.US"), SectorId("china_adr".into())),
            (sym("JPM.US"), SectorId("finance".into())),
            (sym("BAC.US"), SectorId("finance".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        let iq_idx = g.stock_nodes[&sym("IQ.US")];
        let jpm_idx = g.stock_nodes[&sym("JPM.US")];
        let bac_idx = g.stock_nodes[&sym("BAC.US")];

        assert!(g.graph.find_edge(iq_idx, jpm_idx).is_none());
        assert!(g.graph.find_edge(iq_idx, bac_idx).is_none());
    }

    #[test]
    fn graph_rejects_etf_to_single_name_noise() {
        let snap = make_snapshot(vec![
            (
                sym("KWEB.US"),
                make_dims(dec!(0.4), dec!(0.7), dec!(0.2), dec!(0.4), dec!(0)),
            ),
            (
                sym("FSLR.US"),
                make_dims(dec!(0.4), dec!(0.7), dec!(0.2), dec!(0.4), dec!(0)),
            ),
            (
                sym("TAN.US"),
                make_dims(dec!(0.4), dec!(0.7), dec!(0.2), dec!(0.4), dec!(0)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("KWEB.US"), SectorId("etf".into())),
            (sym("FSLR.US"), SectorId("energy".into())),
            (sym("TAN.US"), SectorId("etf".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        let kweb_idx = g.stock_nodes[&sym("KWEB.US")];
        let fslr_idx = g.stock_nodes[&sym("FSLR.US")];

        assert!(g.graph.find_edge(kweb_idx, fslr_idx).is_none());
    }

    #[test]
    fn graph_creates_cross_market_nodes() {
        let snap = make_snapshot(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(-0.2), dec!(0.3), dec!(0.1), dec!(0)),
        )]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());

        assert_eq!(g.cross_market_nodes.len(), 1);
        assert!(g.cross_market_nodes.contains_key(&sym("BABA.US")));
    }

    #[test]
    fn graph_cross_market_edges_bidirectional() {
        let snap = make_snapshot(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(-0.2), dec!(0.3), dec!(0.1), dec!(0)),
        )]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());

        let stock_idx = g.stock_nodes[&sym("BABA.US")];
        let cm_idx = g.cross_market_nodes[&sym("BABA.US")];

        // Should have edges in both directions
        let has_stock_to_cm = g.graph.find_edge(stock_idx, cm_idx).is_some();
        let has_cm_to_stock = g.graph.find_edge(cm_idx, stock_idx).is_some();
        assert!(has_stock_to_cm);
        assert!(has_cm_to_stock);
    }

    #[test]
    fn graph_hk_counterpart_lookup() {
        let snap = make_snapshot(vec![(
            sym("JD.US"),
            make_dims(dec!(0.1), dec!(0.2), dec!(0), dec!(0), dec!(0)),
        )]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        assert_eq!(g.hk_counterpart(&sym("JD.US")), Some(sym("9618.HK")));
        assert_eq!(g.hk_counterpart(&sym("AAPL.US")), None);
    }

    #[test]
    fn graph_no_cross_market_for_non_dual_listed() {
        let snap = make_snapshot(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0), dec!(0.1)),
        )]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        assert!(g.cross_market_nodes.is_empty());
    }

    #[test]
    fn graph_active_cross_market_pairs() {
        let snap = make_snapshot(vec![
            (
                sym("BABA.US"),
                make_dims(dec!(0.1), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("JD.US"),
                make_dims(dec!(0.2), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("AAPL.US"),
                make_dims(dec!(0.3), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
        ]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        let pairs = g.active_cross_market_pairs();
        assert_eq!(pairs.len(), 2); // BABA + JD
    }

    #[test]
    fn graph_empty_dimensions() {
        let snap = make_snapshot(vec![]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        assert!(g.stock_nodes.is_empty());
        assert!(g.sector_nodes.is_empty());
        assert!(g.cross_market_nodes.is_empty());
        assert_eq!(g.graph.node_count(), 0);
    }

    #[test]
    fn graph_mean_direction() {
        let dims = make_dims(dec!(0.2), dec!(0.4), dec!(0.6), dec!(0.8), dec!(1.0));
        let avg = dimension_composite(&dims);
        // (0.2 + 0.4 + 0.6 + 0.8 + 1.0) / 5 = 3.0 / 5 = 0.6
        assert_eq!(avg, dec!(0.6));
    }

    #[test]
    fn graph_dims_to_array_roundtrip() {
        let dims = make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4), dec!(0.5));
        let arr = dims_to_array(&dims);
        assert_eq!(arr, [dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4), dec!(0.5)]);
    }
}
