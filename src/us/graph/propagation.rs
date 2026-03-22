//! Cross-market signal propagation: HK institutional signals → US dual-listed stocks.
//!
//! The HK market closes at 16:00 HKT (08:00 UTC). The US pre-market opens at 04:00 ET (08:00/09:00 UTC).
//! During this overlap window, HK institutional positioning (from broker queues) propagates as
//! external priors into the US tactical system — analogous to how Polymarket priors work for HK.
//!
//! Signal decays linearly: full strength at HK close, half at 3 hours, zero at 6 hours.

use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::us::watchlist::CROSS_MARKET_PAIRS;

// ── HK Snapshot (deserialize only the fields we need) ──

/// Minimal representation of one signal entry from data/live_snapshot.json `top_signals`.
#[derive(Debug, Clone, Deserialize)]
pub struct HkSignalEntry {
    pub symbol: String,
    pub composite: Decimal,
    pub institutional_alignment: Decimal,
    #[serde(default)]
    pub sector_coherence: Option<Decimal>,
    #[serde(default)]
    pub cross_stock_correlation: Decimal,
    #[serde(default)]
    pub mark_price: Option<Decimal>,
}

/// Minimal representation of data/live_snapshot.json.
/// We only deserialize the fields needed for cross-market propagation.
#[derive(Debug, Clone, Deserialize)]
pub struct HkSnapshot {
    pub timestamp: String,
    #[serde(default)]
    pub top_signals: Vec<HkSignalEntry>,
}

// ── Cross-market signal ──

/// A propagated signal from the HK market for a dual-listed stock.
/// These become external priors in the US decision system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossMarketSignal {
    pub hk_symbol: Symbol,
    pub us_symbol: Symbol,
    /// HK composite convergence score (direction + magnitude).
    pub hk_composite: Decimal,
    /// HK institutional alignment (how aligned are institutions on this stock).
    pub hk_inst_alignment: Decimal,
    /// When the HK snapshot was taken.
    pub hk_timestamp: String,
    /// Minutes since HK close (affects decay).
    pub time_since_hk_close_minutes: i64,
    /// Final confidence after time decay: hk_composite.abs() * decay_factor.
    /// Positive = bullish HK signal, negative = bearish.
    pub propagation_confidence: Decimal,
}

/// Compute cross-market signals from an HK snapshot.
///
/// For each dual-listed pair (BABA.US<->9988.HK, etc), extracts the HK signal
/// and applies time decay based on minutes since HK close.
///
/// `minutes_since_hk_close`: caller provides this (current UTC time - 08:00 UTC).
pub fn compute_cross_market_signals(
    hk_snapshot: &HkSnapshot,
    minutes_since_hk_close: i64,
) -> Vec<CrossMarketSignal> {
    // Build lookup: HK symbol string -> signal entry
    let hk_signals: HashMap<&str, &HkSignalEntry> = hk_snapshot
        .top_signals
        .iter()
        .map(|s| (s.symbol.as_str(), s))
        .collect();

    let decay = time_decay(minutes_since_hk_close);
    if decay == Decimal::ZERO {
        return Vec::new();
    }

    CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let hk_signal = hk_signals.get(pair.hk_symbol)?;

            // Skip negligible signals
            if hk_signal.composite == Decimal::ZERO {
                return None;
            }

            // propagation_confidence preserves direction: sign(composite) * |composite| * decay
            let propagation_confidence = hk_signal.composite * decay;

            Some(CrossMarketSignal {
                hk_symbol: Symbol(pair.hk_symbol.to_string()),
                us_symbol: Symbol(pair.us_symbol.to_string()),
                hk_composite: hk_signal.composite,
                hk_inst_alignment: hk_signal.institutional_alignment,
                hk_timestamp: hk_snapshot.timestamp.clone(),
                time_since_hk_close_minutes: minutes_since_hk_close,
                propagation_confidence,
            })
        })
        .collect()
}

/// Read and parse an HK snapshot from a JSON file.
pub fn read_hk_snapshot(path: &str) -> Result<HkSnapshot, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("failed to parse {path}: {e}"))
}

/// Compute minutes since HK market close (16:00 HKT = 08:00 UTC).
/// Returns 0 if the HK market hasn't closed yet today.
pub fn minutes_since_hk_close(now: OffsetDateTime) -> i64 {
    let utc_hour = now.hour() as i64;
    let utc_minute = now.minute() as i64;
    let total_minutes = utc_hour * 60 + utc_minute;
    let hk_close_utc_minutes = 8 * 60; // 08:00 UTC = 16:00 HKT

    if total_minutes < hk_close_utc_minutes {
        // Before HK close — use previous day's close (assume ~24h ago minus elapsed)
        // In practice, the caller handles this via the snapshot's own timestamp
        0
    } else {
        total_minutes - hk_close_utc_minutes
    }
}

/// Linear time decay: 1.0 at 0 minutes, 0.5 at 180 minutes, 0.0 at 360+ minutes.
///
/// This is a principled linear decay, not an arbitrary threshold.
/// The 6-hour window covers the HK close (08:00 UTC) to US regular hours (14:30 UTC).
fn time_decay(minutes_since_close: i64) -> Decimal {
    const FULL_DECAY_MINUTES: i64 = 360; // 6 hours

    if minutes_since_close <= 0 {
        Decimal::ONE
    } else if minutes_since_close >= FULL_DECAY_MINUTES {
        Decimal::ZERO
    } else {
        // Linear: 1.0 - (minutes / 360)
        Decimal::ONE - Decimal::from(minutes_since_close) / Decimal::from(FULL_DECAY_MINUTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_hk_signal(symbol: &str, composite: Decimal, inst_alignment: Decimal) -> HkSignalEntry {
        HkSignalEntry {
            symbol: symbol.into(),
            composite,
            institutional_alignment: inst_alignment,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            mark_price: None,
        }
    }

    fn make_snapshot(signals: Vec<HkSignalEntry>) -> HkSnapshot {
        HkSnapshot {
            timestamp: "2026-03-20T08:00:00Z".into(),
            top_signals: signals,
        }
    }

    // ── time_decay ──

    #[test]
    fn decay_at_zero_minutes() {
        assert_eq!(time_decay(0), dec!(1));
    }

    #[test]
    fn decay_at_180_minutes() {
        // 180 / 360 = 0.5 → 1.0 - 0.5 = 0.5
        assert_eq!(time_decay(180), dec!(0.5));
    }

    #[test]
    fn decay_at_360_minutes() {
        assert_eq!(time_decay(360), dec!(0));
    }

    #[test]
    fn decay_beyond_360() {
        assert_eq!(time_decay(500), dec!(0));
    }

    #[test]
    fn decay_negative_minutes() {
        // Before close: full strength
        assert_eq!(time_decay(-10), dec!(1));
    }

    #[test]
    fn decay_at_90_minutes() {
        // 90 / 360 = 0.25 → 1.0 - 0.25 = 0.75
        assert_eq!(time_decay(90), dec!(0.75));
    }

    #[test]
    fn decay_at_270_minutes() {
        // 270 / 360 = 0.75 → 1.0 - 0.75 = 0.25
        assert_eq!(time_decay(270), dec!(0.25));
    }

    // ── compute_cross_market_signals ──

    #[test]
    fn propagates_dual_listed_signal() {
        let snap = make_snapshot(vec![
            make_hk_signal("9988.HK", dec!(0.6), dec!(0.8)), // BABA
        ]);

        let signals = compute_cross_market_signals(&snap, 0);
        assert_eq!(signals.len(), 1);

        let sig = &signals[0];
        assert_eq!(sig.hk_symbol, sym("9988.HK"));
        assert_eq!(sig.us_symbol, sym("BABA.US"));
        assert_eq!(sig.hk_composite, dec!(0.6));
        assert_eq!(sig.hk_inst_alignment, dec!(0.8));
        // At 0 minutes, decay = 1.0, so confidence = 0.6 * 1.0 = 0.6
        assert_eq!(sig.propagation_confidence, dec!(0.6));
    }

    #[test]
    fn applies_time_decay_to_confidence() {
        let snap = make_snapshot(vec![make_hk_signal("9988.HK", dec!(0.6), dec!(0.8))]);

        // At 180 minutes: decay = 0.5, confidence = 0.6 * 0.5 = 0.3
        let signals = compute_cross_market_signals(&snap, 180);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].propagation_confidence, dec!(0.3));
    }

    #[test]
    fn bearish_signal_preserves_direction() {
        let snap = make_snapshot(vec![
            make_hk_signal("9618.HK", dec!(-0.4), dec!(-0.5)), // JD
        ]);

        let signals = compute_cross_market_signals(&snap, 0);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].propagation_confidence, dec!(-0.4));
        assert_eq!(signals[0].us_symbol, sym("JD.US"));
    }

    #[test]
    fn no_signals_after_full_decay() {
        let snap = make_snapshot(vec![make_hk_signal("9988.HK", dec!(0.6), dec!(0.8))]);

        let signals = compute_cross_market_signals(&snap, 360);
        assert!(signals.is_empty());
    }

    #[test]
    fn ignores_non_dual_listed() {
        let snap = make_snapshot(vec![
            make_hk_signal("700.HK", dec!(0.9), dec!(0.7)), // Tencent: not dual-listed
            make_hk_signal("9988.HK", dec!(0.4), dec!(0.3)), // BABA: dual-listed
        ]);

        let signals = compute_cross_market_signals(&snap, 0);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].hk_symbol, sym("9988.HK"));
    }

    #[test]
    fn ignores_zero_composite() {
        let snap = make_snapshot(vec![make_hk_signal("9988.HK", dec!(0), dec!(0.5))]);

        let signals = compute_cross_market_signals(&snap, 0);
        assert!(signals.is_empty());
    }

    #[test]
    fn multiple_dual_listed_pairs() {
        let snap = make_snapshot(vec![
            make_hk_signal("9988.HK", dec!(0.5), dec!(0.6)), // BABA
            make_hk_signal("9618.HK", dec!(-0.3), dec!(-0.4)), // JD
            make_hk_signal("9888.HK", dec!(0.2), dec!(0.1)), // BIDU
        ]);

        let signals = compute_cross_market_signals(&snap, 90);
        // decay at 90 min = 0.75
        assert_eq!(signals.len(), 3);

        let baba = signals
            .iter()
            .find(|s| s.us_symbol == sym("BABA.US"))
            .unwrap();
        assert_eq!(baba.propagation_confidence, dec!(0.5) * dec!(0.75));

        let jd = signals
            .iter()
            .find(|s| s.us_symbol == sym("JD.US"))
            .unwrap();
        assert_eq!(jd.propagation_confidence, dec!(-0.3) * dec!(0.75));

        let bidu = signals
            .iter()
            .find(|s| s.us_symbol == sym("BIDU.US"))
            .unwrap();
        assert_eq!(bidu.propagation_confidence, dec!(0.2) * dec!(0.75));
    }

    #[test]
    fn empty_snapshot_returns_empty() {
        let snap = make_snapshot(vec![]);
        let signals = compute_cross_market_signals(&snap, 0);
        assert!(signals.is_empty());
    }

    // ── JSON deserialization ──

    #[test]
    fn deserializes_hk_snapshot_json() {
        let json = r#"{
            "tick": 42,
            "timestamp": "2026-03-20T08:00:00Z",
            "market_regime": {},
            "stress": {},
            "top_signals": [
                {
                    "symbol": "9988.HK",
                    "composite": "0.65",
                    "institutional_alignment": "0.80",
                    "sector_coherence": "0.30",
                    "cross_stock_correlation": "0.15",
                    "mark_price": "85.50"
                }
            ]
        }"#;

        let snap: HkSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.timestamp, "2026-03-20T08:00:00Z");
        assert_eq!(snap.top_signals.len(), 1);
        assert_eq!(snap.top_signals[0].composite, dec!(0.65));
        assert_eq!(snap.top_signals[0].institutional_alignment, dec!(0.80));
    }

    #[test]
    fn deserializes_with_missing_optional_fields() {
        let json = r#"{
            "timestamp": "2026-03-20T08:00:00Z",
            "top_signals": [
                {
                    "symbol": "9988.HK",
                    "composite": "0.5",
                    "institutional_alignment": "0.3"
                }
            ]
        }"#;

        let snap: HkSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.top_signals[0].sector_coherence, None);
        assert_eq!(snap.top_signals[0].mark_price, None);
    }

    // ── minutes_since_hk_close ──

    #[test]
    fn minutes_since_close_at_hk_close() {
        // 08:00 UTC = HK close
        let now = time::macros::datetime!(2026-03-20 08:00 UTC);
        assert_eq!(minutes_since_hk_close(now), 0);
    }

    #[test]
    fn minutes_since_close_3h_after() {
        // 11:00 UTC = 3 hours after HK close
        let now = time::macros::datetime!(2026-03-20 11:00 UTC);
        assert_eq!(minutes_since_hk_close(now), 180);
    }

    #[test]
    fn minutes_since_close_before_hk_close() {
        // 06:00 UTC = before HK close
        let now = time::macros::datetime!(2026-03-20 06:00 UTC);
        assert_eq!(minutes_since_hk_close(now), 0);
    }
}
