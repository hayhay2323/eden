//! Persistence record for `RegimeFingerprint`.
//!
//! Mirrors the runtime struct in `pipeline::regime_fingerprint` but
//! stays in the persistence module so the runtime crate doesn't depend
//! on SurrealDB types. The two types are identical in field names and
//! shapes — `RegimeFingerprint` derives Serialize/Deserialize so we
//! could re-use it directly, but keeping a dedicated record type makes
//! migration / backward-compat changes easier (the persistence record
//! can grow `#[serde(default)]` fields independently of the in-memory
//! struct).

use serde::{Deserialize, Serialize};

use crate::pipeline::regime_fingerprint::RegimeFingerprint;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeFingerprintSnapshot {
    pub market: String,
    pub tick: u64,
    /// ISO-8601 UTC string. Migrations 040 standardised snapshot_ts
    /// columns to string for chrono compatibility — we follow the
    /// same convention.
    pub snapshot_ts: String,
    pub stress: f64,
    pub synchrony: f64,
    pub bull_bias: f64,
    pub activity: f64,
    pub turn_pressure: f64,
    #[serde(default)]
    pub planner_utility: Option<f64>,
    #[serde(default)]
    pub regime_continuity: Option<f64>,
    #[serde(default)]
    pub dominant_driver: Option<String>,
    pub legacy_label: String,
    pub legacy_confidence: f64,
    pub bucket_key: String,
}

impl From<&RegimeFingerprint> for RegimeFingerprintSnapshot {
    fn from(fp: &RegimeFingerprint) -> Self {
        Self {
            market: fp.market.clone(),
            tick: fp.tick,
            snapshot_ts: fp.snapshot_ts.clone(),
            stress: fp.stress,
            synchrony: fp.synchrony,
            bull_bias: fp.bull_bias,
            activity: fp.activity,
            turn_pressure: fp.turn_pressure,
            planner_utility: fp.planner_utility,
            regime_continuity: fp.regime_continuity,
            dominant_driver: fp.dominant_driver.clone(),
            legacy_label: fp.legacy_label.clone(),
            legacy_confidence: fp.legacy_confidence,
            bucket_key: fp.bucket_key.clone(),
        }
    }
}

impl RegimeFingerprintSnapshot {
    /// Stable record id used by SurrealDB. Multiple snapshots per
    /// market coexist via the timestamp suffix.
    pub fn record_id(&self) -> String {
        // snapshot_ts is ISO-8601; replace ':' and '-' with '_' so it's
        // a safe SurrealDB id literal. Mirror of intent_belief_snapshot
        // record id pattern.
        let safe_ts: String = self
            .snapshot_ts
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("{}_{}", self.market, safe_ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::regime_classifier::RegimeInputs;
    use crate::pipeline::regime_fingerprint::{build_us_fingerprint, RegimeFingerprint};

    fn sample_fp() -> RegimeFingerprint {
        let inputs = RegimeInputs {
            stress: 0.13,
            synchrony: 0.71,
            planner_utility: 0.57,
            bull_bear_ratio: 1.31,
            active_count: 9,
            planner_utility_trend_24_cycle: 0.10,
            bull_bear_trend_24_cycle: 0.30,
        };
        build_us_fingerprint("us", 100, "2026-04-23T03:30:00Z", inputs, "orderly_trend")
    }

    #[test]
    fn snapshot_round_trip_preserves_fields() {
        let fp = sample_fp();
        let snap: RegimeFingerprintSnapshot = (&fp).into();
        assert_eq!(snap.market, fp.market);
        assert_eq!(snap.tick, fp.tick);
        assert_eq!(snap.snapshot_ts, fp.snapshot_ts);
        assert_eq!(snap.bucket_key, fp.bucket_key);
        assert_eq!(snap.legacy_label, fp.legacy_label);
        assert_eq!(snap.planner_utility, fp.planner_utility);
    }

    #[test]
    fn record_id_is_stable_and_safe() {
        let snap: RegimeFingerprintSnapshot = (&sample_fp()).into();
        let id = snap.record_id();
        assert!(id.starts_with("us_"));
        // No special chars that would break SurrealDB id parsing.
        for c in id.chars() {
            assert!(c.is_ascii_alphanumeric() || c == '_', "bad char {}", c);
        }
    }

    #[test]
    fn snapshot_serializes_optionals() {
        let fp = sample_fp();
        let snap: RegimeFingerprintSnapshot = (&fp).into();
        let json = serde_json::to_value(&snap).unwrap();
        // Required fields present
        assert_eq!(json["market"], "us");
        assert_eq!(json["bucket_key"], fp.bucket_key);
        // planner_utility is Some(0.57) for US fingerprint
        assert!(json["planner_utility"].as_f64().is_some());
        // Optional HK fields are null for US fingerprint
        assert!(json["regime_continuity"].is_null());
        assert!(json["dominant_driver"].is_null());
    }
}
