use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Events the coordinator receives from market runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorEvent {
    /// HK market produced a new snapshot.
    HkUpdate { tick: u64, timestamp: String },
    /// US market produced a new snapshot.
    UsUpdate { tick: u64, timestamp: String },
    /// Scheduled periodic analysis.
    ScheduledCheck,
}

/// Cross-market divergence signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossMarketDivergence {
    pub kind: String,
    pub description: String,
    pub hk_value: Option<f64>,
    pub us_value: Option<f64>,
    pub severity: DivergenceSeverity,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DivergenceSeverity {
    Low,
    Medium,
    High,
}

/// The coordinator's output snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorSnapshot {
    pub generated_at: String,
    pub hk_tick: Option<u64>,
    pub us_tick: Option<u64>,
    pub divergences: Vec<CrossMarketDivergence>,
    pub cross_market_hypotheses: Vec<CrossMarketHypothesis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossMarketHypothesis {
    pub id: String,
    pub label: String,
    pub description: String,
    pub confidence: f64,
    pub supporting_markets: Vec<String>,
}

impl CoordinatorSnapshot {
    pub fn empty() -> Self {
        Self {
            generated_at: String::new(),
            hk_tick: None,
            us_tick: None,
            divergences: Vec::new(),
            cross_market_hypotheses: Vec::new(),
        }
    }

    pub fn has_data(&self) -> bool {
        self.hk_tick.is_some() || self.us_tick.is_some()
    }

    pub fn with_generated_at(mut self, generated_at: OffsetDateTime) -> Self {
        let timestamp = generated_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| generated_at.to_string());
        self.generated_at = timestamp.clone();
        for divergence in &mut self.divergences {
            if divergence.detected_at.is_empty() {
                divergence.detected_at = timestamp.clone();
            }
        }
        self
    }
}
