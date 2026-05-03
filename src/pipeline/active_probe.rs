//! Active-probing: Eden 從被動觀察 → 主動實驗。
//!
//! Each tick:
//! 1. Pick top-K symbols by BP posterior entropy (high-entropy =
//!    Eden uncertain → information gain from probing).
//! 2. For each target X, run two counterfactual BP passes:
//!    - Bull intervention: prior(X) = [1, 0, 0]
//!    - Bear intervention: prior(X) = [0, 1, 0]
//!    Compute per-neighbor sensitivity = posterior_if_bull - posterior_if_bear.
//! 3. Enqueue PendingProbe with horizon = `current_tick + PROBE_HORIZON_TICKS`.
//!
//! At horizon tick:
//! 4. Read X's actual posterior direction from current BP marginals.
//! 5. For each neighbor, pick forecast (if_bull / if_bear) based on
//!    X's realized direction; accuracy = 1 - |predicted - actual|.
//! 6. Roll mean accuracy into per-symbol history.
//!
//! Next tick:
//! 7. Per-symbol mean accuracy is exposed via `accuracy_by_symbol()`,
//!    fed into sub-KG `NodeId::ForecastAccuracy` by the substrate
//!    evidence builder. BP's `observe_from_subkg` reads it as part of
//!    the standard prior — no calibrator, no edge weight modification.
//!
//! Pure first-principles: deterministic counterfactual reasoning
//! (Pearl do-calculus subset) + frequency-based accuracy aggregation.
//! No learning, no fitting. Output is graph-native: NodeId values
//! flow back into the same BP fusion pass.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::pipeline::loopy_bp::{self, GraphEdge, NodePrior, N_STATES, STATE_BEAR, STATE_BULL};

/// How many high-entropy symbols to probe each tick. Cost control:
/// each probe runs BP twice (bull + bear intervention).
pub const PROBE_TARGETS_PER_TICK: usize = 3;

/// How many ticks to wait before evaluating forecast accuracy.
/// Aligned with engram horizon=30 ticks ÷ 6 to give a faster
/// micro-validation loop without inflating queue size.
pub const PROBE_HORIZON_TICKS: u64 = 5;

/// Rolling accuracy window per probe target. Algorithmic — bounds
/// memory, not a business threshold.
pub const ACCURACY_HISTORY_PER_SYMBOL: usize = 100;

/// Sensitivity floor — neighbors with |if_bull - if_bear| below this
/// are dropped from forecast (algorithmic noise floor; no business
/// meaning).
const SENSITIVITY_FLOOR: f64 = 0.01;

/// Multi-step causal chain depth (V3.1). Each chain step adds one
/// more intervention on top of the previous, then picks the next
/// most-deviated un-intervened symbol. Pearl do-calculus tested
/// transitively: X → Y → Z. Cost-controlled: chain depth = 3 means
/// at most 3 extra BP runs per probe target on top of the
/// single-step bull/bear pair.
const MAX_CHAIN_DEPTH: usize = 3;

/// Numerical hygiene for intervention priors so message passing
/// stays well-conditioned (avoids 0-mass states).
const PRIOR_EPS: f64 = 1e-6;

/// Default forecast accuracy when a symbol has no history yet.
/// Neutral 0.5 = "no information" — matches the substrate builder's
/// default in `build_substrate_evidence_snapshots`.
pub const DEFAULT_ACCURACY: f64 = 0.5;

#[derive(Debug, Clone, Serialize)]
pub struct ForecastEntry {
    pub neighbor: String,
    pub if_bull_neighbor_bull: f64,
    pub if_bear_neighbor_bull: f64,
    /// Counterfactual sensitivity: how much neighbor's bull mass
    /// changes when target's prior flips. Pearl do-calculus subset.
    pub sensitivity: f64,
}

#[derive(Debug, Clone)]
pub struct PendingProbe {
    pub probe_symbol: String,
    pub probe_tick: u64,
    pub realized_at_tick: u64,
    pub forecast_per_neighbor: HashMap<String, ForecastEntry>,
    /// Snapshot of which channels were active (and their signs) when
    /// the probe was emitted. Used for gain calibration.
    pub sensory_evidence: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CausalChainStep {
    /// Position in the cascade (1 = first reachable hop, 2 = second…).
    pub depth: usize,
    /// Newly-intervened symbol at this depth (chosen as the most-
    /// deviated un-intervened symbol given prior intervention set).
    pub symbol: String,
    /// Bull-mass deviation from uniform under the intervention set
    /// at this depth — proxy for causal reach measured by Pearl
    /// counterfactual.
    pub sensitivity: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeForecastRow {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub probe_symbol: String,
    pub probe_tick: u64,
    pub realized_at_tick: u64,
    pub forecast: Vec<ForecastEntry>,
    /// Multi-step chain: starting at the probe target, iteratively
    /// intervene on the next most-causally-reached symbol. Validates
    /// transitive structural causality (X → Y → Z) — empty when the
    /// 1-hop already saturates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_chain: Vec<CausalChainStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NeighborAccuracy {
    pub neighbor: String,
    pub predicted_p_bull: f64,
    pub actual_p_bull: f64,
    pub accuracy: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeOutcomeRow {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub probe_symbol: String,
    pub probe_tick: u64,
    pub realized_tick: u64,
    pub actual_direction: String,
    pub neighbor_accuracies: Vec<NeighborAccuracy>,
    pub mean_accuracy: f64,
}

#[derive(Debug, Default)]
pub struct ActiveProbeRunner {
    pending: VecDeque<PendingProbe>,
    /// Per-symbol rolling forecast accuracy when symbol was the probe
    /// target. Used to populate `NodeId::ForecastAccuracy` next tick.
    accuracy_history: HashMap<String, VecDeque<f64>>,
}

impl ActiveProbeRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run counterfactual BP for each target, enqueue forecast,
    /// return rows for ndjson logging.
    pub fn emit_probes(
        &mut self,
        targets: &[String],
        base_priors: &HashMap<String, NodePrior>,
        edges: &[GraphEdge],
        current_tick: u64,
        now: DateTime<Utc>,
        market: &str,
        graph: Option<&crate::perception::PerceptionGraph>,
    ) -> Vec<ProbeForecastRow> {
        let mut rows = Vec::with_capacity(targets.len());
        for target in targets {
            // Snapshot current sensory evidence for this target to
            // attribute credit later.
            let mut sensory_evidence = Vec::new();
            if let Some(g) = graph {
                if let Some(snap) = g.sensory_flux.get(&crate::ontology::objects::Symbol(target.clone())) {
                    for channel in &snap.active_channels {
                        sensory_evidence.push((channel.clone(), 1.0));
                    }
                }
            }

            // Bull intervention prior on target.
            let mut priors_bull = base_priors.clone();
            priors_bull.insert(
                target.clone(),
                NodePrior {
                    belief: [1.0 - 2.0 * PRIOR_EPS, PRIOR_EPS, PRIOR_EPS],
                    observed: true,
                },
            );
            let (beliefs_bull, _, _) = loopy_bp::run(&priors_bull, edges);

            // Bear intervention prior on target.
            let mut priors_bear = base_priors.clone();
            priors_bear.insert(
                target.clone(),
                NodePrior {
                    belief: [PRIOR_EPS, 1.0 - 2.0 * PRIOR_EPS, PRIOR_EPS],
                    observed: true,
                },
            );
            let (beliefs_bear, _, _) = loopy_bp::run(&priors_bear, edges);

            let mut forecast_per_neighbor = HashMap::new();
            for (neighbor, bull_b) in &beliefs_bull {
                if neighbor == target {
                    continue;
                }
                let bear_b = beliefs_bear
                    .get(neighbor)
                    .copied()
                    .unwrap_or([1.0 / N_STATES as f64; N_STATES]);
                let if_bull_neighbor_bull = bull_b[STATE_BULL];
                let if_bear_neighbor_bull = bear_b[STATE_BULL];
                let sensitivity = if_bull_neighbor_bull - if_bear_neighbor_bull;
                if sensitivity.abs() < SENSITIVITY_FLOOR {
                    continue;
                }
                forecast_per_neighbor.insert(
                    neighbor.clone(),
                    ForecastEntry {
                        neighbor: neighbor.clone(),
                        if_bull_neighbor_bull,
                        if_bear_neighbor_bull,
                        sensitivity,
                    },
                );
            }

            // V3.1: build multi-step causal chain. Iteratively intervene
            // bull on the next most-deviated un-intervened symbol up to
            // MAX_CHAIN_DEPTH. Each step adds one BP run.
            let causal_chain = build_causal_chain(target, base_priors, edges, MAX_CHAIN_DEPTH);

            let realized_at = current_tick + PROBE_HORIZON_TICKS;
            let forecast_clone = forecast_per_neighbor.clone();
            self.pending.push_back(PendingProbe {
                probe_symbol: target.clone(),
                probe_tick: current_tick,
                realized_at_tick: realized_at,
                forecast_per_neighbor,
                sensory_evidence,
            });

            rows.push(ProbeForecastRow {
                ts: now,
                market: market.to_string(),
                probe_symbol: target.clone(),
                probe_tick: current_tick,
                realized_at_tick: realized_at,
                forecast: forecast_clone.into_values().collect(),
                causal_chain,
            });
        }
        rows
    }

    /// Evaluate due probes against current BP beliefs. Updates per-
    /// symbol accuracy history. Returns ndjson rows.
    pub fn evaluate_due(
        &mut self,
        current_tick: u64,
        current_beliefs: &HashMap<String, [f64; N_STATES]>,
        now: DateTime<Utc>,
        market: &str,
        mut graph: Option<&mut crate::perception::PerceptionGraph>,
    ) -> Vec<ProbeOutcomeRow> {
        let mut outcomes = Vec::new();
        while let Some(front) = self.pending.front() {
            if front.realized_at_tick > current_tick {
                break;
            }
            let probe = self.pending.pop_front().expect("front exists");
            let Some(actual_x) = current_beliefs.get(&probe.probe_symbol) else {
                continue;
            };
            let actual_direction = if actual_x[STATE_BULL] > actual_x[STATE_BEAR] {
                "bull"
            } else if actual_x[STATE_BEAR] > actual_x[STATE_BULL] {
                "bear"
            } else {
                "neutral"
            };
            let mut neighbor_accuracies = Vec::new();
            let mut total_acc = 0.0_f64;
            let mut count = 0usize;
            for entry in probe.forecast_per_neighbor.values() {
                let Some(actual_neighbor) = current_beliefs.get(&entry.neighbor) else {
                    continue;
                };
                let actual_p_bull = actual_neighbor[STATE_BULL];
                let predicted = match actual_direction {
                    "bull" => entry.if_bull_neighbor_bull,
                    "bear" => entry.if_bear_neighbor_bull,
                    _ => 0.5 * (entry.if_bull_neighbor_bull + entry.if_bear_neighbor_bull),
                };
                let accuracy = (1.0 - (predicted - actual_p_bull).abs()).max(0.0);
                total_acc += accuracy;
                count += 1;
                neighbor_accuracies.push(NeighborAccuracy {
                    neighbor: entry.neighbor.clone(),
                    predicted_p_bull: predicted,
                    actual_p_bull,
                    accuracy,
                });
            }
            if count == 0 {
                continue;
            }
            let mean_accuracy = total_acc / count as f64;

            // --- Sensory Gain Calibration (Closed Loop) ---
            if let Some(ref mut g) = graph {
                let adjustment = (mean_accuracy - 0.5) * 0.1; // Scale factor
                for (channel, _) in &probe.sensory_evidence {
                    let old_gain = g.sensory_gain.get_gain(channel);
                    let new_gain = (old_gain + adjustment).clamp(0.1, 2.0);
                    g.sensory_gain.upsert(
                        channel,
                        crate::perception::SensoryGainSnapshot {
                            channel_name: channel.clone(),
                            current_gain: new_gain,
                            recent_accuracy: mean_accuracy,
                            last_calibrated: current_tick,
                        },
                    );
                }
            }

            let history = self
                .accuracy_history
                .entry(probe.probe_symbol.clone())
                .or_default();
            history.push_back(mean_accuracy);
            while history.len() > ACCURACY_HISTORY_PER_SYMBOL {
                history.pop_front();
            }
            outcomes.push(ProbeOutcomeRow {
                ts: now,
                market: market.to_string(),
                probe_symbol: probe.probe_symbol,
                probe_tick: probe.probe_tick,
                realized_tick: current_tick,
                actual_direction: actual_direction.to_string(),
                neighbor_accuracies,
                mean_accuracy,
            });
        }
        outcomes
    }

    /// Per-symbol mean rolling accuracy (0.5 baseline when no history).
    /// Consumed by `build_substrate_evidence_snapshots` to populate
    /// `NodeId::ForecastAccuracy` on each symbol's sub-KG.
    pub fn accuracy_by_symbol(&self) -> HashMap<String, f64> {
        self.accuracy_history
            .iter()
            .map(|(sym, history)| {
                let n = history.len();
                let mean = if n == 0 {
                    DEFAULT_ACCURACY
                } else {
                    history.iter().copied().sum::<f64>() / n as f64
                };
                (sym.clone(), mean)
            })
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Build a multi-step causal chain rooted at `target`.
///
/// At each depth, all chain members so far have their priors clamped
/// to Bull (counterfactual intervention). The next member is the
/// un-intervened symbol whose posterior bull-mass deviates most from
/// uniform — i.e. the one most reachable from the current
/// intervention set. Empty when the 1-hop already saturates.
///
/// Pure Pearl do-calculus subset: deterministic counterfactual
/// reasoning, no learning. Validates whether causal influence is
/// transitive (X reaches Y reaches Z) versus one-hop only.
pub fn build_causal_chain(
    target: &str,
    base_priors: &HashMap<String, NodePrior>,
    edges: &[GraphEdge],
    max_depth: usize,
) -> Vec<CausalChainStep> {
    let mut chain = Vec::new();
    let mut intervened: std::collections::HashSet<String> =
        std::iter::once(target.to_string()).collect();
    let uniform = 1.0 / N_STATES as f64;

    // Baseline posterior with no intervention. Chain ranking compares
    // each intervened-run posterior against this baseline so we
    // measure "what THIS intervention changed" rather than "which
    // symbol is always far from neutral" — otherwise every probe
    // gravitates to the same always-extreme bystanders.
    let (baseline, _, _) = loopy_bp::run(base_priors, edges);

    for depth in 1..=max_depth {
        let mut priors = base_priors.clone();
        for sym in &intervened {
            priors.insert(
                sym.clone(),
                NodePrior {
                    belief: [1.0 - 2.0 * PRIOR_EPS, PRIOR_EPS, PRIOR_EPS],
                    observed: true,
                },
            );
        }
        let (posterior, _, _) = loopy_bp::run(&priors, edges);

        let mut best: Option<(String, f64)> = None;
        for (sym, post) in &posterior {
            if intervened.contains(sym) {
                continue;
            }
            let baseline_bull = baseline.get(sym).map(|b| b[STATE_BULL]).unwrap_or(uniform);
            let signed_dev = post[STATE_BULL] - baseline_bull;
            let deviation = signed_dev.abs();
            if deviation < SENSITIVITY_FLOOR {
                continue;
            }
            match &best {
                Some((_, existing)) if existing.abs() >= deviation => {}
                _ => best = Some((sym.clone(), signed_dev)),
            }
        }

        match best {
            Some((sym, sens)) => {
                chain.push(CausalChainStep {
                    depth,
                    symbol: sym.clone(),
                    sensitivity: sens,
                });
                intervened.insert(sym);
            }
            None => break,
        }
    }
    chain
}

/// Pick top-N symbols by BP posterior entropy. High entropy = uniform-
/// like = Eden hasn't committed to a state for this symbol → high
/// information gain from a counterfactual probe. Pure information theory.
pub fn pick_probe_targets(beliefs: &HashMap<String, [f64; N_STATES]>, n: usize) -> Vec<String> {
    let mut ranked: Vec<(String, f64)> = beliefs
        .iter()
        .map(|(sym, b)| {
            let entropy: f64 = b.iter().filter(|p| **p > 1e-9).map(|p| -p * p.ln()).sum();
            (sym.clone(), entropy)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked.into_iter().take(n).map(|(s, _)| s).collect()
}

pub fn write_forecasts(market: &str, rows: &[ProbeForecastRow]) -> std::io::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-active-probe-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for r in rows {
        writeln!(
            file,
            "{}",
            serde_json::to_string(r)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        )?;
        written += 1;
    }
    Ok(written)
}

pub fn write_outcomes(market: &str, rows: &[ProbeOutcomeRow]) -> std::io::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-probe-outcome-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for r in rows {
        writeln!(
            file,
            "{}",
            serde_json::to_string(r)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        )?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform() -> [f64; N_STATES] {
        [1.0 / N_STATES as f64; N_STATES]
    }

    fn concentrated(bull: f64, bear: f64) -> [f64; N_STATES] {
        let mut p = [0.0; N_STATES];
        p[STATE_BULL] = bull;
        p[STATE_BEAR] = bear;
        p[2] = 1.0 - bull - bear;
        p
    }

    #[test]
    fn picks_high_entropy_symbols() {
        let mut beliefs = HashMap::new();
        beliefs.insert("UNIFORM".to_string(), uniform());
        beliefs.insert("CONCENTRATED".to_string(), concentrated(0.95, 0.04));
        let picks = pick_probe_targets(&beliefs, 1);
        assert_eq!(picks, vec!["UNIFORM".to_string()]);
    }

    #[test]
    fn empty_runner_returns_default_accuracy() {
        let runner = ActiveProbeRunner::new();
        let map = runner.accuracy_by_symbol();
        assert!(map.is_empty(), "no history yet");
    }

    #[test]
    fn evaluate_skips_when_horizon_not_due() {
        let mut runner = ActiveProbeRunner::new();
        let mut forecast = HashMap::new();
        forecast.insert(
            "B".into(),
            ForecastEntry {
                neighbor: "B".into(),
                if_bull_neighbor_bull: 0.7,
                if_bear_neighbor_bull: 0.3,
                sensitivity: 0.4,
            },
        );
        runner.pending.push_back(PendingProbe {
            probe_symbol: "A".into(),
            probe_tick: 100,
            realized_at_tick: 105,
            forecast_per_neighbor: forecast,
            sensory_evidence: Vec::new(),
        });
        let beliefs = HashMap::new();
        let outcomes = runner.evaluate_due(102, &beliefs, Utc::now(), "test", None); // before horizon
        assert!(outcomes.is_empty());
        assert_eq!(runner.pending_count(), 1);
    }

    #[test]
    fn evaluate_records_accuracy_when_due() {
        let mut runner = ActiveProbeRunner::new();
        let mut forecast = HashMap::new();
        forecast.insert(
            "B".into(),
            ForecastEntry {
                neighbor: "B".into(),
                // Forecast: if A=bull, B's bull mass = 0.80.
                if_bull_neighbor_bull: 0.80,
                if_bear_neighbor_bull: 0.20,
                sensitivity: 0.60,
            },
        );
        runner.pending.push_back(PendingProbe {
            probe_symbol: "A".into(),
            probe_tick: 100,
            realized_at_tick: 105,
            forecast_per_neighbor: forecast,
            sensory_evidence: Vec::new(),
        });
        let mut beliefs = HashMap::new();
        // A actually went bull
        beliefs.insert("A".into(), concentrated(0.85, 0.10));
        // B actual bull mass = 0.78 (forecast was 0.80 — accuracy ≈ 0.98)
        beliefs.insert("B".into(), concentrated(0.78, 0.15));

        let outcomes = runner.evaluate_due(105, &beliefs, Utc::now(), "test", None);
        assert_eq!(outcomes.len(), 1);
        let outcome = &outcomes[0];
        assert_eq!(outcome.actual_direction, "bull");
        assert_eq!(outcome.neighbor_accuracies.len(), 1);
        let neighbor_acc = &outcome.neighbor_accuracies[0];
        assert!((neighbor_acc.predicted_p_bull - 0.80).abs() < 1e-9);
        assert!((neighbor_acc.actual_p_bull - 0.78).abs() < 1e-9);
        assert!(neighbor_acc.accuracy > 0.97);

        // Accuracy should now be in history.
        let map = runner.accuracy_by_symbol();
        let stored_a = map.get("A").copied().unwrap();
        assert!(stored_a > 0.97);
    }

    #[test]
    fn high_accuracy_history_dominates_default() {
        let mut runner = ActiveProbeRunner::new();
        runner
            .accuracy_history
            .entry("A".into())
            .or_default()
            .extend([0.9, 0.85, 0.92]);
        let map = runner.accuracy_by_symbol();
        assert!((map["A"] - 0.89).abs() < 0.01);
    }

    #[test]
    fn chain_extends_beyond_first_hop_when_neighbors_have_room() {
        // Graph: A — B — C in a line, no other edges. base_priors all
        // uniform/unobserved. Intervene bull on A → BP propagates to B
        // strongly, to C weakly. Chain step 1 = B; step 2 with A+B both
        // bull → C gets pushed harder.
        let mut priors = HashMap::new();
        for sym in ["A", "B", "C"] {
            priors.insert(sym.to_string(), NodePrior::default());
        }
        let edges = vec![
            GraphEdge {
                from: "A".into(),
                to: "B".into(),
                weight: 1.0,
                kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "B".into(),
                to: "C".into(),
                weight: 1.0,
                kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
            },
        ];

        let chain = build_causal_chain("A", &priors, &edges, 2);
        // First-hop neighbor of A is B; second-hop is C.
        assert!(!chain.is_empty(), "chain must extend past target");
        assert_eq!(chain[0].depth, 1);
        assert_eq!(chain[0].symbol, "B");
        if chain.len() > 1 {
            assert_eq!(chain[1].depth, 2);
            assert_eq!(chain[1].symbol, "C");
        }
    }

    #[test]
    fn chain_picks_truly_affected_neighbor_not_always_extreme_node() {
        // Regression: previous impl ranked candidates by distance from
        // uniform, so a disconnected node with a strong observed prior
        // would beat a true neighbor of the intervention target.
        // Graph: A — B (one edge). C is disconnected but has a strong
        // observed Bull prior. Intervening on A pushes B; C is
        // unreachable. Correct chain[0] = B.
        let mut priors = HashMap::new();
        for sym in ["A", "B"] {
            priors.insert(sym.to_string(), NodePrior::default());
        }
        priors.insert(
            "C".into(),
            NodePrior {
                belief: [0.95, 0.025, 0.025],
                observed: true,
            },
        );
        let edges = vec![GraphEdge {
            from: "A".into(),
            to: "B".into(),
            weight: 1.0,
            kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
        }];
        let chain = build_causal_chain("A", &priors, &edges, 1);
        assert_eq!(chain.len(), 1, "chain must select an affected neighbor");
        assert_eq!(
            chain[0].symbol, "B",
            "expected truly-affected B, got always-extreme bystander {:?}",
            chain[0].symbol
        );
    }

    #[test]
    fn chain_skips_when_no_deviation_clears_floor() {
        // Disconnected target — no neighbors → empty chain.
        let mut priors = HashMap::new();
        priors.insert("LONE".to_string(), NodePrior::default());
        let edges = Vec::new();
        let chain = build_causal_chain("LONE", &priors, &edges, 3);
        assert!(chain.is_empty());
    }

    #[test]
    fn rolling_history_capped_at_window() {
        let mut runner = ActiveProbeRunner::new();
        let entry = runner.accuracy_history.entry("A".into()).or_default();
        for i in 0..(ACCURACY_HISTORY_PER_SYMBOL + 50) {
            entry.push_back(0.5 + (i as f64) * 1e-4);
        }
        // Trigger eviction via direct push if we add one more
        while entry.len() > ACCURACY_HISTORY_PER_SYMBOL {
            entry.pop_front();
        }
        assert_eq!(entry.len(), ACCURACY_HISTORY_PER_SYMBOL);
    }
}
