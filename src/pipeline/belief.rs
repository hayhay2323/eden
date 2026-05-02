//! Explicit uncertainty wrapper — the foundation of Eden's Bayesian layer.
//!
//! Eden's existing surface (NodePressure, SymbolResidual, PersistentSymbolState)
//! represents signals as scalar Decimals with no uncertainty. That's fine for
//! display and thresholding but can't answer the two questions Y needs:
//!
//!   1. `一葉知森羅` — given sparse observation, what's the posterior
//!      over hidden market state?  → requires a distribution, not a scalar.
//!
//!   2. `掌握權限` — which next observation would reduce uncertainty most?
//!      → requires mutual information, which requires distributions.
//!
//! This module introduces two belief types that can be attached to any
//! existing scalar or categorical value WITHOUT replacing it:
//!
//!   - `GaussianBelief` — for continuous scalars (pressure, residual, etc.)
//!     Uses Welford's online algorithm so variance updates are O(1) and
//!     numerically stable over streaming ticks.
//!
//!   - `CategoricalBelief<K>` — for discrete states (state_kind,
//!     VortexLifecycle, OptionVerdict). Holds a probability mass on each
//!     variant, normalized to sum = 1.
//!
//! Both expose `kl_divergence` (Friston surprise) and `entropy` (for mutual
//! information). These are the primitives later tasks (T27 W2-W4) will
//! compose into active probing + do-calculus + KL-based vortex tension.
//!
//! Design: we compute in `f64` internally (Decimal doesn't support `ln`/`exp`
//! natively) but accept and return `Decimal` at the boundary so Eden's
//! existing arithmetic stays exact where it matters. The f64 path is only
//! used where information theory requires `ln` — a boundary we cross
//! explicitly, not by accident.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Small floor added to variance to avoid division-by-zero when a belief
/// is built from a single observation. Equivalent to ~1 bps of a unit
/// quantity — dominated by any real signal but keeps math well-defined.
const VARIANCE_FLOOR: f64 = 1.0e-8;

/// Number of observations above which a belief is considered "informed"
/// enough that downstream consumers can trust its variance estimate.
pub const BELIEF_INFORMED_MIN_SAMPLES: u32 = 5;

// ---------------------------------------------------------------------------
// GaussianBelief — continuous scalar with online variance
// ---------------------------------------------------------------------------

/// Running Gaussian belief over a scalar quantity.
///
/// Maintained via Welford's online algorithm: every `update` is O(1) in
/// both time and memory, and the variance stays numerically stable even
/// across millions of ticks. The belief is fully determined by `(mean,
/// variance, sample_count)`; we avoid storing raw samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianBelief {
    /// Running mean — the point estimate.
    pub mean: Decimal,
    /// Running variance. Invariant: `>= VARIANCE_FLOOR` when `sample_count >= 1`.
    pub variance: Decimal,
    /// Number of samples that contributed to this belief. 0 means
    /// "uninformed prior"; callers should treat such beliefs as carrying
    /// infinite variance for decision purposes.
    pub sample_count: u32,
    /// Sum of squared deviations from the running mean (Welford's M2).
    /// Kept internally so merges and updates stay exact. Not displayed.
    #[serde(default)]
    m2: Decimal,
}

impl GaussianBelief {
    /// Construct an uninformed prior centered at `mean` with large variance.
    /// Typical use: seeding a node's belief from its most recent point
    /// estimate before any real streaming updates have arrived.
    pub fn uninformed(mean: Decimal) -> Self {
        Self {
            mean,
            variance: Decimal::ONE,
            sample_count: 0,
            m2: Decimal::ZERO,
        }
    }

    /// Construct a belief from a first observation. Variance is set to the
    /// floor so `kl_divergence` stays well-defined; subsequent `update`s
    /// replace it with the real running variance.
    pub fn from_first_sample(value: Decimal) -> Self {
        Self {
            mean: value,
            variance: Decimal::try_from(VARIANCE_FLOOR).unwrap_or(Decimal::ZERO),
            sample_count: 1,
            m2: Decimal::ZERO,
        }
    }

    /// Welford online update. After the call, `sample_count` is incremented
    /// and `mean`/`variance` reflect the new sample. Safe to call any
    /// number of times; numerically stable.
    pub fn update(&mut self, observation: Decimal) {
        self.sample_count = self.sample_count.saturating_add(1);
        let n = Decimal::from(self.sample_count);
        let delta = observation - self.mean;
        self.mean += delta / n;
        let delta2 = observation - self.mean;
        self.m2 += delta * delta2;
        if self.sample_count >= 2 {
            let denom = Decimal::from(self.sample_count - 1);
            let raw = self.m2 / denom;
            let floor = Decimal::try_from(VARIANCE_FLOOR).unwrap_or(Decimal::ZERO);
            self.variance = if raw > floor { raw } else { floor };
        }
    }

    /// True if the belief has enough samples that variance is meaningful.
    /// Single-sample beliefs have floor variance; callers that branch on
    /// "real uncertainty" should check this flag.
    pub fn is_informed(&self) -> bool {
        self.sample_count >= BELIEF_INFORMED_MIN_SAMPLES
    }

    /// Differential entropy of a univariate Gaussian: `0.5 * ln(2πe σ²)`.
    /// Returns `None` if the belief is uninformed. Used as the building
    /// block for mutual information (T27 W2).
    pub fn entropy(&self) -> Option<f64> {
        if self.sample_count < 1 {
            return None;
        }
        let var = self.variance.to_f64().unwrap_or(VARIANCE_FLOOR);
        let safe_var = if var > VARIANCE_FLOOR {
            var
        } else {
            VARIANCE_FLOOR
        };
        // 0.5 * ln(2πe σ²) = 0.5 * (ln(2πe) + ln σ²)
        const LN_2PIE: f64 = 2.837_877_066_409_345_5; // ln(2πe)
        Some(0.5 * (LN_2PIE + safe_var.ln()))
    }

    /// KL divergence from `self` to `other`, treating both as univariate
    /// Gaussians. Defined as:
    ///
    /// ```text
    /// KL(p || q) = ln(σ_q/σ_p) + (σ_p² + (μ_p - μ_q)²) / (2 σ_q²) - 0.5
    /// ```
    ///
    /// Returns `None` if either belief is uninformed (no basis for
    /// computing surprise). In Eden this is the natural "prior vs
    /// observation" surprise that will power the KL-based vortex tension
    /// in T27 W4: `prior = hour layer belief`, `other = tick layer
    /// belief`, `KL` replaces the existing scalar subtraction.
    pub fn kl_divergence(&self, other: &GaussianBelief) -> Option<f64> {
        if self.sample_count < 1 || other.sample_count < 1 {
            return None;
        }
        let mu_p = self.mean.to_f64()?;
        let mu_q = other.mean.to_f64()?;
        let var_p = self
            .variance
            .to_f64()
            .unwrap_or(VARIANCE_FLOOR)
            .max(VARIANCE_FLOOR);
        let var_q = other
            .variance
            .to_f64()
            .unwrap_or(VARIANCE_FLOOR)
            .max(VARIANCE_FLOOR);
        let mean_sq = (mu_p - mu_q).powi(2);
        Some(0.5 * (var_q.ln() - var_p.ln()) + (var_p + mean_sq) / (2.0 * var_q) - 0.5)
    }

    /// Merge another belief into `self` as if their samples were pooled.
    /// Useful for combining per-channel beliefs into a composite belief
    /// (e.g. pressure-channel beliefs into a node-level belief). Based on
    /// Chan et al.'s parallel Welford merge formula.
    pub fn merge(&mut self, other: &GaussianBelief) {
        if other.sample_count == 0 {
            return;
        }
        if self.sample_count == 0 {
            *self = other.clone();
            return;
        }
        let n_a = Decimal::from(self.sample_count);
        let n_b = Decimal::from(other.sample_count);
        let n_total = n_a + n_b;
        let delta = other.mean - self.mean;
        self.mean = (self.mean * n_a + other.mean * n_b) / n_total;
        // M2_total = M2_a + M2_b + delta^2 * n_a * n_b / n_total
        self.m2 = self.m2 + other.m2 + delta * delta * n_a * n_b / n_total;
        self.sample_count = self.sample_count.saturating_add(other.sample_count);
        let denom = Decimal::from(self.sample_count - 1).max(Decimal::ONE);
        let raw = self.m2 / denom;
        let floor = Decimal::try_from(VARIANCE_FLOOR).unwrap_or(Decimal::ZERO);
        self.variance = if raw > floor { raw } else { floor };
    }

    /// Internal Welford M2 (sum of squared deviations from the running
    /// mean). Exposed for snapshot serialization only — downstream code
    /// should not depend on this.
    pub fn m2_internal(&self) -> Decimal {
        self.m2
    }

    /// Restore a belief from previously-serialized internal state. Used
    /// only by snapshot deserialization; normal construction should go
    /// through `from_first_sample` + `update`.
    pub fn from_raw(mean: Decimal, variance: Decimal, m2: Decimal, sample_count: u32) -> Self {
        Self {
            mean,
            variance,
            sample_count,
            m2,
        }
    }
}

// ---------------------------------------------------------------------------
// CategoricalBelief — discrete state with probability mass
// ---------------------------------------------------------------------------

/// Probability distribution over `K` discrete outcomes. The variants live
/// in a fixed `variants` vector so callers can iterate in a deterministic
/// order; the `probs` vector is always the same length, holds non-negative
/// entries, and sums to 1 (enforced on every mutation).
///
/// Used in Eden for:
///   - `PersistentStateKind` posterior (Continuation / TurningPoint / ...)
///   - `VortexLifecycle` posterior (Growing / Peaking / Fading / ...)
///   - `OptionVerdict` posterior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoricalBelief<K: Clone + Eq> {
    pub variants: Vec<K>,
    pub probs: Vec<Decimal>,
    pub sample_count: u32,
}

impl<K: Clone + Eq> CategoricalBelief<K> {
    /// Uniform prior — every variant equiprobable.
    pub fn uniform(variants: Vec<K>) -> Self {
        let n = variants.len().max(1);
        let p = Decimal::ONE / Decimal::from(n as i64);
        let probs = vec![p; variants.len()];
        Self {
            variants,
            probs,
            sample_count: 0,
        }
    }

    /// Restore a categorical belief from previously-serialized state.
    /// Caller guarantees `variants.len() == probs.len()` and probs sum
    /// to 1 (within f64 tolerance). Used only by snapshot deserialization.
    pub fn from_raw(variants: Vec<K>, probs: Vec<Decimal>, sample_count: u32) -> Self {
        Self {
            variants,
            probs,
            sample_count,
        }
    }

    /// Point mass on a specific variant. Used when we have a hard
    /// observation but want to lift it into the same type as our priors
    /// for KL divergence calculation.
    pub fn point_mass(variants: Vec<K>, observed: &K) -> Self {
        let mut probs = vec![Decimal::ZERO; variants.len()];
        if let Some(idx) = variants.iter().position(|v| v == observed) {
            probs[idx] = Decimal::ONE;
        }
        Self {
            variants,
            probs,
            sample_count: 1,
        }
    }

    /// Bayesian update with a single hard observation. Uses a Dirichlet
    /// concentration of 1 so the posterior behaves like empirical counts
    /// smoothed by a uniform prior. This is sufficient for Eden's
    /// operator-surface purpose; fancy priors can be added later.
    pub fn update(&mut self, observed: &K) {
        if let Some(idx) = self.variants.iter().position(|v| v == observed) {
            // Pseudo-count update: multiply by (n+1)/(n+K), add 1/(n+K) at idx.
            let n = Decimal::from(self.sample_count);
            let k = Decimal::from(self.variants.len() as i64);
            let denom = n + k;
            for (i, p) in self.probs.iter_mut().enumerate() {
                *p = (*p * (n + k - Decimal::ONE)
                    + if i == idx {
                        Decimal::ONE
                    } else {
                        Decimal::ZERO
                    })
                    / denom;
            }
        }
        self.sample_count = self.sample_count.saturating_add(1);
        self.renormalize();
    }

    /// Bayesian update with soft evidence. Each likelihood is `P(evidence | variant)`
    /// in the same order as `variants`. This is the categorical counterpart to
    /// Kalman's continuous update: callers can accumulate streaming evidence
    /// without collapsing the posterior into a hard label first.
    pub fn update_likelihoods(&mut self, likelihoods: &[Decimal]) -> bool {
        if likelihoods.len() != self.probs.len() {
            return false;
        }
        let floor = Decimal::try_from(VARIANCE_FLOOR).unwrap_or(Decimal::ZERO);
        for (prob, likelihood) in self.probs.iter_mut().zip(likelihoods.iter()) {
            let safe_likelihood = if *likelihood > floor {
                *likelihood
            } else {
                floor
            };
            *prob *= safe_likelihood;
        }
        self.sample_count = self.sample_count.saturating_add(1);
        self.renormalize();
        true
    }

    fn renormalize(&mut self) {
        let total: Decimal = self.probs.iter().copied().sum();
        if total <= Decimal::ZERO {
            let n = self.probs.len().max(1);
            let p = Decimal::ONE / Decimal::from(n as i64);
            for prob in self.probs.iter_mut() {
                *prob = p;
            }
            return;
        }
        for prob in self.probs.iter_mut() {
            *prob /= total;
        }
    }

    /// Most likely variant under current belief. Convenience for the rare
    /// case a consumer really wants a point classification.
    pub fn argmax(&self) -> Option<&K> {
        let mut best: Option<(usize, Decimal)> = None;
        for (i, p) in self.probs.iter().enumerate() {
            if best.map_or(true, |(_, best_p)| *p > best_p) {
                best = Some((i, *p));
            }
        }
        best.and_then(|(i, _)| self.variants.get(i))
    }

    /// Shannon entropy `-Σ p ln p`, in nats. `None` if the distribution is
    /// degenerate (will never happen post-normalize but we guard anyway).
    pub fn entropy(&self) -> Option<f64> {
        let mut acc = 0.0_f64;
        for p in &self.probs {
            let pf = p.to_f64()?;
            if pf > 0.0 {
                acc -= pf * pf.ln();
            }
        }
        Some(acc)
    }

    /// KL divergence `KL(self || other)` in nats. Treats zero probabilities
    /// under `self` as contributing 0 (standard convention: 0 ln 0 = 0).
    /// Returns `None` if `other` has a zero probability on a variant where
    /// `self` has positive mass (KL would be infinite).
    pub fn kl_divergence(&self, other: &CategoricalBelief<K>) -> Option<f64> {
        if self.variants != other.variants {
            return None;
        }
        let mut acc = 0.0_f64;
        for (p, q) in self.probs.iter().zip(other.probs.iter()) {
            let pf = p.to_f64()?;
            let qf = q.to_f64()?;
            if pf <= 0.0 {
                continue;
            }
            if qf <= 0.0 {
                return None;
            }
            acc += pf * (pf.ln() - qf.ln());
        }
        Some(acc)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn gaussian_from_first_sample_has_floor_variance_and_one_sample() {
        let b = GaussianBelief::from_first_sample(dec!(0.5));
        assert_eq!(b.mean, dec!(0.5));
        assert_eq!(b.sample_count, 1);
        assert!(b.variance.to_f64().unwrap() >= VARIANCE_FLOOR);
    }

    #[test]
    fn gaussian_welford_update_recovers_population_variance() {
        // Samples: 1, 2, 3, 4, 5 → mean 3, sample variance 2.5
        let mut b = GaussianBelief::from_first_sample(dec!(1));
        for v in [2, 3, 4, 5] {
            b.update(Decimal::from(v));
        }
        assert_eq!(b.mean, dec!(3));
        // Sample variance (unbiased, n-1 denominator) = 2.5
        let var = b.variance.to_f64().unwrap();
        assert!(
            (var - 2.5).abs() < 1e-6,
            "expected variance ≈ 2.5, got {}",
            var
        );
        assert_eq!(b.sample_count, 5);
    }

    #[test]
    fn gaussian_kl_is_zero_against_self_and_positive_otherwise() {
        let mut a = GaussianBelief::from_first_sample(dec!(0));
        let mut b = GaussianBelief::from_first_sample(dec!(0));
        for v in [1, 2, 3, 4] {
            a.update(Decimal::from(v));
            b.update(Decimal::from(v));
        }
        let kl_self = a.kl_divergence(&b).unwrap();
        assert!(
            kl_self.abs() < 1e-9,
            "KL(a || a) should be ~0, got {}",
            kl_self
        );

        // Now shift b's mean — KL should strictly increase.
        let mut c = GaussianBelief::from_first_sample(dec!(10));
        for v in [11, 12, 13, 14] {
            c.update(Decimal::from(v));
        }
        let kl_shifted = a.kl_divergence(&c).unwrap();
        assert!(
            kl_shifted > 1.0,
            "KL to shifted Gaussian should be large, got {}",
            kl_shifted
        );
    }

    #[test]
    fn gaussian_merge_equals_pooled_welford() {
        // Pool {1,2,3} and {4,5,6} → should match {1..6} directly.
        let mut a = GaussianBelief::from_first_sample(dec!(1));
        for v in [2, 3] {
            a.update(Decimal::from(v));
        }
        let mut b = GaussianBelief::from_first_sample(dec!(4));
        for v in [5, 6] {
            b.update(Decimal::from(v));
        }
        let mut merged = a.clone();
        merged.merge(&b);

        let mut direct = GaussianBelief::from_first_sample(dec!(1));
        for v in [2, 3, 4, 5, 6] {
            direct.update(Decimal::from(v));
        }

        assert_eq!(merged.sample_count, direct.sample_count);
        let m_mean = merged.mean.to_f64().unwrap();
        let d_mean = direct.mean.to_f64().unwrap();
        assert!(
            (m_mean - d_mean).abs() < 1e-9,
            "merged mean {} vs direct {}",
            m_mean,
            d_mean
        );
        let m_var = merged.variance.to_f64().unwrap();
        let d_var = direct.variance.to_f64().unwrap();
        assert!(
            (m_var - d_var).abs() < 1e-6,
            "merged variance {} vs direct {}",
            m_var,
            d_var
        );
    }

    #[test]
    fn gaussian_entropy_increases_with_variance() {
        let mut tight = GaussianBelief::from_first_sample(dec!(0));
        for v in [0, 0, 0, 0] {
            tight.update(Decimal::from(v));
        }
        let mut wide = GaussianBelief::from_first_sample(dec!(-10));
        for v in [-5, 0, 5, 10] {
            wide.update(Decimal::from(v));
        }
        let h_tight = tight.entropy().unwrap();
        let h_wide = wide.entropy().unwrap();
        assert!(
            h_wide > h_tight,
            "wider distribution should have higher entropy: tight {} vs wide {}",
            h_tight,
            h_wide
        );
    }

    #[test]
    fn categorical_uniform_entropy_is_ln_k() {
        let b: CategoricalBelief<&str> = CategoricalBelief::uniform(vec!["a", "b", "c", "d"]);
        let h = b.entropy().unwrap();
        assert!(
            (h - (4.0_f64).ln()).abs() < 1e-9,
            "uniform entropy over 4 should be ln 4, got {}",
            h
        );
    }

    #[test]
    fn categorical_update_concentrates_on_repeated_observation() {
        let mut b: CategoricalBelief<&str> = CategoricalBelief::uniform(vec!["a", "b", "c"]);
        for _ in 0..10 {
            b.update(&"a");
        }
        let a_idx = b.variants.iter().position(|v| *v == "a").unwrap();
        assert!(
            b.probs[a_idx] > dec!(0.5),
            "after 10 observations of 'a', P(a) should exceed 0.5, got {}",
            b.probs[a_idx]
        );
        assert_eq!(b.argmax(), Some(&"a"));
    }

    #[test]
    fn categorical_likelihood_update_accumulates_soft_evidence() {
        let mut b: CategoricalBelief<&str> = CategoricalBelief::uniform(vec!["a", "b", "c"]);
        for _ in 0..4 {
            assert!(b.update_likelihoods(&[dec!(0.85), dec!(0.10), dec!(0.05)]));
        }

        let a_idx = b.variants.iter().position(|v| *v == "a").unwrap();
        assert!(
            b.probs[a_idx] > dec!(0.95),
            "soft evidence should concentrate on 'a', got {}",
            b.probs[a_idx]
        );
        assert_eq!(b.argmax(), Some(&"a"));
    }

    #[test]
    fn categorical_kl_handles_zero_mass_correctly() {
        let variants = vec!["a", "b"];
        // P is all mass on a; Q is uniform → KL(P || Q) = ln 2.
        let p = CategoricalBelief::point_mass(variants.clone(), &"a");
        let q: CategoricalBelief<&str> = CategoricalBelief::uniform(variants.clone());
        let kl = p.kl_divergence(&q).unwrap();
        assert!(
            (kl - (2.0_f64).ln()).abs() < 1e-9,
            "KL(point || uniform) over 2 should be ln 2, got {}",
            kl
        );
        // Reverse direction is infinite because P has zero where Q has mass.
        // Actually: KL(uniform || point) diverges because the point-mass has
        // zero prob on "b" where uniform has 0.5. Our implementation returns
        // None in this case.
        assert!(q.kl_divergence(&p).is_none());
    }

    #[test]
    fn gaussian_uninformed_returns_none_for_kl_and_entropy() {
        let uninformed = GaussianBelief::uninformed(dec!(0));
        assert!(uninformed.entropy().is_none());
        let informed = GaussianBelief::from_first_sample(dec!(0));
        assert!(uninformed.kl_divergence(&informed).is_none());
        assert!(informed.kl_divergence(&uninformed).is_none());
    }
}
