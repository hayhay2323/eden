use super::signals::CrossMarketAnalyzer;
use super::types::{CoordinatorEvent, CoordinatorSnapshot};

/// Coordinator state tracking both markets.
pub struct CoordinatorState {
    pub hk_tick: Option<u64>,
    pub us_tick: Option<u64>,
    pub hk_regime: Option<String>,
    pub us_regime: Option<String>,
    pub hk_stress: Option<f64>,
    pub us_stress: Option<f64>,
    pub latest_snapshot: CoordinatorSnapshot,
}

impl CoordinatorState {
    pub fn new() -> Self {
        Self {
            hk_tick: None,
            us_tick: None,
            hk_regime: None,
            us_regime: None,
            hk_stress: None,
            us_stress: None,
            latest_snapshot: CoordinatorSnapshot::empty(),
        }
    }

    pub fn both_markets_active(&self) -> bool {
        self.hk_tick.is_some() && self.us_tick.is_some()
    }
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self::new()
    }
}

/// The market coordinator that orchestrates cross-market reasoning.
pub struct MarketCoordinator {
    state: CoordinatorState,
}

impl MarketCoordinator {
    pub fn new() -> Self {
        Self {
            state: CoordinatorState::new(),
        }
    }

    /// Process an incoming coordinator event.
    pub fn handle_event(&mut self, event: CoordinatorEvent) -> Option<CoordinatorSnapshot> {
        match event {
            CoordinatorEvent::HkUpdate { tick, .. } => {
                self.state.hk_tick = Some(tick);
            }
            CoordinatorEvent::UsUpdate { tick, .. } => {
                self.state.us_tick = Some(tick);
            }
            CoordinatorEvent::ScheduledCheck => {}
        }

        if self.state.both_markets_active() {
            Some(self.analyze())
        } else {
            None
        }
    }

    /// Update HK market state for cross-market analysis.
    pub fn update_hk(&mut self, regime: Option<String>, stress: Option<f64>) {
        self.state.hk_regime = regime;
        self.state.hk_stress = stress;
    }

    /// Update US market state for cross-market analysis.
    pub fn update_us(&mut self, regime: Option<String>, stress: Option<f64>) {
        self.state.us_regime = regime;
        self.state.us_stress = stress;
    }

    /// Run cross-market analysis and produce a snapshot.
    pub fn analyze(&mut self) -> CoordinatorSnapshot {
        let divergences = CrossMarketAnalyzer::detect_divergences(
            self.state.hk_regime.as_deref(),
            self.state.us_regime.as_deref(),
            self.state.hk_stress,
            self.state.us_stress,
        );

        let hypotheses = CrossMarketAnalyzer::generate_hypotheses(&divergences);

        let snapshot = CoordinatorSnapshot {
            generated_at: String::new(), // caller fills timestamp
            hk_tick: self.state.hk_tick,
            us_tick: self.state.us_tick,
            divergences,
            cross_market_hypotheses: hypotheses,
        };

        self.state.latest_snapshot = snapshot.clone();
        snapshot
    }

    /// Get the latest coordinator snapshot.
    pub fn latest_snapshot(&self) -> &CoordinatorSnapshot {
        &self.state.latest_snapshot
    }

    /// Get coordinator state reference.
    pub fn state(&self) -> &CoordinatorState {
        &self.state
    }
}

impl Default for MarketCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::CoordinatorEvent;
    use super::*;

    #[test]
    fn coordinator_requires_both_markets() {
        let mut coord = MarketCoordinator::new();
        let result = coord.handle_event(CoordinatorEvent::HkUpdate {
            tick: 1,
            timestamp: "t1".into(),
        });
        assert!(
            result.is_none(),
            "Should return None when only HK market is active"
        );
    }

    #[test]
    fn coordinator_produces_snapshot_when_both_active() {
        let mut coord = MarketCoordinator::new();
        coord.handle_event(CoordinatorEvent::HkUpdate {
            tick: 10,
            timestamp: "t1".into(),
        });
        let snapshot = coord.handle_event(CoordinatorEvent::UsUpdate {
            tick: 20,
            timestamp: "t2".into(),
        });
        assert!(
            snapshot.is_some(),
            "Should produce snapshot when both active"
        );
        let snap = snapshot.unwrap();
        assert_eq!(snap.hk_tick, Some(10));
        assert_eq!(snap.us_tick, Some(20));
    }
}
