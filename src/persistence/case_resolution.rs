//! Case-resolution persistence record.
//!
//! One row per tactical setup. Written first on primary horizon settle,
//! possibly upgraded on each supplemental settle. The `resolution_history`
//! is append-only — every upgrade (kind or finality) adds exactly one
//! transition. Never rewritten or collapsed.

use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;
use crate::ontology::resolution::{
    CaseResolution, CaseResolutionTransition, HorizonResolution, ResolutionSource,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolutionRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub primary_horizon: HorizonBucket,
    pub resolution: CaseResolution,
    pub resolution_source: ResolutionSource,
    /// Denormalized snapshot of horizon resolutions at the time of the
    /// latest update. NOT source of truth — the horizon_evaluation table is.
    pub horizon_resolution_snapshot: Vec<HorizonResolution>,
    /// Append-only. Every upgrade adds one transition. Never rewritten.
    pub resolution_history: Vec<CaseResolutionTransition>,
    #[serde(with = "rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Intent kind from `TacticalSetup.inferred_intent.kind`. New in Wave 4.
    /// Pre-Wave-4 records deserialize with empty string.
    #[serde(default)]
    pub intent_kind: String,
    /// Case signature from `TacticalSetup.case_signature`. New in Wave 4.
    /// Pre-Wave-4 records deserialize with empty string.
    #[serde(default)]
    pub signature: String,
}

impl CaseResolutionRecord {
    /// Construct the record_id from a setup_id. One record per setup.
    pub fn build_id(setup_id: &str) -> String {
        format!("case-resolution:{setup_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::resolution::{CaseResolutionKind, ResolutionFinality};
    use rust_decimal_macros::dec;
    use time::macros::datetime;

    fn sample() -> CaseResolutionRecord {
        CaseResolutionRecord {
            record_id: "case-resolution:setup-1".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            symbol: Some("FICO.US".into()),
            primary_horizon: HorizonBucket::Fast5m,
            resolution: CaseResolution {
                kind: CaseResolutionKind::Confirmed,
                finality: ResolutionFinality::Provisional,
                narrative: "test".into(),
                net_return: dec!(0.02),
            },
            resolution_source: ResolutionSource::Auto,
            horizon_resolution_snapshot: vec![],
            resolution_history: vec![CaseResolutionTransition {
                from_kind: None,
                from_finality: None,
                to_kind: CaseResolutionKind::Confirmed,
                to_finality: ResolutionFinality::Provisional,
                triggered_by_horizon: HorizonBucket::Fast5m,
                at: datetime!(2026-04-13 14:05 UTC),
                reason: "primary settled".into(),
            }],
            created_at: datetime!(2026-04-13 14:05 UTC),
            updated_at: datetime!(2026-04-13 14:05 UTC),
            intent_kind: String::new(),
            signature: String::new(),
        }
    }

    #[test]
    fn record_roundtrip() {
        let record = sample();
        let json = serde_json::to_string(&record).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn build_id_is_deterministic() {
        assert_eq!(
            CaseResolutionRecord::build_id("setup-7"),
            "case-resolution:setup-7",
        );
    }

    #[test]
    fn intent_kind_and_signature_roundtrip() {
        let mut record = sample();
        record.intent_kind = "accumulation".into();
        record.signature = "isolated:burst:contradictory".into();
        let json = serde_json::to_string(&record).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.intent_kind, "accumulation");
        assert_eq!(parsed.signature, "isolated:burst:contradictory");
    }

    #[test]
    fn legacy_record_without_intent_kind_deserializes_empty() {
        // Records from before Wave 4 lack intent_kind and signature.
        // Serialize a sample (which has empty strings), strip those fields,
        // then deserialize — must produce empty strings via #[serde(default)].
        let record = sample();
        let mut json_val: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        // Remove Wave 4 fields to simulate a legacy record.
        json_val.as_object_mut().unwrap().remove("intent_kind");
        json_val.as_object_mut().unwrap().remove("signature");
        let stripped = serde_json::to_string(&json_val).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&stripped).unwrap();
        assert_eq!(parsed.intent_kind, "");
        assert_eq!(parsed.signature, "");
    }

    #[test]
    fn record_with_upgrade_history_serializes() {
        let mut record = sample();
        record.resolution_history.push(CaseResolutionTransition {
            from_kind: Some(CaseResolutionKind::Confirmed),
            from_finality: Some(ResolutionFinality::Provisional),
            to_kind: CaseResolutionKind::Confirmed,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: HorizonBucket::Session,
            at: datetime!(2026-04-13 20:00 UTC),
            reason: "all horizons settled".into(),
        });
        let json = serde_json::to_string(&record).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resolution_history.len(), 2);
    }
}
