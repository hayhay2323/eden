use super::*;
use crate::live_snapshot::{
    LiveClusterState, LiveLineageMetric, LiveRawSource, LiveSignalTranslationGap,
    LiveSuccessPattern, LiveTemporalBar, LiveWorldSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarketSessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SymbolStateId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CaseContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecommendationContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MacroEventContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowContractId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalHistoryRef {
    pub key: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "rfc3339::option"
    )]
    pub latest_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalGraphRef {
    pub node_id: String,
    pub node_kind: KnowledgeNodeKind,
    pub endpoint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalObjectKind {
    MarketSession,
    SymbolState,
    PerceptualState,
    PerceptualEvidence,
    PerceptualExpectation,
    AttentionAllocation,
    PerceptualUncertainty,
    Case,
    Recommendation,
    MacroEvent,
    Thread,
    Workflow,
}

const OPERATIONAL_OBJECT_KIND_SLUGS: &[(OperationalObjectKind, &str)] = &[
    (OperationalObjectKind::MarketSession, "market_session"),
    (OperationalObjectKind::SymbolState, "symbol_state"),
    (OperationalObjectKind::PerceptualState, "perceptual_state"),
    (
        OperationalObjectKind::PerceptualEvidence,
        "perceptual_evidence",
    ),
    (
        OperationalObjectKind::PerceptualExpectation,
        "perceptual_expectation",
    ),
    (
        OperationalObjectKind::AttentionAllocation,
        "attention_allocation",
    ),
    (
        OperationalObjectKind::PerceptualUncertainty,
        "perceptual_uncertainty",
    ),
    (OperationalObjectKind::Case, "case"),
    (OperationalObjectKind::Recommendation, "recommendation"),
    (OperationalObjectKind::MacroEvent, "macro_event"),
    (OperationalObjectKind::Thread, "thread"),
    (OperationalObjectKind::Workflow, "workflow"),
];

impl OperationalObjectKind {
    pub fn slug(self) -> &'static str {
        OPERATIONAL_OBJECT_KIND_SLUGS
            .iter()
            .find_map(|(kind, slug)| (*kind == self).then_some(*slug))
            .expect("operational object kind slug mapping should stay exhaustive")
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        OPERATIONAL_OBJECT_KIND_SLUGS
            .iter()
            .find_map(|(kind, slug)| slug.eq_ignore_ascii_case(raw).then_some(*kind))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalObjectRef {
    pub id: String,
    pub kind: OperationalObjectKind,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcomes: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendationHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcomes: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketSessionRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolStateRelationships {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perceptual_state: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_evidence: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opposing_evidence: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_evidence: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectations: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_allocations: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertainties: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_events: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRelationships {
    pub symbol: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationRelationships {
    pub symbol: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MacroEventRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolStateSummary {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perceptual_state_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perceptual_trend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weighted_support_fraction: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_support_fraction: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_supporting_evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_opposing_evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_missing_evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_statuses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_composite: Option<Decimal>,
    pub has_depth: bool,
    pub has_brokers: bool,
    pub invalidated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_falsifier: Option<String>,
    pub latest_event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationSummary {
    pub action: String,
    pub bias: String,
    pub severity: String,
    pub confidence: Decimal,
    pub best_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_confirmation_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_margin: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_success_pattern_signature: Option<String>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance_reason_code: ActionGovernanceReasonCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventSummary {
    pub headline: String,
    pub event_type: String,
    pub authority_level: String,
    pub confidence: Decimal,
    pub confirmation_state: String,
    pub primary_scope: String,
    pub preferred_expression: String,
    pub affected_symbol_count: usize,
    pub affected_sector_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalRelationshipGroup {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalNeighborhood {
    pub root: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<OperationalRelationshipGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_ref: Option<OperationalGraphRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history_refs: Vec<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalNavigation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<OperationalGraphRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<OperationalRelationshipGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSessionContract {
    pub id: MarketSessionId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub computed_at: OffsetDateTime,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    pub should_speak: bool,
    pub priority: Decimal,
    pub active_thread_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_headline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_tools: Vec<AgentSuggestedToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_summary: Option<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: MarketSessionRelationships,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbol_refs: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolStateContract {
    pub id: SymbolStateId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: SymbolStateRelationships,
    pub summary: SymbolStateSummary,
    pub graph_ref: OperationalGraphRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perceptual_state: Option<crate::ontology::world::PerceptualState>,
    pub state: AgentSymbolState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualStateContract {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub graph_ref: OperationalGraphRef,
    pub state: crate::ontology::world::PerceptualState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualEvidenceContract {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub graph_ref: OperationalGraphRef,
    pub evidence: crate::ontology::world::PerceptualEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualExpectationContract {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub graph_ref: OperationalGraphRef,
    pub expectation: crate::ontology::world::PerceptualExpectation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionAllocationContract {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub graph_ref: OperationalGraphRef,
    pub allocation: crate::ontology::world::AttentionAllocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualUncertaintyContract {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub graph_ref: OperationalGraphRef,
    pub uncertainty: crate::ontology::world::PerceptualUncertainty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseContract {
    pub id: CaseContractId,
    pub setup_id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub title: String,
    pub action: String,
    pub workflow_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_gap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_leader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_horizon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_primary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_horizon_gate_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causal_narrative: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tension_driver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_isolated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_active_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_silent_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_confirmation_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_margin: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_velocity: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_acceleration: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_family: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reason_subreasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_since_first_seen: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_position_in_range: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state_confidence: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_velocity_5t: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_fraction_velocity_5t: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_confidence: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absence_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_winner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_runner_up: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_rank: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_persistence_ticks: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_stability_rounds: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_success_pattern_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_violations: Vec<ExpectationViolation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_ids: Vec<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub relationships: CaseRelationships,
    pub symbol_ref: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_refs: Vec<OperationalObjectRef>,
    pub graph_ref: OperationalGraphRef,
    #[serde(default)]
    pub history_refs: CaseHistoryRefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationContract {
    pub id: RecommendationContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_case_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_setup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_workflow_id: Option<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub relationships: RecommendationRelationships,
    pub summary: RecommendationSummary,
    pub symbol_ref: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    pub graph_ref: OperationalGraphRef,
    pub recommendation: AgentRecommendation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_violations: Vec<ExpectationViolation>,
    #[serde(default)]
    pub history_refs: RecommendationHistoryRefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventContract {
    pub id: MacroEventContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: MacroEventRelationships,
    pub summary: MacroEventSummary,
    pub graph_ref: OperationalGraphRef,
    pub event: AgentMacroEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContract {
    pub id: ThreadContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub thread: AgentThread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContract {
    pub id: WorkflowContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    pub synthetic: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub case_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_ids: Vec<String>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: WorkflowRelationships,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub case_refs: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_refs: Vec<OperationalObjectRef>,
    #[serde(default)]
    pub history_refs: WorkflowHistoryRefs,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemOrigin {
    Case,
    Judgment,
    Thread,
    MacroEvent,
    #[default]
    Operator,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemGrain {
    #[default]
    Market,
    Sector,
    Symbol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OperatorWorkflowSurface {
    pub id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub lane: String,
    pub status: String,
    pub priority: Decimal,
    pub object_kind: String,
    pub object_id: String,
    pub title: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlock_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorWorkItem {
    pub id: String,
    #[serde(default)]
    pub origin: WorkItemOrigin,
    #[serde(default)]
    pub grain: WorkItemGrain,
    pub lane: String,
    pub status: String,
    pub priority: Decimal,
    pub scope_kind: String,
    pub scope_id: String,
    pub title: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_confirmation_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub competition_margin: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cohort_id: Option<String>,
    // Compatibility fields during the queue migration. Prefer `navigation` + `source_refs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<OperationalObjectRef>,
    #[serde(default)]
    pub navigation: OperationalNavigation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CohortSignal {
    pub id: String,
    pub market: LiveMarket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub driver_class: String,
    pub action: String,
    pub member_count: usize,
    pub mean_confidence: Decimal,
    pub mean_peer_confirmation_ratio: Decimal,
    pub mean_competition_margin: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalSidecars {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_flows: Vec<AgentSectorFlow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sources: Vec<crate::live_snapshot::LiveRawSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signal_translation_gaps: Vec<crate::live_snapshot::LiveSignalTranslationGap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cluster_states: Vec<crate::live_snapshot::LiveClusterState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_summary: Option<crate::live_snapshot::LiveWorldSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backward_investigations: Vec<BackwardInvestigation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_state: Option<WorldStateSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_event_candidates: Vec<AgentMacroEventCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_links: Vec<AgentKnowledgeLink>,
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    pub(crate) operator_workflows: Vec<OperatorWorkflowSurface>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operator_work_items: Vec<OperatorWorkItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cohort_signals: Vec<CohortSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganOverviewContract {
    pub role: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub computed_at: OffsetDateTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_summary: Option<LiveWorldSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_state: Option<WorldStateSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cluster_states: Vec<LiveClusterState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sources: Vec<LiveRawSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signal_translation_gaps: Vec<LiveSignalTranslationGap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_states: Vec<PerceptualStateContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_evidence: Vec<PerceptualEvidenceContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_expectations: Vec<PerceptualExpectationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_allocations: Vec<AttentionAllocationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_uncertainties: Vec<PerceptualUncertaintyContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projected_cases: Vec<CaseContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalSnapshot {
    pub version: u32,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub computed_at: OffsetDateTime,
    pub market_session: MarketSessionContract,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_turns: Vec<AgentTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<AgentNotice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_transitions: Vec<AgentTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<SymbolStateContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_states: Vec<PerceptualStateContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_evidence: Vec<PerceptualEvidenceContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_expectations: Vec<PerceptualExpectationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_allocations: Vec<AttentionAllocationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_uncertainties: Vec<PerceptualUncertaintyContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<CaseContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_recommendation: Option<AgentMarketRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_recommendations: Vec<AgentSectorRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<RecommendationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_events: Vec<MacroEventContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<ThreadContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflows: Vec<WorkflowContract>,
    #[serde(default)]
    pub sidecars: OperationalSidecars,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LiveEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temporal_bars: Vec<LiveTemporalBar>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineage: Vec<LiveLineageMetric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub success_patterns: Vec<LiveSuccessPattern>,
}

impl OperationalSnapshot {
    pub fn market_session_ref(&self) -> OperationalObjectRef {
        OperationalObjectRef {
            id: self.market_session.id.0.clone(),
            kind: OperationalObjectKind::MarketSession,
            endpoint: format!("/api/ontology/{}/market-session", market_slug(self.market)),
            label: Some(format!(
                "{} Market",
                market_slug(self.market).to_ascii_uppercase()
            )),
        }
    }

    pub fn symbol(&self, symbol: &str) -> Option<&SymbolStateContract> {
        self.symbols
            .iter()
            .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
    }

    pub fn perceptual_state(&self, state_id: &str) -> Option<&PerceptualStateContract> {
        self.perceptual_states
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(state_id))
    }

    pub fn perceptual_evidence(&self, evidence_id: &str) -> Option<&PerceptualEvidenceContract> {
        self.perceptual_evidence
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(evidence_id))
    }

    pub fn perceptual_expectation(
        &self,
        expectation_id: &str,
    ) -> Option<&PerceptualExpectationContract> {
        self.perceptual_expectations
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(expectation_id))
    }

    pub fn attention_allocation(
        &self,
        allocation_id: &str,
    ) -> Option<&AttentionAllocationContract> {
        self.attention_allocations
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(allocation_id))
    }

    pub fn perceptual_uncertainty(
        &self,
        uncertainty_id: &str,
    ) -> Option<&PerceptualUncertaintyContract> {
        self.perceptual_uncertainties
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(uncertainty_id))
    }

    pub fn case(&self, case_id: &str) -> Option<&CaseContract> {
        self.cases
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(case_id))
    }

    pub fn recommendation(&self, recommendation_id: &str) -> Option<&RecommendationContract> {
        self.recommendations
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(recommendation_id))
    }

    pub fn macro_event(&self, event_id: &str) -> Option<&MacroEventContract> {
        self.macro_events
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(event_id))
    }

    pub fn thread(&self, thread_id: &str) -> Option<&ThreadContract> {
        self.threads
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(thread_id))
    }

    pub fn workflow(&self, workflow_id: &str) -> Option<&WorkflowContract> {
        self.workflows
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(workflow_id))
    }

    pub fn sector_flow(&self, sector: &str) -> Option<&AgentSectorFlow> {
        self.sidecars
            .sector_flows
            .iter()
            .find(|item| item.sector.eq_ignore_ascii_case(sector))
    }

    pub fn backward_investigation(&self, symbol: &str) -> Option<&BackwardInvestigation> {
        self.sidecars
            .backward_investigations
            .iter()
            .find(|item| match &item.leaf_scope {
                crate::ontology::ReasoningScope::Symbol(candidate) => {
                    candidate.0.eq_ignore_ascii_case(symbol)
                }
                _ => false,
            })
    }

    pub fn world_state(&self) -> Option<&WorldStateSnapshot> {
        self.sidecars.world_state.as_ref()
    }

    pub fn organ_overview(&self) -> OrganOverviewContract {
        OrganOverviewContract {
            role: "sensory_organ".into(),
            market: self.market,
            source_tick: self.source_tick,
            observed_at: self.observed_at,
            computed_at: self.computed_at,
            world_summary: self.sidecars.world_summary.clone(),
            world_state: self.sidecars.world_state.clone(),
            cluster_states: self.sidecars.cluster_states.clone(),
            raw_sources: self.sidecars.raw_sources.clone(),
            signal_translation_gaps: self.sidecars.signal_translation_gaps.clone(),
            perceptual_states: self.perceptual_states.clone(),
            perceptual_evidence: self.perceptual_evidence.clone(),
            perceptual_expectations: self.perceptual_expectations.clone(),
            attention_allocations: self.attention_allocations.clone(),
            perceptual_uncertainties: self.perceptual_uncertainties.clone(),
            projected_cases: self.cases.clone(),
        }
    }

    pub fn navigation(
        &self,
        kind: OperationalObjectKind,
        id: &str,
    ) -> Option<&OperationalNavigation> {
        match kind {
            OperationalObjectKind::MarketSession => self
                .market_session
                .id
                .0
                .eq_ignore_ascii_case(id)
                .then_some(&self.market_session.navigation),
            OperationalObjectKind::SymbolState => self
                .symbols
                .iter()
                .find(|item| {
                    item.id.0.eq_ignore_ascii_case(id) || item.symbol.eq_ignore_ascii_case(id)
                })
                .map(|item| &item.navigation),
            OperationalObjectKind::PerceptualState => {
                self.perceptual_state(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::PerceptualEvidence => {
                self.perceptual_evidence(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::PerceptualExpectation => {
                self.perceptual_expectation(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::AttentionAllocation => {
                self.attention_allocation(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::PerceptualUncertainty => {
                self.perceptual_uncertainty(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::Case => self.case(id).map(|item| &item.navigation),
            OperationalObjectKind::Recommendation => {
                self.recommendation(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::MacroEvent => self.macro_event(id).map(|item| &item.navigation),
            OperationalObjectKind::Thread => self.thread(id).map(|item| &item.navigation),
            OperationalObjectKind::Workflow => self.workflow(id).map(|item| &item.navigation),
        }
    }

    pub fn resolve_object_ref(
        &self,
        object_ref: &OperationalObjectRef,
    ) -> Option<OperationalObjectRef> {
        self.navigation(object_ref.kind, &object_ref.id)
            .and_then(|navigation| navigation.self_ref.clone())
            .or_else(|| match object_ref.kind {
                OperationalObjectKind::MarketSession => Some(self.market_session_ref()),
                _ => None,
            })
    }

    pub fn neighborhood(
        &self,
        kind: OperationalObjectKind,
        id: &str,
    ) -> Option<OperationalNeighborhood> {
        let navigation = self.navigation(kind, id)?.clone();
        let root = navigation.self_ref.clone().or_else(|| {
            self.resolve_object_ref(&OperationalObjectRef {
                id: id.into(),
                kind,
                endpoint: String::new(),
                label: None,
            })
        })?;

        Some(OperationalNeighborhood {
            root,
            relationships: navigation.relationships,
            graph_ref: navigation.graph,
            history_refs: navigation.history,
        })
    }
}

pub(crate) fn case_self_ref(market: LiveMarket, item: &CaseContract) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Case,
        endpoint: format!("/api/ontology/{}/cases/{}", market_slug(market), item.id.0),
        label: Some(item.title.clone()),
    }
}

pub(crate) fn recommendation_self_ref(
    market: LiveMarket,
    item: &RecommendationContract,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Recommendation,
        endpoint: format!(
            "/api/ontology/{}/recommendations/{}",
            market_slug(market),
            item.id.0
        ),
        label: item.recommendation.title.clone(),
    }
}

pub(crate) fn macro_event_self_ref(
    market: LiveMarket,
    item: &MacroEventContract,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::MacroEvent,
        endpoint: format!(
            "/api/ontology/{}/macro-events/{}",
            market_slug(market),
            item.id.0
        ),
        label: Some(item.event.headline.clone()),
    }
}

pub(crate) fn workflow_self_ref(
    market: LiveMarket,
    item: &WorkflowContract,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Workflow,
        endpoint: format!(
            "/api/ontology/{}/workflows/{}",
            market_slug(market),
            item.id.0
        ),
        label: Some(item.stage.clone()),
    }
}

pub(crate) fn collect_case_history_refs(item: &CaseContract) -> Vec<OperationalHistoryRef> {
    item.history_refs
        .workflow
        .clone()
        .into_iter()
        .chain(item.history_refs.reasoning.clone())
        .chain(item.history_refs.outcomes.clone())
        .collect()
}

pub(crate) fn collect_recommendation_history_refs(
    item: &RecommendationContract,
) -> Vec<OperationalHistoryRef> {
    item.history_refs
        .journal
        .clone()
        .into_iter()
        .chain(item.history_refs.workflow.clone())
        .chain(item.history_refs.outcomes.clone())
        .collect()
}

pub(crate) fn collect_workflow_history_refs(item: &WorkflowContract) -> Vec<OperationalHistoryRef> {
    item.history_refs.events.clone().into_iter().collect()
}

pub fn operational_snapshot_path(market: CaseMarket) -> String {
    match market {
        CaseMarket::Hk => std::env::var("EDEN_HK_OPERATIONAL_SNAPSHOT_PATH")
            .or_else(|_| std::env::var("EDEN_OPERATIONAL_SNAPSHOT_PATH"))
            .unwrap_or_else(|_| "data/operational_snapshot.json".into()),
        CaseMarket::Us => std::env::var("EDEN_US_OPERATIONAL_SNAPSHOT_PATH")
            .unwrap_or_else(|_| "data/us_operational_snapshot.json".into()),
    }
}

pub async fn load_operational_snapshot(
    market: CaseMarket,
) -> Result<OperationalSnapshot, Box<dyn std::error::Error>> {
    let path = operational_snapshot_path(market);
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}
