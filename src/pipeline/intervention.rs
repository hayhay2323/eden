//! Intervention-effect estimation over a causal graph.
//!
//! Eden's KG (BrainGraph for HK, UsGraph for US) has typed edges that
//! approximate causal direction: institution → stock (holdings drive
//! price), stock → sector (member → aggregate), stock → stock (cross-
//! stock propagation). This module answers the operator question:
//!
//!   **"If X moves in direction D, which other nodes does the graph
//!    expect to move, and by how much?"**
//!
//! Honest framing: this is NOT full Pearl do-calculus. Full do-calculus
//! requires a known Structural Causal Model with explicit confounders;
//! Eden has a correlation graph with rough causal hints. What we compute
//! is a **forward propagation intervention estimate** — BFS from the
//! intervened node, accumulating weighted effect along causal edges,
//! attenuated per hop. It's the `P(Y | do(X))` shape without the full
//! back-door adjustment; sufficient for operator-surface "掌握權限"
//! queries but not for publishable causal claims.
//!
//! Wiring path: Eden's existing graph crates implement `CausalGraphView`
//! via thin adapters (later task). This module only needs the trait.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

/// Minimum absolute effect that survives pruning. Propagation stops
/// branch-wise when the attenuated effect falls below this threshold.
/// Keeps the BFS bounded even on dense graphs.
const EFFECT_PRUNE_THRESHOLD: f64 = 1.0e-3;

/// Abstract read-only view of a causal graph. Each edge carries a signed
/// weight in `[-1, 1]`: positive means "source direction propagates as
/// same direction on target", negative means anti-direction (short
/// pair, opposing institution flow, etc).
///
/// Implementations must be deterministic — same query returns same edges.
pub trait CausalGraphView {
    type Node: Clone + Eq + Hash;

    /// Directed causal successors of `from`. Returns `(neighbor,
    /// signed_weight)`. Weight magnitude should reflect edge strength
    /// (e.g. correlation, holding concentration); sign encodes same /
    /// opposing direction.
    fn outgoing_causal_edges(&self, from: &Self::Node) -> Vec<(Self::Node, f64)>;
}

/// A single propagated intervention effect on a downstream node.
#[derive(Debug, Clone)]
pub struct InterventionEffect<N> {
    pub target: N,
    /// Signed effect in the same units as the intervention. Positive =
    /// same direction as intervention, negative = opposing.
    pub expected_effect: f64,
    /// Hops away from the intervened node. Closer = more trustworthy.
    pub hops_away: usize,
    /// Attenuation factor applied — effectively `confidence` in the edge
    /// chain that produced this effect. In `[0, 1]`, higher = stronger.
    pub attenuation: f64,
}

/// Propagate an intervention forward through the graph.
///
/// `direction`: signed intensity of the intervention (+1 = "buy strongly",
/// -1 = "sell strongly"; smaller magnitudes = mild intervention).
///
/// `max_hops`: cut the BFS beyond this depth. In practice 2–3 hops is
/// plenty for Eden's KG — beyond that, effect is below noise.
///
/// `attenuation_per_hop`: multiplicative decay per BFS level, in `[0, 1]`.
/// Typical: 0.7. Lower = effect decays faster with distance.
///
/// Returns effects sorted by descending absolute expected_effect. The
/// intervened node itself is not in the result set.
pub fn propagate_intervention<G: CausalGraphView>(
    graph: &G,
    intervene_on: &G::Node,
    direction: f64,
    max_hops: usize,
    attenuation_per_hop: f64,
) -> Vec<InterventionEffect<G::Node>> {
    let mut effects: HashMap<G::Node, InterventionEffect<G::Node>> = HashMap::new();
    let mut visited: HashSet<G::Node> = HashSet::new();
    visited.insert(intervene_on.clone());

    // BFS frontier: (node, accumulated_signed_effect, hops, attenuation)
    let mut frontier: VecDeque<(G::Node, f64, usize, f64)> = VecDeque::new();
    frontier.push_back((intervene_on.clone(), direction, 0, 1.0));

    while let Some((node, inbound_effect, hops, inbound_att)) = frontier.pop_front() {
        if hops >= max_hops {
            continue;
        }
        let next_hops = hops + 1;
        let next_att = inbound_att * attenuation_per_hop.clamp(0.0, 1.0);
        for (neighbor, edge_weight) in graph.outgoing_causal_edges(&node) {
            if visited.contains(&neighbor) {
                continue;
            }
            let propagated = inbound_effect * edge_weight * attenuation_per_hop.clamp(0.0, 1.0);
            if propagated.abs() < EFFECT_PRUNE_THRESHOLD {
                continue;
            }
            let merged = match effects.get(&neighbor) {
                Some(existing) => InterventionEffect {
                    target: neighbor.clone(),
                    expected_effect: existing.expected_effect + propagated,
                    hops_away: existing.hops_away.min(next_hops),
                    attenuation: existing.attenuation.max(next_att),
                },
                None => InterventionEffect {
                    target: neighbor.clone(),
                    expected_effect: propagated,
                    hops_away: next_hops,
                    attenuation: next_att,
                },
            };
            effects.insert(neighbor.clone(), merged);
            visited.insert(neighbor.clone());
            frontier.push_back((neighbor, propagated, next_hops, next_att));
        }
    }

    let mut ranked: Vec<InterventionEffect<G::Node>> = effects.into_values().collect();
    ranked.sort_by(|a, b| {
        b.expected_effect
            .abs()
            .partial_cmp(&a.expected_effect.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked
}

/// Compare two candidate interventions by which produces a larger total
/// absolute effect across the graph. Operator use: "should I be watching
/// JPM or Morgan Stanley — which one, if they moved, would shake more of
/// my universe?"
///
/// Returns `Ordering::Greater` if `a` moves more total mass than `b`.
pub fn compare_intervention_breadth<G: CausalGraphView>(
    graph: &G,
    a: &G::Node,
    b: &G::Node,
    direction: f64,
    max_hops: usize,
    attenuation_per_hop: f64,
) -> std::cmp::Ordering {
    fn total_mass<G: CausalGraphView>(
        graph: &G,
        from: &G::Node,
        direction: f64,
        max_hops: usize,
        att: f64,
    ) -> f64 {
        propagate_intervention(graph, from, direction, max_hops, att)
            .iter()
            .map(|e| e.expected_effect.abs())
            .sum()
    }
    let mass_a = total_mass(graph, a, direction, max_hops, attenuation_per_hop);
    let mass_b = total_mass(graph, b, direction, max_hops, attenuation_per_hop);
    mass_a
        .partial_cmp(&mass_b)
        .unwrap_or(std::cmp::Ordering::Equal)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny fixture graph for deterministic testing.
    struct TestGraph {
        edges: HashMap<&'static str, Vec<(&'static str, f64)>>,
    }

    impl CausalGraphView for TestGraph {
        type Node = &'static str;
        fn outgoing_causal_edges(&self, from: &Self::Node) -> Vec<(Self::Node, f64)> {
            self.edges.get(from).cloned().unwrap_or_default()
        }
    }

    fn build_graph() -> TestGraph {
        // A → B (strong positive)
        // A → C (weak positive)
        // B → D (positive)
        // C → E (strong positive)
        // B → F (negative — inverse relationship)
        let mut edges: HashMap<&'static str, Vec<(&'static str, f64)>> = HashMap::new();
        edges.insert("A", vec![("B", 0.9), ("C", 0.3)]);
        edges.insert("B", vec![("D", 0.8), ("F", -0.7)]);
        edges.insert("C", vec![("E", 0.95)]);
        edges.insert("D", vec![]);
        edges.insert("E", vec![]);
        edges.insert("F", vec![]);
        TestGraph { edges }
    }

    #[test]
    fn intervention_propagates_with_expected_attenuation() {
        let g = build_graph();
        let effects = propagate_intervention(&g, &"A", 1.0, 3, 0.7);
        let effect_map: HashMap<&'static str, f64> = effects
            .iter()
            .map(|e| (e.target, e.expected_effect))
            .collect();
        // A → B: 1.0 × 0.9 × 0.7 = 0.63
        assert!(
            (effect_map["B"] - 0.63).abs() < 1e-9,
            "B effect = {}",
            effect_map["B"]
        );
        // A → C: 1.0 × 0.3 × 0.7 = 0.21
        assert!(
            (effect_map["C"] - 0.21).abs() < 1e-9,
            "C effect = {}",
            effect_map["C"]
        );
        // A → B → D: 0.63 × 0.8 × 0.7 = 0.3528
        assert!(
            (effect_map["D"] - 0.3528).abs() < 1e-9,
            "D effect = {}",
            effect_map["D"]
        );
    }

    #[test]
    fn negative_edge_flips_downstream_direction() {
        let g = build_graph();
        let effects = propagate_intervention(&g, &"A", 1.0, 3, 0.7);
        let f = effects.iter().find(|e| e.target == "F").unwrap();
        // A → B → F: 0.63 × (-0.7) × 0.7 = -0.3087
        assert!(
            f.expected_effect < 0.0,
            "F should have negative effect (anti-edge), got {}",
            f.expected_effect
        );
    }

    #[test]
    fn max_hops_bounds_propagation() {
        let g = build_graph();
        let one_hop = propagate_intervention(&g, &"A", 1.0, 1, 0.7);
        let reached: HashSet<&'static str> = one_hop.iter().map(|e| e.target).collect();
        assert!(reached.contains("B"), "B should be reached in 1 hop");
        assert!(reached.contains("C"), "C should be reached in 1 hop");
        assert!(
            !reached.contains("D"),
            "D should NOT be reached in 1 hop (requires 2)"
        );
    }

    #[test]
    fn pruning_drops_near_zero_effect() {
        let g = build_graph();
        // Intervention tiny enough that 3-hop propagation falls below threshold.
        let effects = propagate_intervention(&g, &"A", 0.005, 3, 0.5);
        for e in &effects {
            assert!(
                e.expected_effect.abs() >= EFFECT_PRUNE_THRESHOLD - 1e-9,
                "effect {} on {} should not have survived pruning",
                e.expected_effect,
                e.target
            );
        }
    }

    #[test]
    fn ranking_sorts_by_absolute_effect() {
        let g = build_graph();
        let effects = propagate_intervention(&g, &"A", 1.0, 3, 0.7);
        let abs_values: Vec<f64> = effects.iter().map(|e| e.expected_effect.abs()).collect();
        for pair in abs_values.windows(2) {
            assert!(
                pair[0] >= pair[1],
                "sort invariant violated: {} vs {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn compare_breadth_prefers_node_with_larger_cascade() {
        let g = build_graph();
        // A has downstream cascade (B, C, D, E, F); E is leaf with no children.
        let ordering = compare_intervention_breadth(&g, &"A", &"E", 1.0, 3, 0.7);
        assert_eq!(
            ordering,
            std::cmp::Ordering::Greater,
            "A has larger cascade than E"
        );
    }
}
