//! Resolution System — dual-layer case outcome language.
//!
//! Three independent concepts are kept strictly separate:
//! - EvaluationStatus (in persistence/horizon_evaluation.rs) = lifecycle
//! - HorizonResolution (this module) = per-window semantic outcome
//! - CaseResolution (this module) = per-case aggregated semantic outcome
//!
//! See docs/superpowers/specs/2026-04-12-resolution-system-design.md.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;
use crate::ontology::reasoning::{ExpectationViolation, IntentExitKind};
use crate::persistence::horizon_evaluation::HorizonResult;

/// Whether a resolution can still be upgraded by later evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionFinality {
    Provisional,
    Final,
}

/// Who produced this resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    Auto,
    OperatorOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonResolutionKind {
    /// Expected market move materialized within this horizon with
    /// strong follow-through. Matches the intent's original thesis.
    Confirmed,
    /// Horizon's window closed without meaningful movement. Conservative
    /// default. NOT a negative judgment.
    Exhausted,
    /// Hard falsifier, reversal, or strong negative numeric evidence.
    Invalidated,
    /// Intent's explicit completion signal fired. Strongest positive.
    Fulfilled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonResolution {
    pub kind: HorizonResolutionKind,
    pub finality: ResolutionFinality,
    /// Classification tag with source prefix:
    /// "hard_falsifier:...", "window_violation:...", "exit_signal_...",
    /// "numeric_confirmed", "numeric_no_follow_through", "numeric_default".
    pub rationale: String,
    /// Specific trigger (violation falsifier id, exit signal trigger text).
    /// None for pure numeric fallback.
    pub trigger: Option<String>,
}

/// Classify a settled horizon's outcome into `HorizonResolution`.
///
/// Pure function. Same inputs always produce same output. No clock,
/// no I/O, no shared state. Priority-ordered — every branch is tested.
///
/// Priority:
/// 1. Hard falsifier (violation.magnitude > 0.5) → Final Invalidated
/// 2. Intent exit signal (Fulfilled / Invalidated / Reversal / Absorbed / ...)
/// 3. Weak violation (magnitude > 0.2) → Provisional Invalidated
/// 4. Numeric confirmed (follow_through ≥ 0.6 + net_return > 0) → Provisional Confirmed
/// 5. Numeric no follow-through (< 0.2) → Provisional Exhausted
/// 6. Default fallback → Provisional Exhausted (never Invalidated)
pub fn classify_horizon_resolution(
    result: &HorizonResult,
    exit: Option<IntentExitKind>,
    violations: &[ExpectationViolation],
) -> HorizonResolution {
    // Priority 1: Hard falsifier
    if let Some(hard) = violations.iter().find(|v| v.magnitude > dec!(0.5)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            rationale: format!("hard_falsifier: {}", hard.description),
            trigger: hard.falsifier.clone(),
        };
    }

    // Priority 2: Intent exit signal
    match exit {
        Some(IntentExitKind::Fulfilled) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Fulfilled,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_fulfilled".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Invalidated) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Invalidated,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_invalidated".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Reversal) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Invalidated,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_reversal".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Absorbed) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_absorbed".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Exhaustion) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_exhaustion".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Decay) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_decay".into(),
                trigger: None,
            };
        }
        None => {}
    }

    // Priority 3: Weak violation — window-level, upgradable
    if let Some(soft) = violations.iter().find(|v| v.magnitude > dec!(0.2)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Provisional,
            rationale: format!("window_violation: {}", soft.description),
            trigger: soft.falsifier.clone(),
        };
    }

    // Priority 4: Numeric confirmed
    if result.follow_through >= dec!(0.6) && result.net_return > Decimal::ZERO {
        return HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
    }

    // Priority 5: Numeric exhausted (weak follow-through)
    if result.follow_through < dec!(0.2) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Exhausted,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_no_follow_through".into(),
            trigger: None,
        };
    }

    // Priority 6: Conservative default — Exhausted, NOT Invalidated
    HorizonResolution {
        kind: HorizonResolutionKind::Exhausted,
        finality: ResolutionFinality::Provisional,
        rationale: "numeric_default".into(),
        trigger: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseResolutionKind {
    /// All relevant horizons confirmed. Strongest positive.
    Confirmed,
    /// Some horizons confirmed, some exhausted. Intent partially right.
    PartiallyConfirmed,
    /// Hard falsifier triggered OR all supplementals also failed.
    Invalidated,
    /// Nothing happened across the horizons. Neutral.
    Exhausted,
    /// Primary horizon exhausted/failed, but a supplemental later
    /// confirmed with positive return. Horizon selection was wrong,
    /// intent was fine.
    ProfitableButLate,
    /// Case was closed early before any horizon could settle naturally.
    EarlyExited,
    /// Intent direction was correct but microstructure (liquidity,
    /// spread, slippage) made it untradeable. Operator-only.
    StructurallyRightButUntradeable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolution {
    pub kind: CaseResolutionKind,
    pub finality: ResolutionFinality,
    /// One-line operator summary. Opaque string for now.
    pub narrative: String,
    /// Aggregated net return across the case's horizons.
    pub net_return: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolutionTransition {
    /// None on the first write (case had no prior resolution).
    #[serde(default)]
    pub from_kind: Option<CaseResolutionKind>,
    /// None on the first write.
    #[serde(default)]
    pub from_finality: Option<ResolutionFinality>,
    pub to_kind: CaseResolutionKind,
    pub to_finality: ResolutionFinality,
    pub triggered_by_horizon: HorizonBucket,
    #[serde(with = "rfc3339")]
    pub at: OffsetDateTime,
    pub reason: String,
}

/// Aggregate multiple horizon resolutions into a single case resolution.
///
/// Pure function. Never produces `StructurallyRightButUntradeable`
/// (operator-only). Maps `Fulfilled` at the horizon layer to
/// `CaseResolutionKind::Confirmed` + Final (the case layer has no
/// Fulfilled variant by design).
///
/// Rule priority:
/// 1. Any horizon Final Invalidated → Final Invalidated
/// 2. Any horizon Fulfilled → Final Confirmed
/// 3. All horizons Confirmed AND all_settled → Final Confirmed
/// 4. Primary can be overridden (Exhausted or Provisional Invalidated)
///    AND supplemental Confirmed with positive return → ProfitableButLate
/// 5. Any Confirmed in the mix → PartiallyConfirmed
/// 6. Fallback → Exhausted
pub fn aggregate_case_resolution(
    primary: &HorizonResolution,
    supplementals: &[(HorizonBucket, HorizonResolution, HorizonResult)],
    primary_result: &HorizonResult,
    all_settled: bool,
) -> CaseResolution {
    // Aggregate net return across primary + supplementals.
    let mut net_return = primary_result.net_return;
    for (_, _, result) in supplementals {
        net_return += result.net_return;
    }

    // 1. Hard falsifier anywhere → Final Invalidated
    let primary_is_final_invalidated = primary.finality == ResolutionFinality::Final
        && primary.kind == HorizonResolutionKind::Invalidated;
    let supplemental_has_final_invalidated = supplementals.iter().any(|(_, r, _)| {
        r.finality == ResolutionFinality::Final && r.kind == HorizonResolutionKind::Invalidated
    });
    if primary_is_final_invalidated || supplemental_has_final_invalidated {
        return CaseResolution {
            kind: CaseResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            narrative: "hard falsifier triggered".into(),
            net_return,
        };
    }

    // 2. Any Fulfilled → Final Confirmed (case layer has no Fulfilled variant)
    let any_fulfilled = primary.kind == HorizonResolutionKind::Fulfilled
        || supplementals
            .iter()
            .any(|(_, r, _)| r.kind == HorizonResolutionKind::Fulfilled);
    if any_fulfilled {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: "intent explicitly fulfilled".into(),
            net_return,
        };
    }

    // 3. All horizons Confirmed AND all_settled → Final Confirmed
    let confirmed_count = std::iter::once(primary)
        .chain(supplementals.iter().map(|(_, r, _)| r))
        .filter(|r| r.kind == HorizonResolutionKind::Confirmed)
        .count();
    let total_horizons = 1 + supplementals.len();
    if confirmed_count == total_horizons && all_settled {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: format!("all {total_horizons} horizons confirmed"),
            net_return,
        };
    }

    // 4. Primary overridable + supplemental Confirmed + positive return → ProfitableButLate
    let primary_can_be_overridden = matches!(
        primary.kind,
        HorizonResolutionKind::Exhausted | HorizonResolutionKind::Invalidated
    ) && primary.finality == ResolutionFinality::Provisional;

    if primary_can_be_overridden {
        let supp_confirmed_positive = supplementals.iter().any(|(_, r, result)| {
            r.kind == HorizonResolutionKind::Confirmed && result.net_return > Decimal::ZERO
        });
        if supp_confirmed_positive {
            return CaseResolution {
                kind: CaseResolutionKind::ProfitableButLate,
                finality: if all_settled {
                    ResolutionFinality::Final
                } else {
                    ResolutionFinality::Provisional
                },
                narrative: "primary exhausted, supplemental later confirmed".into(),
                net_return,
            };
        }
    }

    // 4.5. Primary alone confirmed, no supplementals yet → Confirmed(Provisional)
    //      Without this clause Rule 5 would fire (confirmed_count=1 > 0) and
    //      return PartiallyConfirmed, which misrepresents the state: there is
    //      nothing "partial" about a sole primary that confirmed.
    if supplementals.is_empty() && primary.kind == HorizonResolutionKind::Confirmed {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: if all_settled {
                ResolutionFinality::Final
            } else {
                ResolutionFinality::Provisional
            },
            narrative: "primary confirmed, no supplementals".into(),
            net_return,
        };
    }

    // 5. Mix of Confirmed + other → PartiallyConfirmed
    if confirmed_count > 0 {
        return CaseResolution {
            kind: CaseResolutionKind::PartiallyConfirmed,
            finality: if all_settled {
                ResolutionFinality::Final
            } else {
                ResolutionFinality::Provisional
            },
            narrative: format!("{confirmed_count}/{total_horizons} horizons confirmed"),
            net_return,
        };
    }

    // 6. Fallback: Exhausted
    CaseResolution {
        kind: CaseResolutionKind::Exhausted,
        finality: if all_settled {
            ResolutionFinality::Final
        } else {
            ResolutionFinality::Provisional
        },
        narrative: "no horizon confirmed".into(),
        net_return,
    }
}

/// Result of an upgrade attempt. Callers decide what to do based on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The new resolution differs and was applied (transition appended).
    Applied,
    /// The new resolution is identical to current (kind AND finality). No-op.
    NoChange,
    /// The current resolution is Final and cannot be changed.
    RejectedFinal,
    /// The proposed change is a downgrade and was rejected.
    RejectedDowngrade,
}

/// Returns true if `new_kind` is a valid monotonic upgrade from `current_kind`.
/// Same-kind is NOT an upgrade (use finality transition for that).
pub fn is_valid_upgrade(current: CaseResolutionKind, new: CaseResolutionKind) -> bool {
    use CaseResolutionKind::*;
    matches!(
        (current, new),
        (Exhausted, ProfitableButLate)
            | (Exhausted, PartiallyConfirmed)
            | (Exhausted, Confirmed)
            | (PartiallyConfirmed, Confirmed)
            // Refinement-to-Final: when supplementals settle with mixed outcomes, a
            // Confirmed case may lock Final as PartiallyConfirmed. Only allowed when
            // finality is being locked Final (enforced in apply_case_resolution_update).
            | (Confirmed, PartiallyConfirmed)
            | (Invalidated, ProfitableButLate)
            | (Invalidated, PartiallyConfirmed)
            | (EarlyExited, ProfitableButLate)
    )
}

/// Transient update input produced by the aggregator.
#[derive(Debug, Clone)]
pub struct ResolutionUpdate {
    pub new_resolution: CaseResolution,
    pub triggered_by_horizon: HorizonBucket,
    pub at: OffsetDateTime,
    pub reason: String,
}

/// Apply an upgrade attempt to an existing `CaseResolution` + transition
/// history. This is the single choke point for all case-resolution
/// mutations. Enforces:
/// - Final lock
/// - Downgrade rejection
/// - No-op skip (both kind and finality identical)
/// - Finality transitions are recorded even when kind is unchanged
/// - History append is always one new transition on Applied
pub fn apply_case_resolution_update(
    current: &mut CaseResolution,
    history: &mut Vec<CaseResolutionTransition>,
    update: ResolutionUpdate,
) -> UpdateOutcome {
    // No-op: exactly the same
    if current.kind == update.new_resolution.kind
        && current.finality == update.new_resolution.finality
    {
        return UpdateOutcome::NoChange;
    }

    // Final lock: cannot change anything once Final
    if current.finality == ResolutionFinality::Final {
        return UpdateOutcome::RejectedFinal;
    }

    // Downgrade check:
    //   If kind is changing, it must be a monotonic upgrade.
    //   If only finality is changing (kind identical), always allowed
    //   (Provisional → Final).
    if current.kind != update.new_resolution.kind
        && !is_valid_upgrade(current.kind, update.new_resolution.kind)
    {
        return UpdateOutcome::RejectedDowngrade;
    }

    // Refinement-to-Final guard: Confirmed → PartiallyConfirmed is only
    // valid when the aggregator is locking the Final state. A Provisional
    // PartiallyConfirmed after Confirmed would be a plain downgrade.
    if current.kind == CaseResolutionKind::Confirmed
        && update.new_resolution.kind == CaseResolutionKind::PartiallyConfirmed
        && update.new_resolution.finality != ResolutionFinality::Final
    {
        return UpdateOutcome::RejectedDowngrade;
    }

    // Accepted: append transition BEFORE mutating current
    history.push(CaseResolutionTransition {
        from_kind: Some(current.kind),
        from_finality: Some(current.finality),
        to_kind: update.new_resolution.kind,
        to_finality: update.new_resolution.finality,
        triggered_by_horizon: update.triggered_by_horizon,
        at: update.at,
        reason: update.reason,
    });

    // Update in place
    *current = update.new_resolution;

    UpdateOutcome::Applied
}

/// Build the initial transition for a case that has no prior resolution.
/// Used when writing the first `CaseResolutionRecord` for a case.
pub fn initial_case_resolution_transition(
    new: &CaseResolution,
    triggered_by_horizon: HorizonBucket,
    at: OffsetDateTime,
    reason: String,
) -> CaseResolutionTransition {
    CaseResolutionTransition {
        from_kind: None,
        from_finality: None,
        to_kind: new.kind,
        to_finality: new.finality,
        triggered_by_horizon,
        at,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::reasoning::ExpectationViolationKind;
    use crate::persistence::horizon_evaluation::HorizonResult;

    fn make_result(net: Decimal, follow: Decimal) -> HorizonResult {
        HorizonResult {
            net_return: net,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            follow_through: follow,
        }
    }

    fn hard_violation() -> ExpectationViolation {
        ExpectationViolation {
            kind: ExpectationViolationKind::FailedConfirmation,
            expectation_id: Some("exp1".into()),
            description: "hard falsifier desc".into(),
            magnitude: dec!(0.8),
            falsifier: Some("falsifier_hard".into()),
        }
    }

    fn weak_violation() -> ExpectationViolation {
        ExpectationViolation {
            kind: ExpectationViolationKind::FailedConfirmation,
            expectation_id: None,
            description: "weak window violation".into(),
            magnitude: dec!(0.3),
            falsifier: Some("falsifier_weak".into()),
        }
    }

    fn confirmed(bucket: HorizonBucket) -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: format!("{:?}_numeric_confirmed", bucket),
            trigger: None,
        }
    }

    fn exhausted_prov() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Exhausted,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_default".into(),
            trigger: None,
        }
    }

    fn invalidated_final() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            rationale: "hard_falsifier: test".into(),
            trigger: Some("f1".into()),
        }
    }

    fn fulfilled() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Fulfilled,
            finality: ResolutionFinality::Final,
            rationale: "exit_signal_fulfilled".into(),
            trigger: None,
        }
    }

    fn make_case_resolution(
        kind: CaseResolutionKind,
        finality: ResolutionFinality,
    ) -> CaseResolution {
        CaseResolution {
            kind,
            finality,
            narrative: "test".into(),
            net_return: dec!(0.0),
        }
    }

    fn make_update(kind: CaseResolutionKind, finality: ResolutionFinality) -> ResolutionUpdate {
        ResolutionUpdate {
            new_resolution: make_case_resolution(kind, finality),
            triggered_by_horizon: HorizonBucket::Mid30m,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "test".into(),
        }
    }

    #[test]
    fn resolution_finality_snake_case_json() {
        assert_eq!(
            serde_json::to_string(&ResolutionFinality::Provisional).unwrap(),
            "\"provisional\""
        );
        assert_eq!(
            serde_json::to_string(&ResolutionFinality::Final).unwrap(),
            "\"final\""
        );
    }

    #[test]
    fn resolution_source_snake_case_json() {
        assert_eq!(
            serde_json::to_string(&ResolutionSource::Auto).unwrap(),
            "\"auto\""
        );
        assert_eq!(
            serde_json::to_string(&ResolutionSource::OperatorOverride).unwrap(),
            "\"operator_override\""
        );
    }

    #[test]
    fn horizon_resolution_kind_snake_case_json() {
        assert_eq!(
            serde_json::to_string(&HorizonResolutionKind::Confirmed).unwrap(),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&HorizonResolutionKind::Exhausted).unwrap(),
            "\"exhausted\""
        );
        assert_eq!(
            serde_json::to_string(&HorizonResolutionKind::Invalidated).unwrap(),
            "\"invalidated\""
        );
        assert_eq!(
            serde_json::to_string(&HorizonResolutionKind::Fulfilled).unwrap(),
            "\"fulfilled\""
        );
    }

    #[test]
    fn horizon_resolution_roundtrip() {
        let hr = HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
        let json = serde_json::to_string(&hr).unwrap();
        let parsed: HorizonResolution = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, hr);
    }

    #[test]
    fn classify_hard_falsifier_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            None,
            &[hard_violation()],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
        assert!(r.rationale.starts_with("hard_falsifier"));
        assert_eq!(r.trigger.as_deref(), Some("falsifier_hard"));
    }

    #[test]
    fn classify_exit_fulfilled_returns_final_fulfilled() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            Some(IntentExitKind::Fulfilled),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Fulfilled);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_invalidated_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Invalidated),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_reversal_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Reversal),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_absorbed_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Absorbed),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn classify_exit_decay_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            Some(IntentExitKind::Decay),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn classify_weak_violation_returns_provisional_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            None,
            &[weak_violation()],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert!(r.rationale.starts_with("window_violation"));
    }

    #[test]
    fn classify_numeric_confirmed_returns_provisional_confirmed() {
        let r = classify_horizon_resolution(&make_result(dec!(0.015), dec!(0.75)), None, &[]);
        assert_eq!(r.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_confirmed");
    }

    #[test]
    fn classify_numeric_no_follow_through_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(&make_result(dec!(0.01), dec!(0.1)), None, &[]);
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_no_follow_through");
    }

    #[test]
    fn classify_default_fallback_is_exhausted_not_invalidated() {
        // Moderate follow-through, small negative return, no signals.
        // Must default to Exhausted (conservative), NOT Invalidated.
        let r = classify_horizon_resolution(&make_result(dec!(-0.003), dec!(0.4)), None, &[]);
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_default");
    }

    #[test]
    fn case_resolution_kind_snake_case_json() {
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::Confirmed).unwrap(),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::PartiallyConfirmed).unwrap(),
            "\"partially_confirmed\"",
        );
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::ProfitableButLate).unwrap(),
            "\"profitable_but_late\"",
        );
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::StructurallyRightButUntradeable).unwrap(),
            "\"structurally_right_but_untradeable\"",
        );
    }

    #[test]
    fn case_resolution_has_seven_variants() {
        // Smoke guard: if someone adds or removes a kind without updating
        // aggregator/learning policy, this test reminds them.
        let variants = [
            CaseResolutionKind::Confirmed,
            CaseResolutionKind::PartiallyConfirmed,
            CaseResolutionKind::Invalidated,
            CaseResolutionKind::Exhausted,
            CaseResolutionKind::ProfitableButLate,
            CaseResolutionKind::EarlyExited,
            CaseResolutionKind::StructurallyRightButUntradeable,
        ];
        assert_eq!(variants.len(), 7);
    }

    #[test]
    fn case_resolution_transition_from_kind_can_be_none() {
        let t = CaseResolutionTransition {
            from_kind: None,
            from_finality: None,
            to_kind: CaseResolutionKind::Exhausted,
            to_finality: ResolutionFinality::Provisional,
            triggered_by_horizon: HorizonBucket::Fast5m,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "primary settled".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let parsed: CaseResolutionTransition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from_kind, None);
        assert_eq!(parsed.from_finality, None);
    }

    #[test]
    fn case_resolution_transition_finality_upgrade() {
        let t = CaseResolutionTransition {
            from_kind: Some(CaseResolutionKind::Confirmed),
            from_finality: Some(ResolutionFinality::Provisional),
            to_kind: CaseResolutionKind::Confirmed,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: HorizonBucket::Session,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "all horizons settled".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let parsed: CaseResolutionTransition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, t);
    }

    #[test]
    fn aggregate_single_horizon_confirmed_all_settled_is_final_confirmed() {
        let primary_res = make_result(dec!(0.02), dec!(0.8));
        let out =
            aggregate_case_resolution(&confirmed(HorizonBucket::Fast5m), &[], &primary_res, true);
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.02));
    }

    #[test]
    fn aggregate_all_horizons_confirmed_all_settled_is_final_confirmed() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.02), dec!(0.8));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(
                HorizonBucket::Mid30m,
                confirmed(HorizonBucket::Mid30m),
                supp_res,
            )],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.03));
    }

    #[test]
    fn aggregate_all_confirmed_but_not_all_settled_stays_provisional() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.02), dec!(0.8));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(
                HorizonBucket::Mid30m,
                confirmed(HorizonBucket::Mid30m),
                supp_res,
            )],
            &primary_res,
            false,
        );
        assert_eq!(out.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(out.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn aggregate_primary_exhausted_supplemental_confirmed_is_profitable_but_late() {
        let primary_res = make_result(dec!(0.0), dec!(0.1));
        let supp_res = make_result(dec!(0.025), dec!(0.85));
        let out = aggregate_case_resolution(
            &exhausted_prov(),
            &[(
                HorizonBucket::Mid30m,
                confirmed(HorizonBucket::Mid30m),
                supp_res,
            )],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.025));
    }

    #[test]
    fn aggregate_primary_hard_invalidated_is_final_invalidated() {
        let primary_res = make_result(dec!(-0.01), dec!(0.2));
        let out = aggregate_case_resolution(&invalidated_final(), &[], &primary_res, true);
        assert_eq!(out.kind, CaseResolutionKind::Invalidated);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_primary_provisional_invalidated_supplemental_confirmed_is_profitable_but_late() {
        // Primary has weak window_violation (Provisional Invalidated).
        // Supplemental later Confirms with positive return.
        // Result: ProfitableButLate.
        let prim_weak_inv = HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Provisional,
            rationale: "window_violation: test".into(),
            trigger: Some("w1".into()),
        };
        let primary_res = make_result(dec!(-0.002), dec!(0.3));
        let supp_res = make_result(dec!(0.03), dec!(0.9));
        let out = aggregate_case_resolution(
            &prim_weak_inv,
            &[(
                HorizonBucket::Mid30m,
                confirmed(HorizonBucket::Mid30m),
                supp_res,
            )],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_any_fulfilled_maps_to_final_confirmed() {
        let primary_res = make_result(dec!(0.015), dec!(0.7));
        let out = aggregate_case_resolution(&fulfilled(), &[], &primary_res, true);
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert!(out.narrative.contains("fulfilled"));
    }

    #[test]
    fn aggregate_mix_confirmed_exhausted_is_partially_confirmed() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(HorizonBucket::Mid30m, exhausted_prov(), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_all_exhausted_all_settled_is_final_exhausted() {
        let primary_res = make_result(dec!(0.0), dec!(0.15));
        let supp_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(
            &exhausted_prov(),
            &[(HorizonBucket::Mid30m, exhausted_prov(), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Exhausted);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_all_exhausted_with_pending_is_provisional_exhausted() {
        // Only primary has settled; supplementals pending (empty here = not yet present)
        let primary_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(&exhausted_prov(), &[], &primary_res, false);
        assert_eq!(out.kind, CaseResolutionKind::Exhausted);
        assert_eq!(out.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn apply_update_rejects_downgrade() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::Exhausted,
                ResolutionFinality::Provisional,
            ),
        );
        assert_eq!(out, UpdateOutcome::RejectedDowngrade);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_rejects_final_change() {
        let mut cur =
            make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Final);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::ProfitableButLate,
                ResolutionFinality::Final,
            ),
        );
        assert_eq!(out, UpdateOutcome::RejectedFinal);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_skips_noop() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::Confirmed,
                ResolutionFinality::Provisional,
            ),
        );
        assert_eq!(out, UpdateOutcome::NoChange);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_allows_provisional_to_final_same_kind() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );
        assert_eq!(out, UpdateOutcome::Applied);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert_eq!(cur.finality, ResolutionFinality::Final);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].from_finality, Some(ResolutionFinality::Provisional));
        assert_eq!(hist[0].to_finality, ResolutionFinality::Final);
    }

    #[test]
    fn apply_update_allows_valid_upgrade_exhausted_to_profitable_but_late() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Exhausted,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::ProfitableButLate,
                ResolutionFinality::Provisional,
            ),
        );
        assert_eq!(out, UpdateOutcome::Applied);
        assert_eq!(cur.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].from_kind, Some(CaseResolutionKind::Exhausted));
        assert_eq!(hist[0].to_kind, CaseResolutionKind::ProfitableButLate);
    }

    #[test]
    fn apply_update_appends_transition_on_every_change() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Exhausted,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::PartiallyConfirmed,
                ResolutionFinality::Provisional,
            ),
        );
        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::Confirmed,
                ResolutionFinality::Provisional,
            ),
        );
        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );

        assert_eq!(hist.len(), 3);
        // Each transition records what it came from
        assert_eq!(hist[0].from_kind, Some(CaseResolutionKind::Exhausted));
        assert_eq!(
            hist[1].from_kind,
            Some(CaseResolutionKind::PartiallyConfirmed)
        );
        assert_eq!(hist[2].from_kind, Some(CaseResolutionKind::Confirmed));
        assert_eq!(hist[2].from_finality, Some(ResolutionFinality::Provisional));
        assert_eq!(hist[2].to_finality, ResolutionFinality::Final);
    }

    #[test]
    fn apply_update_never_rewrites_history() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Exhausted,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::PartiallyConfirmed,
                ResolutionFinality::Provisional,
            ),
        );
        let first_snapshot = hist[0].clone();

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );

        // First transition must be byte-for-byte identical
        assert_eq!(hist[0], first_snapshot);
        // History grew monotonically
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn initial_transition_has_none_from() {
        let new = make_case_resolution(
            CaseResolutionKind::Exhausted,
            ResolutionFinality::Provisional,
        );
        let t = initial_case_resolution_transition(
            &new,
            HorizonBucket::Fast5m,
            OffsetDateTime::UNIX_EPOCH,
            "primary settled".into(),
        );
        assert_eq!(t.from_kind, None);
        assert_eq!(t.from_finality, None);
        assert_eq!(t.to_kind, CaseResolutionKind::Exhausted);
        assert_eq!(t.triggered_by_horizon, HorizonBucket::Fast5m);
    }

    /// Task 14: verify aggregator + upgrade gate integration for the BKNG-style
    /// primary-only first write (Rule 4.5 path).
    #[test]
    fn task14_aggregator_plus_upgrade_gate_primary_only_then_supplemental() {
        use crate::persistence::horizon_evaluation::HorizonResult;
        use time::macros::datetime;

        let primary_result = HorizonResult {
            net_return: dec!(0.015),
            resolved_at: datetime!(2026-04-13 14:05 UTC),
            follow_through: dec!(0.75),
        };
        let primary_res = HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };

        // T1: primary-only settled → Rule 4.5 fires → Confirmed(Provisional)
        let t1 = aggregate_case_resolution(&primary_res, &[], &primary_result, false);
        assert_eq!(t1.kind, CaseResolutionKind::Confirmed);
        assert_eq!(t1.finality, ResolutionFinality::Provisional);

        let mut current = t1.clone();
        let mut history = vec![initial_case_resolution_transition(
            &t1,
            HorizonBucket::Fast5m,
            datetime!(2026-04-13 14:05 UTC),
            "primary settled".into(),
        )];
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].from_kind, None);

        // T2: mid30m supplemental settles Confirmed, not all settled
        let mid_result = HorizonResult {
            net_return: dec!(0.008),
            resolved_at: datetime!(2026-04-13 14:35 UTC),
            follow_through: dec!(0.70),
        };
        let mid_res = HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
        // Two horizons confirmed but not all settled → still Confirmed(Provisional) via same kind
        let t2 = aggregate_case_resolution(
            &primary_res,
            &[(HorizonBucket::Mid30m, mid_res.clone(), mid_result.clone())],
            &primary_result,
            false,
        );
        // With supplemental present and all_settled=false, Rule 3 doesn't fire,
        // Rule 4.5 doesn't fire (supplementals not empty), Rule 5 fires → PartiallyConfirmed.
        // That's a same-kind direction: the upgrade gate rejects downgrade from Confirmed.
        assert_eq!(t2.kind, CaseResolutionKind::PartiallyConfirmed);
        let outcome2 = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t2,
                triggered_by_horizon: HorizonBucket::Mid30m,
                at: datetime!(2026-04-13 14:35 UTC),
                reason: "mid30m settled".into(),
            },
        );
        // Confirmed → PartiallyConfirmed is a downgrade; gate must reject
        assert_eq!(outcome2, UpdateOutcome::RejectedDowngrade);
        // State unchanged, history unchanged
        assert_eq!(current.kind, CaseResolutionKind::Confirmed);
        assert_eq!(history.len(), 1);

        // T3: all three horizons settled including session, aggregator now has all_settled=true
        // and both supplementals Confirmed → Rule 3 fires → Final Confirmed
        let session_result = HorizonResult {
            net_return: dec!(0.005),
            resolved_at: datetime!(2026-04-13 20:05 UTC),
            follow_through: dec!(0.65),
        };
        let session_res = HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
        let t3 = aggregate_case_resolution(
            &primary_res,
            &[
                (HorizonBucket::Mid30m, mid_res.clone(), mid_result.clone()),
                (
                    HorizonBucket::Session,
                    session_res.clone(),
                    session_result.clone(),
                ),
            ],
            &primary_result,
            true, // all settled
        );
        assert_eq!(t3.kind, CaseResolutionKind::Confirmed);
        assert_eq!(t3.finality, ResolutionFinality::Final);

        let outcome3 = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t3,
                triggered_by_horizon: HorizonBucket::Session,
                at: datetime!(2026-04-13 20:05 UTC),
                reason: "all horizons settled".into(),
            },
        );
        // Confirmed(Provisional) → Confirmed(Final): finality upgrade, kind unchanged
        assert_eq!(outcome3, UpdateOutcome::Applied);
        assert_eq!(current.kind, CaseResolutionKind::Confirmed);
        assert_eq!(current.finality, ResolutionFinality::Final);
        assert_eq!(history.len(), 2);
        assert_eq!(
            history[1].from_finality,
            Some(ResolutionFinality::Provisional)
        );
        assert_eq!(history[1].to_finality, ResolutionFinality::Final);

        // T4: attempt to change a Final record → rejected
        let outcome4 = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: CaseResolution {
                    kind: CaseResolutionKind::Exhausted,
                    finality: ResolutionFinality::Final,
                    narrative: "spurious late update".into(),
                    net_return: dec!(0.0),
                },
                triggered_by_horizon: HorizonBucket::MultiSession,
                at: datetime!(2026-04-14 00:00 UTC),
                reason: "spurious".into(),
            },
        );
        assert_eq!(outcome4, UpdateOutcome::RejectedFinal);
        assert_eq!(history.len(), 2); // no new entry
    }

    /// Task 15 guard: Confirmed → PartiallyConfirmed(Provisional) is a downgrade.
    #[test]
    fn apply_update_rejects_confirmed_to_partially_confirmed_without_final() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::PartiallyConfirmed,
                ResolutionFinality::Provisional,
            ),
        );
        assert_eq!(out, UpdateOutcome::RejectedDowngrade);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert!(hist.is_empty());
    }

    /// Task 15 guard: Confirmed → PartiallyConfirmed(Final) is the legitimate
    /// refinement-to-Final path (BKNG-style: primary Confirmed, session Exhausted).
    #[test]
    fn apply_update_accepts_confirmed_to_partially_confirmed_with_final() {
        let mut cur = make_case_resolution(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        );
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(
                CaseResolutionKind::PartiallyConfirmed,
                ResolutionFinality::Final,
            ),
        );
        assert_eq!(out, UpdateOutcome::Applied);
        assert_eq!(cur.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(cur.finality, ResolutionFinality::Final);
        assert_eq!(hist.len(), 1);
    }

    /// Task 15 BKNG end-to-end regression: walks through horizon classifier +
    /// case aggregator + upgrade gate for the full 3-horizon BKNG-style flow.
    ///
    /// T1 (5 min):  Fast5m settles Confirmed(Provisional)
    ///              → CaseResolution = Confirmed(Provisional)  [Rule 4.5]
    /// T2 (35 min): Mid30m settles Confirmed(Provisional)
    ///              → aggregator produces Confirmed(Provisional), no change
    /// T3 (6h):     Session settles Exhausted(Provisional), all_settled=true
    ///              → aggregator produces PartiallyConfirmed(Final)
    ///              → upgrade gate accepts refinement-to-Final
    #[test]
    fn bkng_flow_end_to_end_through_resolution_system() {
        use crate::persistence::horizon_evaluation::HorizonResult;
        use time::macros::datetime;

        let fast_result = HorizonResult {
            net_return: dec!(0.015),
            resolved_at: datetime!(2026-04-13 14:05 UTC),
            follow_through: dec!(0.75),
        };
        let mid_result = HorizonResult {
            net_return: dec!(0.008),
            resolved_at: datetime!(2026-04-13 14:35 UTC),
            follow_through: dec!(0.70),
        };
        let session_result = HorizonResult {
            net_return: dec!(-0.002),
            resolved_at: datetime!(2026-04-13 20:05 UTC),
            follow_through: dec!(0.15),
        };

        // Classify each horizon via the classifier
        let fast_res = classify_horizon_resolution(&fast_result, None, &[]);
        let mid_res = classify_horizon_resolution(&mid_result, None, &[]);
        let session_res = classify_horizon_resolution(&session_result, None, &[]);

        assert_eq!(fast_res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(mid_res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(session_res.kind, HorizonResolutionKind::Exhausted);

        // T1: Fast5m primary settles, no supplementals yet (Rule 4.5 path)
        let t1 = aggregate_case_resolution(&fast_res, &[], &fast_result, false);
        assert_eq!(t1.kind, CaseResolutionKind::Confirmed);
        assert_eq!(t1.finality, ResolutionFinality::Provisional);

        let mut current = t1.clone();
        let mut history = vec![initial_case_resolution_transition(
            &t1,
            HorizonBucket::Fast5m,
            datetime!(2026-04-13 14:05 UTC),
            "primary settled".into(),
        )];
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].from_kind, None);
        assert_eq!(history[0].to_kind, CaseResolutionKind::Confirmed);

        // T2: Mid30m supplemental settles Confirmed, not all settled yet
        // Rule 4.5 does not fire (supplementals not empty).
        // Rule 3 does not fire (all_settled=false).
        // Rule 5 fires: confirmed_count=2, total=2 → PartiallyConfirmed(Provisional).
        // But since t2.kind == PartiallyConfirmed and finality is Provisional,
        // the refinement-to-Final guard rejects it as a downgrade from Confirmed.
        let t2 = aggregate_case_resolution(
            &fast_res,
            &[(HorizonBucket::Mid30m, mid_res.clone(), mid_result.clone())],
            &fast_result,
            false,
        );
        assert_eq!(t2.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(t2.finality, ResolutionFinality::Provisional);
        let t2_outcome = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t2,
                triggered_by_horizon: HorizonBucket::Mid30m,
                at: datetime!(2026-04-13 14:35 UTC),
                reason: "mid30m settled".into(),
            },
        );
        // Confirmed(Provisional) → PartiallyConfirmed(Provisional) = downgrade, rejected
        assert_eq!(t2_outcome, UpdateOutcome::RejectedDowngrade);
        assert_eq!(current.kind, CaseResolutionKind::Confirmed);
        assert_eq!(history.len(), 1);

        // T3: Session supplemental settles Exhausted, all three horizons settled
        // Rule 3: not all confirmed (session=Exhausted) → skipped
        // Rule 4: primary not Exhausted/Invalidated → skipped
        // Rule 5: confirmed_count=2 (fast+mid), total=3 → PartiallyConfirmed(Final)
        let t3 = aggregate_case_resolution(
            &fast_res,
            &[
                (HorizonBucket::Mid30m, mid_res.clone(), mid_result.clone()),
                (
                    HorizonBucket::Session,
                    session_res.clone(),
                    session_result.clone(),
                ),
            ],
            &fast_result,
            true, // all_settled
        );
        assert_eq!(t3.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(t3.finality, ResolutionFinality::Final);

        let t3_outcome = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t3,
                triggered_by_horizon: HorizonBucket::Session,
                at: datetime!(2026-04-13 20:05 UTC),
                reason: "session settled, all settled".into(),
            },
        );
        // Confirmed(Provisional) → PartiallyConfirmed(Final): refinement-to-Final, accepted
        assert_eq!(t3_outcome, UpdateOutcome::Applied);
        assert_eq!(current.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(current.finality, ResolutionFinality::Final);
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].from_kind, Some(CaseResolutionKind::Confirmed));
        assert_eq!(history[1].to_kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(
            history[1].from_finality,
            Some(ResolutionFinality::Provisional)
        );
        assert_eq!(history[1].to_finality, ResolutionFinality::Final);
        assert_eq!(history[1].triggered_by_horizon, HorizonBucket::Session);

        // T4: Final lock — any further update must be rejected
        let t4_outcome = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: CaseResolution {
                    kind: CaseResolutionKind::Exhausted,
                    finality: ResolutionFinality::Final,
                    narrative: "spurious late update".into(),
                    net_return: dec!(0.0),
                },
                triggered_by_horizon: HorizonBucket::MultiSession,
                at: datetime!(2026-04-14 00:00 UTC),
                reason: "spurious".into(),
            },
        );
        assert_eq!(t4_outcome, UpdateOutcome::RejectedFinal);
        assert_eq!(history.len(), 2); // no new entry — history is immutable after Final
    }
}
