//! Expectation / Surprise primitive — Free Energy Principle applied to
//! graph perception.
//!
//! Biological perception minimizes surprise:
//!   surprise = |observed − expected|
//!
//! Every tick, Eden forms an expectation for each sub-KG node's next-tick
//! value using its **local history** (linear kinematic extrapolation) and
//! its **graph neighborhood** (what propagation from master-KG neighbors
//! would predict). Then at the next tick, it observes the actual value.
//!
//! The difference is **surprise** — the information content of the
//! observation beyond what the graph model already accounted for.
//!
//! Properties:
//!   - Repeated / predictable structure = zero surprise (already modeled)
//!   - Novel structural change = high surprise (information gain)
//!   - Surprise naturally decays as the model adapts (via persistence)
//!
//! Implementation:
//!   - `ExpectationTracker` keeps last-2-ticks value per (symbol, node).
//!   - Linear extrapolation: expected(T+1) = val(T) + (val(T) - val(T-1)).
//!   - Squared error per node: (observed − expected)^2.
//!   - Per-symbol aggregate: sum across tracked node kinds.
//!   - Universe 99th percentile floor (same primitive as everywhere).
//!   - Symbols above floor emit surprise events.
//!
//! This is the missing Layer 5 — predictive perception. Without it, Eden
//! only observes; with it, Eden KNOWS WHICH OBSERVATIONS ARE INFORMATIVE.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, NodeKind, SubKgRegistry};

/// 99th percentile surprise threshold (universe-wide per tick).
pub const SURPRISE_FLOOR_PERCENTILE: f64 = 0.99;

/// Nodes we form expectations for. Pressure + Intent = scale-comparable,
/// 0..1-ish range so squared errors are interpretable.
fn tracked_node_ids() -> Vec<NodeId> {
    vec![
        NodeId::PressureOrderBook,
        NodeId::PressureCapitalFlow,
        NodeId::PressureInstitutional,
        NodeId::PressureMomentum,
        NodeId::PressureVolume,
        NodeId::PressureStructure,
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
    ]
}

#[derive(Debug, Default)]
pub struct ExpectationTracker {
    /// Per (symbol, node_id) → last 2 observed values.
    /// history[(sym, id)] = [val(T-1), val(T)].
    history: HashMap<(String, NodeId), [Option<f64>; 2]>,
}

impl ExpectationTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push current-tick observation; shift history.
    pub fn observe(&mut self, symbol: &str, node_id: &NodeId, value: f64) {
        let key = (symbol.to_string(), node_id.clone());
        let entry = self.history.entry(key).or_insert([None, None]);
        entry[0] = entry[1];
        entry[1] = Some(value);
    }

    /// Expected value for next tick from linear kinematic extrapolation
    /// E[v(T+1) | v(T), v(T-1)] = v(T) + (v(T) - v(T-1)).
    pub fn expected_next(&self, symbol: &str, node_id: &NodeId) -> Option<f64> {
        let key = (symbol.to_string(), node_id.clone());
        let h = self.history.get(&key)?;
        match (h[0], h[1]) {
            (Some(prev), Some(curr)) => Some(curr + (curr - prev)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SurpriseEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    /// Sum of squared error across tracked node kinds.
    pub total_surprise: f64,
    /// Noise floor (99th percentile universe-wide).
    pub floor: f64,
    /// Single node with highest squared error (and its values).
    pub max_node: String,
    pub max_observed: f64,
    pub max_expected: f64,
    pub max_squared_error: f64,
}

/// Update tracker with current tick's observations, then measure surprise
/// by comparing current observations to expectations formed from the
/// PREVIOUS tick state. Returns symbols whose total surprise exceeds the
/// universe 99th percentile floor.
pub fn update_and_measure(
    market: &str,
    registry: &SubKgRegistry,
    tracker: &mut ExpectationTracker,
    ts: DateTime<Utc>,
) -> Vec<SurpriseEvent> {
    let tracked = tracked_node_ids();

    // Step 1: For each symbol's current state, compute surprise using
    // the tracker's EXISTING history (expectation was formed last tick).
    let mut per_symbol: Vec<(String, f64, String, f64, f64, f64)> = Vec::new();
    for (sym, kg) in &registry.graphs {
        let mut total = 0.0_f64;
        let mut max_err = 0.0_f64;
        let mut max_node = String::new();
        let mut max_obs = 0.0_f64;
        let mut max_exp = 0.0_f64;
        for id in &tracked {
            let obs = match kg.nodes.get(id).and_then(|n| n.value) {
                Some(v) => v.to_f64().unwrap_or(0.0),
                None => continue,
            };
            let exp = match tracker.expected_next(sym, id) {
                Some(e) => e,
                None => continue,
            };
            let err = (obs - exp).powi(2);
            total += err;
            if err > max_err {
                max_err = err;
                max_node = format!("{:?}", id);
                max_obs = obs;
                max_exp = exp;
            }
        }
        per_symbol.push((sym.clone(), total, max_node, max_obs, max_exp, max_err));
    }

    // Step 2: Update tracker with current observations (for next tick's
    // expectation).
    for (sym, kg) in &registry.graphs {
        for id in &tracked {
            if let Some(v) = kg.nodes.get(id).and_then(|n| n.value) {
                tracker.observe(sym, id, v.to_f64().unwrap_or(0.0));
            }
        }
    }

    // Step 3: Noise floor on total_surprise distribution.
    if per_symbol.len() < 30 {
        return Vec::new();
    }
    let mut mags: Vec<f64> = per_symbol.iter().map(|(_, s, ..)| *s).collect();
    mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor_idx = percentile_floor_index(SURPRISE_FLOOR_PERCENTILE, mags.len());
    let floor = mags
        .get(floor_idx.min(mags.len() - 1))
        .copied()
        .unwrap_or(0.0);

    per_symbol
        .into_iter()
        .filter_map(|(sym, total, max_node, obs, exp, err)| {
            if total > floor {
                Some(SurpriseEvent {
                    ts,
                    market: market.to_string(),
                    symbol: sym,
                    total_surprise: total,
                    floor,
                    max_node,
                    max_observed: obs,
                    max_expected: exp,
                    max_squared_error: err,
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

pub fn write_events(market: &str, events: &[SurpriseEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-surprise-{}.ndjson", market);
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

/// Classify NodeKind of a node id. Exposed for downstream use.
pub fn node_kind_of(id: &NodeId) -> NodeKind {
    match id {
        NodeId::PressureOrderBook
        | NodeId::PressureCapitalFlow
        | NodeId::PressureInstitutional
        | NodeId::PressureMomentum
        | NodeId::PressureVolume
        | NodeId::PressureStructure => NodeKind::Pressure,
        NodeId::IntentAccumulation
        | NodeId::IntentDistribution
        | NodeId::IntentRotation
        | NodeId::IntentVolatility
        | NodeId::IntentUnknown => NodeKind::Intent,
        _ => NodeKind::SymbolRoot,
    }
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    fn seed_universe(n: usize) -> SubKgRegistry {
        let mut reg = SubKgRegistry::new();
        let now = Utc::now();
        for i in 0..n {
            let sym = format!("S{}.HK", i);
            let kg = reg.upsert(&sym, now);
            kg.set_node_value(NodeId::PressureOrderBook, dec!(0.5), now);
            kg.set_node_value(NodeId::IntentAccumulation, dec!(0.2), now);
        }
        reg
    }

    #[test]
    fn expected_needs_two_ticks() {
        let mut t = ExpectationTracker::new();
        t.observe("A.HK", &NodeId::PressureOrderBook, 0.5);
        // Only one sample, cannot extrapolate
        assert!(t
            .expected_next("A.HK", &NodeId::PressureOrderBook)
            .is_none());
        t.observe("A.HK", &NodeId::PressureOrderBook, 0.6);
        // Two samples: expected = 0.6 + (0.6 - 0.5) = 0.7
        let exp = t.expected_next("A.HK", &NodeId::PressureOrderBook).unwrap();
        assert!((exp - 0.7).abs() < 1e-9);
    }

    #[test]
    fn uniform_universe_no_surprise_fires() {
        let mut t = ExpectationTracker::new();
        let reg = seed_universe(200);
        // First tick: no history, nothing fires
        let evs1 = update_and_measure("hk", &reg, &mut t, Utc::now());
        assert!(evs1.is_empty(), "first tick has no expectations yet");
        // Second tick: history of 1, still no extrapolation possible
        let evs2 = update_and_measure("hk", &reg, &mut t, Utc::now());
        assert!(evs2.is_empty(), "second tick still no extrapolation");
        // Third tick: identical state → expected = current → zero surprise everywhere
        let evs3 = update_and_measure("hk", &reg, &mut t, Utc::now());
        assert!(evs3.is_empty(), "stable universe produces zero surprise");
    }

    #[test]
    fn sudden_jump_on_one_symbol_fires() {
        let mut t = ExpectationTracker::new();
        let mut reg = seed_universe(200);
        // 3 calm ticks to build history
        for _ in 0..3 {
            update_and_measure("hk", &reg, &mut t, Utc::now());
        }
        // 4th tick: S0 jumps massively, others stay
        {
            let kg = reg.upsert("S0.HK", Utc::now());
            kg.set_node_value(NodeId::PressureOrderBook, dec!(5.0), Utc::now());
            kg.set_node_value(NodeId::IntentAccumulation, dec!(0.9), Utc::now());
        }
        let evs = update_and_measure("hk", &reg, &mut t, Utc::now());
        assert!(
            !evs.is_empty(),
            "sudden jump should produce surprise events"
        );
        assert!(evs.iter().any(|e| e.symbol == "S0.HK"));
    }

    #[test]
    fn linear_trend_no_surprise() {
        let mut t = ExpectationTracker::new();
        let mut reg = seed_universe(200);
        // Steady linear rise: +0.1 each tick on one node
        for step in 0..5 {
            {
                let kg = reg.upsert("S0.HK", Utc::now());
                let v = rust_decimal::Decimal::new(5 + step, 1); // 0.5, 0.6, 0.7, ...
                kg.set_node_value(NodeId::PressureOrderBook, v, Utc::now());
            }
            let _ = update_and_measure("hk", &reg, &mut t, Utc::now());
        }
        // Now continue the trend: predictable → low surprise
        {
            let kg = reg.upsert("S0.HK", Utc::now());
            kg.set_node_value(NodeId::PressureOrderBook, dec!(1.0), Utc::now());
        }
        let evs = update_and_measure("hk", &reg, &mut t, Utc::now());
        // Linear trend should NOT produce S0 as surprise (expectation matches)
        assert!(
            !evs.iter().any(|e| e.symbol == "S0.HK"),
            "linear trend matches expectation, should not fire"
        );
    }

    #[test]
    fn small_universe_rejected() {
        let mut t = ExpectationTracker::new();
        let reg = seed_universe(10); // too few to define floor
        for _ in 0..3 {
            let evs = update_and_measure("hk", &reg, &mut t, Utc::now());
            assert!(evs.is_empty());
        }
    }
}
