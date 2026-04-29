use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::math::cosine_similarity;
use crate::ontology::objects::{InstitutionId, Symbol};
use crate::ontology::reasoning::{ActionDirection, ActionNode, ActionNodeStage};
use crate::pipeline::dimensions::SymbolDimensions;

use super::decision::{same_sign, signed_position_return};
use super::graph::{dims_to_array, BrainGraph, EdgeKind, NodeKind};

// ── Structural Fingerprint (captured at entry) ──

#[derive(Debug, Clone)]
pub struct StructuralFingerprint {
    pub symbol: Symbol,
    pub entry_tick: u64,
    pub entry_timestamp: OffsetDateTime,
    pub entry_price: Option<Decimal>,
    pub entry_composite: Decimal,
    pub entry_regime: crate::action::narrative::Regime,
    pub institutional_directions: Vec<(InstitutionId, Decimal)>,
    pub sector_mean_coherence: Option<Decimal>,
    pub correlated_stocks: Vec<(Symbol, Decimal)>,
    pub entry_dimensions: SymbolDimensions,
}

impl StructuralFingerprint {
    /// Capture the structural fingerprint of a stock at entry time.
    pub fn capture(
        symbol: &Symbol,
        brain: &BrainGraph,
        entry_tick: u64,
        entry_price: Option<Decimal>,
    ) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;
        let stock_node = match &brain.graph[stock_idx] {
            NodeKind::Stock(s) => s,
            _ => return None,
        };

        // Institutional directions
        let mut institutional_directions = Vec::new();
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let source = edge.source();
                if let NodeKind::Institution(inst) = &brain.graph[source] {
                    institutional_directions.push((inst.institution_id, e.direction));
                }
            }
        }

        // Sector coherence
        let mut sector_mean_coherence = None;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_mean_coherence = Some(s.mean_coherence);
                }
            }
        }

        // Correlated stocks
        let mut correlated_stocks = Vec::new();
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    correlated_stocks.push((neighbor.symbol.clone(), e.similarity));
                }
            }
        }

        Some(StructuralFingerprint {
            symbol: symbol.clone(),
            entry_tick,
            entry_timestamp: brain.timestamp,
            entry_price,
            entry_composite: Decimal::ZERO, // Filled by caller with convergence score
            entry_regime: stock_node.regime,
            institutional_directions,
            sector_mean_coherence,
            correlated_stocks,
            entry_dimensions: stock_node.dimensions.clone(),
        })
    }
}

impl ActionNode {
    pub fn from_hk_fingerprint(symbol: &Symbol, fingerprint: &StructuralFingerprint) -> Self {
        Self::from_hk_position(
            symbol,
            fingerprint,
            fingerprint.entry_tick,
            None,
            None,
            None,
        )
    }

    pub fn from_hk_position(
        symbol: &Symbol,
        fingerprint: &StructuralFingerprint,
        current_tick: u64,
        current_confidence: Option<Decimal>,
        current_price: Option<Decimal>,
        degradation: Option<&StructuralDegradation>,
    ) -> Self {
        let direction = if fingerprint.entry_composite > Decimal::ZERO {
            ActionDirection::Long
        } else if fingerprint.entry_composite < Decimal::ZERO {
            ActionDirection::Short
        } else {
            ActionDirection::Neutral
        };
        let entry_confidence = fingerprint.entry_composite.abs();
        let degradation_score = degradation.map(|item| item.composite_degradation);
        let exit_forming = degradation
            .map(|item| item.composite_degradation >= Decimal::new(45, 2))
            .unwrap_or(false);
        let pnl = fingerprint
            .entry_price
            .zip(current_price)
            .and_then(|(entry, current)| signed_position_return(entry, current, direction));

        Self {
            workflow_id: format!("hk-position:{}", symbol),
            symbol: symbol.clone(),
            market: symbol.market(),
            sector: None,
            stage: ActionNodeStage::Monitoring,
            direction,
            entry_confidence,
            current_confidence: current_confidence.unwrap_or(entry_confidence),
            entry_price: fingerprint.entry_price,
            pnl,
            age_ticks: current_tick.saturating_sub(fingerprint.entry_tick),
            degradation_score,
            exit_forming,
        }
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
                                    edge.target() == stock_idx && same_sign(e.direction, *entry_dir)
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
            Decimal::from(retained) / Decimal::from(fingerprint.correlated_stocks.len() as i64)
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
        // Convert retentions to degradation (1 - retention).
        // For sector_coherence_change: only negative changes (sector weakened) count as
        // degradation; positive changes (sector strengthened) are not penalised.
        let inst_degradation = Decimal::ONE - institution_retention;
        let corr_degradation = Decimal::ONE - correlation_retention;
        let mut signals = vec![inst_degradation, corr_degradation, dimension_drift];
        if fingerprint.sector_mean_coherence.is_some() {
            signals.push((-sector_coherence_change).max(Decimal::ZERO));
        }
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
