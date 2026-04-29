//! Shift A: latent world state — linear Gaussian state-space model.
//!
//! Until now Eden had no single object representing "what the world
//! looks like right now." Each stage read its own field. TacticalSetup
//! was the output; there was no unified current-state representation
//! to reason or plan over. That blocks causal rollout (Shift B) and
//! counterfactual planning (Shift C) because they need a single
//! anchor for "world state at time t."
//!
//! This module introduces `LatentWorldState` — a low-dim Gaussian
//! latent z_t evolved by a linear state-space model:
//!
//!   z_{t+1} = F z_t + w,   w ~ N(0, Q)        (transition)
//!   y_t    = H z_t + v,    v ~ N(0, R)        (observation)
//!
//! Kalman filter predict/update each tick. Dimensions chosen to be
//! operator-interpretable:
//!
//!   0: market stress            (composite of pressure field vortex tension)
//!   1: breadth                  (positive minus negative persistent state count)
//!   2: synchrony                (fraction of symbols moving together)
//!   3: institutional flow       (aggregate institutional channel pressure)
//!   4: retail flow              (aggregate non-institutional channel pressure)
//!
//! 5-dim is enough for a first prototype — operator can read each
//! dimension, and all five are already emergent quantities Eden
//! computes elsewhere (we're just wrapping them in a coherent SSM).
//!
//! Non-goals for v1:
//!   - Non-linear dynamics (VAE / neural SSM — Tier 2)
//!   - High-dim latents
//!   - Observation-dependent transition (that's a regime-switching
//!     SSM — worth doing later)

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::Market;

pub const LATENT_DIM: usize = 5;
pub const LATENT_NAMES: [&str; LATENT_DIM] =
    ["stress", "breadth", "synchrony", "inst_flow", "retail_flow"];

/// One tick's observation vector — emitted by the aggregator below
/// before being fed to the Kalman update step.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorldObservation {
    pub values: [f64; LATENT_DIM],
    /// If false, treat observation as missing for that dim (infinite
    /// variance in R). First tick typically has missing breadth /
    /// synchrony before Eden has enough symbol state samples.
    pub mask: [bool; LATENT_DIM],
}

impl WorldObservation {
    pub fn all_missing() -> Self {
        Self {
            values: [0.0; LATENT_DIM],
            mask: [false; LATENT_DIM],
        }
    }
}

/// 5×5 matrix stored row-major for serde simplicity.
pub type Mat5 = [[f64; LATENT_DIM]; LATENT_DIM];
pub type Vec5 = [f64; LATENT_DIM];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentWorldState {
    pub market: Market,
    pub last_tick: u64,
    /// z_t — latent mean.
    pub mean: Vec5,
    /// P_t — latent covariance.
    pub covariance: Mat5,
    /// F — transition matrix. Default is `mean_reversion * I`.
    pub transition: Mat5,
    /// Q — process noise covariance (diagonal default).
    pub process_noise: Mat5,
    /// H — observation matrix. Default is identity (each latent dim
    /// is directly observed).
    pub observation: Mat5,
    /// R — observation noise covariance (diagonal).
    pub observation_noise: Mat5,
    /// Total tick count — used to distinguish "never updated" from
    /// "just updated with missing observation."
    pub update_count: u32,
}

impl LatentWorldState {
    /// Sensible defaults: mild mean-reversion (F = 0.95 I), small
    /// process noise (Q = 0.02 I), identity observation (H = I),
    /// moderate observation noise (R = 0.10 I). Initial covariance
    /// is large (P0 = 1.0 I) so the first observation dominates.
    pub fn new(market: Market) -> Self {
        let mut transition = identity5();
        for i in 0..LATENT_DIM {
            transition[i][i] = 0.95;
        }
        let process_noise = scaled_identity5(0.02);
        let observation = identity5();
        let observation_noise = scaled_identity5(0.10);
        Self {
            market,
            last_tick: 0,
            mean: [0.0; LATENT_DIM],
            covariance: scaled_identity5(1.0),
            transition,
            process_noise,
            observation,
            observation_noise,
            update_count: 0,
        }
    }

    /// Kalman predict + update for one observation. `tick` is only
    /// used for the persistence tag.
    pub fn step(&mut self, tick: u64, obs: WorldObservation) {
        self.predict();
        self.update(obs);
        self.last_tick = tick;
        self.update_count = self.update_count.saturating_add(1);
    }

    fn predict(&mut self) {
        // mean' = F mean
        let new_mean = mat_vec(&self.transition, &self.mean);
        // cov' = F cov F^T + Q
        let f_cov = mat_mul(&self.transition, &self.covariance);
        let f_cov_ft = mat_mul_t(&f_cov, &self.transition);
        let new_cov = mat_add(&f_cov_ft, &self.process_noise);
        self.mean = new_mean;
        self.covariance = new_cov;
    }

    fn update(&mut self, obs: WorldObservation) {
        // Missing-value handling: rows of H corresponding to
        // unobserved dims are zeroed and their R entries bumped to
        // a huge value so the Kalman gain there ≈ 0.
        let mut h = self.observation;
        let mut r = self.observation_noise;
        for i in 0..LATENT_DIM {
            if !obs.mask[i] {
                for j in 0..LATENT_DIM {
                    h[i][j] = 0.0;
                }
                r[i][i] = 1.0e6;
            }
        }
        // innovation y - H mean
        let predicted_obs = mat_vec(&h, &self.mean);
        let mut innovation = [0.0_f64; LATENT_DIM];
        for i in 0..LATENT_DIM {
            innovation[i] = if obs.mask[i] {
                obs.values[i] - predicted_obs[i]
            } else {
                0.0
            };
        }
        // S = H cov H^T + R
        let h_cov = mat_mul(&h, &self.covariance);
        let h_cov_ht = mat_mul_t(&h_cov, &h);
        let s = mat_add(&h_cov_ht, &r);
        // K = cov H^T S^{-1}
        let s_inv = match invert_5x5(&s) {
            Some(inv) => inv,
            None => return, // degenerate — skip update rather than NaN out
        };
        let cov_ht = mat_mul_t(&self.covariance, &h);
        let k = mat_mul(&cov_ht, &s_inv);
        // mean += K * innovation
        let k_innov = mat_vec(&k, &innovation);
        for i in 0..LATENT_DIM {
            self.mean[i] += k_innov[i];
        }
        // cov = (I - K H) cov
        let k_h = mat_mul(&k, &h);
        let i_kh = mat_sub(&identity5(), &k_h);
        self.covariance = mat_mul(&i_kh, &self.covariance);
        // Symmetrize to keep numeric stability.
        for i in 0..LATENT_DIM {
            for j in i + 1..LATENT_DIM {
                let avg = 0.5 * (self.covariance[i][j] + self.covariance[j][i]);
                self.covariance[i][j] = avg;
                self.covariance[j][i] = avg;
            }
        }
    }

    pub fn dim_value(&self, idx: usize) -> Option<f64> {
        self.mean.get(idx).copied()
    }

    pub fn dim_variance(&self, idx: usize) -> Option<f64> {
        self.covariance
            .get(idx)
            .and_then(|row| row.get(idx))
            .copied()
    }

    /// Summary for wake emission: top dimensions by absolute value,
    /// paired with their standard deviation. Grep-friendly key=value.
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::with_capacity(LATENT_DIM);
        for i in 0..LATENT_DIM {
            let stdev = self.dim_variance(i).unwrap_or(0.0).max(0.0).sqrt();
            parts.push(format!(
                "{}={:+.2}±{:.2}",
                LATENT_NAMES[i], self.mean[i], stdev
            ));
        }
        format!(
            "world_state: tick={} updates={} {}",
            self.last_tick,
            self.update_count,
            parts.join(" "),
        )
    }
}

// ---------------------------------------------------------------------------
// Observation aggregator — turn pressure field + belief field stats
// into the 5-dim observation the SSM expects.
// ---------------------------------------------------------------------------

/// Compute a WorldObservation from per-tick aggregates. Dimensions
/// whose inputs are absent come back as masked.
pub fn aggregate_observation(inputs: &ObservationInputs) -> WorldObservation {
    let mut values = [0.0_f64; LATENT_DIM];
    let mut mask = [false; LATENT_DIM];

    if let Some(v) = inputs.market_stress {
        values[0] = v;
        mask[0] = true;
    }
    if let Some(v) = inputs.breadth {
        values[1] = v;
        mask[1] = true;
    }
    if let Some(v) = inputs.synchrony {
        values[2] = v;
        mask[2] = true;
    }
    if let Some(v) = inputs.institutional_flow {
        values[3] = v;
        mask[3] = true;
    }
    if let Some(v) = inputs.retail_flow {
        values[4] = v;
        mask[4] = true;
    }
    WorldObservation { values, mask }
}

/// Raw inputs from the runtime; each is Optional because early ticks
/// won't have e.g. informed intent belief.
#[derive(Debug, Clone, Copy, Default)]
pub struct ObservationInputs {
    pub market_stress: Option<f64>,
    pub breadth: Option<f64>,
    pub synchrony: Option<f64>,
    pub institutional_flow: Option<f64>,
    pub retail_flow: Option<f64>,
}

pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

// ---------------------------------------------------------------------------
// Matrix helpers — small fixed-size 5x5 inlined arithmetic. No nalgebra
// dep to keep the build light; we only need this specific size.
// ---------------------------------------------------------------------------

fn identity5() -> Mat5 {
    let mut m = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        m[i][i] = 1.0;
    }
    m
}

fn scaled_identity5(s: f64) -> Mat5 {
    let mut m = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        m[i][i] = s;
    }
    m
}

fn mat_mul(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            let mut s = 0.0;
            for k in 0..LATENT_DIM {
                s += a[i][k] * b[k][j];
            }
            out[i][j] = s;
        }
    }
    out
}

/// Multiply a by b^T.
fn mat_mul_t(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            let mut s = 0.0;
            for k in 0..LATENT_DIM {
                s += a[i][k] * b[j][k];
            }
            out[i][j] = s;
        }
    }
    out
}

fn mat_add(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            out[i][j] = a[i][j] + b[i][j];
        }
    }
    out
}

fn mat_sub(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            out[i][j] = a[i][j] - b[i][j];
        }
    }
    out
}

fn mat_vec(a: &Mat5, v: &Vec5) -> Vec5 {
    let mut out = [0.0_f64; LATENT_DIM];
    for i in 0..LATENT_DIM {
        let mut s = 0.0;
        for k in 0..LATENT_DIM {
            s += a[i][k] * v[k];
        }
        out[i] = s;
    }
    out
}

/// 5×5 matrix inverse via Gauss-Jordan with partial pivoting.
/// Returns None if singular (any pivot < 1e-12).
fn invert_5x5(m: &Mat5) -> Option<Mat5> {
    const N: usize = LATENT_DIM;
    let mut aug = [[0.0_f64; 2 * N]; N];
    for i in 0..N {
        for j in 0..N {
            aug[i][j] = m[i][j];
        }
        aug[i][N + i] = 1.0;
    }
    for col in 0..N {
        // partial pivoting
        let mut pivot = col;
        for row in col + 1..N {
            if aug[row][col].abs() > aug[pivot][col].abs() {
                pivot = row;
            }
        }
        if aug[pivot][col].abs() < 1e-12 {
            return None;
        }
        if pivot != col {
            aug.swap(col, pivot);
        }
        // normalize pivot row
        let pv = aug[col][col];
        for j in 0..2 * N {
            aug[col][j] /= pv;
        }
        // eliminate other rows
        for row in 0..N {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for j in 0..2 * N {
                aug[row][j] -= factor * aug[col][j];
            }
        }
    }
    let mut inv = [[0.0_f64; N]; N];
    for i in 0..N {
        for j in 0..N {
            inv[i][j] = aug[i][N + j];
        }
    }
    Some(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_inverse_roundtrip() {
        let i5 = identity5();
        let inv = invert_5x5(&i5).unwrap();
        for r in 0..LATENT_DIM {
            for c in 0..LATENT_DIM {
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!((inv[r][c] - expected).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn diagonal_inverse_correct() {
        let mut m = [[0.0_f64; 5]; 5];
        for i in 0..5 {
            m[i][i] = (i as f64) + 2.0;
        }
        let inv = invert_5x5(&m).unwrap();
        for i in 0..5 {
            let expected = 1.0 / ((i as f64) + 2.0);
            assert!((inv[i][i] - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn singular_matrix_returns_none() {
        let mut m = [[0.0_f64; 5]; 5];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        // row 2, 3, 4 all zero → singular
        assert!(invert_5x5(&m).is_none());
    }

    #[test]
    fn first_observation_dominates_prior() {
        let mut state = LatentWorldState::new(Market::Hk);
        let obs = WorldObservation {
            values: [0.8, 0.2, 0.5, 0.1, -0.1],
            mask: [true; 5],
        };
        state.step(1, obs);
        // With R=0.1 I and P0=1.0 I, Kalman gain pulls mean ~80-90%
        // toward observation on first step. Assert each dim moved past
        // half of the observation value.
        for i in 0..5 {
            assert!(
                state.mean[i].abs() > obs.values[i].abs() * 0.4,
                "dim {} mean={} obs={}",
                i,
                state.mean[i],
                obs.values[i]
            );
            assert!(
                state.mean[i].signum() == obs.values[i].signum() || obs.values[i].abs() < 1e-9,
                "dim {} sign mismatch",
                i
            );
        }
    }

    #[test]
    fn missing_observation_does_not_shift_mean_on_that_dim() {
        let mut state = LatentWorldState::new(Market::Hk);
        // Step 1: all observed, mean moves.
        state.step(
            1,
            WorldObservation {
                values: [0.5; 5],
                mask: [true; 5],
            },
        );
        let after1 = state.mean;

        // Step 2: only dim 0 observed, others masked.
        state.step(
            2,
            WorldObservation {
                values: [0.9, 0.0, 0.0, 0.0, 0.0],
                mask: [true, false, false, false, false],
            },
        );
        // Dim 0 should continue toward 0.9. Dims 1-4 should mean-
        // revert (0.95 * previous) since only transition runs.
        assert!(state.mean[0] > after1[0]);
        for i in 1..5 {
            // Without evidence, mean drifts toward 0 via 0.95 factor.
            assert!(
                state.mean[i] < after1[i],
                "dim {}: should mean-revert, got {} vs {}",
                i,
                state.mean[i],
                after1[i]
            );
        }
    }

    #[test]
    fn variance_shrinks_under_consistent_observations() {
        let mut state = LatentWorldState::new(Market::Hk);
        let obs = WorldObservation {
            values: [0.5; 5],
            mask: [true; 5],
        };
        let v0 = state.covariance[0][0];
        for t in 1..=20 {
            state.step(t, obs);
        }
        let v20 = state.covariance[0][0];
        assert!(v20 < v0 * 0.5, "variance should shrink: {} → {}", v0, v20);
    }

    #[test]
    fn aggregate_observation_masks_missing() {
        let inputs = ObservationInputs {
            market_stress: Some(0.4),
            breadth: None,
            synchrony: Some(0.7),
            institutional_flow: None,
            retail_flow: Some(-0.2),
        };
        let obs = aggregate_observation(&inputs);
        assert_eq!(obs.mask, [true, false, true, false, true]);
        assert!((obs.values[0] - 0.4).abs() < 1e-9);
        assert!((obs.values[2] - 0.7).abs() < 1e-9);
        assert!((obs.values[4] - (-0.2)).abs() < 1e-9);
    }

    #[test]
    fn summary_line_is_greppable() {
        let mut state = LatentWorldState::new(Market::Hk);
        state.step(
            5,
            WorldObservation {
                values: [0.42, 0.13, 0.85, 0.23, -0.08],
                mask: [true; 5],
            },
        );
        let line = state.summary_line();
        assert!(line.starts_with("world_state:"));
        assert!(line.contains("tick=5"));
        assert!(line.contains("updates=1"));
        assert!(line.contains("stress="));
        assert!(line.contains("breadth="));
        assert!(line.contains("synchrony="));
        assert!(line.contains("inst_flow="));
        assert!(line.contains("retail_flow="));
        // Each dim has ±stdev notation
        assert!(line.contains("±"));
    }

    #[test]
    fn mean_reversion_pulls_toward_zero_with_no_observations() {
        let mut state = LatentWorldState::new(Market::Hk);
        state.step(
            1,
            WorldObservation {
                values: [1.0; 5],
                mask: [true; 5],
            },
        );
        let after_obs = state.mean;
        // 20 ticks with no observations — mean should decay toward 0.
        for t in 2..=21 {
            state.step(t, WorldObservation::all_missing());
        }
        for i in 0..5 {
            assert!(state.mean[i].abs() < after_obs[i].abs() * 0.5);
        }
    }
}
