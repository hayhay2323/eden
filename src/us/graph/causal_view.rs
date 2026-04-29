//! `CausalGraphView` adapter for UsGraph.
//!
//! US counterpart of `graph::causal_view::BrainGraphCausalView`. Exposes
//! UsGraph's `UsStockToStock` edges as a causal graph over `Symbol`
//! nodes so `pipeline::intervention::propagate_intervention` can run
//! over US market topology.
//!
//! Scope: stock-level similarity edges only. Cross-market (HK↔US)
//! edges are excluded — they're their own causal layer and would
//! double-count if mixed in here. Sector edges are compositional, not
//! causal. If an operator wants cross-market intervention reasoning,
//! that's a separate query (future work).

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rust_decimal::prelude::ToPrimitive;

use crate::ontology::objects::Symbol;
use crate::pipeline::intervention::CausalGraphView;
use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};

pub struct UsGraphCausalView<'a> {
    graph: &'a UsGraph,
}

impl<'a> UsGraphCausalView<'a> {
    pub fn new(graph: &'a UsGraph) -> Self {
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
            let weight = match edge.weight() {
                UsEdgeKind::StockToStock(s2s) => s2s.similarity.to_f64().unwrap_or(0.0),
                _ => continue,
            };
            if weight.abs() < f64::EPSILON {
                continue;
            }
            let Some(UsNodeKind::Stock(stock_node)) = self.graph.graph.node_weight(target_idx)
            else {
                continue;
            };
            out.push((stock_node.symbol.clone(), weight));
        }
        out
    }
}

impl<'a> CausalGraphView for UsGraphCausalView<'a> {
    type Node = Symbol;

    fn outgoing_causal_edges(&self, from: &Self::Node) -> Vec<(Self::Node, f64)> {
        self.outgoing_stock_similarities(from)
    }
}

// Tests live in pipeline::intervention::tests against a deterministic
// fixture graph — any adapter bug would fail integration-level tests
// of the runtime. UsGraph fixture construction is heavy so we don't
// duplicate coverage at the adapter layer.
