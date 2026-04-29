//! Oscillation tracker — how often a symbol's setup goes present→absent (or
//! back) within a rolling window of recent operator cycles.
//!
//! Motivated by live-operator observations (2026-04-22 session):
//!
//!   Symbol   Oscillations in session    Observer reaction
//!   ──────   ────────────────────────   ────────────────────────────
//!   COIN     3 (short→gone→short→gone→long)   veto twice, skip entries
//!   ISRG     3 (short→long→gone→long)          slow realisation
//!   MSTR     3 (long→observe→long→gone→…)      near-entry exit churn
//!   UPST     2 (short→long→short)              single flip ok, two = noise
//!
//! Operator heuristic that emerged: **3+ oscillations in a 20-cycle window
//! = blacklist until the symbol is quiet for 5 cycles**.  This tracker
//! provides the raw count so downstream code / wake lines can apply that
//! heuristic (or others) consistently.
//!
//! An *oscillation* is a transition of presence state: present→absent or
//! absent→present. A symbol that is continuously present (or continuously
//! absent) has zero oscillations. Direction flips (short↔long) are NOT
//! counted here — that is a separate concept (see `direction_flip_counter`
//! to be added later).
//!
//! Data layout is a per-symbol ring buffer of `bool` presence observations
//! keyed by cycle. The buffer is bounded by `window_size`; older entries
//! are dropped. Caller invokes `observe(&symbol, present)` once per
//! operator cycle per symbol of interest.

use std::collections::{HashMap, VecDeque};

/// Default rolling window length (in operator cycles). 20 cycles ≈ 40 min
/// at a 2-minute cadence — enough to catch multi-flip noise but short
/// enough that a symbol's behaviour can reset.
pub const DEFAULT_WINDOW: usize = 20;

/// Oscillation count threshold above which a symbol is considered noisy.
pub const NOISY_THRESHOLD: usize = 3;

/// Number of consecutive absent cycles that clears a symbol back to quiet.
pub const QUIET_RESET_CYCLES: usize = 5;

#[derive(Debug, Clone)]
pub struct OscillationTracker {
    window_size: usize,
    histories: HashMap<String, VecDeque<bool>>,
}

impl OscillationTracker {
    pub fn new() -> Self {
        Self::with_window(DEFAULT_WINDOW)
    }

    pub fn with_window(window_size: usize) -> Self {
        assert!(
            window_size >= 2,
            "window must be at least 2 to count a transition"
        );
        Self {
            window_size,
            histories: HashMap::new(),
        }
    }

    /// Record one observation for a symbol. Should be called exactly once
    /// per operator cycle per symbol of interest.
    pub fn observe(&mut self, symbol: &str, present: bool) {
        let history = self
            .histories
            .entry(symbol.to_string())
            .or_insert_with(|| VecDeque::with_capacity(self.window_size));
        history.push_back(present);
        while history.len() > self.window_size {
            history.pop_front();
        }
    }

    /// Number of presence transitions in the current window for `symbol`.
    pub fn oscillation_count(&self, symbol: &str) -> usize {
        self.histories
            .get(symbol)
            .map(|h| count_transitions(h.iter().copied()))
            .unwrap_or(0)
    }

    /// Has this symbol been continuously absent for at least
    /// `QUIET_RESET_CYCLES` of its most recent observations? Used as the
    /// blacklist-release condition.
    pub fn is_quiet_reset(&self, symbol: &str) -> bool {
        let Some(history) = self.histories.get(symbol) else {
            return false;
        };
        if history.len() < QUIET_RESET_CYCLES {
            return false;
        }
        history
            .iter()
            .rev()
            .take(QUIET_RESET_CYCLES)
            .all(|present| !*present)
    }

    /// True when the symbol's oscillation count is at or above the noisy
    /// threshold AND it is not currently in a quiet-reset window.
    pub fn is_noisy(&self, symbol: &str) -> bool {
        self.oscillation_count(symbol) >= NOISY_THRESHOLD && !self.is_quiet_reset(symbol)
    }

    /// All symbols currently flagged as noisy (see `is_noisy`). Useful for
    /// building a per-tick blacklist to emit into wake.reasons.
    pub fn noisy_symbols(&self) -> Vec<String> {
        self.histories
            .keys()
            .filter(|s| self.is_noisy(s))
            .cloned()
            .collect()
    }

    /// All symbols that still have an in-memory oscillation history.
    /// Callers use this to continue feeding absent observations after a
    /// symbol leaves the active set so quiet-reset windows can complete.
    pub fn tracked_symbols(&self) -> Vec<String> {
        self.histories.keys().cloned().collect()
    }

    /// Snapshot of all tracked symbols and their oscillation counts. Useful
    /// for debugging / dream-report authoring.
    pub fn all_counts(&self) -> Vec<(String, usize)> {
        self.histories
            .iter()
            .map(|(sym, hist)| (sym.clone(), count_transitions(hist.iter().copied())))
            .collect()
    }
}

impl Default for OscillationTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure function over a boolean sequence: count state transitions.
/// Transitions at either end of the sequence are not counted (we only see
/// changes between consecutive observations).
pub fn count_transitions<I>(iter: I) -> usize
where
    I: IntoIterator<Item = bool>,
{
    let mut count = 0usize;
    let mut prev: Option<bool> = None;
    for cur in iter {
        if let Some(p) = prev {
            if p != cur {
                count += 1;
            }
        }
        prev = Some(cur);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_transitions_empty_is_zero() {
        let seq: Vec<bool> = vec![];
        assert_eq!(count_transitions(seq), 0);
    }

    #[test]
    fn count_transitions_single_element_is_zero() {
        assert_eq!(count_transitions([true]), 0);
        assert_eq!(count_transitions([false]), 0);
    }

    #[test]
    fn count_transitions_steady_is_zero() {
        assert_eq!(count_transitions([true, true, true]), 0);
        assert_eq!(count_transitions([false, false, false, false]), 0);
    }

    #[test]
    fn count_transitions_single_flip_is_one() {
        assert_eq!(count_transitions([true, false]), 1);
        assert_eq!(count_transitions([false, true]), 1);
        assert_eq!(count_transitions([true, true, false]), 1);
    }

    #[test]
    fn count_transitions_coin_pattern() {
        // COIN cycle #43–49 approx: short→short→short→gone→short→gone→long
        // Presence: T,T,T,F,T,F,T — transitions: T→F (1), F→T (2), T→F (3),
        // F→T (4) = 4.
        let presence = [true, true, true, false, true, false, true];
        assert_eq!(count_transitions(presence), 4);
    }

    #[test]
    fn tracker_records_and_counts() {
        let mut t = OscillationTracker::with_window(10);
        for p in [true, true, false, true, false, true] {
            t.observe("MSTR.US", p);
        }
        assert_eq!(t.oscillation_count("MSTR.US"), 4);
    }

    #[test]
    fn tracker_window_drops_old() {
        // With window=3, only the last three observations count.
        let mut t = OscillationTracker::with_window(3);
        for p in [true, false, true, false, true] {
            t.observe("X.US", p);
        }
        // Last three presences: true, false, true → 2 transitions.
        assert_eq!(t.oscillation_count("X.US"), 2);
    }

    #[test]
    fn tracker_unknown_symbol_is_zero() {
        let t = OscillationTracker::new();
        assert_eq!(t.oscillation_count("NOPE.US"), 0);
        assert!(!t.is_noisy("NOPE.US"));
        assert!(!t.is_quiet_reset("NOPE.US"));
    }

    #[test]
    fn is_noisy_fires_at_threshold() {
        let mut t = OscillationTracker::with_window(20);
        // 3 transitions: T F T F
        for p in [true, false, true, false] {
            t.observe("COIN.US", p);
        }
        assert_eq!(t.oscillation_count("COIN.US"), 3);
        assert!(t.is_noisy("COIN.US"));
    }

    #[test]
    fn quiet_reset_clears_noisy_flag() {
        let mut t = OscillationTracker::with_window(20);
        for p in [true, false, true, false, true] {
            t.observe("COIN.US", p);
        }
        assert!(t.is_noisy("COIN.US"));

        // Now absent for 5 straight cycles — should clear.
        for _ in 0..QUIET_RESET_CYCLES {
            t.observe("COIN.US", false);
        }
        assert!(t.is_quiet_reset("COIN.US"));
        assert!(!t.is_noisy("COIN.US"));
    }

    #[test]
    fn noisy_symbols_excludes_quiet_and_clean() {
        let mut t = OscillationTracker::with_window(20);
        // COIN — noisy
        for p in [true, false, true, false, true] {
            t.observe("COIN.US", p);
        }
        // STABLE — always present, 0 transitions
        for _ in 0..10 {
            t.observe("STABLE.US", true);
        }
        // RESET — noisy but then reset
        for p in [true, false, true, false, true] {
            t.observe("RESET.US", p);
        }
        for _ in 0..QUIET_RESET_CYCLES {
            t.observe("RESET.US", false);
        }

        let noisy = t.noisy_symbols();
        assert!(noisy.contains(&"COIN.US".to_string()));
        assert!(!noisy.contains(&"STABLE.US".to_string()));
        assert!(!noisy.contains(&"RESET.US".to_string()));
    }

    #[test]
    fn tracked_symbols_returns_every_observed_symbol() {
        let mut t = OscillationTracker::with_window(20);
        t.observe("A.US", true);
        t.observe("B.US", false);
        let tracked = t.tracked_symbols();
        assert!(tracked.contains(&"A.US".to_string()));
        assert!(tracked.contains(&"B.US".to_string()));
    }

    #[test]
    fn all_counts_returns_every_tracked_symbol() {
        let mut t = OscillationTracker::with_window(20);
        t.observe("A.US", true);
        t.observe("B.US", false);
        let snap: std::collections::HashMap<_, _> = t.all_counts().into_iter().collect();
        assert_eq!(snap.get("A.US"), Some(&0));
        assert_eq!(snap.get("B.US"), Some(&0));
    }
}
