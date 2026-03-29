use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::cases::CaseSummary;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseReasoningAssessmentRecord {
    pub assessment_id: String,
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub market: String,
    pub symbol: String,
    pub title: String,
    pub sector: Option<String>,
    pub recommended_action: String,
    pub workflow_state: String,
    pub market_regime_bias: Option<String>,
    pub market_regime_confidence: Option<Decimal>,
    pub market_breadth_delta: Option<Decimal>,
    pub market_average_return: Option<Decimal>,
    pub market_directional_consensus: Option<Decimal>,
    pub source: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub actor: Option<String>,
    pub note: Option<String>,
    pub law_kinds: Vec<String>,
    pub predicate_kinds: Vec<String>,
    pub composite_state_kinds: Vec<String>,
    pub primary_mechanism_kind: Option<String>,
    pub primary_mechanism_score: Option<Decimal>,
    pub competing_mechanism_kinds: Vec<String>,
    pub invalidation_rules: Vec<String>,
    pub reasoning_profile: crate::ontology::CaseReasoningProfile,
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
            owner: summary.owner.clone(),
            reviewer: summary.reviewer.clone(),
            actor: summary.workflow_actor.clone(),
            note: summary.workflow_note.clone(),
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
            reasoning_profile: summary.reasoning_profile.clone(),
        }
    }

    pub fn record_id(&self) -> &str {
        &self.assessment_id
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
            why_now: "test".into(),
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
        };

        let record = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        assert_eq!(record.market, "hk");
        assert_eq!(record.sector.as_deref(), Some("Technology"));
        assert_eq!(
            record.primary_mechanism_kind.as_deref(),
            Some("Mechanical Execution Signature")
        );
        assert_eq!(record.market_regime_bias.as_deref(), Some("neutral"));
        assert_eq!(record.law_kinds, vec!["Persistence"]);
        assert_eq!(record.predicate_kinds, vec!["Signal Recurs"]);
    }
}
