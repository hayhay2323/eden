//! Mutual information and information-gain queries built on top of
//! `pipeline::belief`. This is the "掌握權限" math — given a prior and a
//! candidate observation, how many nats of uncertainty would acquiring
//! that observation remove?
//!
//! Operator use: on every tick, rank candidate observations (which
//! symbol's pressure to look at, which option chain to query, which
//! cross-market pair to watch) by expected information gain. The top-k
//! go into `wake.reasons` as "attention_priority: …" lines.
//!
//! This module provides two closed-form cases plus a ranking helper:
//!
//!   1. Gaussian-over-Gaussian — hidden state and observation both
//!      continuous. Models the observation as `H + noise` with known
//!      noise variance; `I(H;O) = 0.5 ln(1 + σ²_H / σ²_N)`.
//!
//!   2. Categorical-over-Gaussian — hidden state discrete (regime,
//!      state_kind), observation continuous per variant. The mixture
//!      marginal has no closed-form entropy, so we use the
//!      moment-matched Gaussian upper bound, which gives a *lower*
//!      bound on mutual information — conservative for operator ranking
//!      ("would definitely learn at least this much").
//!
//! The lower-bound path means: ranking can be trusted even though the
//! absolute numbers are slightly pessimistic. Good for ordering, not for
//! precise information accounting.

use rust_decimal::prelude::ToPrimitive;

use crate::pipeline::belief::{CategoricalBelief, GaussianBelief};

/// Floor on observation-noise variance. Zero-noise observations would
/// produce infinite MI; we clamp so rankings stay finite and well-defined.
const NOISE_VARIANCE_FLOOR: f64 = 1.0e-6;

/// Closed-form mutual information between a Gaussian prior over the
/// hidden state `H` and a Gaussian observation `O = H + noise`.
///
/// Returns `None` if the prior is uninformed (variance not yet estimable).
///
/// Intuition: if the prior is already confident (`σ²_H` small) or the
/// observation is noisy (`σ²_N` large), the observation contributes
/// little. Large prior uncertainty with low observation noise = big win.
pub fn gaussian_over_gaussian(
    prior: &GaussianBelief,
    observation_noise_variance: f64,
) -> Option<f64> {
    if !prior.is_informed() {
        return None;
    }
    let var_h = prior.variance.to_f64()?;
    let var_n = observation_noise_variance.max(NOISE_VARIANCE_FLOOR);
    if var_h <= 0.0 {
        return Some(0.0);
    }
    Some(0.5 * (1.0 + var_h / var_n).ln())
}

/// Mutual information between a categorical prior over hidden state and
/// a continuous observation whose distribution per variant is Gaussian.
///
/// Model: `P(H = h) = prior.probs[i]`, `O | H = h ~ N(μ_h, σ²_h)` given
/// by `conditional_observations[i]`. All variant distributions share
/// dimension (scalar observation).
///
/// Algorithm:
///   `I(H;O) = H(O) − H(O|H)`  (in nats)
///   `H(O|H) = Σ_h P(h) · H(N(μ_h, σ²_h))`
///   `H(O)`  ≈ H of moment-matched Gaussian (upper bound ⇒ lower bound on MI)
///
/// Returns `None` if:
///   - the prior has no variants
///   - `conditional_observations` length mismatches the prior
///   - any conditional belief is uninformed (can't estimate its variance)
pub fn categorical_over_gaussian<K>(
    prior: &CategoricalBelief<K>,
    conditional_observations: &[GaussianBelief],
) -> Option<f64>
where
    K: Clone + Eq,
{
    if prior.variants.is_empty() || prior.variants.len() != conditional_observations.len() {
        return None;
    }
    let mut conditional_entropy = 0.0_f64;
    let mut mix_mean = 0.0_f64;
    let mut mix_second_moment = 0.0_f64;
    for (prob, obs) in prior.probs.iter().zip(conditional_observations.iter()) {
        let p = prob.to_f64()?;
        if p <= 0.0 {
            continue;
        }
        let h = obs.entropy()?;
        conditional_entropy += p * h;
        let mu = obs.mean.to_f64()?;
        let var = obs.variance.to_f64()?;
        mix_mean += p * mu;
        mix_second_moment += p * (mu * mu + var);
    }
    // Variance of the mixture via law of total variance: E[O²] − E[O]².
    let mix_variance = (mix_second_moment - mix_mean * mix_mean).max(NOISE_VARIANCE_FLOOR);
    // Moment-matched Gaussian has entropy 0.5·ln(2πe·σ²). Using LN_2PIE
    // constant locally to avoid re-exporting from belief.rs.
    const LN_2PIE: f64 = 2.837_877_066_409_345_5;
    let marginal_entropy_upper_bound = 0.5 * (LN_2PIE + mix_variance.ln());
    let mi_lower_bound = marginal_entropy_upper_bound - conditional_entropy;
    // Negative MI is an artefact of the moment-matching bound when the
    // conditionals are nearly identical; clamp at 0 for monotone ranking.
    Some(mi_lower_bound.max(0.0))
}

/// Rank a set of candidate observations by their information gain about a
/// given Gaussian hidden state. Each candidate is identified by a string
/// name and characterized by its observation noise variance.
///
/// Returns `Vec<(name, info_gain_in_nats)>` sorted descending. Unranked
/// (informed-prior fails) candidates are dropped silently.
pub fn rank_candidates_gaussian_prior(
    prior: &GaussianBelief,
    candidates: &[(String, f64)],
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = candidates
        .iter()
        .filter_map(|(name, noise_var)| {
            gaussian_over_gaussian(prior, *noise_var).map(|gain| (name.clone(), gain))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

/// Rank candidates against a categorical prior. Each candidate has its
/// own observation model: a Gaussian belief per variant of the hidden
/// state. Candidates whose model length doesn't match the prior, or
/// whose conditional beliefs aren't informed, are dropped.
pub fn rank_candidates_categorical_prior<K: Clone + Eq>(
    prior: &CategoricalBelief<K>,
    candidates: &[(String, Vec<GaussianBelief>)],
) -> Vec<(String, f64)> {
    let mut ranked: Vec<(String, f64)> = candidates
        .iter()
        .filter_map(|(name, obs_per_variant)| {
            categorical_over_gaussian(prior, obs_per_variant).map(|gain| (name.clone(), gain))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn informed_gaussian(values: &[i64]) -> GaussianBelief {
        let mut b = GaussianBelief::from_first_sample(Decimal::from(values[0]));
        for v in &values[1..] {
            b.update(Decimal::from(*v));
        }
        b
    }

    #[test]
    fn gaussian_mi_higher_when_prior_uncertainty_is_higher() {
        // Two priors with same mean, different variance.
        let tight = informed_gaussian(&[0, 0, 0, 0, 0, 0]);
        let wide = informed_gaussian(&[-10, -5, 0, 5, 10, -8, 7]);
        let mi_tight = gaussian_over_gaussian(&tight, 1.0).unwrap();
        let mi_wide = gaussian_over_gaussian(&wide, 1.0).unwrap();
        assert!(
            mi_wide > mi_tight,
            "wider prior should give larger MI: tight={} wide={}",
            mi_tight,
            mi_wide
        );
    }

    #[test]
    fn gaussian_mi_lower_when_observation_is_noisy() {
        let prior = informed_gaussian(&[1, 2, 3, 4, 5]);
        let mi_clean = gaussian_over_gaussian(&prior, 0.01).unwrap();
        let mi_noisy = gaussian_over_gaussian(&prior, 100.0).unwrap();
        assert!(
            mi_clean > mi_noisy,
            "clean observation should give larger MI: clean={} noisy={}",
            mi_clean,
            mi_noisy
        );
    }

    #[test]
    fn gaussian_mi_returns_none_for_uninformed_prior() {
        let prior = GaussianBelief::uninformed(dec!(0));
        assert!(gaussian_over_gaussian(&prior, 1.0).is_none());
    }

    #[test]
    fn categorical_mi_zero_when_conditional_distributions_identical() {
        // Two states, both produce observations with identical mean/variance.
        // Observation gives no information about which state is active.
        let variants = vec!["A", "B"];
        let prior: CategoricalBelief<&str> = CategoricalBelief::uniform(variants);
        let obs_a = informed_gaussian(&[0, 1, 2, 3, 4, 5]);
        let obs_b = informed_gaussian(&[0, 1, 2, 3, 4, 5]);
        let mi = categorical_over_gaussian(&prior, &[obs_a, obs_b]).unwrap();
        assert!(
            mi < 0.05,
            "identical conditional distributions should give near-zero MI, got {}",
            mi
        );
    }

    #[test]
    fn categorical_mi_positive_when_conditional_distributions_differ() {
        // Two well-separated clusters: observation should reveal which state.
        let variants = vec!["Low", "High"];
        let prior: CategoricalBelief<&str> = CategoricalBelief::uniform(variants);
        let obs_low = informed_gaussian(&[-5, -4, -5, -6, -5, -4]);
        let obs_high = informed_gaussian(&[5, 6, 5, 4, 5, 6]);
        let mi = categorical_over_gaussian(&prior, &[obs_low, obs_high]).unwrap();
        assert!(
            mi > 0.3,
            "well-separated conditionals should give substantial MI, got {}",
            mi
        );
    }

    #[test]
    fn categorical_mi_mismatch_returns_none() {
        let prior: CategoricalBelief<&str> = CategoricalBelief::uniform(vec!["A", "B", "C"]);
        let mi = categorical_over_gaussian(&prior, &[informed_gaussian(&[1, 2])]);
        assert!(mi.is_none(), "length mismatch must return None");
    }

    #[test]
    fn ranking_orders_candidates_by_gain() {
        let prior = informed_gaussian(&[-2, -1, 0, 1, 2]);
        let candidates = vec![
            ("noisy_observation".to_string(), 100.0),
            ("clean_observation".to_string(), 0.1),
            ("mid_observation".to_string(), 1.0),
        ];
        let ranked = rank_candidates_gaussian_prior(&prior, &candidates);
        assert_eq!(ranked[0].0, "clean_observation");
        assert_eq!(ranked[2].0, "noisy_observation");
        assert!(ranked[0].1 > ranked[1].1 && ranked[1].1 > ranked[2].1);
    }

    #[test]
    fn ranking_drops_uninformed_priors() {
        let prior = GaussianBelief::uninformed(dec!(0));
        let candidates = vec![("a".to_string(), 1.0)];
        let ranked = rank_candidates_gaussian_prior(&prior, &candidates);
        assert!(ranked.is_empty());
    }
}
