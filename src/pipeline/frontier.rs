use std::collections::HashSet;

use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::pipeline::symbol_sub_kg::{EdgeKind, NodeFreshness, NodeId, NodeKind, SubKgRegistry};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphFrontier {
    pub tick: u64,
    pub symbols: Vec<FrontierSymbol>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierSymbol {
    pub symbol: String,
    pub nodes: Vec<FrontierNode>,
    pub edges: Vec<FrontierEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierNode {
    pub id: String,
    pub kind: NodeKind,
    pub value: Option<f64>,
    pub aux: Option<f64>,
    pub last_seen_tick: u64,
    pub freshness: NodeFreshness,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPropagationPlan {
    pub tick: u64,
    pub hops: Vec<FrontierPropagationHop>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPropagationHop {
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: f64,
    pub source_kind: NodeKind,
    pub source_value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPropagationCandidate {
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: f64,
    pub source_kind: NodeKind,
    pub source_value: Option<f64>,
    pub influence: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPropagationDryRun {
    pub tick: u64,
    pub updates: Vec<FrontierDryRunUpdate>,
    pub mean_abs_delta: Option<f64>,
    pub max_abs_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierDryRunUpdate {
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub proposed_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPressureCandidateCache {
    pub tick: u64,
    pub updates: Vec<FrontierPressureCandidateUpdate>,
    pub mean_abs_delta: Option<f64>,
    pub max_abs_delta: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPressureCandidateUpdate {
    pub symbol: String,
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub proposed_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPressureConvergenceGate {
    pub tick: u64,
    pub noise_floor: Option<f64>,
    pub passed: Vec<FrontierPressureCandidateUpdate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierNextProposal {
    pub tick: u64,
    pub entries: Vec<FrontierNextProposalEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierNextProposalEntry {
    pub symbol: String,
    pub node_id: String,
    pub source_node_id: String,
    pub source_edge_kind: EdgeKind,
    pub proposed_value: f64,
    pub proposed_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierBoundedPropagationSummary {
    pub tick: u64,
    pub requested_rounds: usize,
    pub rounds: Vec<FrontierPropagationRoundSummary>,
    pub final_proposals: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontierPropagationRoundSummary {
    pub round: usize,
    pub input_proposals: usize,
    pub produced_hops: usize,
    pub produced_proposals: usize,
    pub produced: Vec<FrontierNextProposalEntry>,
}

impl FrontierNextProposal {
    pub fn from_pressure_gate(gate: &FrontierPressureConvergenceGate) -> Self {
        let mut entries: Vec<FrontierNextProposalEntry> = gate
            .passed
            .iter()
            .filter(|update| update.proposed_delta.is_finite())
            .map(|update| FrontierNextProposalEntry {
                symbol: update.symbol.clone(),
                node_id: update.to.clone(),
                source_node_id: update.from.clone(),
                source_edge_kind: update.kind,
                proposed_value: update.proposed_delta,
                proposed_delta: update.proposed_delta,
            })
            .collect();
        entries.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.source_node_id.cmp(&b.source_node_id))
        });
        Self {
            tick: gate.tick,
            entries,
        }
    }
}

impl FrontierPressureConvergenceGate {
    pub fn from_cache(cache: &FrontierPressureCandidateCache) -> Self {
        let passed = match cache.mean_abs_delta {
            Some(noise_floor) if cache.updates.len() > 1 && noise_floor.is_finite() => cache
                .updates
                .iter()
                .filter(|update| update.proposed_delta.abs() > noise_floor)
                .cloned()
                .collect(),
            _ => Vec::new(),
        };

        Self {
            tick: cache.tick,
            noise_floor: cache.mean_abs_delta,
            passed,
        }
    }
}

impl FrontierPressureCandidateCache {
    pub fn from_dry_run(dry_run: &FrontierPropagationDryRun) -> Self {
        let mut updates: Vec<FrontierPressureCandidateUpdate> = dry_run
            .updates
            .iter()
            .filter(|update| {
                is_pressure_candidate_edge(update.kind) && is_pressure_target(&update.to)
            })
            .map(|update| FrontierPressureCandidateUpdate {
                symbol: update.symbol.clone(),
                from: update.from.clone(),
                to: update.to.clone(),
                kind: update.kind,
                proposed_delta: update.proposed_delta,
            })
            .collect();
        updates.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.from.cmp(&b.from))
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
        });

        let mean_abs_delta = if updates.is_empty() {
            None
        } else {
            Some(
                updates
                    .iter()
                    .map(|update| update.proposed_delta.abs())
                    .sum::<f64>()
                    / updates.len() as f64,
            )
        };
        let max_abs_delta = updates
            .iter()
            .map(|update| update.proposed_delta.abs())
            .max_by(|a, b| a.total_cmp(b));

        Self {
            tick: dry_run.tick,
            updates,
            mean_abs_delta,
            max_abs_delta,
        }
    }
}

impl FrontierPropagationDryRun {
    pub fn from_candidates(tick: u64, candidates: &[FrontierPropagationCandidate]) -> Self {
        let mut updates: Vec<FrontierDryRunUpdate> = candidates
            .iter()
            .filter_map(|candidate| {
                let proposed_delta = candidate.influence?;
                if !proposed_delta.is_finite() {
                    return None;
                }
                Some(FrontierDryRunUpdate {
                    symbol: candidate.symbol.clone(),
                    from: candidate.from.clone(),
                    to: candidate.to.clone(),
                    kind: candidate.kind,
                    proposed_delta,
                })
            })
            .collect();
        updates.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.from.cmp(&b.from))
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
        });

        let mean_abs_delta = if updates.is_empty() {
            None
        } else {
            Some(
                updates
                    .iter()
                    .map(|update| update.proposed_delta.abs())
                    .sum::<f64>()
                    / updates.len() as f64,
            )
        };
        let max_abs_delta = updates
            .iter()
            .map(|update| update.proposed_delta.abs())
            .max_by(|a, b| a.total_cmp(b));

        Self {
            tick,
            updates,
            mean_abs_delta,
            max_abs_delta,
        }
    }
}

impl FrontierPropagationPlan {
    pub fn propagation_candidates(&self) -> Vec<FrontierPropagationCandidate> {
        let mut candidates: Vec<FrontierPropagationCandidate> = self
            .hops
            .iter()
            .filter(|hop| is_directional_propagation_edge(hop.kind))
            .map(|hop| FrontierPropagationCandidate {
                symbol: hop.symbol.clone(),
                from: hop.from.clone(),
                to: hop.to.clone(),
                kind: hop.kind,
                weight: hop.weight,
                source_kind: hop.source_kind,
                source_value: hop.source_value,
                influence: hop.source_value.map(|value| value * hop.weight),
            })
            .collect();
        candidates.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.from.cmp(&b.from))
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
        });
        candidates
    }
}

fn is_directional_propagation_edge(kind: EdgeKind) -> bool {
    matches!(
        kind,
        EdgeKind::Contributes
            | EdgeKind::FlowToPressure
            | EdgeKind::Evidence
            | EdgeKind::IntentToState
    )
}

fn is_pressure_candidate_edge(kind: EdgeKind) -> bool {
    matches!(kind, EdgeKind::Contributes | EdgeKind::FlowToPressure)
}

fn is_pressure_target(node_id: &str) -> bool {
    node_id.starts_with("Pressure")
}

impl GraphFrontier {
    pub fn from_subkg_registry(tick: u64, registry: &SubKgRegistry) -> Self {
        let mut symbols: Vec<FrontierSymbol> = registry
            .graphs
            .iter()
            .filter_map(|(symbol, kg)| {
                let fresh_ids: HashSet<NodeId> = kg
                    .nodes
                    .iter()
                    .filter(|(_, activation)| {
                        activation.last_seen_tick == tick
                            && activation.freshness == NodeFreshness::Fresh
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                if fresh_ids.is_empty() {
                    return None;
                }

                let mut nodes: Vec<FrontierNode> = fresh_ids
                    .iter()
                    .filter_map(|id| {
                        let activation = kg.nodes.get(id)?;
                        Some(FrontierNode {
                            id: id.to_serde_key(),
                            kind: activation.kind,
                            value: activation.value.and_then(|value| value.to_f64()),
                            aux: activation.aux.and_then(|value| value.to_f64()),
                            last_seen_tick: activation.last_seen_tick,
                            freshness: activation.freshness,
                        })
                    })
                    .collect();
                nodes.sort_by(|a, b| a.id.cmp(&b.id));

                let mut edges: Vec<FrontierEdge> = kg
                    .edges
                    .iter()
                    .map(|edge| FrontierEdge {
                        from: edge.from.to_serde_key(),
                        to: edge.to.to_serde_key(),
                        kind: edge.kind,
                        weight: edge.weight.to_f64().unwrap_or(0.0),
                    })
                    .collect();
                edges.sort_by(|a, b| {
                    a.from
                        .cmp(&b.from)
                        .then_with(|| a.to.cmp(&b.to))
                        .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
                });

                Some(FrontierSymbol {
                    symbol: symbol.clone(),
                    nodes,
                    edges,
                })
            })
            .collect();
        symbols.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        Self { tick, symbols }
    }

    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    pub fn node_count(&self) -> usize {
        self.symbols.iter().map(|symbol| symbol.nodes.len()).sum()
    }

    pub fn edge_count(&self) -> usize {
        self.symbols.iter().map(|symbol| symbol.edges.len()).sum()
    }

    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.symbols.iter().any(|entry| entry.symbol == symbol)
    }

    pub fn nodes_for(&self, symbol: &str) -> Option<&[FrontierNode]> {
        self.symbols
            .iter()
            .find(|entry| entry.symbol == symbol)
            .map(|entry| entry.nodes.as_slice())
    }

    pub fn edges_for(&self, symbol: &str) -> Option<&[FrontierEdge]> {
        self.symbols
            .iter()
            .find(|entry| entry.symbol == symbol)
            .map(|entry| entry.edges.as_slice())
    }

    pub fn local_propagation_plan(&self) -> FrontierPropagationPlan {
        let mut hops = Vec::new();
        for symbol in &self.symbols {
            for node in &symbol.nodes {
                for edge in symbol.edges.iter().filter(|edge| edge.from == node.id) {
                    hops.push(FrontierPropagationHop {
                        symbol: symbol.symbol.clone(),
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        kind: edge.kind,
                        weight: edge.weight,
                        source_kind: node.kind,
                        source_value: node.value,
                    });
                }
            }
        }
        hops.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.from.cmp(&b.from))
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| format!("{:?}", a.kind).cmp(&format!("{:?}", b.kind)))
        });
        FrontierPropagationPlan {
            tick: self.tick,
            hops,
        }
    }

    pub fn bounded_propagation_summary(
        &self,
        seed: &FrontierNextProposal,
        max_rounds: usize,
    ) -> FrontierBoundedPropagationSummary {
        let mut current = seed.entries.clone();
        let mut rounds = Vec::new();

        for round in 1..=max_rounds {
            if current.is_empty() {
                break;
            }

            let produced = self.expand_proposals_once(&current);
            rounds.push(FrontierPropagationRoundSummary {
                round,
                input_proposals: current.len(),
                produced_hops: produced.len(),
                produced_proposals: produced.len(),
                produced: produced.clone(),
            });
            current = produced;
        }

        FrontierBoundedPropagationSummary {
            tick: self.tick,
            requested_rounds: max_rounds,
            final_proposals: current.len(),
            rounds,
        }
    }

    fn expand_proposals_once(
        &self,
        proposals: &[FrontierNextProposalEntry],
    ) -> Vec<FrontierNextProposalEntry> {
        let mut produced = Vec::new();
        for proposal in proposals {
            let Some(symbol) = self
                .symbols
                .iter()
                .find(|entry| entry.symbol == proposal.symbol)
            else {
                continue;
            };
            for edge in symbol.edges.iter().filter(|edge| {
                edge.from == proposal.node_id && is_directional_propagation_edge(edge.kind)
            }) {
                let proposed_delta = proposal.proposed_delta * edge.weight;
                if !proposed_delta.is_finite() {
                    continue;
                }
                produced.push(FrontierNextProposalEntry {
                    symbol: proposal.symbol.clone(),
                    node_id: edge.to.clone(),
                    source_node_id: edge.from.clone(),
                    source_edge_kind: edge.kind,
                    proposed_value: proposed_delta,
                    proposed_delta,
                });
            }
        }
        produced.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.node_id.cmp(&b.node_id))
                .then_with(|| a.source_node_id.cmp(&b.source_node_id))
                .then_with(|| {
                    format!("{:?}", a.source_edge_kind).cmp(&format!("{:?}", b.source_edge_kind))
                })
        });
        produced
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rust_decimal_macros::dec;

    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};

    use super::*;

    #[test]
    fn frontier_extracts_only_fresh_nodes_for_the_current_tick() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();

        let fresh = registry.upsert("FRESH.US", now);
        fresh.set_tick(7, now);
        fresh.set_node_value(NodeId::LastPrice, dec!(11.0), now);

        let unchanged = registry.upsert("UNCHANGED.US", now);
        unchanged.set_tick(6, now);
        unchanged.set_node_value(NodeId::LastPrice, dec!(10.0), now);
        unchanged.set_tick(7, now);
        unchanged.set_node_value(NodeId::LastPrice, dec!(10.0), now);

        let stale = registry.upsert("STALE.US", now);
        stale.set_tick(6, now);
        stale.set_node_value(NodeId::LastPrice, dec!(9.0), now);

        let frontier = GraphFrontier::from_subkg_registry(7, &registry);

        assert_eq!(frontier.tick, 7);
        assert_eq!(frontier.symbol_count(), 1);
        assert_eq!(frontier.node_count(), 1);
        assert!(frontier.contains_symbol("FRESH.US"));
        assert!(!frontier.contains_symbol("UNCHANGED.US"));
        assert!(!frontier.contains_symbol("STALE.US"));

        let nodes = frontier.nodes_for("FRESH.US").expect("fresh symbol");
        assert_eq!(nodes[0].id, "LastPrice");
        assert_eq!(nodes[0].last_seen_tick, 7);
        assert!(!frontier.edges_for("FRESH.US").unwrap().is_empty());
    }

    #[test]
    fn local_propagation_plan_follows_outgoing_edges_from_fresh_nodes_only() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();
        let kg = registry.upsert("FLOW.US", now);
        kg.set_tick(11, now);
        kg.set_node_value(NodeId::Turnover, dec!(1000000), now);

        let frontier = GraphFrontier::from_subkg_registry(11, &registry);
        let plan = frontier.local_propagation_plan();

        assert_eq!(plan.tick, 11);
        assert!(plan.hops.iter().any(|hop| hop.symbol == "FLOW.US"
            && hop.from == "Turnover"
            && hop.to == "PressureCapitalFlow"
            && hop.kind == EdgeKind::Contributes));
        assert!(!plan
            .hops
            .iter()
            .any(|hop| hop.from == "Symbol" && hop.to == "Turnover"));
    }

    #[test]
    fn propagation_candidates_keep_only_directional_semantic_edges() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();
        let kg = registry.upsert("FLOW.US", now);
        kg.set_tick(12, now);
        kg.set_node_value(NodeId::Turnover, dec!(1000000), now);
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.8), now);

        let frontier = GraphFrontier::from_subkg_registry(12, &registry);
        let plan = frontier.local_propagation_plan();
        let candidates = plan.propagation_candidates();

        assert!(candidates.iter().any(|candidate| {
            candidate.symbol == "FLOW.US"
                && candidate.from == "Turnover"
                && candidate.to == "PressureCapitalFlow"
                && candidate.kind == EdgeKind::Contributes
                && candidate.source_value == Some(1000000.0)
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.symbol == "FLOW.US"
                && candidate.from == "PressureCapitalFlow"
                && candidate.to == "IntentAccumulation"
                && candidate.kind == EdgeKind::Evidence
        }));
        assert!(!candidates
            .iter()
            .any(|candidate| candidate.kind == EdgeKind::Membership));
    }

    #[test]
    fn dry_run_updates_compute_target_deltas_without_mutating_graph() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();
        let kg = registry.upsert("FLOW.US", now);
        kg.set_tick(13, now);
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.8), now);

        let frontier = GraphFrontier::from_subkg_registry(13, &registry);
        let plan = frontier.local_propagation_plan();
        let candidates = plan.propagation_candidates();
        let dry_run = FrontierPropagationDryRun::from_candidates(13, &candidates);

        assert_eq!(dry_run.tick, 13);
        assert!(dry_run.updates.iter().any(|update| {
            update.symbol == "FLOW.US"
                && update.from == "PressureCapitalFlow"
                && update.to == "IntentAccumulation"
                && update.kind == EdgeKind::Evidence
                && (update.proposed_delta - 0.8).abs() < 1e-9
        }));
        assert!(dry_run.max_abs_delta.unwrap() >= 0.8);
        assert!(dry_run.mean_abs_delta.unwrap() > 0.0);
        assert_eq!(
            registry
                .get("FLOW.US")
                .unwrap()
                .nodes
                .get(&NodeId::IntentAccumulation)
                .unwrap()
                .value,
            None,
            "dry-run must not mutate target nodes"
        );
    }

    #[test]
    fn pressure_candidate_cache_keeps_only_pressure_target_updates() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();
        let kg = registry.upsert("FLOW.US", now);
        kg.set_tick(14, now);
        kg.set_node_value(NodeId::Turnover, dec!(100.0), now);
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.8), now);

        let frontier = GraphFrontier::from_subkg_registry(14, &registry);
        let plan = frontier.local_propagation_plan();
        let candidates = plan.propagation_candidates();
        let dry_run = FrontierPropagationDryRun::from_candidates(14, &candidates);
        let cache = FrontierPressureCandidateCache::from_dry_run(&dry_run);

        assert_eq!(cache.tick, 14);
        assert!(cache.updates.iter().any(|update| {
            update.symbol == "FLOW.US"
                && update.from == "Turnover"
                && update.to == "PressureCapitalFlow"
                && update.kind == EdgeKind::Contributes
                && (update.proposed_delta - 100.0).abs() < 1e-9
        }));
        assert!(!cache
            .updates
            .iter()
            .any(|update| update.to == "IntentAccumulation"));
        assert_eq!(
            registry
                .get("FLOW.US")
                .unwrap()
                .nodes
                .get(&NodeId::PressureCapitalFlow)
                .unwrap()
                .value,
            Some(dec!(0.8)),
            "cache must not overwrite existing pressure node value"
        );
    }

    #[test]
    fn pressure_convergence_gate_passes_only_above_self_distribution_noise() {
        let cache = FrontierPressureCandidateCache {
            tick: 15,
            updates: vec![
                FrontierPressureCandidateUpdate {
                    symbol: "LOW.US".to_string(),
                    from: "Turnover".to_string(),
                    to: "PressureCapitalFlow".to_string(),
                    kind: EdgeKind::Contributes,
                    proposed_delta: 0.1,
                },
                FrontierPressureCandidateUpdate {
                    symbol: "HIGH.US".to_string(),
                    from: "Turnover".to_string(),
                    to: "PressureCapitalFlow".to_string(),
                    kind: EdgeKind::Contributes,
                    proposed_delta: 1.0,
                },
            ],
            mean_abs_delta: Some(0.55),
            max_abs_delta: Some(1.0),
        };

        let gate = FrontierPressureConvergenceGate::from_cache(&cache);

        assert_eq!(gate.tick, 15);
        assert_eq!(gate.noise_floor, Some(0.55));
        assert_eq!(gate.passed.len(), 1);
        assert_eq!(gate.passed[0].symbol, "HIGH.US");
        assert!((gate.passed[0].proposed_delta - 1.0).abs() < 1e-9);
    }

    #[test]
    fn next_frontier_proposal_promotes_passed_pressure_targets_without_mutation() {
        let gate = FrontierPressureConvergenceGate {
            tick: 16,
            noise_floor: Some(0.5),
            passed: vec![FrontierPressureCandidateUpdate {
                symbol: "FLOW.US".to_string(),
                from: "Turnover".to_string(),
                to: "PressureCapitalFlow".to_string(),
                kind: EdgeKind::Contributes,
                proposed_delta: 1.2,
            }],
        };

        let proposal = FrontierNextProposal::from_pressure_gate(&gate);

        assert_eq!(proposal.tick, 16);
        assert_eq!(proposal.entries.len(), 1);
        assert_eq!(proposal.entries[0].symbol, "FLOW.US");
        assert_eq!(proposal.entries[0].node_id, "PressureCapitalFlow");
        assert_eq!(proposal.entries[0].source_node_id, "Turnover");
        assert_eq!(proposal.entries[0].proposed_value, 1.2);
        assert_eq!(proposal.entries[0].proposed_delta, 1.2);
    }

    #[test]
    fn bounded_propagation_summary_expands_proposals_along_local_adjacency() {
        let now = Utc::now();
        let mut registry = SubKgRegistry::default();
        let kg = registry.upsert("FLOW.US", now);
        kg.set_tick(17, now);
        kg.set_node_value(NodeId::Turnover, dec!(100.0), now);
        let frontier = GraphFrontier::from_subkg_registry(17, &registry);
        let seed = FrontierNextProposal {
            tick: 17,
            entries: vec![FrontierNextProposalEntry {
                symbol: "FLOW.US".to_string(),
                node_id: "PressureCapitalFlow".to_string(),
                source_node_id: "Turnover".to_string(),
                source_edge_kind: EdgeKind::Contributes,
                proposed_value: 1.0,
                proposed_delta: 1.0,
            }],
        };

        let summary = frontier.bounded_propagation_summary(&seed, 2);

        assert_eq!(summary.tick, 17);
        assert_eq!(summary.rounds.len(), 2);
        assert_eq!(summary.rounds[0].input_proposals, 1);
        assert!(summary.rounds[0].produced_proposals >= 1);
        assert!(summary.rounds[0]
            .produced
            .iter()
            .any(|entry| entry.node_id == "IntentAccumulation"
                && entry.source_node_id == "PressureCapitalFlow"));
        assert!(summary.rounds[1]
            .produced
            .iter()
            .any(|entry| entry.node_id == "StateClassification"
                && entry.source_node_id == "IntentAccumulation"));
        assert!(summary.final_proposals >= 1);
    }
}
