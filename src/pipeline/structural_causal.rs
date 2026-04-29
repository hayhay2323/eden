//! Shift B: structural causal model over the latent world state.
//!
//! `intervention.rs` does forward BFS on learned edge weights — that's
//! correlation-weighted propagation, not causal. Shift B moves to an
//! explicit structural causal model (SCM) in the Pearl / Spirtes
//! sense: each endogenous variable Y_i is written as
//!
//!   Y_i = α_i + Σ_{j ∈ pa(i)} β_{ij} * Y_j + U_i
//!
//! where `pa(i)` is the parent set in the DAG and `U_i` is the
//! exogenous noise term. With this form `do(Y_i := c)` means
//! replacing the structural equation for Y_i with the constant c and
//! propagating through the remaining equations in topological order.
//! That's true do-calculus — identical-in-form to Pearl's
//! interventional distribution for linear Gaussian SCMs.
//!
//! v1 scope:
//!   - Nodes = the 5 latent_world_state dimensions (stress, breadth,
//!     synchrony, inst_flow, retail_flow). Small enough to hand-
//!     specify priors; large enough to be useful for operator what-if.
//!   - Linear Gaussian structural equations.
//!   - Parameters are caller-specified (hand priors). Online fitting
//!     from latent_world_state history is v2.
//!   - `do_intervention(idx, value)` returns the full 5-dim expected
//!     mean after the intervention propagates through the DAG.
//!   - Variance propagation through linear combinations so callers
//!     can cite uncertainty alongside the point estimate.

use serde::{Deserialize, Serialize};

use crate::pipeline::latent_world_state::{LATENT_DIM, LATENT_NAMES};

/// One endogenous variable's structural equation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralEquation {
    /// Intercept α_i.
    pub intercept: f64,
    /// Coefficient vector β_{i,:}. Position j is β_{i,j}. Entries for
    /// j ∉ pa(i) MUST be zero (that's how the DAG is encoded in the
    /// coefficient matrix).
    pub coefs: [f64; LATENT_DIM],
    /// Exogenous noise variance.
    pub noise_var: f64,
}

impl StructuralEquation {
    pub fn identity(idx: usize) -> Self {
        // Fully-self equation Y_i = Y_i — degenerate; caller should
        // overwrite. Here just so default construction doesn't panic.
        let mut coefs = [0.0; LATENT_DIM];
        coefs[idx] = 1.0;
        Self {
            intercept: 0.0,
            coefs,
            noise_var: 0.0,
        }
    }

    pub fn root(intercept: f64, noise_var: f64) -> Self {
        Self {
            intercept,
            coefs: [0.0; LATENT_DIM],
            noise_var,
        }
    }

    pub fn from_parents(intercept: f64, parent_coefs: &[(usize, f64)], noise_var: f64) -> Self {
        let mut coefs = [0.0; LATENT_DIM];
        for (j, c) in parent_coefs {
            coefs[*j] = *c;
        }
        Self {
            intercept,
            coefs,
            noise_var,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralCausalModel {
    /// equations[i] is the structural equation for latent dim i.
    pub equations: [StructuralEquation; LATENT_DIM],
    /// Topological order of the DAG (indices into equations).
    /// Stored explicitly so do-intervention can iterate without
    /// re-solving it each call.
    pub topo_order: [usize; LATENT_DIM],
}

/// Error raised when the coefficient matrix implies a cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScmError {
    CycleDetected,
    IndexOutOfRange,
}

impl StructuralCausalModel {
    /// Build from equations; validates acyclic + derives topo order.
    pub fn try_new(equations: [StructuralEquation; LATENT_DIM]) -> Result<Self, ScmError> {
        let topo = topological_sort(&equations)?;
        Ok(Self {
            equations,
            topo_order: topo,
        })
    }

    /// Default SCM for the 5-dim latent world: stress is a root;
    /// breadth is driven negatively by stress; synchrony positively by
    /// stress; inst_flow by (−stress, +synchrony); retail_flow by
    /// (+breadth, −stress). These are operator-interpretable priors
    /// to be refined once we fit from data.
    pub fn default_latent_scm() -> Self {
        let mut eqs: [StructuralEquation; LATENT_DIM] = [
            StructuralEquation::identity(0),
            StructuralEquation::identity(1),
            StructuralEquation::identity(2),
            StructuralEquation::identity(3),
            StructuralEquation::identity(4),
        ];
        // stress (0): root.
        eqs[0] = StructuralEquation::root(0.0, 0.15);
        // breadth (1): negative effect of stress.
        eqs[1] = StructuralEquation::from_parents(0.0, &[(0, -0.40)], 0.10);
        // synchrony (2): positive effect of stress.
        eqs[2] = StructuralEquation::from_parents(0.0, &[(0, 0.50)], 0.10);
        // inst_flow (3): negative stress, positive synchrony.
        eqs[3] = StructuralEquation::from_parents(0.0, &[(0, -0.30), (2, 0.20)], 0.10);
        // retail_flow (4): positive breadth, negative stress.
        eqs[4] = StructuralEquation::from_parents(0.0, &[(0, -0.15), (1, 0.35)], 0.10);
        StructuralCausalModel::try_new(eqs).expect("default SCM is acyclic by construction")
    }

    /// Compute expected latent mean after `do(Y_idx := value)`.
    /// Propagates through the DAG in topological order, using the
    /// intervention constant for the target node and structural
    /// equations for all others.
    pub fn do_intervention(&self, idx: usize, value: f64) -> Result<[f64; LATENT_DIM], ScmError> {
        if idx >= LATENT_DIM {
            return Err(ScmError::IndexOutOfRange);
        }
        let mut mean = [0.0_f64; LATENT_DIM];
        for &node in &self.topo_order {
            if node == idx {
                mean[node] = value;
                continue;
            }
            let eq = &self.equations[node];
            let mut m = eq.intercept;
            for j in 0..LATENT_DIM {
                m += eq.coefs[j] * mean[j];
            }
            mean[node] = m;
        }
        Ok(mean)
    }

    /// Propagate variance through the SCM assuming all exogenous
    /// noises are independent Gaussians. For linear equations the
    /// variance at node i is:
    ///   Var(Y_i) = Σ_{j ∈ pa(i)} β_{ij}^2 * Var(Y_j) + noise_var_i
    /// (No covariance terms — this is the "no shared noise" case,
    /// which is the standard linear-Gaussian SCM assumption.)
    pub fn do_intervention_with_variance(
        &self,
        idx: usize,
        value: f64,
    ) -> Result<([f64; LATENT_DIM], [f64; LATENT_DIM]), ScmError> {
        let mean = self.do_intervention(idx, value)?;
        let mut var = [0.0_f64; LATENT_DIM];
        for &node in &self.topo_order {
            if node == idx {
                var[node] = 0.0;
                continue;
            }
            let eq = &self.equations[node];
            let mut v = eq.noise_var;
            for j in 0..LATENT_DIM {
                v += eq.coefs[j] * eq.coefs[j] * var[j];
            }
            var[node] = v;
        }
        Ok((mean, var))
    }

    /// Expected baseline (no intervention) — propagate structural
    /// equations from nothing, treating all roots as their intercepts.
    pub fn expected_baseline(&self) -> [f64; LATENT_DIM] {
        let mut mean = [0.0_f64; LATENT_DIM];
        for &node in &self.topo_order {
            let eq = &self.equations[node];
            let mut m = eq.intercept;
            for j in 0..LATENT_DIM {
                m += eq.coefs[j] * mean[j];
            }
            mean[node] = m;
        }
        mean
    }

    /// Describe the cascade from an intervention as a grep-friendly
    /// line. Useful for wake emission: "do(stress := 1.5) cascade:
    /// breadth=-0.60, synchrony=+0.75, ..."
    pub fn describe_intervention(&self, idx: usize, value: f64) -> String {
        let baseline = self.expected_baseline();
        let Ok(intervened) = self.do_intervention(idx, value) else {
            return format!("do({} := {:.2}): invalid index", LATENT_NAMES[idx], value);
        };
        let mut parts = Vec::new();
        for i in 0..LATENT_DIM {
            if i == idx {
                continue;
            }
            let delta = intervened[i] - baseline[i];
            if delta.abs() < 1e-4 {
                continue;
            }
            parts.push(format!("{}={:+.2}", LATENT_NAMES[i], delta));
        }
        if parts.is_empty() {
            return format!("do({} := {:.2}): no cascade", LATENT_NAMES[idx], value);
        }
        format!(
            "do({} := {:.2}) cascade: {}",
            LATENT_NAMES[idx],
            value,
            parts.join(", "),
        )
    }
}

// ---------------------------------------------------------------------------
// Topological sort over the coefficient-DAG
// ---------------------------------------------------------------------------

fn topological_sort(
    equations: &[StructuralEquation; LATENT_DIM],
) -> Result<[usize; LATENT_DIM], ScmError> {
    // in_degree[i] = number of j where β_{i,j} != 0 AND j != i
    // (self-loops on root equations with β_{i,i}=0 are fine; identity
    // placeholders have β_{i,i}=1 but no other incoming edges.)
    let mut in_degree = [0usize; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            if i == j {
                continue;
            }
            if equations[i].coefs[j].abs() > f64::EPSILON {
                in_degree[i] += 1;
            }
        }
    }
    let mut queue: Vec<usize> = (0..LATENT_DIM).filter(|i| in_degree[*i] == 0).collect();
    let mut order = [0usize; LATENT_DIM];
    let mut written = 0;
    while let Some(node) = queue.pop() {
        order[written] = node;
        written += 1;
        for child in 0..LATENT_DIM {
            if child == node {
                continue;
            }
            if equations[child].coefs[node].abs() > f64::EPSILON {
                in_degree[child] -= 1;
                if in_degree[child] == 0 {
                    queue.push(child);
                }
            }
        }
    }
    if written != LATENT_DIM {
        return Err(ScmError::CycleDetected);
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scm_is_acyclic() {
        let scm = StructuralCausalModel::default_latent_scm();
        // topo_order has LATENT_DIM unique entries in 0..LATENT_DIM
        let mut seen = [false; LATENT_DIM];
        for &n in &scm.topo_order {
            assert!(n < LATENT_DIM);
            assert!(!seen[n]);
            seen[n] = true;
        }
    }

    #[test]
    fn cycle_detection_rejects_mutual_edges() {
        let mut eqs: [StructuralEquation; LATENT_DIM] = [
            StructuralEquation::identity(0),
            StructuralEquation::identity(1),
            StructuralEquation::identity(2),
            StructuralEquation::identity(3),
            StructuralEquation::identity(4),
        ];
        // 0 → 1 and 1 → 0 creates a cycle.
        eqs[0] = StructuralEquation::from_parents(0.0, &[(1, 0.5)], 0.1);
        eqs[1] = StructuralEquation::from_parents(0.0, &[(0, 0.5)], 0.1);
        assert_eq!(
            StructuralCausalModel::try_new(eqs).err(),
            Some(ScmError::CycleDetected)
        );
    }

    #[test]
    fn do_intervention_on_root_cascades_downstream() {
        let scm = StructuralCausalModel::default_latent_scm();
        let out = scm.do_intervention(0, 1.0).unwrap();
        // stress = 1.0; synchrony should be +0.50 (direct coef),
        // breadth should be -0.40. inst_flow: -0.30*1.0 + 0.20*0.50 = -0.20.
        // retail_flow: -0.15*1.0 + 0.35*(-0.40) = -0.29.
        assert!((out[0] - 1.0).abs() < 1e-9);
        assert!((out[2] - 0.50).abs() < 1e-9);
        assert!((out[1] - (-0.40)).abs() < 1e-9);
        assert!((out[3] - (-0.20)).abs() < 1e-9);
        assert!((out[4] - (-0.29)).abs() < 1e-9);
    }

    #[test]
    fn do_intervention_on_downstream_leaves_upstream_untouched() {
        let scm = StructuralCausalModel::default_latent_scm();
        // Intervene on retail_flow (leaf). stress, breadth, synchrony,
        // inst_flow all take their baseline values (all 0 with default
        // intercepts).
        let out = scm.do_intervention(4, 5.0).unwrap();
        assert!((out[4] - 5.0).abs() < 1e-9);
        for i in 0..4 {
            assert!(
                out[i].abs() < 1e-9,
                "upstream dim {} should be 0, got {}",
                i,
                out[i]
            );
        }
    }

    #[test]
    fn variance_propagates_through_linear_coefs() {
        let scm = StructuralCausalModel::default_latent_scm();
        let (_m, var) = scm.do_intervention_with_variance(0, 1.0).unwrap();
        // stress has 0 variance (intervened).
        assert!((var[0] - 0.0).abs() < 1e-9);
        // synchrony: β^2 * 0 + noise_var = 0 + 0.10 = 0.10.
        assert!((var[2] - 0.10).abs() < 1e-9);
        // breadth: β^2 * 0 + 0.10 = 0.10.
        assert!((var[1] - 0.10).abs() < 1e-9);
        // inst_flow: (-0.30)^2 * 0 + (0.20)^2 * 0.10 + 0.10
        //            = 0 + 0.004 + 0.10 = 0.104.
        assert!((var[3] - 0.104).abs() < 1e-6);
    }

    #[test]
    fn describe_intervention_line_shape() {
        let scm = StructuralCausalModel::default_latent_scm();
        let line = scm.describe_intervention(0, 1.5);
        assert!(line.starts_with("do(stress := 1.50) cascade:"));
        assert!(line.contains("breadth="));
        assert!(line.contains("synchrony="));
    }

    #[test]
    fn describe_intervention_on_leaf_reports_no_cascade() {
        let scm = StructuralCausalModel::default_latent_scm();
        let line = scm.describe_intervention(4, 5.0);
        assert!(line.contains("no cascade") || line.contains("cascade:"));
    }

    #[test]
    fn custom_scm_rejects_out_of_range_index() {
        let scm = StructuralCausalModel::default_latent_scm();
        let err = scm.do_intervention(10, 1.0).unwrap_err();
        assert_eq!(err, ScmError::IndexOutOfRange);
    }
}
