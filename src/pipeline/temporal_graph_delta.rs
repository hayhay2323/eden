//! Temporal deltas between visual graph frames.
//!
//! Deltas are an observability artifact: they describe how the graph
//! changed between two `VisualGraphFrame`s. They are not read by BP and
//! do not alter sub-KG or master-KG state.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::pipeline::visual_graph_frame::{
    VisualGraphFrame, VisualMasterEdge, VisualSubKgEdge, VisualSubKgNode, VisualSymbolFrame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaStatus {
    Appeared,
    Disappeared,
    Changed,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemporalGraphDelta {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub previous_tick: u64,
    pub counts: TemporalGraphDeltaCounts,
    pub node_deltas: Vec<NodeDelta>,
    pub subkg_edge_deltas: Vec<SubKgEdgeDelta>,
    pub master_edge_deltas: Vec<MasterEdgeDelta>,
    pub posterior_deltas: Vec<PosteriorDelta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TemporalGraphDeltaCounts {
    pub node_deltas: usize,
    pub subkg_edge_deltas: usize,
    pub master_edge_deltas: usize,
    pub posterior_deltas: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeDelta {
    pub symbol: String,
    pub node_id: String,
    pub kind: String,
    pub status: DeltaStatus,
    pub previous_value: Option<f64>,
    pub current_value: Option<f64>,
    pub value_delta: Option<f64>,
    pub previous_aux: Option<f64>,
    pub current_aux: Option<f64>,
    pub aux_delta: Option<f64>,
    pub previous_label: Option<String>,
    pub current_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubKgEdgeDelta {
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub status: DeltaStatus,
    pub previous_weight: Option<f64>,
    pub current_weight: Option<f64>,
    pub weight_delta: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MasterEdgeDelta {
    pub from: String,
    pub to: String,
    pub status: DeltaStatus,
    pub previous_weight: Option<f64>,
    pub current_weight: Option<f64>,
    pub weight_delta: Option<f64>,
    pub previous_from_p_bull: Option<f64>,
    pub current_from_p_bull: Option<f64>,
    pub previous_to_p_bull: Option<f64>,
    pub current_to_p_bull: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PosteriorDelta {
    pub symbol: String,
    pub previous_p_bull: f64,
    pub current_p_bull: f64,
    pub delta_p_bull: f64,
    pub previous_p_bear: f64,
    pub current_p_bear: f64,
    pub delta_p_bear: f64,
    pub previous_p_neutral: f64,
    pub current_p_neutral: f64,
    pub delta_p_neutral: f64,
}

pub fn build_delta(
    market: &str,
    tick: u64,
    previous: &VisualGraphFrame,
    current: &VisualGraphFrame,
    ts: DateTime<Utc>,
) -> TemporalGraphDelta {
    let node_deltas = node_deltas(previous, current);
    let subkg_edge_deltas = subkg_edge_deltas(previous, current);
    let master_edge_deltas = master_edge_deltas(previous, current);
    let posterior_deltas = posterior_deltas(previous, current);

    TemporalGraphDelta {
        ts,
        market: market.to_string(),
        tick,
        previous_tick: previous.tick,
        counts: TemporalGraphDeltaCounts {
            node_deltas: node_deltas.len(),
            subkg_edge_deltas: subkg_edge_deltas.len(),
            master_edge_deltas: master_edge_deltas.len(),
            posterior_deltas: posterior_deltas.len(),
        },
        node_deltas,
        subkg_edge_deltas,
        master_edge_deltas,
        posterior_deltas,
    }
}

pub fn write_delta(market: &str, delta: &TemporalGraphDelta) -> std::io::Result<usize> {
    let path = format!(".run/eden-graph-delta-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(delta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(file, "{}", line)?;
    Ok(1)
}

fn node_deltas(previous: &VisualGraphFrame, current: &VisualGraphFrame) -> Vec<NodeDelta> {
    let previous_nodes = node_map(previous);
    let current_nodes = node_map(current);
    let keys = union_keys(previous_nodes.keys(), current_nodes.keys());
    let mut rows = Vec::new();

    for key in keys {
        let prev = previous_nodes.get(&key);
        let curr = current_nodes.get(&key);
        let status = status(prev.is_some(), curr.is_some(), || prev != curr);
        let Some(status) = status else {
            continue;
        };
        let node = curr.or(prev).expect("one side exists");
        rows.push(NodeDelta {
            symbol: key.0.clone(),
            node_id: key.1.clone(),
            kind: node.kind.clone(),
            status,
            previous_value: prev.and_then(|n| n.value),
            current_value: curr.and_then(|n| n.value),
            value_delta: option_delta(prev.and_then(|n| n.value), curr.and_then(|n| n.value)),
            previous_aux: prev.and_then(|n| n.aux),
            current_aux: curr.and_then(|n| n.aux),
            aux_delta: option_delta(prev.and_then(|n| n.aux), curr.and_then(|n| n.aux)),
            previous_label: prev.and_then(|n| n.label.clone()),
            current_label: curr.and_then(|n| n.label.clone()),
        });
    }
    rows.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then_with(|| a.node_id.cmp(&b.node_id))
    });
    rows
}

fn subkg_edge_deltas(
    previous: &VisualGraphFrame,
    current: &VisualGraphFrame,
) -> Vec<SubKgEdgeDelta> {
    let previous_edges = subkg_edge_map(previous);
    let current_edges = subkg_edge_map(current);
    let keys = union_keys(previous_edges.keys(), current_edges.keys());
    let mut rows = Vec::new();

    for key in keys {
        let prev = previous_edges.get(&key);
        let curr = current_edges.get(&key);
        let status = status(prev.is_some(), curr.is_some(), || prev != curr);
        let Some(status) = status else {
            continue;
        };
        let edge = curr.or(prev).expect("one side exists");
        rows.push(SubKgEdgeDelta {
            symbol: key.0.clone(),
            from: key.1.clone(),
            to: key.2.clone(),
            kind: edge.kind.clone(),
            status,
            previous_weight: prev.map(|e| e.weight),
            current_weight: curr.map(|e| e.weight),
            weight_delta: option_delta(prev.map(|e| e.weight), curr.map(|e| e.weight)),
        });
    }
    rows.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then_with(|| a.from.cmp(&b.from))
            .then_with(|| a.to.cmp(&b.to))
            .then_with(|| a.kind.cmp(&b.kind))
    });
    rows
}

fn master_edge_deltas(
    previous: &VisualGraphFrame,
    current: &VisualGraphFrame,
) -> Vec<MasterEdgeDelta> {
    let previous_edges = master_edge_map(previous);
    let current_edges = master_edge_map(current);
    let keys = union_keys(previous_edges.keys(), current_edges.keys());
    let mut rows = Vec::new();

    for key in keys {
        let prev = previous_edges.get(&key);
        let curr = current_edges.get(&key);
        let status = status(prev.is_some(), curr.is_some(), || prev != curr);
        let Some(status) = status else {
            continue;
        };
        rows.push(MasterEdgeDelta {
            from: key.0.clone(),
            to: key.1.clone(),
            status,
            previous_weight: prev.map(|e| e.weight),
            current_weight: curr.map(|e| e.weight),
            weight_delta: option_delta(prev.map(|e| e.weight), curr.map(|e| e.weight)),
            previous_from_p_bull: prev.and_then(|e| e.from_p_bull),
            current_from_p_bull: curr.and_then(|e| e.from_p_bull),
            previous_to_p_bull: prev.and_then(|e| e.to_p_bull),
            current_to_p_bull: curr.and_then(|e| e.to_p_bull),
        });
    }
    rows.sort_by(|a, b| a.from.cmp(&b.from).then_with(|| a.to.cmp(&b.to)));
    rows
}

fn posterior_deltas(
    previous: &VisualGraphFrame,
    current: &VisualGraphFrame,
) -> Vec<PosteriorDelta> {
    let previous_symbols = symbol_map(previous);
    let current_symbols = symbol_map(current);
    let keys = union_keys(previous_symbols.keys(), current_symbols.keys());
    let mut rows = Vec::new();

    for symbol in keys {
        let Some(prev) = previous_symbols.get(&symbol) else {
            continue;
        };
        let Some(curr) = current_symbols.get(&symbol) else {
            continue;
        };
        if prev.p_bull == curr.p_bull
            && prev.p_bear == curr.p_bear
            && prev.p_neutral == curr.p_neutral
        {
            continue;
        }
        rows.push(PosteriorDelta {
            symbol: symbol.clone(),
            previous_p_bull: prev.p_bull,
            current_p_bull: curr.p_bull,
            delta_p_bull: curr.p_bull - prev.p_bull,
            previous_p_bear: prev.p_bear,
            current_p_bear: curr.p_bear,
            delta_p_bear: curr.p_bear - prev.p_bear,
            previous_p_neutral: prev.p_neutral,
            current_p_neutral: curr.p_neutral,
            delta_p_neutral: curr.p_neutral - prev.p_neutral,
        });
    }
    rows.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    rows
}

fn node_map(frame: &VisualGraphFrame) -> HashMap<(String, String), VisualSubKgNode> {
    let mut out = HashMap::new();
    for symbol in &frame.symbols {
        for node in &symbol.nodes {
            out.insert((symbol.symbol.clone(), node.id.clone()), node.clone());
        }
    }
    out
}

fn subkg_edge_map(
    frame: &VisualGraphFrame,
) -> HashMap<(String, String, String, String), VisualSubKgEdge> {
    let mut out = HashMap::new();
    for symbol in &frame.symbols {
        for edge in &symbol.edges {
            out.insert(
                (
                    symbol.symbol.clone(),
                    edge.from.clone(),
                    edge.to.clone(),
                    edge.kind.clone(),
                ),
                edge.clone(),
            );
        }
    }
    out
}

fn master_edge_map(frame: &VisualGraphFrame) -> HashMap<(String, String), VisualMasterEdge> {
    frame
        .master_edges
        .iter()
        .map(|edge| ((edge.from.clone(), edge.to.clone()), edge.clone()))
        .collect()
}

fn symbol_map(frame: &VisualGraphFrame) -> HashMap<String, VisualSymbolFrame> {
    frame
        .symbols
        .iter()
        .map(|symbol| (symbol.symbol.clone(), symbol.clone()))
        .collect()
}

fn union_keys<'a, K, I>(a: I, b: I) -> Vec<K>
where
    K: Ord + Clone + 'a,
    I: IntoIterator<Item = &'a K>,
{
    let mut keys = BTreeSet::new();
    keys.extend(a.into_iter().cloned());
    keys.extend(b.into_iter().cloned());
    keys.into_iter().collect()
}

fn status<F>(previous: bool, current: bool, changed: F) -> Option<DeltaStatus>
where
    F: FnOnce() -> bool,
{
    match (previous, current) {
        (false, true) => Some(DeltaStatus::Appeared),
        (true, false) => Some(DeltaStatus::Disappeared),
        (true, true) if changed() => Some(DeltaStatus::Changed),
        _ => None,
    }
}

fn option_delta(previous: Option<f64>, current: Option<f64>) -> Option<f64> {
    previous.zip(current).map(|(prev, curr)| curr - prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    use crate::pipeline::loopy_bp::{GraphEdge, NodePrior};
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use crate::pipeline::visual_graph_frame::build_visual_graph_frame;
    use rust_decimal_macros::dec;

    fn frame(node_value: rust_decimal::Decimal, edge_weight: f64, p_bull: f64) -> VisualGraphFrame {
        let now = Utc::now();
        let mut registry = SubKgRegistry::new();
        registry
            .upsert("A.US", now)
            .set_node_value(NodeId::PressureCapitalFlow, node_value, now);
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
        beliefs.insert("A.US".to_string(), [p_bull, 0.1, 0.1]);
        beliefs.insert("B.US".to_string(), [0.6, 0.2, 0.2]);
        let edges = vec![GraphEdge {
            from: "A.US".to_string(),
            to: "B.US".to_string(),
            weight: edge_weight,
            kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
        }];

        build_visual_graph_frame("us", 1, &registry, &edges, &priors, &beliefs, now)
    }

    #[test]
    fn delta_reports_node_master_edge_and_posterior_drift() {
        let previous = frame(dec!(0.5), 0.4, 0.7);
        let current = frame(dec!(0.8), 0.9, 0.9);

        let delta = build_delta("us", 2, &previous, &current, Utc::now());

        assert!(delta
            .node_deltas
            .iter()
            .any(|d| d.node_id == "PressureCapitalFlow"
                && d.value_delta
                    .map(|v| (v - 0.3).abs() < 1e-9)
                    .unwrap_or(false)));
        assert!(delta.master_edge_deltas.iter().any(|d| d.from == "A.US"
            && d.to == "B.US"
            && d.weight_delta
                .map(|v| (v - 0.5).abs() < 1e-9)
                .unwrap_or(false)));
        assert!(delta
            .posterior_deltas
            .iter()
            .any(|d| d.symbol == "A.US" && (d.delta_p_bull - 0.2).abs() < 1e-9));
    }

    #[test]
    fn delta_omits_unchanged_frames() {
        let previous = frame(dec!(0.5), 0.4, 0.7);
        let current = frame(dec!(0.5), 0.4, 0.7);

        let delta = build_delta("us", 2, &previous, &current, Utc::now());

        assert_eq!(delta.counts.node_deltas, 0);
        assert_eq!(delta.counts.master_edge_deltas, 0);
        assert_eq!(delta.counts.posterior_deltas, 0);
    }
}
