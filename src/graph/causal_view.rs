//! `CausalGraphView` adapter for BrainGraph.
//!
//! This is the thin bridge between Eden's BrainGraph (HK-side knowledge
//! graph) and the abstract `CausalGraphView` trait in
//! `pipeline::intervention`. Exposing just enough graph structure to let
//! the intervention propagator run forward BFS, without leaking petgraph
//! or NodeKind internals across the module boundary.
//!
//! Scope: stock-level causal propagation along StockToStock edges.
//! Similarity is treated as a positive signed weight (co-moving direction
//! propagates as same direction). Institution / sector edges are
//! intentionally excluded — those are compositional rather than causal
//! in the operator sense ("JPM holds X" doesn't cause X to move in the
//! same way "X correlates with Y" does).

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rust_decimal::prelude::ToPrimitive;

use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};
use crate::ontology::objects::Symbol;
use crate::pipeline::intervention::CausalGraphView;

/// Read-only view of BrainGraph that presents its StockToStock edges as
/// a causal graph over `Symbol` nodes.
pub struct BrainGraphCausalView<'a> {
    graph: &'a BrainGraph,
}

impl<'a> BrainGraphCausalView<'a> {
    pub fn new(graph: &'a BrainGraph) -> Self {
        Self { graph }
    }

    fn outgoing_stock_similarities(&self, symbol: &Symbol) -> Vec<(Symbol, f64)> {
        let Some(&from_idx) = self.graph.stock_nodes.get(symbol) else {
            return Vec::new();
        };
        let mut out: Vec<(Symbol, f64)> = Vec::new();
        for edge in self
            .graph
            .graph
            .edges_directed(from_idx, Direction::Outgoing)
        {
            let target_idx: NodeIndex = edge.target();
            // Only stock→stock edges contribute; institution or sector
            // edges skew toward compositional relationships that don't
            // propagate direction the same way.
            let weight = match edge.weight() {
                EdgeKind::StockToStock(s2s) => s2s.similarity.to_f64().unwrap_or(0.0),
                _ => continue,
            };
            if weight.abs() < f64::EPSILON {
                continue;
            }
            let Some(NodeKind::Stock(stock_node)) = self.graph.graph.node_weight(target_idx) else {
                continue;
            };
            out.push((stock_node.symbol.clone(), weight));
        }
        out
    }
}

impl<'a> CausalGraphView for BrainGraphCausalView<'a> {
    type Node = Symbol;

    fn outgoing_causal_edges(&self, from: &Self::Node) -> Vec<(Self::Node, f64)> {
        self.outgoing_stock_similarities(from)
    }
}

// Tests live alongside the propagation algorithm in
// `pipeline::intervention::tests` — that covers the core logic with
// deterministic fixture graphs. This module is a thin petgraph-to-trait
// adapter with no logic of its own; any behavior bug would show up in
// integration tests of the runtime. Full fixture construction of
// BrainGraph is heavy (NarrativeSnapshot / DimensionSnapshot /
// LinkSnapshot / ObjectStore), so we don't add redundant coverage here.
