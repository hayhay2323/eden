use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;

fn default_bucket_session() -> crate::ontology::horizon::HorizonBucket {
    crate::ontology::horizon::HorizonBucket::Session
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredArchetypeRecord {
    pub archetype_id: String,
    pub market: String,
    pub archetype_key: String,
    pub label: String,
    pub topology: Option<String>,
    pub temporal_shape: Option<String>,
    pub conflict_shape: Option<String>,
    pub dominant_channels: Vec<String>,
    pub expectation_violation_kinds: Vec<String>,
    pub family_label: Option<String>,
    /// Trading horizon bucket. New in Wave 3.
    /// Pre-Wave-3 records lazily deserialize with `bucket = Session`,
    /// matching the legacy "intraday" → Session mapping rule.
    #[serde(default = "default_bucket_session")]
    pub bucket: crate::ontology::horizon::HorizonBucket,
    pub samples: u64,
    pub hits: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub hit_rate: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_net_return: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_affinity: Decimal,
    pub updated_at: String,
    /// Outcome distribution counts. New in Wave 4.
    /// Pre-Wave-4 records deserialize with all counts = 0.
    #[serde(default)]
    pub confirmed_count: u64,
    #[serde(default)]
    pub invalidated_count: u64,
    #[serde(default)]
    pub profitable_but_late_count: u64,
    #[serde(default)]
    pub partially_confirmed_count: u64,
    #[serde(default)]
    pub exhausted_count: u64,
    #[serde(default)]
    pub early_exited_count: u64,
    #[serde(default)]
    pub structurally_right_count: u64,
}

/// Build the canonical archetype key.
///
/// Wave 3 changes the main learning key from `(intent_kind, signature)` to
/// `(intent_kind, bucket, signature)` so that buckets become a first-class
/// learning dimension. Pre-Wave-3 records that lacked bucket get `Session`
/// as default on deserialization.
pub fn build_archetype_key(
    intent_kind: &str,
    bucket: crate::ontology::horizon::HorizonBucket,
    signature: &str,
) -> String {
    let bucket_str = match bucket {
        crate::ontology::horizon::HorizonBucket::Tick50 => "tick50",
        crate::ontology::horizon::HorizonBucket::Fast5m => "fast5m",
        crate::ontology::horizon::HorizonBucket::Mid30m => "mid30m",
        crate::ontology::horizon::HorizonBucket::Session => "session",
        crate::ontology::horizon::HorizonBucket::MultiSession => "multi_session",
    };
    format!("{intent_kind}:{bucket_str}:{signature}")
}

impl DiscoveredArchetypeRecord {
    pub fn record_id(&self) -> &str {
        &self.archetype_id
    }
}

pub fn build_discovered_archetypes(
    market: &str,
    assessments: &[CaseReasoningAssessmentRecord],
    outcomes: &[CaseRealizedOutcomeRecord],
    recorded_at: OffsetDateTime,
) -> Vec<DiscoveredArchetypeRecord> {
    let latest_assessment_by_setup = assessments
        .iter()
        .filter(|assessment| assessment.market == market)
        .fold(
            std::collections::HashMap::<&str, &CaseReasoningAssessmentRecord>::new(),
            |mut acc, assessment| {
                match acc.get(assessment.setup_id.as_str()) {
                    Some(existing) if existing.recorded_at >= assessment.recorded_at => {}
                    _ => {
                        acc.insert(assessment.setup_id.as_str(), assessment);
                    }
                }
                acc
            },
        );

    let mut grouped = std::collections::HashMap::<
        String,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Vec<String>,
            Vec<String>,
            Option<String>,
            u64,
            u64,
            Decimal,
            Decimal,
            crate::ontology::horizon::HorizonBucket,
        ),
    >::new();

    for outcome in outcomes.iter().filter(|outcome| outcome.market == market) {
        let Some(assessment) = latest_assessment_by_setup
            .get(outcome.setup_id.as_str())
            .copied()
        else {
            continue;
        };

        // Derive the canonical (intent_kind, bucket, signature) triple so that
        // the key matches exactly what recompute_archetype_shard_distribution looks up.
        let intent_kind = assessment
            .inferred_intent
            .as_ref()
            .map(|i| format!("{:?}", i.kind).to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".into());
        let bucket = assessment
            .primary_horizon
            .unwrap_or(crate::ontology::horizon::HorizonBucket::Session);
        let case_sig_str = assessment
            .case_signature
            .as_ref()
            .map(|s| {
                format!(
                    "{}:{}:{}",
                    format!("{:?}", s.topology).to_ascii_lowercase(),
                    format!("{:?}", s.temporal_shape).to_ascii_lowercase(),
                    format!("{:?}", s.conflict_shape).to_ascii_lowercase(),
                )
            })
            .unwrap_or_else(|| "unknown:unknown:unknown".into());
        let key = build_archetype_key(&intent_kind, bucket, &case_sig_str);

        let projection = assessment
            .archetype_projections
            .iter()
            .max_by(|left, right| left.affinity.cmp(&right.affinity));
        let label = projection
            .map(|projection| projection.label.clone())
            .or_else(|| assessment.family_label.clone())
            .unwrap_or_else(|| intent_kind.clone());
        let topology = assessment
            .case_signature
            .as_ref()
            .map(|signature| format!("{:?}", signature.topology).to_ascii_lowercase());
        let temporal_shape = assessment
            .case_signature
            .as_ref()
            .map(|signature| format!("{:?}", signature.temporal_shape).to_ascii_lowercase());
        let conflict_shape = assessment
            .case_signature
            .as_ref()
            .map(|signature| format!("{:?}", signature.conflict_shape).to_ascii_lowercase());
        let dominant_channels = assessment
            .case_signature
            .as_ref()
            .map(|signature| {
                signature
                    .active_channels
                    .iter()
                    .map(|channel| format!("{channel:?}").to_ascii_lowercase())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let violation_kinds = assessment
            .expectation_violations
            .iter()
            .map(|violation| format!("{:?}", violation.kind).to_ascii_lowercase())
            .collect::<Vec<_>>();
        let affinity = projection
            .map(|projection| projection.affinity)
            .unwrap_or(Decimal::ZERO);
        let hit = u64::from(outcome.followed_through && outcome.net_return > Decimal::ZERO);

        let entry = grouped.entry(key.clone()).or_insert((
            label,
            topology,
            temporal_shape,
            conflict_shape,
            dominant_channels,
            violation_kinds,
            assessment.family_label.clone(),
            0,
            0,
            Decimal::ZERO,
            Decimal::ZERO,
            bucket,
        ));
        entry.7 += 1;
        entry.8 += hit;
        entry.9 += outcome.net_return;
        entry.10 += affinity;
    }

    let updated_at = recorded_at
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| recorded_at.to_string());

    let mut records = grouped
        .into_iter()
        .map(
            |(
                archetype_key,
                (
                    label,
                    topology,
                    temporal_shape,
                    conflict_shape,
                    dominant_channels,
                    expectation_violation_kinds,
                    family_label,
                    samples,
                    hits,
                    total_net_return,
                    total_affinity,
                    bucket,
                ),
            )| {
                let samples_decimal = Decimal::from(samples.max(1));
                DiscoveredArchetypeRecord {
                    archetype_id: format!("archetype:{market}:{archetype_key}"),
                    market: market.to_string(),
                    archetype_key,
                    label,
                    topology,
                    temporal_shape,
                    conflict_shape,
                    dominant_channels,
                    expectation_violation_kinds,
                    family_label,
                    bucket,
                    samples,
                    hits,
                    hit_rate: Decimal::from(hits) / samples_decimal,
                    mean_net_return: total_net_return / samples_decimal,
                    mean_affinity: total_affinity / samples_decimal,
                    updated_at: updated_at.clone(),
                    confirmed_count: 0,
                    invalidated_count: 0,
                    profitable_but_late_count: 0,
                    partially_confirmed_count: 0,
                    exhausted_count: 0,
                    early_exited_count: 0,
                    structurally_right_count: 0,
                }
            },
        )
        .collect::<Vec<_>>();

    records.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| right.mean_net_return.cmp(&left.mean_net_return))
            .then_with(|| left.archetype_key.cmp(&right.archetype_key))
    });
    records.truncate(64);
    records
}

/// Recompute the outcome distribution counts for a single archetype shard
/// `(intent_kind, bucket, signature)` from the authoritative case_resolution
/// table and write the updated counts back to the discovered_archetype record.
///
/// This function is the source-of-truth refresh: it queries
/// `case_resolution` directly for records matching this shard (no row-count
/// cap), counts by resolution kind, then updates the archetype record's
/// distribution fields and writes it back.
///
/// Note: `load_all_case_resolutions` (capped at 10k rows) is intentionally
/// NOT used here — once `case_resolution` grows past 10k rows the full-scan
/// approach would silently truncate samples and drift distribution counts
/// downward. Use `load_case_resolutions_for_shard` instead.
///
/// Feature-gated: only compiled when `persistence` feature is active.
#[cfg(feature = "persistence")]
pub async fn recompute_archetype_shard_distribution(
    store: &crate::persistence::store::EdenStore,
    intent_kind: &str,
    bucket: crate::ontology::horizon::HorizonBucket,
    signature: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::ontology::resolution::CaseResolutionKind;

    let shard_resolutions = store
        .load_case_resolutions_for_shard(intent_kind, bucket, signature)
        .await?;
    let mut counts = OutcomeCounts::default();
    for record in shard_resolutions {
        match record.resolution.kind {
            CaseResolutionKind::Confirmed => counts.confirmed += 1,
            CaseResolutionKind::PartiallyConfirmed => counts.partially_confirmed += 1,
            CaseResolutionKind::Invalidated => counts.invalidated += 1,
            CaseResolutionKind::Exhausted => counts.exhausted += 1,
            CaseResolutionKind::ProfitableButLate => counts.profitable_but_late += 1,
            CaseResolutionKind::EarlyExited => counts.early_exited += 1,
            CaseResolutionKind::StructurallyRightButUntradeable => counts.structurally_right += 1,
        }
    }

    let archetype_key = build_archetype_key(intent_kind, bucket, signature);
    if let Some(mut record) = store.load_archetype_by_key(&archetype_key).await? {
        record.confirmed_count = counts.confirmed;
        record.partially_confirmed_count = counts.partially_confirmed;
        record.invalidated_count = counts.invalidated;
        record.exhausted_count = counts.exhausted;
        record.profitable_but_late_count = counts.profitable_but_late;
        record.early_exited_count = counts.early_exited;
        record.structurally_right_count = counts.structurally_right;
        store.write_archetypes(&[record]).await?;
    }
    Ok(())
}

#[cfg(feature = "persistence")]
#[derive(Default)]
struct OutcomeCounts {
    confirmed: u64,
    partially_confirmed: u64,
    invalidated: u64,
    exhausted: u64,
    profitable_but_late: u64,
    early_exited: u64,
    structurally_right: u64,
}

#[cfg(test)]
mod horizon_key_tests {
    use super::*;
    use crate::ontology::horizon::HorizonBucket;
    use rust_decimal_macros::dec;

    fn sample_record() -> DiscoveredArchetypeRecord {
        DiscoveredArchetypeRecord {
            archetype_id: "id-1".into(),
            market: "us".into(),
            archetype_key: "intent:fast5m:sig".into(),
            label: "test".into(),
            topology: None,
            temporal_shape: None,
            conflict_shape: None,
            dominant_channels: vec![],
            expectation_violation_kinds: vec![],
            family_label: None,
            bucket: HorizonBucket::Fast5m,
            samples: 10,
            hits: 5,
            hit_rate: dec!(0.5),
            mean_net_return: dec!(0.0),
            mean_affinity: dec!(0.0),
            updated_at: "2026-04-12T00:00:00Z".into(),
            confirmed_count: 0,
            invalidated_count: 0,
            profitable_but_late_count: 0,
            partially_confirmed_count: 0,
            exhausted_count: 0,
            early_exited_count: 0,
            structurally_right_count: 0,
        }
    }

    #[test]
    fn archetype_record_has_outcome_distribution_fields() {
        let record = DiscoveredArchetypeRecord {
            confirmed_count: 3,
            invalidated_count: 1,
            profitable_but_late_count: 2,
            partially_confirmed_count: 0,
            exhausted_count: 5,
            early_exited_count: 1,
            structurally_right_count: 0,
            ..sample_record()
        };
        assert_eq!(record.confirmed_count, 3);
        assert_eq!(record.profitable_but_late_count, 2);
    }

    #[test]
    fn legacy_archetype_without_distribution_deserializes_as_zero() {
        // JSON from before Wave 4 lacks the distribution fields
        let json = r#"{
            "archetype_id": "id-1",
            "market": "us",
            "archetype_key": "intent:fast5m:sig",
            "label": "test",
            "topology": null,
            "temporal_shape": null,
            "conflict_shape": null,
            "dominant_channels": [],
            "expectation_violation_kinds": [],
            "family_label": null,
            "samples": 10,
            "hits": 5,
            "hit_rate": "0.5",
            "mean_net_return": "0.0",
            "mean_affinity": "0.0",
            "updated_at": "2026-04-12T00:00:00Z",
            "bucket": "fast5m"
        }"#;
        let record: DiscoveredArchetypeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.confirmed_count, 0);
        assert_eq!(record.profitable_but_late_count, 0);
    }

    #[test]
    fn archetype_key_roundtrip_build_recompute_match() {
        let ik = "failed_propagation";
        let bucket = HorizonBucket::Fast5m;
        let sig = "flow+absence";
        let build_key = build_archetype_key(ik, bucket, sig);
        let recompute_key = build_archetype_key(ik, bucket, sig);
        assert_eq!(build_key, recompute_key);
        assert!(
            build_key.contains("fast5m"),
            "key should contain bucket token"
        );
        assert!(
            !build_key.contains("session"),
            "Session is not the default anymore"
        );
    }

    #[test]
    fn archetype_key_includes_bucket() {
        let key = build_archetype_key("failed_propagation", HorizonBucket::Fast5m, "sig-abc");
        assert_eq!(key, "failed_propagation:fast5m:sig-abc");
    }

    #[test]
    fn archetype_key_session_format() {
        let key = build_archetype_key("breakout_contagion", HorizonBucket::Session, "sig-1");
        assert_eq!(key, "breakout_contagion:session:sig-1");
    }

    #[test]
    fn archetype_key_multi_session_format() {
        let key = build_archetype_key("regime_shift", HorizonBucket::MultiSession, "sig-2");
        assert_eq!(key, "regime_shift:multi_session:sig-2");
    }

    #[test]
    fn legacy_record_deserializes_with_session_default() {
        // Pre-Wave-3 records lack the `bucket` field. They should deserialize
        // with `bucket = Session` as the conservative default.
        let json = r#"{
            "archetype_id": "id-1",
            "market": "us",
            "archetype_key": "failed_propagation::sig-abc",
            "label": "legacy",
            "topology": null,
            "temporal_shape": null,
            "conflict_shape": null,
            "dominant_channels": [],
            "expectation_violation_kinds": [],
            "family_label": null,
            "samples": 10,
            "hits": 5,
            "hit_rate": "0.5",
            "mean_net_return": "0.0",
            "mean_affinity": "0.0",
            "updated_at": "2026-04-12T00:00:00Z"
        }"#;
        let record: DiscoveredArchetypeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.bucket, HorizonBucket::Session);
    }
}
