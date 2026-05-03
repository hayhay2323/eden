//! Cluster sync detector — emergence layer on top of sub-KG.
//!
//! Reads `SubKgRegistry` (per-symbol typed-node graphs) and master KG
//! cluster definitions (e.g. sector membership). Detects when multiple
//! cluster members show synchronized activation on the same subset of
//! sub-KG node kinds.
//!
//! No direction inference. No pattern templates. Just "these N symbols
//! in this cluster have activated these K node kinds simultaneously" —
//! the SHAPE of co-activation is the signal, not a pre-specified pattern.
//!
//! v1 universal threshold (mechanism-level, not pattern-specific):
//!   - A node is "lit" if its absolute activation > LIT_THRESHOLD.
//!   - A cluster syncs if ≥SYNC_MEMBER_MIN members are lit on the same
//!     ≥SYNC_KIND_MIN node kinds.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, NodeKind, SubKgRegistry};

/// Minimum |activation| to consider a node "lit". This is a universal
/// mechanism threshold, not a pattern-specific rule. Tunable via
/// online learning later (Phase C).
const LIT_THRESHOLD: f64 = 0.30;

/// Minimum cluster members that must be lit together for emergence.
const SYNC_MEMBER_MIN: usize = 3;

/// Minimum distinct node kinds that must be lit together.
const SYNC_KIND_MIN: usize = 2;

#[derive(Debug, Clone, Serialize)]
pub struct ClusterSyncEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    /// Cluster identity (e.g., sector_id).
    pub cluster_key: String,
    pub cluster_total_members: usize,
    /// Subset of cluster members that share the same lit-node-kind set.
    pub sync_member_count: usize,
    pub sync_members: Vec<String>,
    pub lit_node_kinds: Vec<String>,
    /// For each lit kind, mean activation across sync members.
    pub mean_activation_per_kind: HashMap<String, f64>,
    /// Strongest single member contributing to the sync (highest mean
    /// activation across lit kinds).
    pub strongest_member: Option<String>,
    pub strongest_member_mean_activation: f64,
}

/// Run cluster-sync detection.
///
/// `clusters` maps a cluster_key (sector_id, peer_cluster_anchor, etc.) to
/// the symbols that belong. Caller controls cluster definition.
pub fn detect_cluster_sync(
    market: &str,
    registry: &SubKgRegistry,
    clusters: &HashMap<String, Vec<String>>,
    ts: DateTime<Utc>,
) -> Vec<ClusterSyncEvent> {
    let mut events = Vec::new();

    for (cluster_key, members) in clusters {
        if members.len() < SYNC_MEMBER_MIN {
            continue;
        }

        // For each member, compute its lit NodeKinds + per-kind mean activation
        let mut per_member: HashMap<&String, HashMap<NodeKind, f64>> = HashMap::new();
        for sym in members {
            if let Some(kg) = registry.get(sym) {
                let lit = compute_lit_kinds(kg);
                if !lit.is_empty() {
                    per_member.insert(sym, lit);
                }
            }
        }
        if per_member.len() < SYNC_MEMBER_MIN {
            continue;
        }

        // Find the dominant lit-kind set: the largest subset of node kinds
        // that ≥SYNC_MEMBER_MIN members share.
        // v1 simple approach: take intersection of lit kinds across members
        // that have ≥SYNC_KIND_MIN lit kinds, expanded to the largest
        // member-set with intersection size ≥SYNC_KIND_MIN.
        let common_kinds = greatest_common_lit_kinds(&per_member, SYNC_KIND_MIN, SYNC_MEMBER_MIN);
        if common_kinds.is_empty() {
            continue;
        }

        // Identify which members actually share that kind set
        let sync_members: Vec<&String> = per_member
            .iter()
            .filter(|(_, kinds)| common_kinds.iter().all(|k| kinds.contains_key(k)))
            .map(|(sym, _)| *sym)
            .collect();
        if sync_members.len() < SYNC_MEMBER_MIN {
            continue;
        }

        // Compute mean activation per lit kind (across sync members only)
        let mut mean_per_kind: HashMap<String, f64> = HashMap::new();
        for k in &common_kinds {
            let vals: Vec<f64> = sync_members
                .iter()
                .filter_map(|m| per_member.get(*m).and_then(|kinds| kinds.get(k)).copied())
                .collect();
            let mean = if vals.is_empty() {
                0.0
            } else {
                vals.iter().sum::<f64>() / vals.len() as f64
            };
            mean_per_kind.insert(format!("{:?}", k), mean);
        }

        // Strongest member = highest mean across lit kinds
        let mut strongest: Option<(&String, f64)> = None;
        for m in &sync_members {
            if let Some(kinds) = per_member.get(*m) {
                let avg: f64 = common_kinds
                    .iter()
                    .map(|k| kinds.get(k).copied().unwrap_or(0.0))
                    .sum::<f64>()
                    / common_kinds.len().max(1) as f64;
                if strongest.as_ref().map(|(_, v)| avg > *v).unwrap_or(true) {
                    strongest = Some((*m, avg));
                }
            }
        }

        events.push(ClusterSyncEvent {
            ts,
            market: market.to_string(),
            cluster_key: cluster_key.clone(),
            cluster_total_members: members.len(),
            sync_member_count: sync_members.len(),
            sync_members: sync_members.iter().map(|s| s.to_string()).collect(),
            lit_node_kinds: common_kinds.iter().map(|k| format!("{:?}", k)).collect(),
            mean_activation_per_kind: mean_per_kind,
            strongest_member: strongest.map(|(s, _)| s.to_string()),
            strongest_member_mean_activation: strongest.map(|(_, v)| v).unwrap_or(0.0),
        });
    }

    events
}

/// Per-symbol: which NodeKinds are lit + their mean activation magnitude.
///
/// Only NodeKinds with values on a [-1..1]-ish scale count toward lit
/// detection — Pressure (channel net) and Intent (posterior probability).
/// Raw scalars (Price, Volume, Depth) and presence flags (Broker, State)
/// don't have a natural lit threshold; they're always present once data
/// flows. Phase 2: per-kind normalized lit thresholds.
fn compute_lit_kinds(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG) -> HashMap<NodeKind, f64> {
    let mut by_kind: HashMap<NodeKind, Vec<f64>> = HashMap::new();
    for (id, act) in &kg.nodes {
        if matches!(id, NodeId::Symbol | NodeId::SectorRef) {
            continue;
        }
        // Only Pressure + Intent are scale-comparable for v1 lit detection.
        if !matches!(act.kind, NodeKind::Pressure | NodeKind::Intent) {
            continue;
        }
        let mag = act
            .value
            .map(|v| v.abs().to_f64().unwrap_or(0.0))
            .unwrap_or(0.0);
        if mag >= LIT_THRESHOLD {
            by_kind.entry(act.kind).or_default().push(mag);
        }
    }
    by_kind
        .into_iter()
        .map(|(k, vals)| {
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            (k, mean)
        })
        .collect()
}

/// Find the largest set of NodeKinds shared by ≥min_members members,
/// where each shared set has ≥min_kinds elements. Returns the intersection
/// across the largest qualifying member-subset.
fn greatest_common_lit_kinds(
    per_member: &HashMap<&String, HashMap<NodeKind, f64>>,
    min_kinds: usize,
    min_members: usize,
) -> Vec<NodeKind> {
    if per_member.len() < min_members {
        return Vec::new();
    }
    // Count how often each kind appears across members
    let mut kind_freq: HashMap<NodeKind, usize> = HashMap::new();
    for kinds in per_member.values() {
        for k in kinds.keys() {
            *kind_freq.entry(*k).or_insert(0) += 1;
        }
    }
    // Take kinds appearing in ≥min_members members
    let common: Vec<NodeKind> = kind_freq
        .into_iter()
        .filter(|(_, c)| *c >= min_members)
        .map(|(k, _)| k)
        .collect();
    if common.len() >= min_kinds {
        common
    } else {
        Vec::new()
    }
}

/// Append cluster-sync events to NDJSON file. One JSON per line.
pub fn write_events(market: &str, events: &[ClusterSyncEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-emergence-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    for ev in events {
        let line = serde_json::to_string(ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

/// Project per-tick cluster sync events into the unified PerceptionGraph.
pub fn apply_to_perception_graph(
    events: &[ClusterSyncEvent],
    graph: &mut crate::perception::PerceptionGraph,
    tick: u64,
) {
    for ev in events {
        graph.emergence.upsert(
            ev.cluster_key.clone(),
            crate::perception::EmergenceSnapshot {
                sector: ev.cluster_key.clone(),
                total_members: ev.cluster_total_members as u32,
                sync_member_count: ev.sync_member_count as u32,
                sync_members: ev.sync_members.clone(),
                mean_activation_intent: ev
                    .mean_activation_per_kind
                    .get("Intent")
                    .copied()
                    .unwrap_or(0.0),
                mean_activation_pressure: ev
                    .mean_activation_per_kind
                    .get("Pressure")
                    .copied()
                    .unwrap_or(0.0),
                strongest_member: ev.strongest_member.clone().unwrap_or_default(),
                strongest_activation: ev.strongest_member_mean_activation,
                last_tick: tick,
            },
        );
    }
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn lit_symbol(reg: &mut SubKgRegistry, sym: &str, vals: &[(NodeId, Decimal)]) {
        let now = chrono::Utc::now();
        let kg = reg.upsert(sym, now);
        for (id, v) in vals {
            kg.set_node_value(id.clone(), *v, now);
        }
    }

    #[test]
    fn no_sync_when_only_one_member_lit() {
        let mut reg = SubKgRegistry::new();
        lit_symbol(&mut reg, "A.HK", &[(NodeId::PressureOrderBook, dec!(0.5))]);
        let mut clusters = HashMap::new();
        clusters.insert(
            "sec1".into(),
            vec!["A.HK".into(), "B.HK".into(), "C.HK".into()],
        );
        let evs = detect_cluster_sync("hk", &reg, &clusters, Utc::now());
        assert!(evs.is_empty());
    }

    #[test]
    fn sync_when_3_members_share_2_lit_kinds() {
        let mut reg = SubKgRegistry::new();
        for sym in ["A.HK", "B.HK", "C.HK"] {
            lit_symbol(
                &mut reg,
                sym,
                &[
                    (NodeId::PressureOrderBook, dec!(0.6)),
                    (NodeId::IntentAccumulation, dec!(0.5)),
                ],
            );
        }
        let mut clusters = HashMap::new();
        clusters.insert(
            "sec1".into(),
            vec!["A.HK".into(), "B.HK".into(), "C.HK".into()],
        );
        let evs = detect_cluster_sync("hk", &reg, &clusters, Utc::now());
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.sync_member_count, 3);
        assert_eq!(ev.lit_node_kinds.len(), 2);
    }

    #[test]
    fn no_sync_when_below_threshold() {
        let mut reg = SubKgRegistry::new();
        for sym in ["A.HK", "B.HK", "C.HK"] {
            lit_symbol(
                &mut reg,
                sym,
                &[
                    (NodeId::PressureOrderBook, dec!(0.1)), // below LIT_THRESHOLD
                    (NodeId::IntentAccumulation, dec!(0.2)),
                ],
            );
        }
        let mut clusters = HashMap::new();
        clusters.insert(
            "sec1".into(),
            vec!["A.HK".into(), "B.HK".into(), "C.HK".into()],
        );
        let evs = detect_cluster_sync("hk", &reg, &clusters, Utc::now());
        assert!(evs.is_empty());
    }

    #[test]
    fn strongest_member_picks_highest_mean() {
        let mut reg = SubKgRegistry::new();
        lit_symbol(
            &mut reg,
            "A.HK",
            &[
                (NodeId::PressureOrderBook, dec!(0.4)),
                (NodeId::IntentAccumulation, dec!(0.4)),
            ],
        );
        lit_symbol(
            &mut reg,
            "B.HK",
            &[
                (NodeId::PressureOrderBook, dec!(0.9)),
                (NodeId::IntentAccumulation, dec!(0.8)),
            ],
        );
        lit_symbol(
            &mut reg,
            "C.HK",
            &[
                (NodeId::PressureOrderBook, dec!(0.5)),
                (NodeId::IntentAccumulation, dec!(0.6)),
            ],
        );
        let mut clusters = HashMap::new();
        clusters.insert(
            "sec1".into(),
            vec!["A.HK".into(), "B.HK".into(), "C.HK".into()],
        );
        let evs = detect_cluster_sync("hk", &reg, &clusters, Utc::now());
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].strongest_member.as_deref(), Some("B.HK"));
    }

    #[test]
    fn cluster_below_min_members_skipped() {
        let mut reg = SubKgRegistry::new();
        lit_symbol(&mut reg, "A.HK", &[(NodeId::PressureOrderBook, dec!(0.6))]);
        lit_symbol(&mut reg, "B.HK", &[(NodeId::PressureOrderBook, dec!(0.6))]);
        let mut clusters = HashMap::new();
        clusters.insert("sec1".into(), vec!["A.HK".into(), "B.HK".into()]);
        let evs = detect_cluster_sync("hk", &reg, &clusters, Utc::now());
        assert!(evs.is_empty());
    }
}
