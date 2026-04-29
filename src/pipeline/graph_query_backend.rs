//! Query backend for Eden's graph-native visual frame.
//!
//! The query layer reads `VisualGraphFrame` and active-probe accuracy
//! snapshots. It does not compute new signals and does not feed back
//! into BP. Its job is to provide backend-ready graph queries for UI,
//! CLI, or operator inspection.

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::Serialize;

use crate::pipeline::symbol_sub_kg::NodeKind;
use crate::pipeline::visual_graph_frame::{
    VisualGraphFrame, VisualMasterEdge, VisualSubKgEdge, VisualSubKgNode,
};

#[derive(Debug, Clone, Serialize)]
pub struct GraphPosterior {
    pub p_bull: f64,
    pub p_bear: f64,
    pub p_neutral: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EgoGraphResult {
    pub symbol: String,
    pub posterior: GraphPosterior,
    pub observed_prior: bool,
    pub nodes: Vec<VisualSubKgNode>,
    pub subkg_edges: Vec<VisualSubKgEdge>,
    pub incoming_master_edges: Vec<InfluenceEdge>,
    pub outgoing_master_edges: Vec<InfluenceEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeKindHit {
    pub symbol: String,
    pub node: VisualSubKgNode,
    pub posterior: GraphPosterior,
}

#[derive(Debug, Clone, Serialize)]
pub struct InfluenceEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub from_p_bull: Option<f64>,
    pub to_p_bull: Option<f64>,
    pub delta_p_bull: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InfluenceResult {
    pub symbol: String,
    pub incoming: Vec<InfluenceEdge>,
    pub outgoing: Vec<InfluenceEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeAccuracyHit {
    pub symbol: String,
    pub mean_accuracy: f64,
}

pub struct GraphQueryBackend<'a> {
    frame: &'a VisualGraphFrame,
}

impl<'a> GraphQueryBackend<'a> {
    pub fn new(frame: &'a VisualGraphFrame) -> Self {
        Self { frame }
    }

    pub fn ego(&self, symbol: &str) -> Option<EgoGraphResult> {
        let center = self.frame.symbols.iter().find(|s| s.symbol == symbol)?;
        let influence = self.influence(symbol);
        Some(EgoGraphResult {
            symbol: center.symbol.clone(),
            posterior: GraphPosterior {
                p_bull: center.p_bull,
                p_bear: center.p_bear,
                p_neutral: center.p_neutral,
            },
            observed_prior: center.observed_prior,
            nodes: center.nodes.clone(),
            subkg_edges: center.edges.clone(),
            incoming_master_edges: influence.incoming,
            outgoing_master_edges: influence.outgoing,
        })
    }

    pub fn nodes_by_kind(&self, kind: NodeKind) -> Vec<NodeKindHit> {
        let kind_key = format!("{:?}", kind);
        let mut hits = Vec::new();
        for symbol in &self.frame.symbols {
            for node in &symbol.nodes {
                if node.kind == kind_key {
                    hits.push(NodeKindHit {
                        symbol: symbol.symbol.clone(),
                        node: node.clone(),
                        posterior: GraphPosterior {
                            p_bull: symbol.p_bull,
                            p_bear: symbol.p_bear,
                            p_neutral: symbol.p_neutral,
                        },
                    });
                }
            }
        }
        hits.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.node.id.cmp(&b.node.id))
        });
        hits
    }

    pub fn influence(&self, symbol: &str) -> InfluenceResult {
        let mut incoming = Vec::new();
        let mut outgoing = Vec::new();
        for edge in &self.frame.master_edges {
            let converted = influence_edge(edge);
            if edge.to == symbol {
                incoming.push(converted.clone());
            }
            if edge.from == symbol {
                outgoing.push(converted);
            }
        }
        sort_influence(&mut incoming);
        sort_influence(&mut outgoing);
        InfluenceResult {
            symbol: symbol.to_string(),
            incoming,
            outgoing,
        }
    }
}

pub fn ranked_probe_accuracy(accuracy_by_symbol: &HashMap<String, f64>) -> Vec<ProbeAccuracyHit> {
    let mut hits: Vec<ProbeAccuracyHit> = accuracy_by_symbol
        .iter()
        .map(|(symbol, mean_accuracy)| ProbeAccuracyHit {
            symbol: symbol.clone(),
            mean_accuracy: *mean_accuracy,
        })
        .collect();
    hits.sort_by(|a, b| {
        b.mean_accuracy
            .partial_cmp(&a.mean_accuracy)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    hits
}

pub fn probe_accuracy_for_symbol(
    accuracy_by_symbol: &HashMap<String, f64>,
    symbol: &str,
) -> Option<ProbeAccuracyHit> {
    accuracy_by_symbol
        .get(symbol)
        .copied()
        .map(|mean_accuracy| ProbeAccuracyHit {
            symbol: symbol.to_string(),
            mean_accuracy,
        })
}

fn influence_edge(edge: &VisualMasterEdge) -> InfluenceEdge {
    InfluenceEdge {
        from: edge.from.clone(),
        to: edge.to.clone(),
        weight: edge.weight,
        from_p_bull: edge.from_p_bull,
        to_p_bull: edge.to_p_bull,
        delta_p_bull: edge
            .from_p_bull
            .zip(edge.to_p_bull)
            .map(|(from, to)| to - from),
    }
}

fn sort_influence(edges: &mut [InfluenceEdge]) {
    edges.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| {
                b.delta_p_bull
                    .map(f64::abs)
                    .unwrap_or(0.0)
                    .total_cmp(&a.delta_p_bull.map(f64::abs).unwrap_or(0.0))
            })
            .then_with(|| a.from.cmp(&b.from))
            .then_with(|| a.to.cmp(&b.to))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::pipeline::loopy_bp::{GraphEdge, NodePrior};
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use crate::pipeline::visual_graph_frame::build_visual_graph_frame;
    use rust_decimal_macros::dec;

    fn sample_frame() -> VisualGraphFrame {
        let now = Utc::now();
        let mut registry = SubKgRegistry::new();
        registry
            .upsert("A.US", now)
            .set_node_value(NodeId::PressureCapitalFlow, dec!(0.7), now);
        registry
            .upsert("B.US", now)
            .set_node_value(NodeId::IntentAccumulation, dec!(0.6), now);

        let mut priors = HashMap::new();
        priors.insert(
            "A.US".to_string(),
            NodePrior {
                belief: [0.7, 0.2, 0.1],
                observed: true,
            },
        );
        priors.insert("B.US".to_string(), NodePrior::default());

        let mut beliefs = HashMap::new();
        beliefs.insert("A.US".to_string(), [0.8, 0.1, 0.1]);
        beliefs.insert("B.US".to_string(), [0.55, 0.25, 0.20]);
        let edges = vec![
            GraphEdge {
                from: "A.US".to_string(),
                to: "B.US".to_string(),
                weight: 0.9,
                kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "B.US".to_string(),
                to: "A.US".to_string(),
                weight: 0.4,
                kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
            },
        ];

        build_visual_graph_frame("us", 7, &registry, &edges, &priors, &beliefs, now)
    }

    #[test]
    fn ego_returns_subkg_and_master_edges() {
        let frame = sample_frame();
        let backend = GraphQueryBackend::new(&frame);

        let ego = backend.ego("A.US").expect("ego result");

        assert_eq!(ego.symbol, "A.US");
        assert!(ego.observed_prior);
        assert_eq!(ego.posterior.p_bull, 0.8);
        assert_eq!(ego.outgoing_master_edges.len(), 1);
        assert_eq!(ego.incoming_master_edges.len(), 1);
        assert!(ego.nodes.iter().any(|n| n.id == "PressureCapitalFlow"));
    }

    #[test]
    fn nodekind_query_returns_matching_nodes_across_symbols() {
        let frame = sample_frame();
        let backend = GraphQueryBackend::new(&frame);

        let pressure = backend.nodes_by_kind(NodeKind::Pressure);
        let intent = backend.nodes_by_kind(NodeKind::Intent);

        assert_eq!(pressure.len(), 1);
        assert_eq!(pressure[0].symbol, "A.US");
        assert_eq!(intent.len(), 1);
        assert_eq!(intent[0].symbol, "B.US");
    }

    #[test]
    fn influence_query_sorts_by_weight() {
        let frame = sample_frame();
        let backend = GraphQueryBackend::new(&frame);

        let influence = backend.influence("A.US");

        assert_eq!(influence.outgoing[0].to, "B.US");
        assert_eq!(influence.incoming[0].from, "B.US");
        assert_eq!(influence.outgoing[0].delta_p_bull, Some(-0.25));
    }

    #[test]
    fn probe_accuracy_queries_rank_and_lookup() {
        let mut accuracy = HashMap::new();
        accuracy.insert("B.US".to_string(), 0.7);
        accuracy.insert("A.US".to_string(), 0.9);

        let ranked = ranked_probe_accuracy(&accuracy);
        let one = probe_accuracy_for_symbol(&accuracy, "B.US").unwrap();

        assert_eq!(ranked[0].symbol, "A.US");
        assert_eq!(one.mean_accuracy, 0.7);
    }
}
