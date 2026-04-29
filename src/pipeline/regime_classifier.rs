//! Regime data inputs (RegimeType + classify deleted per first-principles
//! audit).
//!
//! The categorical RegimeType enum (BlowOffTop / OrderlyTrend /
//! RangeCompression / Coiling / Capitulation / Mixed) was rule-based
//! if-else bucketing on stress / synchrony / bull-bear ratio. Six
//! buckets covering a continuous regime space — exactly the kind of
//! pre-defined categorical taxonomy first-principles audit rejects.
//!
//! What's kept: `RegimeInputs` struct + `from_live` constructor.
//! These are pure data transport — read live world-state metrics into
//! a typed bag, then `regime_fingerprint::build_us_fingerprint`
//! consumes them to produce a continuous quantized 5-dim signature.
//! The continuous fingerprint captures the same regime structure
//! without categorical overlay.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy)]
pub struct RegimeInputs {
    pub stress: f64,
    pub synchrony: f64,
    pub planner_utility: f64,
    /// bull count / short count — 1.0 is neutral; >1 bullish, <1 bearish.
    pub bull_bear_ratio: f64,
    pub active_count: usize,
    pub planner_utility_trend_24_cycle: f64,
    pub bull_bear_trend_24_cycle: f64,
}

impl RegimeInputs {
    pub fn from_live(
        stress: Option<Decimal>,
        synchrony: Option<Decimal>,
        planner_utility: Option<Decimal>,
        bull_count: usize,
        bear_count: usize,
        active_count: usize,
        planner_utility_trend_24_cycle: f64,
        bull_bear_trend_24_cycle: f64,
    ) -> Self {
        RegimeInputs {
            stress: stress.and_then(|d| d.to_f64()).unwrap_or(0.0),
            synchrony: synchrony.and_then(|d| d.to_f64()).unwrap_or(0.0),
            planner_utility: planner_utility.and_then(|d| d.to_f64()).unwrap_or(0.0),
            bull_bear_ratio: if bear_count == 0 {
                bull_count as f64
            } else {
                bull_count as f64 / bear_count.max(1) as f64
            },
            active_count,
            planner_utility_trend_24_cycle,
            bull_bear_trend_24_cycle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bull_bear_ratio_from_counts_handles_zero_bear() {
        let inputs = RegimeInputs::from_live(
            None, None, None, 10, 0, // zero bears → ratio = 10 (not divide-by-zero)
            10, 0.0, 0.0,
        );
        assert_eq!(inputs.bull_bear_ratio, 10.0);
    }
}
