use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::cases::CaseSummary;
use crate::ontology::{Hypothesis, TacticalSetup};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::tactical_setup::TacticalSetupRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReasoningAssessmentRecord {
    pub assessment_id: String,
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub market: String,
    pub symbol: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_label: Option<String>,
    pub sector: Option<String>,
    pub recommended_action: String,
    pub workflow_state: String,
    pub market_regime_bias: Option<String>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub market_regime_confidence: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub market_breadth_delta: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub market_average_return: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub market_directional_consensus: Option<Decimal>,
    pub source: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_family: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reason_subreasons: Vec<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub actor: Option<String>,
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_decision_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_state: Option<String>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub timing_position_in_range: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state: Option<String>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub local_state_confidence: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub actionability_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_persistence_ticks: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_stability_rounds: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_reason_codes: Vec<String>,
    pub law_kinds: Vec<String>,
    pub predicate_kinds: Vec<String>,
    pub composite_state_kinds: Vec<String>,
    pub primary_mechanism_kind: Option<String>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub primary_mechanism_score: Option<Decimal>,
    pub competing_mechanism_kinds: Vec<String>,
    pub invalidation_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<crate::ontology::CaseSignature>,
    #[serde(default)]
    pub archetype_projections: Vec<crate::ontology::ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<crate::ontology::IntentHypothesis>,
    /// Primary trading horizon bucket, populated from TacticalSetupRecord. New in Wave 5.
    /// Pre-Wave-5 records and auto-assessment records may not carry this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_horizon: Option<crate::ontology::horizon::HorizonBucket>,
    #[serde(default)]
    pub expectation_bindings: Vec<crate::ontology::ExpectationBinding>,
    #[serde(default)]
    pub expectation_violations: Vec<crate::ontology::ExpectationViolation>,
    pub reasoning_profile: crate::ontology::CaseReasoningProfile,
    /// Regime fingerprint bucket key, populated at runtime when the
    /// assessment is recorded. Used as `conditioned_on` value with
    /// `scope="regime_bucket"` in `ConditionedLearningAdjustment`. None
    /// for older records or paths that haven't yet wired the runtime
    /// fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime_bucket: Option<String>,
}

impl CaseReasoningAssessmentRecord {
    pub fn from_case_summary(
        summary: &CaseSummary,
        recorded_at: OffsetDateTime,
        source: impl Into<String>,
    ) -> Self {
        let source = source.into();
        let primary_mechanism_kind = summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.kind.label().to_string());
        let primary_mechanism_score = summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.score);

        Self {
            assessment_id: assessment_id_for(&summary.setup_id, &source, recorded_at),
            setup_id: summary.setup_id.clone(),
            workflow_id: summary.workflow_id.clone(),
            market: match summary.market {
                crate::live_snapshot::LiveMarket::Hk => "hk".into(),
                crate::live_snapshot::LiveMarket::Us => "us".into(),
            },
            symbol: summary.symbol.clone(),
            title: summary.title.clone(),
            family_label: summary.family_label.clone(),
            sector: summary.sector.clone(),
            recommended_action: summary.recommended_action.clone(),
            workflow_state: summary.workflow_state.clone(),
            market_regime_bias: Some(summary.market_regime_bias.clone()),
            market_regime_confidence: Some(summary.market_regime_confidence),
            market_breadth_delta: Some(summary.market_breadth_delta),
            market_average_return: Some(summary.market_average_return),
            market_directional_consensus: summary.market_directional_consensus,
            source,
            recorded_at,
            review_reason_code: summary.review_reason_code.clone(),
            review_reason_family: summary.review_reason_family.clone(),
            review_reason_subreasons: summary.review_reason_subreasons.clone(),
            owner: summary.owner.clone(),
            reviewer: summary.reviewer.clone(),
            actor: summary.workflow_actor.clone(),
            note: summary.workflow_note.clone(),
            operator_decision_kind: None,
            freshness_state: summary.freshness_state.clone(),
            timing_state: summary.timing_state.clone(),
            timing_position_in_range: summary.timing_position_in_range,
            local_state: summary.local_state.clone(),
            local_state_confidence: summary.local_state_confidence,
            actionability_score: summary.actionability_score,
            actionability_state: summary.actionability_state.clone(),
            state_persistence_ticks: summary.state_persistence_ticks,
            direction_stability_rounds: summary.direction_stability_rounds,
            state_reason_codes: summary.state_reason_codes.clone(),
            law_kinds: summary
                .reasoning_profile
                .laws
                .iter()
                .map(|item| item.kind.label().to_string())
                .collect(),
            predicate_kinds: summary
                .reasoning_profile
                .predicates
                .iter()
                .map(|item| item.kind.label().to_string())
                .collect(),
            composite_state_kinds: summary
                .reasoning_profile
                .composite_states
                .iter()
                .map(|item| item.kind.label().to_string())
                .collect(),
            primary_mechanism_kind,
            primary_mechanism_score,
            competing_mechanism_kinds: summary
                .reasoning_profile
                .competing_mechanisms
                .iter()
                .map(|item| item.kind.label().to_string())
                .collect(),
            invalidation_rules: summary.invalidation_rules.clone(),
            case_signature: summary.case_signature.clone(),
            archetype_projections: summary.archetype_projections.clone(),
            inferred_intent: summary.inferred_intent.clone(),
            primary_horizon: None,
            expectation_bindings: Vec::new(),
            expectation_violations: Vec::new(),
            reasoning_profile: summary.reasoning_profile.clone(),
            regime_bucket: None,
        }
    }

    pub fn from_realized_outcome(outcome: &CaseRealizedOutcomeRecord) -> Self {
        use crate::ontology::{
            CaseReasoningProfile, CompositeState, CompositeStateKind, HumanReviewContext,
            HumanReviewReason, HumanReviewReasonKind, HumanReviewVerdict,
        };

        let (verdict, reasons, composite_state_kinds) =
            if outcome.followed_through && outcome.net_return > Decimal::ZERO {
                (HumanReviewVerdict::Confirmed, vec![], vec![])
            } else if outcome.invalidated {
                (
                    HumanReviewVerdict::Rejected,
                    vec![HumanReviewReason {
                        kind: HumanReviewReasonKind::MechanismMismatch,
                        label: "Mechanism Mismatch".into(),
                        confidence: Decimal::new(8, 1),
                    }],
                    vec!["Reflexive Correction".to_string()],
                )
            } else if !outcome.followed_through && outcome.net_return < Decimal::ZERO {
                (
                    HumanReviewVerdict::Rejected,
                    vec![HumanReviewReason {
                        kind: HumanReviewReasonKind::EvidenceTooWeak,
                        label: "Evidence Too Weak".into(),
                        confidence: Decimal::new(7, 1),
                    }],
                    vec!["Reflexive Correction".to_string()],
                )
            } else if outcome.net_return < Decimal::ZERO {
                (
                    HumanReviewVerdict::Rejected,
                    vec![HumanReviewReason {
                        kind: HumanReviewReasonKind::TimingMismatch,
                        label: "Timing Mismatch".into(),
                        confidence: Decimal::new(6, 1),
                    }],
                    vec!["Narrative Failure".to_string()],
                )
            } else {
                (HumanReviewVerdict::Confirmed, vec![], vec![])
            };

        let composite_states = composite_state_kinds
            .iter()
            .map(|label| CompositeState {
                kind: CompositeStateKind::ReflexiveCorrection,
                label: label.clone(),
                score: Decimal::new(7, 1),
                summary: format!(
                    "auto-assessed from realized outcome: net={}",
                    outcome.net_return
                ),
                predicates: vec![],
            })
            .collect();

        let human_review = if !reasons.is_empty() || matches!(verdict, HumanReviewVerdict::Rejected)
        {
            Some(HumanReviewContext {
                verdict,
                verdict_label: verdict.label().to_string(),
                confidence: Decimal::new(7, 1),
                reasons,
                note: Some(format!(
                    "auto: net_return={}, followed_through={}, invalidated={}",
                    outcome.net_return, outcome.followed_through, outcome.invalidated
                )),
            })
        } else {
            None
        };

        let primary_mechanism_kind = if outcome.invalidated {
            Some("Narrative Failure".to_string())
        } else if !outcome.followed_through {
            Some("Narrative Failure".to_string())
        } else {
            None
        };

        Self {
            assessment_id: assessment_id_for(
                &outcome.setup_id,
                "outcome_auto",
                outcome.resolved_at,
            ),
            setup_id: outcome.setup_id.clone(),
            workflow_id: outcome.workflow_id.clone(),
            market: outcome.market.clone(),
            symbol: outcome.symbol.clone().unwrap_or_default(),
            title: format!("{} outcome", outcome.family),
            family_label: Some(outcome.family.clone()),
            sector: None,
            recommended_action: String::new(),
            workflow_state: "resolved".into(),
            market_regime_bias: Some(outcome.market_regime.clone()),
            market_regime_confidence: None,
            market_breadth_delta: None,
            market_average_return: None,
            market_directional_consensus: None,
            source: "outcome_auto".into(),
            recorded_at: outcome.resolved_at,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            owner: None,
            reviewer: None,
            actor: Some("system".into()),
            note: Some(format!(
                "auto-assessment from realized outcome: net_return={}",
                outcome.net_return
            )),
            operator_decision_kind: None,
            freshness_state: Some("fresh".into()),
            timing_state: Some("timely".into()),
            timing_position_in_range: Some(Decimal::new(45, 2)),
            local_state: Some("low_information".into()),
            local_state_confidence: Some(Decimal::new(75, 2)),
            actionability_state: Some("do_not_trade".into()),
            actionability_score: None,
            state_persistence_ticks: Some(1),
            direction_stability_rounds: Some(1),
            state_reason_codes: vec!["insufficient_source_count".into()],
            law_kinds: vec![],
            predicate_kinds: vec![],
            composite_state_kinds,
            primary_mechanism_kind,
            primary_mechanism_score: None,
            competing_mechanism_kinds: vec![],
            invalidation_rules: vec![],
            case_signature: None,
            archetype_projections: Vec::new(),
            inferred_intent: None,
            primary_horizon: None,
            expectation_bindings: Vec::new(),
            expectation_violations: Vec::new(),
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![],
                composite_states,
                human_review,
                primary_mechanism: None,
                competing_mechanisms: vec![],
                automated_invalidations: vec![],
            },
            regime_bucket: None,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.assessment_id
    }

    pub fn apply_setup_projection(
        &mut self,
        setup: &TacticalSetup,
        hypothesis: Option<&Hypothesis>,
    ) {
        self.case_signature = Some(setup.case_signature(hypothesis));
        self.archetype_projections = setup.archetype_projections(hypothesis);
        self.inferred_intent = Some(setup.intent_hypothesis(hypothesis));
        self.expectation_bindings = hypothesis
            .map(Hypothesis::expected_bindings)
            .unwrap_or_default();
        self.expectation_violations = hypothesis
            .map(Hypothesis::expectation_violations)
            .unwrap_or_default();
        if self.family_label.is_none() {
            // family_label fallback now sourced from Hypothesis.family_label
            // (still REQUIRED on Hypothesis). TacticalSetup.family_key
            // deleted per first-principles audit.
            self.family_label = hypothesis.map(|h| h.family_label.clone());
        }
    }

    pub fn apply_setup_record_projection(&mut self, setup: &TacticalSetupRecord) {
        self.case_signature = setup.case_signature.clone();
        self.archetype_projections = setup.archetype_projections.clone();
        self.inferred_intent = setup.inferred_intent.clone();
        self.primary_horizon = Some(setup.primary_horizon);
        self.expectation_bindings = setup.expectation_bindings.clone();
        self.expectation_violations = setup.expectation_violations.clone();
        // family_label fallback from setup record removed — TacticalSetupRecord.family_key
        // and risk_notes "family=" encoding both deleted per first-principles audit.
    }
}

pub fn auto_assessments_from_outcomes(
    outcomes: &[CaseRealizedOutcomeRecord],
) -> Vec<CaseReasoningAssessmentRecord> {
    outcomes
        .iter()
        .map(CaseReasoningAssessmentRecord::from_realized_outcome)
        .collect()
}

pub fn apply_setup_records_to_assessments(
    assessments: &mut [CaseReasoningAssessmentRecord],
    setup_records: &[TacticalSetupRecord],
) {
    let setup_by_id = setup_records
        .iter()
        .map(|record| (record.setup_id.as_str(), record))
        .collect::<std::collections::HashMap<_, _>>();

    for assessment in assessments {
        if let Some(setup) = setup_by_id.get(assessment.setup_id.as_str()).copied() {
            assessment.apply_setup_record_projection(setup);
        }
    }
}

pub fn assessment_id_for(setup_id: &str, source: &str, recorded_at: OffsetDateTime) -> String {
    format!(
        "{}:{}:{}",
        setup_id,
        source,
        recorded_at.unix_timestamp_nanos()
    )
}

/// Shared backfill: generate auto-assessments from historical realized outcomes
/// so that doctrine pressure has seed data on first boot. Used by both HK and US runtimes.
#[cfg(feature = "persistence")]
pub async fn backfill_doctrine_assessments(
    store: &crate::persistence::store::EdenStore,
    market: &str,
) {
    if let Ok(outcomes) = store
        .recent_case_realized_outcomes_by_market(market, 500)
        .await
    {
        if !outcomes.is_empty() {
            let mut assessments = auto_assessments_from_outcomes(&outcomes);
            let setup_ids = outcomes
                .iter()
                .map(|outcome| outcome.setup_id.clone())
                .collect::<Vec<_>>();
            if let Ok(setup_records) = store.tactical_setups_by_ids(&setup_ids).await {
                apply_setup_records_to_assessments(&mut assessments, &setup_records);
            }
            if !assessments.is_empty() {
                let count = assessments.len();
                if let Err(err) = store.write_case_reasoning_assessments(&assessments).await {
                    eprintln!(
                        "[{}] failed to backfill doctrine assessments: {}",
                        market, err
                    );
                } else {
                    eprintln!(
                        "[{}] backfilled {} doctrine assessments from {} historical outcomes",
                        market,
                        count,
                        outcomes.len()
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::cases::{CaseEvidence, CaseSummary};
    use crate::live_snapshot::LiveMarket;
    use crate::ontology::{
        AtomicPredicate, AtomicPredicateKind, CaseReasoningProfile, CompositeState,
        CompositeStateKind, GoverningLawKind, LawActivation, MechanismCandidate,
        MechanismCandidateKind,
    };

    #[test]
    fn assessment_record_flattens_profile_metadata() {
        let summary = CaseSummary {
            case_id: "setup:1".into(),
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            execution_policy: None,
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            queue_pin: None,
            workflow_actor: Some("actor".into()),
            workflow_note: Some("note".into()),
            symbol: "700.HK".into(),
            title: "Long 700".into(),
            sector: Some("Technology".into()),
            market: LiveMarket::Hk,
            recommended_action: "enter".into(),
            workflow_state: "review".into(),
            governance: None,
            governance_bucket: "review_required".into(),
            governance_reason_code: None,
            governance_reason: None,
            market_regime_bias: "neutral".into(),
            market_regime_confidence: dec!(0.25),
            market_breadth_delta: dec!(-0.10),
            market_average_return: dec!(-0.01),
            market_directional_consensus: Some(dec!(0.02)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            freshness_state: Some("fresh".into()),
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: Some("timely".into()),
            timing_position_in_range: Some(dec!(0.45)),
            local_state: Some("low_information".into()),
            local_state_confidence: Some(dec!(0.75)),
            actionability_score: None,
            actionability_state: Some("do_not_trade".into()),
            confidence_velocity_5t: None,
            support_fraction_velocity_5t: None,
            driver_confidence: None,
            absence_summary: None,
            competition_summary: None,
            competition_winner: None,
            competition_runner_up: None,
            priority_rank: None,
            state_persistence_ticks: Some(1),
            direction_stability_rounds: Some(1),
            state_reason_codes: vec!["insufficient_source_count".into()],
            why_now: "test".into(),
            primary_lens: None,
            primary_driver: None,
            family_label: None,
            counter_label: None,
            hypothesis_status: None,
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            case_signature: None,
            archetype_projections: vec![],
            inferred_intent: None,
            intent_opportunities: vec![],
            expectation_binding_count: 0,
            expectation_violation_count: 0,
            key_evidence: vec![CaseEvidence {
                description: "x".into(),
                weight: dec!(0.5),
                direction: dec!(0.5),
            }],
            invalidation_rules: vec!["撤回".into()],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![LawActivation {
                    kind: GoverningLawKind::Persistence,
                    label: "Persistence".into(),
                    score: dec!(0.8),
                    summary: "s".into(),
                }],
                predicates: vec![AtomicPredicate {
                    kind: AtomicPredicateKind::SignalRecurs,
                    label: "Signal Recurs".into(),
                    law: GoverningLawKind::Persistence,
                    score: dec!(0.7),
                    summary: "s".into(),
                    evidence: vec![],
                }],
                composite_states: vec![CompositeState {
                    kind: CompositeStateKind::DirectionalReinforcement,
                    label: "Directional Reinforcement".into(),
                    score: dec!(0.75),
                    summary: "s".into(),
                    predicates: vec![AtomicPredicateKind::SignalRecurs],
                }],
                human_review: None,
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::MechanicalExecutionSignature,
                    label: "Mechanical Execution Signature".into(),
                    score: dec!(0.77),
                    summary: "s".into(),
                    supporting_states: vec![CompositeStateKind::DirectionalReinforcement],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
                automated_invalidations: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
            case_resolution: None,
            horizon_breakdown: None,
        };

        let record = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        assert_eq!(record.market, "hk");
        assert_eq!(record.sector.as_deref(), Some("Technology"));
        assert!(record.family_label.is_none());
        assert_eq!(
            record.primary_mechanism_kind.as_deref(),
            Some("Mechanical Execution Signature")
        );
        assert_eq!(record.market_regime_bias.as_deref(), Some("neutral"));
        assert_eq!(record.law_kinds, vec!["Persistence"]);
        assert_eq!(record.predicate_kinds, vec!["Signal Recurs"]);
        assert_eq!(record.local_state.as_deref(), Some("low_information"));
        assert_eq!(record.actionability_state.as_deref(), Some("do_not_trade"));
    }

    #[test]
    fn auto_assessment_from_failed_outcome() {
        let outcome = CaseRealizedOutcomeRecord {
            setup_id: "setup:100".into(),
            workflow_id: Some("wf:100".into()),
            market: "hk".into(),
            symbol: Some("700.HK".into()),
            primary_lens: None,
            family: "Propagation Chain".into(),
            session: "morning".into(),
            market_regime: "neutral".into(),
            entry_tick: 10,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 20,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            direction: 1,
            return_pct: dec!(-0.03),
            net_return: dec!(-0.03),
            max_favorable_excursion: dec!(0.01),
            max_adverse_excursion: dec!(-0.04),
            followed_through: false,
            invalidated: false,
            structure_retained: false,
            convergence_score: dec!(0.2),
        };
        let record = CaseReasoningAssessmentRecord::from_realized_outcome(&outcome);
        assert_eq!(record.source, "outcome_auto");
        assert_eq!(record.family_label.as_deref(), Some("Propagation Chain"));
        assert!(record
            .composite_state_kinds
            .contains(&"Reflexive Correction".to_string()));
        let review = record.reasoning_profile.human_review.as_ref().unwrap();
        assert!(matches!(
            review.verdict,
            crate::ontology::HumanReviewVerdict::Rejected
        ));
        assert!(review
            .reasons
            .iter()
            .any(|r| r.label == "Evidence Too Weak"));
    }

    #[test]
    fn auto_assessment_from_invalidated_outcome() {
        let outcome = CaseRealizedOutcomeRecord {
            setup_id: "setup:101".into(),
            workflow_id: None,
            market: "us".into(),
            symbol: Some("AAPL".into()),
            primary_lens: None,
            family: "Momentum Continuation".into(),
            session: "regular".into(),
            market_regime: "bullish".into(),
            entry_tick: 5,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 15,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            direction: 1,
            return_pct: dec!(-0.05),
            net_return: dec!(-0.05),
            max_favorable_excursion: dec!(0.0),
            max_adverse_excursion: dec!(-0.06),
            followed_through: false,
            invalidated: true,
            structure_retained: false,
            convergence_score: dec!(0.1),
        };
        let record = CaseReasoningAssessmentRecord::from_realized_outcome(&outcome);
        assert_eq!(
            record.primary_mechanism_kind.as_deref(),
            Some("Narrative Failure")
        );
        let review = record.reasoning_profile.human_review.as_ref().unwrap();
        assert!(review
            .reasons
            .iter()
            .any(|r| r.label == "Mechanism Mismatch"));
    }

    #[test]
    fn auto_assessment_from_successful_outcome() {
        let outcome = CaseRealizedOutcomeRecord {
            setup_id: "setup:102".into(),
            workflow_id: Some("wf:102".into()),
            market: "hk".into(),
            symbol: Some("9988.HK".into()),
            primary_lens: None,
            family: "Directed Flow".into(),
            session: "morning".into(),
            market_regime: "neutral".into(),
            entry_tick: 1,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 10,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            direction: 1,
            return_pct: dec!(0.05),
            net_return: dec!(0.05),
            max_favorable_excursion: dec!(0.06),
            max_adverse_excursion: dec!(-0.01),
            followed_through: true,
            invalidated: false,
            structure_retained: true,
            convergence_score: dec!(0.8),
        };
        let record = CaseReasoningAssessmentRecord::from_realized_outcome(&outcome);
        assert!(record.reasoning_profile.human_review.is_none());
        assert!(record.composite_state_kinds.is_empty());
    }
}
