use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;

use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::ontology::store::ObjectStore;

use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};
use super::{ConflictHistory, GraphInsights, InstitutionalConflict, StockCluster};

pub(super) fn compute_clusters(
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

pub(super) fn compute_conflicts(
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
