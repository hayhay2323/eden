//! Signal velocity — per-symbol confidence change rate across cycles.
//!
//! The `AgentStructureState` already carries `confidence` but not its
//! delta. An operator reading the snapshot only sees a static number, so
//! cannot distinguish "conf=0.95 and rising toward 1.0" from "conf=0.95
//! and decaying from 1.0". These two states have very different trade
//! implications:
//!
//!   Rising:  setup strengthening, early-confirmation window — may be
//!            about to hit action=enter; worth watching for entry.
//!   Falling: setup losing support, late-stage — may disappear soon;
//!            worth NOT entering even at high nominal confidence.
//!
//! This module provides a stateful tracker that records the most recent
//! two confidence readings per symbol and exposes a velocity (confidence
//! delta per observation) plus a direction label.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct ConfidencePoint {
    pub confidence: f64,
    /// Cycle number at which the observation was recorded. Used to
    /// disambiguate gaps — if the caller observed a symbol at cycle 10
    /// and then again at cycle 13, that is three ticks of evolution, not
    /// one.
    pub cycle: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VelocityDirection {
    Rising,
    Falling,
    Flat,
}

impl VelocityDirection {
    pub fn label(self) -> &'static str {
        match self {
            VelocityDirection::Rising => "rising",
            VelocityDirection::Falling => "falling",
            VelocityDirection::Flat => "flat",
        }
    }
}

/// Delta threshold below which we treat velocity as flat. Confidence in
/// the snapshot is a fraction in [0, 1] with typical tick-to-tick noise of
/// ~0.005; we require at least 1 % absolute change per cycle to call it
/// a move.
pub const FLAT_BAND: f64 = 0.01;

#[derive(Debug, Clone, Copy)]
pub struct SignalVelocity {
    /// Signed change in confidence per cycle since the prior observation.
    pub per_cycle: f64,
    pub direction: VelocityDirection,
}

impl SignalVelocity {
    pub fn flat() -> Self {
        SignalVelocity {
            per_cycle: 0.0,
            direction: VelocityDirection::Flat,
        }
    }

    pub fn from_points(prev: ConfidencePoint, cur: ConfidencePoint) -> Self {
        let cycle_gap = cur.cycle.saturating_sub(prev.cycle).max(1) as f64;
        let delta_total = cur.confidence - prev.confidence;
        let per_cycle = delta_total / cycle_gap;
        let direction = classify(per_cycle);
        SignalVelocity {
            per_cycle,
            direction,
        }
    }
}

fn classify(per_cycle: f64) -> VelocityDirection {
    if per_cycle > FLAT_BAND {
        VelocityDirection::Rising
    } else if per_cycle < -FLAT_BAND {
        VelocityDirection::Falling
    } else {
        VelocityDirection::Flat
    }
}

#[derive(Debug, Clone, Default)]
pub struct SignalVelocityTracker {
    // Keep only the most recent observation per symbol; velocity is
    // computed against it on the next observe() call.
    last_point: HashMap<String, ConfidencePoint>,
    // Store the most recently computed velocity per symbol so callers can
    // read it without re-observing.
    last_velocity: HashMap<String, SignalVelocity>,
}

impl SignalVelocityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new confidence observation and return the velocity vs the
    /// prior observation (or `None` if this is the first).
    pub fn observe(
        &mut self,
        symbol: &str,
        confidence: Decimal,
        cycle: u64,
    ) -> Option<SignalVelocity> {
        let confidence = confidence.to_f64().unwrap_or(0.0);
        let cur = ConfidencePoint { confidence, cycle };
        let computed = self
            .last_point
            .get(symbol)
            .copied()
            .map(|prev| SignalVelocity::from_points(prev, cur));
        self.last_point.insert(symbol.to_string(), cur);
        if let Some(v) = computed {
            self.last_velocity.insert(symbol.to_string(), v);
        }
        computed
    }

    /// Last computed velocity for `symbol`, or `None` if we only have one
    /// observation so far (or never saw it).
    pub fn velocity(&self, symbol: &str) -> Option<SignalVelocity> {
        self.last_velocity.get(symbol).copied()
    }

    /// Mark a symbol as gone from active_structures this cycle. Drops any
    /// tracked state so the next reappearance starts fresh (after a
    /// direction flip or a gap, historical velocity is stale).
    pub fn drop(&mut self, symbol: &str) {
        self.last_point.remove(symbol);
        self.last_velocity.remove(symbol);
    }

    /// List all symbols whose most recent velocity is `Rising`. Useful
    /// for operator wake "setups strengthening" line.
    pub fn rising_symbols(&self) -> Vec<String> {
        self.last_velocity
            .iter()
            .filter(|(_, v)| v.direction == VelocityDirection::Rising)
            .map(|(s, _)| s.clone())
            .collect()
    }

    pub fn falling_symbols(&self) -> Vec<String> {
        self.last_velocity
            .iter()
            .filter(|(_, v)| v.direction == VelocityDirection::Falling)
            .map(|(s, _)| s.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn first_observation_returns_none() {
        let mut t = SignalVelocityTracker::new();
        let v = t.observe("COIN.US", dec!(0.85), 10);
        assert!(v.is_none());
    }

    #[test]
    fn second_observation_reports_rising() {
        let mut t = SignalVelocityTracker::new();
        t.observe("COIN.US", dec!(0.85), 10);
        let v = t.observe("COIN.US", dec!(0.95), 11).unwrap();
        assert_eq!(v.direction, VelocityDirection::Rising);
        assert!((v.per_cycle - 0.10).abs() < 1e-6);
    }

    #[test]
    fn second_observation_reports_falling() {
        let mut t = SignalVelocityTracker::new();
        t.observe("X.US", dec!(0.95), 20);
        let v = t.observe("X.US", dec!(0.85), 21).unwrap();
        assert_eq!(v.direction, VelocityDirection::Falling);
        assert!(v.per_cycle < 0.0);
    }

    #[test]
    fn small_moves_are_flat() {
        let mut t = SignalVelocityTracker::new();
        t.observe("Y.US", dec!(0.900), 5);
        let v = t.observe("Y.US", dec!(0.905), 6).unwrap();
        assert_eq!(v.direction, VelocityDirection::Flat);
    }

    #[test]
    fn cycle_gap_normalizes_per_cycle_rate() {
        // A +0.10 change over 5 cycles = 0.02/cycle (rising, but gentle).
        let mut t = SignalVelocityTracker::new();
        t.observe("Z.US", dec!(0.50), 1);
        let v = t.observe("Z.US", dec!(0.60), 6).unwrap();
        assert_eq!(v.direction, VelocityDirection::Rising);
        assert!((v.per_cycle - 0.02).abs() < 1e-6);
    }

    #[test]
    fn drop_resets_state() {
        let mut t = SignalVelocityTracker::new();
        t.observe("A.US", dec!(0.70), 1);
        t.observe("A.US", dec!(0.80), 2);
        assert!(t.velocity("A.US").is_some());
        t.drop("A.US");
        assert!(t.velocity("A.US").is_none());
        // First observe after drop should not produce a velocity.
        let v = t.observe("A.US", dec!(0.90), 3);
        assert!(v.is_none());
    }

    #[test]
    fn rising_and_falling_symbol_lists_partition_correctly() {
        let mut t = SignalVelocityTracker::new();
        t.observe("UP.US", dec!(0.5), 1);
        t.observe("UP.US", dec!(0.7), 2);
        t.observe("DOWN.US", dec!(0.8), 1);
        t.observe("DOWN.US", dec!(0.6), 2);
        t.observe("FLAT.US", dec!(0.5), 1);
        t.observe("FLAT.US", dec!(0.505), 2);
        let rising = t.rising_symbols();
        let falling = t.falling_symbols();
        assert!(rising.contains(&"UP.US".to_string()));
        assert!(!rising.contains(&"DOWN.US".to_string()));
        assert!(falling.contains(&"DOWN.US".to_string()));
        assert!(!falling.contains(&"UP.US".to_string()));
        assert!(!rising.contains(&"FLAT.US".to_string()));
        assert!(!falling.contains(&"FLAT.US".to_string()));
    }

    #[test]
    fn direction_label_strings() {
        assert_eq!(VelocityDirection::Rising.label(), "rising");
        assert_eq!(VelocityDirection::Falling.label(), "falling");
        assert_eq!(VelocityDirection::Flat.label(), "flat");
    }
}
