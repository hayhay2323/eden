use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{ActionDirection, ActionNode, ActionNodeStage};
use crate::us::common::dimension_composite;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};

use super::workflow::UsActionWorkflow;

// ── Structural fingerprint ──

/// Captures the structural state of a US position at entry time.
/// Used as the baseline for measuring structural degradation over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsStructuralFingerprint {
    pub symbol: Symbol,
    /// Monotonic tick counter at entry.
    pub entry_tick: u64,
    /// Last-done price at entry, if available.
    pub entry_price: Option<Decimal>,
    /// Composite signal score at entry (weighted average of all dimensions).
    pub entry_composite: Decimal,
    /// Capital flow direction at entry.
    pub entry_capital_flow: Decimal,
    /// Price momentum at entry.
    pub entry_momentum: Decimal,
    /// Volume profile at entry.
    pub entry_volume: Decimal,
}

impl UsStructuralFingerprint {
    /// Build a fingerprint from a symbol's current dimensions and a tick counter.
    pub fn capture(
        symbol: Symbol,
        tick: u64,
        price: Option<Decimal>,
        dims: &UsSymbolDimensions,
    ) -> Self {
        let entry_composite = dimension_composite(dims);
        Self {
            symbol,
            entry_tick: tick,
            entry_price: price,
            entry_composite,
            entry_capital_flow: dims.capital_flow_direction,
            entry_momentum: dims.price_momentum,
            entry_volume: dims.volume_profile,
        }
    }
}

impl ActionNode {
    pub fn from_us_position(
        fingerprint: &UsStructuralFingerprint,
        workflow: Option<&UsActionWorkflow>,
        current_tick: u64,
    ) -> Self {
        let direction = if fingerprint.entry_composite > Decimal::ZERO {
            ActionDirection::Long
        } else if fingerprint.entry_composite < Decimal::ZERO {
            ActionDirection::Short
        } else {
            ActionDirection::Neutral
        };

        Self {
            workflow_id: workflow
                .map(|workflow| workflow.workflow_id.clone())
                .unwrap_or_else(|| format!("us-position:{}", fingerprint.symbol)),
            symbol: fingerprint.symbol.clone(),
            market: fingerprint.symbol.market(),
            sector: None,
            stage: workflow
                .map(|workflow| match workflow.stage {
                    super::workflow::UsActionStage::Suggested => ActionNodeStage::Suggested,
                    super::workflow::UsActionStage::Confirmed => ActionNodeStage::Confirmed,
                    super::workflow::UsActionStage::Executed => ActionNodeStage::Executed,
                    super::workflow::UsActionStage::Monitoring => ActionNodeStage::Monitoring,
                    super::workflow::UsActionStage::Reviewed => ActionNodeStage::Reviewed,
                })
                .unwrap_or(ActionNodeStage::Monitoring),
            direction,
            entry_confidence: fingerprint.entry_composite.abs(),
            current_confidence: workflow
                .map(|workflow| workflow.current_confidence)
                .unwrap_or(fingerprint.entry_composite.abs()),
            entry_price: workflow
                .and_then(|workflow| workflow.entry_price)
                .or(fingerprint.entry_price),
            pnl: workflow.and_then(|workflow| workflow.pnl),
            age_ticks: current_tick.saturating_sub(fingerprint.entry_tick),
            degradation_score: workflow.and_then(|workflow| {
                workflow.degradation.as_ref().map(|degradation| {
                    degradation
                        .composite_drift
                        .abs()
                        .max(degradation.momentum_decay)
                        .max(degradation.volume_dry_up)
                })
            }),
            exit_forming: workflow
                .and_then(|workflow| {
                    workflow
                        .degradation
                        .as_ref()
                        .map(|degradation| degradation.should_exit)
                })
                .unwrap_or(false),
        }
    }
}

// ── Structural degradation ──

/// Measures how much the structure has changed since the fingerprint was recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsStructuralDegradation {
    pub symbol: Symbol,
    /// current composite minus entry composite (positive = improved, negative = degraded).
    pub composite_drift: Decimal,
    /// True when the capital flow direction has flipped sign relative to entry.
    pub capital_flow_reversal: bool,
    /// How much momentum has weakened (entry_momentum - current_momentum, floored at 0).
    pub momentum_decay: Decimal,
    /// How much volume has dropped (entry_volume - current_volume, floored at 0).
    pub volume_dry_up: Decimal,
    /// Number of ticks the position has been held.
    pub ticks_held: u64,
    /// True when any exit condition is met.
    pub should_exit: bool,
}

impl UsStructuralDegradation {
    /// Evaluate exit conditions and set `should_exit`.
    ///
    /// Exit if ANY of the following:
    /// - composite_drift magnitude > 0.3 in the direction opposing entry
    /// - capital_flow_reversal AND momentum_decay > 0.2
    /// - ticks_held > 500
    /// - volume_dry_up > 0.5
    fn evaluate(mut self, entry_composite: Decimal) -> Self {
        let composite_degraded = {
            // Degradation means drift is opposite in sign to entry direction
            // and the magnitude exceeds the threshold.
            let opposing_direction = if entry_composite >= Decimal::ZERO {
                self.composite_drift < Decimal::ZERO
            } else {
                self.composite_drift > Decimal::ZERO
            };
            opposing_direction && self.composite_drift.abs() > Decimal::new(3, 1)
        };

        self.should_exit = composite_degraded
            || (self.capital_flow_reversal && self.momentum_decay > Decimal::new(2, 1))
            || self.ticks_held > 500
            || self.volume_dry_up > Decimal::new(5, 1);

        self
    }
}

// ── Position tracker ──

/// In-memory store of active US position fingerprints.
/// Tracks entered positions and monitors structural degradation across ticks.
pub struct UsPositionTracker {
    positions: HashMap<Symbol, UsStructuralFingerprint>,
}

impl UsPositionTracker {
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    /// Record a new position entry. Overwrites any existing fingerprint for the symbol.
    pub fn enter(&mut self, fingerprint: UsStructuralFingerprint) {
        self.positions
            .insert(fingerprint.symbol.clone(), fingerprint);
    }

    /// Remove and return the fingerprint for a symbol. Returns None if not tracked.
    pub fn exit(&mut self, symbol: &Symbol) -> Option<UsStructuralFingerprint> {
        self.positions.remove(symbol)
    }

    /// Returns true if the symbol is currently tracked.
    pub fn is_active(&self, symbol: &Symbol) -> bool {
        self.positions.contains_key(symbol)
    }

    /// Returns references to all active fingerprints.
    pub fn active_fingerprints(&self) -> Vec<&UsStructuralFingerprint> {
        self.positions.values().collect()
    }

    /// Number of actively tracked positions.
    pub fn active_count(&self) -> usize {
        self.positions.len()
    }

    /// Compute structural degradation for a single fingerprint against current dimensions.
    pub fn compute_degradation(
        fingerprint: &UsStructuralFingerprint,
        current_dims: &UsSymbolDimensions,
        current_tick: u64,
    ) -> UsStructuralDegradation {
        let current_composite = dimension_composite(current_dims);
        let composite_drift = current_composite - fingerprint.entry_composite;

        let capital_flow_reversal = sign(current_dims.capital_flow_direction)
            != sign(fingerprint.entry_capital_flow)
            && fingerprint.entry_capital_flow != Decimal::ZERO
            && current_dims.capital_flow_direction != Decimal::ZERO;

        // Momentum decay: how much momentum has weakened from the entry level.
        // Only counts weakening (positive decay value), not improvement.
        let momentum_decay = (fingerprint.entry_momentum.abs() - current_dims.price_momentum.abs())
            .max(Decimal::ZERO);

        // Volume dry-up: fraction by which volume has dropped from entry level.
        // Only counts drops (positive value).
        let volume_dry_up = if fingerprint.entry_volume > Decimal::ZERO {
            ((fingerprint.entry_volume - current_dims.volume_profile) / fingerprint.entry_volume)
                .max(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };

        let ticks_held = current_tick.saturating_sub(fingerprint.entry_tick);

        UsStructuralDegradation {
            symbol: fingerprint.symbol.clone(),
            composite_drift,
            capital_flow_reversal,
            momentum_decay,
            volume_dry_up,
            ticks_held,
            should_exit: false,
        }
        .evaluate(fingerprint.entry_composite)
    }

    /// Returns degradation records for all positions that should exit,
    /// given the current dimension snapshot.
    pub fn auto_exit_candidates(
        &self,
        snapshot: &UsDimensionSnapshot,
    ) -> Vec<UsStructuralDegradation> {
        let current_tick = snapshot.timestamp.unix_timestamp() as u64;
        self.positions
            .values()
            .filter_map(|fp| {
                let dims = snapshot.dimensions.get(&fp.symbol)?;
                let degradation = Self::compute_degradation(fp, dims, current_tick);
                if degradation.should_exit {
                    Some(degradation)
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for UsPositionTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ──

/// Sign helper: returns 1, -1, or 0.
fn sign(value: Decimal) -> i8 {
    if value > Decimal::ZERO {
        1
    } else if value < Decimal::ZERO {
        -1
    } else {
        0
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn dims(
        capital_flow: Decimal,
        momentum: Decimal,
        volume: Decimal,
        prepost: Decimal,
        valuation: Decimal,
    ) -> UsSymbolDimensions {
        UsSymbolDimensions {
            capital_flow_direction: capital_flow,
            price_momentum: momentum,
            volume_profile: volume,
            pre_post_market_anomaly: prepost,
            valuation,
            multi_horizon_momentum: Decimal::ZERO,
        }
    }

    fn make_fingerprint(symbol: &str, tick: u64, composite: Decimal) -> UsStructuralFingerprint {
        let d = dims(
            composite,
            composite,
            composite.abs(),
            Decimal::ZERO,
            Decimal::ZERO,
        );
        UsStructuralFingerprint {
            symbol: sym(symbol),
            entry_tick: tick,
            entry_price: Some(dec!(100)),
            entry_composite: composite,
            entry_capital_flow: d.capital_flow_direction,
            entry_momentum: d.price_momentum,
            entry_volume: d.volume_profile,
        }
    }

    // ── enter / exit / is_active ──

    #[test]
    fn enter_and_is_active() {
        let mut tracker = UsPositionTracker::new();
        tracker.enter(make_fingerprint("AAPL.US", 1, dec!(0.5)));
        assert!(tracker.is_active(&sym("AAPL.US")));
        assert!(!tracker.is_active(&sym("NVDA.US")));
        assert_eq!(tracker.active_count(), 1);
    }

    #[test]
    fn exit_removes_and_returns_fingerprint() {
        let mut tracker = UsPositionTracker::new();
        tracker.enter(make_fingerprint("TSLA.US", 10, dec!(0.4)));

        let removed = tracker.exit(&sym("TSLA.US"));
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().entry_composite, dec!(0.4));
        assert!(!tracker.is_active(&sym("TSLA.US")));
        assert_eq!(tracker.active_count(), 0);

        // Exiting a non-existent position returns None.
        assert!(tracker.exit(&sym("TSLA.US")).is_none());
    }

    #[test]
    fn enter_overwrites_existing_position() {
        let mut tracker = UsPositionTracker::new();
        tracker.enter(make_fingerprint("MSFT.US", 1, dec!(0.3)));
        tracker.enter(make_fingerprint("MSFT.US", 2, dec!(0.8)));
        assert_eq!(tracker.active_count(), 1);
        let fps = tracker.active_fingerprints();
        assert_eq!(fps[0].entry_composite, dec!(0.8));
    }

    #[test]
    fn active_fingerprints_returns_all_entries() {
        let mut tracker = UsPositionTracker::new();
        tracker.enter(make_fingerprint("AAPL.US", 1, dec!(0.5)));
        tracker.enter(make_fingerprint("NVDA.US", 2, dec!(0.6)));
        assert_eq!(tracker.active_fingerprints().len(), 2);
    }

    // ── compute_degradation ──

    #[test]
    fn no_degradation_when_unchanged() {
        let fp = make_fingerprint("AAPL.US", 100, dec!(0.5));
        // Current dims match entry exactly.
        let current = dims(
            dec!(0.5),
            dec!(0.5),
            dec!(0.5),
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let deg = UsPositionTracker::compute_degradation(&fp, &current, 110);
        assert!(!deg.should_exit);
        assert_eq!(deg.ticks_held, 10);
    }

    #[test]
    fn exits_on_composite_drift_opposing_entry() {
        // Entry composite is positive; strong negative drift triggers exit.
        let fp = UsStructuralFingerprint {
            symbol: sym("TSLA.US"),
            entry_tick: 0,
            entry_price: Some(dec!(200)),
            entry_composite: dec!(0.5),
            entry_capital_flow: dec!(0.5),
            entry_momentum: dec!(0.5),
            entry_volume: dec!(0.5),
        };
        // Composite collapses to -0.5 => drift = -1.0, magnitude > 0.3, opposing entry
        let current = dims(
            dec!(-0.5),
            dec!(-0.5),
            dec!(-0.5),
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let deg = UsPositionTracker::compute_degradation(&fp, &current, 10);
        assert!(deg.composite_drift < dec!(-0.3));
        assert!(deg.should_exit);
    }

    #[test]
    fn exits_on_capital_flow_reversal_with_momentum_decay() {
        let fp = UsStructuralFingerprint {
            symbol: sym("NVDA.US"),
            entry_tick: 0,
            entry_price: None,
            entry_composite: dec!(0.4),
            entry_capital_flow: dec!(0.6), // positive inflow at entry
            entry_momentum: dec!(0.8),
            entry_volume: dec!(0.5),
        };
        // Flow reverses to negative, momentum drops significantly.
        let current = dims(
            dec!(-0.3),
            dec!(0.5),
            dec!(0.5),
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let deg = UsPositionTracker::compute_degradation(&fp, &current, 50);
        assert!(deg.capital_flow_reversal);
        assert!(deg.momentum_decay > dec!(0.2));
        assert!(deg.should_exit);
    }

    #[test]
    fn exits_on_stale_ticks_held() {
        let fp = make_fingerprint("BABA.US", 0, dec!(0.3));
        // Same dims as entry — structure has not degraded.
        let current = dims(
            dec!(0.3),
            dec!(0.3),
            dec!(0.3),
            Decimal::ZERO,
            Decimal::ZERO,
        );
        // But 501 ticks have passed.
        let deg = UsPositionTracker::compute_degradation(&fp, &current, 501);
        assert!(deg.ticks_held > 500);
        assert!(deg.should_exit);
    }

    #[test]
    fn exits_on_volume_dry_up() {
        let fp = UsStructuralFingerprint {
            symbol: sym("MSFT.US"),
            entry_tick: 0,
            entry_price: Some(dec!(300)),
            entry_composite: dec!(0.4),
            entry_capital_flow: dec!(0.4),
            entry_momentum: dec!(0.4),
            entry_volume: dec!(0.8), // high volume at entry
        };
        // Volume drops to 0 — 100% dry-up.
        let current = dims(
            dec!(0.4),
            dec!(0.4),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let deg = UsPositionTracker::compute_degradation(&fp, &current, 10);
        assert!(deg.volume_dry_up > dec!(0.5));
        assert!(deg.should_exit);
    }

    // ── auto_exit_candidates ──

    #[test]
    fn auto_exit_candidates_returns_only_failing_positions() {
        use std::collections::HashMap;

        let mut tracker = UsPositionTracker::new();

        // Healthy position.
        tracker.enter(UsStructuralFingerprint {
            symbol: sym("AAPL.US"),
            entry_tick: 0,
            entry_price: Some(dec!(150)),
            entry_composite: dec!(0.4),
            entry_capital_flow: dec!(0.4),
            entry_momentum: dec!(0.4),
            entry_volume: dec!(0.4),
        });

        // Degraded position — composite has flipped strongly.
        tracker.enter(UsStructuralFingerprint {
            symbol: sym("TSLA.US"),
            entry_tick: 0,
            entry_price: Some(dec!(200)),
            entry_composite: dec!(0.5),
            entry_capital_flow: dec!(0.5),
            entry_momentum: dec!(0.5),
            entry_volume: dec!(0.5),
        });

        let snapshot = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: {
                let mut m = HashMap::new();
                // AAPL: unchanged
                m.insert(
                    sym("AAPL.US"),
                    dims(
                        dec!(0.4),
                        dec!(0.4),
                        dec!(0.4),
                        Decimal::ZERO,
                        Decimal::ZERO,
                    ),
                );
                // TSLA: composite collapses
                m.insert(
                    sym("TSLA.US"),
                    dims(
                        dec!(-0.5),
                        dec!(-0.5),
                        dec!(-0.5),
                        Decimal::ZERO,
                        Decimal::ZERO,
                    ),
                );
                m
            },
        };

        let candidates = tracker.auto_exit_candidates(&snapshot);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].symbol, sym("TSLA.US"));
        assert!(candidates[0].should_exit);
    }
}
