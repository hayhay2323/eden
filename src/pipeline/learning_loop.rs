use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::math::clamp_unit_interval;
use crate::ontology::{CaseReasoningProfile, HumanReviewReasonKind};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
use crate::pipeline::mechanism_inference::{
    build_reasoning_profile, infer_mechanisms_with_factor_adjustments,
    retain_explanatory_mechanisms,
};

#[path = "learning_loop/feedback.rs"]
mod feedback;
#[path = "learning_loop/outcome_context.rs"]
mod outcome_context;
#[path = "learning_loop/types.rs"]
mod types;

pub use feedback::{apply_learning_feedback, derive_learning_feedback};
pub use outcome_context::{
    derive_outcome_learning_context_from_case_outcomes,
    derive_outcome_learning_context_from_hk_rows, derive_outcome_learning_context_from_us_rows,
};
pub use types::{
    ConditionedLearningAdjustment, LearningAdjustment, MechanismFactorAdjustment,
    OutcomeLearningContext, ReasoningLearningFeedback,
};

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::cases::{CaseEvidence, CaseSummary};
    use crate::live_snapshot::LiveMarket;
    use crate::ontology::{
        AtomicPredicate, AtomicPredicateKind, CaseReasoningProfile, CompositeState,
        CompositeStateKind, GoverningLawKind, MechanismCandidate, MechanismCandidateKind,
    };
    use crate::persistence::lineage_metric_row::LineageMetricRowRecord;

    #[test]
    fn feedback_penalizes_reviewed_mechanisms() {
        let mut summary = CaseSummary {
            case_id: "setup:1".into(),
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            execution_policy: None,
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            queue_pin: None,
            workflow_actor: Some("actor".into()),
            workflow_note: Some("reject narrative".into()),
            symbol: "A.US".into(),
            title: "Case".into(),
            sector: Some("Technology".into()),
            market: LiveMarket::Us,
            recommended_action: "enter".into(),
            workflow_state: "review".into(),
            governance: None,
            governance_bucket: "review_required".into(),
            governance_reason_code: None,
            governance_reason: None,
            market_regime_bias: "neutral".into(),
            market_regime_confidence: dec!(0.40),
            market_breadth_delta: dec!(-0.10),
            market_average_return: dec!(0.01),
            market_directional_consensus: Some(dec!(0.05)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            review_reason_code: None,
            why_now: "why".into(),
            primary_lens: None,
            primary_driver: None,
            family_label: None,
            counter_label: None,
            hypothesis_status: None,
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            key_evidence: vec![CaseEvidence {
                description: "x".into(),
                weight: dec!(0.5),
                direction: dec!(0.5),
            }],
            invalidation_rules: vec![],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![AtomicPredicate {
                    kind: AtomicPredicateKind::CounterevidencePresent,
                    label: "Counterevidence Present".into(),
                    law: GoverningLawKind::Competition,
                    score: dec!(0.6),
                    summary: "s".into(),
                    evidence: vec![],
                }],
                composite_states: vec![CompositeState {
                    kind: CompositeStateKind::ReflexiveCorrection,
                    label: "Reflexive Correction".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    predicates: vec![AtomicPredicateKind::CounterevidencePresent],
                }],
                human_review: Some(crate::ontology::HumanReviewContext {
                    verdict: crate::ontology::HumanReviewVerdict::Rejected,
                    verdict_label: "Rejected".into(),
                    confidence: dec!(0.8),
                    reasons: vec![crate::ontology::HumanReviewReason {
                        kind: crate::ontology::HumanReviewReasonKind::MechanismMismatch,
                        label: "Mechanism Mismatch".into(),
                        confidence: dec!(0.8),
                    }],
                    note: Some("reject narrative".into()),
                }),
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::NarrativeFailure,
                    label: "Narrative Failure".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    supporting_states: vec![CompositeStateKind::ReflexiveCorrection],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![crate::ontology::MechanismFactor {
                        key: "state:reflexive_correction".into(),
                        label: "Reflexive Correction".into(),
                        source: crate::ontology::MechanismFactorSource::State,
                        activation: dec!(0.7),
                        base_weight: dec!(0.5),
                        learned_weight_delta: Decimal::ZERO,
                        effective_weight: dec!(0.5),
                        contribution: dec!(0.35),
                    }],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
                automated_invalidations: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let runtime = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        summary.workflow_state = "review".into();
        let workflow = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
            "workflow_update",
        );

        let feedback =
            derive_learning_feedback(&[runtime, workflow], &OutcomeLearningContext::default());
        assert_eq!(feedback.paired_examples, 1);
        assert_eq!(feedback.corrected_examples, 1);
        assert!(feedback
            .mechanism_adjustments
            .iter()
            .any(|item| item.label == "Narrative Failure" && item.delta < Decimal::ZERO));
        assert!(feedback
            .mechanism_factor_adjustments
            .iter()
            .any(|item| item.mechanism == "Narrative Failure"));
        assert!(feedback
            .conditioned_adjustments
            .iter()
            .any(|item| item.scope == "state" && item.conditioned_on == "Reflexive Correction"));
    }

    #[test]
    fn feedback_rewards_reinforced_mechanisms() {
        let summary = CaseSummary {
            case_id: "setup:2".into(),
            setup_id: "setup:2".into(),
            workflow_id: Some("wf:2".into()),
            execution_policy: None,
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            queue_pin: None,
            workflow_actor: Some("actor".into()),
            workflow_note: Some("confirmed".into()),
            symbol: "B.US".into(),
            title: "Case".into(),
            sector: Some("Financials".into()),
            market: LiveMarket::Us,
            recommended_action: "enter".into(),
            workflow_state: "confirm".into(),
            governance: None,
            governance_bucket: "review_required".into(),
            governance_reason_code: None,
            governance_reason: None,
            market_regime_bias: "risk_on".into(),
            market_regime_confidence: dec!(0.70),
            market_breadth_delta: dec!(0.20),
            market_average_return: dec!(0.03),
            market_directional_consensus: Some(dec!(0.18)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            review_reason_code: None,
            why_now: "why".into(),
            primary_lens: None,
            primary_driver: None,
            family_label: None,
            counter_label: None,
            hypothesis_status: None,
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            key_evidence: vec![],
            invalidation_rules: vec![],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![],
                composite_states: vec![],
                human_review: Some(crate::ontology::HumanReviewContext {
                    verdict: crate::ontology::HumanReviewVerdict::Confirmed,
                    verdict_label: "Confirmed".into(),
                    confidence: dec!(0.6),
                    reasons: vec![],
                    note: Some("confirmed".into()),
                }),
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::MechanicalExecutionSignature,
                    label: "Mechanical Execution Signature".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    supporting_states: vec![CompositeStateKind::DirectionalReinforcement],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![crate::ontology::MechanismFactor {
                        key: "state:directional_reinforcement".into(),
                        label: "Directional Reinforcement".into(),
                        source: crate::ontology::MechanismFactorSource::State,
                        activation: dec!(0.7),
                        base_weight: dec!(0.45),
                        learned_weight_delta: Decimal::ZERO,
                        effective_weight: dec!(0.45),
                        contribution: dec!(0.315),
                    }],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
                automated_invalidations: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let runtime = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        let workflow = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
            "workflow_update",
        );

        let feedback =
            derive_learning_feedback(&[runtime, workflow], &OutcomeLearningContext::default());
        assert_eq!(feedback.paired_examples, 1);
        assert_eq!(feedback.reinforced_examples, 1);
        assert!(feedback.mechanism_adjustments.iter().any(|item| item.label
            == "Mechanical Execution Signature"
            && item.delta > Decimal::ZERO));
    }

    #[test]
    fn outcome_context_scales_feedback() {
        let context = OutcomeLearningContext {
            reward_multiplier: dec!(0.50),
            penalty_multiplier: dec!(0.25),
            source: "test".into(),
            ..OutcomeLearningContext::default()
        };

        assert_eq!(
            feedback::clamp_delta(dec!(0.04) * (Decimal::ONE + context.reward_multiplier)),
            dec!(0.06)
        );
        assert_eq!(
            feedback::clamp_delta(dec!(-0.04) * (Decimal::ONE + context.penalty_multiplier)),
            dec!(-0.05)
        );
    }

    #[test]
    fn hk_outcome_context_reads_follow_through_and_invalidation() {
        let rows = vec![
            LineageMetricRowRecord {
                row_id: "1".into(),
                snapshot_id: "s1".into(),
                tick_number: 1,
                recorded_at: OffsetDateTime::UNIX_EPOCH,
                window_size: 10,
                bucket: "promoted_outcomes".into(),
                rank: 0,
                label: "x".into(),
                family: None,
                session: None,
                market_regime: None,
                total: 10,
                resolved: 8,
                hits: 6,
                hit_rate: dec!(0.75),
                mean_return: dec!(0.03),
                mean_net_return: dec!(0.04),
                follow_expectancy: dec!(0.04),
                fade_expectancy: dec!(-0.02),
                wait_expectancy: dec!(0),
                mean_mfe: dec!(0.05),
                mean_mae: dec!(-0.02),
                follow_through_rate: dec!(0.70),
                invalidation_rate: dec!(0.10),
                structure_retention_rate: dec!(0.80),
                mean_convergence_score: dec!(0.60),
                mean_external_delta: dec!(0.02),
                external_follow_through_rate: dec!(0.40),
            },
            LineageMetricRowRecord {
                row_id: "2".into(),
                snapshot_id: "s1".into(),
                tick_number: 1,
                recorded_at: OffsetDateTime::UNIX_EPOCH,
                window_size: 10,
                bucket: "falsified_outcomes".into(),
                rank: 0,
                label: "y".into(),
                family: None,
                session: None,
                market_regime: None,
                total: 10,
                resolved: 8,
                hits: 5,
                hit_rate: dec!(0.62),
                mean_return: dec!(0.02),
                mean_net_return: dec!(0.03),
                follow_expectancy: dec!(0.03),
                fade_expectancy: dec!(0.01),
                wait_expectancy: dec!(0),
                mean_mfe: dec!(0.04),
                mean_mae: dec!(-0.02),
                follow_through_rate: dec!(0.60),
                invalidation_rate: dec!(0.75),
                structure_retention_rate: dec!(0.30),
                mean_convergence_score: dec!(0.50),
                mean_external_delta: dec!(0.01),
                external_follow_through_rate: dec!(0.20),
            },
        ];

        let context = derive_outcome_learning_context_from_hk_rows(&rows);
        assert_eq!(context.source, "hk_lineage");
        assert!(context.reward_multiplier > Decimal::ZERO);
        assert!(context.penalty_multiplier > Decimal::ZERO);
        assert_eq!(context.promoted_follow_through, dec!(0.70));
        assert_eq!(context.falsified_invalidation, dec!(0.75));
    }

    #[test]
    fn negative_return_normalization_is_nonzero_for_small_losses() {
        assert_eq!(
            outcome_context::normalize_negative_return(dec!(-0.01)),
            dec!(0.08)
        );
        assert_eq!(
            outcome_context::normalize_negative_return(dec!(0.01)),
            Decimal::ZERO
        );
    }

    #[test]
    fn hk_penalty_strength_counts_negative_mean_returns() {
        let rows = vec![LineageMetricRowRecord {
            row_id: "loss".into(),
            snapshot_id: "s1".into(),
            tick_number: 1,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            window_size: 10,
            bucket: "falsified_outcomes".into(),
            rank: 0,
            label: "x".into(),
            family: None,
            session: None,
            market_regime: None,
            total: 10,
            resolved: 10,
            hits: 0,
            hit_rate: Decimal::ZERO,
            mean_return: dec!(-0.01),
            mean_net_return: dec!(-0.01),
            follow_expectancy: dec!(-0.01),
            fade_expectancy: dec!(0.02),
            wait_expectancy: dec!(0),
            mean_mfe: Decimal::ZERO,
            mean_mae: dec!(-0.02),
            follow_through_rate: Decimal::ZERO,
            invalidation_rate: Decimal::ZERO,
            structure_retention_rate: Decimal::ZERO,
            mean_convergence_score: Decimal::ZERO,
            mean_external_delta: Decimal::ZERO,
            external_follow_through_rate: Decimal::ZERO,
        }];

        let context = derive_outcome_learning_context_from_hk_rows(&rows);
        assert!(context.penalty_multiplier > Decimal::ZERO);
    }
}
