use std::collections::{HashMap, HashSet};

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};
use crate::graph::insights::GraphInsights;
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{PropagationPath, PropagationStep, ReasoningScope};
use crate::ontology::scope_node_id;
pub fn derive_propagation_paths(
    insights: &GraphInsights,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    const MAX_ONE_HOP_PATHS: usize = 20;
    const MAX_TWO_HOP_PATHS: usize = 20;
    const MAX_THREE_HOP_SEEDS: usize = 8;
    const MAX_THREE_HOP_PATHS: usize = 12;
    let hop_decay_2 = Decimal::new(85, 2);
    let hop_decay_3 = Decimal::new(70, 2);

    let mut one_hop_paths = Vec::new();
    one_hop_paths.extend(rotation_one_hop_paths(insights, observed_at));
    one_hop_paths.extend(shared_holder_one_hop_paths(insights));
    one_hop_paths.extend(shared_holder_bridge_paths(insights));
    one_hop_paths.extend(market_stress_sector_paths(insights));

    let two_hop_paths = derive_two_hop_paths(&one_hop_paths, hop_decay_2)
        .into_iter()
        .take(MAX_TWO_HOP_PATHS)
        .collect::<Vec<_>>();
    let three_hop_paths = derive_three_hop_paths(
        &two_hop_paths
            .iter()
            .take(MAX_THREE_HOP_SEEDS)
            .cloned()
            .collect::<Vec<_>>(),
        &one_hop_paths,
        hop_decay_3,
    )
    .into_iter()
    .take(MAX_THREE_HOP_PATHS)
    .collect::<Vec<_>>();

    one_hop_paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    let mut paths = one_hop_paths
        .into_iter()
        .take(MAX_ONE_HOP_PATHS)
        .collect::<Vec<_>>();
    paths.extend(two_hop_paths);
    paths.extend(three_hop_paths);
    paths = canonicalize_paths(paths);
    paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    paths
}

fn diffusion_source_threshold() -> Decimal {
    Decimal::new(3, 2)
}

fn diffusion_min_confidence() -> Decimal {
    Decimal::new(2, 2)
}

pub fn derive_diffusion_propagation_paths(
    brain: &BrainGraph,
    stock_deltas: &HashMap<Symbol, Decimal>,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    const MAX_ONE_HOP_PATHS: usize = 24;
    const MAX_TWO_HOP_PATHS: usize = 24;
    const MAX_THREE_HOP_SEEDS: usize = 10;
    const MAX_THREE_HOP_PATHS: usize = 16;
    let hop_decay_2 = Decimal::new(85, 2);
    let hop_decay_3 = Decimal::new(70, 2);

    let node_deltas = derive_diffusion_node_deltas(brain, stock_deltas);
    let mut one_hop_paths = derive_diffusion_one_hop_paths(brain, &node_deltas, observed_at);
    let two_hop_paths = derive_two_hop_paths(&one_hop_paths, hop_decay_2)
        .into_iter()
        .take(MAX_TWO_HOP_PATHS)
        .collect::<Vec<_>>();
    let three_hop_paths = derive_three_hop_paths(
        &two_hop_paths
            .iter()
            .take(MAX_THREE_HOP_SEEDS)
            .cloned()
            .collect::<Vec<_>>(),
        &one_hop_paths,
        hop_decay_3,
    )
    .into_iter()
    .take(MAX_THREE_HOP_PATHS)
    .collect::<Vec<_>>();

    one_hop_paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    let mut paths = one_hop_paths
        .into_iter()
        .take(MAX_ONE_HOP_PATHS)
        .collect::<Vec<_>>();
    paths.extend(two_hop_paths);
    paths.extend(three_hop_paths);
    paths = canonicalize_paths(paths);
    paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    paths
}

fn derive_diffusion_node_deltas(
    brain: &BrainGraph,
    stock_deltas: &HashMap<Symbol, Decimal>,
) -> HashMap<ReasoningScope, Decimal> {
    let mut deltas = stock_deltas
        .iter()
        .filter_map(|(symbol, delta)| {
            (delta.abs() >= diffusion_source_threshold())
                .then_some((ReasoningScope::Symbol(symbol.clone()), *delta))
        })
        .collect::<HashMap<_, _>>();

    if !stock_deltas.is_empty() {
        let market_delta = stock_deltas.values().copied().sum::<Decimal>()
            / Decimal::from(stock_deltas.len() as i64);
        if market_delta.abs() >= diffusion_source_threshold() {
            deltas.insert(ReasoningScope::market(), market_delta.round_dp(4));
        }
    }

    for (sector_id, &sector_idx) in &brain.sector_nodes {
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        for edge in brain
            .graph
            .edges_directed(sector_idx, GraphDirection::Incoming)
        {
            let EdgeKind::StockToSector(rel) = edge.weight() else {
                continue;
            };
            let NodeKind::Stock(stock) = &brain.graph[edge.source()] else {
                continue;
            };
            let Some(delta) = stock_deltas.get(&stock.symbol) else {
                continue;
            };
            if delta.abs() < diffusion_source_threshold() {
                continue;
            }
            weighted_sum += *delta * rel.weight;
            weight_total += rel.weight.abs();
        }
        if weight_total > Decimal::ZERO {
            let sector_delta = weighted_sum / weight_total;
            if sector_delta.abs() >= diffusion_source_threshold() {
                deltas.insert(
                    ReasoningScope::Sector(sector_id.clone()),
                    sector_delta.round_dp(4),
                );
            }
        }
    }

    for (institution_id, &institution_idx) in &brain.institution_nodes {
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        for edge in brain
            .graph
            .edges_directed(institution_idx, GraphDirection::Outgoing)
        {
            let EdgeKind::InstitutionToStock(rel) = edge.weight() else {
                continue;
            };
            let NodeKind::Stock(stock) = &brain.graph[edge.target()] else {
                continue;
            };
            let Some(delta) = stock_deltas.get(&stock.symbol) else {
                continue;
            };
            if delta.abs() < diffusion_source_threshold() {
                continue;
            }
            let weight = Decimal::from(rel.seat_count.min(12) as i64) / Decimal::from(12);
            weighted_sum += *delta * weight;
            weight_total += weight;
        }
        if weight_total > Decimal::ZERO {
            let institution_delta = weighted_sum / weight_total;
            if institution_delta.abs() >= diffusion_source_threshold() {
                deltas.insert(
                    ReasoningScope::Institution(*institution_id),
                    institution_delta.round_dp(4),
                );
            }
        }
    }

    deltas
}

fn derive_diffusion_one_hop_paths(
    brain: &BrainGraph,
    node_deltas: &HashMap<ReasoningScope, Decimal>,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for edge in brain.graph.edge_references() {
        match edge.weight() {
            EdgeKind::StockToStock(rel) => {
                let (NodeKind::Stock(source), NodeKind::Stock(target)) =
                    (&brain.graph[edge.source()], &brain.graph[edge.target()])
                else {
                    continue;
                };
                if rel.similarity <= Decimal::ZERO {
                    continue;
                }
                push_diffusion_path(
                    &mut paths,
                    &ReasoningScope::Symbol(source.symbol.clone()),
                    &ReasoningScope::Symbol(target.symbol.clone()),
                    "stock similarity diffusion",
                    rel.similarity.abs(),
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
            }
            EdgeKind::StockToSector(rel) => {
                let (NodeKind::Stock(source), NodeKind::Sector(target)) =
                    (&brain.graph[edge.source()], &brain.graph[edge.target()])
                else {
                    continue;
                };
                let source_scope = ReasoningScope::Symbol(source.symbol.clone());
                let target_scope = ReasoningScope::Sector(target.sector_id.clone());
                push_diffusion_path(
                    &mut paths,
                    &source_scope,
                    &target_scope,
                    "sector diffusion",
                    rel.weight.abs(),
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
                push_diffusion_path(
                    &mut paths,
                    &target_scope,
                    &source_scope,
                    "sector diffusion",
                    rel.weight.abs(),
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
            }
            EdgeKind::InstitutionToStock(rel) => {
                let (NodeKind::Institution(source), NodeKind::Stock(target)) =
                    (&brain.graph[edge.source()], &brain.graph[edge.target()])
                else {
                    continue;
                };
                let edge_weight = Decimal::from(rel.seat_count.min(12) as i64) / Decimal::from(12);
                let source_scope = ReasoningScope::Institution(source.institution_id);
                let target_scope = ReasoningScope::Symbol(target.symbol.clone());
                push_diffusion_path(
                    &mut paths,
                    &source_scope,
                    &target_scope,
                    "institution diffusion",
                    edge_weight,
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
                push_diffusion_path(
                    &mut paths,
                    &target_scope,
                    &source_scope,
                    "institution diffusion",
                    edge_weight,
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
            }
            EdgeKind::InstitutionToInstitution(rel) => {
                let (NodeKind::Institution(source), NodeKind::Institution(target)) =
                    (&brain.graph[edge.source()], &brain.graph[edge.target()])
                else {
                    continue;
                };
                push_diffusion_path(
                    &mut paths,
                    &ReasoningScope::Institution(source.institution_id),
                    &ReasoningScope::Institution(target.institution_id),
                    "institution affinity diffusion",
                    rel.jaccard.abs(),
                    node_deltas,
                    &rel.provenance.inputs,
                    observed_at,
                );
            }
        }
    }

    paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    paths.dedup_by(|a, b| a.path_id == b.path_id);
    paths
}

fn push_diffusion_path(
    paths: &mut Vec<PropagationPath>,
    from: &ReasoningScope,
    to: &ReasoningScope,
    mechanism: &str,
    edge_weight: Decimal,
    node_deltas: &HashMap<ReasoningScope, Decimal>,
    references: &[String],
    observed_at: OffsetDateTime,
) {
    if from == to || edge_weight <= Decimal::ZERO {
        return;
    }
    let Some(source_delta) = node_deltas.get(from).copied() else {
        return;
    };
    if source_delta.abs() < diffusion_source_threshold() {
        return;
    }
    let target_delta = node_deltas.get(to).copied().unwrap_or(Decimal::ZERO);
    let lag_factor = diffusion_lag_factor(source_delta, target_delta);
    let confidence =
        (source_delta.abs().min(Decimal::ONE) * edge_weight.abs().min(Decimal::ONE) * lag_factor)
            .round_dp(4)
            .clamp(Decimal::ZERO, Decimal::ONE);
    if confidence < diffusion_min_confidence() {
        return;
    }

    let path_id = format!(
        "path:diffusion:{}:{}:{}",
        mechanism_slug(mechanism),
        scope_node_id(from),
        scope_node_id(to),
    );
    let mut step_references = references.to_vec();
    step_references.push(format!(
        "diffusion_source:{}={}",
        scope_node_id(from),
        source_delta.round_dp(4)
    ));
    step_references.push(format!(
        "diffusion_target:{}={}",
        scope_node_id(to),
        target_delta.round_dp(4)
    ));
    step_references.push(format!("observed_at:{}", observed_at));

    paths.push(PropagationPath {
        path_id,
        summary: format!(
            "{} may diffuse into {} via {}",
            scope_title(from),
            scope_title(to),
            mechanism
        ),
        confidence,
        steps: vec![PropagationStep {
            from: from.clone(),
            to: to.clone(),
            mechanism: mechanism.into(),
            confidence,
            references: step_references,
        }],
    });
}

fn diffusion_lag_factor(source_delta: Decimal, target_delta: Decimal) -> Decimal {
    let source_magnitude = source_delta.abs().min(Decimal::ONE);
    if source_magnitude <= Decimal::ZERO {
        return Decimal::ZERO;
    }

    let absorbed =
        if target_delta != Decimal::ZERO && source_delta.signum() == target_delta.signum() {
            (target_delta.abs() / source_magnitude).clamp(Decimal::ZERO, Decimal::ONE)
        } else {
            Decimal::ZERO
        };
    let opposite_direction_bonus =
        if target_delta != Decimal::ZERO && source_delta.signum() != target_delta.signum() {
            Decimal::new(15, 2)
        } else {
            Decimal::ZERO
        };

    (Decimal::ONE - absorbed + opposite_direction_bonus).clamp(Decimal::new(15, 2), Decimal::ONE)
}

fn mechanism_slug(mechanism: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in mechanism.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('_');
            last_was_sep = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn rotation_one_hop_paths(
    insights: &crate::graph::insights::GraphInsights,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    insights
        .rotations
        .iter()
        .take(10)
        .map(|rotation| {
            let confidence = rotation.spread.abs().min(Decimal::ONE);
            PropagationPath {
                path_id: format!(
                    "path:rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                ),
                summary: format!(
                    "rotation pressure may propagate from {} to {}",
                    rotation.from_sector, rotation.to_sector
                ),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::Sector(rotation.from_sector.clone().into()),
                    to: ReasoningScope::Sector(rotation.to_sector.clone().into()),
                    mechanism: if rotation.widening {
                        "capital rotation widening".into()
                    } else {
                        "capital rotation narrowing".into()
                    },
                    confidence,
                    references: vec![
                        format!("rotation:{}:{}", rotation.from_sector, rotation.to_sector),
                        format!("observed_at:{}", observed_at),
                    ],
                }],
            }
        })
        .collect()
}

fn shared_holder_one_hop_paths(
    insights: &crate::graph::insights::GraphInsights,
) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for shared in insights.shared_holders.iter().take(10) {
        let confidence = shared.jaccard.min(Decimal::ONE);
        for (from, to) in [
            (&shared.symbol_a, &shared.symbol_b),
            (&shared.symbol_b, &shared.symbol_a),
        ] {
            paths.push(PropagationPath {
                path_id: format!("path:shared_holder:{}:{}", from, to),
                summary: format!(
                    "shared-holder overlap may transmit repricing between {} and {}",
                    from, to
                ),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::Symbol(from.clone()),
                    to: ReasoningScope::Symbol(to.clone()),
                    mechanism: "shared holder overlap".into(),
                    confidence,
                    references: vec![
                        format!("shared_holder:{}", from),
                        format!("shared_holder:{}", to),
                    ],
                }],
            });
        }
    }

    paths
}

fn shared_holder_bridge_paths(
    insights: &crate::graph::insights::GraphInsights,
) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for shared in insights.shared_holders.iter().take(10) {
        let confidence = shared.jaccard.min(Decimal::ONE);
        let Some(sector_a) = shared.sector_a.as_ref() else {
            continue;
        };
        let Some(sector_b) = shared.sector_b.as_ref() else {
            continue;
        };

        let bridges = [
            (
                ReasoningScope::Sector(sector_a.0.as_str().into()),
                ReasoningScope::Symbol(shared.symbol_b.clone()),
                format!(
                    "shared-holder sector spillover from {} into {}",
                    sector_a, shared.symbol_b
                ),
                format!("path:bridge:sector_symbol:{}:{}", sector_a, shared.symbol_b),
                "shared-holder sector spillover",
            ),
            (
                ReasoningScope::Sector(sector_b.0.as_str().into()),
                ReasoningScope::Symbol(shared.symbol_a.clone()),
                format!(
                    "shared-holder sector spillover from {} into {}",
                    sector_b, shared.symbol_a
                ),
                format!("path:bridge:sector_symbol:{}:{}", sector_b, shared.symbol_a),
                "shared-holder sector spillover",
            ),
            (
                ReasoningScope::Symbol(shared.symbol_a.clone()),
                ReasoningScope::Sector(sector_b.0.as_str().into()),
                format!(
                    "peer stock {} may spill into sector {}",
                    shared.symbol_a, sector_b
                ),
                format!("path:bridge:symbol_sector:{}:{}", shared.symbol_a, sector_b),
                "peer sector spillover",
            ),
            (
                ReasoningScope::Symbol(shared.symbol_b.clone()),
                ReasoningScope::Sector(sector_a.0.as_str().into()),
                format!(
                    "peer stock {} may spill into sector {}",
                    shared.symbol_b, sector_a
                ),
                format!("path:bridge:symbol_sector:{}:{}", shared.symbol_b, sector_a),
                "peer sector spillover",
            ),
        ];

        for (from, to, summary, path_id, mechanism) in bridges {
            paths.push(PropagationPath {
                path_id,
                summary,
                confidence,
                steps: vec![PropagationStep {
                    from,
                    to,
                    mechanism: mechanism.into(),
                    confidence,
                    references: vec![
                        format!("shared_holder:{}", shared.symbol_a),
                        format!("shared_holder:{}", shared.symbol_b),
                    ],
                }],
            });
        }
    }

    paths
}

fn market_stress_sector_paths(
    insights: &crate::graph::insights::GraphInsights,
) -> Vec<PropagationPath> {
    let stress = insights.stress.composite_stress.min(Decimal::ONE);
    if stress <= Decimal::ZERO {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for rotation in insights.rotations.iter().take(8) {
        for sector in [&rotation.from_sector, &rotation.to_sector] {
            if !seen.insert(sector.to_string()) {
                continue;
            }
            let confidence =
                ((stress + rotation.spread.abs().min(Decimal::ONE)) / Decimal::from(2)).round_dp(4);
            paths.push(PropagationPath {
                path_id: format!("path:market_stress:{}", sector),
                summary: format!("market stress may concentrate into sector {}", sector),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::market(),
                    to: ReasoningScope::Sector(sector.0.as_str().into()),
                    mechanism: "market stress concentration".into(),
                    confidence,
                    references: vec![
                        format!(
                            "market_stress:{}",
                            insights.stress.composite_stress.round_dp(4)
                        ),
                        format!("rotation_sector:{}", sector),
                    ],
                }],
            });
        }
    }
    paths
}

fn derive_two_hop_paths(
    one_hop_paths: &[PropagationPath],
    hop_decay: Decimal,
) -> Vec<PropagationPath> {
    derive_extended_paths(one_hop_paths, one_hop_paths, hop_decay, 2)
}

fn derive_three_hop_paths(
    two_hop_paths: &[PropagationPath],
    one_hop_paths: &[PropagationPath],
    hop_decay: Decimal,
) -> Vec<PropagationPath> {
    derive_extended_paths(two_hop_paths, one_hop_paths, hop_decay, 3)
}

fn derive_extended_paths(
    seed_paths: &[PropagationPath],
    extension_paths: &[PropagationPath],
    hop_decay: Decimal,
    total_hops: usize,
) -> Vec<PropagationPath> {
    let mut derived = Vec::new();

    for left in seed_paths {
        let Some(left_tail) = left.steps.last() else {
            continue;
        };
        for right in extension_paths {
            let Some(right_head) = right.steps.first() else {
                continue;
            };
            if left.path_id == right.path_id || left_tail.to != right_head.from {
                continue;
            }
            if path_contains_scope(left, &right_head.to) {
                continue;
            }

            let confidence = (left.confidence * right.confidence * hop_decay).round_dp(4);
            if confidence <= Decimal::ZERO {
                continue;
            }

            let mut steps = left.steps.clone();
            steps.extend(right.steps.clone());
            let summary = format!(
                "{} -> {} via {}",
                scope_title(&left.steps[0].from),
                scope_title(&steps.last().expect("extended path tail").to),
                steps
                    .iter()
                    .map(|step| step.mechanism.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> "),
            );
            let path_id = format!("path:{}hop:{}=>{}", total_hops, left.path_id, right.path_id);
            let mut references = left
                .steps
                .iter()
                .flat_map(|step| step.references.clone())
                .collect::<Vec<_>>();
            references.extend(right_head.references.clone());
            if let Some(last_step) = steps.last_mut() {
                last_step.references.extend(references);
            }

            derived.push(PropagationPath {
                path_id,
                summary,
                confidence,
                steps,
            });
        }
    }

    derived.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    derived.dedup_by(|a, b| a.path_id == b.path_id);
    derived
}

pub fn mechanism_family(mechanism: &str) -> &'static str {
    if mechanism.contains("stock similarity diffusion") {
        "stock_diffusion"
    } else if mechanism.contains("sector diffusion") {
        "sector_diffusion"
    } else if mechanism.contains("institution affinity diffusion") {
        "institution_affinity"
    } else if mechanism.contains("institution diffusion") {
        "institution_diffusion"
    } else if mechanism.contains("shared holder") || mechanism.contains("shared-holder") {
        "shared_holder"
    } else if mechanism.contains("rotation") {
        "rotation"
    } else if mechanism.contains("market stress") {
        "market_stress"
    } else if mechanism.contains("sector spillover") {
        "sector_symbol_bridge"
    } else {
        "other"
    }
}

fn mechanism_is_symmetric(mechanism: &str) -> bool {
    matches!(
        mechanism_family(mechanism),
        "shared_holder" | "sector_symbol_bridge" | "stock_diffusion" | "institution_affinity"
    )
}

pub fn path_has_family(path: &PropagationPath, family: &str) -> bool {
    path.steps
        .iter()
        .any(|step| mechanism_family(&step.mechanism) == family)
}

pub fn path_is_mixed_multi_hop(path: &PropagationPath) -> bool {
    if path.steps.len() < 2 {
        return false;
    }
    let families = path
        .steps
        .iter()
        .map(|step| mechanism_family(&step.mechanism))
        .collect::<HashSet<_>>();
    families.len() > 1
}

pub fn path_contains_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}

pub fn hop_penalty(hops: usize) -> Decimal {
    match hops {
        0 | 1 => Decimal::ONE,
        2 => Decimal::new(80, 2),
        3 => Decimal::new(60, 2),
        _ => Decimal::new(50, 2),
    }
}

pub(super) fn canonicalize_paths(paths: Vec<PropagationPath>) -> Vec<PropagationPath> {
    let mut ranked = paths;
    ranked.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.path_id.cmp(&b.path_id))
    });

    let mut seen = HashSet::new();
    let mut canonical = Vec::new();
    for path in ranked {
        let key = canonical_path_key(&path);
        if seen.insert(key) {
            canonical.push(path);
        }
    }
    canonical
}

fn canonical_path_key(path: &PropagationPath) -> String {
    let forward = path_directional_signature(path);
    if path
        .steps
        .iter()
        .all(|step| mechanism_is_symmetric(&step.mechanism))
    {
        let reverse = path_reverse_signature(path);
        if reverse < forward {
            reverse
        } else {
            forward
        }
    } else {
        forward
    }
}

fn path_directional_signature(path: &PropagationPath) -> String {
    path.steps
        .iter()
        .map(|step| {
            format!(
                "{}:{}:{}",
                mechanism_family(&step.mechanism),
                scope_id(&step.from),
                scope_id(&step.to)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn path_reverse_signature(path: &PropagationPath) -> String {
    path.steps
        .iter()
        .rev()
        .map(|step| {
            format!(
                "{}:{}:{}",
                mechanism_family(&step.mechanism),
                scope_id(&step.to),
                scope_id(&step.from)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => sector.to_string(),
        ReasoningScope::Institution(institution) => institution.to_string(),
        ReasoningScope::Theme(theme) => theme.to_string(),
        ReasoningScope::Region(region) => region.to_string(),
        ReasoningScope::Custom(value) => value.to_string(),
    }
}

pub fn scope_title(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "Market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => format!("Sector {}", sector),
        ReasoningScope::Institution(institution) => format!("Institution {}", institution),
        ReasoningScope::Theme(theme) => format!("Theme {}", theme),
        ReasoningScope::Region(region) => format!("Region {}", region),
        ReasoningScope::Custom(value) => value.to_string(),
    }
}
