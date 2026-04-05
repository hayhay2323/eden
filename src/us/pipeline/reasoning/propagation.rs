use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{PropagationPath, PropagationStep, ReasoningScope};
use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::graph::propagation::CrossMarketSignal;

use super::{scope_id, scope_label, UsStructuralRankMetrics};

fn diffusion_source_threshold() -> Decimal {
    Decimal::new(2, 2)
}

fn diffusion_min_confidence() -> Decimal {
    Decimal::new(2, 3)
}

pub(super) fn derive_diffusion_propagation_paths(
    graph: &UsGraph,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
    cross_market_signals: &[CrossMarketSignal],
) -> Vec<PropagationPath> {
    let node_deltas = derive_diffusion_node_deltas(graph, structural_metrics);
    let mut paths = derive_diffusion_one_hop_paths(graph, &node_deltas);
    paths.extend(derive_cross_market_diffusion_paths(
        cross_market_signals,
        &node_deltas,
    ));
    let two_hop = derive_diffusion_extended_paths(&paths, &paths, Decimal::new(85, 2), 2);
    let three_hop = derive_diffusion_extended_paths(&two_hop, &paths, Decimal::new(70, 2), 3);
    paths.extend(two_hop.into_iter().take(24));
    paths.extend(three_hop.into_iter().take(16));
    paths.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| left.path_id.cmp(&right.path_id))
    });
    paths.dedup_by(|left, right| left.path_id == right.path_id);
    paths
}

fn derive_diffusion_node_deltas(
    graph: &UsGraph,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
) -> HashMap<ReasoningScope, Decimal> {
    let Some(structural_metrics) = structural_metrics else {
        return HashMap::new();
    };

    let mut deltas = structural_metrics
        .iter()
        .filter_map(|(symbol, metrics)| {
            (metrics.composite_delta.abs() >= diffusion_source_threshold()).then_some((
                ReasoningScope::Symbol(symbol.clone()),
                metrics.composite_delta,
            ))
        })
        .collect::<HashMap<_, _>>();

    for (sector_id, &sector_idx) in &graph.sector_nodes {
        let mut total = Decimal::ZERO;
        let mut count = 0i64;
        for edge in graph
            .graph
            .edges_directed(sector_idx, GraphDirection::Incoming)
        {
            let UsEdgeKind::StockToSector(_) = edge.weight() else {
                continue;
            };
            let UsNodeKind::Stock(stock) = &graph.graph[edge.source()] else {
                continue;
            };
            let Some(metrics) = structural_metrics.get(&stock.symbol) else {
                continue;
            };
            if metrics.composite_delta.abs() < diffusion_source_threshold() {
                continue;
            }
            total += metrics.composite_delta;
            count += 1;
        }
        if count > 0 {
            let sector_delta = total / Decimal::from(count);
            if sector_delta.abs() >= diffusion_source_threshold() {
                deltas.insert(
                    ReasoningScope::Sector(sector_id.clone()),
                    sector_delta.round_dp(4),
                );
            }
        }
    }

    deltas
}

fn derive_diffusion_one_hop_paths(
    graph: &UsGraph,
    node_deltas: &HashMap<ReasoningScope, Decimal>,
) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for edge in graph.graph.edge_references() {
        match edge.weight() {
            UsEdgeKind::StockToStock(rel) => {
                let (UsNodeKind::Stock(source), UsNodeKind::Stock(target)) =
                    (&graph.graph[edge.source()], &graph.graph[edge.target()])
                else {
                    continue;
                };
                push_diffusion_path(
                    &mut paths,
                    ReasoningScope::Symbol(source.symbol.clone()),
                    ReasoningScope::Symbol(target.symbol.clone()),
                    "stock diffusion",
                    rel.similarity.abs(),
                    node_deltas,
                    vec![
                        format!("stock_similarity:{}", source.symbol),
                        format!("stock_similarity:{}", target.symbol),
                    ],
                );
            }
            UsEdgeKind::StockToSector(_) => {
                let (UsNodeKind::Stock(source), UsNodeKind::Sector(target)) =
                    (&graph.graph[edge.source()], &graph.graph[edge.target()])
                else {
                    continue;
                };
                push_diffusion_path(
                    &mut paths,
                    ReasoningScope::Symbol(source.symbol.clone()),
                    ReasoningScope::Sector(target.sector_id.clone()),
                    "sector diffusion",
                    Decimal::new(55, 2),
                    node_deltas,
                    vec![format!("sector_membership:{}", source.symbol)],
                );
                push_diffusion_path(
                    &mut paths,
                    ReasoningScope::Sector(target.sector_id.clone()),
                    ReasoningScope::Symbol(source.symbol.clone()),
                    "sector diffusion",
                    Decimal::new(55, 2),
                    node_deltas,
                    vec![format!("sector_membership:{}", source.symbol)],
                );
            }
            UsEdgeKind::CrossMarket(_) => {}
        }
    }

    paths
}

fn derive_cross_market_diffusion_paths(
    cross_market_signals: &[CrossMarketSignal],
    node_deltas: &HashMap<ReasoningScope, Decimal>,
) -> Vec<PropagationPath> {
    let mut paths = Vec::new();
    for signal in cross_market_signals {
        if signal.propagation_confidence.abs() < diffusion_min_confidence() {
            continue;
        }
        let target_scope = ReasoningScope::Symbol(signal.us_symbol.clone());
        let target_delta = node_deltas
            .get(&target_scope)
            .copied()
            .unwrap_or(Decimal::ZERO);
        let confidence = (signal.propagation_confidence.abs().min(Decimal::ONE)
            * diffusion_lag_factor(signal.propagation_confidence, target_delta))
        .round_dp(4)
        .clamp(Decimal::ZERO, Decimal::ONE);
        if confidence < diffusion_min_confidence() {
            continue;
        }
        paths.push(PropagationPath {
            path_id: format!(
                "path:diffusion:{}:{}:{}",
                mechanism_slug("cross-market diffusion"),
                signal.hk_symbol,
                signal.us_symbol
            ),
            summary: format!(
                "{} may diffuse into {} via cross-market linkage",
                signal.hk_symbol, signal.us_symbol
            ),
            confidence,
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(signal.hk_symbol.clone()),
                to: target_scope,
                mechanism: "cross-market diffusion".into(),
                confidence,
                references: vec![
                    format!("hk_symbol:{}", signal.hk_symbol),
                    format!("us_symbol:{}", signal.us_symbol),
                ],
            }],
        });
    }
    paths
}

fn push_diffusion_path(
    paths: &mut Vec<PropagationPath>,
    from: ReasoningScope,
    to: ReasoningScope,
    mechanism: &str,
    edge_weight: Decimal,
    node_deltas: &HashMap<ReasoningScope, Decimal>,
    references: Vec<String>,
) {
    if from == to || edge_weight <= Decimal::ZERO {
        return;
    }
    let Some(source_delta) = node_deltas.get(&from).copied() else {
        return;
    };
    let target_delta = node_deltas.get(&to).copied().unwrap_or(Decimal::ZERO);
    let confidence = (source_delta.abs().min(Decimal::ONE)
        * edge_weight.min(Decimal::ONE)
        * diffusion_lag_factor(source_delta, target_delta))
    .round_dp(4)
    .clamp(Decimal::ZERO, Decimal::ONE);
    if confidence < diffusion_min_confidence() {
        return;
    }

    let path_id = format!(
        "path:diffusion:{}:{}:{}",
        mechanism_slug(mechanism),
        scope_id(&from),
        scope_id(&to)
    );
    paths.push(PropagationPath {
        path_id,
        summary: format!(
            "{} may diffuse into {} via {}",
            scope_label(&from),
            scope_label(&to),
            mechanism
        ),
        confidence,
        steps: vec![PropagationStep {
            from,
            to,
            mechanism: mechanism.into(),
            confidence,
            references,
        }],
    });
}

fn derive_diffusion_extended_paths(
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
            if diffusion_path_contains_scope(left, &right_head.to) {
                continue;
            }

            let confidence = (left.confidence * right.confidence * hop_decay)
                .round_dp(4)
                .clamp(Decimal::ZERO, Decimal::ONE);
            if confidence < diffusion_min_confidence() {
                continue;
            }

            let mut steps = left.steps.clone();
            steps.extend(right.steps.clone());
            derived.push(PropagationPath {
                path_id: format!("path:{}hop:{}=>{}", total_hops, left.path_id, right.path_id),
                summary: format!(
                    "{} -> {} via {}",
                    scope_label(&left.steps[0].from),
                    scope_label(&steps.last().expect("diffusion path tail").to),
                    steps
                        .iter()
                        .map(|step| step.mechanism.as_str())
                        .collect::<Vec<_>>()
                        .join(" -> "),
                ),
                confidence,
                steps,
            });
        }
    }

    derived
}

fn diffusion_path_contains_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
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

    (Decimal::ONE - absorbed).clamp(Decimal::new(15, 2), Decimal::ONE)
}

fn mechanism_slug(mechanism: &str) -> String {
    mechanism
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .replace("__", "_")
}
