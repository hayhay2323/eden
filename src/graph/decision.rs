use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::math::cosine_similarity;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::{InstitutionId, Symbol};
use crate::ontology::store::ObjectStore;
use crate::pipeline::dimensions::SymbolDimensions;

use super::graph::{dims_to_array, BrainGraph, EdgeKind, NodeKind};

// ── Convergence ──

#[derive(Debug, Clone)]
pub struct ConvergenceScore {
    pub symbol: Symbol,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub composite: Decimal,
}

impl ConvergenceScore {
    /// Compute convergence score for a stock in the BrainGraph.
    fn compute(symbol: &Symbol, brain: &BrainGraph) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;

        // 1. institutional_alignment: weighted avg of institution edge directions, weighted by seat_count
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Incoming) {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let w = Decimal::from(e.seat_count as i64);
                weighted_sum += e.direction * w;
                weight_total += w;
            }
        }
        let institutional_alignment = if weight_total > Decimal::ZERO {
            weighted_sum / weight_total
        } else {
            Decimal::ZERO
        };

        // 2. sector_coherence: sector node's mean_coherence via stock→sector edge
        let mut sector_coherence = None;
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Outgoing) {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_coherence = Some(s.mean_coherence);
                }
            }
        }

        // 3. cross_stock_correlation: mean of (similarity * neighbor.mean_direction) across stock↔stock
        let mut corr_sum = Decimal::ZERO;
        let mut corr_count = 0i64;
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Outgoing) {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    corr_sum += e.similarity * neighbor.mean_direction;
                    corr_count += 1;
                }
            }
        }
        let cross_stock_correlation = if corr_count > 0 {
            corr_sum / Decimal::from(corr_count)
        } else {
            Decimal::ZERO
        };

        // 4. composite: mean of nonzero components (equal weight)
        let mut components = Vec::new();
        if institutional_alignment != Decimal::ZERO {
            components.push(institutional_alignment);
        }
        if let Some(sc) = sector_coherence {
            if sc != Decimal::ZERO {
                components.push(sc);
            }
        }
        if cross_stock_correlation != Decimal::ZERO {
            components.push(cross_stock_correlation);
        }
        let composite = if components.is_empty() {
            Decimal::ZERO
        } else {
            let sum: Decimal = components.iter().sum();
            sum / Decimal::from(components.len() as i64)
        };

        Some(ConvergenceScore {
            symbol: symbol.clone(),
            institutional_alignment,
            sector_coherence,
            cross_stock_correlation,
            composite,
        })
    }
}

// ── Structural Fingerprint (captured at entry) ──

#[derive(Debug, Clone)]
pub struct StructuralFingerprint {
    pub symbol: Symbol,
    pub entry_timestamp: OffsetDateTime,
    pub entry_composite: Decimal,
    pub entry_regime: crate::action::narrative::Regime,
    pub institutional_directions: Vec<(InstitutionId, Decimal)>,
    pub sector_mean_coherence: Option<Decimal>,
    pub correlated_stocks: Vec<(Symbol, Decimal)>,
    pub entry_dimensions: SymbolDimensions,
}

impl StructuralFingerprint {
    /// Capture the structural fingerprint of a stock at entry time.
    pub fn capture(symbol: &Symbol, brain: &BrainGraph) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;
        let stock_node = match &brain.graph[stock_idx] {
            NodeKind::Stock(s) => s,
            _ => return None,
        };

        // Institutional directions
        let mut institutional_directions = Vec::new();
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Incoming) {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let source = edge.source();
                if let NodeKind::Institution(inst) = &brain.graph[source] {
                    institutional_directions.push((inst.institution_id, e.direction));
                }
            }
        }

        // Sector coherence
        let mut sector_mean_coherence = None;
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Outgoing) {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_mean_coherence = Some(s.mean_coherence);
                }
            }
        }

        // Correlated stocks
        let mut correlated_stocks = Vec::new();
        for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Outgoing) {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    correlated_stocks.push((neighbor.symbol.clone(), e.similarity));
                }
            }
        }

        Some(StructuralFingerprint {
            symbol: symbol.clone(),
            entry_timestamp: brain.timestamp,
            entry_composite: Decimal::ZERO, // Filled by caller with convergence score
            entry_regime: stock_node.regime,
            institutional_directions,
            sector_mean_coherence,
            correlated_stocks,
            entry_dimensions: stock_node.dimensions.clone(),
        })
    }
}

// ── Structural Degradation ──

#[derive(Debug, Clone)]
pub struct StructuralDegradation {
    pub symbol: Symbol,
    pub institution_retention: Decimal,
    pub sector_coherence_change: Decimal,
    pub correlation_retention: Decimal,
    pub dimension_drift: Decimal,
    pub composite_degradation: Decimal,
}

impl StructuralDegradation {
    /// Compute how much the structure has degraded since entry.
    pub fn compute(fingerprint: &StructuralFingerprint, brain: &BrainGraph) -> Self {
        let symbol = &fingerprint.symbol;

        // institution_retention: fraction of original institutions still present with same direction sign
        let institution_retention = if fingerprint.institutional_directions.is_empty() {
            Decimal::ONE // No institutions at entry → nothing to lose
        } else {
            let mut retained = 0i64;
            for (inst_id, entry_dir) in &fingerprint.institutional_directions {
                if let Some(&inst_idx) = brain.institution_nodes.get(inst_id) {
                    // Check if this institution still has an edge to this stock with same sign
                    if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
                        let still_present = brain
                            .graph
                            .edges_directed(inst_idx, GraphDirection::Outgoing)
                            .any(|edge| {
                                if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                                    edge.target() == stock_idx
                                        && same_sign(e.direction, *entry_dir)
                                } else {
                                    false
                                }
                            });
                        if still_present {
                            retained += 1;
                        }
                    }
                }
            }
            Decimal::from(retained)
                / Decimal::from(fingerprint.institutional_directions.len() as i64)
        };

        // sector_coherence_change: current minus entry
        let sector_coherence_change = if let Some(entry_sc) = fingerprint.sector_mean_coherence {
            let current_sc = brain
                .stock_nodes
                .get(symbol)
                .and_then(|&idx| {
                    brain
                        .graph
                        .edges_directed(idx, GraphDirection::Outgoing)
                        .find_map(|edge| {
                            if let EdgeKind::StockToSector(_) = edge.weight() {
                                if let NodeKind::Sector(s) = &brain.graph[edge.target()] {
                                    return Some(s.mean_coherence);
                                }
                            }
                            None
                        })
                })
                .unwrap_or(Decimal::ZERO);
            current_sc - entry_sc
        } else {
            Decimal::ZERO
        };

        // correlation_retention: fraction of correlated stocks still correlated
        let correlation_retention = if fingerprint.correlated_stocks.is_empty() {
            Decimal::ONE
        } else {
            let mut retained = 0i64;
            if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
                let current_neighbors: HashMap<Symbol, Decimal> = brain
                    .graph
                    .edges_directed(stock_idx, GraphDirection::Outgoing)
                    .filter_map(|edge| {
                        if let EdgeKind::StockToStock(e) = edge.weight() {
                            if let NodeKind::Stock(neighbor) = &brain.graph[edge.target()] {
                                return Some((neighbor.symbol.clone(), e.similarity));
                            }
                        }
                        None
                    })
                    .collect();
                for (sym, _) in &fingerprint.correlated_stocks {
                    if current_neighbors.contains_key(sym) {
                        retained += 1;
                    }
                }
            }
            Decimal::from(retained)
                / Decimal::from(fingerprint.correlated_stocks.len() as i64)
        };

        // dimension_drift: 1 - cosine_similarity(entry, current)
        let dimension_drift = if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
            if let NodeKind::Stock(current) = &brain.graph[stock_idx] {
                let entry_vec = dims_to_array(&fingerprint.entry_dimensions);
                let current_vec = dims_to_array(&current.dimensions);
                Decimal::ONE - cosine_similarity(entry_vec, current_vec)
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ONE // Stock gone → max drift
        };

        // composite_degradation: mean of degradation signals
        // Convert retentions to degradation (1 - retention), keep change/drift as-is
        let inst_degradation = Decimal::ONE - institution_retention;
        let corr_degradation = Decimal::ONE - correlation_retention;
        let signals = [
            inst_degradation,
            sector_coherence_change.abs(),
            corr_degradation,
            dimension_drift,
        ];
        let composite_degradation =
            signals.iter().sum::<Decimal>() / Decimal::from(signals.len() as i64);

        StructuralDegradation {
            symbol: symbol.clone(),
            institution_retention,
            sector_coherence_change,
            correlation_retention,
            dimension_drift,
            composite_degradation,
        }
    }
}

fn same_sign(a: Decimal, b: Decimal) -> bool {
    (a > Decimal::ZERO && b > Decimal::ZERO)
        || (a < Decimal::ZERO && b < Decimal::ZERO)
        || (a == Decimal::ZERO && b == Decimal::ZERO)
}

// ── Order Suggestion ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct OrderSuggestion {
    pub symbol: Symbol,
    pub direction: OrderDirection,
    pub convergence: ConvergenceScore,
    pub suggested_quantity: i32,
    pub price_low: Option<Decimal>,
    pub price_high: Option<Decimal>,
    pub requires_confirmation: bool,
}

// ── Decision Snapshot ──

#[derive(Debug)]
pub struct DecisionSnapshot {
    pub timestamp: OffsetDateTime,
    pub convergence_scores: HashMap<Symbol, ConvergenceScore>,
    pub order_suggestions: Vec<OrderSuggestion>,
    pub degradations: HashMap<Symbol, StructuralDegradation>,
}

impl DecisionSnapshot {
    /// Compute all convergence scores, order suggestions, and structural degradations.
    pub fn compute(
        brain: &BrainGraph,
        links: &LinkSnapshot,
        active_fingerprints: &[StructuralFingerprint],
        store: &ObjectStore,
    ) -> Self {
        // Compute ConvergenceScore for all stock nodes
        let mut convergence_scores = HashMap::new();
        for symbol in brain.stock_nodes.keys() {
            if let Some(score) = ConvergenceScore::compute(symbol, brain) {
                convergence_scores.insert(symbol.clone(), score);
            }
        }

        // Build order book price lookup
        let mut best_bid: HashMap<Symbol, Decimal> = HashMap::new();
        let mut best_ask: HashMap<Symbol, Decimal> = HashMap::new();
        for ob in &links.order_books {
            if let Some(level) = ob.bid_levels.first() {
                if let Some(price) = level.price {
                    best_bid.insert(ob.symbol.clone(), price);
                }
            }
            if let Some(level) = ob.ask_levels.first() {
                if let Some(price) = level.price {
                    best_ask.insert(ob.symbol.clone(), price);
                }
            }
        }

        // Generate OrderSuggestion for stocks with |composite| > 0
        let mut order_suggestions = Vec::new();
        for (symbol, score) in &convergence_scores {
            if score.composite == Decimal::ZERO {
                continue;
            }
            let direction = if score.composite > Decimal::ZERO {
                OrderDirection::Buy
            } else {
                OrderDirection::Sell
            };
            let lot_size = store
                .stocks
                .get(symbol)
                .map(|s| s.lot_size)
                .unwrap_or(100);

            order_suggestions.push(OrderSuggestion {
                symbol: symbol.clone(),
                direction,
                convergence: score.clone(),
                suggested_quantity: lot_size,
                price_low: best_bid.get(symbol).copied(),
                price_high: best_ask.get(symbol).copied(),
                requires_confirmation: true,
            });
        }

        // Compute StructuralDegradation for all active fingerprints
        let mut degradations = HashMap::new();
        for fp in active_fingerprints {
            let deg = StructuralDegradation::compute(fp, brain);
            degradations.insert(fp.symbol.clone(), deg);
        }

        DecisionSnapshot {
            timestamp: brain.timestamp,
            convergence_scores,
            order_suggestions,
            degradations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::narrative::{
        DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
    };
    use crate::graph::graph::BrainGraph;
    use crate::logic::tension::Dimension;
    use crate::ontology::links::*;
    use crate::ontology::objects::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_store_with_stocks(stocks: Vec<Stock>) -> ObjectStore {
        let mut stock_map = HashMap::new();
        for s in stocks {
            stock_map.insert(s.symbol.clone(), s);
        }
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: stock_map,
            sectors: HashMap::new(),
            broker_to_institution: HashMap::new(),
        }
    }

    fn make_stock(symbol: &str, lot_size: i32) -> Stock {
        Stock {
            symbol: sym(symbol),
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size,
            sector_id: None,
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
            depth_structure_imbalance: Decimal::ZERO,
        }
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
            trade_activities: vec![],
        }
    }

    fn build_brain(
        narratives: HashMap<Symbol, SymbolNarrative>,
        dimensions: HashMap<Symbol, SymbolDimensions>,
        links: &LinkSnapshot,
        store: &ObjectStore,
    ) -> BrainGraph {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let dims = crate::pipeline::dimensions::DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        BrainGraph::compute(&narrative, &dims, links, store)
    }

    // ── Convergence Tests ──

    #[test]
    fn all_bullish_convergence() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2, 3],
            seat_count: 3,
        });

        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        // All bullish → positive composite
        assert!(score.composite > Decimal::ZERO);
        assert!(score.institutional_alignment > Decimal::ZERO);
        assert!(score.cross_stock_correlation > Decimal::ZERO);
    }

    #[test]
    fn conflicted_signals() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(-0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(-0.4), dec!(-0.4), dec!(-0.4), dec!(-0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![sym("700.HK")],
            bid_symbols: vec![],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![],
            seat_count: 2,
        });

        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        // Institution selling, correlated stock bearish → negative composite
        assert!(score.composite < Decimal::ZERO);
    }

    #[test]
    fn no_institutions_convergence() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        assert_eq!(score.institutional_alignment, Decimal::ZERO);
        // No neighbors either, so composite = 0
        assert_eq!(score.composite, Decimal::ZERO);
    }

    // ── Fingerprint + Degradation Tests ──

    #[test]
    fn fingerprint_no_degradation() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let fp = StructuralFingerprint::capture(&sym("700.HK"), &brain).unwrap();
        // Same brain → no degradation
        let deg = StructuralDegradation::compute(&fp, &brain);
        // dimension_drift should be ~0 (same dims)
        assert!(deg.dimension_drift.abs() < dec!(0.001));
        assert_eq!(deg.institution_retention, Decimal::ONE); // no institutions → 1
        assert_eq!(deg.correlation_retention, Decimal::ONE); // no correlations → 1
    }

    #[test]
    fn full_degradation() {
        // Build entry brain
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });

        let store = make_store_with_stocks(vec![]);
        let entry_brain = build_brain(
            narratives.clone(),
            dimensions.clone(),
            &links,
            &store,
        );
        let mut fp = StructuralFingerprint::capture(&sym("700.HK"), &entry_brain).unwrap();
        fp.entry_composite = dec!(0.5);

        // Build degraded brain — institution gone, dimensions flipped
        let mut narratives2 = HashMap::new();
        narratives2.insert(sym("700.HK"), make_narrative(dec!(-0.3), dec!(-0.5)));

        let mut dimensions2 = HashMap::new();
        dimensions2.insert(
            sym("700.HK"),
            make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
        );

        let empty = empty_links();
        let degraded_brain = build_brain(narratives2, dimensions2, &empty, &store);

        let deg = StructuralDegradation::compute(&fp, &degraded_brain);
        // Institution gone → retention = 0
        assert_eq!(deg.institution_retention, Decimal::ZERO);
        // Dimensions flipped → drift should be ~2
        assert!(deg.dimension_drift > dec!(1.5));
        // Overall high degradation
        assert!(deg.composite_degradation > dec!(0.5));
    }

    // ── Order Suggestion Tests ──

    #[test]
    fn order_direction_from_composite() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2],
            seat_count: 2,
        });

        let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        let suggestion = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"));
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.direction, OrderDirection::Buy);
        assert_eq!(s.suggested_quantity, 100);
        assert!(s.requires_confirmation);
    }

    #[test]
    fn price_range_from_order_book() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)));
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });
        links.order_books.push(OrderBookObservation {
            symbol: sym("700.HK"),
            bid_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(350)),
                volume: 1000,
                order_num: 10,
            }],
            ask_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(351)),
                volume: 800,
                order_num: 8,
            }],
            total_bid_volume: 1000,
            total_ask_volume: 800,
            total_bid_orders: 10,
            total_ask_orders: 8,
            spread: Some(dec!(1)),
            bid_level_count: 1,
            ask_level_count: 1,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        });

        let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        let s = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"))
            .unwrap();
        assert_eq!(s.price_low, Some(dec!(350)));
        assert_eq!(s.price_high, Some(dec!(351)));
    }

    #[test]
    fn zero_composite_no_suggestions() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0), dec!(0)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0), dec!(0), dec!(0), dec!(0)));

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        // Composite is zero → no suggestion
        let suggestion = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"));
        assert!(suggestion.is_none());
    }
}
