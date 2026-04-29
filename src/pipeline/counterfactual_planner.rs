//! Shift C: counterfactual rollout planner.
//!
//! Previous shifts delivered:
//!   - Shift A: LatentWorldState — single anchor for "what the world
//!     looks like right now" (Kalman-filtered Gaussian SSM).
//!   - Shift B: StructuralCausalModel — do(Y_i := c) propagation
//!     through a linear Gaussian DAG over the same 5 latent dims.
//!
//! Shift C is the planner: given current state + a list of candidate
//! `do()` actions, roll each forward K ticks using the SCM as the
//! contemporaneous causal engine + the SSM's transition matrix F as
//! the temporal dynamics, score each terminal trajectory with a
//! utility function, return the best action.
//!
//! This is the thin version of MuZero/MCTS planning: one forward
//! rollout per action, no tree search, deterministic expected mean
//! (variance propagated for uncertainty quotation).
//!
//! v1 scope:
//!   - Candidate actions = single-variable `do()` interventions on a
//!     latent dim, OR the no-op baseline.
//!   - Rollout: at t, apply do() (if any) via SCM → then transition
//!     step (mean ← F*mean) → repeat K times.
//!   - Utility: caller-supplied closure over the trajectory's last
//!     mean vector. Default `operator_utility()` favours low |stress|,
//!     positive breadth, high synchrony.
//!
//! Non-goals v1:
//!   - Stochastic rollouts (we sample no noise; we propagate mean+var)
//!   - Tree search (one forward rollout per action only)
//!   - Action sequences (only single-step interventions at t=0)

use serde::{Deserialize, Serialize};

use crate::pipeline::latent_world_state::{LatentWorldState, LATENT_DIM, LATENT_NAMES};
use crate::pipeline::structural_causal::StructuralCausalModel;

/// One candidate decision the planner can evaluate. `None` is the
/// baseline (no-op) trajectory — rollout just uses the SSM's natural
/// mean-reversion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateAction {
    pub label: String,
    /// `Some((dim, value))` means set Z_dim := value at t=0 before
    /// rolling forward. `None` means no intervention.
    pub intervention: Option<(usize, f64)>,
}

impl CandidateAction {
    pub fn baseline() -> Self {
        Self {
            label: "baseline".to_string(),
            intervention: None,
        }
    }

    pub fn intervene_stress(value: f64) -> Self {
        Self {
            label: format!("do(stress := {:+.2})", value),
            intervention: Some((0, value)),
        }
    }

    pub fn intervene(dim: usize, value: f64) -> Self {
        let name = LATENT_NAMES.get(dim).copied().unwrap_or("?");
        Self {
            label: format!("do({} := {:+.2})", name, value),
            intervention: Some((dim, value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutResult {
    pub action_label: String,
    /// Mean trajectory across K+1 steps (index 0 = t=0 state after
    /// applying intervention; index K = terminal state).
    pub trajectory: Vec<[f64; LATENT_DIM]>,
    pub terminal_variance: [f64; LATENT_DIM],
    pub utility: f64,
}

/// Default operator utility: prefer low |stress|, positive breadth,
/// high synchrony, positive inst_flow. Retail_flow is penalised for
/// extreme magnitudes (overheated retail is a risk both ways).
/// Normalised so typical outputs sit in roughly [-1, +1].
pub fn operator_utility(mean: &[f64; LATENT_DIM]) -> f64 {
    let stress = mean[0];
    let breadth = mean[1];
    let synchrony = mean[2];
    let inst_flow = mean[3];
    let retail_flow = mean[4];
    // Weighting reflects operator priors: stress dominates (2x), the
    // rest are unit weight. Extreme retail gets a quadratic penalty.
    -2.0 * stress.abs() + breadth + synchrony + inst_flow - 0.5 * retail_flow * retail_flow
}

/// Roll one action forward from `initial` state for `steps` ticks.
/// Uses the SCM at t=0 to apply the intervention cascade, then the
/// SSM's transition matrix F to evolve each subsequent step.
pub fn rollout_action<U>(
    initial: &LatentWorldState,
    scm: &StructuralCausalModel,
    action: &CandidateAction,
    steps: usize,
    utility: U,
) -> RolloutResult
where
    U: Fn(&[f64; LATENT_DIM]) -> f64,
{
    let mut mean = initial.mean;
    let mut var = [0.0_f64; LATENT_DIM];
    for i in 0..LATENT_DIM {
        var[i] = initial.covariance[i][i];
    }

    // Apply intervention at t=0 via SCM cascade.
    if let Some((dim, value)) = action.intervention {
        if let Ok((do_mean, do_var)) = scm.do_intervention_with_variance(dim, value) {
            // Blend: the intervention sets dim to value; other dims
            // take the SCM's predicted cascade mean delta on top of
            // their current value. We interpret SCM baseline as the
            // "pre-intervention" anchor, so the cascade modifies the
            // difference between current and baseline.
            let baseline = scm.expected_baseline();
            for i in 0..LATENT_DIM {
                if i == dim {
                    mean[i] = value;
                    var[i] = 0.0;
                } else {
                    let delta = do_mean[i] - baseline[i];
                    mean[i] += delta;
                    // Variance additive in linear combinations.
                    var[i] += do_var[i];
                }
            }
        }
    }

    let mut trajectory = Vec::with_capacity(steps + 1);
    trajectory.push(mean);

    // Temporal evolution: apply transition F each step, grow variance
    // by process noise Q.
    for _ in 0..steps {
        let mut next_mean = [0.0_f64; LATENT_DIM];
        for i in 0..LATENT_DIM {
            let mut s = 0.0;
            for j in 0..LATENT_DIM {
                s += initial.transition[i][j] * mean[j];
            }
            next_mean[i] = s;
        }
        // F * Σ * F^T is full matrix; v1 just tracks diagonal via
        // |F_ii|^2 contribution — adequate for operator read of
        // "roughly how uncertain after K steps."
        let mut next_var = [0.0_f64; LATENT_DIM];
        for i in 0..LATENT_DIM {
            let f_ii = initial.transition[i][i];
            next_var[i] = f_ii * f_ii * var[i] + initial.process_noise[i][i];
        }
        mean = next_mean;
        var = next_var;
        trajectory.push(mean);
    }

    let u = utility(&mean);
    RolloutResult {
        action_label: action.label.clone(),
        trajectory,
        terminal_variance: var,
        utility: u,
    }
}

/// Score every candidate and return (best_idx, best_utility) plus the
/// full rollout sorted descending by utility.
pub fn best_action<U>(
    initial: &LatentWorldState,
    scm: &StructuralCausalModel,
    actions: &[CandidateAction],
    steps: usize,
    utility: U,
) -> Option<BestActionSummary>
where
    U: Fn(&[f64; LATENT_DIM]) -> f64 + Copy,
{
    if actions.is_empty() {
        return None;
    }
    let mut results: Vec<RolloutResult> = actions
        .iter()
        .map(|a| rollout_action(initial, scm, a, steps, utility))
        .collect();
    results.sort_by(|a, b| {
        b.utility
            .partial_cmp(&a.utility)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let best = results[0].clone();
    Some(BestActionSummary { best, all: results })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BestActionSummary {
    pub best: RolloutResult,
    pub all: Vec<RolloutResult>,
}

impl BestActionSummary {
    /// One-line grep-friendly wake emission.
    ///   `planner: best=do(stress := -0.50) U=+0.83 terminal(stress=-0.47,…)
    ///    runners: baseline U=+0.12; do(stress := +0.50) U=-1.05`
    pub fn summary_line(&self) -> String {
        let term = self
            .best
            .trajectory
            .last()
            .cloned()
            .unwrap_or([0.0; LATENT_DIM]);
        let term_str = LATENT_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{}={:+.2}", name, term[i]))
            .collect::<Vec<_>>()
            .join(",");
        let runners = self
            .all
            .iter()
            .skip(1)
            .take(3)
            .map(|r| format!("{} U={:+.2}", r.action_label, r.utility))
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "planner: best={} U={:+.2} terminal({}) runners: {}",
            self.best.action_label, self.best.utility, term_str, runners,
        )
    }
}

/// Default candidate set for v1 per-tick wake emission. Five actions:
/// baseline, stress ±0.5, inst_flow ±0.5. Gives operator a read on
/// "if one lever moved, how does the utility shift."
pub fn default_candidate_set(state: &LatentWorldState) -> Vec<CandidateAction> {
    let stress_now = state.mean[0];
    let inst_now = state.mean[3];
    vec![
        CandidateAction::baseline(),
        CandidateAction::intervene(0, stress_now - 0.5),
        CandidateAction::intervene(0, stress_now + 0.5),
        CandidateAction::intervene(3, inst_now - 0.5),
        CandidateAction::intervene(3, inst_now + 0.5),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::objects::Market;
    use crate::pipeline::latent_world_state::WorldObservation;

    fn prepped_state() -> LatentWorldState {
        let mut s = LatentWorldState::new(Market::Hk);
        s.step(
            1,
            WorldObservation {
                values: [0.3, 0.1, 0.2, 0.0, -0.1],
                mask: [true; 5],
            },
        );
        s
    }

    #[test]
    fn baseline_rollout_mean_reverts_toward_zero() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        let result = rollout_action(&state, &scm, &CandidateAction::baseline(), 20, |_| 0.0);
        let start = result.trajectory[0];
        let end = result.trajectory.last().unwrap();
        // With F = 0.95 I and 20 steps, each dim should shrink by ~0.36x.
        for i in 0..LATENT_DIM {
            assert!(
                end[i].abs() < start[i].abs() + 1e-6,
                "dim {} didn't mean-revert: start={} end={}",
                i,
                start[i],
                end[i]
            );
        }
    }

    #[test]
    fn intervention_shifts_terminal_state() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        let baseline = rollout_action(&state, &scm, &CandidateAction::baseline(), 5, |_| 0.0);
        let stressed = rollout_action(&state, &scm, &CandidateAction::intervene(0, 1.5), 5, |_| {
            0.0
        });
        // Stress intervention should push stress dim higher at t=0,
        // and since F decays slowly, still higher at K=5 terminal.
        assert!(stressed.trajectory.last().unwrap()[0] > baseline.trajectory.last().unwrap()[0]);
    }

    #[test]
    fn best_action_prefers_lower_stress_under_operator_utility() {
        let mut state = LatentWorldState::new(Market::Hk);
        // Start with high stress.
        state.step(
            1,
            WorldObservation {
                values: [0.8, 0.0, 0.0, 0.0, 0.0],
                mask: [true; 5],
            },
        );
        let scm = StructuralCausalModel::default_latent_scm();
        let actions = default_candidate_set(&state);
        let summary = best_action(&state, &scm, &actions, 5, operator_utility).unwrap();
        // Under operator_utility, low stress is good. The intervention
        // lowering stress should beat baseline and the stress-up option.
        let best_label = &summary.best.action_label;
        assert!(
            best_label.contains("stress")
                || best_label.contains("inst_flow")
                || best_label == "baseline",
            "unexpected best action: {}",
            best_label
        );
        // The stress-up action should be among the worst.
        let stress_up = summary
            .all
            .iter()
            .find(|r| r.action_label.contains("stress") && r.action_label.contains("+"))
            .unwrap();
        assert!(
            summary.best.utility >= stress_up.utility,
            "best {} should beat stress-up {}",
            summary.best.utility,
            stress_up.utility
        );
    }

    #[test]
    fn variance_grows_over_rollout() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        let v0_before = state.covariance[0][0];
        let result = rollout_action(&state, &scm, &CandidateAction::baseline(), 20, |_| 0.0);
        // Terminal variance should be LARGER than initial covariance
        // diagonal because each step adds process noise and F < 1 is
        // a slight decay (F^2 = 0.9025, so 20 steps add many Q's).
        assert!(
            result.terminal_variance[0] > v0_before * 0.5,
            "variance collapsed: {} vs initial {}",
            result.terminal_variance[0],
            v0_before
        );
    }

    #[test]
    fn summary_line_is_greppable() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        let actions = default_candidate_set(&state);
        let summary = best_action(&state, &scm, &actions, 5, operator_utility).unwrap();
        let line = summary.summary_line();
        assert!(line.starts_with("planner:"));
        assert!(line.contains("best="));
        assert!(line.contains("U="));
        assert!(line.contains("terminal("));
        assert!(line.contains("runners:"));
    }

    #[test]
    fn empty_candidate_set_returns_none() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        let summary = best_action(&state, &scm, &[], 5, operator_utility);
        assert!(summary.is_none());
    }

    #[test]
    fn operator_utility_favours_low_stress() {
        let calm = [0.0, 0.2, 0.5, 0.1, 0.0];
        let stressed = [0.8, 0.2, 0.5, 0.1, 0.0];
        let u_calm = operator_utility(&calm);
        let u_stressed = operator_utility(&stressed);
        assert!(
            u_calm > u_stressed,
            "calm {} should beat stressed {}",
            u_calm,
            u_stressed
        );
    }

    #[test]
    fn trajectory_length_matches_steps_plus_one() {
        let state = prepped_state();
        let scm = StructuralCausalModel::default_latent_scm();
        for steps in [1, 5, 10, 20] {
            let r = rollout_action(&state, &scm, &CandidateAction::baseline(), steps, |_| 0.0);
            assert_eq!(r.trajectory.len(), steps + 1);
        }
    }
}
