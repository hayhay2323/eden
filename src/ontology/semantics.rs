use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::laws::LawActivation;
use crate::ontology::mechanisms::MechanismCandidate;
use crate::ontology::predicates::AtomicPredicate;
use crate::ontology::states::CompositeState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanReviewVerdict {
    Confirmed,
    ReviewRequested,
    Rejected,
    Modified,
}

impl HumanReviewVerdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Confirmed => "Confirmed",
            Self::ReviewRequested => "Review Requested",
            Self::Rejected => "Rejected",
            Self::Modified => "Modified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanReviewReasonKind {
    MechanismMismatch,
    TimingMismatch,
    RiskTooHigh,
    EventRisk,
    ExecutionConstraint,
    EvidenceTooWeak,
    Unspecified,
}

impl HumanReviewReasonKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MechanismMismatch => "Mechanism Mismatch",
            Self::TimingMismatch => "Timing Mismatch",
            Self::RiskTooHigh => "Risk Too High",
            Self::EventRisk => "Event Risk",
            Self::ExecutionConstraint => "Execution Constraint",
            Self::EvidenceTooWeak => "Evidence Too Weak",
            Self::Unspecified => "Unspecified",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanReviewReason {
    pub kind: HumanReviewReasonKind,
    pub label: String,
    pub confidence: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanReviewContext {
    pub verdict: HumanReviewVerdict,
    pub verdict_label: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<HumanReviewReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseReasoningProfile {
    pub laws: Vec<LawActivation>,
    pub predicates: Vec<AtomicPredicate>,
    pub composite_states: Vec<CompositeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_review: Option<HumanReviewContext>,
    pub primary_mechanism: Option<MechanismCandidate>,
    pub competing_mechanisms: Vec<MechanismCandidate>,
    /// Mechanisms whose supporting conditions have collapsed, detected automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automated_invalidations: Vec<MechanismInvalidation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismInvalidation {
    pub mechanism: String,
    pub reason: String,
}
