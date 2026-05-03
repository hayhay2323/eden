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

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::ontology::objects::{Market, Symbol};
use crate::ontology::{IntentDirection, IntentKind, IntentState};

/// Per-symbol KL-surprise reading: how unusual the channel-level belief
/// distribution shift is compared to recent baseline.
///
/// Current magnitude measures "surprise" in bits (KL divergence);
/// direction maps to {bullish, bearish, neutral} surprise based on
/// which bin the probability mass shifted into.
#[derive(Debug, Clone, PartialEq)]
pub struct KlSurpriseSnapshot {
    pub magnitude: Decimal,
    pub direction: Decimal,
    pub observed: f64,
    pub expected: f64,
    pub last_tick: u64,
}

/// KL-surprise sub-graph keyed by symbol.
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
        self.by_symbol.get(symbol).cloned()
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

/// Per-(sector, kind) belief kinetics reading: velocity and
/// acceleration of the activation field.
///
/// Mirrors the `sector_kinematics` NDJSON stream but kept in-memory
/// for fast multi-modal queries.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorKinematicsSnapshot {
    pub level_now: f64,
    pub velocity: f64,
    pub acceleration: f64,
    pub classification: String,
    pub last_tick: u64,
}

/// Belief kinetics sub-graph keyed by (sector_id, node_kind).
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

    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &SectorKinematicsSnapshot)> {
        self.by_sector_kind.iter()
    }
}

/// Per-(sector, kind) cross-sector contrast reading: how this sector's
/// activation compares to the mean across all OTHER sectors. Center −
/// surround mirrors the biological vision primitive that
/// `cross_sector_contrast` already computes per-tick.
///
/// All values are raw f64 because that's what the detector emits.
/// `last_tick` is the apply-call tick (events are derived from this
/// tick's `SectorSubKgRegistry`, so observation == apply).
#[derive(Debug, Clone, PartialEq)]
pub struct SectorContrastSnapshot {
    pub center_activation: f64,
    pub surround_mean: f64,
    pub contrast: f64,
    pub surround_count: usize,
    pub last_tick: u64,
}

/// Cross-sector contrast sub-graph keyed by (sector_id, node_kind).
/// Holds the latest contrast reading per (sector, kind) — Y can ask
/// "is energy a standout this tick?" without watching the event
/// stream.
#[derive(Debug, Clone, Default)]
pub struct SectorContrastSubGraph {
    by_sector_kind: HashMap<(String, String), SectorContrastSnapshot>,
}

impl SectorContrastSubGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(
        &mut self,
        sector_id: String,
        node_kind: String,
        snapshot: SectorContrastSnapshot,
    ) {
        self.by_sector_kind.insert((sector_id, node_kind), snapshot);
    }

    pub fn get(&self, sector_id: &str, node_kind: &str) -> Option<SectorContrastSnapshot> {
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

    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &SectorContrastSnapshot)> {
        self.by_sector_kind.iter()
    }
}

/// Snapshot of market-level world intent reflection.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldIntentSnapshot {
    pub intent_id: String,
    pub kind: IntentKind,
    pub direction: IntentDirection,
    pub state: IntentState,
    pub confidence: Decimal,
    pub urgency: Decimal,
    pub persistence: Decimal,
    pub conflict_score: Decimal,
    pub strength: Decimal,
    pub rationale: String,
    pub top_expectation: Option<String>,
    pub top_falsifier: Option<String>,
    pub expectation_count: usize,
    pub top_violation: Option<String>,
    pub violation_count: usize,
    pub reflection_observations: usize,
    pub reflection_reliability: Option<Decimal>,
    pub reflection_violation_rate: Option<Decimal>,
    pub reflection_calibration_gap: Option<Decimal>,
    pub latest_reflection: Option<String>,
    pub last_tick: u64,
}

/// Latest market-level world intent keyed by market. Runtime contexts
/// are currently per-market, but the key keeps the graph shape stable
/// for cross-market readers.
#[derive(Debug, Clone, Default)]
pub struct WorldIntentSubGraph {
    by_market: HashMap<Market, WorldIntentSnapshot>,
}

impl WorldIntentSubGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&mut self, market: Market, snapshot: WorldIntentSnapshot) {
        self.by_market.insert(market, snapshot);
    }

    pub fn get(&self, market: Market) -> Option<WorldIntentSnapshot> {
        self.by_market.get(&market).cloned()
    }

    pub fn len(&self) -> usize {
        self.by_market.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_market.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Market, &WorldIntentSnapshot)> {
        self.by_market.iter()
    }
}

/// Snapshot of an emergent cluster event.
#[derive(Debug, Clone, PartialEq)]
pub struct EmergenceSnapshot {
    pub sector: String,
    pub total_members: u32,
    pub sync_member_count: u32,
    pub sync_members: Vec<String>,
    pub mean_activation_intent: f64,
    pub mean_activation_pressure: f64,
    pub strongest_member: String,
    pub strongest_activation: f64,
    pub last_tick: u64,
}

/// Emergence sub-graph keyed by sector.
#[derive(Debug, Clone, Default)]
pub struct EmergenceSubGraph {
    by_sector: HashMap<String, EmergenceSnapshot>,
}

impl EmergenceSubGraph {
    pub fn upsert(&mut self, sector: String, snapshot: EmergenceSnapshot) {
        self.by_sector.insert(sector, snapshot);
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &EmergenceSnapshot)> {
        self.by_sector.iter()
    }
}

/// Snapshot of a causal lead-lag relationship.
#[derive(Debug, Clone, PartialEq)]
pub struct LeadLagSnapshot {
    pub leader: String,
    pub follower: String,
    pub lag_ticks: i32,
    pub correlation: f64,
    pub n_samples: usize,
    pub direction: String,
    pub last_tick: u64,
}

/// Lead-lag sub-graph keyed by (leader, follower, lag).
#[derive(Debug, Clone, Default)]
pub struct LeadLagSubGraph {
    by_edge: HashMap<(String, String, i32), LeadLagSnapshot>,
}

impl LeadLagSubGraph {
    pub fn upsert(&mut self, key: (String, String, i32), snapshot: LeadLagSnapshot) {
        self.by_edge.insert(key, snapshot);
    }
    pub fn iter(&self) -> impl Iterator<Item = (&(String, String, i32), &LeadLagSnapshot)> {
        self.by_edge.iter()
    }
}

/// Snapshot of a symbol's contrast against its master-KG neighborhood.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolContrastSnapshot {
    pub symbol: Symbol,
    pub sector_id: Option<String>,
    pub node_kind: String,
    pub center_activation: f64,
    pub surround_mean: f64,
    pub contrast: f64,
    pub last_tick: u64,
}

/// Symbol contrast sub-graph keyed by (symbol, node_kind).
#[derive(Debug, Clone, Default)]
pub struct SymbolContrastSubGraph {
    by_symbol_kind: HashMap<(Symbol, String), SymbolContrastSnapshot>,
}

impl SymbolContrastSubGraph {
    pub fn upsert(&mut self, key: (Symbol, String), snapshot: SymbolContrastSnapshot) {
        self.by_symbol_kind.insert(key, snapshot);
    }
    pub fn iter(&self) -> impl Iterator<Item = (&(Symbol, String), &SymbolContrastSnapshot)> {
        self.by_symbol_kind.iter()
    }
}

/// Snapshot of sensory 'Energy Flux' — how intensely information is
/// hitting a symbol and whether independent channels are aligned.
/// Realizes the 'Y' (Origin) archetype: inferring truth from the
/// power activity of the info-stream.
#[derive(Debug, Clone, PartialEq)]
pub struct SensoryFluxSnapshot {
    /// Total power: sum of absolute magnitudes across all 6 channels.
    pub total_flux: f64,
    /// Phase coherence: how many channels point in the same direction.
    /// 1.0 = perfect alignment; 0.0 = complete chaos.
    pub coherence: f64,
    /// Which channels contributed to the current vortex.
    pub active_channels: Vec<String>,
    pub last_tick: u64,
}

/// Sensory flux sub-graph keyed by symbol.
#[derive(Debug, Clone, Default)]
pub struct SensoryFluxSubGraph {
    by_symbol: HashMap<Symbol, SensoryFluxSnapshot>,
}

impl SensoryFluxSubGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn upsert(&mut self, symbol: Symbol, snapshot: SensoryFluxSnapshot) {
        self.by_symbol.insert(symbol, snapshot);
    }
    pub fn get(&self, symbol: &Symbol) -> Option<SensoryFluxSnapshot> {
        self.by_symbol.get(symbol).cloned()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&Symbol, &SensoryFluxSnapshot)> {
        self.by_symbol.iter()
    }
    pub fn decay(&mut self, factor: f64) {
        for snap in self.by_symbol.values_mut() {
            snap.total_flux *= factor;
            snap.coherence *= factor;
        }
        self.by_symbol.retain(|_, s| s.total_flux > 0.05);
    }
}

/// Snapshot of aggregated energy for an ontological theme or sector.
#[derive(Debug, Clone, PartialEq)]
pub struct ThematicFluxSnapshot {
    pub theme_id: String,
    pub theme_name: String,
    pub total_energy: f64,
    pub collective_coherence: f64,
    pub active_member_count: u32,
    pub leader_symbol: Option<String>,
    pub last_tick: u64,
}

/// Thematic flux sub-graph keyed by theme/sector ID.
#[derive(Debug, Clone, Default)]
pub struct ThematicFluxSubGraph {
    by_theme: HashMap<String, ThematicFluxSnapshot>,
}

impl ThematicFluxSubGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn upsert(&mut self, theme_id: String, snapshot: ThematicFluxSnapshot) {
        self.by_theme.insert(theme_id, snapshot);
    }
    pub fn get(&self, theme_id: &str) -> Option<ThematicFluxSnapshot> {
        self.by_theme.get(theme_id).cloned()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ThematicFluxSnapshot)> {
        self.by_theme.iter()
    }
    pub fn decay(&mut self, factor: f64) {
        for snap in self.by_theme.values_mut() {
            snap.total_energy *= factor;
            snap.collective_coherence *= factor;
        }
        self.by_theme.retain(|_, s| s.total_energy > 0.1);
    }
}

/// Snapshot of a synthetic (emergent) sector.
#[derive(Debug, Clone, PartialEq)]
pub struct SyntheticSectorSnapshot {
    pub synth_id: String,
    pub members: Vec<String>,
    pub total_energy: f64,
    pub collective_coherence: f64,
    pub last_tick: u64,
}

/// Synthetic sector sub-graph keyed by synth_id.
#[derive(Debug, Clone, Default)]
pub struct SyntheticSectorSubGraph {
    by_id: HashMap<String, SyntheticSectorSnapshot>,
}

impl SyntheticSectorSubGraph {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn upsert(&mut self, id: String, snapshot: SyntheticSectorSnapshot) {
        self.by_id.insert(id, snapshot);
    }
    pub fn clear(&mut self) {
        self.by_id.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SyntheticSectorSnapshot)> {
        self.by_id.iter()
    }
    pub fn decay(&mut self, factor: f64) {
        for snap in self.by_id.values_mut() {
            snap.total_energy *= factor;
            snap.collective_coherence *= factor;
        }
        self.by_id.retain(|_, s| s.total_energy > 0.1);
    }
}

/// Snapshot of the dynamic trust (gain) allocated to a sensory channel.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SensoryGainSnapshot {
    pub channel_name: String,
    pub current_gain: f64,
    pub recent_accuracy: f64,
    pub last_calibrated: u64,
}

/// Sensory gain ledger: tracks the dynamic weights of the 6+1 channels.
///
/// **Closed-loop learning**: gains are updated by `active_probe.rs:310-326`
/// after each probe outcome — `(mean_accuracy - 0.5) * 0.1` adjustment,
/// clamped to `[0.1, 2.0]`. Read by `prior_from_pressure_channels` in
/// `loopy_bp.rs:382` so BP priors reflect learned channel trust.
///
/// **Persistence**: snapshots survive session restarts via
/// `sensory_gain_ledger_path(slug)` (a small JSON file, atomic
/// overwrite). `PerceptionGraph::persistent(slug)` loads the file on
/// startup; `save_sensory_gain_to_path` is called by
/// `active_probe::evaluate_due` after every gain update so the next
/// session resumes from learned weights instead of the First Principle
/// seed defaults below. Sync-contract doc:
/// `docs/architecture/perception-graph-sync-contract.md`.
#[derive(Debug, Clone, Default)]
pub struct SensoryGainLedger {
    by_channel: HashMap<String, SensoryGainSnapshot>,
}

impl SensoryGainLedger {
    pub fn new() -> Self {
        let mut s = Self::default();
        // Seed with First Principle defaults (overwritten by learning;
        // see `active_probe.rs:310` for the closed-loop update path).
        let defaults = [
            ("OrderBook", 0.3),
            ("Structure", 0.2),
            ("CapitalFlow", 1.0),
            ("Momentum", 0.5),
            ("Institutional", 0.3),
            ("Option", 0.4),
            ("Memory", 0.6),
        ];
        for (name, gain) in defaults {
            s.by_channel.insert(
                name.to_string(),
                SensoryGainSnapshot {
                    channel_name: name.to_string(),
                    current_gain: gain,
                    recent_accuracy: 0.5, // Informed but neutral
                    last_calibrated: 0,
                },
            );
        }
        s
    }
    pub fn upsert(&mut self, name: &str, snapshot: SensoryGainSnapshot) {
        self.by_channel.insert(name.to_string(), snapshot);
    }
    pub fn get_gain(&self, name: &str) -> f64 {
        self.by_channel
            .get(name)
            .map(|s| s.current_gain)
            .unwrap_or(0.1)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SensoryGainSnapshot)> {
        self.by_channel.iter()
    }

    /// Snapshot the ledger to a deterministic-order `Vec` for
    /// serialization. Sorted by `channel_name` so successive saves of
    /// the same ledger produce identical JSON on disk (useful for
    /// `git diff` and content-addressed comparisons).
    pub fn to_records(&self) -> Vec<SensoryGainSnapshot> {
        let mut out: Vec<_> = self.by_channel.values().cloned().collect();
        out.sort_by(|a, b| a.channel_name.cmp(&b.channel_name));
        out
    }

    /// Rebuild a ledger from previously-serialized records.
    pub fn from_records(records: Vec<SensoryGainSnapshot>) -> Self {
        let mut s = Self::default();
        for r in records {
            s.by_channel.insert(r.channel_name.clone(), r);
        }
        s
    }
}

/// Path of the sensory-gain persistence file for a given market slug
/// (`"hk"` / `"us"`). Mirrors the `world_reflection_ledger_path`
/// pattern but is a single JSON snapshot rather than an append-only
/// NDJSON tail — gains are state, not events.
pub fn sensory_gain_ledger_path(market_slug: &str) -> String {
    format!(".run/sensory-gain-{}.json", market_slug)
}

/// Atomically overwrite the sensory-gain JSON for `path`. Writes the
/// ledger as a `Vec<SensoryGainSnapshot>` so the file can be inspected
/// or hand-edited.
///
/// Returns `Ok(())` even if the parent dir didn't exist (created on
/// demand). Errors only propagate when serialization or the actual
/// write fails.
pub fn save_sensory_gain_to_path(
    ledger: &SensoryGainLedger,
    path: &str,
) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let records = ledger.to_records();
    let payload = serde_json::to_string(&records)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, payload)
}

/// Load a sensory-gain ledger from disk; if the file is missing,
/// unreadable, or malformed, fall back to `SensoryGainLedger::new()`
/// (the First Principle seed defaults). Logging the load failure is
/// the caller's job — this function is fail-soft so a corrupt or
/// missing file never prevents runtime startup.
pub fn load_sensory_gain_from_path(path: &str) -> SensoryGainLedger {
    let Ok(content) = std::fs::read_to_string(path) else {
        return SensoryGainLedger::new();
    };
    match serde_json::from_str::<Vec<SensoryGainSnapshot>>(&content) {
        Ok(records) if !records.is_empty() => SensoryGainLedger::from_records(records),
        _ => SensoryGainLedger::new(),
    }
}

/// Eden's unified perception graph. Composed of typed sub-graphs, one
/// per perceiver. Add new sub-graphs as detectors migrate off NDJSON.
#[derive(Debug, Clone, Default)]
pub struct PerceptionGraph {
    pub kl_surprise: KlSurpriseSubGraph,
    pub sector_kinematics: SectorKinematicsSubGraph,
    pub sector_contrast: SectorContrastSubGraph,
    pub world_intent: WorldIntentSubGraph,
    pub emergence: EmergenceSubGraph,
    pub lead_lag: LeadLagSubGraph,
    pub symbol_contrast: SymbolContrastSubGraph,
    pub sensory_flux: SensoryFluxSubGraph,
    pub thematic_flux: ThematicFluxSubGraph,
    pub synthetic_sectors: SyntheticSectorSubGraph,
    pub sensory_gain: SensoryGainLedger,
}

impl PerceptionGraph {
    pub fn new() -> Self {
        Self {
            sensory_gain: SensoryGainLedger::new(),
            ..Default::default()
        }
    }

    /// Build a `PerceptionGraph` whose `sensory_gain` ledger is loaded
    /// from `sensory_gain_ledger_path(market_slug)`. All other
    /// sub-graphs start empty as in `new()` — only the ledger
    /// persists across sessions.
    ///
    /// `market_slug` should be `"hk"` or `"us"` (matches what
    /// `MarketId::slug()` returns).
    pub fn persistent(market_slug: &str) -> Self {
        let path = sensory_gain_ledger_path(market_slug);
        let mut graph = Self::new();
        graph.sensory_gain = load_sensory_gain_from_path(&path);
        graph
    }

    /// Apply energy decay to the entire graph. Simulates 'Inertia' in the
    /// sensory field. Strong vortices take time to dissipate.
    pub fn decay_energy(&mut self) {
        let decay_factor = 0.90; // 10% energy loss per tick
        self.sensory_flux.decay(decay_factor);
        self.thematic_flux.decay(decay_factor);
        self.synthetic_sectors.decay(decay_factor);
    }

    /// Project the unified perception graph into a serializable EdenPerception report.
    /// This is the "graph-native" version of read_perception_streams.
    pub fn to_report(
        &self,
        market: Market,
        tick: u64,
        timestamp: String,
        cfg: &crate::agent::PerceptionFilterConfig,
    ) -> crate::agent::EdenPerception {
        use crate::agent::{
            BeliefKinetic, ChannelGain, EmergentCluster, LeadLagEdge, RegimePerception,
            SensoryFlux, SymbolContrast, SurpriseAlert, ThematicVortex,
        };

        let mut report = crate::agent::EdenPerception {
            schema_version: 1,
            market: match market {
                Market::Hk => crate::live_snapshot::LiveMarket::Hk,
                Market::Us => crate::live_snapshot::LiveMarket::Us,
            },
            tick,
            timestamp,
            emergent_clusters: Vec::new(),
            sector_leaders: Vec::new(),
            causal_chains: Vec::new(),
            anomaly_alerts: Vec::new(),
            regime: None,
            belief_kinetics: Vec::new(),
            signature_replays: Vec::new(),
            pre_market_movers: Vec::new(),
            catalysts: Vec::new(),
            sensory_vortices: Vec::new(),
            thematic_vortices: Vec::new(),
            sensory_gain: self
                .sensory_gain
                .to_records()
                .into_iter()
                .map(|snap| ChannelGain {
                    channel_name: snap.channel_name,
                    current_gain: snap.current_gain,
                    recent_accuracy: snap.recent_accuracy,
                    last_calibrated: snap.last_calibrated,
                })
                .collect(),
        };

        // -2. Synthetic Sectors (Fluid Narratives)
        for (_, snap) in self.synthetic_sectors.iter() {
            // Lower threshold for SYNTH: any significant group energy (> 0.5)
            if snap.total_energy > 0.5 {
                report.thematic_vortices.push(ThematicVortex {
                    theme_id: snap.synth_id.clone(),
                    theme_name: format!("Emergent Narrative {}", snap.synth_id),
                    total_energy: snap.total_energy,
                    collective_coherence: snap.collective_coherence,
                    active_member_count: snap.members.len() as u32,
                    leader_symbol: snap.members.first().cloned(),
                });
            }
        }

        // -1. Thematic Flux (Semantic Energy Centers)
        for (_, snap) in self.thematic_flux.iter() {
            // Lower threshold: 0.5 energy shows the 'pre-vortex' heating.
            if snap.total_energy > 0.5 {
                report.thematic_vortices.push(ThematicVortex {
                    theme_id: snap.theme_id.clone(),
                    theme_name: snap.theme_name.clone(),
                    total_energy: snap.total_energy,
                    collective_coherence: snap.collective_coherence,
                    active_member_count: snap.active_member_count,
                    leader_symbol: snap.leader_symbol.clone(),
                });
            }
        }
        report.thematic_vortices.sort_by(|a, b| {
            b.total_energy
                .partial_cmp(&a.total_energy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 0. Sensory Flux (Energy Vortices)
        for (symbol, snap) in self.sensory_flux.iter() {
            // Relax thresholds to show the full energy gradient.
            if snap.total_flux > 0.1 {
                report.sensory_vortices.push(SensoryFlux {
                    symbol: symbol.0.clone(),
                    flux_magnitude: snap.total_flux,
                    coherence_ratio: snap.coherence,
                    active_channels: snap.active_channels.clone(),
                });
            }
        }
        report.sensory_vortices.sort_by(|a, b| {
            b.flux_magnitude
                .partial_cmp(&a.flux_magnitude)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // 1. Emergence
        for (_, snap) in self.emergence.iter() {
            let total = snap.total_members.max(1);
            let sync_pct = snap.sync_member_count as f64 / total as f64;
            if sync_pct >= cfg.min_cluster_sync_pct {
                report.emergent_clusters.push(EmergentCluster {
                    sector: snap.sector.clone(),
                    total_members: snap.total_members,
                    sync_member_count: snap.sync_member_count,
                    sync_ratio: format!("{}/{}", snap.sync_member_count, snap.total_members),
                    sync_pct,
                    strongest_member: snap.strongest_member.clone(),
                    strongest_activation: snap.strongest_activation,
                    mean_activation_intent: snap.mean_activation_intent,
                    mean_activation_pressure: snap.mean_activation_pressure,
                    members: snap.sync_members.clone(),
                });
            }
        }

        // 2. Symbol Contrast (Sector Leaders / Standouts)
        for (key, snap) in self.symbol_contrast.iter() {
            // 1.0 contrast is already noticeable in the field.
            if snap.contrast >= 1.0 {
                report.sector_leaders.push(SymbolContrast {
                    symbol: key.0 .0.clone(),
                    sector: snap.sector_id.clone(),
                    center_activation: snap.center_activation,
                    sector_mean: snap.surround_mean,
                    vs_sector_contrast: snap.contrast,
                    node_kind: snap.node_kind.clone(),
                    persistence_ticks: None,
                });
            }
        }
        report.sector_leaders.sort_by(|a, b| {
            b.vs_sector_contrast
                .partial_cmp(&a.vs_sector_contrast)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        report.sector_leaders.truncate(cfg.max_leaders);

        // 3. Lead-Lag (Causal Chains)
        for (_, snap) in self.lead_lag.iter() {
            // Any meaningful correlation (> 0.3) is part of the field topology.
            if snap.correlation.abs() >= 0.3
                && snap.n_samples >= 8
            {
                report.causal_chains.push(LeadLagEdge {
                    leader: snap.leader.clone(),
                    follower: snap.follower.clone(),
                    lag_ticks: snap.lag_ticks,
                    correlation: snap.correlation,
                    n_samples: snap.n_samples,
                    direction: snap.direction.clone(),
                });
            }
        }
        report.causal_chains.sort_by(|a, b| {
            b.correlation
                .abs()
                .partial_cmp(&a.correlation.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        report.causal_chains.truncate(cfg.max_chains);

        // 4. Surprise (Anomaly Alerts)
        for (symbol, snap) in self.kl_surprise.iter() {
            let mag_f64 = snap.magnitude.to_f64().unwrap_or(0.0);
            // 1.0 surprise is the statistical 'Gaussian' first principle threshold.
            if mag_f64 >= 1.0 {
                report.anomaly_alerts.push(SurpriseAlert {
                    symbol: symbol.0.clone(),
                    channel: "KLSurprise".to_string(),
                    observed: snap.observed,
                    expected: snap.expected,
                    squared_error: mag_f64 * mag_f64,
                    total_surprise: mag_f64,
                    floor: 1.0,
                    deviation_kind: if snap.direction > Decimal::ZERO {
                        "above_expected".to_string()
                    } else {
                        "below_expected".to_string()
                    },
                });
            }
        }
        report.anomaly_alerts.sort_by(|a, b| {
            b.total_surprise
                .partial_cmp(&a.total_surprise)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        report.anomaly_alerts.truncate(cfg.max_anomalies);

        // 5. Regime
        if let Some(snap) = self.world_intent.get(market) {
            report.regime = Some(RegimePerception {
                bucket: format!("{:?}|{:?}|{:?}", snap.kind, snap.direction, snap.state),
                historical_visits: snap.reflection_observations as u32,
                last_seen_tick: Some(snap.last_tick),
                forward_outcomes: Vec::new(), // Skeleton
            });
        }

        // 6. Belief Kinetics (from sector_kinematics sub-graph)
        for (key, snap) in self.sector_kinematics.iter() {
            if snap.velocity.abs() >= cfg.min_kinetic_velocity {
                report.belief_kinetics.push(BeliefKinetic {
                    symbol: format!("{}:{}", key.0, key.1),
                    belief_now: snap.level_now,
                    velocity: snap.velocity,
                    acceleration: snap.acceleration,
                    streak_ticks: 1, // Snapshot doesn't carry history
                });
            }
        }
        report.belief_kinetics.sort_by(|a, b| {
            b.velocity
                .abs()
                .partial_cmp(&a.velocity.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        report.belief_kinetics.truncate(cfg.max_kinetics);

        report
    }

    /// Read facade for a single symbol's perception across all sub-
    /// graphs. Returns `None` for any modality the symbol has no
    /// reading in yet.
    pub fn node(&self, symbol: &Symbol) -> NodeView {
        NodeView {
            symbol: symbol.clone(),
            kl_surprise: self.kl_surprise.get(symbol),
            sensory_flux: self.sensory_flux.get(symbol),
        }
    }

    /// Market-level read facade. This is the Y-facing counterpart to
    /// `node()`: world intent lives on the graph, not in wake strings.
    pub fn world(&self, market: Market) -> WorldView {
        WorldView {
            market,
            world_intent: self.world_intent.get(market),
        }
    }
}

/// Per-symbol read view across every perceiver. The shape Y queries.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    pub symbol: Symbol,
    pub kl_surprise: Option<KlSurpriseSnapshot>,
    pub sensory_flux: Option<SensoryFluxSnapshot>,
}

/// Market-level read view.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldView {
    pub market: Market,
    pub world_intent: Option<WorldIntentSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn kl_surprise_subgraph_upsert_and_get() {
        let mut graph = PerceptionGraph::new();
        let s = Symbol("AAPL.US".to_string());
        graph.kl_surprise.upsert(
            s.clone(),
            KlSurpriseSnapshot {
                magnitude: dec!(0.9),
                direction: dec!(1),
                observed: 0.5,
                expected: 0.2,
                last_tick: 2,
            },
        );

        let snap = graph.kl_surprise.get(&s).expect("reading present");
        assert_eq!(snap.last_tick, 2);
        assert_eq!(snap.magnitude, dec!(0.9));
    }

    #[test]
    fn node_view_assembles_modalities() {
        let mut graph = PerceptionGraph::new();
        let s = Symbol("0700.HK".to_string());
        graph.kl_surprise.upsert(
            s.clone(),
            KlSurpriseSnapshot {
                magnitude: dec!(0.1),
                direction: dec!(-1),
                observed: 0.1,
                expected: 0.3,
                last_tick: 42,
            },
        );

        let view = graph.node(&s);
        assert_eq!(view.symbol, s);
        assert!(view.kl_surprise.is_some());
    }

    #[test]
    fn world_view_assembles_market_modalities() {
        let mut graph = PerceptionGraph::new();
        graph.world_intent.upsert(
            Market::Hk,
            WorldIntentSnapshot {
                intent_id: "test".to_string(),
                kind: IntentKind::Accumulation,
                direction: IntentDirection::Buy,
                state: IntentState::Active,
                confidence: dec!(0.8),
                urgency: dec!(0.5),
                persistence: dec!(0.9),
                conflict_score: dec!(0.1),
                strength: dec!(0.7),
                rationale: "test".to_string(),
                top_expectation: None,
                top_falsifier: None,
                expectation_count: 0,
                top_violation: None,
                violation_count: 0,
                reflection_observations: 10,
                reflection_reliability: None,
                reflection_violation_rate: None,
                reflection_calibration_gap: None,
                latest_reflection: None,
                last_tick: 100,
            },
        );

        let view = graph.world(Market::Hk);
        assert!(view.world_intent.is_some());
    }

    #[test]
    fn sensory_gain_records_round_trip_preserves_state() {
        let mut ledger = SensoryGainLedger::new();
        ledger.upsert(
            "CapitalFlow",
            SensoryGainSnapshot {
                channel_name: "CapitalFlow".to_string(),
                current_gain: 1.42,
                recent_accuracy: 0.78,
                last_calibrated: 17,
            },
        );
        ledger.upsert(
            "OrderBook",
            SensoryGainSnapshot {
                channel_name: "OrderBook".to_string(),
                current_gain: 0.13,
                recent_accuracy: 0.41,
                last_calibrated: 23,
            },
        );

        let records = ledger.to_records();
        // Sorted by channel_name for deterministic on-disk format.
        assert_eq!(records[0].channel_name, "CapitalFlow");
        let cf_idx = records
            .iter()
            .position(|r| r.channel_name == "CapitalFlow")
            .unwrap();
        let ob_idx = records
            .iter()
            .position(|r| r.channel_name == "OrderBook")
            .unwrap();
        assert!(cf_idx < ob_idx, "records sorted alphabetically");

        let restored = SensoryGainLedger::from_records(records);
        assert!((restored.get_gain("CapitalFlow") - 1.42).abs() < 1e-9);
        assert!((restored.get_gain("OrderBook") - 0.13).abs() < 1e-9);
        // A channel never inserted reads the unknown-default 0.1.
        assert!((restored.get_gain("NotInLedger") - 0.1).abs() < 1e-9);
    }

    #[test]
    fn sensory_gain_persistence_round_trip_via_disk() {
        let dir = std::env::temp_dir().join(format!(
            "eden-sensory-gain-test-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sensory-gain.json");
        let path_str = path.to_string_lossy().to_string();

        let mut original = SensoryGainLedger::new();
        original.upsert(
            "Memory",
            SensoryGainSnapshot {
                channel_name: "Memory".to_string(),
                current_gain: 1.85,
                recent_accuracy: 0.92,
                last_calibrated: 999,
            },
        );

        save_sensory_gain_to_path(&original, &path_str).expect("save");
        let loaded = load_sensory_gain_from_path(&path_str);
        assert!((loaded.get_gain("Memory") - 1.85).abs() < 1e-9);

        // Cleanup; failure to clean is fine.
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn load_sensory_gain_from_missing_path_returns_seed_defaults() {
        let path = "/nonexistent/path/that/does/not/exist.json";
        let ledger = load_sensory_gain_from_path(path);
        // Seed defaults populate the seven canonical channels.
        assert!((ledger.get_gain("CapitalFlow") - 1.0).abs() < 1e-9);
        assert!((ledger.get_gain("OrderBook") - 0.3).abs() < 1e-9);
    }
}
