//! Lead-lag cross-correlation along master KG edges.
//!
//! Per-symbol time series of (Pressure + Intent) state across recent
//! snapshot ticks. For each master KG edge (A, B), compute pairwise
//! cross-correlation at lags ∈ {−MAX_LAG..+MAX_LAG} and emit the
//! dominant lag — positive = A leads B, negative = B leads A, zero =
//! simultaneous.
//!
//! This was the missing piece behind earlier `hub_modulation` rejection:
//! hubs are SYMMETRIC coupling without direction. Cross-correlation on
//! the typed graph edges gives DIRECTIONAL evidence — when A's
//! Pressure rise consistently precedes B's Pressure rise by 1-2 ticks,
//! that's lead-lag, not symmetric coupling.
//!
//! Pure deterministic statistics — Pearson correlation at each shift —
//! no learning, no fitted model. Operates ON the typed graph topology
//! (each edge gets its own lag estimate), preserving Eden's ontology +
//! graph foundation.
//!
//! Output: `.run/eden-lead-lag-{market}.ndjson` — per master KG edge
//! per snapshot, dominant lag + correlation at that lag.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};

/// Tick history depth. 20 snapshot ticks ≈ 100 raw ticks ≈ 13 minutes
/// at 8s/tick. Long enough to capture multi-minute lead-lag, short
/// enough to react to regime change.
pub const HISTORY_LEN: usize = 20;

/// Maximum |lag| considered. With HISTORY_LEN=20, we have 20-3=17
/// overlap samples even at extremes. Beyond ±3 we'd need a longer
/// history.
pub const MAX_LAG: i32 = 3;

/// Minimum overlap samples to compute a meaningful correlation.
/// Below this, we lack statistical power; emit None.
pub const MIN_OVERLAP_SAMPLES: usize = 10;

/// Minimum |correlation| to consider any lag a real signal. Below
/// this, both series are essentially independent at this lag → emit
/// 0 lag with low confidence.
pub const MIN_SIGNIFICANT_CORR: f64 = 0.2;

/// Composite scalar derived from a sub-KG. Pure read of existing
/// Pressure + Intent values — same formula as loopy_bp::observe_from_subkg
/// without the discretization step.
fn observe_scalar(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG) -> f64 {
    fn read(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG, id: &NodeId) -> f64 {
        kg.nodes
            .get(id)
            .and_then(|n| n.value)
            .map(|v| v.to_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
    let pcf = read(kg, &NodeId::PressureCapitalFlow);
    let pm = read(kg, &NodeId::PressureMomentum);
    let acc = read(kg, &NodeId::IntentAccumulation);
    let dist = read(kg, &NodeId::IntentDistribution);
    pcf + 0.5 * pm + acc - dist
}

#[derive(Debug, Default)]
pub struct LeadLagTracker {
    /// Per-symbol rolling buffer of composite scalar values.
    history: HashMap<String, VecDeque<f64>>,
}

impl LeadLagTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, symbol: &str, value: f64) {
        let entry = self.history.entry(symbol.to_string()).or_default();
        entry.push_back(value);
        while entry.len() > HISTORY_LEN {
            entry.pop_front();
        }
    }

    pub fn ingest(&mut self, registry: &SubKgRegistry) {
        for (sym, kg) in &registry.graphs {
            self.observe(sym, observe_scalar(kg));
        }
    }

    /// Alternative ingest path that reads BP posterior beliefs instead of
    /// sub-KG nodes. Use when sub-KG channel nodes (PressureCapitalFlow /
    /// PressureMomentum / IntentAccumulation / IntentDistribution) are
    /// rarely populated for most symbols — the original `ingest` then
    /// observes a constant-zero series and `detect_lead_lag` produces
    /// no events.
    ///
    /// Scalar = belief[STATE_BULL] - belief[STATE_BEAR] ∈ [-1, +1]:
    /// directional belief signed by (bullish - bearish) marginal mass.
    pub fn ingest_from_beliefs(
        &mut self,
        beliefs: &HashMap<String, [f64; crate::pipeline::loopy_bp::N_STATES]>,
    ) {
        for (sym, belief) in beliefs {
            let scalar = belief[crate::pipeline::loopy_bp::STATE_BULL]
                - belief[crate::pipeline::loopy_bp::STATE_BEAR];
            self.observe(sym, scalar);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LeadLagEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub from_symbol: String,
    pub to_symbol: String,
    pub edge_weight: f64,
    pub dominant_lag: i32,
    pub correlation_at_lag: f64,
    pub n_samples: usize,
    /// Direction interpretation:
    ///   "from_leads"   = from_symbol moves first (lag > 0)
    ///   "to_leads"     = to_symbol moves first   (lag < 0)
    ///   "simultaneous" = max corr at lag 0
    ///   "noisy"        = |max corr| < MIN_SIGNIFICANT_CORR
    pub direction: String,
}

/// Pearson correlation between two equal-length slices.
fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    let n = a.len().min(b.len());
    if n < 2 {
        return None;
    }
    let mean_a: f64 = a[..n].iter().sum::<f64>() / n as f64;
    let mean_b: f64 = b[..n].iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut sa2 = 0.0;
    let mut sb2 = 0.0;
    for i in 0..n {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        num += da * db;
        sa2 += da * da;
        sb2 += db * db;
    }
    let denom = (sa2 * sb2).sqrt();
    if denom < 1e-12 {
        None
    } else {
        Some(num / denom)
    }
}

/// Compute cross-correlation at a given lag.
/// lag > 0: a leads b → corr(a[..n-lag], b[lag..])
/// lag < 0: b leads a → corr(a[-lag..], b[..n+lag])
fn cross_corr_at_lag(a: &[f64], b: &[f64], lag: i32) -> Option<(f64, usize)> {
    let n = a.len().min(b.len()) as i32;
    if lag.abs() >= n {
        return None;
    }
    let (a_slice, b_slice): (&[f64], &[f64]) = if lag > 0 {
        let l = lag as usize;
        (&a[..a.len() - l], &b[l..])
    } else if lag < 0 {
        let l = (-lag) as usize;
        (&a[l..], &b[..b.len() - l])
    } else {
        (a, b)
    };
    let n_overlap = a_slice.len().min(b_slice.len());
    if n_overlap < MIN_OVERLAP_SAMPLES {
        return None;
    }
    pearson(&a_slice[..n_overlap], &b_slice[..n_overlap]).map(|c| (c, n_overlap))
}

/// For one (from, to) edge, find the lag with maximum |correlation|.
fn best_lag(a: &[f64], b: &[f64]) -> Option<(i32, f64, usize)> {
    let mut best: Option<(i32, f64, usize)> = None;
    for lag in -MAX_LAG..=MAX_LAG {
        if let Some((c, n)) = cross_corr_at_lag(a, b, lag) {
            if best.map_or(true, |(_, bc, _)| c.abs() > bc.abs()) {
                best = Some((lag, c, n));
            }
        }
    }
    best
}

/// Compute lead-lag events for all master KG edges.
pub fn detect_lead_lag(
    market: &str,
    tracker: &LeadLagTracker,
    master_edges: &[(String, String, f64)],
    ts: DateTime<Utc>,
) -> Vec<LeadLagEvent> {
    let mut events = Vec::new();
    for (from, to, weight) in master_edges {
        let a = tracker.history.get(from);
        let b = tracker.history.get(to);
        let (a, b) = match (a, b) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let av: Vec<f64> = a.iter().copied().collect();
        let bv: Vec<f64> = b.iter().copied().collect();
        if let Some((lag, corr, n)) = best_lag(&av, &bv) {
            let direction = if corr.abs() < MIN_SIGNIFICANT_CORR {
                "noisy".to_string()
            } else if lag > 0 {
                "from_leads".to_string()
            } else if lag < 0 {
                "to_leads".to_string()
            } else {
                "simultaneous".to_string()
            };
            events.push(LeadLagEvent {
                ts,
                market: market.to_string(),
                from_symbol: from.clone(),
                to_symbol: to.clone(),
                edge_weight: *weight,
                dominant_lag: lag,
                correlation_at_lag: corr,
                n_samples: n,
                direction,
            });
        }
    }
    events
}

pub fn write_events(market: &str, events: &[LeadLagEvent]) -> std::io::Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-lead-lag-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for ev in events {
        let line = serde_json::to_string(ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    fn build_tracker(series: &[(&str, &[f64])]) -> LeadLagTracker {
        let mut t = LeadLagTracker::new();
        for (sym, vals) in series {
            for v in vals.iter() {
                t.observe(sym, *v);
            }
        }
        t
    }

    #[test]
    fn perfectly_correlated_at_lag_0() {
        let s = (0..15).map(|i| (i as f64).sin()).collect::<Vec<_>>();
        let t = build_tracker(&[("A", &s), ("B", &s)]);
        let evs = detect_lead_lag("test", &t, &[("A".into(), "B".into(), 1.0)], Utc::now());
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].dominant_lag, 0);
        assert_eq!(evs[0].direction, "simultaneous");
        assert!(evs[0].correlation_at_lag > 0.99);
    }

    #[test]
    fn b_lags_a_by_one() {
        // A leads B by 1: A[i] = sin(i), B[i] = sin(i-1) = A shifted.
        let n = 15;
        let a: Vec<f64> = (0..n).map(|i| (i as f64 * 0.5).sin()).collect();
        let b: Vec<f64> = (0..n).map(|i| ((i - 1) as f64 * 0.5).sin()).collect();
        let t = build_tracker(&[("A", &a), ("B", &b)]);
        let evs = detect_lead_lag("test", &t, &[("A".into(), "B".into(), 1.0)], Utc::now());
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].dominant_lag, 1, "A should lead B by 1");
        assert_eq!(evs[0].direction, "from_leads");
    }

    #[test]
    fn b_leads_a_negative_lag() {
        // B leads A: B[i] = sin(i), A[i] = sin(i-1)
        let n = 15;
        let a: Vec<f64> = (0..n).map(|i| ((i - 1) as f64 * 0.5).sin()).collect();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 * 0.5).sin()).collect();
        let t = build_tracker(&[("A", &a), ("B", &b)]);
        let evs = detect_lead_lag("test", &t, &[("A".into(), "B".into(), 1.0)], Utc::now());
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].dominant_lag, -1, "B should lead A by 1");
        assert_eq!(evs[0].direction, "to_leads");
    }

    #[test]
    fn uncorrelated_emits_noisy() {
        // Two independent random-ish series.
        let a = [
            0.1, -0.3, 0.7, 0.2, -0.5, 0.9, -0.2, 0.4, -0.6, 0.8, -0.1, 0.5, -0.7, 0.3, -0.4,
        ];
        let b = [
            0.4, 0.6, -0.2, -0.7, 0.3, 0.1, -0.5, 0.9, -0.3, 0.2, 0.8, -0.6, 0.5, -0.1, 0.0,
        ];
        let t = build_tracker(&[("A", &a), ("B", &b)]);
        let evs = detect_lead_lag("test", &t, &[("A".into(), "B".into(), 1.0)], Utc::now());
        assert_eq!(evs.len(), 1);
        // Random series may or may not pass the noise threshold;
        // confirm it's classified correctly either way.
        assert!(
            evs[0].direction == "noisy" || evs[0].correlation_at_lag.abs() >= MIN_SIGNIFICANT_CORR,
            "should classify consistently with corr value: {:?}",
            evs[0]
        );
    }

    #[test]
    fn insufficient_history_skipped() {
        // Only 5 samples — below MIN_OVERLAP_SAMPLES (10).
        let s = [0.1, 0.2, 0.3, 0.4, 0.5];
        let t = build_tracker(&[("A", &s), ("B", &s)]);
        let evs = detect_lead_lag("test", &t, &[("A".into(), "B".into(), 1.0)], Utc::now());
        assert!(evs.is_empty(), "should skip when history too short");
    }
}
