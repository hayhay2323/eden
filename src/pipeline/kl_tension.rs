//! KL-divergence based vortex tension — Friston-style surprise.
//!
//! Eden's `PressureVortex.tension` currently stores the scalar
//! `|tick_direction − hour_direction|`: a simple "how far has the
//! short-term drifted from the long-term". That captures shift but not
//! the *scale* of that shift relative to recent volatility. Two symbols
//! both showing tick−hour = 0.3 can mean radically different things —
//! 0.3 on a normally quiet symbol is huge; 0.3 on a chaotic symbol is
//! background noise.
//!
//! This module adds the properly-scaled alternative: treat the hour
//! layer as a running Gaussian belief (what we *expected*), the tick
//! layer as a point observation (what we *saw*), and report the KL
//! divergence. KL is dimensionally meaningful (nats of surprise),
//! auto-scales with recent variance, and matches the Free-Energy
//! Principle framing where surprise drives perception updates.
//!
//! Non-goals: we do NOT modify `PressureVortex.tension` or the existing
//! vortex detection code path. This is a parallel computation that
//! downstream consumers can invoke when they have access to a pressure
//! history. Wiring it into the vortex surface is a separate task after
//! live validation.

use rust_decimal::Decimal;

use crate::pipeline::belief::GaussianBelief;

/// Compute KL-based tension from histories of tick-layer and hour-layer
/// composite pressures.
///
/// - `tick_history`: most recent N tick-layer composites (e.g. last 5–10)
/// - `hour_history`: most recent M hour-layer composites (M >> N; e.g. last 30+)
///
/// Treats hour_history as the "prior expectation" (wider, steadier) and
/// tick_history as "the new observation" (narrow, recent). Returns the
/// KL divergence `KL(tick_belief || hour_belief)` in nats — the surprise
/// the hour-layer prior feels when it sees the tick-layer observation.
///
/// Returns `None` if either history is too short to estimate variance
/// (< BELIEF_INFORMED_MIN_SAMPLES samples).
///
/// Interpretation: KL ≈ 0 means tick layer is living inside the hour
/// belief (quiet). KL > 1 means the tick layer is a multi-sigma
/// displacement from the hour prior (the vortex moment).
pub fn kl_tension_from_histories(
    tick_history: &[Decimal],
    hour_history: &[Decimal],
) -> Option<f64> {
    let tick_belief = build_belief_from_history(tick_history)?;
    let hour_belief = build_belief_from_history(hour_history)?;
    tick_belief.kl_divergence(&hour_belief)
}

/// Scale-aware temporal divergence: signed shift between tick and hour
/// means, normalized by hour-layer standard deviation. This is
/// complementary to KL — KL gives magnitude of surprise without sign,
/// z_score gives the direction. Together they fully characterize the
/// vortex in a statistically grounded way.
///
/// Returns `None` if hour_history is too short or has zero variance.
pub fn tension_z_score(tick_history: &[Decimal], hour_history: &[Decimal]) -> Option<f64> {
    let tick_belief = build_belief_from_history(tick_history)?;
    let hour_belief = build_belief_from_history(hour_history)?;
    use rust_decimal::prelude::ToPrimitive;
    let tick_mean = tick_belief.mean.to_f64()?;
    let hour_mean = hour_belief.mean.to_f64()?;
    let hour_var = hour_belief.variance.to_f64()?;
    if hour_var <= 0.0 {
        return None;
    }
    Some((tick_mean - hour_mean) / hour_var.sqrt())
}

/// Build a GaussianBelief from a scalar history. Returned belief is
/// informed if `samples.len() >= BELIEF_INFORMED_MIN_SAMPLES`, else None.
fn build_belief_from_history(samples: &[Decimal]) -> Option<GaussianBelief> {
    if samples.is_empty() {
        return None;
    }
    let mut belief = GaussianBelief::from_first_sample(samples[0]);
    for value in &samples[1..] {
        belief.update(*value);
    }
    if belief.is_informed() {
        Some(belief)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn history(values: &[f64]) -> Vec<Decimal> {
        values
            .iter()
            .map(|v| Decimal::try_from(*v).unwrap())
            .collect()
    }

    #[test]
    fn kl_is_small_when_tick_matches_hour_distribution() {
        // Both layers sampled from similar distribution
        let tick = history(&[0.1, 0.05, 0.12, 0.08, 0.09, 0.10]);
        let hour = history(&[
            0.1, 0.12, 0.08, 0.11, 0.09, 0.10, 0.13, 0.07, 0.11, 0.09, 0.12, 0.08,
        ]);
        let kl = kl_tension_from_histories(&tick, &hour).unwrap();
        assert!(
            kl.abs() < 0.5,
            "KL should be small when tick lives inside hour belief, got {}",
            kl
        );
    }

    #[test]
    fn kl_is_large_when_tick_shifts_far_from_hour() {
        // Hour layer hovering around 0, tick layer suddenly at 2.0
        let tick = history(&[2.0, 2.1, 1.9, 2.0, 2.1, 1.95]);
        let hour = history(&[
            0.0, 0.05, -0.05, 0.02, 0.01, -0.02, 0.03, 0.0, -0.01, 0.04, 0.01, 0.0,
        ]);
        let kl = kl_tension_from_histories(&tick, &hour).unwrap();
        assert!(
            kl > 10.0,
            "KL should be large when tick is many sigma from hour prior, got {}",
            kl
        );
    }

    #[test]
    fn kl_returns_none_when_history_too_short() {
        let tick = history(&[0.1, 0.2]);
        let hour = history(&[0.0, 0.05, 0.02, 0.03, 0.01, 0.04]);
        assert!(kl_tension_from_histories(&tick, &hour).is_none());
    }

    #[test]
    fn z_score_signs_with_drift_direction() {
        let hour = history(&[0.0, 0.01, -0.01, 0.0, 0.01, -0.01, 0.005, 0.0]);
        let tick_up = history(&[0.5, 0.55, 0.48, 0.52, 0.50, 0.54]);
        let tick_dn = history(&[-0.5, -0.48, -0.52, -0.55, -0.49, -0.51]);
        let z_up = tension_z_score(&tick_up, &hour).unwrap();
        let z_dn = tension_z_score(&tick_dn, &hour).unwrap();
        assert!(
            z_up > 0.0,
            "upward drift should yield positive z, got {}",
            z_up
        );
        assert!(
            z_dn < 0.0,
            "downward drift should yield negative z, got {}",
            z_dn
        );
        // Magnitudes should be comparable because drifts are symmetric.
        assert!(
            (z_up.abs() - z_dn.abs()).abs() < 2.0,
            "symmetric drifts should yield similar |z|: up={} dn={}",
            z_up,
            z_dn
        );
    }

    #[test]
    fn kl_auto_scales_with_hour_layer_volatility() {
        // Same tick displacement (both 0.3 above hour mean), but one hour
        // layer is calm and the other is noisy. KL should flag the calm
        // one as surprising and the noisy one as quiet.
        let tick = history(&[0.3, 0.32, 0.28, 0.31, 0.30, 0.29]);
        let calm_hour = history(&[
            0.0, 0.005, -0.005, 0.003, 0.001, -0.002, 0.002, 0.0, -0.001, 0.003, 0.001, -0.001,
        ]);
        let noisy_hour = history(&[
            0.0, 0.5, -0.4, 0.6, -0.5, 0.3, -0.6, 0.5, -0.3, 0.4, -0.5, 0.2,
        ]);
        let kl_calm = kl_tension_from_histories(&tick, &calm_hour).unwrap();
        let kl_noisy = kl_tension_from_histories(&tick, &noisy_hour).unwrap();
        assert!(
            kl_calm > kl_noisy,
            "same displacement should feel MORE surprising against calm prior: calm={} noisy={}",
            kl_calm,
            kl_noisy
        );
    }
}
