//! Cross-symbol activation propagation along master-KG edges.
//!
//! Each sub-KG (per-symbol) holds local activations from observed data.
//! Master KG (BrainGraph for HK / UsGraph for US) carries Stock-Stock
//! edges representing peer / sector / similarity relationships.
//!
//! When sub-KG A's continuous activations (Pressure, Intent) take a
//! value, that activation propagates to sub-KG B via the master-KG
//! edge with magnitude `source_value × edge_weight × propagation_rate`.
//!
//! This is pure graph physics — no inference rules, no thresholds for
//! "what counts as a signal". Propagation is the consequence of the
//! topology, not a learned policy.
//!
//! Output: `.run/eden-propagation-{market}.ndjson` — for each symbol,
//! the total propagated influx per node kind (sum across neighbors).
//! Operator can read propagation snapshots to see "1347 is receiving
//! cross-symbol activation from 981 / 800 / etc on PressureCapitalFlow".

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, NodeKind, SubKgRegistry};

/// Default propagation rate: how much of source activation transfers
/// per unit edge weight. Universal mechanism (not per-symbol tuned).
pub const DEFAULT_PROPAGATION_RATE: f64 = 0.50;

/// Master-KG edge: directed from src to dst with weight in [0, 1].
#[derive(Debug, Clone)]
pub struct MasterEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub edge_type: String, // e.g., "StockToStock", "PeerSimilarity"
}

#[derive(Debug, Clone, Serialize)]
pub struct PropagationSnapshot {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    /// For each propagable node kind, total influx received from
    /// master-KG neighbors (sum of source_value × edge_weight × rate).
    pub influx_per_node_kind: HashMap<String, f64>,
    /// Per-source contribution: which neighbors contributed and how much.
    pub source_contributions: Vec<SourceContribution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceContribution {
    pub source_symbol: String,
    pub edge_type: String,
    pub edge_weight: f64,
    pub total_transferred: f64, // sum across all node kinds
}

/// Compute cross-symbol activation propagation for one tick.
///
/// For each master-KG edge (src, dst, weight, type):
///   For each propagable NodeKind (Pressure, Intent) in src's sub-KG:
///     transfer = abs(src_value) * weight * rate
///     accumulate into dst's PropagationSnapshot
///
/// Returns one PropagationSnapshot per destination symbol that received
/// any influx (sources are left out of the output).
pub fn propagate(
    market: &str,
    registry: &SubKgRegistry,
    edges: &[MasterEdge],
    rate: f64,
    ts: DateTime<Utc>,
) -> Vec<PropagationSnapshot> {
    // Per-symbol per-kind influx accumulator
    let mut influx: HashMap<String, HashMap<String, f64>> = HashMap::new();
    // Per-symbol source contributions
    let mut sources: HashMap<String, HashMap<String, SourceContribution>> = HashMap::new();

    let propagable_kinds = [NodeKind::Pressure, NodeKind::Intent];

    for edge in edges {
        let src = match registry.get(&edge.from) {
            Some(s) => s,
            None => continue,
        };
        let mut transfer_total = 0.0_f64;

        for (id, act) in &src.nodes {
            if !propagable_kinds.contains(&act.kind) {
                continue;
            }
            // Skip the symbol root and references
            if matches!(id, NodeId::Symbol | NodeId::SectorRef) {
                continue;
            }
            let v = act
                .value
                .map(|x| x.abs().to_f64().unwrap_or(0.0))
                .unwrap_or(0.0);
            if v < f64::EPSILON {
                continue;
            }
            let transfer = v * edge.weight * rate;
            transfer_total += transfer;

            let kind_label = format!("{:?}", act.kind);
            let dst_influx = influx.entry(edge.to.clone()).or_default();
            *dst_influx.entry(kind_label).or_insert(0.0) += transfer;
        }

        if transfer_total > f64::EPSILON {
            let dst_sources = sources.entry(edge.to.clone()).or_default();
            let key = format!("{}|{}", edge.from, edge.edge_type);
            dst_sources
                .entry(key)
                .and_modify(|c| c.total_transferred += transfer_total)
                .or_insert(SourceContribution {
                    source_symbol: edge.from.clone(),
                    edge_type: edge.edge_type.clone(),
                    edge_weight: edge.weight,
                    total_transferred: transfer_total,
                });
        }
    }

    // Build snapshots
    let mut snapshots = Vec::new();
    for (sym, kinds) in influx {
        let mut contribs: Vec<SourceContribution> = sources
            .remove(&sym)
            .map(|m| m.into_values().collect())
            .unwrap_or_default();
        contribs.sort_by(|a, b| {
            b.total_transferred
                .partial_cmp(&a.total_transferred)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        snapshots.push(PropagationSnapshot {
            ts,
            market: market.to_string(),
            symbol: sym,
            influx_per_node_kind: kinds,
            source_contributions: contribs,
        });
    }
    snapshots
}

/// Append one snapshot per symbol per call to NDJSON.
pub fn write_snapshots(market: &str, snapshots: &[PropagationSnapshot]) -> std::io::Result<()> {
    if snapshots.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-propagation-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    for snap in snapshots {
        let line = serde_json::to_string(snap)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::SubKgRegistry;
    use rust_decimal_macros::dec;

    #[test]
    fn no_propagation_when_no_edges() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("A.HK", Utc::now()).set_node_value(
            NodeId::PressureCapitalFlow,
            dec!(0.8),
            Utc::now(),
        );
        let snaps = propagate("hk", &reg, &[], DEFAULT_PROPAGATION_RATE, Utc::now());
        assert!(snaps.is_empty());
    }

    #[test]
    fn propagation_transfers_fraction_along_edge() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("A.HK", Utc::now()).set_node_value(
            NodeId::PressureCapitalFlow,
            dec!(0.8),
            Utc::now(),
        );
        reg.upsert("B.HK", Utc::now()); // empty sub-KG
        let edges = vec![MasterEdge {
            from: "A.HK".into(),
            to: "B.HK".into(),
            weight: 0.5,
            edge_type: "StockToStock".into(),
        }];
        let snaps = propagate("hk", &reg, &edges, 1.0, Utc::now());
        assert_eq!(snaps.len(), 1);
        let s = &snaps[0];
        assert_eq!(s.symbol, "B.HK");
        // Influx: 0.8 * 0.5 * 1.0 = 0.4
        let pressure_influx = s
            .influx_per_node_kind
            .get("Pressure")
            .copied()
            .unwrap_or(0.0);
        assert!((pressure_influx - 0.4).abs() < 1e-6);
    }

    #[test]
    fn multiple_sources_aggregate_into_destination() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("A.HK", Utc::now()).set_node_value(
            NodeId::PressureCapitalFlow,
            dec!(0.6),
            Utc::now(),
        );
        reg.upsert("C.HK", Utc::now()).set_node_value(
            NodeId::PressureCapitalFlow,
            dec!(0.4),
            Utc::now(),
        );
        reg.upsert("B.HK", Utc::now());
        let edges = vec![
            MasterEdge {
                from: "A.HK".into(),
                to: "B.HK".into(),
                weight: 0.5,
                edge_type: "Peer".into(),
            },
            MasterEdge {
                from: "C.HK".into(),
                to: "B.HK".into(),
                weight: 0.3,
                edge_type: "Peer".into(),
            },
        ];
        let snaps = propagate("hk", &reg, &edges, 1.0, Utc::now());
        let b_snap = snaps.iter().find(|s| s.symbol == "B.HK").unwrap();
        // Influx: 0.6*0.5 + 0.4*0.3 = 0.3 + 0.12 = 0.42
        let p = b_snap.influx_per_node_kind["Pressure"];
        assert!((p - 0.42).abs() < 1e-6);
        assert_eq!(b_snap.source_contributions.len(), 2);
    }

    #[test]
    fn propagation_only_on_pressure_intent_not_price() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("A.HK", Utc::now())
            .set_node_value(NodeId::LastPrice, dec!(50.0), Utc::now());
        reg.upsert("B.HK", Utc::now());
        let edges = vec![MasterEdge {
            from: "A.HK".into(),
            to: "B.HK".into(),
            weight: 1.0,
            edge_type: "Peer".into(),
        }];
        let snaps = propagate("hk", &reg, &edges, 1.0, Utc::now());
        // Price doesn't propagate, so no snapshot for B
        assert!(snaps.is_empty());
    }
}
