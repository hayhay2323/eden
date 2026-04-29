//! Weisfeiler-Lehman graph signature on Eden's typed sub-KG.
//!
//! Eden's per-symbol sub-KG is a typed graph: 92 nodes with 28
//! NodeKinds, ~250 edges with 23 EdgeKinds, plus dynamic broker /
//! fund-holder nodes. Reducing it to a 5-dimensional regime
//! fingerprint (or any flat vector) throws away the topology + the
//! type information that make Eden Eden.
//!
//! WL signature is the right tool for typed-graph similarity:
//!
//!   for each node v, initial label = (NodeKind, value-bucket) tuple
//!   for h iterations:
//!     new_label(v) = hash( old_label(v) ‖ sorted(neighbor labels)
//!                          interleaved with (edge_kind, neighbor) tuples )
//!   signature = multiset histogram of all final labels
//!
//! Two sub-KGs that have the same multiset are h-WL-equivalent —
//! structurally indistinguishable up to depth h. With h=2 or 3 this
//! captures local typed-graph patterns exactly.
//!
//! Pure deterministic. No learning. No magic threshold. O(h × |edges|)
//! per signature.
//!
//! Output: `.run/eden-wl-signatures-{market}.ndjson` — one row per
//! symbol per snapshot tick. Per-symbol signature_hash becomes the key
//! for graph-structural analog lookup (a future iteration replaces the
//! flat-bucket regime_analog_index).

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{EdgeKind, NodeActivation, NodeId, SymbolSubKG};

/// WL relabel iterations. h=2 captures depth-2 local structure
/// (every node "knows" its 2-hop neighborhood). Higher h gives more
/// resolution at cost of more compute. 2 is the sweet spot for typed
/// graphs of this density.
pub const WL_ITERATIONS: usize = 2;

/// Quantize NodeActivation.value into a coarse bucket. Pure rule —
/// thresholds chosen to give Pressure-style and Intent-style nodes
/// (which share [-1, 1] / [0, 1] semantics) discrete buckets without
/// over-fragmenting.
fn value_bucket(act: &NodeActivation) -> &'static str {
    match act.value {
        None => "off",
        Some(v) => {
            let f = v.abs().to_f64().unwrap_or(0.0);
            if f >= 0.7 {
                "hi"
            } else if f >= 0.3 {
                "md"
            } else if f > 0.0 {
                "lo"
            } else {
                "z0"
            }
        }
    }
}

/// Initial label = (NodeKind, value bucket). Captures both type and
/// magnitude so two pressure nodes with very different magnitudes
/// don't collapse to the same identity.
fn initial_label(act: &NodeActivation) -> String {
    format!("{:?}:{}", act.kind, value_bucket(act))
}

/// One WL relabel step. New label = hash(old || sorted(neighbor edge-tagged labels)).
fn relabel_step(
    nodes: &HashMap<NodeId, NodeActivation>,
    current_labels: &HashMap<NodeId, String>,
    neighbors: &HashMap<NodeId, Vec<(EdgeKind, NodeId)>>,
) -> HashMap<NodeId, String> {
    let mut next = HashMap::with_capacity(nodes.len());
    for (id, _act) in nodes {
        let own = current_labels
            .get(id)
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        // Tag each neighbor with the edge kind so two neighbors of
        // different relationship types don't fold into the same
        // multiset entry.
        let mut nbr_labels: Vec<String> = match neighbors.get(id) {
            Some(list) => list
                .iter()
                .map(|(ek, n)| {
                    let nl = current_labels
                        .get(n)
                        .cloned()
                        .unwrap_or_else(|| "?".to_string());
                    format!("{:?}|{}", ek, nl)
                })
                .collect(),
            None => Vec::new(),
        };
        nbr_labels.sort();
        // Stable hash of (own || sorted neighbor list).
        let mut h = std::collections::hash_map::DefaultHasher::new();
        own.hash(&mut h);
        for nl in &nbr_labels {
            nl.hash(&mut h);
        }
        next.insert(id.clone(), format!("{:016x}", h.finish()));
    }
    next
}

/// Build adjacency (NodeId → list of (EdgeKind, neighbor NodeId))
/// from the sub-KG edges. Treats edges as undirected for WL purposes —
/// two-way propagation reflects the spatial nature of the graph.
fn build_adjacency(kg: &SymbolSubKG) -> HashMap<NodeId, Vec<(EdgeKind, NodeId)>> {
    let mut adj: HashMap<NodeId, Vec<(EdgeKind, NodeId)>> = HashMap::new();
    for e in &kg.edges {
        adj.entry(e.from.clone())
            .or_default()
            .push((e.kind, e.to.clone()));
        adj.entry(e.to.clone())
            .or_default()
            .push((e.kind, e.from.clone()));
    }
    adj
}

/// Per-sub-KG WL signature. The histogram is a multiset of final
/// labels (label → count) — two sub-KGs with the same histogram are
/// h-WL-equivalent.
#[derive(Debug, Clone, Serialize)]
pub struct WLSignature {
    pub iterations: usize,
    pub n_nodes: usize,
    /// label → count
    pub histogram: BTreeMap<String, usize>,
    /// Compact stable hash over the full histogram, for O(1)
    /// HashMap-keying of analog index.
    pub signature_hash: String,
}

impl WLSignature {
    pub fn jaccard_similarity(&self, other: &WLSignature) -> f64 {
        let mut inter = 0_usize;
        let mut union = 0_usize;
        let mut keys: std::collections::HashSet<&String> = self.histogram.keys().collect();
        keys.extend(other.histogram.keys());
        for k in keys {
            let a = self.histogram.get(k).copied().unwrap_or(0);
            let b = other.histogram.get(k).copied().unwrap_or(0);
            inter += a.min(b);
            union += a.max(b);
        }
        if union == 0 {
            0.0
        } else {
            inter as f64 / union as f64
        }
    }
}

/// Compute WL signature for one sub-KG.
pub fn wl_signature(kg: &SymbolSubKG, iterations: usize) -> WLSignature {
    // Initial labels.
    let mut labels: HashMap<NodeId, String> = kg
        .nodes
        .iter()
        .map(|(id, act)| (id.clone(), initial_label(act)))
        .collect();
    let adj = build_adjacency(kg);
    for _ in 0..iterations {
        labels = relabel_step(&kg.nodes, &labels, &adj);
    }
    // Histogram of final labels.
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    for (_id, l) in labels {
        *hist.entry(l).or_insert(0) += 1;
    }
    // Stable hash over the full histogram (BTreeMap iteration order
    // is deterministic, so the hash is reproducible).
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for (k, v) in &hist {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    let signature_hash = format!("{:016x}", h.finish());
    WLSignature {
        iterations,
        n_nodes: kg.nodes.len(),
        histogram: hist,
        signature_hash,
    }
}

/// Per-symbol signature row, ready for ndjson dump.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolSignatureRow {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    pub iterations: usize,
    pub n_nodes: usize,
    pub signature_hash: String,
    /// Top 10 most common labels (by count) for operator inspection.
    /// Full histogram is implied by signature_hash and recoverable on
    /// recompute, so we don't bloat ndjson with it.
    pub top_labels: Vec<(String, usize)>,
}

pub fn build_signature_rows(
    market: &str,
    registry: &crate::pipeline::symbol_sub_kg::SubKgRegistry,
    iterations: usize,
    ts: DateTime<Utc>,
) -> Vec<SymbolSignatureRow> {
    let mut out = Vec::with_capacity(registry.graphs.len());
    for (sym, kg) in &registry.graphs {
        let sig = wl_signature(kg, iterations);
        let mut top: Vec<(String, usize)> =
            sig.histogram.iter().map(|(k, v)| (k.clone(), *v)).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        top.truncate(10);
        out.push(SymbolSignatureRow {
            ts,
            market: market.to_string(),
            symbol: sym.clone(),
            iterations: sig.iterations,
            n_nodes: sig.n_nodes,
            signature_hash: sig.signature_hash,
            top_labels: top,
        });
    }
    out
}

pub fn write_signature_rows(market: &str, rows: &[SymbolSignatureRow]) -> std::io::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-wl-signatures-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for row in rows {
        let line = serde_json::to_string(row)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    fn fresh(sym: &str) -> SymbolSubKG {
        SymbolSubKG::new_empty(sym.into(), Utc::now())
    }

    #[test]
    fn identical_subkgs_produce_identical_signatures() {
        let kg_a = fresh("A.HK");
        let kg_b = fresh("B.HK");
        let sig_a = wl_signature(&kg_a, WL_ITERATIONS);
        let sig_b = wl_signature(&kg_b, WL_ITERATIONS);
        assert_eq!(
            sig_a.signature_hash, sig_b.signature_hash,
            "two empty sub-KGs should share the signature; differ only by symbol id which we don't include"
        );
        assert_eq!(sig_a.histogram, sig_b.histogram);
    }

    #[test]
    fn value_change_changes_signature() {
        let mut kg_a = fresh("X.HK");
        let mut kg_b = fresh("X.HK");
        kg_a.set_node_value(NodeId::PressureOrderBook, dec!(0.05), Utc::now()); // lo
        kg_b.set_node_value(NodeId::PressureOrderBook, dec!(0.85), Utc::now()); // hi
        let sig_a = wl_signature(&kg_a, WL_ITERATIONS);
        let sig_b = wl_signature(&kg_b, WL_ITERATIONS);
        assert_ne!(
            sig_a.signature_hash, sig_b.signature_hash,
            "value bucket change should propagate via initial labels"
        );
    }

    #[test]
    fn structural_change_changes_signature() {
        // Adding a broker creates a new node + BrokerSits edge, which
        // changes structure, not just values.
        let kg_a = fresh("X.HK");
        let mut kg_b = fresh("X.HK");
        kg_b.add_or_update_broker(
            "B7777".into(),
            Some((crate::pipeline::symbol_sub_kg::Side::Bid, 1)),
            Utc::now(),
        );
        let sig_a = wl_signature(&kg_a, WL_ITERATIONS);
        let sig_b = wl_signature(&kg_b, WL_ITERATIONS);
        assert_ne!(
            sig_a.signature_hash, sig_b.signature_hash,
            "adding a broker node + edge should change structure"
        );
        assert!(sig_b.n_nodes > sig_a.n_nodes);
    }

    #[test]
    fn jaccard_self_is_one() {
        let mut kg = fresh("X.HK");
        kg.set_node_value(NodeId::PressureOrderBook, dec!(0.5), Utc::now());
        kg.set_node_value(NodeId::IntentAccumulation, dec!(0.4), Utc::now());
        let sig = wl_signature(&kg, WL_ITERATIONS);
        let j = sig.jaccard_similarity(&sig);
        assert!((j - 1.0).abs() < 1e-9, "self-jaccard must be 1.0");
    }

    #[test]
    fn jaccard_disjoint_is_low() {
        let mut kg_a = fresh("X.HK");
        let mut kg_b = fresh("Y.HK");
        // Wildly different value patterns → very different histograms.
        kg_a.set_node_value(NodeId::PressureOrderBook, dec!(0.85), Utc::now());
        kg_a.set_node_value(NodeId::IntentAccumulation, dec!(0.85), Utc::now());
        kg_b.set_node_value(NodeId::PressureMomentum, dec!(0.85), Utc::now());
        kg_b.set_node_value(NodeId::IntentDistribution, dec!(0.85), Utc::now());
        let sig_a = wl_signature(&kg_a, WL_ITERATIONS);
        let sig_b = wl_signature(&kg_b, WL_ITERATIONS);
        let j = sig_a.jaccard_similarity(&sig_b);
        // They share most empty nodes so jaccard isn't tiny, but lit
        // patterns differ; just verify it's < 1.0.
        assert!(
            j < 1.0,
            "different lit patterns must reduce jaccard from 1.0"
        );
    }

    #[test]
    fn signature_rows_one_per_symbol() {
        let mut reg = SubKgRegistry::new();
        for sym in ["A.HK", "B.HK", "C.HK"] {
            reg.upsert(sym, Utc::now()).set_node_value(
                NodeId::PressureOrderBook,
                dec!(0.5),
                Utc::now(),
            );
        }
        let rows = build_signature_rows("hk", &reg, WL_ITERATIONS, Utc::now());
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.iterations, WL_ITERATIONS);
            assert!(!row.signature_hash.is_empty());
            assert!(!row.top_labels.is_empty());
        }
    }
}
