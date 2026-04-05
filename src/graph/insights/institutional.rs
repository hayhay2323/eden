use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;

use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};
use crate::ontology::objects::{InstitutionId, Symbol};
use crate::ontology::store::ObjectStore;

use super::{GraphInsights, InstitutionExodus, InstitutionRotation, SharedHolderAnomaly};

pub(super) fn compute_institution_rotations(brain: &BrainGraph) -> Vec<InstitutionRotation> {
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

pub(super) fn collect_institution_stock_counts(
    brain: &BrainGraph,
) -> HashMap<InstitutionId, usize> {
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

pub(super) fn compute_institution_exoduses(
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

pub(super) fn compute_shared_holders(
    brain: &BrainGraph,
    store: &ObjectStore,
) -> Vec<SharedHolderAnomaly> {
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
