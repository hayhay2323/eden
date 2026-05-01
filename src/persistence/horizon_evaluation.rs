//! Horizon evaluation persistence — materializes `HorizonExpiry` into
//! concrete `due_at` timestamps and tracks settlement status.
//!
//! One record per horizon per case: a case with primary `Fast5m` and
//! secondary `[Mid30m, Session]` produces three records, all settled
//! independently when their `due_at` hits (or earlier on exit signal).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::horizon::{CaseHorizon, HorizonBucket, HorizonExpiry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    /// Horizon evaluation scheduled but the reference window hasn't
    /// been reached yet. No result, no resolution.
    Pending,
    /// The reference window (`due_at`) has been reached. Numeric result
    /// should be computed but the horizon-level classifier may not yet
    /// have run. Transitional state.
    Due,
    /// Fully settled with a resolution attached.
    Resolved,
    /// Exit signal or operator action ended this horizon before `due_at`.
    /// Resolution is set at the moment of early exit.
    EarlyExited,
    /// Settle could not run because the original entry-context setup
    /// was no longer present (e.g. a `pf:*` setup whose hour-direction
    /// flipped, leaving the old `setup_id` orphaned). After
    /// `DEFAULT_SETTLE_ATTEMPTS` failed lookups we stop trying — the
    /// record is preserved for audit but excluded from future settle
    /// passes.
    Expired,
}

/// Number of `settle_live_horizons_*` passes to wait for entry
/// context to become available before declaring a record `Expired`.
/// Settle runs roughly once per tick; at the production tick rate
/// observed in May 2026 (~44 s/tick) this is ~75 minutes of grace.
pub const DEFAULT_SETTLE_ATTEMPTS: u32 = 100;

fn default_settle_attempts() -> u32 {
    DEFAULT_SETTLE_ATTEMPTS
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonResult {
    pub net_return: Decimal,
    #[serde(with = "rfc3339")]
    pub resolved_at: OffsetDateTime,
    pub follow_through: Decimal,
}

/// Persistence record for one horizon evaluation. Written at case open
/// (status=Pending, result=None) and updated when settled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonEvaluationRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    pub horizon: HorizonBucket,
    pub primary: bool,
    #[serde(with = "rfc3339")]
    pub due_at: OffsetDateTime,
    pub status: EvaluationStatus,
    pub result: Option<HorizonResult>,
    /// New in Resolution System Wave 2. Written when the record transitions
    /// from Due/EarlyExited to Resolved with a classifier output. Legacy
    /// records without this field deserialize as None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<crate::ontology::resolution::HorizonResolution>,
    /// Remaining settle attempts before expiring a never-resolvable
    /// record (typically a `pf:*` setup whose hour-direction flipped
    /// and orphaned the original `setup_id`). Decremented on each
    /// settle pass that cannot find entry context; on reaching zero
    /// the record's `status` flips to `Expired` and it stops being
    /// returned by `unresolved_horizons_for_market`. Legacy records
    /// without this field deserialize at the default starting value.
    #[serde(default = "default_settle_attempts")]
    pub attempts_remaining: u32,
}

/// Decrement each record's `attempts_remaining` and expire any that
/// fall to zero. Records already in a terminal status (`Resolved`,
/// `EarlyExited`, `Expired`) are left untouched. Returns the count of
/// records that newly transitioned to `Expired` so callers can emit
/// one log per transition (instead of one per missed settle pass —
/// the source of the 4 154 / session log spam observed pre-fix).
pub fn decrement_attempts_or_expire(records: &mut [HorizonEvaluationRecord]) -> usize {
    let mut newly_expired = 0usize;
    for record in records.iter_mut() {
        match record.status {
            EvaluationStatus::Resolved
            | EvaluationStatus::EarlyExited
            | EvaluationStatus::Expired => continue,
            EvaluationStatus::Pending | EvaluationStatus::Due => {}
        }
        if record.attempts_remaining > 1 {
            record.attempts_remaining -= 1;
        } else {
            record.attempts_remaining = 0;
            record.status = EvaluationStatus::Expired;
            newly_expired += 1;
        }
    }
    newly_expired
}

impl HorizonEvaluationRecord {
    pub fn build_id(setup_id: &str, horizon: HorizonBucket) -> String {
        format!("horizon-eval:{setup_id}:{horizon:?}")
    }

    /// Convert a `HorizonExpiry` + bucket + reference timestamp into a concrete
    /// `due_at`. This is the single choke point for expiry materialization.
    ///
    /// Rules:
    /// - `UntilNextBucket` → reference + bucket's natural window length
    ///   (Fast5m = 5m, Mid30m = 30m, Session = 6h, MultiSession = 24h)
    /// - `UntilSessionClose` → reference + 6h (approximate; a calendar-aware
    ///   resolver in Wave 3+ can refine this)
    /// - `FixedTicks(n)` → reference + n seconds (one tick ≈ 1 second in runtime)
    /// - `None` → reference + 365 days (far-future sentinel)
    pub fn materialize_due_at(
        expiry: HorizonExpiry,
        bucket: HorizonBucket,
        reference: OffsetDateTime,
    ) -> OffsetDateTime {
        match expiry {
            HorizonExpiry::UntilNextBucket => {
                let minutes = match bucket {
                    HorizonBucket::Tick50 => 5i64,
                    HorizonBucket::Fast5m => 5,
                    HorizonBucket::Mid30m => 30,
                    HorizonBucket::Session => 6 * 60,
                    HorizonBucket::MultiSession => 24 * 60,
                };
                reference + time::Duration::minutes(minutes)
            }
            HorizonExpiry::UntilSessionClose => reference + time::Duration::hours(6),
            HorizonExpiry::FixedTicks(n) => reference + time::Duration::seconds(n as i64),
            HorizonExpiry::None => reference + time::Duration::days(365),
        }
    }

    /// Build pending records for a case's primary + secondary horizons.
    /// Called at case open (status = Pending, result = None).
    ///
    /// Invariant: record count == 1 + case_horizon.secondary.len().
    /// The primary record has `primary = true` and the first `due_at`.
    pub fn pending_for_case(
        setup_id: &str,
        market: &str,
        case_horizon: &CaseHorizon,
        now: OffsetDateTime,
    ) -> Vec<HorizonEvaluationRecord> {
        let mut records = Vec::with_capacity(1 + case_horizon.secondary.len());
        records.push(HorizonEvaluationRecord {
            record_id: Self::build_id(setup_id, case_horizon.primary),
            setup_id: setup_id.to_string(),
            market: market.to_string(),
            horizon: case_horizon.primary,
            primary: true,
            due_at: Self::materialize_due_at(case_horizon.expiry, case_horizon.primary, now),
            status: EvaluationStatus::Pending,
            result: None,
            resolution: None,
            attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
        });
        for sec in &case_horizon.secondary {
            records.push(HorizonEvaluationRecord {
                record_id: Self::build_id(setup_id, sec.bucket),
                setup_id: setup_id.to_string(),
                market: market.to_string(),
                horizon: sec.bucket,
                primary: false,
                due_at: Self::materialize_due_at(case_horizon.expiry, sec.bucket, now),
                status: EvaluationStatus::Pending,
                result: None,
                resolution: None,
                attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
            });
        }
        records
    }
}

/// Settle a single horizon evaluation record: flip status, attach
/// result and resolution. Called when due_at has been reached or an
/// exit signal fires. See Resolution System Wave 2.
///
/// Invariant: status must be Resolved or EarlyExited after this call.
/// Callers must not pass Pending or Due as new_status.
#[cfg_attr(not(any(feature = "persistence", test)), allow(dead_code))]
pub(crate) fn settle_horizon_evaluation(
    record: &mut HorizonEvaluationRecord,
    result: HorizonResult,
    exit: Option<crate::ontology::reasoning::IntentExitKind>,
    violations: &[crate::ontology::reasoning::ExpectationViolation],
    new_status: EvaluationStatus,
) {
    debug_assert!(
        matches!(
            new_status,
            EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
        ),
        "settle must set Resolved or EarlyExited, not {:?}",
        new_status,
    );
    record.status = new_status;
    let resolution =
        crate::ontology::resolution::classify_horizon_resolution(&result, exit, violations);
    record.result = Some(result);
    record.resolution = Some(resolution);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::macros::datetime;

    fn pending_record(attempts: u32) -> HorizonEvaluationRecord {
        HorizonEvaluationRecord {
            record_id: "horizon-eval:test:Fast5m".into(),
            setup_id: "test".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2099-12-31 23:59 UTC),
            status: EvaluationStatus::Pending,
            result: None,
            resolution: None,
            attempts_remaining: attempts,
        }
    }

    #[test]
    fn decrement_returns_zero_for_empty_slice() {
        let mut records: Vec<HorizonEvaluationRecord> = Vec::new();
        let expired = decrement_attempts_or_expire(&mut records);
        assert_eq!(expired, 0);
    }

    #[test]
    fn decrement_only_lowers_counter_when_above_one() {
        let mut records = vec![pending_record(5), pending_record(2)];
        let expired = decrement_attempts_or_expire(&mut records);
        assert_eq!(expired, 0, "neither record should expire on this pass");
        assert_eq!(records[0].attempts_remaining, 4);
        assert_eq!(records[1].attempts_remaining, 1);
        assert!(records.iter().all(|r| r.status == EvaluationStatus::Pending));
    }

    #[test]
    fn decrement_expires_records_at_one_remaining() {
        let mut records = vec![pending_record(1), pending_record(3)];
        let expired = decrement_attempts_or_expire(&mut records);
        assert_eq!(expired, 1, "the record at 1 must transition to Expired");
        assert_eq!(records[0].status, EvaluationStatus::Expired);
        assert_eq!(records[0].attempts_remaining, 0);
        assert_eq!(records[1].status, EvaluationStatus::Pending);
        assert_eq!(records[1].attempts_remaining, 2);
    }

    #[test]
    fn decrement_leaves_terminal_records_alone() {
        let mut already_expired = pending_record(0);
        already_expired.status = EvaluationStatus::Expired;
        let mut resolved = pending_record(50);
        resolved.status = EvaluationStatus::Resolved;
        let mut records = vec![already_expired.clone(), resolved.clone()];
        let expired = decrement_attempts_or_expire(&mut records);
        assert_eq!(expired, 0);
        assert_eq!(records[0], already_expired, "Expired untouched");
        assert_eq!(records[1], resolved, "Resolved untouched");
    }

    #[test]
    fn record_roundtrip_pending() {
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Fast5m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Pending,
            result: None,
            resolution: None,
            attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn record_roundtrip_resolved_with_result() {
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Mid30m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Mid30m,
            primary: false,
            due_at: datetime!(2026-04-13 15:00 UTC),
            status: EvaluationStatus::Resolved,
            result: Some(HorizonResult {
                net_return: dec!(0.023),
                resolved_at: datetime!(2026-04-13 15:00 UTC),
                follow_through: dec!(0.85),
            }),
            resolution: None,
            attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn record_with_resolution_serializes_and_deserializes() {
        use crate::ontology::resolution::{
            HorizonResolution, HorizonResolutionKind, ResolutionFinality,
        };
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Fast5m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Resolved,
            result: Some(HorizonResult {
                net_return: dec!(0.02),
                resolved_at: datetime!(2026-04-13 14:35 UTC),
                follow_through: dec!(0.8),
            }),
            resolution: Some(HorizonResolution {
                kind: HorizonResolutionKind::Confirmed,
                finality: ResolutionFinality::Provisional,
                rationale: "numeric_confirmed".into(),
                trigger: None,
            }),
            attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn legacy_record_without_resolution_field_deserializes_with_none() {
        // Manually build a JSON payload lacking the `resolution` field
        let json = r#"{
            "record_id": "horizon-eval:legacy:Fast5m",
            "setup_id": "legacy",
            "market": "us",
            "horizon": "fast5m",
            "primary": true,
            "due_at": "2026-04-13T14:35:00Z",
            "status": "resolved",
            "result": {
                "net_return": "0.01",
                "resolved_at": "2026-04-13T14:35:00Z",
                "follow_through": "0.7"
            }
        }"#;
        let parsed: HorizonEvaluationRecord = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.resolution, None);
    }

    #[test]
    fn build_id_is_deterministic() {
        let id = HorizonEvaluationRecord::build_id("setup-7", HorizonBucket::Session);
        assert_eq!(id, "horizon-eval:setup-7:Session");
    }

    #[test]
    fn materialize_fast5m_until_next_bucket_is_five_min() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::UntilNextBucket,
            HorizonBucket::Fast5m,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 14:05 UTC));
    }

    #[test]
    fn materialize_mid30m_until_next_bucket_is_thirty_min() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::UntilNextBucket,
            HorizonBucket::Mid30m,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 14:30 UTC));
    }

    #[test]
    fn materialize_session_until_session_close_is_six_hours() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::UntilSessionClose,
            HorizonBucket::Session,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 20:00 UTC));
    }

    #[test]
    fn materialize_fixed_ticks_is_n_seconds() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::FixedTicks(300),
            HorizonBucket::Fast5m,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 14:05 UTC));
    }

    #[test]
    fn pending_for_case_creates_primary_plus_each_secondary() {
        use crate::ontology::horizon::{
            CaseHorizon, HorizonBucket, HorizonExpiry, SecondaryHorizon, SessionPhase, Urgency,
        };
        use rust_decimal_macros::dec;
        let horizon = CaseHorizon::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            SessionPhase::Opening,
            HorizonExpiry::UntilNextBucket,
            vec![
                SecondaryHorizon {
                    bucket: HorizonBucket::Mid30m,
                    confidence: dec!(0.7),
                },
                SecondaryHorizon {
                    bucket: HorizonBucket::Session,
                    confidence: dec!(0.4),
                },
            ],
        );
        let now = datetime!(2026-04-13 14:00 UTC);
        let records = HorizonEvaluationRecord::pending_for_case("setup-1", "us", &horizon, now);
        assert_eq!(records.len(), 3);
        assert!(records[0].primary);
        assert!(!records[1].primary);
        assert!(!records[2].primary);
        assert_eq!(records[0].horizon, HorizonBucket::Fast5m);
        assert_eq!(records[1].horizon, HorizonBucket::Mid30m);
        assert_eq!(records[2].horizon, HorizonBucket::Session);
        // Each record has its own due_at derived from its own bucket
        assert_eq!(records[0].due_at, datetime!(2026-04-13 14:05 UTC));
        assert_eq!(records[1].due_at, datetime!(2026-04-13 14:30 UTC));
        assert_eq!(records[2].due_at, datetime!(2026-04-13 20:00 UTC));
        // All start as Pending
        assert!(records
            .iter()
            .all(|r| r.status == EvaluationStatus::Pending));
        assert!(records.iter().all(|r| r.result.is_none()));
    }

    #[test]
    fn settle_horizon_evaluation_sets_resolution() {
        use crate::ontology::resolution::{HorizonResolutionKind, ResolutionFinality};
        let mut record = HorizonEvaluationRecord {
            record_id: "test".into(),
            setup_id: "test".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Due,
            result: None,
            resolution: None,
            attempts_remaining: DEFAULT_SETTLE_ATTEMPTS,
        };
        let result = HorizonResult {
            net_return: dec!(0.02),
            resolved_at: datetime!(2026-04-13 14:35 UTC),
            follow_through: dec!(0.75),
        };
        super::settle_horizon_evaluation(
            &mut record,
            result,
            None,
            &[],
            EvaluationStatus::Resolved,
        );
        assert_eq!(record.status, EvaluationStatus::Resolved);
        assert!(record.result.is_some());
        let res = record.resolution.as_ref().unwrap();
        assert_eq!(res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(res.finality, ResolutionFinality::Provisional);
    }
}
