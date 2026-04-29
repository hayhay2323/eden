//! Graph kinematics — first and second time derivatives of sub-KG
//! activations, plus force balance and zero-crossing detection.
//!
//! A static graph (current-tick spatial contrast) can show where the
//! structure is unusual RIGHT NOW. But it can't show **turning points**.
//!
//! Physics first-principles: at a turning point, position is extremal
//! AND velocity = 0. For a ball at the top: y = max, dy/dt = 0. You
//! don't need history to know it's the top — you see dy/dt → 0 and
//! position high.
//!
//! For Eden:
//!   level(symbol, kind)       = current activation (already in sub-KG)
//!   velocity(symbol, kind)    = d/dt level   (this module)
//!   acceleration(symbol, kind)= d²/dt² level (this module)
//!
//! Force balance (buy vs sell equivalent) per symbol:
//!   buy_force  = sum(Accumulative broker activations)
//!                + max(0, PressureCapitalFlow)
//!                + max(0, TradeTapeBuyMinusSell30s)
//!                + IntentAccumulation posterior
//!   sell_force = sum(Distributive broker activations)
//!                + max(0, -PressureCapitalFlow)
//!                + max(0, -TradeTapeBuyMinusSell30s)
//!                + IntentDistribution posterior
//!   balance    = buy_force − sell_force
//!
//! Turning point = **velocity sign reversal** (mathematical zero
//! crossing, no threshold) on force balance.
//!
//! No rules. No patterns. Pure kinematics of graph activation field.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};

/// Ring buffer depth for time derivatives. 5 ticks ≈ 40s at 8s/tick.
pub const HISTORY_LEN: usize = 5;

/// Noise floor percentile for turning-point events. Within each tick's
/// distribution of |balance| × |velocity reversal|, only the top
/// (1 - percentile) events qualify as real structural reversals.
/// HFT-size oscillations form the floor; real regime turns exceed it.
pub const KINEMATICS_NOISE_FLOOR_PERCENTILE: f64 = 0.99;

/// Velocity computed as (latest − Nth-oldest) / span.
/// Using span = HISTORY_LEN - 1 so velocity is "change over 4 ticks".
fn velocity_of(history: &VecDeque<f64>) -> Option<f64> {
    if history.len() < 2 {
        return None;
    }
    let latest = *history.back()?;
    let oldest = *history.front()?;
    let span = (history.len() - 1) as f64;
    Some((latest - oldest) / span)
}

/// Acceleration: midpoint second difference.
fn acceleration_of(history: &VecDeque<f64>) -> Option<f64> {
    if history.len() < 3 {
        return None;
    }
    let latest = *history.back()?;
    let mid = history[history.len() / 2];
    let oldest = *history.front()?;
    let span = (history.len() - 1) as f64 / 2.0;
    if span < 1.0 {
        return None;
    }
    let v_recent = (latest - mid) / span;
    let v_prev = (mid - oldest) / span;
    Some(v_recent - v_prev)
}

#[derive(Debug, Default)]
pub struct KinematicsTracker {
    /// Per-(symbol, NodeId) rolling history of activation values.
    history: HashMap<(String, NodeId), VecDeque<f64>>,
    /// Per-symbol rolling force balance history.
    balance_history: HashMap<String, VecDeque<f64>>,
}

impl KinematicsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingest a current-tick value for a specific node.
    pub fn observe(&mut self, symbol: &str, node_id: &NodeId, value: f64) {
        let key = (symbol.to_string(), node_id.clone());
        let entry = self.history.entry(key).or_default();
        entry.push_back(value);
        while entry.len() > HISTORY_LEN {
            entry.pop_front();
        }
    }

    pub fn observe_balance(&mut self, symbol: &str, balance: f64) {
        let entry = self.balance_history.entry(symbol.to_string()).or_default();
        entry.push_back(balance);
        while entry.len() > HISTORY_LEN {
            entry.pop_front();
        }
    }

    pub fn velocity(&self, symbol: &str, node_id: &NodeId) -> Option<f64> {
        self.history
            .get(&(symbol.to_string(), node_id.clone()))
            .and_then(velocity_of)
    }

    pub fn acceleration(&self, symbol: &str, node_id: &NodeId) -> Option<f64> {
        self.history
            .get(&(symbol.to_string(), node_id.clone()))
            .and_then(acceleration_of)
    }

    pub fn balance_velocity(&self, symbol: &str) -> Option<f64> {
        self.balance_history.get(symbol).and_then(velocity_of)
    }

    /// Zero-crossing of balance velocity (turn forming):
    /// previous velocity has opposite sign from current velocity.
    pub fn balance_velocity_zero_crossed(&self, symbol: &str) -> Option<ZeroCrossDir> {
        let h = self.balance_history.get(symbol)?;
        if h.len() < 4 {
            return None;
        }
        // Two halves of buffer: velocity over first half vs second half.
        let mid = h.len() / 2;
        let first: VecDeque<f64> = h.iter().take(mid + 1).cloned().collect();
        let second: VecDeque<f64> = h.iter().skip(mid).cloned().collect();
        let v1 = velocity_of(&first)?;
        let v2 = velocity_of(&second)?;
        if v1 > 0.0 && v2 < 0.0 {
            Some(ZeroCrossDir::PosToNeg) // top forming
        } else if v1 < 0.0 && v2 > 0.0 {
            Some(ZeroCrossDir::NegToPos) // bottom forming
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ZeroCrossDir {
    /// Momentum shifting from positive to negative (top forming).
    PosToNeg,
    /// Momentum shifting from negative to positive (bottom forming).
    NegToPos,
}

#[derive(Debug, Clone, Serialize)]
pub struct KinematicsEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    pub event_kind: String, // "TopForming" | "BottomForming"
    pub balance_now: f64,
    pub balance_recent_velocity: f64,
    pub balance_prev_velocity: f64,
    /// Current activation sum for "buy" forces
    pub buy_force: f64,
    pub sell_force: f64,
    /// Key contributors (top node kinds currently driving buy + sell)
    pub contrib_note: String,
}

/// Compute force balance per symbol from current sub-KG state.
/// Pure read, no history.
fn compute_force_balance(registry: &SubKgRegistry) -> HashMap<String, (f64, f64)> {
    use crate::pipeline::symbol_sub_kg::NodeKind;
    let mut out: HashMap<String, (f64, f64)> = HashMap::new();

    for (sym, kg) in &registry.graphs {
        let mut buy: f64 = 0.0;
        let mut sell: f64 = 0.0;

        for (id, a) in &kg.nodes {
            let v = a.value.map(|x| x.to_f64().unwrap_or(0.0)).unwrap_or(0.0);
            match a.kind {
                NodeKind::Broker => match a.label.as_deref() {
                    Some("Accumulative") => buy += v,
                    Some("Distributive") => sell += v,
                    _ => {}
                },
                NodeKind::Intent => match id {
                    NodeId::IntentAccumulation => buy += v,
                    NodeId::IntentDistribution => sell += v,
                    _ => {}
                },
                NodeKind::Pressure => match id {
                    NodeId::PressureCapitalFlow
                    | NodeId::PressureInstitutional
                    | NodeId::PressureOrderBook => {
                        if v > 0.0 {
                            buy += v;
                        } else {
                            sell += -v;
                        }
                    }
                    _ => {}
                },
                NodeKind::Microstructure => match id {
                    NodeId::TradeTapeBuyMinusSell30s => {
                        if v > 0.0 {
                            buy += v.abs();
                        } else {
                            sell += v.abs();
                        }
                    }
                    NodeId::DepthAsymmetryTop3 => {
                        // 0.5 = balanced. Above → buy bias.
                        let shifted = v - 0.5;
                        if shifted > 0.0 {
                            buy += shifted;
                        } else {
                            sell += -shifted;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        out.insert(sym.clone(), (buy, sell));
    }
    out
}

/// Run kinematics update for this tick.
/// 1. Update tracker histories from current registry state.
/// 2. Compute turning-point zero-crossing events.
pub fn update_and_detect(
    market: &str,
    registry: &SubKgRegistry,
    tracker: &mut KinematicsTracker,
    ts: DateTime<Utc>,
) -> Vec<KinematicsEvent> {
    let forces = compute_force_balance(registry);

    // Update balance history
    for (sym, (buy, sell)) in &forces {
        tracker.observe_balance(sym, buy - sell);
    }

    // Update a few key node histories for velocity queries downstream.
    // (Full per-node tracking would be 494 × 87 ≈ 43K series — too heavy.
    // Track the high-signal ones: PressureCapitalFlow, IntentAccumulation,
    // IntentDistribution, TradeTapeBuyMinusSell30s.)
    let tracked_nodes = [
        NodeId::PressureCapitalFlow,
        NodeId::PressureMomentum,
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
        NodeId::TradeTapeBuyMinusSell30s,
        NodeId::VwapDeviationPct,
        // Added for consistency_gauge: stealth-accumulation detection
        NodeId::Volume,
        NodeId::LastPrice,
    ];
    for (sym, kg) in &registry.graphs {
        for id in &tracked_nodes {
            if let Some(node) = kg.nodes.get(id) {
                if let Some(v) = node.value {
                    tracker.observe(sym, id, v.to_f64().unwrap_or(0.0));
                }
            }
        }
    }

    // Detect turning points via balance velocity zero-crossing.
    // Collect ALL candidates first, then apply noise-floor subtraction.
    let mut candidates = Vec::new();
    for (sym, (buy, sell)) in &forces {
        if let Some(cross) = tracker.balance_velocity_zero_crossed(sym) {
            let h = tracker.balance_history.get(sym).unwrap();
            let mid = h.len() / 2;
            let first: VecDeque<f64> = h.iter().take(mid + 1).cloned().collect();
            let second: VecDeque<f64> = h.iter().skip(mid).cloned().collect();
            let v_prev = velocity_of(&first).unwrap_or(0.0);
            let v_now = velocity_of(&second).unwrap_or(0.0);
            let balance_now = buy - sell;
            // Reversal magnitude = |balance| × |velocity flip amplitude|.
            // Physical meaning: how strong is the position AND how sharp
            // is the velocity reversal. HFT tiny oscillations near zero
            // balance with small flips have tiny magnitude; real regime
            // turns have high magnitude.
            let reversal_mag = balance_now.abs() * (v_now - v_prev).abs();
            candidates.push((
                KinematicsEvent {
                    ts,
                    market: market.to_string(),
                    symbol: sym.clone(),
                    event_kind: match cross {
                        ZeroCrossDir::PosToNeg => "TopForming".into(),
                        ZeroCrossDir::NegToPos => "BottomForming".into(),
                    },
                    balance_now,
                    balance_recent_velocity: v_now,
                    balance_prev_velocity: v_prev,
                    buy_force: *buy,
                    sell_force: *sell,
                    contrib_note: format!(
                        "buy={:.2} sell={:.2} bal={:.2} mag={:.3}",
                        buy, sell, balance_now, reversal_mag
                    ),
                },
                reversal_mag,
            ));
        }
    }
    // Noise floor: percentile of reversal magnitudes this tick.
    // Only reversals exceeding the floor qualify as real structural turns.
    if candidates.len() < 10 {
        return Vec::new();
    }
    let mut mags: Vec<f64> = candidates.iter().map(|(_, m)| *m).collect();
    mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor_idx = (KINEMATICS_NOISE_FLOOR_PERCENTILE * mags.len() as f64) as usize;
    let floor = mags
        .get(floor_idx.min(mags.len() - 1))
        .copied()
        .unwrap_or(0.0);
    candidates
        .into_iter()
        .filter_map(|(ev, mag)| if mag > floor { Some(ev) } else { None })
        .collect()
}

pub fn write_events(market: &str, events: &[KinematicsEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-kinematics-{}.ndjson", market);
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

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    fn vd(v: &[f64]) -> VecDeque<f64> {
        v.iter().cloned().collect()
    }

    #[test]
    fn velocity_linear_rise() {
        let h = vd(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let v = velocity_of(&h).unwrap();
        assert!((v - 1.0).abs() < 1e-9);
    }

    #[test]
    fn velocity_linear_fall() {
        let h = vd(&[5.0, 4.0, 3.0, 2.0, 1.0]);
        let v = velocity_of(&h).unwrap();
        assert!((v - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn acceleration_constant_velocity_is_zero() {
        let h = vd(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        let a = acceleration_of(&h).unwrap();
        assert!(a.abs() < 1e-9);
    }

    #[test]
    fn acceleration_detects_slowing() {
        // velocity going 2, 1 → decelerating
        let h = vd(&[0.0, 2.0, 4.0, 5.0, 6.0]);
        let a = acceleration_of(&h).unwrap();
        assert!(
            a < 0.0,
            "decelerating should produce negative accel, got {}",
            a
        );
    }

    #[test]
    fn tracker_observe_and_velocity() {
        let mut t = KinematicsTracker::new();
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            t.observe("A.HK", &NodeId::PressureCapitalFlow, v);
        }
        let v = t.velocity("A.HK", &NodeId::PressureCapitalFlow).unwrap();
        assert!((v - 1.0).abs() < 1e-9);
    }

    #[test]
    fn zero_cross_pos_to_neg_detects_top() {
        let mut t = KinematicsTracker::new();
        // Balance rising then falling: 0, 1, 2, 1.5, 1 → velocity first +0.67 then -0.5
        for b in [0.0, 1.0, 2.0, 1.5, 1.0] {
            t.observe_balance("A.HK", b);
        }
        let cross = t.balance_velocity_zero_crossed("A.HK");
        assert_eq!(cross, Some(ZeroCrossDir::PosToNeg));
    }

    #[test]
    fn zero_cross_neg_to_pos_detects_bottom() {
        let mut t = KinematicsTracker::new();
        for b in [5.0, 3.0, 1.0, 2.0, 4.0] {
            t.observe_balance("A.HK", b);
        }
        let cross = t.balance_velocity_zero_crossed("A.HK");
        assert_eq!(cross, Some(ZeroCrossDir::NegToPos));
    }

    #[test]
    fn no_cross_when_monotonic() {
        let mut t = KinematicsTracker::new();
        for b in [1.0, 2.0, 3.0, 4.0, 5.0] {
            t.observe_balance("A.HK", b);
        }
        let cross = t.balance_velocity_zero_crossed("A.HK");
        assert_eq!(cross, None);
    }
}
