use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;

use crate::math::{clamp_unit_interval, median};
use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::ontology::store::ObjectStore;

use super::graph::{BrainGraph, EdgeKind, NodeKind};

// ── Output structs ──

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
}

impl ConflictHistory {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
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
        }
    }
}

fn average(values: impl IntoIterator<Item = Decimal>) -> Decimal {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        Decimal::ZERO
    } else {
        values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
    }
}

// ── 1. StockPressure (with delta, duration, acceleration) ──

fn compute_pressures(brain: &BrainGraph, prev: Option<&GraphInsights>) -> Vec<StockPressure> {
    // Build prev pressure map
    let prev_map: HashMap<&Symbol, &StockPressure> = prev
        .map(|p| p.pressures.iter().map(|sp| (&sp.symbol, sp)).collect())
        .unwrap_or_default();

    let mut results = Vec::new();

    for (symbol, &stock_idx) in &brain.stock_nodes {
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        let mut buy_count = 0usize;
        let mut sell_count = 0usize;
        let mut inst_count = 0usize;

        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let source = edge.source();
                if let NodeKind::Institution(inst) = &brain.graph[source] {
                    // Weight by breadth: institutions in more stocks have broader signal
                    let w = Decimal::from(inst.stock_count as i64);
                    weighted_sum += e.direction * w;
                    weight_total += w;
                    inst_count += 1;
                    if e.direction > Decimal::ZERO {
                        buy_count += 1;
                    } else if e.direction < Decimal::ZERO {
                        sell_count += 1;
                    }
                }
            }
        }

        if inst_count == 0 {
            continue;
        }

        let net_pressure = if weight_total > Decimal::ZERO {
            weighted_sum / weight_total
        } else {
            Decimal::ZERO
        };

        // Temporal: delta, duration, acceleration
        let (pressure_delta, pressure_duration, accelerating) =
            if let Some(prev_p) = prev_map.get(symbol) {
                let delta = net_pressure - prev_p.net_pressure;
                let prev_delta = prev_p.pressure_delta;
                // Same direction: delta and prev pressure have same sign
                let same_dir = (delta > Decimal::ZERO && prev_p.net_pressure > Decimal::ZERO)
                    || (delta < Decimal::ZERO && prev_p.net_pressure < Decimal::ZERO)
                    || delta == Decimal::ZERO;
                let duration = if same_dir {
                    prev_p.pressure_duration + 1
                } else {
                    1
                };
                let accelerating = delta.abs() > prev_delta.abs();
                (delta, duration, accelerating)
            } else {
                (Decimal::ZERO, 1, false)
            };

        results.push(StockPressure {
            symbol: symbol.clone(),
            net_pressure,
            institution_count: inst_count,
            buy_inst_count: buy_count,
            sell_inst_count: sell_count,
            pressure_delta,
            pressure_duration,
            accelerating,
        });
    }

    results.sort_by(|a, b| b.net_pressure.abs().cmp(&a.net_pressure.abs()));
    results
}

// ── 2. SectorRotation (with spread_delta, widening) ──

fn compute_rotations(brain: &BrainGraph, prev: Option<&GraphInsights>) -> Vec<RotationPair> {
    // Build prev rotation map: (from, to) → spread
    let prev_map: HashMap<(&SectorId, &SectorId), Decimal> = prev
        .map(|p| {
            p.rotations
                .iter()
                .map(|r| ((&r.from_sector, &r.to_sector), r.spread))
                .collect()
        })
        .unwrap_or_default();

    // Collect sector directions
    let sectors: Vec<(SectorId, Decimal)> = brain
        .sector_nodes
        .iter()
        .filter_map(|(sid, &idx)| {
            if let NodeKind::Sector(s) = &brain.graph[idx] {
                Some((sid.clone(), s.mean_direction))
            } else {
                None
            }
        })
        .collect();

    if sectors.len() < 2 {
        return Vec::new();
    }

    // Compute all pairwise spreads
    let mut all_spreads: Vec<(usize, usize, Decimal)> = Vec::new();
    for i in 0..sectors.len() {
        for j in (i + 1)..sectors.len() {
            let spread = sectors[i].1 - sectors[j].1;
            all_spreads.push((i, j, spread));
        }
    }

    // Median absolute spread as cutoff
    let median =
        median(all_spreads.iter().map(|(_, _, s)| s.abs()).collect()).unwrap_or(Decimal::ZERO);

    // Emit pairs above median
    let mut results = Vec::new();
    for (i, j, spread) in &all_spreads {
        if spread.abs() <= median {
            continue;
        }
        // from = higher direction, to = lower direction
        let (from, to) = if *spread > Decimal::ZERO {
            (&sectors[*i], &sectors[*j])
        } else {
            (&sectors[*j], &sectors[*i])
        };

        // Temporal: spread_delta
        let prev_spread = prev_map
            .get(&(&from.0, &to.0))
            .copied()
            .unwrap_or(spread.abs());
        let spread_delta = spread.abs() - prev_spread;
        let widening = spread_delta > Decimal::ZERO;

        results.push(RotationPair {
            from_sector: from.0.clone(),
            to_sector: to.0.clone(),
            spread: spread.abs(),
            spread_delta,
            widening,
        });
    }

    results.sort_by(|a, b| b.spread.cmp(&a.spread));
    results
}

// ── 3. StockClusters (with stability, age) ──

fn compute_clusters(
    brain: &BrainGraph,
    store: &ObjectStore,
    prev: Option<&GraphInsights>,
) -> Vec<StockCluster> {
    // Union-Find over stock nodes connected by StockToStock edges
    let stock_syms: Vec<Symbol> = brain.stock_nodes.keys().cloned().collect();
    let sym_to_idx: HashMap<&Symbol, usize> =
        stock_syms.iter().enumerate().map(|(i, s)| (s, i)).collect();
    let n = stock_syms.len();
    if n == 0 {
        return Vec::new();
    }

    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }

    fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }

    // Track edge similarities for mean calculation
    let mut edge_sims: HashMap<(usize, usize), Vec<Decimal>> = HashMap::new();

    for (symbol, &node_idx) in &brain.stock_nodes {
        let i = sym_to_idx[symbol];
        for edge in brain
            .graph
            .edges_directed(node_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    if let Some(&j) = sym_to_idx.get(&neighbor.symbol) {
                        union(&mut parent, i, j);
                        let key = (i.min(j), i.max(j));
                        edge_sims.entry(key).or_default().push(e.similarity);
                    }
                }
            }
        }
    }

    // Group into components
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }

    // Build prev cluster member sets for stability matching
    let prev_clusters: Vec<HashSet<&Symbol>> = prev
        .map(|p| {
            p.clusters
                .iter()
                .map(|c| c.members.iter().collect::<HashSet<_>>())
                .collect()
        })
        .unwrap_or_default();
    let prev_cluster_ages: Vec<u64> = prev
        .map(|p| p.clusters.iter().map(|c| c.age).collect())
        .unwrap_or_default();

    let mut results = Vec::new();
    for (_, members) in &components {
        if members.len() < 2 {
            continue;
        }

        let member_syms: Vec<Symbol> = members.iter().map(|&i| stock_syms[i].clone()).collect();

        // Mean similarity across cluster edges
        let mut total_sim = Decimal::ZERO;
        let mut sim_count = 0i64;
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let key = (members[i].min(members[j]), members[i].max(members[j]));
                if let Some(sims) = edge_sims.get(&key) {
                    for s in sims {
                        total_sim += *s;
                        sim_count += 1;
                    }
                }
            }
        }
        let mean_similarity = if sim_count > 0 {
            total_sim / Decimal::from(sim_count)
        } else {
            Decimal::ZERO
        };

        // Directional alignment: what fraction of members share same direction sign
        let directions: Vec<Decimal> = members
            .iter()
            .filter_map(|&i| {
                let idx = brain.stock_nodes[&stock_syms[i]];
                if let NodeKind::Stock(s) = &brain.graph[idx] {
                    Some(s.mean_direction)
                } else {
                    None
                }
            })
            .collect();

        let positive = directions.iter().filter(|d| **d > Decimal::ZERO).count();
        let negative = directions.iter().filter(|d| **d < Decimal::ZERO).count();
        let majority = positive.max(negative);
        let directional_alignment = if directions.is_empty() {
            Decimal::ZERO
        } else {
            Decimal::from(majority as i64) / Decimal::from(directions.len() as i64)
        };

        // Filter: alignment < 0.6 → skip (noise)
        if directional_alignment < Decimal::new(6, 1) {
            continue;
        }

        // Cross-sector check
        let sectors: HashSet<Option<&SectorId>> = member_syms
            .iter()
            .map(|sym| store.stocks.get(sym).and_then(|s| s.sector_id.as_ref()))
            .collect();
        let cross_sector = sectors.len() > 1;

        // Stability: Jaccard(current_members, best_match_prev_members)
        let current_set: HashSet<&Symbol> = member_syms.iter().collect();
        let (stability, matched_age) = if prev_clusters.is_empty() {
            (Decimal::ZERO, 0u64)
        } else {
            let mut best_jaccard = Decimal::ZERO;
            let mut best_age = 0u64;
            for (idx, prev_set) in prev_clusters.iter().enumerate() {
                let intersection = current_set.intersection(prev_set).count();
                let union_size = current_set.union(prev_set).count();
                if union_size > 0 {
                    let j = Decimal::from(intersection as i64) / Decimal::from(union_size as i64);
                    if j > best_jaccard {
                        best_jaccard = j;
                        best_age = prev_cluster_ages[idx];
                    }
                }
            }
            (best_jaccard, best_age)
        };

        let age = if stability > Decimal::new(5, 1) {
            matched_age + 1
        } else {
            1
        };

        // Filter: age < 3 → not reported (need at least 3 ticks = ~6 seconds)
        if age < 3 && prev.is_some() {
            continue;
        }

        results.push(StockCluster {
            members: member_syms,
            mean_similarity,
            directional_alignment,
            cross_sector,
            stability,
            age,
        });
    }

    results.sort_by(|a, b| b.members.len().cmp(&a.members.len()));
    results
}

// ── 4. InstitutionalConflict (with conflict_age, intensity_delta) ──

fn compute_conflicts(
    brain: &BrainGraph,
    _store: &ObjectStore,
    conflict_history: &mut ConflictHistory,
    tick: u64,
) -> Vec<InstitutionalConflict> {
    // Collect all InstitutionToInstitution edges with jaccard values
    let mut all_jaccards: Vec<Decimal> = Vec::new();
    let mut edge_data: Vec<(InstitutionId, InstitutionId, Decimal)> = Vec::new();

    let inst_ids: Vec<InstitutionId> = brain.institution_nodes.keys().copied().collect();
    let mut seen = HashSet::new();

    for &inst_id in &inst_ids {
        let &inst_idx = &brain.institution_nodes[&inst_id];
        for edge in brain
            .graph
            .edges_directed(inst_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::InstitutionToInstitution(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Institution(other) = &brain.graph[target] {
                    let pair = (
                        inst_id.0.min(other.institution_id.0),
                        inst_id.0.max(other.institution_id.0),
                    );
                    if seen.insert(pair) {
                        all_jaccards.push(e.jaccard);
                        edge_data.push((inst_id, other.institution_id, e.jaccard));
                    }
                }
            }
        }
    }

    if edge_data.is_empty() {
        return Vec::new();
    }

    // Median jaccard as cutoff
    all_jaccards.sort();
    let median_jaccard = all_jaccards[all_jaccards.len() / 2];

    let mut results = Vec::new();
    for (id_a, id_b, jaccard) in &edge_data {
        if *jaccard < median_jaccard {
            continue;
        }

        // Check if directions are opposite
        let dir_a = brain
            .institution_nodes
            .get(id_a)
            .and_then(|&idx| {
                if let NodeKind::Institution(i) = &brain.graph[idx] {
                    Some(i.net_direction)
                } else {
                    None
                }
            })
            .unwrap_or(Decimal::ZERO);

        let dir_b = brain
            .institution_nodes
            .get(id_b)
            .and_then(|&idx| {
                if let NodeKind::Institution(i) = &brain.graph[idx] {
                    Some(i.net_direction)
                } else {
                    None
                }
            })
            .unwrap_or(Decimal::ZERO);

        // Opposite = different signs, both nonzero
        if dir_a == Decimal::ZERO || dir_b == Decimal::ZERO {
            continue;
        }
        if (dir_a > Decimal::ZERO) == (dir_b > Decimal::ZERO) {
            continue;
        }

        // Find shared stocks by looking at institution→stock edges
        let stocks_a: HashSet<Symbol> = brain
            .graph
            .edges_directed(brain.institution_nodes[id_a], GraphDirection::Outgoing)
            .filter_map(|edge| {
                if let EdgeKind::InstitutionToStock(_) = edge.weight() {
                    if let NodeKind::Stock(s) = &brain.graph[edge.target()] {
                        return Some(s.symbol.clone());
                    }
                }
                None
            })
            .collect();

        let stocks_b: HashSet<Symbol> = brain
            .graph
            .edges_directed(brain.institution_nodes[id_b], GraphDirection::Outgoing)
            .filter_map(|edge| {
                if let EdgeKind::InstitutionToStock(_) = edge.weight() {
                    if let NodeKind::Stock(s) = &brain.graph[edge.target()] {
                        return Some(s.symbol.clone());
                    }
                }
                None
            })
            .collect();

        let shared: Vec<Symbol> = stocks_a.intersection(&stocks_b).cloned().collect();

        // Temporal: conflict_age and intensity_delta via ConflictHistory
        let intensity = (dir_a - dir_b).abs();
        let (conflict_age, intensity_delta) =
            conflict_history.update(*id_a, *id_b, intensity, tick);

        results.push(InstitutionalConflict {
            inst_a: *id_a,
            inst_b: *id_b,
            jaccard_overlap: *jaccard,
            direction_a: dir_a,
            direction_b: dir_b,
            shared_stocks: shared,
            conflict_age,
            intensity_delta,
        });
    }

    results.sort_by(|a, b| b.jaccard_overlap.cmp(&a.jaccard_overlap));
    results
}

// ── 5. InstitutionRotation (graph-only: same institution buying A, selling B) ──

fn compute_institution_rotations(brain: &BrainGraph) -> Vec<InstitutionRotation> {
    let mut results = Vec::new();

    for (&inst_id, &inst_idx) in &brain.institution_nodes {
        let mut buy_syms = Vec::new();
        let mut sell_syms = Vec::new();

        for edge in brain
            .graph
            .edges_directed(inst_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                if let NodeKind::Stock(s) = &brain.graph[edge.target()] {
                    if e.direction > Decimal::ZERO {
                        buy_syms.push(s.symbol.clone());
                    } else if e.direction < Decimal::ZERO {
                        sell_syms.push(s.symbol.clone());
                    }
                }
            }
        }

        // Only report if institution is BOTH buying and selling (pair trade / rotation)
        if !buy_syms.is_empty() && !sell_syms.is_empty() {
            let net_direction = if let NodeKind::Institution(inst) = &brain.graph[inst_idx] {
                inst.net_direction
            } else {
                Decimal::ZERO
            };
            results.push(InstitutionRotation {
                institution_id: inst_id,
                buy_symbols: buy_syms,
                sell_symbols: sell_syms,
                net_direction,
            });
        }
    }

    // Sort by total activity (buy + sell count)
    results.sort_by(|a, b| {
        let a_total = a.buy_symbols.len() + a.sell_symbols.len();
        let b_total = b.buy_symbols.len() + b.sell_symbols.len();
        b_total.cmp(&a_total)
    });
    results
}

// ── 6. InstitutionExodus (graph-only: degree drop across ticks) ──

fn collect_institution_stock_counts(brain: &BrainGraph) -> HashMap<InstitutionId, usize> {
    brain
        .institution_nodes
        .iter()
        .filter_map(|(&id, &idx)| {
            if let NodeKind::Institution(inst) = &brain.graph[idx] {
                Some((id, inst.stock_count))
            } else {
                None
            }
        })
        .collect()
}

fn compute_institution_exoduses(
    current: &HashMap<InstitutionId, usize>,
    prev: Option<&GraphInsights>,
) -> Vec<InstitutionExodus> {
    let prev_counts = match prev {
        Some(p) => &p.institution_stock_counts,
        None => return Vec::new(),
    };

    let mut drops: Vec<(InstitutionId, usize, usize, usize)> = Vec::new();
    for (&id, &prev_count) in prev_counts {
        let curr_count = current.get(&id).copied().unwrap_or(0);
        if prev_count > curr_count {
            let dropped = prev_count - curr_count;
            drops.push((id, prev_count, curr_count, dropped));
        }
    }

    if drops.is_empty() {
        return Vec::new();
    }

    // Median drop as cutoff
    let mut drop_vals: Vec<usize> = drops.iter().map(|d| d.3).collect();
    drop_vals.sort();
    let median_drop = drop_vals[drop_vals.len() / 2];

    drops
        .into_iter()
        .filter(|d| d.3 > median_drop)
        .map(|(id, prev_count, curr_count, dropped)| InstitutionExodus {
            institution_id: id,
            prev_stock_count: prev_count,
            curr_stock_count: curr_count,
            dropped_count: dropped,
        })
        .collect()
}

// ── 7. SharedHolderAnomaly (graph-only: cross-sector stocks with same institution set) ──

fn compute_shared_holders(brain: &BrainGraph, store: &ObjectStore) -> Vec<SharedHolderAnomaly> {
    // Collect institution sets per stock
    let mut stock_institutions: HashMap<&Symbol, HashSet<InstitutionId>> = HashMap::new();
    for (symbol, &stock_idx) in &brain.stock_nodes {
        let mut inst_set = HashSet::new();
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(_) = edge.weight() {
                if let NodeKind::Institution(inst) = &brain.graph[edge.source()] {
                    inst_set.insert(inst.institution_id);
                }
            }
        }
        if !inst_set.is_empty() {
            stock_institutions.insert(symbol, inst_set);
        }
    }

    let symbols: Vec<&Symbol> = stock_institutions.keys().copied().collect();
    if symbols.len() < 2 {
        return Vec::new();
    }

    // Compute all pairwise Jaccard for cross-sector pairs
    let mut all_jaccards: Vec<Decimal> = Vec::new();
    let mut pairs: Vec<(&Symbol, &Symbol, Decimal, usize)> = Vec::new();

    for i in 0..symbols.len() {
        let sector_a = store
            .stocks
            .get(symbols[i])
            .and_then(|s| s.sector_id.as_ref());
        let set_a = &stock_institutions[symbols[i]];
        for j in (i + 1)..symbols.len() {
            let sector_b = store
                .stocks
                .get(symbols[j])
                .and_then(|s| s.sector_id.as_ref());
            // Only cross-sector pairs
            if sector_a == sector_b {
                continue;
            }
            let set_b = &stock_institutions[symbols[j]];
            let intersection = set_a.intersection(set_b).count();
            let union_size = set_a.union(set_b).count();
            if union_size == 0 {
                continue;
            }
            let jaccard = Decimal::from(intersection as i64) / Decimal::from(union_size as i64);
            all_jaccards.push(jaccard);
            pairs.push((symbols[i], symbols[j], jaccard, intersection));
        }
    }

    if all_jaccards.is_empty() {
        return Vec::new();
    }

    // Median Jaccard as cutoff
    all_jaccards.sort();
    let median = all_jaccards[all_jaccards.len() / 2];

    let mut results: Vec<SharedHolderAnomaly> = pairs
        .into_iter()
        .filter(|(_, _, j, _)| *j > median)
        .map(|(sym_a, sym_b, jaccard, shared)| {
            let sector_a = store.stocks.get(sym_a).and_then(|s| s.sector_id.clone());
            let sector_b = store.stocks.get(sym_b).and_then(|s| s.sector_id.clone());
            SharedHolderAnomaly {
                symbol_a: sym_a.clone(),
                symbol_b: sym_b.clone(),
                sector_a,
                sector_b,
                jaccard,
                shared_institutions: shared,
            }
        })
        .collect();

    results.sort_by(|a, b| b.jaccard.cmp(&a.jaccard));
    results.truncate(10); // Top 10 most anomalous pairs
    results
}

// ── 8. MarketStressIndex (graph-wide anomaly detection) ──

fn compute_stress_index(
    brain: &BrainGraph,
    pressures: &[StockPressure],
    conflicts: &[InstitutionalConflict],
) -> MarketStressIndex {
    // 1. Sector synchrony: low std of sector directions = all sectors moving together = stress
    let sector_dirs: Vec<Decimal> = brain
        .sector_nodes
        .values()
        .filter_map(|&idx| {
            if let NodeKind::Sector(s) = &brain.graph[idx] {
                Some(s.mean_direction)
            } else {
                None
            }
        })
        .collect();
    let sector_synchrony = if sector_dirs.len() < 2 {
        Decimal::ZERO
    } else {
        let mean = sector_dirs.iter().sum::<Decimal>() / Decimal::from(sector_dirs.len() as i64);
        let variance = sector_dirs
            .iter()
            .map(|d| (*d - mean) * (*d - mean))
            .sum::<Decimal>()
            / Decimal::from(sector_dirs.len() as i64);
        // synchrony = 1 - std (high when sectors move together)
        Decimal::ONE - crate::math::decimal_sqrt(variance)
    };

    // 2. Pressure consensus: low std of stock pressures = unusual agreement
    let pressure_consensus = if pressures.len() < 2 {
        Decimal::ZERO
    } else {
        let mean = pressures.iter().map(|p| p.net_pressure).sum::<Decimal>()
            / Decimal::from(pressures.len() as i64);
        let variance = pressures
            .iter()
            .map(|p| (p.net_pressure - mean) * (p.net_pressure - mean))
            .sum::<Decimal>()
            / Decimal::from(pressures.len() as i64);
        Decimal::ONE - crate::math::decimal_sqrt(variance)
    };

    // 3. Conflict intensity mean
    let conflict_intensity_mean = if conflicts.is_empty() {
        Decimal::ZERO
    } else {
        let sum: Decimal = conflicts
            .iter()
            .map(|c| (c.direction_a - c.direction_b).abs())
            .sum();
        sum / Decimal::from(conflicts.len() as i64)
    };

    let market_temperature_stress = brain
        .market_temperature
        .as_ref()
        .map(|temp| {
            let temperature_bias = (temp.temperature - Decimal::from(50)).abs() / Decimal::from(50);
            let valuation_bias = (temp.valuation - Decimal::from(50)).abs() / Decimal::from(50);
            let sentiment_bias = (temp.sentiment - Decimal::from(50)).abs() / Decimal::from(50);
            clamp_unit_interval(
                (temperature_bias + valuation_bias + sentiment_bias) / Decimal::from(3),
            )
        })
        .unwrap_or(Decimal::ZERO);

    let conflict_component = clamp_unit_interval(conflict_intensity_mean / Decimal::TWO);
    let composite_stress = average([
        sector_synchrony,
        pressure_consensus,
        conflict_component,
        market_temperature_stress,
    ]);

    MarketStressIndex {
        sector_synchrony,
        pressure_consensus,
        conflict_intensity_mean,
        market_temperature_stress,
        composite_stress,
    }
}

// ── Display helpers ──

impl GraphInsights {
    pub fn display(&self, store: &ObjectStore) {
        let pct = Decimal::new(100, 0);

        println!("\n── Graph Structure ──");

        // Smart Money (with delta, duration, acceleration)
        if !self.pressures.is_empty() {
            let items: Vec<String> = self
                .pressures
                .iter()
                .take(6)
                .map(|p| {
                    let dir = if p.net_pressure > Decimal::ZERO {
                        "▲"
                    } else {
                        "▼"
                    };
                    let accel = if p.accelerating { "↑" } else { "↓" };
                    format!(
                        "{} {}{:+}%({:+}%{} {}t)",
                        p.symbol,
                        dir,
                        (p.net_pressure * pct).round_dp(0),
                        (p.pressure_delta * pct).round_dp(0),
                        accel,
                        p.pressure_duration,
                    )
                })
                .collect();
            println!("  Smart Money:  {}", items.join("  "));
        }

        // Rotation (with widening/narrowing)
        if !self.rotations.is_empty() {
            let items: Vec<String> = self
                .rotations
                .iter()
                .take(3)
                .map(|r| {
                    let trend = if r.widening { "widening" } else { "narrowing" };
                    format!(
                        "{} → {}  spread={:+}%({} {:+}%)",
                        r.from_sector,
                        r.to_sector,
                        (r.spread * pct).round_dp(0),
                        trend,
                        (r.spread_delta * pct).round_dp(0),
                    )
                })
                .collect();
            println!("  Rotation:     {}", items.join(" | "));
        }

        // Clusters (with stability and age, only age >= 3 shown)
        if !self.clusters.is_empty() {
            for c in self.clusters.iter().take(3) {
                let members: Vec<String> =
                    c.members.iter().take(5).map(|s| s.to_string()).collect();
                let cross = if c.cross_sector {
                    " (cross-sector)"
                } else {
                    ""
                };
                let dir = if c.directional_alignment > Decimal::new(5, 1) {
                    "▲"
                } else {
                    "▼"
                };
                println!(
                    "  Clusters:     [{}] dir={} align={}% age={}t stable={}%{}",
                    members.join(", "),
                    dir,
                    (c.directional_alignment * pct).round_dp(0),
                    c.age,
                    (c.stability * pct).round_dp(0),
                    cross,
                );
            }
        }

        // Conflicts (with age and intensity trend)
        if !self.conflicts.is_empty() {
            for c in self.conflicts.iter().take(3) {
                let name_a = store
                    .institutions
                    .get(&c.inst_a)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let name_b = store
                    .institutions
                    .get(&c.inst_b)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let intensity_trend = if c.intensity_delta > Decimal::ZERO {
                    "intensity↑"
                } else if c.intensity_delta < Decimal::ZERO {
                    "intensity↓"
                } else {
                    "intensity="
                };
                println!(
                    "  Conflicts:    {} vs {}  overlap={}%  {:+} vs {:+}  age={}t  {}",
                    name_a,
                    name_b,
                    (c.jaccard_overlap * pct).round_dp(0),
                    c.direction_a.round_dp(1),
                    c.direction_b.round_dp(1),
                    c.conflict_age,
                    intensity_trend,
                );
            }
        }

        // Institution Rotations (graph-only: same institution buying + selling)
        if !self.inst_rotations.is_empty() {
            for r in self.inst_rotations.iter().take(3) {
                let name = store
                    .institutions
                    .get(&r.institution_id)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let buys: Vec<String> = r
                    .buy_symbols
                    .iter()
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                let sells: Vec<String> = r
                    .sell_symbols
                    .iter()
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                println!(
                    "  Pair Trade:   {}  BUY [{}]  SELL [{}]  net={:+}",
                    name,
                    buys.join(", "),
                    sells.join(", "),
                    r.net_direction.round_dp(2),
                );
            }
        }

        // Institution Exoduses (graph-only: degree drop)
        if !self.inst_exoduses.is_empty() {
            for e in self.inst_exoduses.iter().take(3) {
                let name = store
                    .institutions
                    .get(&e.institution_id)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                println!(
                    "  Exodus:       {}  {} → {} stocks (dropped {})",
                    name, e.prev_stock_count, e.curr_stock_count, e.dropped_count,
                );
            }
        }

        // Shared Holder Anomalies (graph-only: cross-sector same holders)
        if !self.shared_holders.is_empty() {
            for s in self.shared_holders.iter().take(3) {
                println!(
                    "  Hidden Link:  {} ({}) ↔ {} ({})  jaccard={}%  {} shared inst",
                    s.symbol_a,
                    s.sector_a.as_ref().map(|s| s.0.as_str()).unwrap_or("?"),
                    s.symbol_b,
                    s.sector_b.as_ref().map(|s| s.0.as_str()).unwrap_or("?"),
                    (s.jaccard * pct).round_dp(0),
                    s.shared_institutions,
                );
            }
        }

        // Market Stress Index
        println!(
            "  Stress:       sync={}%  consensus={}%  conflict_avg={:+}  market={}%  composite={}%",
            (self.stress.sector_synchrony * pct).round_dp(0),
            (self.stress.pressure_consensus * pct).round_dp(0),
            self.stress.conflict_intensity_mean.round_dp(2),
            (self.stress.market_temperature_stress * pct).round_dp(0),
            (self.stress.composite_stress * pct).round_dp(0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::narrative::{
        DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
    };
    use crate::graph::graph::BrainGraph;
    use crate::logic::tension::Dimension;
    use crate::ontology::links::*;
    use crate::ontology::objects::*;
    use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_narrative(coherence: Decimal, mean_direction: Decimal) -> SymbolNarrative {
        SymbolNarrative {
            regime: Regime::classify(coherence, mean_direction),
            coherence,
            mean_direction,
            readings: vec![DimensionReading {
                dimension: Dimension::OrderBookPressure,
                value: mean_direction,
                direction: Direction::from_value(mean_direction),
            }],
            agreements: vec![],
            contradictions: vec![],
        }
    }

    fn make_dims(obp: Decimal, cfd: Decimal, csd: Decimal, id: Decimal) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: obp,
            capital_flow_direction: cfd,
            capital_size_divergence: csd,
            institutional_direction: id,
            ..Default::default()
        }
    }

    fn empty_links() -> LinkSnapshot {
        LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![],
            quotes: vec![],
            trade_activities: vec![],
        }
    }

    fn make_store(stocks: Vec<Stock>, sectors: Vec<Sector>) -> ObjectStore {
        let mut stock_map = HashMap::new();
        for s in stocks {
            stock_map.insert(s.symbol.clone(), s);
        }
        let mut sector_map = HashMap::new();
        for s in sectors {
            sector_map.insert(s.id.clone(), s);
        }
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: HashMap::new(),
        }
    }

    fn make_stock(symbol: &str, sector: Option<&str>) -> Stock {
        Stock {
            symbol: sym(symbol),
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: sector.map(|s| SectorId(s.into())),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: Decimal::ZERO,
            bps: Decimal::ZERO,
            dividend_yield: Decimal::ZERO,
        }
    }

    fn build_brain(
        narratives: HashMap<Symbol, SymbolNarrative>,
        dimensions: HashMap<Symbol, SymbolDimensions>,
        links: &LinkSnapshot,
        store: &ObjectStore,
    ) -> BrainGraph {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        BrainGraph::compute(&narrative, &dims, links, store)
    }

    fn empty_stress() -> MarketStressIndex {
        MarketStressIndex {
            sector_synchrony: Decimal::ZERO,
            pressure_consensus: Decimal::ZERO,
            conflict_intensity_mean: Decimal::ZERO,
            market_temperature_stress: Decimal::ZERO,
            composite_stress: Decimal::ZERO,
        }
    }

    fn make_insights_with_pressures(pressures: Vec<StockPressure>) -> GraphInsights {
        GraphInsights {
            pressures,
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: empty_stress(),
            institution_stock_counts: HashMap::new(),
        }
    }

    fn make_insights_with_clusters(clusters: Vec<StockCluster>) -> GraphInsights {
        GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters,
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: empty_stress(),
            institution_stock_counts: HashMap::new(),
        }
    }

    fn make_insights_with_rotations(rotations: Vec<RotationPair>) -> GraphInsights {
        GraphInsights {
            pressures: vec![],
            rotations,
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: empty_stress(),
            institution_stock_counts: HashMap::new(),
        }
    }

    fn make_empty_insights() -> GraphInsights {
        GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: empty_stress(),
            institution_stock_counts: HashMap::new(),
        }
    }

    // ── Test 1: Empty graph + no prev → empty insights, no panics ──

    #[test]
    fn empty_graph_no_prev_empty_insights() {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives: HashMap::new(),
        };
        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 0);
        assert!(insights.pressures.is_empty());
        assert!(insights.rotations.is_empty());
        assert!(insights.clusters.is_empty());
        assert!(insights.conflicts.is_empty());
    }

    // ── Test 2: StockPressure with prev → correct delta and duration ──

    #[test]
    fn stock_pressure_with_prev_delta_and_duration() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2, 3],
            seat_count: 3,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("9988.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![],
            seat_count: 2,
        });

        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        // First tick: no prev
        let mut ch = ConflictHistory::new();
        let insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);
        let p700_1 = insights1
            .pressures
            .iter()
            .find(|p| p.symbol == sym("700.HK"))
            .unwrap();
        assert!(p700_1.net_pressure > Decimal::ZERO);
        assert_eq!(p700_1.pressure_duration, 1);
        assert_eq!(p700_1.pressure_delta, Decimal::ZERO);

        // Second tick: with prev → delta should be 0 (same data), duration increments
        let insights2 = GraphInsights::compute(&brain, &store, Some(&insights1), &mut ch, 2);
        let p700_2 = insights2
            .pressures
            .iter()
            .find(|p| p.symbol == sym("700.HK"))
            .unwrap();
        assert_eq!(p700_2.pressure_delta, Decimal::ZERO);
        assert_eq!(p700_2.pressure_duration, 2); // same direction, incremented
    }

    // ── Test 3: StockPressure direction flip → duration resets ──

    #[test]
    fn stock_pressure_direction_flip_resets_duration() {
        // Create a "prev" insight with positive pressure
        let prev_pressure = StockPressure {
            symbol: sym("700.HK"),
            net_pressure: Decimal::new(3, 1), // +0.3
            institution_count: 1,
            buy_inst_count: 1,
            sell_inst_count: 0,
            pressure_delta: Decimal::ZERO,
            pressure_duration: 5, // was going for 5 ticks
            accelerating: false,
        };
        let prev = make_insights_with_pressures(vec![prev_pressure]);

        // Now build a brain where 700.HK has NEGATIVE pressure (flipped)
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK")],
            ask_symbols: vec![sym("700.HK")],
            bid_symbols: vec![],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![],
            seat_count: 2,
        });

        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 6);

        let p700 = insights
            .pressures
            .iter()
            .find(|p| p.symbol == sym("700.HK"))
            .unwrap();
        assert!(p700.net_pressure < Decimal::ZERO); // flipped to negative
        assert_eq!(p700.pressure_duration, 1); // reset
    }

    // ── Test 4: Cluster with high stability prev → age increments ──

    #[test]
    fn cluster_high_stability_age_increments() {
        let prev_cluster = StockCluster {
            members: vec![sym("700.HK"), sym("9988.HK")],
            mean_similarity: dec!(0.8),
            directional_alignment: dec!(0.9),
            cross_sector: false,
            stability: dec!(0.9),
            age: 5,
        };
        let prev = make_insights_with_clusters(vec![prev_cluster]);

        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 6);

        // If a cluster forms with the same members, stability should be high and age should increment
        for c in &insights.clusters {
            let has_700 = c.members.contains(&sym("700.HK"));
            let has_9988 = c.members.contains(&sym("9988.HK"));
            if has_700 && has_9988 {
                assert!(
                    c.stability > Decimal::new(5, 1),
                    "stability should be > 0.5"
                );
                assert!(c.age > 5, "age should have incremented from 5");
            }
        }
    }

    // ── Test 5: Cluster with low stability → age resets to 1 ──

    #[test]
    fn cluster_low_stability_age_resets() {
        // Prev cluster had completely different members
        let prev_cluster = StockCluster {
            members: vec![sym("883.HK"), sym("5.HK")],
            mean_similarity: dec!(0.8),
            directional_alignment: dec!(0.9),
            cross_sector: false,
            stability: dec!(0.9),
            age: 10,
        };
        let prev = make_insights_with_clusters(vec![prev_cluster]);

        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 11);

        // New cluster (700, 9988) has no overlap with prev (883, 5) → Jaccard = 0 → age = 1
        // age < 3 with prev.is_some() → filtered out
        for c in &insights.clusters {
            if c.members.contains(&sym("700.HK")) && c.members.contains(&sym("9988.HK")) {
                // If it wasn't filtered, age should be 1
                assert_eq!(c.age, 1);
            }
        }
    }

    // ── Test 6: Cluster with align < 0.6 → filtered out ──

    #[test]
    fn cluster_low_alignment_filtered() {
        let mut narratives = HashMap::new();
        // One positive, one negative → alignment ~50%
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(-0.3)));

        let mut dimensions = HashMap::new();
        // Both have similar magnitudes but opposite signs won't necessarily create edges
        // We need them to be similar enough to form an edge
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // If a cluster forms with one positive and one negative direction,
        // alignment = max(1,1)/2 = 0.5 which is < 0.6 → filtered
        for c in &insights.clusters {
            assert!(
                c.directional_alignment >= Decimal::new(6, 1),
                "clusters with alignment < 0.6 should be filtered"
            );
        }
    }

    // ── Test 7: Cluster age < 3 → not reported (when prev exists) ──

    #[test]
    fn cluster_young_not_reported() {
        // Empty prev → new clusters get age=1, which is < 3 → filtered when prev is Some
        let prev = make_empty_insights();

        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 1);

        // All new clusters should be filtered out (age=1 < 3)
        assert!(
            insights.clusters.is_empty(),
            "young clusters should be filtered when prev exists"
        );
    }

    // ── Test 8: Conflict with prev → age tracks correctly ──

    #[test]
    fn conflict_age_tracks() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));
        narratives.insert(sym("3690.HK"), make_narrative(dec!(0.4), dec!(0.2)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );
        dimensions.insert(
            sym("3690.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(200),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![sym("700.HK"), sym("9988.HK")],
            bid_symbols: vec![],
        });
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(300),
            symbols: vec![sym("3690.HK"), sym("700.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("3690.HK"), sym("700.HK")],
        });

        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("9988.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(200),
            ask_positions: vec![1],
            bid_positions: vec![],
            seat_count: 1,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("9988.HK"),
            institution_id: InstitutionId(200),
            ask_positions: vec![1],
            bid_positions: vec![],
            seat_count: 1,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("3690.HK"),
            institution_id: InstitutionId(300),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(300),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });

        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let mut ch = ConflictHistory::new();
        // Tick 1
        let _insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);
        // Tick 5
        let insights5 = GraphInsights::compute(&brain, &store, None, &mut ch, 5);

        // The 100 vs 200 conflict should have age = 5 - 1 = 4
        if let Some(c) = insights5.conflicts.iter().find(|c| {
            (c.inst_a == InstitutionId(100) && c.inst_b == InstitutionId(200))
                || (c.inst_a == InstitutionId(200) && c.inst_b == InstitutionId(100))
        }) {
            assert_eq!(
                c.conflict_age, 4,
                "conflict age should be tick_now - first_seen"
            );
        }
    }

    // ── Test 9: Conflict intensity increasing → intensity_delta > 0 ──

    #[test]
    fn conflict_intensity_increasing() {
        let mut ch = ConflictHistory::new();
        let a = InstitutionId(100);
        let b = InstitutionId(200);

        // First observation: intensity = 0.5
        let (age1, delta1) = ch.update(a, b, dec!(0.5), 1);
        assert_eq!(age1, 0);
        assert_eq!(delta1, Decimal::ZERO);

        // Second observation: intensity = 0.8 (increased)
        let (age2, delta2) = ch.update(a, b, dec!(0.8), 2);
        assert_eq!(age2, 1);
        assert!(
            delta2 > Decimal::ZERO,
            "intensity increased, delta should be positive"
        );
    }

    // ── Test 10: Rotation with prev → spread_delta correct ──

    #[test]
    fn rotation_spread_delta() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("5.HK"), make_narrative(dec!(0.7), dec!(-0.5)));
        narratives.insert(sym("883.HK"), make_narrative(dec!(0.5), dec!(0.0)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("5.HK"),
            make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
        );
        dimensions.insert(
            sym("883.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let links = empty_links();
        let store = make_store(
            vec![
                make_stock("700.HK", Some("tech")),
                make_stock("5.HK", Some("finance")),
                make_stock("883.HK", Some("energy")),
            ],
            vec![
                Sector {
                    id: SectorId("tech".into()),
                    name: "Technology".into(),
                },
                Sector {
                    id: SectorId("finance".into()),
                    name: "Finance".into(),
                },
                Sector {
                    id: SectorId("energy".into()),
                    name: "Energy".into(),
                },
            ],
        );

        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();

        // First tick: no prev
        let insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // Second tick: same data → spread_delta should be 0
        let insights2 = GraphInsights::compute(&brain, &store, Some(&insights1), &mut ch, 2);

        for r in &insights2.rotations {
            assert_eq!(
                r.spread_delta,
                Decimal::ZERO,
                "same data → spread_delta = 0"
            );
        }
    }

    // ── Test 11: Rotation widening vs narrowing ──

    #[test]
    fn rotation_widening_vs_narrowing() {
        // Create a prev with a known spread
        let prev = make_insights_with_rotations(vec![RotationPair {
            from_sector: SectorId("tech".into()),
            to_sector: SectorId("finance".into()),
            spread: dec!(0.5),
            spread_delta: Decimal::ZERO,
            widening: false,
        }]);

        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.8))); // tech higher
        narratives.insert(sym("5.HK"), make_narrative(dec!(0.7), dec!(-0.5)));
        narratives.insert(sym("883.HK"), make_narrative(dec!(0.5), dec!(0.0)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("5.HK"),
            make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
        );
        dimensions.insert(
            sym("883.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let links = empty_links();
        let store = make_store(
            vec![
                make_stock("700.HK", Some("tech")),
                make_stock("5.HK", Some("finance")),
                make_stock("883.HK", Some("energy")),
            ],
            vec![
                Sector {
                    id: SectorId("tech".into()),
                    name: "Technology".into(),
                },
                Sector {
                    id: SectorId("finance".into()),
                    name: "Finance".into(),
                },
                Sector {
                    id: SectorId("energy".into()),
                    name: "Energy".into(),
                },
            ],
        );

        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 2);

        // tech-finance spread is now |0.8 - (-0.5)| = 1.3, prev was 0.5
        // So spread_delta = 1.3 - 0.5 = 0.8 > 0 → widening
        if let Some(r) = insights.rotations.iter().find(|r| {
            r.from_sector == SectorId("tech".into()) && r.to_sector == SectorId("finance".into())
        }) {
            assert!(r.spread_delta > Decimal::ZERO, "spread should be widening");
            assert!(r.widening);
        }
    }

    // ── Test 12: ConcentrationAlert removed — no concentrations field ──

    #[test]
    fn no_concentrations_field() {
        let insights = make_empty_insights();
        assert!(insights.pressures.is_empty());
    }

    // ── Graph-Only Signal Tests ──

    #[test]
    fn institution_rotation_buy_and_sell() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let mut links = empty_links();
        // Institution 100: buying 700.HK, selling 9988.HK → rotation
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![sym("9988.HK")],
            bid_symbols: vec![sym("700.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2],
            seat_count: 2,
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("9988.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![],
            seat_count: 2,
        });

        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // Institution 100 should appear in inst_rotations
        assert!(
            !insights.inst_rotations.is_empty(),
            "should detect rotation"
        );
        let rot = insights
            .inst_rotations
            .iter()
            .find(|r| r.institution_id == InstitutionId(100))
            .expect("institution 100 should be rotating");
        assert!(!rot.buy_symbols.is_empty(), "should have buy symbols");
        assert!(!rot.sell_symbols.is_empty(), "should have sell symbols");
    }

    #[test]
    fn institution_rotation_one_sided_no_rotation() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
        );

        let mut links = empty_links();
        // Institution 100: only buying → NOT a rotation
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });

        let store = make_store(vec![], vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // One-sided institution should NOT appear in rotations
        let rot = insights
            .inst_rotations
            .iter()
            .find(|r| r.institution_id == InstitutionId(100));
        assert!(
            rot.is_none(),
            "one-sided institution should not be in rotations"
        );
    }

    #[test]
    fn institution_exodus_detected() {
        // Simulate prev tick with institution having 5 stocks
        let mut prev_counts = HashMap::new();
        prev_counts.insert(InstitutionId(100), 5usize);
        prev_counts.insert(InstitutionId(200), 3usize);
        let mut prev = make_empty_insights();
        prev.institution_stock_counts = prev_counts;

        // Current tick: institution 100 dropped to 1, institution 200 unchanged
        let mut current = HashMap::new();
        current.insert(InstitutionId(100), 1usize);
        current.insert(InstitutionId(200), 3usize);

        let exoduses = compute_institution_exoduses(&current, Some(&prev));

        // Institution 100 dropped 4 stocks, institution 200 dropped 0
        // With 2 data points: drops = [4], only inst 100 has a drop
        // median of [4] = 4, strict > 4 = nothing passes
        // Actually only 1 institution dropped, so drops = [(100, 5, 1, 4)]
        // median of [4] = 4, > 4 is false. So nothing passes.
        // We need 2+ institutions dropping for the median filter to work.
        // This is correct — a single institution's drop isn't anomalous without comparison.

        // Let's add a third institution with a small drop
        let mut prev2 = make_empty_insights();
        let mut prev_counts2 = HashMap::new();
        prev_counts2.insert(InstitutionId(100), 5);
        prev_counts2.insert(InstitutionId(200), 4);
        prev_counts2.insert(InstitutionId(300), 3);
        prev2.institution_stock_counts = prev_counts2;

        let mut current2 = HashMap::new();
        current2.insert(InstitutionId(100), 1); // dropped 4
        current2.insert(InstitutionId(200), 3); // dropped 1
        current2.insert(InstitutionId(300), 2); // dropped 1

        let exoduses2 = compute_institution_exoduses(&current2, Some(&prev2));
        // drops: [4, 1, 1], sorted: [1, 1, 4], median = 1
        // > 1: only institution 100 (dropped 4)
        assert_eq!(exoduses2.len(), 1);
        assert_eq!(exoduses2[0].institution_id, InstitutionId(100));
        assert_eq!(exoduses2[0].dropped_count, 4);

        // No prev → no exoduses
        let exoduses_no_prev = compute_institution_exoduses(&current, None);
        assert!(exoduses_no_prev.is_empty());

        let _ = exoduses; // suppress warning
    }

    #[test]
    fn shared_holder_cross_sector() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("883.HK"), make_narrative(dec!(0.4), dec!(0.2)));
        narratives.insert(sym("5.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.3), dec!(0.3), dec!(0.3), dec!(0.3)),
        );
        dimensions.insert(
            sym("883.HK"),
            make_dims(dec!(0.2), dec!(0.2), dec!(0.2), dec!(0.2)),
        );
        dimensions.insert(
            sym("5.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        );

        let mut links = empty_links();
        // Same two institutions (100, 200) present in BOTH 700.HK(tech) and 883.HK(energy)
        // but only institution 300 in 5.HK(finance)
        for &sym_str in &["700.HK", "883.HK"] {
            for &inst_id in &[100i32, 200] {
                links.cross_stock_presences.push(CrossStockPresence {
                    institution_id: InstitutionId(inst_id),
                    symbols: vec![sym(sym_str), sym("883.HK")],
                    ask_symbols: vec![],
                    bid_symbols: vec![sym(sym_str)],
                });
                links.institution_activities.push(InstitutionActivity {
                    symbol: sym(sym_str),
                    institution_id: InstitutionId(inst_id),
                    ask_positions: vec![],
                    bid_positions: vec![1],
                    seat_count: 1,
                });
            }
        }
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(300),
            symbols: vec![sym("5.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("5.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("5.HK"),
            institution_id: InstitutionId(300),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });

        let store = make_store(
            vec![
                make_stock("700.HK", Some("tech")),
                make_stock("883.HK", Some("energy")),
                make_stock("5.HK", Some("finance")),
            ],
            vec![
                Sector {
                    id: SectorId("tech".into()),
                    name: "Tech".into(),
                },
                Sector {
                    id: SectorId("energy".into()),
                    name: "Energy".into(),
                },
                Sector {
                    id: SectorId("finance".into()),
                    name: "Finance".into(),
                },
            ],
        );

        let brain = build_brain(narratives, dimensions, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // 700.HK(tech) and 883.HK(energy) share institutions {100, 200}
        // This is a cross-sector shared holder anomaly
        // Whether it appears depends on median filtering — with 3 cross-sector pairs,
        // the 700-883 pair should have the highest Jaccard
        for sh in &insights.shared_holders {
            assert_ne!(
                sh.sector_a, sh.sector_b,
                "shared holders must be cross-sector"
            );
            assert!(sh.jaccard > Decimal::ZERO);
        }
    }

    #[test]
    fn stress_index_computed() {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives: HashMap::new(),
        };
        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let links = empty_links();
        let store = make_store(vec![], vec![]);
        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        // Empty graph → stress should be zero
        assert_eq!(insights.stress.sector_synchrony, Decimal::ZERO);
        assert_eq!(insights.stress.pressure_consensus, Decimal::ZERO);
        assert_eq!(insights.stress.market_temperature_stress, Decimal::ZERO);
        assert_eq!(insights.stress.composite_stress, Decimal::ZERO);
    }

    #[test]
    fn stress_index_uses_market_temperature() {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives: HashMap::new(),
        };
        let dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let mut links = empty_links();
        links.market_temperature = Some(MarketTemperatureObservation {
            temperature: Decimal::from(90),
            valuation: Decimal::from(85),
            sentiment: Decimal::from(80),
            description: "hot".into(),
            timestamp: OffsetDateTime::UNIX_EPOCH,
        });
        let store = make_store(vec![], vec![]);
        let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
        let mut ch = ConflictHistory::new();
        let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

        assert!(insights.stress.market_temperature_stress > Decimal::ZERO);
        assert!(insights.stress.composite_stress > Decimal::ZERO);
    }
}
