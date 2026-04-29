//! Universal structural consistency gauge.
//!
//! First-principles: physical systems have equations of state — relations
//! between observable quantities that hold when the system is in "normal"
//! dynamics. When these relations break, some hidden force is at work.
//!
//! For markets, graph-defined relationships:
//!   - Volume × |Price velocity|  correlate   (flow → move equivalence)
//!   - Broker role distribution   unimodal   (identity is stable)
//!   - Broker pair co-occurrence  independent (decisions uncorrelated)
//!   - Many more: DepthAsymmetry × TradeTape, PressureCapitalFlow × IntentAccum,
//!     PriceVelocity × SectorRelStrength, ...
//!
//! When these relations BREAK on a specific entity, it's a structural signal:
//!   - Volume high + Price stable         = stealth accumulation
//!   - Broker on bid AND ask              = role switch (hiding identity)
//!   - Broker pair always co-present      = institutional footprint
//!
//! The DETECTION primitive is universal: **outlier from the current
//! universe distribution**. No patterns, no learned thresholds, no
//! if-else per case. Caller specifies WHICH quantities define the
//! relationship (ontological choice); gauge detects WHICH entities
//! violate it (statistical primitive).
//!
//! Two levels:
//!   - `outliers_1d`: single metric, tail of distribution = outliers.
//!   - `residuals_2d`: two metrics, linear regression, large residuals
//!     = entities violating the expected correlation.
//!
//! Output: `.run/eden-consistency-{market}.ndjson`, one line per event.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Percentile threshold for calling an entity an outlier.
/// 0.99 = top 1% of |metric| or |residual|.
pub const OUTLIER_PERCENTILE: f64 = 0.99;

/// Minimum sample size before the gauge returns anything. Below this,
/// the distribution is too sparse to define "typical".
pub const MIN_SAMPLES: usize = 30;

#[derive(Debug, Clone, Serialize)]
pub struct ConsistencyEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    /// Human-readable label of the relationship being checked
    /// (e.g., "volume_price_decoupling", "broker_role_entropy").
    pub relationship: String,
    pub entity: String,
    /// The metric value(s) observed on this entity.
    pub metric_primary: f64,
    /// For 2D gauges: the second metric.
    pub metric_secondary: Option<f64>,
    /// Expected value given the universe-wide relationship (0 for 1D).
    pub expected: f64,
    /// |observed − expected| (or just |observed| for 1D absolute outliers).
    pub residual: f64,
    /// Noise floor of |residual| at this tick (for context).
    pub floor: f64,
}

/// 1D outlier detection: |metric| above percentile of universe distribution.
/// Use for: broker role entropy, broker pair MI, etc.
pub fn outliers_1d(
    market: &str,
    relationship: &str,
    values: &[(String, f64)],
    percentile: f64,
    ts: DateTime<Utc>,
) -> Vec<ConsistencyEvent> {
    if values.len() < MIN_SAMPLES {
        return Vec::new();
    }
    let mut abs_vals: Vec<f64> = values.iter().map(|(_, v)| v.abs()).collect();
    abs_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor_idx = percentile_floor_index(percentile, abs_vals.len());
    let floor = abs_vals
        .get(floor_idx.min(abs_vals.len() - 1))
        .copied()
        .unwrap_or(0.0);

    values
        .iter()
        .filter_map(|(entity, v)| {
            if v.abs() > floor {
                Some(ConsistencyEvent {
                    ts,
                    market: market.to_string(),
                    relationship: relationship.to_string(),
                    entity: entity.clone(),
                    metric_primary: *v,
                    metric_secondary: None,
                    expected: 0.0,
                    residual: v.abs(),
                    floor,
                })
            } else {
                None
            }
        })
        .collect()
}

/// 2D residual outliers: linear regression y = a*x + b across universe,
/// then identify entities whose residual |y − (a*x + b)| exceeds the
/// universe percentile. Use for: volume × price velocity (stealth),
/// capital_flow × intent_accumulation (belief mismatch), etc.
pub fn residuals_2d(
    market: &str,
    relationship: &str,
    pairs: &[(String, f64, f64)],
    percentile: f64,
    ts: DateTime<Utc>,
) -> Vec<ConsistencyEvent> {
    if pairs.len() < MIN_SAMPLES {
        return Vec::new();
    }
    let n = pairs.len() as f64;
    let sum_x: f64 = pairs.iter().map(|(_, x, _)| x).sum();
    let sum_y: f64 = pairs.iter().map(|(_, _, y)| y).sum();
    let sum_xx: f64 = pairs.iter().map(|(_, x, _)| x * x).sum();
    let sum_xy: f64 = pairs.iter().map(|(_, x, y)| x * y).sum();
    let mean_x = sum_x / n;
    let mean_y = sum_y / n;
    let denom = sum_xx - n * mean_x * mean_x;
    let a = if denom.abs() < 1e-12 {
        0.0
    } else {
        (sum_xy - n * mean_x * mean_y) / denom
    };
    let b = mean_y - a * mean_x;

    // Compute residuals
    let mut with_residuals: Vec<(String, f64, f64, f64, f64)> = pairs
        .iter()
        .map(|(e, x, y)| {
            let expected = a * x + b;
            let residual = y - expected;
            (e.clone(), *x, *y, expected, residual)
        })
        .collect();

    // Noise floor from |residual| distribution
    let mut abs_res: Vec<f64> = with_residuals
        .iter()
        .map(|(_, _, _, _, r)| r.abs())
        .collect();
    abs_res.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor_idx = percentile_floor_index(percentile, abs_res.len());
    let floor = abs_res
        .get(floor_idx.min(abs_res.len() - 1))
        .copied()
        .unwrap_or(0.0);

    with_residuals
        .drain(..)
        .filter_map(|(entity, x, y, expected, residual)| {
            if residual.abs() > floor {
                Some(ConsistencyEvent {
                    ts,
                    market: market.to_string(),
                    relationship: relationship.to_string(),
                    entity,
                    metric_primary: x,
                    metric_secondary: Some(y),
                    expected,
                    residual: residual.abs(),
                    floor,
                })
            } else {
                None
            }
        })
        .collect()
}

fn percentile_floor_index(percentile: f64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let rank = (percentile.clamp(0.0, 1.0) * len as f64).ceil() as usize;
    rank.saturating_sub(1).min(len - 1)
}

pub fn write_events(market: &str, events: &[ConsistencyEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-consistency-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    for ev in events {
        let line = serde_json::to_string(ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

// ---------------- Metric extractors (first-principles primitives) ----------------

/// Shannon entropy of a discrete probability distribution.
pub fn entropy(probs: &[f64]) -> f64 {
    let mut h = 0.0;
    for p in probs {
        if *p > 0.0 {
            h -= p * p.ln();
        }
    }
    h
}

/// Mutual information estimator for two binary events across N observations.
/// p_a = P(A occurred), p_b = P(B occurred), p_ab = P(both).
/// MI = p_ab * log(p_ab / (p_a * p_b)) + ... (4-term formula)
pub fn mutual_information_binary(p_a: f64, p_b: f64, p_ab: f64) -> f64 {
    let p_a_only = p_a - p_ab; // A occurred but not B
    let p_b_only = p_b - p_ab; // B occurred but not A
    let p_neither = 1.0 - p_a - p_b + p_ab;

    let term = |p: f64, marg_a: f64, marg_b: f64| -> f64 {
        if p < 1e-12 || marg_a < 1e-12 || marg_b < 1e-12 {
            0.0
        } else {
            p * (p / (marg_a * marg_b)).ln()
        }
    };
    term(p_ab, p_a, p_b)
        + term(p_a_only, p_a, 1.0 - p_b)
        + term(p_b_only, 1.0 - p_a, p_b)
        + term(p_neither, 1.0 - p_a, 1.0 - p_b)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outliers_1d_uniform_returns_nothing() {
        let vals: Vec<(String, f64)> = (0..100).map(|i| (format!("E{}", i), 1.0)).collect();
        let evs = outliers_1d("hk", "test", &vals, 0.99, Utc::now());
        assert!(evs.is_empty());
    }

    #[test]
    fn outliers_1d_tail_survives() {
        let mut vals: Vec<(String, f64)> = (0..100).map(|i| (format!("E{}", i), 1.0)).collect();
        vals[0] = ("E0".into(), 100.0);
        let evs = outliers_1d("hk", "test", &vals, 0.99, Utc::now());
        assert!(!evs.is_empty());
        assert_eq!(evs[0].entity, "E0");
    }

    #[test]
    fn outliers_1d_respects_min_samples() {
        let vals: Vec<(String, f64)> = (0..5).map(|i| (format!("E{}", i), i as f64)).collect();
        let evs = outliers_1d("hk", "test", &vals, 0.99, Utc::now());
        assert!(evs.is_empty(), "should skip when fewer than MIN_SAMPLES");
    }

    #[test]
    fn residuals_2d_perfect_line_nothing_fires() {
        // y = 2x exactly for all; all residuals = 0
        let pairs: Vec<(String, f64, f64)> = (0..100)
            .map(|i| (format!("E{}", i), i as f64, (i * 2) as f64))
            .collect();
        let evs = residuals_2d("hk", "test", &pairs, 0.99, Utc::now());
        assert!(evs.is_empty());
    }

    #[test]
    fn residuals_2d_outlier_off_line_fires() {
        // Most: y = 2x, but E0 has y = 100 (massive outlier)
        let mut pairs: Vec<(String, f64, f64)> = (0..100)
            .map(|i| (format!("E{}", i), i as f64, (i * 2) as f64))
            .collect();
        pairs[0] = ("E0".into(), 0.0, 1000.0); // y far from y = 2x
        let evs = residuals_2d("hk", "test", &pairs, 0.99, Utc::now());
        assert!(!evs.is_empty());
        assert!(evs.iter().any(|e| e.entity == "E0"));
    }

    #[test]
    fn entropy_uniform_max() {
        let probs = vec![0.25, 0.25, 0.25, 0.25];
        let h = entropy(&probs);
        assert!((h - 4.0_f64.ln()).abs() < 1e-9);
    }

    #[test]
    fn entropy_point_mass_zero() {
        let probs = vec![1.0, 0.0, 0.0];
        let h = entropy(&probs);
        assert!(h.abs() < 1e-9);
    }

    #[test]
    fn mi_independent_is_zero() {
        // If P(A) = 0.5, P(B) = 0.5, P(AB) = 0.25 = P(A) * P(B) → MI = 0
        let mi = mutual_information_binary(0.5, 0.5, 0.25);
        assert!(mi.abs() < 1e-9);
    }

    #[test]
    fn mi_perfect_correlation_positive() {
        // A always when B: P(A)=P(B)=P(AB)=0.5
        let mi = mutual_information_binary(0.5, 0.5, 0.5);
        assert!(mi > 0.5); // max possible = ln(2) ≈ 0.69
    }
}
