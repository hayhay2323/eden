use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::laws::LawActivation;
use crate::ontology::mechanisms::MechanismCandidate;
use crate::ontology::predicates::AtomicPredicate;
use crate::ontology::reasoning::{ActionDirection, IntentDirection};
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

pub fn action_direction_from_intent_direction(
    direction: IntentDirection,
) -> Option<ActionDirection> {
    match direction {
        IntentDirection::Buy => Some(ActionDirection::Long),
        IntentDirection::Sell => Some(ActionDirection::Short),
        IntentDirection::Mixed | IntentDirection::Neutral => None,
    }
}

pub fn action_direction_from_case_label(value: &str) -> Option<ActionDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        "buy" | "long" => Some(ActionDirection::Long),
        "sell" | "short" => Some(ActionDirection::Short),
        "neutral" | "mixed" => Some(ActionDirection::Neutral),
        _ => None,
    }
}

pub fn action_direction_from_title_prefix(title: &str) -> Option<ActionDirection> {
    match title.split_whitespace().next()? {
        "Long" | "Buy" => Some(ActionDirection::Long),
        "Short" | "Sell" => Some(ActionDirection::Short),
        _ => None,
    }
}

pub fn infer_action_direction_from_texts(texts: &[&str]) -> Option<ActionDirection> {
    let lower = texts.join(" ").to_ascii_lowercase();
    let buy = [
        "long", "buy", "bid", "bull", "accum", "inflow", "support", "upside",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let sell = [
        "short",
        "sell",
        "bear",
        "distribution",
        "outflow",
        "downside",
        "liquidat",
        "unwind",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    match (buy, sell) {
        (true, false) => Some(ActionDirection::Long),
        (false, true) => Some(ActionDirection::Short),
        _ => None,
    }
}

pub fn action_direction_case_label(direction: ActionDirection) -> Option<&'static str> {
    match direction {
        ActionDirection::Long => Some("buy"),
        ActionDirection::Short => Some("sell"),
        ActionDirection::Neutral => None,
    }
}

pub fn action_direction_position_label(direction: ActionDirection) -> &'static str {
    match direction {
        ActionDirection::Long => "long",
        ActionDirection::Short => "short",
        ActionDirection::Neutral => "neutral",
    }
}

pub fn action_direction_sign(direction: ActionDirection) -> i8 {
    match direction {
        ActionDirection::Long => 1,
        ActionDirection::Short => -1,
        ActionDirection::Neutral => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_prefix_direction_accepts_canonical_action_words() {
        assert_eq!(
            action_direction_from_title_prefix("Long 700.HK"),
            Some(ActionDirection::Long)
        );
        assert_eq!(
            action_direction_from_title_prefix("Buy AAPL.US"),
            Some(ActionDirection::Long)
        );
        assert_eq!(
            action_direction_from_title_prefix("Short TSLA.US"),
            Some(ActionDirection::Short)
        );
        assert_eq!(
            action_direction_from_title_prefix("Sell 9988.HK"),
            Some(ActionDirection::Short)
        );
    }

    #[test]
    fn intent_direction_maps_only_directional_intents() {
        assert_eq!(
            action_direction_from_intent_direction(IntentDirection::Buy),
            Some(ActionDirection::Long)
        );
        assert_eq!(
            action_direction_from_intent_direction(IntentDirection::Sell),
            Some(ActionDirection::Short)
        );
        assert_eq!(
            action_direction_from_intent_direction(IntentDirection::Mixed),
            None
        );
        assert_eq!(
            action_direction_from_intent_direction(IntentDirection::Neutral),
            None
        );
    }

    #[test]
    fn text_direction_requires_one_sided_evidence() {
        assert_eq!(
            infer_action_direction_from_texts(&["accumulation with inflow"]),
            Some(ActionDirection::Long)
        );
        assert_eq!(
            infer_action_direction_from_texts(&["distribution with outflow"]),
            Some(ActionDirection::Short)
        );
        assert_eq!(
            infer_action_direction_from_texts(&["buy pressure but bearish unwind"]),
            None
        );
    }
}
