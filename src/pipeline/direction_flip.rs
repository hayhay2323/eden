//! Direction flip detector — tracks the prior direction of each symbol
//! and flags the tick on which a short→long or long→short transition
//! occurs.
//!
//! Motivated by 2026-04-22 live session observations that the highest-
//! leverage early entry signal is the *first cycle* a setup flips
//! direction (e.g. MSTR short → long at cycle #26, ISRG short → long at
//! cycle #52). Waiting 4 cycles for persistence confirmation cost 5-6%
//! of the move. This tracker surfaces the flip moment so downstream code
//! can emit a `breakout` action (planned as optimization #1) or at
//! minimum let the operator know a flip just happened.
//!
//! Note: only *direction changes* are flips. A symbol going
//! present→absent→present with the same direction is NOT a flip — that
//! is an oscillation (tracked in `pipeline::oscillation`). Direction
//! flips and oscillations are complementary metrics.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Long,
    Short,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Long => "long",
            Direction::Short => "short",
        }
    }
}

/// Outcome of observing a fresh direction state for a symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlipEvent {
    /// Symbol had no prior tracked direction — first time we've seen it.
    FirstObservation,
    /// Current direction matches the last-known; no flip.
    Unchanged,
    /// Current direction differs from last-known; this is a flip. Holds
    /// the previous direction for narrative purposes.
    Flipped { previous: Direction },
}

impl FlipEvent {
    pub fn is_flip(self) -> bool {
        matches!(self, FlipEvent::Flipped { .. })
    }
}

#[derive(Debug, Clone, Default)]
pub struct DirectionFlipTracker {
    last: HashMap<String, Direction>,
    flip_count: HashMap<String, u32>,
}

impl DirectionFlipTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new direction observation for `symbol` and return whether
    /// a flip occurred.
    pub fn observe(&mut self, symbol: &str, direction: Direction) -> FlipEvent {
        match self.last.insert(symbol.to_string(), direction) {
            None => FlipEvent::FirstObservation,
            Some(prev) if prev == direction => FlipEvent::Unchanged,
            Some(prev) => {
                *self.flip_count.entry(symbol.to_string()).or_insert(0) += 1;
                FlipEvent::Flipped { previous: prev }
            }
        }
    }

    /// Clear tracking for a symbol (e.g. when it disappears from
    /// active_structures for so long we no longer consider the last
    /// direction meaningful).
    pub fn forget(&mut self, symbol: &str) {
        self.last.remove(symbol);
        // Keep flip_count so we can still report historical noisiness.
    }

    /// Last known direction for `symbol`, if we have one.
    pub fn last_direction(&self, symbol: &str) -> Option<Direction> {
        self.last.get(symbol).copied()
    }

    /// Total number of flips recorded for `symbol` across the whole
    /// tracker lifetime.
    pub fn flip_count(&self, symbol: &str) -> u32 {
        self.flip_count.get(symbol).copied().unwrap_or(0)
    }

    /// All symbols we currently have a direction on.
    pub fn known_symbols(&self) -> Vec<String> {
        self.last.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_is_not_a_flip() {
        let mut t = DirectionFlipTracker::new();
        assert_eq!(
            t.observe("A.US", Direction::Long),
            FlipEvent::FirstObservation
        );
        assert_eq!(t.flip_count("A.US"), 0);
    }

    #[test]
    fn same_direction_is_unchanged() {
        let mut t = DirectionFlipTracker::new();
        t.observe("A.US", Direction::Long);
        assert_eq!(t.observe("A.US", Direction::Long), FlipEvent::Unchanged);
        assert_eq!(t.flip_count("A.US"), 0);
    }

    #[test]
    fn direction_change_is_flip() {
        let mut t = DirectionFlipTracker::new();
        t.observe("A.US", Direction::Long);
        let ev = t.observe("A.US", Direction::Short);
        assert_eq!(
            ev,
            FlipEvent::Flipped {
                previous: Direction::Long
            }
        );
        assert!(ev.is_flip());
        assert_eq!(t.flip_count("A.US"), 1);
    }

    #[test]
    fn flip_count_accumulates() {
        let mut t = DirectionFlipTracker::new();
        // Short→Long→Short→Long = 3 flips
        t.observe("MSTR.US", Direction::Short);
        t.observe("MSTR.US", Direction::Long);
        t.observe("MSTR.US", Direction::Short);
        t.observe("MSTR.US", Direction::Long);
        assert_eq!(t.flip_count("MSTR.US"), 3);
    }

    #[test]
    fn last_direction_reflects_most_recent() {
        let mut t = DirectionFlipTracker::new();
        t.observe("X.US", Direction::Short);
        assert_eq!(t.last_direction("X.US"), Some(Direction::Short));
        t.observe("X.US", Direction::Long);
        assert_eq!(t.last_direction("X.US"), Some(Direction::Long));
    }

    #[test]
    fn forget_clears_direction_but_keeps_count() {
        let mut t = DirectionFlipTracker::new();
        t.observe("Y.US", Direction::Long);
        t.observe("Y.US", Direction::Short);
        assert_eq!(t.flip_count("Y.US"), 1);
        t.forget("Y.US");
        assert_eq!(t.last_direction("Y.US"), None);
        // Historical flip count is preserved:
        assert_eq!(t.flip_count("Y.US"), 1);
        // Next observation after forget is first_observation:
        let ev = t.observe("Y.US", Direction::Long);
        assert_eq!(ev, FlipEvent::FirstObservation);
    }

    #[test]
    fn unknown_symbol_reports_zero_flips_and_no_direction() {
        let t = DirectionFlipTracker::new();
        assert_eq!(t.flip_count("UNKNOWN.US"), 0);
        assert_eq!(t.last_direction("UNKNOWN.US"), None);
    }

    #[test]
    fn known_symbols_includes_observed() {
        let mut t = DirectionFlipTracker::new();
        t.observe("A.US", Direction::Long);
        t.observe("B.US", Direction::Short);
        let known = t.known_symbols();
        assert_eq!(known.len(), 2);
        assert!(known.contains(&"A.US".to_string()));
        assert!(known.contains(&"B.US".to_string()));
    }

    #[test]
    fn session_2026_04_22_trace_matches_observation() {
        // Sequence from live: MSTR short cycles 23-24, flipped to long at
        // cycle 26, stayed long through 29, disappeared, returned long. We
        // only count long↔short flips, so observe(short) at 24 is
        // Unchanged (vs 23), observe(long) at 26 is Flipped.
        let mut t = DirectionFlipTracker::new();
        assert_eq!(
            t.observe("MSTR.US", Direction::Short),
            FlipEvent::FirstObservation
        );
        assert_eq!(t.observe("MSTR.US", Direction::Short), FlipEvent::Unchanged);
        assert_eq!(
            t.observe("MSTR.US", Direction::Long),
            FlipEvent::Flipped {
                previous: Direction::Short
            }
        );
        assert_eq!(t.flip_count("MSTR.US"), 1);
    }
}
