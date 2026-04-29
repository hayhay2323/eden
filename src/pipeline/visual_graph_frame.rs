//! Visual graph frame export.
//!
//! This module materializes Eden's backend visual model as a single
//! inspectable graph frame: active sub-KG nodes, sub-KG edges, master
//! KG edges, and BP posterior state. It is artifact-only and never
//! feeds back into inference.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::core::market::MarketDataCapability;
use crate::core::market::MarketRegistry;
use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};
use crate::pipeline::encoded_tick_frame::{EncodedSubKgFrame, EncodedTickFrame};
use crate::pipeline::loopy_bp::{
    GraphEdge, NodePrior, N_STATES, STATE_BEAR, STATE_BULL, STATE_NEUTRAL,
};
use crate::pipeline::symbol_sub_kg::{
    NodeFreshness, NodeProvenanceSource, SubKgRegistry, SymbolSubKG,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualGraphFrame {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub counts: VisualGraphCounts,
    pub symbols: Vec<VisualSymbolFrame>,
    pub master_edges: Vec<VisualMasterEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualGraphCounts {
    pub symbols: usize,
    pub subkg_nodes: usize,
    pub subkg_edges: usize,
    pub master_edges: usize,
    pub observed_priors: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualSymbolFrame {
    pub symbol: String,
    pub observed_prior: bool,
    pub p_bull: f64,
    pub p_bear: f64,
    pub p_neutral: f64,
    pub nodes: Vec<VisualSubKgNode>,
    pub edges: Vec<VisualSubKgEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualSubKgNode {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aux: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub last_seen_tick: u64,
    pub age_ticks: u64,
    pub freshness: NodeFreshness,
    pub provenance_source: NodeProvenanceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_capability: Option<MarketDataCapability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualSubKgEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualMasterEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_p_bull: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_p_bull: Option<f64>,
}

pub fn build_visual_graph_frame(
    market: &str,
    tick: u64,
    registry: &SubKgRegistry,
    master_edges: &[GraphEdge],
    priors: &HashMap<String, NodePrior>,
    beliefs: &HashMap<String, [f64; N_STATES]>,
    ts: DateTime<Utc>,
) -> VisualGraphFrame {
    let mut symbols: Vec<String> = registry.graphs.keys().cloned().collect();
    symbols.sort();

    let mut symbol_frames = Vec::with_capacity(symbols.len());
    let mut subkg_node_count = 0usize;
    let mut subkg_edge_count = 0usize;
    let mut observed_priors = 0usize;

    for symbol in symbols {
        let Some(kg) = registry.graphs.get(&symbol) else {
            continue;
        };
        let prior = priors.get(&symbol).cloned().unwrap_or_default();
        if prior.observed {
            observed_priors += 1;
        }
        let posterior = beliefs
            .get(&symbol)
            .copied()
            .unwrap_or([1.0 / N_STATES as f64; N_STATES]);
        let (nodes, visible_node_ids) = visual_nodes(kg);
        let edges = visual_edges(kg, &visible_node_ids);
        subkg_node_count += nodes.len();
        subkg_edge_count += edges.len();
        symbol_frames.push(VisualSymbolFrame {
            symbol,
            observed_prior: prior.observed,
            p_bull: posterior[STATE_BULL],
            p_bear: posterior[STATE_BEAR],
            p_neutral: posterior[STATE_NEUTRAL],
            nodes,
            edges,
        });
    }

    let mut visual_master_edges: Vec<VisualMasterEdge> = master_edges
        .iter()
        .map(|edge| VisualMasterEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            weight: edge.weight,
            from_p_bull: beliefs.get(&edge.from).map(|b| b[STATE_BULL]),
            to_p_bull: beliefs.get(&edge.to).map(|b| b[STATE_BULL]),
        })
        .collect();
    visual_master_edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.weight.total_cmp(&b.weight))
    });

    VisualGraphFrame {
        ts,
        market: market.to_string(),
        tick,
        counts: VisualGraphCounts {
            symbols: symbol_frames.len(),
            subkg_nodes: subkg_node_count,
            subkg_edges: subkg_edge_count,
            master_edges: visual_master_edges.len(),
            observed_priors,
        },
        symbols: symbol_frames,
        master_edges: visual_master_edges,
    }
}

pub fn build_visual_graph_frame_from_encoded(encoded: &EncodedTickFrame) -> VisualGraphFrame {
    let p_bull_by_symbol: HashMap<&str, f64> = encoded
        .symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .bp
                .as_ref()
                .map(|bp| (symbol.symbol.as_str(), bp.p_bull))
        })
        .collect();

    let mut symbol_frames = Vec::new();
    let mut subkg_node_count = 0usize;
    let mut subkg_edge_count = 0usize;
    let mut observed_priors = 0usize;

    for symbol in &encoded.symbols {
        if symbol.subkg.is_none() {
            continue;
        }

        let bp = symbol.bp.as_ref();
        if bp.map(|bp| bp.observed_prior).unwrap_or(false) {
            observed_priors += 1;
        }

        let (nodes, edges) = symbol
            .subkg
            .as_ref()
            .map(visual_from_encoded_subkg)
            .unwrap_or_default();
        subkg_node_count += nodes.len();
        subkg_edge_count += edges.len();

        symbol_frames.push(VisualSymbolFrame {
            symbol: symbol.symbol.clone(),
            observed_prior: bp.map(|bp| bp.observed_prior).unwrap_or(false),
            p_bull: bp.map(|bp| bp.p_bull).unwrap_or(1.0 / N_STATES as f64),
            p_bear: bp.map(|bp| bp.p_bear).unwrap_or(1.0 / N_STATES as f64),
            p_neutral: bp.map(|bp| bp.p_neutral).unwrap_or(1.0 / N_STATES as f64),
            nodes,
            edges,
        });
    }
    symbol_frames.sort_by(|a, b| a.symbol.cmp(&b.symbol));

    let mut master_edges: Vec<VisualMasterEdge> = encoded
        .master_edges
        .iter()
        .map(|edge| VisualMasterEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            weight: edge.weight,
            from_p_bull: p_bull_by_symbol.get(edge.from.as_str()).copied(),
            to_p_bull: p_bull_by_symbol.get(edge.to.as_str()).copied(),
        })
        .collect();
    master_edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.weight.total_cmp(&b.weight))
    });

    VisualGraphFrame {
        ts: encoded.ts.clone(),
        market: encoded.market.clone(),
        tick: encoded.tick,
        counts: VisualGraphCounts {
            symbols: symbol_frames.len(),
            subkg_nodes: subkg_node_count,
            subkg_edges: subkg_edge_count,
            master_edges: master_edges.len(),
            observed_priors,
        },
        symbols: symbol_frames,
        master_edges,
    }
}

pub fn write_frame(market: &str, frame: &VisualGraphFrame) -> std::io::Result<usize> {
    let market = MarketRegistry::by_slug(market).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown market for visual graph frame: {market}"),
        )
    })?;
    RuntimeArtifactStore::default().append_json_line(
        RuntimeArtifactKind::VisualGraphFrame,
        market,
        frame,
    )?;
    Ok(1)
}

fn visual_nodes(kg: &SymbolSubKG) -> (Vec<VisualSubKgNode>, HashSet<String>) {
    let mut rows: Vec<VisualSubKgNode> = kg
        .nodes
        .iter()
        .filter(|(_, activation)| {
            node_is_visible(activation.value, activation.aux, &activation.label)
        })
        .map(|(id, activation)| VisualSubKgNode {
            id: id.to_serde_key(),
            kind: format!("{:?}", activation.kind),
            value: activation.value.and_then(|v| v.to_f64()),
            aux: activation.aux.and_then(|v| v.to_f64()),
            label: activation.label.clone(),
            last_seen_tick: activation.last_seen_tick,
            age_ticks: activation.age_ticks,
            freshness: activation.freshness,
            provenance_source: activation.provenance_source,
            market_capability: activation.market_capability,
        })
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    let visible = rows.iter().map(|n| n.id.clone()).collect();
    (rows, visible)
}

fn visual_edges(kg: &SymbolSubKG, visible_node_ids: &HashSet<String>) -> Vec<VisualSubKgEdge> {
    let mut rows: Vec<VisualSubKgEdge> = kg
        .edges
        .iter()
        .filter_map(|edge| {
            let from = edge.from.to_serde_key();
            let to = edge.to.to_serde_key();
            if !visible_node_ids.contains(&from) || !visible_node_ids.contains(&to) {
                return None;
            }
            Some(VisualSubKgEdge {
                from,
                to,
                kind: format!("{:?}", edge.kind),
                weight: edge.weight.to_f64().unwrap_or(0.0),
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    rows
}

fn visual_from_encoded_subkg(
    subkg: &EncodedSubKgFrame,
) -> (Vec<VisualSubKgNode>, Vec<VisualSubKgEdge>) {
    let mut nodes: Vec<VisualSubKgNode> = subkg
        .nodes
        .iter()
        .filter(|node| encoded_node_is_visible(node.value, node.aux, &node.label))
        .map(|node| VisualSubKgNode {
            id: node.id.clone(),
            kind: node.kind.clone(),
            value: node.value,
            aux: node.aux,
            label: node.label.clone(),
            last_seen_tick: node.last_seen_tick,
            age_ticks: node.age_ticks,
            freshness: node.freshness,
            provenance_source: node.provenance_source,
            market_capability: node.market_capability,
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let visible_node_ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    let mut edges: Vec<VisualSubKgEdge> = subkg
        .edges
        .iter()
        .filter(|edge| visible_node_ids.contains(&edge.from) && visible_node_ids.contains(&edge.to))
        .map(|edge| VisualSubKgEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            kind: edge.kind.clone(),
            weight: edge.weight,
        })
        .collect();
    edges.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.kind.cmp(&b.kind))
    });

    (nodes, edges)
}

fn node_is_visible(value: Option<Decimal>, aux: Option<Decimal>, label: &Option<String>) -> bool {
    value.map(|v| v != Decimal::ZERO).unwrap_or(false) || aux.is_some() || label.is_some()
}

fn encoded_node_is_visible(value: Option<f64>, aux: Option<f64>, label: &Option<String>) -> bool {
    value.map(|v| v != 0.0).unwrap_or(false) || aux.is_some() || label.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    #[test]
    fn frame_contains_subkg_nodes_master_edges_and_bp_posterior() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::new();
        let kg = registry.upsert("A.US", now);
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.7), now);
        registry.upsert("B.US", now);

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
        beliefs.insert("B.US".to_string(), [0.6, 0.2, 0.2]);
        let edges = vec![GraphEdge {
            from: "A.US".to_string(),
            to: "B.US".to_string(),
            weight: 0.9,
            kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
        }];

        let frame = build_visual_graph_frame("us", 12, &registry, &edges, &priors, &beliefs, now);

        assert_eq!(frame.counts.symbols, 2);
        assert_eq!(frame.counts.master_edges, 1);
        assert_eq!(frame.counts.observed_priors, 1);
        assert!(frame.counts.subkg_nodes >= 1);
        let a = frame.symbols.iter().find(|s| s.symbol == "A.US").unwrap();
        assert_eq!(a.p_bull, 0.8);
        let pressure = a
            .nodes
            .iter()
            .find(|n| n.id == "PressureCapitalFlow")
            .expect("pressure node");
        assert_eq!(
            pressure.provenance_source,
            crate::pipeline::symbol_sub_kg::NodeProvenanceSource::PressureField
        );
        assert!(a.nodes.iter().any(|n| n.id == "PressureCapitalFlow"));
    }

    #[test]
    fn visual_frame_parity_raw_vs_encoded() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::new();
        let kg = registry.upsert("A.US", now);
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.7), now);
        registry.upsert("B.US", now);

        let mut priors = HashMap::new();
        priors.insert(
            "A.US".to_string(),
            NodePrior {
                belief: [0.7, 0.2, 0.1],
                observed: true,
            },
        );
        let mut beliefs = HashMap::new();
        beliefs.insert("A.US".to_string(), [0.8, 0.1, 0.1]);
        beliefs.insert("B.US".to_string(), [0.6, 0.2, 0.2]);
        let edges = vec![GraphEdge {
            from: "A.US".to_string(),
            to: "B.US".to_string(),
            weight: 0.9,
            kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
        }];

        let raw_frame =
            build_visual_graph_frame("us", 12, &registry, &edges, &priors, &beliefs, now);

        let mut encoded = EncodedTickFrame::new("us", 12, now);
        encoded.attach_subkg_registry(&registry);
        encoded.attach_bp_state(&priors, &beliefs, &edges);

        let frame = build_visual_graph_frame_from_encoded(&encoded);

        assert_eq!(frame, raw_frame);
        assert_eq!(frame.market, "us");
        assert_eq!(frame.counts.symbols, 2);
        assert_eq!(frame.counts.master_edges, 1);
        assert_eq!(frame.counts.observed_priors, 1);
        let a = frame.symbols.iter().find(|s| s.symbol == "A.US").unwrap();
        assert_eq!(a.p_bull, 0.8);
        assert!(a.nodes.iter().any(|n| n.id == "PressureCapitalFlow"));
        assert_eq!(frame.master_edges[0].from_p_bull, Some(0.8));
    }
}
