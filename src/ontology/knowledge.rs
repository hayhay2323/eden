use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::live_snapshot::LiveMarket;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEventImpact {
    pub primary_scope: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secondary_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_markets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_sectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_symbols: Vec<String>,
    pub preferred_expression: String,
    pub requires_market_confirmation: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisive_factors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMacroEventCandidate {
    pub candidate_id: String,
    pub tick: u64,
    pub market: LiveMarket,
    pub source_kind: String,
    pub source_name: String,
    pub event_type: String,
    pub authority_level: String,
    pub headline: String,
    pub summary: String,
    pub confidence: Decimal,
    pub novelty_score: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jurisdictions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    pub impact: AgentEventImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMacroEvent {
    pub event_id: String,
    pub tick: u64,
    pub market: LiveMarket,
    pub event_type: String,
    pub authority_level: String,
    pub headline: String,
    pub summary: String,
    pub confidence: Decimal,
    pub confirmation_state: String,
    pub impact: AgentEventImpact,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_notice_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promotion_reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeNodeKind {
    Market,
    Symbol,
    Sector,
    Institution,
    Theme,
    Region,
    Custom,
    MacroEvent,
    Decision,
    Hypothesis,
    Setup,
    Mechanism,
    WorldEntity,
    BackwardCause,
    Position,
}

impl KnowledgeNodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Symbol => "symbol",
            Self::Sector => "sector",
            Self::Institution => "institution",
            Self::Theme => "theme",
            Self::Region => "region",
            Self::Custom => "custom",
            Self::MacroEvent => "macro_event",
            Self::Decision => "decision",
            Self::Hypothesis => "hypothesis",
            Self::Setup => "setup",
            Self::Mechanism => "mechanism",
            Self::WorldEntity => "world_entity",
            Self::BackwardCause => "backward_cause",
            Self::Position => "position",
        }
    }
}

impl From<&str> for KnowledgeNodeKind {
    fn from(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "market" => Self::Market,
            "symbol" => Self::Symbol,
            "sector" => Self::Sector,
            "institution" => Self::Institution,
            "theme" => Self::Theme,
            "region" => Self::Region,
            "custom" => Self::Custom,
            "macro_event" => Self::MacroEvent,
            "decision" => Self::Decision,
            "hypothesis" => Self::Hypothesis,
            "setup" => Self::Setup,
            "mechanism" => Self::Mechanism,
            "world_entity" => Self::WorldEntity,
            "backward_cause" => Self::BackwardCause,
            "position" => Self::Position,
            _ => Self::Custom,
        }
    }
}

impl std::fmt::Display for KnowledgeNodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKnowledgeNodeRef {
    pub node_kind: KnowledgeNodeKind,
    pub node_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRefKind {
    Hypothesis,
    Setup,
    Mechanism,
    Scope,
    BackwardCause,
    Position,
    Workflow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub kind: EvidenceRefKind,
    pub ref_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRelation {
    ImpactsMarket,
    ImpactsSector,
    ImpactsSymbol,
    SupportsDecision,
    DominatesScope,
    DescribesScope,
    TargetsScope,
    CandidateForLeaf,
    LeadingCauseForLeaf,
    TracksSymbol,
    TracksSector,
    InstantiatesHypothesis,
    PrimaryMechanism,
    CompetingMechanism,
}

impl KnowledgeRelation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ImpactsMarket => "impacts_market",
            Self::ImpactsSector => "impacts_sector",
            Self::ImpactsSymbol => "impacts_symbol",
            Self::SupportsDecision => "supports_decision",
            Self::DominatesScope => "dominates_scope",
            Self::DescribesScope => "describes_scope",
            Self::TargetsScope => "targets_scope",
            Self::CandidateForLeaf => "candidate_for_leaf",
            Self::LeadingCauseForLeaf => "leading_cause_for_leaf",
            Self::TracksSymbol => "tracks_symbol",
            Self::TracksSector => "tracks_sector",
            Self::InstantiatesHypothesis => "instantiates_hypothesis",
            Self::PrimaryMechanism => "primary_mechanism",
            Self::CompetingMechanism => "competing_mechanism",
        }
    }
}

impl std::fmt::Display for KnowledgeRelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub enum KnowledgeLinkAttributes {
    #[default]
    Generic,
    ImpactsMarket {
        event_type: String,
        authority_level: String,
        primary_scope: String,
        preferred_expression: String,
    },
    ImpactsSector {
        event_type: String,
        authority_level: String,
        primary_scope: String,
        preferred_expression: String,
    },
    ImpactsSymbol {
        event_type: String,
        authority_level: String,
        primary_scope: String,
        preferred_expression: String,
    },
    SupportsDecision {
        decision_scope_kind: String,
        primary_scope: String,
        best_action: String,
    },
    DominatesScope {
        decision_scope_kind: String,
        primary_scope: String,
        best_action: String,
    },
    DescribesScope {
        layer: String,
        regime: String,
    },
    TargetsScope {
        source_kind: String,
        scope_kind: String,
    },
    CandidateForLeaf {
        leaf_regime: String,
        contest_state: String,
    },
    LeadingCauseForLeaf {
        leaf_regime: String,
        contest_state: String,
        leader_streak: u64,
        cause_gap: Option<Decimal>,
    },
    TracksSymbol {
        stage: String,
        direction: String,
        age_ticks: u64,
        exit_forming: bool,
    },
    TracksSector {
        stage: String,
        direction: String,
        age_ticks: u64,
        exit_forming: bool,
    },
    InstantiatesHypothesis {
        action: String,
        confidence_gap: Decimal,
    },
    PrimaryMechanism {
        mechanism_score: Decimal,
        case_action: String,
    },
    CompetingMechanism {
        mechanism_score: Decimal,
        case_action: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKnowledgeLink {
    pub link_id: String,
    pub relation: KnowledgeRelation,
    pub source: AgentKnowledgeNodeRef,
    pub target: AgentKnowledgeNodeRef,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "matches_generic_link_attributes")]
    pub attributes: KnowledgeLinkAttributes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

fn matches_generic_link_attributes(value: &KnowledgeLinkAttributes) -> bool {
    matches!(value, KnowledgeLinkAttributes::Generic)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEventKind {
    LeadingCauseAssessment,
    HypothesisInstantiation,
    MechanismAssessment,
    PositionTracking,
}

impl KnowledgeEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LeadingCauseAssessment => "leading_cause_assessment",
            Self::HypothesisInstantiation => "hypothesis_instantiation",
            Self::MechanismAssessment => "mechanism_assessment",
            Self::PositionTracking => "position_tracking",
        }
    }
}

impl std::fmt::Display for KnowledgeEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub enum KnowledgeEventAttributes {
    #[default]
    Generic,
    LeadingCauseAssessment {
        leaf_regime: String,
        contest_state: String,
        leader_streak: u64,
        cause_gap: Option<Decimal>,
    },
    HypothesisInstantiation {
        action: String,
        confidence_gap: Decimal,
        scope_kind: String,
    },
    MechanismAssessment {
        role: String,
        mechanism_score: Decimal,
        case_action: String,
    },
    PositionTracking {
        scope_kind: String,
        stage: String,
        direction: String,
        age_ticks: u64,
        exit_forming: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKnowledgeEvent {
    pub event_id: String,
    pub kind: KnowledgeEventKind,
    pub subject: AgentKnowledgeNodeRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<AgentKnowledgeNodeRef>,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "matches_generic_event_attributes")]
    pub attributes: KnowledgeEventAttributes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
}

fn matches_generic_event_attributes(value: &KnowledgeEventAttributes) -> bool {
    matches!(value, KnowledgeEventAttributes::Generic)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "schema", rename_all = "snake_case")]
pub enum KnowledgeNodeAttributes {
    Generic,
    MacroEvent {
        event_type: String,
        authority_level: String,
        confidence: Decimal,
        confirmation_state: String,
        primary_scope: String,
        preferred_expression: String,
        requires_market_confirmation: bool,
        affected_markets: Vec<String>,
        affected_sectors: Vec<String>,
        affected_symbols: Vec<String>,
        decisive_factors: Vec<String>,
    },
    Scope {
        scope_kind: String,
        scope_label: String,
    },
    Hypothesis {
        family_key: String,
        family_label: String,
        statement: String,
        confidence: Decimal,
        local_support_weight: Decimal,
        local_contradict_weight: Decimal,
        propagated_support_weight: Decimal,
        propagated_contradict_weight: Decimal,
        propagation_path_ids: Vec<String>,
        expected_observations: Vec<String>,
    },
    Setup {
        action: String,
        time_horizon: String,
        confidence: Decimal,
        confidence_gap: Decimal,
        heuristic_edge: Decimal,
        workflow_id: Option<String>,
        entry_rationale: String,
        risk_notes: Vec<String>,
    },
    Mechanism {
        label: String,
        summary: String,
        invalidation: Vec<String>,
        human_checks: Vec<String>,
    },
    WorldEntity {
        layer: String,
        regime: String,
        confidence: Decimal,
        local_support: Decimal,
        propagated_support: Decimal,
        drivers: Vec<String>,
    },
    BackwardCause {
        layer: String,
        depth: u8,
        explanation: String,
        chain_summary: Option<String>,
        confidence: Decimal,
        support_weight: Decimal,
        contradict_weight: Decimal,
        net_conviction: Decimal,
        competitive_score: Decimal,
        falsifier: Option<String>,
        references: Vec<String>,
    },
    Position {
        market: String,
        symbol: String,
        sector: Option<String>,
        stage: String,
        direction: String,
        entry_confidence: Decimal,
        current_confidence: Decimal,
        entry_price: Option<Decimal>,
        pnl: Option<Decimal>,
        age_ticks: u64,
        degradation_score: Option<Decimal>,
        exit_forming: bool,
    },
    Decision {
        scope_kind: String,
        title: String,
        bias: String,
        best_action: String,
        regime_bias: String,
        alpha_horizon: String,
        confidence: Decimal,
        score: Decimal,
        preferred_expression: Option<String>,
        reference_symbols: Vec<String>,
    },
}

pub fn merged_knowledge_links(
    snapshot_links: &[AgentKnowledgeLink],
    recommendation_links: &[AgentKnowledgeLink],
) -> Vec<AgentKnowledgeLink> {
    let mut links = BTreeMap::new();
    for link in snapshot_links.iter().chain(recommendation_links.iter()) {
        links.insert(link.link_id.clone(), link.clone());
    }
    links.into_values().collect()
}

pub fn merged_knowledge_events(
    snapshot_events: &[AgentKnowledgeEvent],
    recommendation_events: &[AgentKnowledgeEvent],
) -> Vec<AgentKnowledgeEvent> {
    let mut events = BTreeMap::new();
    for event in snapshot_events.iter().chain(recommendation_events.iter()) {
        events.insert(event.event_id.clone(), event.clone());
    }
    events.into_values().collect()
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn merged_knowledge_links_deduplicates_by_link_id() {
        let link = AgentKnowledgeLink {
            link_id: "link:1".into(),
            relation: KnowledgeRelation::ImpactsSymbol,
            source: AgentKnowledgeNodeRef {
                node_kind: KnowledgeNodeKind::MacroEvent,
                node_id: "macro_event:1".into(),
                label: "Fed repricing".into(),
            },
            target: AgentKnowledgeNodeRef {
                node_kind: KnowledgeNodeKind::Symbol,
                node_id: "symbol:700.HK".into(),
                label: "700.HK".into(),
            },
            confidence: dec!(0.7),
            attributes: KnowledgeLinkAttributes::ImpactsSymbol {
                event_type: "rates_macro".into(),
                authority_level: "high".into(),
                primary_scope: "market".into(),
                preferred_expression: "risk_off".into(),
            },
            rationale: Some("rates hit property".into()),
        };

        let merged =
            merged_knowledge_links(std::slice::from_ref(&link), std::slice::from_ref(&link));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].link_id, "link:1");
    }

    #[test]
    fn knowledge_node_attributes_serialize_as_tagged_objects() {
        let value = serde_json::to_value(KnowledgeNodeAttributes::Scope {
            scope_kind: "symbol".into(),
            scope_label: "700.HK".into(),
        })
        .unwrap();
        assert_eq!(value["schema"], "scope");
        assert_eq!(value["scope_kind"], "symbol");
    }

    #[test]
    fn knowledge_link_attributes_serialize_as_tagged_objects() {
        let value = serde_json::to_value(KnowledgeLinkAttributes::SupportsDecision {
            decision_scope_kind: "symbol".into(),
            primary_scope: "market".into(),
            best_action: "follow".into(),
        })
        .unwrap();
        assert_eq!(value["schema"], "supports_decision");
        assert_eq!(value["decision_scope_kind"], "symbol");
    }

    #[test]
    fn knowledge_relation_serializes_as_snake_case() {
        let value = serde_json::to_value(KnowledgeRelation::PrimaryMechanism).unwrap();
        assert_eq!(value, serde_json::json!("primary_mechanism"));
    }

    #[test]
    fn merged_knowledge_events_deduplicates_by_event_id() {
        let event = AgentKnowledgeEvent {
            event_id: "event:1".into(),
            kind: KnowledgeEventKind::PositionTracking,
            subject: AgentKnowledgeNodeRef {
                node_kind: KnowledgeNodeKind::Position,
                node_id: "position:wf:1".into(),
                label: "700.HK long".into(),
            },
            object: Some(AgentKnowledgeNodeRef {
                node_kind: KnowledgeNodeKind::Symbol,
                node_id: "symbol:700.hk".into(),
                label: "700.HK".into(),
            }),
            confidence: dec!(0.8),
            evidence: vec![EvidenceRef {
                kind: EvidenceRefKind::Workflow,
                ref_id: "wf:1".into(),
                label: None,
            }],
            attributes: KnowledgeEventAttributes::PositionTracking {
                scope_kind: "symbol".into(),
                stage: "monitoring".into(),
                direction: "long".into(),
                age_ticks: 12,
                exit_forming: false,
            },
            rationale: Some("tracking active position".into()),
        };

        let merged =
            merged_knowledge_events(std::slice::from_ref(&event), std::slice::from_ref(&event));
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].event_id, "event:1");
    }

    #[test]
    fn knowledge_event_attributes_serialize_as_tagged_objects() {
        let value = serde_json::to_value(KnowledgeEventAttributes::MechanismAssessment {
            role: "primary".into(),
            mechanism_score: dec!(0.8),
            case_action: "enter".into(),
        })
        .unwrap();
        assert_eq!(value["schema"], "mechanism_assessment");
        assert_eq!(value["role"], "primary");
    }
}
