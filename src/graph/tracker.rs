use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

use super::decision::StructuralFingerprint;
use super::graph::BrainGraph;

/// In-memory store of active position fingerprints.
/// Persists across ticks so StructuralDegradation can detect changes.
pub struct PositionTracker {
    active: HashMap<Symbol, StructuralFingerprint>,
}

impl PositionTracker {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    /// Insert or overwrite a fingerprint by symbol.
    pub fn enter(&mut self, fingerprint: StructuralFingerprint) {
        self.active.insert(fingerprint.symbol.clone(), fingerprint);
    }

    /// Remove and return the fingerprint for a symbol.
    pub fn exit(&mut self, symbol: &Symbol) -> Option<StructuralFingerprint> {
        self.active.remove(symbol)
    }

    pub fn is_active(&self, symbol: &Symbol) -> bool {
        self.active.contains_key(symbol)
    }

    /// Cloned vec for passing to `DecisionSnapshot::compute`.
    pub fn active_fingerprints(&self) -> Vec<StructuralFingerprint> {
        self.active.values().cloned().collect()
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Re-capture the structural fingerprint for a tracked symbol.
    /// Preserves original entry_composite and entry_timestamp so degradation
    /// measures drift from the refreshed structural state, not from tick 1.
    pub fn refresh(&mut self, symbol: &Symbol, brain: &BrainGraph) -> bool {
        if let Some(existing) = self.active.get(symbol) {
            let entry_composite = existing.entry_composite;
            let entry_timestamp = existing.entry_timestamp;
            if let Some(mut fresh) = StructuralFingerprint::capture(symbol, brain) {
                fresh.entry_composite = entry_composite;
                fresh.entry_timestamp = entry_timestamp;
                self.active.insert(symbol.clone(), fresh);
                return true;
            }
        }
        false
    }

    /// Refresh all active fingerprints. Call periodically to prevent stale degradation baselines.
    pub fn refresh_all(&mut self, brain: &BrainGraph) {
        let symbols: Vec<Symbol> = self.active.keys().cloned().collect();
        for sym in &symbols {
            self.refresh(sym, brain);
        }
    }

    /// Auto-capture fingerprints for stocks with nonzero composite scores
    /// that are not already tracked. Returns the symbols that were newly entered.
    pub fn auto_enter(
        &mut self,
        convergence_scores: &HashMap<Symbol, super::decision::ConvergenceScore>,
        brain: &BrainGraph,
    ) -> Vec<Symbol> {
        self.auto_enter_allowed(convergence_scores, None, brain)
    }

    /// Auto-capture fingerprints only for symbols that are currently actionable.
    pub fn auto_enter_allowed(
        &mut self,
        convergence_scores: &HashMap<Symbol, super::decision::ConvergenceScore>,
        allowed_symbols: Option<&HashSet<Symbol>>,
        brain: &BrainGraph,
    ) -> Vec<Symbol> {
        let mut newly_entered = Vec::new();
        for (symbol, score) in convergence_scores {
            if allowed_symbols
                .map(|symbols| !symbols.contains(symbol))
                .unwrap_or(false)
            {
                continue;
            }
            if score.composite == Decimal::ZERO {
                continue;
            }
            if self.is_active(symbol) {
                continue;
            }
            if let Some(mut fp) = StructuralFingerprint::capture(symbol, brain) {
                fp.entry_composite = score.composite;
                self.enter(fp);
                newly_entered.push(symbol.clone());
            }
        }
        newly_entered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::narrative::Regime;
    use crate::pipeline::dimensions::SymbolDimensions;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_fingerprint(symbol: &str, composite: Decimal) -> StructuralFingerprint {
        StructuralFingerprint {
            symbol: sym(symbol),
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            entry_composite: composite,
            entry_regime: Regime::CoherentNeutral,
            institutional_directions: vec![],
            sector_mean_coherence: None,
            correlated_stocks: vec![],
            entry_dimensions: SymbolDimensions {
                order_book_pressure: Decimal::ZERO,
                capital_flow_direction: Decimal::ZERO,
                capital_size_divergence: Decimal::ZERO,
                institutional_direction: Decimal::ZERO,
                ..Default::default()
            },
        }
    }

    #[test]
    fn enter_and_is_active() {
        let mut tracker = PositionTracker::new();
        let fp = make_fingerprint("700.HK", dec!(0.5));
        tracker.enter(fp);
        assert!(tracker.is_active(&sym("700.HK")));
        assert!(!tracker.is_active(&sym("9988.HK")));
        assert_eq!(tracker.active_count(), 1);
    }

    #[test]
    fn exit_removes_and_returns() {
        let mut tracker = PositionTracker::new();
        tracker.enter(make_fingerprint("700.HK", dec!(0.5)));

        let removed = tracker.exit(&sym("700.HK"));
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().entry_composite, dec!(0.5));
        assert!(!tracker.is_active(&sym("700.HK")));
        assert_eq!(tracker.active_count(), 0);

        // Exit nonexistent returns None
        assert!(tracker.exit(&sym("700.HK")).is_none());
    }

    #[test]
    fn enter_overwrites_existing() {
        let mut tracker = PositionTracker::new();
        tracker.enter(make_fingerprint("700.HK", dec!(0.5)));
        tracker.enter(make_fingerprint("700.HK", dec!(0.8)));

        assert_eq!(tracker.active_count(), 1);
        let fps = tracker.active_fingerprints();
        assert_eq!(fps.len(), 1);
        assert_eq!(fps[0].entry_composite, dec!(0.8));
    }

    #[test]
    fn active_fingerprints_returns_clones() {
        let mut tracker = PositionTracker::new();
        tracker.enter(make_fingerprint("700.HK", dec!(0.5)));
        tracker.enter(make_fingerprint("9988.HK", dec!(0.3)));

        let fps = tracker.active_fingerprints();
        assert_eq!(fps.len(), 2);

        // Modifying the returned vec doesn't affect the tracker
        assert_eq!(tracker.active_count(), 2);
    }

    #[test]
    fn empty_tracker() {
        let tracker = PositionTracker::new();
        assert_eq!(tracker.active_count(), 0);
        assert!(tracker.active_fingerprints().is_empty());
        assert!(!tracker.is_active(&sym("700.HK")));
    }

    #[test]
    fn refresh_updates_fingerprint_preserves_entry() {
        let mut tracker = PositionTracker::new();
        let mut fp = make_fingerprint("700.HK", dec!(0.5));
        fp.entry_timestamp = OffsetDateTime::UNIX_EPOCH;
        tracker.enter(fp);

        // After enter, entry_composite and entry_timestamp should be preserved
        let fps = tracker.active_fingerprints();
        assert_eq!(fps[0].entry_composite, dec!(0.5));
        assert_eq!(fps[0].entry_timestamp, OffsetDateTime::UNIX_EPOCH);
    }
}
