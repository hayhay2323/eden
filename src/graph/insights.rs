use std::collections::HashMap;
use rust_decimal::Decimal;

use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::ontology::store::ObjectStore;

use super::graph::BrainGraph;

#[path = "insights/institutional.rs"]
mod institutional;
#[path = "insights/market_metrics.rs"]
mod market_metrics;
#[path = "insights/relationships.rs"]
mod relationships;
#[path = "insights/display.rs"]
mod display;
use institutional::{
    collect_institution_stock_counts, compute_institution_exoduses,
    compute_institution_rotations, compute_shared_holders,
};
use market_metrics::{compute_pressures, compute_rotations, compute_stress_index};
use relationships::{compute_clusters, compute_conflicts};

// ── Output structs ──

/// Per-symbol profile of edge temporal characteristics.
#[derive(Debug, Clone)]
pub struct EdgeTemporalProfile {
    pub symbol: Symbol,
    pub mean_institution_edge_age: Decimal,
    pub mean_edge_stability: Decimal,
    pub new_edge_count: usize,
    pub oldest_edge_age: u64,
}

#[derive(Debug, Clone)]
pub struct StockPressure {
    pub symbol: Symbol,
    pub net_pressure: Decimal,
    pub institution_count: usize,
    pub buy_inst_count: usize,
    pub sell_inst_count: usize,
    pub pressure_delta: Decimal,
    pub pressure_duration: u64,
    pub accelerating: bool,
}

#[derive(Debug, Clone)]
pub struct RotationPair {
    pub from_sector: SectorId,
    pub to_sector: SectorId,
    pub spread: Decimal,
    pub spread_delta: Decimal,
    pub widening: bool,
}

#[derive(Debug, Clone)]
pub struct StockCluster {
    pub members: Vec<Symbol>,
    pub mean_similarity: Decimal,
    pub directional_alignment: Decimal,
    pub cross_sector: bool,
    pub stability: Decimal,
    pub age: u64,
}

#[derive(Debug, Clone)]
pub struct InstitutionalConflict {
    pub inst_a: InstitutionId,
    pub inst_b: InstitutionId,
    pub jaccard_overlap: Decimal,
    pub direction_a: Decimal,
    pub direction_b: Decimal,
    pub shared_stocks: Vec<Symbol>,
    pub conflict_age: u64,
    pub intensity_delta: Decimal,
}

// ── Graph-Only Signals (require multi-entity graph traversal) ──

/// Same institution buying some stocks and selling others simultaneously.
/// Only detectable by traversing Institution→Stock edges across multiple stocks.
#[derive(Debug, Clone)]
pub struct InstitutionRotation {
    pub institution_id: InstitutionId,
    pub buy_symbols: Vec<Symbol>,
    pub sell_symbols: Vec<Symbol>,
    pub net_direction: Decimal,
}

/// Institution suddenly disappearing from multiple stocks (degree drop).
/// Requires comparing Institution node's edge count across ticks.
#[derive(Debug, Clone)]
pub struct InstitutionExodus {
    pub institution_id: InstitutionId,
    pub prev_stock_count: usize,
    pub curr_stock_count: usize,
    pub dropped_count: usize,
}

/// Two stocks in different sectors held by nearly identical institution sets.
/// Requires comparing incoming Institution→Stock edge sets of two Stock nodes.
#[derive(Debug, Clone)]
pub struct SharedHolderAnomaly {
    pub symbol_a: Symbol,
    pub symbol_b: Symbol,
    pub sector_a: Option<SectorId>,
    pub sector_b: Option<SectorId>,
    pub jaccard: Decimal,
    pub shared_institutions: usize,
}

/// Aggregate market stress indicator computed from graph-wide patterns.
#[derive(Debug, Clone)]
pub struct MarketStressIndex {
    pub sector_synchrony: Decimal,
    pub pressure_consensus: Decimal,
    pub conflict_intensity_mean: Decimal,
    pub market_temperature_stress: Decimal,
    pub composite_stress: Decimal,
}

// ── ConflictHistory ──

#[derive(Debug, Clone)]
struct ConflictRecord {
    first_seen: u64,
    last_seen: u64,
    prev_intensity: Decimal,
    count: u64,
}

#[derive(Debug, Clone)]
pub struct ConflictHistory {
    records: HashMap<(InstitutionId, InstitutionId), ConflictRecord>,
    max_entries: usize,
}

const DEFAULT_CONFLICT_HISTORY_MAX: usize = 4096;

impl ConflictHistory {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            max_entries: DEFAULT_CONFLICT_HISTORY_MAX,
        }
    }

    /// Evict the single oldest entry when the history exceeds its capacity.
    fn evict_if_full(&mut self) {
        if self.records.len() <= self.max_entries {
            return;
        }
        // Only one entry is added per update, so at most one needs eviction.
        if let Some(oldest_key) = self
            .records
            .iter()
            .min_by_key(|(_, v)| v.last_seen)
            .map(|(k, _)| *k)
        {
            self.records.remove(&oldest_key);
        }
    }

    fn canonical_key(a: InstitutionId, b: InstitutionId) -> (InstitutionId, InstitutionId) {
        if a.0 <= b.0 {
            (a, b)
        } else {
            (b, a)
        }
    }

    fn update(
        &mut self,
        a: InstitutionId,
        b: InstitutionId,
        intensity: Decimal,
        tick: u64,
    ) -> (u64, Decimal) {
        let key = Self::canonical_key(a, b);
        let record = self.records.entry(key).or_insert(ConflictRecord {
            first_seen: tick,
            last_seen: tick,
            prev_intensity: intensity,
            count: 0,
        });
        let age = tick.saturating_sub(record.first_seen);
        let intensity_delta = intensity - record.prev_intensity;
        record.last_seen = tick;
        record.prev_intensity = intensity;
        record.count += 1;
        self.evict_if_full();
        (age, intensity_delta)
    }
}

// ── GraphInsights ──

#[derive(Debug)]
pub struct GraphInsights {
    pub pressures: Vec<StockPressure>,
    pub rotations: Vec<RotationPair>,
    pub clusters: Vec<StockCluster>,
    pub conflicts: Vec<InstitutionalConflict>,
    // Graph-only signals
    pub inst_rotations: Vec<InstitutionRotation>,
    pub inst_exoduses: Vec<InstitutionExodus>,
    pub shared_holders: Vec<SharedHolderAnomaly>,
    pub stress: MarketStressIndex,
    // Per-institution stock counts for cross-tick exodus detection
    pub institution_stock_counts: HashMap<InstitutionId, usize>,
    pub edge_profiles: Vec<EdgeTemporalProfile>,
}

impl GraphInsights {
    pub fn compute(
        brain: &BrainGraph,
        store: &ObjectStore,
        prev: Option<&GraphInsights>,
        conflict_history: &mut ConflictHistory,
        tick: u64,
    ) -> Self {
        let pressures = compute_pressures(brain, prev);
        let rotations = compute_rotations(brain, prev);
        let clusters = compute_clusters(brain, store, prev);
        let conflicts = compute_conflicts(brain, store, conflict_history, tick);
        let inst_rotations = compute_institution_rotations(brain);
        let institution_stock_counts = collect_institution_stock_counts(brain);
        let inst_exoduses = compute_institution_exoduses(&institution_stock_counts, prev);
        let shared_holders = compute_shared_holders(brain, store);
        let stress = compute_stress_index(brain, &pressures, &conflicts);

        GraphInsights {
            pressures,
            rotations,
            clusters,
            conflicts,
            inst_rotations,
            inst_exoduses,
            shared_holders,
            stress,
            institution_stock_counts,
            edge_profiles: Vec::new(),
        }
    }
}

// ── 1. StockPressure (with delta, duration, acceleration) ──

// ── 2. SectorRotation (with spread_delta, widening) ──

// ── 3. StockClusters (with stability, age) ──

// ── 4. InstitutionalConflict (with conflict_age, intensity_delta) ──

// ── 5. InstitutionRotation (graph-only: same institution buying A, selling B) ──

// ── 6. InstitutionExodus (graph-only: degree drop across ticks) ──

// ── 7. SharedHolderAnomaly (graph-only: cross-sector stocks with same institution set) ──

// ── 8. MarketStressIndex (graph-wide anomaly detection) ──

// ── Display helpers ──



#[cfg(test)]
#[path = "insights_tests.rs"]
mod tests;
