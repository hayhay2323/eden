//! PerceptionGraph — eden's unified internal world model.
//!
//! Replaces sharded detector → NDJSON streams with a single typed graph
//! that perceivers mutate and L4 (Y interface) reads from. Composed of
//! per-perceiver sub-graphs (KL surprise, future detectors) so each
//! perceiver owns its own slice without a god-struct.
//!
//! Per the eden thesis: a sensory organ doesn't separate taste, sight,
//! and hearing into distinct streams. Perception is unified at the
//! graph level; modality-specific projections happen at the read
//! boundary (`NodeView`).

use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

/// Per-symbol KL-surprise reading: how unusual the channel-level belief
/// shift was, and which way the dominant channel moved. Magnitude is
/// `tanh(|max_z| / 2)` ∈ [0, 1]; direction is the sign of the dominant
/// channel's mean shift, ∈ {-1, 0, +1}.
///
/// Mirrors the tuple `KlSurpriseTracker::surprise_summary` returned;
/// converted from raw HashMap into a typed snapshot so consumers can
/// move from "function argument" to "graph node" without changing
/// semantics.
///
/// `last_tick` is exposed for consumer-side staleness checks: the
/// graph carries the *latest* reading per symbol and never evicts on
/// its own. If a symbol drops out of the universe its snapshot
/// remains until the perceiver overwrites it. Y / L4 readers that
/// care about freshness must compare `last_tick` against the current
/// tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KlSurpriseSnapshot {
    pub magnitude: Decimal,
    pub direction: Decimal,
    pub last_tick: u64,
}

/// KL-surprise sub-graph keyed by symbol. One slot per symbol; later
/// observations overwrite earlier ones (the tracker's EWMA already
/// holds the historical baseline — the graph carries the *current*
/// reading).
#[derive(Debug, Clone, Default)]
pub struct KlSurpriseSubGraph {
    by_symbol: HashMap<Symbol, KlSurpriseSnapshot>,
}

impl KlSurpriseSubGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&mut self, symbol: Symbol, snapshot: KlSurpriseSnapshot) {
        self.by_symbol.insert(symbol, snapshot);
    }

    pub fn get(&self, symbol: &Symbol) -> Option<KlSurpriseSnapshot> {
        self.by_symbol.get(symbol).copied()
    }

    pub fn len(&self) -> usize {
        self.by_symbol.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_symbol.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Symbol, &KlSurpriseSnapshot)> {
        self.by_symbol.iter()
    }
}

/// Per-(sector, kind) kinematic state: where the sector mean is now,
/// how fast it's moving, whether it's accelerating, and (optionally)
/// the latest turning-point classification. Values are raw f64 because
/// that's the form the kinematics detector produces; Y / L4 readers
/// should compare `last_tick` against the current tick to judge
/// freshness (the graph never evicts on its own).
#[derive(Debug, Clone, PartialEq)]
pub struct SectorKinematicsSnapshot {
    pub level_now: f64,
    pub velocity: f64,
    pub acceleration: f64,
    /// String label of the latest turning-point event, e.g.
    /// "TopForming" / "BottomForming" / "Accelerating" / "Decaying".
    /// `None` until the detector has classified at least once.
    pub classification: Option<String>,
    pub last_tick: u64,
}

/// Sector-kinematics sub-graph keyed by (sector_id, node_kind). Mirror
/// of the existing `pipeline::sector_kinematics` NDJSON output, but
/// held in-graph so Y can read "what's energy sector doing right now"
/// without watching an event stream.
#[derive(Debug, Clone, Default)]
pub struct SectorKinematicsSubGraph {
    by_sector_kind: HashMap<(String, String), SectorKinematicsSnapshot>,
}

impl SectorKinematicsSubGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(
        &mut self,
        sector_id: String,
        node_kind: String,
        snapshot: SectorKinematicsSnapshot,
    ) {
        self.by_sector_kind.insert((sector_id, node_kind), snapshot);
    }

    pub fn get(&self, sector_id: &str, node_kind: &str) -> Option<SectorKinematicsSnapshot> {
        self.by_sector_kind
            .get(&(sector_id.to_string(), node_kind.to_string()))
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.by_sector_kind.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_sector_kind.is_empty()
    }

    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (&(String, String), &SectorKinematicsSnapshot)> {
        self.by_sector_kind.iter()
    }
}

/// Eden's unified perception graph. Composed of typed sub-graphs, one
/// per perceiver. Add new sub-graphs as detectors migrate off NDJSON.
#[derive(Debug, Clone, Default)]
pub struct PerceptionGraph {
    pub kl_surprise: KlSurpriseSubGraph,
    pub sector_kinematics: SectorKinematicsSubGraph,
}

impl PerceptionGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read facade for a single symbol's perception across all sub-
    /// graphs. Returns `None` for any modality the symbol has no
    /// reading in yet.
    pub fn node(&self, symbol: &Symbol) -> NodeView {
        NodeView {
            symbol: symbol.clone(),
            kl_surprise: self.kl_surprise.get(symbol),
        }
    }
}

/// Per-symbol read view across every perceiver. The shape Y queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub symbol: Symbol,
    pub kl_surprise: Option<KlSurpriseSnapshot>,
}

impl NodeView {
    pub fn has_kl_surprise(&self) -> bool {
        self.kl_surprise.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.to_string())
    }

    #[test]
    fn fresh_graph_is_empty() {
        let graph = PerceptionGraph::new();
        assert!(graph.kl_surprise.is_empty());
    }

    #[test]
    fn fresh_graph_node_view_has_no_readings() {
        let graph = PerceptionGraph::new();
        let view = graph.node(&sym("AAPL.US"));
        assert_eq!(view.symbol, sym("AAPL.US"));
        assert!(view.kl_surprise.is_none());
        assert!(!view.has_kl_surprise());
    }

    #[test]
    fn upsert_kl_surprise_then_read_via_node_view() {
        let mut graph = PerceptionGraph::new();
        graph.kl_surprise.upsert(
            sym("AAPL.US"),
            KlSurpriseSnapshot {
                magnitude: dec!(0.42),
                direction: dec!(1),
                last_tick: 7,
            },
        );

        let view = graph.node(&sym("AAPL.US"));
        let snap = view.kl_surprise.expect("expected reading after upsert");
        assert_eq!(snap.magnitude, dec!(0.42));
        assert_eq!(snap.direction, dec!(1));
        assert_eq!(snap.last_tick, 7);
        assert!(view.has_kl_surprise());
    }

    #[test]
    fn upsert_overwrites_previous_reading() {
        let mut graph = PerceptionGraph::new();
        let s = sym("MSFT.US");
        graph.kl_surprise.upsert(
            s.clone(),
            KlSurpriseSnapshot {
                magnitude: dec!(0.1),
                direction: dec!(-1),
                last_tick: 1,
            },
        );
        graph.kl_surprise.upsert(
            s.clone(),
            KlSurpriseSnapshot {
                magnitude: dec!(0.9),
                direction: dec!(1),
                last_tick: 2,
            },
        );

        let snap = graph.kl_surprise.get(&s).expect("reading present");
        assert_eq!(snap.last_tick, 2);
        assert_eq!(snap.magnitude, dec!(0.9));
        assert_eq!(snap.direction, dec!(1));
        assert_eq!(graph.kl_surprise.len(), 1);
    }

    #[test]
    fn fresh_graph_has_empty_sector_kinematics() {
        let graph = PerceptionGraph::new();
        assert!(graph.sector_kinematics.is_empty());
        assert_eq!(graph.sector_kinematics.len(), 0);
    }

    #[test]
    fn upsert_sector_kinematics_then_read() {
        let mut graph = PerceptionGraph::new();
        graph.sector_kinematics.upsert(
            "tech".into(),
            "Pressure".into(),
            SectorKinematicsSnapshot {
                level_now: 0.42,
                velocity: 0.05,
                acceleration: -0.01,
                classification: Some("TopForming".into()),
                last_tick: 5,
            },
        );

        let snap = graph
            .sector_kinematics
            .get("tech", "Pressure")
            .expect("reading present after upsert");
        assert_eq!(snap.level_now, 0.42);
        assert_eq!(snap.velocity, 0.05);
        assert_eq!(snap.acceleration, -0.01);
        assert_eq!(snap.classification.as_deref(), Some("TopForming"));
        assert_eq!(snap.last_tick, 5);
    }

    #[test]
    fn distinct_sectors_keep_separate_kinematic_readings() {
        let mut graph = PerceptionGraph::new();
        graph.sector_kinematics.upsert(
            "tech".into(),
            "Pressure".into(),
            SectorKinematicsSnapshot {
                level_now: 0.5,
                velocity: 0.0,
                acceleration: 0.0,
                classification: None,
                last_tick: 1,
            },
        );
        graph.sector_kinematics.upsert(
            "energy".into(),
            "Pressure".into(),
            SectorKinematicsSnapshot {
                level_now: -0.3,
                velocity: -0.1,
                acceleration: 0.0,
                classification: Some("Decaying".into()),
                last_tick: 1,
            },
        );

        assert_eq!(graph.sector_kinematics.len(), 2);
        assert_eq!(
            graph.sector_kinematics.get("tech", "Pressure").unwrap().level_now,
            0.5
        );
        assert_eq!(
            graph
                .sector_kinematics
                .get("energy", "Pressure")
                .unwrap()
                .classification
                .as_deref(),
            Some("Decaying")
        );
    }

    #[test]
    fn same_sector_different_kind_kept_separate() {
        let mut graph = PerceptionGraph::new();
        graph.sector_kinematics.upsert(
            "tech".into(),
            "Pressure".into(),
            SectorKinematicsSnapshot {
                level_now: 0.5,
                velocity: 0.0,
                acceleration: 0.0,
                classification: None,
                last_tick: 1,
            },
        );
        graph.sector_kinematics.upsert(
            "tech".into(),
            "Intent".into(),
            SectorKinematicsSnapshot {
                level_now: 0.9,
                velocity: 0.0,
                acceleration: 0.0,
                classification: None,
                last_tick: 1,
            },
        );

        assert_eq!(graph.sector_kinematics.len(), 2);
        assert_ne!(
            graph.sector_kinematics.get("tech", "Pressure").unwrap().level_now,
            graph.sector_kinematics.get("tech", "Intent").unwrap().level_now,
        );
    }

    #[test]
    fn distinct_symbols_keep_separate_readings() {
        let mut graph = PerceptionGraph::new();
        graph.kl_surprise.upsert(
            sym("AAPL.US"),
            KlSurpriseSnapshot {
                magnitude: dec!(0.5),
                direction: dec!(1),
                last_tick: 3,
            },
        );
        graph.kl_surprise.upsert(
            sym("0700.HK"),
            KlSurpriseSnapshot {
                magnitude: dec!(0.7),
                direction: dec!(-1),
                last_tick: 3,
            },
        );

        assert_eq!(graph.kl_surprise.len(), 2);
        let aapl = graph.node(&sym("AAPL.US")).kl_surprise.unwrap();
        let tcent = graph.node(&sym("0700.HK")).kl_surprise.unwrap();
        assert_eq!(aapl.direction, dec!(1));
        assert_eq!(tcent.direction, dec!(-1));
    }
}
