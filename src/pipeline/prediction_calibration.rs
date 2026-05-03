//! Forward prediction + realized-surprise calibration loop.
//!
//! The classical eden surprise channel measures (observation_now vs
//! model_expectation_now) — that's an inline correction signal, not a
//! prediction-error signal. To know whether eden's beliefs ARE
//! predictive, we need:
//!
//!   1. At tick T, write a prediction for tick T+H beliefs.
//!   2. At tick T+H, read the prediction-from-T-back and compute KL
//!      divergence vs actual beliefs at T+H.
//!   3. Track per-symbol prediction quality over time → tell Y which
//!      eden signals are reliably predictive vs noise.
//!
//! This module is the **skeleton** of that loop. The prediction model is
//! deliberately naive (random walk: predict_T+H = belief_T), so KL
//! reflects how much beliefs actually drift over H ticks. A future
//! prediction model can swap in (e.g. velocity-extrapolated, learned
//! from historical signature → outcome) without changing the I/O shape.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::pipeline::loopy_bp::N_STATES;

/// Horizon in ticks for forward predictions. 5 is a balance between
/// giving the model time to be wrong and being short enough that we
/// learn fast. Tunable.
pub const PREDICTION_HORIZON_TICKS: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRecord {
    pub ts: DateTime<Utc>,
    pub market: String,
    /// Tick at which this prediction was made.
    pub tick_predicted_at: u64,
    /// Tick this prediction is for (always = tick_predicted_at + horizon).
    pub tick_target: u64,
    pub horizon: u64,
    /// Per-symbol predicted belief distribution (sum = 1).
    pub predictions: Vec<PredictedSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictedSymbol {
    pub symbol: String,
    pub belief: [f64; N_STATES],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealizationRecord {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick_realized: u64,
    pub tick_predicted_at: u64,
    pub horizon: u64,
    pub n_symbols: usize,
    pub mean_kl: f64,
    pub median_kl: f64,
    pub max_kl: f64,
    pub max_kl_symbol: Option<String>,
    /// Top-K symbols by absolute KL (most divergent — likely actionable).
    pub top_divergent: Vec<DivergentOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DivergentOutcome {
    pub symbol: String,
    pub predicted: [f64; N_STATES],
    pub actual: [f64; N_STATES],
    pub kl: f64,
}

/// Write naive-baseline predictions: predict belief at T+horizon =
/// belief at T (random-walk null model). Future better predictors (e.g.
/// velocity-extrapolated) can replace this body without changing the
/// on-disk schema.
pub fn write_predictions(
    market: &str,
    tick: u64,
    beliefs: &HashMap<String, [f64; N_STATES]>,
    horizon: u64,
) -> std::io::Result<()> {
    let predictions: Vec<PredictedSymbol> = beliefs
        .iter()
        .map(|(sym, belief)| PredictedSymbol {
            symbol: sym.clone(),
            belief: *belief,
        })
        .collect();
    if predictions.is_empty() {
        return Ok(());
    }
    let record = PredictionRecord {
        ts: Utc::now(),
        market: market.to_string(),
        tick_predicted_at: tick,
        tick_target: tick + horizon,
        horizon,
        predictions,
    };
    let path = format!(".run/eden-predictions-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(&record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Read the prediction made `horizon` ticks ago and compute realized
/// KL divergence against current beliefs. Writes a `RealizationRecord`
/// summary to the realized-predictions stream.
///
/// Returns `None` if no matching prediction exists yet (early ticks of
/// a fresh session).
pub fn realize_predictions(
    market: &str,
    current_tick: u64,
    current_beliefs: &HashMap<String, [f64; N_STATES]>,
    horizon: u64,
) -> Option<RealizationRecord> {
    if current_tick < horizon {
        return None;
    }
    let target_tick_predicted_at = current_tick - horizon;
    let path = format!(".run/eden-predictions-{}.ndjson", market);
    let path = std::path::Path::new(&path);

    // Tail-read recent predictions and find the one made
    // `target_tick_predicted_at`. Buffer 1 MB is enough for ~1000 ticks
    // worth of predictions even at 500 symbols/tick.
    let raw: Vec<PredictionRecord> = crate::live_snapshot::tail_records(path, 4 * 1024 * 1024, 200);
    let prediction = raw
        .into_iter()
        .find(|r| r.tick_predicted_at == target_tick_predicted_at)?;

    let predicted_map: HashMap<String, [f64; N_STATES]> = prediction
        .predictions
        .into_iter()
        .map(|p| (p.symbol, p.belief))
        .collect();

    let mut kl_per_symbol: Vec<(String, [f64; N_STATES], [f64; N_STATES], f64)> = Vec::new();
    for (sym, actual) in current_beliefs {
        if let Some(predicted) = predicted_map.get(sym) {
            let kl = kl_divergence(predicted, actual);
            kl_per_symbol.push((sym.clone(), *predicted, *actual, kl));
        }
    }
    if kl_per_symbol.is_empty() {
        return None;
    }

    let n = kl_per_symbol.len();
    let mut kl_values: Vec<f64> = kl_per_symbol.iter().map(|(_, _, _, kl)| *kl).collect();
    kl_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean_kl = kl_values.iter().sum::<f64>() / n as f64;
    let median_kl = kl_values[n / 2];
    let max_kl = *kl_values.last().unwrap_or(&0.0);

    let mut top_divergent_sorted = kl_per_symbol.clone();
    top_divergent_sorted.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let max_kl_symbol = top_divergent_sorted.first().map(|(s, _, _, _)| s.clone());
    let top_divergent: Vec<DivergentOutcome> = top_divergent_sorted
        .into_iter()
        .take(15)
        .map(|(symbol, predicted, actual, kl)| DivergentOutcome {
            symbol,
            predicted,
            actual,
            kl,
        })
        .collect();

    let record = RealizationRecord {
        ts: Utc::now(),
        market: market.to_string(),
        tick_realized: current_tick,
        tick_predicted_at: target_tick_predicted_at,
        horizon,
        n_symbols: n,
        mean_kl,
        median_kl,
        max_kl,
        max_kl_symbol,
        top_divergent,
    };

    let realized_path = format!(".run/eden-prediction-realized-{}.ndjson", market);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&realized_path)
    {
        if let Ok(line) = serde_json::to_string(&record) {
            let _ = writeln!(file, "{}", line);
        }
    }

    Some(record)
}

/// KL divergence D_KL(p || q) for two discrete distributions.
/// Returns 0.0 when distributions match exactly. Uses small epsilon to
/// avoid log(0).
fn kl_divergence(p: &[f64; N_STATES], q: &[f64; N_STATES]) -> f64 {
    let eps = 1e-12;
    let mut kl = 0.0;
    for i in 0..N_STATES {
        let pi = p[i].max(eps);
        let qi = q[i].max(eps);
        kl += pi * (pi / qi).ln();
    }
    kl.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kl_zero_for_identical_distributions() {
        let p = [0.5, 0.3, 0.2];
        let q = [0.5, 0.3, 0.2];
        assert!(kl_divergence(&p, &q).abs() < 1e-9);
    }

    #[test]
    fn kl_positive_for_different_distributions() {
        let p = [0.7, 0.2, 0.1];
        let q = [0.3, 0.3, 0.4];
        assert!(kl_divergence(&p, &q) > 0.1);
    }

    #[test]
    fn realize_returns_none_before_horizon() {
        let beliefs: HashMap<String, [f64; N_STATES]> = HashMap::new();
        // tick=2 with horizon=5 → can't realize anything
        let result = realize_predictions("test_market_zzz", 2, &beliefs, 5);
        assert!(result.is_none());
    }
}
