use serde::{Deserialize, Serialize};

/// A lightweight view over the per-tick live state.
///
/// Deliberately decoupled from `LiveSnapshot` so that the context layer
/// does not depend on snapshot internals. Producers populate this from
/// whatever live data source is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveContext {
    /// Monotonically increasing tick counter.
    pub tick_count: u64,
    /// ISO-8601 timestamp of this tick.
    pub timestamp: String,
    /// Human-readable market mood label, if determined (e.g. "risk-on").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_mood: Option<String>,
    /// Current regime label (e.g. "bullish", "neutral", "bearish").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<String>,
    /// Composite stress level in [0, 1], if computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stress_level: Option<f64>,
    /// Number of active signals in this tick.
    pub active_signal_count: usize,
}

impl LiveContext {
    /// Create a minimal live context for the given tick.
    pub fn new(tick_count: u64, timestamp: String) -> Self {
        Self {
            tick_count,
            timestamp,
            market_mood: None,
            regime: None,
            stress_level: None,
            active_signal_count: 0,
        }
    }
}
