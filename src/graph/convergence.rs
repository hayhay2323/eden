use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;

use crate::ontology::microstructure::MicrostructureDeltas;
use crate::ontology::objects::Symbol;

use super::graph::{BrainGraph, EdgeKind, NodeKind};

// ── Convergence ──

#[derive(Debug, Clone)]
pub struct ConvergenceScore {
    pub symbol: Symbol,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub composite: Decimal,
    // Temporal stability (None when temporal context unavailable)
    pub edge_stability: Option<Decimal>,
    pub institutional_edge_age: Option<Decimal>,
    pub new_edge_fraction: Option<Decimal>,
    // Microstructure confirmation
    pub microstructure_confirmation: Option<Decimal>,
    // Confidence metrics
    pub component_spread: Option<Decimal>,
    pub temporal_weight: Option<Decimal>,
}

impl ConvergenceScore {
    /// Compute convergence score for a stock in the BrainGraph.
    pub fn compute(
        symbol: &Symbol,
        brain: &BrainGraph,
        temporal_ctx: Option<&TemporalConvergenceContext>,
    ) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;

        // 1. institutional_alignment: weighted avg of institution edge directions, weighted by seat_count
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
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

        // 2. sector_coherence: sector node's mean_direction (signed) via stock→sector edge.
        //    We use mean_direction rather than mean_coherence so this component is
        //    directionally consistent with the other signed components.
        let mut sector_coherence = None;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_coherence = Some(s.mean_direction);
                }
            }
        }

        // 3. cross_stock_correlation: mean of (similarity * neighbor.mean_direction) across stock↔stock
        let mut corr_sum = Decimal::ZERO;
        let mut corr_count = 0i64;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
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
        let raw_composite = if components.is_empty() {
            Decimal::ZERO
        } else {
            let sum: Decimal = components.iter().sum();
            sum / Decimal::from(components.len() as i64)
        };

        // 5. Component spread: max - min of nonzero components (agreement metric)
        let component_spread = if components.len() >= 2 {
            let max = components.iter().copied().max().unwrap_or(Decimal::ZERO);
            let min = components.iter().copied().min().unwrap_or(Decimal::ZERO);
            Some(max - min)
        } else {
            None
        };

        // 6. Temporal enrichment (only when context provided)
        let (
            edge_stability,
            institutional_edge_age,
            new_edge_fraction,
            temporal_weight,
            microstructure_confirmation,
        ) = if let Some(ctx) = temporal_ctx {
            compute_temporal_enrichment(symbol, brain, stock_idx, ctx)
        } else {
            (None, None, None, None, None)
        };

        // 7. Activity gate: dampen composite for stocks with no trading activity.
        //    If activity_momentum and order_book_pressure are both zero, the stock
        //    has no volume confirmation — convergence is structural-only and unreliable.
        let stock_node = &brain.graph[stock_idx];
        let activity_gate = if let NodeKind::Stock(sn) = stock_node {
            let has_activity = sn.dimensions.activity_momentum != Decimal::ZERO
                || sn.dimensions.order_book_pressure != Decimal::ZERO;
            if has_activity {
                Decimal::ONE
            } else {
                // No trading activity — halve the composite to reflect low confidence
                Decimal::new(5, 1)
            }
        } else {
            Decimal::ONE
        };

        // 8. Apply temporal weight, microstructure confirmation, and activity gate
        let with_micro = raw_composite + microstructure_confirmation.unwrap_or(Decimal::ZERO);
        let composite = with_micro * temporal_weight.unwrap_or(Decimal::ONE) * activity_gate;

        Some(ConvergenceScore {
            symbol: symbol.clone(),
            institutional_alignment,
            sector_coherence,
            cross_stock_correlation,
            composite,
            edge_stability,
            institutional_edge_age,
            new_edge_fraction,
            microstructure_confirmation,
            component_spread,
            temporal_weight,
        })
    }
}

/// Context for temporal enrichment of convergence scores.
/// When provided, ConvergenceScore::compute() will incorporate edge stability,
/// microstructure confirmation, and rolling statistics.
pub struct TemporalConvergenceContext<'a> {
    pub edge_registry: &'a crate::graph::temporal::TemporalEdgeRegistry,
    pub tick: u64,
    pub microstructure_deltas: Option<&'a MicrostructureDeltas>,
    pub rolling_composites: std::collections::HashMap<Symbol, RollingStats>,
}

/// Rolling statistics for a signal time series.
pub struct RollingStats {
    pub mean: Decimal,
    pub stddev: Decimal,
    pub trend: Decimal,
    pub sample_count: usize,
}

// ── Temporal enrichment helper ──

fn compute_temporal_enrichment(
    symbol: &Symbol,
    brain: &BrainGraph,
    stock_idx: petgraph::graph::NodeIndex,
    ctx: &TemporalConvergenceContext,
) -> (
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
    Option<Decimal>,
) {
    use crate::graph::temporal::GraphEdgeId;

    // Collect edge stability metrics from institution→stock edges
    let mut stability_sum = Decimal::ZERO;
    let mut age_sum: u64 = 0;
    let mut new_count: u64 = 0;
    let mut edge_count: u64 = 0;

    for edge in brain
        .graph
        .edges_directed(stock_idx, GraphDirection::Incoming)
    {
        if let EdgeKind::InstitutionToStock(_) = edge.weight() {
            let source = edge.source();
            if let NodeKind::Institution(inst) = &brain.graph[source] {
                let edge_id = GraphEdgeId::institution_to_stock(inst.institution_id, symbol);
                if let Some(state) = ctx.edge_registry.edge_state(&edge_id) {
                    let stability = if state.seen_count > 0 {
                        Decimal::ONE
                            - Decimal::from(state.disappearance_count)
                                / Decimal::from(state.seen_count)
                    } else {
                        Decimal::ZERO
                    };
                    let age = ctx.tick.saturating_sub(state.first_seen_tick);

                    stability_sum += stability;
                    age_sum += age;
                    if age < 5 {
                        new_count += 1;
                    }
                    edge_count += 1;
                }
            }
        }
    }

    if edge_count == 0 {
        return (None, None, None, None, None);
    }

    let edge_stability = stability_sum / Decimal::from(edge_count);
    let mean_age = Decimal::from(age_sum) / Decimal::from(edge_count);
    let new_fraction = Decimal::from(new_count) / Decimal::from(edge_count);

    // temporal_weight: [0.5, 1.0] based on edge stability
    // Stable edges → weight closer to 1.0 (trust the signal)
    // Unstable edges → weight closer to 0.5 (dampen the signal)
    let mut tw = Decimal::new(5, 1) + edge_stability * Decimal::new(5, 1);
    // Extra dampening when majority of edges are brand new
    if new_fraction > Decimal::new(5, 1) {
        tw *= Decimal::new(8, 1);
    }

    // Microstructure confirmation from order book deltas
    let micro_confirm = ctx
        .microstructure_deltas
        .and_then(|deltas| {
            deltas
                .order_book_deltas
                .iter()
                .find(|d| &d.symbol == symbol)
        })
        .and_then(|delta| {
            // If spread changed, use direction agreement
            delta.spread_change.map(|(old_spread, new_spread)| {
                let spread_narrowing = new_spread < old_spread;
                // +0.1 if spread narrows (more liquidity), -0.1 if widens
                if spread_narrowing {
                    Decimal::new(1, 1)
                } else {
                    Decimal::new(-1, 1)
                }
            })
        });

    (
        Some(edge_stability),
        Some(mean_age),
        Some(new_fraction),
        Some(tw),
        micro_confirm,
    )
}
