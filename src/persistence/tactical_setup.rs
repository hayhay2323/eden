use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::reasoning::{
    ArchetypeProjection, CaseSignature, ExpectationBinding, ExpectationViolation, Hypothesis,
    IntentHypothesis, TacticalAction, TacticalSetup,
};

fn default_bucket_session() -> crate::ontology::horizon::HorizonBucket {
    crate::ontology::horizon::HorizonBucket::Session
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TacticalSetupRecord {
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub scope_key: String,
    pub title: String,
    pub action: TacticalAction,
    pub time_horizon: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence_gap: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub heuristic_edge: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub convergence_score: Option<Decimal>,
    pub workflow_id: Option<String>,
    pub entry_rationale: String,
    pub risk_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default)]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default)]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default)]
    pub expectation_violations: Vec<ExpectationViolation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    /// Primary trading horizon bucket. New in Wave 5.
    /// Pre-Wave-5 records deserialize with `bucket = Session` as the conservative default.
    #[serde(default = "default_bucket_session")]
    pub primary_horizon: crate::ontology::horizon::HorizonBucket,
    pub based_on: Vec<String>,
    pub blocked_by: Vec<String>,
    pub promoted_by: Vec<String>,
    pub falsified_by: Vec<String>,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
}

impl TacticalSetupRecord {
    pub fn from_setup(setup: &TacticalSetup, recorded_at: OffsetDateTime) -> Self {
        Self::from_setup_with_hypothesis(setup, None, recorded_at)
    }

    pub fn from_setup_with_hypothesis(
        setup: &TacticalSetup,
        hypothesis: Option<&Hypothesis>,
        recorded_at: OffsetDateTime,
    ) -> Self {
        Self {
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope_key: format!("{:?}", setup.scope),
            title: setup.title.clone(),
            action: setup.action,
            time_horizon: setup.horizon.primary.to_legacy_string().to_string(),
            confidence: setup.confidence,
            confidence_gap: setup.confidence_gap,
            heuristic_edge: setup.heuristic_edge,
            convergence_score: setup.convergence_score,
            workflow_id: Some(setup.workflow_id.clone().unwrap_or_else(|| {
                crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                    &setup.setup_id,
                )
            })),
            entry_rationale: setup.entry_rationale.clone(),
            risk_notes: setup.risk_notes.clone(),
            case_signature: Some(setup.case_signature(hypothesis)),
            archetype_projections: setup.archetype_projections(hypothesis),
            expectation_bindings: hypothesis
                .map(Hypothesis::expected_bindings)
                .unwrap_or_default(),
            expectation_violations: hypothesis
                .map(Hypothesis::expectation_violations)
                .unwrap_or_default(),
            inferred_intent: Some(setup.intent_hypothesis(hypothesis)),
            primary_horizon: setup.horizon.primary,
            based_on: setup.lineage.based_on.clone(),
            blocked_by: setup.lineage.blocked_by.clone(),
            promoted_by: setup.lineage.promoted_by.clone(),
            falsified_by: setup.lineage.falsified_by.clone(),
            recorded_at,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.setup_id
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::ontology::reasoning::{default_case_horizon, ReasoningScope};
    use crate::ontology::ProvenanceMetadata;
    use crate::ontology::ProvenanceSource;
    use crate::ontology::Symbol;

    #[test]
    fn tactical_setup_record_preserves_gap_and_runner_up() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_trace_id("setup:700.HK:enter")
            .with_inputs(["hyp:700.HK:flow"]),
            lineage: crate::ontology::DecisionLineage {
                based_on: vec!["hyp:700.HK:flow".into()],
                blocked_by: vec![],
                promoted_by: vec!["review -> enter".into()],
                falsified_by: vec!["local flow flips negative".into()],
            },
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.62),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.11),
            convergence_score: Some(dec!(0.44)),
            convergence_detail: None,
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow explanation leads".into(),
            causal_narrative: None,
            risk_notes: vec!["runner-up remains close".into()],
            review_reason_code: None,
            policy_verdict: None,
        };

        let record = TacticalSetupRecord::from_setup(&setup, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(
            record.runner_up_hypothesis_id.as_deref(),
            Some("hyp:700.HK:risk")
        );
        assert_eq!(record.confidence_gap, dec!(0.18));
        assert_eq!(record.convergence_score, Some(dec!(0.44)));
        assert_eq!(record.based_on, vec!["hyp:700.HK:flow"]);
        assert_eq!(record.promoted_by, vec!["review -> enter"]);
        assert_eq!(record.falsified_by, vec!["local flow flips negative"]);
    }
}
