//! Horizon System — unified trading-time language.
//!
//! Four independent time concepts are kept strictly separate:
//! - TimeScale (in pipeline/pressure.rs) = compute time, not in this module
//! - HorizonBucket = trading time (this module's main enum)
//! - SessionPhase = market context
//! - Urgency = action timing
//!
//! See docs/superpowers/specs/2026-04-12-horizon-system-design.md.

#[allow(unused_imports)]
use rust_decimal::Decimal;
#[allow(unused_imports)]
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use time::OffsetDateTime;

/// Trading-time language. These are trading opportunity categories,
/// not minute counts. A "Fast5m" bucket means "short-term opportunity
/// where decisions live at the seconds-to-minutes scale."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonBucket {
    /// Very short-term tick window (~50 ticks). Used internally by lineage
    /// metrics as a baseline window; excluded from multi-horizon gate checks.
    Tick50,
    Fast5m,
    Mid30m,
    Session,
    MultiSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Immediate,
    Normal,
    Relaxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    PreMarket,
    Opening,
    Midday,
    Closing,
    AfterHours,
}

impl SessionPhase {
    /// Produce the canonical snake_case label used in JSON output and
    /// persisted session strings. This is the single source of truth for
    /// the emit boundary — callers that need a `&str` for storage or API
    /// responses should call this rather than hand-rolling a match.
    pub fn as_label(self) -> &'static str {
        match self {
            SessionPhase::PreMarket => "pre_market",
            SessionPhase::Opening => "opening",
            SessionPhase::Midday => "midday",
            SessionPhase::Closing => "closing",
            SessionPhase::AfterHours => "after_hours",
        }
    }
}

/// Relative expiry — concrete `due_at` derived at display/execution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum HorizonExpiry {
    UntilNextBucket,
    UntilSessionClose,
    FixedTicks(u64),
    None,
}

impl HorizonBucket {
    /// Deterministic lookup table for legacy `time_horizon: String` values.
    /// No runtime inference — this is a fixed mapping.
    ///
    /// "intraday" maps to `Session` as a conservative default so that old
    /// records never accidentally poison Fast5m learning buckets.
    pub fn from_legacy_string(s: &str) -> HorizonBucket {
        match s {
            "50t" => HorizonBucket::Tick50,
            "intraday" => HorizonBucket::Session,
            "session" => HorizonBucket::Session,
            "multi_session" | "multi-session" => HorizonBucket::MultiSession,
            "multi-hour" | "multihour" => HorizonBucket::Mid30m,
            _ => HorizonBucket::Session,
        }
    }

    /// Forward derivation used for dual-writing the legacy `time_horizon`
    /// string field during Wave 2. Source of truth is always the bucket.
    pub fn to_legacy_string(self) -> &'static str {
        match self {
            HorizonBucket::Tick50 => "50t",
            HorizonBucket::Fast5m | HorizonBucket::Mid30m => "intraday",
            HorizonBucket::Session => "session",
            HorizonBucket::MultiSession => "multi_session",
        }
    }
}

/// One window in an Intent's horizon profile. An intent can have multiple —
/// the same underlying process may be viable in multiple buckets with
/// different bias/confidence. This will later live on
/// `IntentHypothesis.opportunities` (Wave 2 wires it in).
///
/// Note: intentionally does not reference `IntentOpportunityBias` to keep
/// this module free of reverse dependencies during Wave 1. Wave 2 will
/// evolve `IntentOpportunityWindow` in `ontology::reasoning` to carry
/// `bucket` and `urgency` fields directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonWindow {
    pub bucket: HorizonBucket,
    pub urgency: Urgency,
    pub confidence: Decimal,
    pub alignment: Decimal,
    pub rationale: String,
}

/// Secondary horizons on a Case — context only. Carries just enough info
/// for display and delayed confirmation. Must never contain the primary bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondaryHorizon {
    pub bucket: HorizonBucket,
    pub confidence: Decimal,
}

/// A Case's operational horizon — single primary choice, one rhythm.
///
/// Invariant: `primary` must never appear in `secondary`. The `new`
/// constructor is the single choke point that enforces this by filtering
/// any secondary entry whose bucket equals `primary`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseHorizon {
    pub primary: HorizonBucket,
    pub urgency: Urgency,
    pub secondary: Vec<SecondaryHorizon>,
    pub session_phase: SessionPhase,
    pub expiry: HorizonExpiry,
}

impl CaseHorizon {
    /// Construct a `CaseHorizon`, enforcing the single-primary invariant.
    /// Any secondary entry whose bucket equals `primary` is silently
    /// filtered out — this is the single choke point for the invariant.
    pub fn new(
        primary: HorizonBucket,
        urgency: Urgency,
        session_phase: SessionPhase,
        expiry: HorizonExpiry,
        secondary: Vec<SecondaryHorizon>,
    ) -> Self {
        let secondary = secondary
            .into_iter()
            .filter(|s| s.bucket != primary)
            .collect();
        Self {
            primary,
            urgency,
            secondary,
            session_phase,
            expiry,
        }
    }
}

/// Minimal bias enum used by the urgency computation.
/// The full `IntentOpportunityBias` lives in `ontology::reasoning`;
/// we accept a lowered copy to keep this module free of reverse deps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyBias {
    Enter,
    Hold,
    Watch,
    Exit,
}

/// Minimal intent-state enum used by the urgency computation.
/// The full `IntentState` lives in `ontology::reasoning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyIntentState {
    Forming,
    Active,
    Other,
}

/// Locked rules for computing `Urgency` from the context. This is NOT a
/// heuristic — every branch is explicit and tested.
pub fn compute_urgency(
    intent_state: UrgencyIntentState,
    bucket: HorizonBucket,
    bias: UrgencyBias,
    conflict_score: Decimal,
    exit_signal_present: bool,
) -> Urgency {
    // Exit signals are always immediate.
    if bias == UrgencyBias::Exit && exit_signal_present {
        return Urgency::Immediate;
    }

    match (bucket, bias, intent_state) {
        // Fast + Enter with high conflict while forming = window closing
        (HorizonBucket::Fast5m, UrgencyBias::Enter, UrgencyIntentState::Forming)
            if conflict_score > dec!(0.6) =>
        {
            Urgency::Immediate
        }

        // Active hold in mid bucket = normal pace
        (HorizonBucket::Mid30m, UrgencyBias::Hold, UrgencyIntentState::Active) => Urgency::Normal,

        // Forming mid entry = normal
        (HorizonBucket::Mid30m, UrgencyBias::Enter, UrgencyIntentState::Forming) => Urgency::Normal,

        // Session/MultiSession watch = relaxed regardless of state
        (HorizonBucket::Session, UrgencyBias::Watch, _) => Urgency::Relaxed,
        (HorizonBucket::MultiSession, UrgencyBias::Watch, _) => Urgency::Relaxed,

        // Default
        _ => Urgency::Normal,
    }
}

use crate::core::market::MarketId;

/// Classifier for `SessionPhase` given a timestamp and market.
/// Phase 1/2 uses `TimestampSessionResolver`; Phase 3+ can swap in a
/// calendar-aware resolver that handles half-days and holidays.
pub trait SessionPhaseResolver: Send + Sync {
    fn classify(&self, market: MarketId, ts: OffsetDateTime) -> SessionPhase;
}

/// Pure timestamp rule-based resolver. Handles normal US and HK sessions.
/// Does NOT handle half-days, early closes, or market holidays — those
/// require a calendar-aware resolver swapped in later.
///
/// US session (Eastern time, DST ignored for simplicity, offset -5):
/// - 04:00-09:30 ET → PreMarket
/// - 09:30-10:30 ET → Opening
/// - 10:30-15:00 ET → Midday
/// - 15:00-16:00 ET → Closing
/// - 16:00-20:00 ET → AfterHours
///
/// HK session (Hong Kong time, offset +8):
/// - before 09:30 HKT → PreMarket
/// - 09:30-10:30 HKT → Opening
/// - 10:30-15:00 HKT → Midday
/// - 15:00-16:00 HKT → Closing
/// - after 16:00 HKT → AfterHours
pub struct TimestampSessionResolver;

impl SessionPhaseResolver for TimestampSessionResolver {
    fn classify(&self, market: MarketId, ts: OffsetDateTime) -> SessionPhase {
        let offset_hours: i64 = match market {
            MarketId::Hk => 8,
            MarketId::Us => -5,
        };
        let local = ts + time::Duration::hours(offset_hours);
        let minutes = local.hour() as i32 * 60 + local.minute() as i32;

        let opening_start = 9 * 60 + 30;
        let opening_end = 10 * 60 + 30;
        let closing_start = 15 * 60;
        let closing_end = 16 * 60;
        let pre_start = 4 * 60;
        let after_end = 20 * 60;

        if minutes >= opening_start && minutes < opening_end {
            SessionPhase::Opening
        } else if minutes >= opening_end && minutes < closing_start {
            SessionPhase::Midday
        } else if minutes >= closing_start && minutes < closing_end {
            SessionPhase::Closing
        } else if minutes >= pre_start && minutes < opening_start {
            SessionPhase::PreMarket
        } else if minutes >= closing_end && minutes < after_end {
            SessionPhase::AfterHours
        } else {
            SessionPhase::AfterHours
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_bucket_serializes_to_snake_case() {
        let json = serde_json::to_string(&HorizonBucket::Fast5m).unwrap();
        assert_eq!(json, "\"fast5m\"");
        let json = serde_json::to_string(&HorizonBucket::MultiSession).unwrap();
        assert_eq!(json, "\"multi_session\"");
    }

    #[test]
    fn urgency_serializes_to_snake_case() {
        let json = serde_json::to_string(&Urgency::Immediate).unwrap();
        assert_eq!(json, "\"immediate\"");
    }

    #[test]
    fn session_phase_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionPhase::PreMarket).unwrap();
        assert_eq!(json, "\"pre_market\"");
    }

    #[test]
    fn legacy_string_intraday_maps_to_session() {
        assert_eq!(
            HorizonBucket::from_legacy_string("intraday"),
            HorizonBucket::Session
        );
    }

    #[test]
    fn legacy_string_session_maps_to_session() {
        assert_eq!(
            HorizonBucket::from_legacy_string("session"),
            HorizonBucket::Session
        );
    }

    #[test]
    fn legacy_string_multi_session_maps_to_multi_session() {
        assert_eq!(
            HorizonBucket::from_legacy_string("multi_session"),
            HorizonBucket::MultiSession,
        );
        assert_eq!(
            HorizonBucket::from_legacy_string("multi-session"),
            HorizonBucket::MultiSession,
        );
    }

    #[test]
    fn legacy_string_multi_hour_maps_to_mid30m() {
        assert_eq!(
            HorizonBucket::from_legacy_string("multi-hour"),
            HorizonBucket::Mid30m
        );
    }

    #[test]
    fn legacy_string_unknown_falls_back_to_session() {
        assert_eq!(
            HorizonBucket::from_legacy_string("whatever"),
            HorizonBucket::Session
        );
        assert_eq!(
            HorizonBucket::from_legacy_string(""),
            HorizonBucket::Session
        );
    }

    #[test]
    fn forward_derivation_is_unique() {
        assert_eq!(HorizonBucket::Fast5m.to_legacy_string(), "intraday");
        assert_eq!(HorizonBucket::Mid30m.to_legacy_string(), "intraday");
        assert_eq!(HorizonBucket::Session.to_legacy_string(), "session");
        assert_eq!(
            HorizonBucket::MultiSession.to_legacy_string(),
            "multi_session"
        );
    }

    #[test]
    fn horizon_expiry_serialization_roundtrip() {
        let expiry = HorizonExpiry::FixedTicks(300);
        let json = serde_json::to_string(&expiry).unwrap();
        let parsed: HorizonExpiry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HorizonExpiry::FixedTicks(300));
    }

    #[test]
    fn case_horizon_invariant_primary_not_in_secondary() {
        let ch = CaseHorizon::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            SessionPhase::Opening,
            HorizonExpiry::UntilNextBucket,
            vec![
                SecondaryHorizon {
                    bucket: HorizonBucket::Fast5m,
                    confidence: dec!(0.5),
                },
                SecondaryHorizon {
                    bucket: HorizonBucket::Mid30m,
                    confidence: dec!(0.7),
                },
            ],
        );
        // Fast5m should have been filtered from secondary
        assert_eq!(ch.secondary.len(), 1);
        assert_eq!(ch.secondary[0].bucket, HorizonBucket::Mid30m);
        assert!(!ch.secondary.iter().any(|s| s.bucket == ch.primary));
    }

    #[test]
    fn case_horizon_primary_is_single() {
        let ch = CaseHorizon::new(
            HorizonBucket::Session,
            Urgency::Relaxed,
            SessionPhase::Midday,
            HorizonExpiry::UntilSessionClose,
            vec![],
        );
        assert_eq!(ch.primary, HorizonBucket::Session);
    }

    #[test]
    fn urgency_fast5m_enter_forming_high_conflict_is_immediate() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Fast5m,
            UrgencyBias::Enter,
            dec!(0.7),
            false,
        );
        assert_eq!(u, Urgency::Immediate);
    }

    #[test]
    fn urgency_fast5m_enter_forming_low_conflict_is_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Fast5m,
            UrgencyBias::Enter,
            dec!(0.3),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_exit_signal_is_always_immediate() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Session,
            UrgencyBias::Exit,
            dec!(0.0),
            true,
        );
        assert_eq!(u, Urgency::Immediate);
    }

    #[test]
    fn urgency_exit_without_signal_defaults_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Mid30m,
            UrgencyBias::Exit,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_mid30m_hold_active_is_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Mid30m,
            UrgencyBias::Hold,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_session_watch_is_relaxed() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Session,
            UrgencyBias::Watch,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Relaxed);
    }

    #[test]
    fn urgency_multi_session_watch_is_relaxed() {
        let u = compute_urgency(
            UrgencyIntentState::Other,
            HorizonBucket::MultiSession,
            UrgencyBias::Watch,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Relaxed);
    }

    #[test]
    fn urgency_unknown_combination_defaults_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Other,
            HorizonBucket::Session,
            UrgencyBias::Hold,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    use time::macros::datetime;

    #[test]
    fn timestamp_resolver_us_opening() {
        let r = TimestampSessionResolver;
        // 14:45 UTC = 09:45 ET (offset -5) — inside opening window
        let ts = datetime!(2026-04-13 14:45 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Opening);
    }

    #[test]
    fn timestamp_resolver_us_midday() {
        let r = TimestampSessionResolver;
        // 17:00 UTC = 12:00 ET — midday
        let ts = datetime!(2026-04-13 17:00 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Midday);
    }

    #[test]
    fn timestamp_resolver_us_closing() {
        let r = TimestampSessionResolver;
        // 20:30 UTC = 15:30 ET — closing
        let ts = datetime!(2026-04-13 20:30 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Closing);
    }

    #[test]
    fn timestamp_resolver_hk_opening() {
        let r = TimestampSessionResolver;
        // 01:45 UTC = 09:45 HKT — opening
        let ts = datetime!(2026-04-14 01:45 UTC);
        assert_eq!(r.classify(MarketId::Hk, ts), SessionPhase::Opening);
    }

    #[test]
    fn timestamp_resolver_us_premarket() {
        let r = TimestampSessionResolver;
        // 13:00 UTC = 08:00 ET — pre-market
        let ts = datetime!(2026-04-13 13:00 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::PreMarket);
    }
}
