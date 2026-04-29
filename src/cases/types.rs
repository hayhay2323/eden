use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::action::workflow::{
    ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode,
};
use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure,
    LiveScorecard, LiveSignal, LiveStressSnapshot, LiveTacticalCase,
};
use crate::ontology::{
    ArchetypeProjection, CaseReasoningProfile, CaseSignature, ExpectationBinding,
    ExpectationViolation, IntentHypothesis, IntentOpportunityWindow,
};
use crate::pipeline::learning_loop::ReasoningLearningFeedback;

pub(super) struct SnapshotCaseLookups<'a> {
    pub(super) chains: HashMap<&'a str, &'a LiveBackwardChain>,
    pub(super) pressures: HashMap<&'a str, &'a LivePressure>,
    pub(super) signals: HashMap<&'a str, &'a LiveSignal>,
    pub(super) causals: HashMap<&'a str, &'a LiveCausalLeader>,
    pub(super) tracks: HashMap<&'a str, &'a LiveHypothesisTrack>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseMarket {
    Hk,
    Us,
}

impl CaseMarket {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "hk" => Some(Self::Hk),
            "us" => Some(Self::Us),
            _ => None,
        }
    }

    pub fn snapshot_path(self) -> (&'static str, &'static str) {
        match self {
            Self::Hk => ("EDEN_LIVE_SNAPSHOT_PATH", "data/live_snapshot.json"),
            Self::Us => ("EDEN_US_LIVE_SNAPSHOT_PATH", "data/us_live_snapshot.json"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseListResponse {
    pub context: CaseMarketContext,
    pub cases: Vec<CaseSummary>,
    pub governance_buckets: CaseGovernanceBuckets,
    pub governance_reason_buckets: CaseGovernanceReasonBuckets,
    pub primary_lens_buckets: CasePrimaryLensBuckets,
    pub queue_pin_buckets: CaseQueuePinBuckets,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseBriefingResponse {
    pub context: CaseMarketContext,
    pub metrics: CaseBriefingMetrics,
    pub priority_cases: Vec<CaseSummary>,
    pub review_cases: Vec<CaseSummary>,
    pub watch_cases: Vec<CaseSummary>,
    pub governance_buckets: CaseGovernanceBuckets,
    pub governance_reason_buckets: CaseGovernanceReasonBuckets,
    pub primary_lens_buckets: CasePrimaryLensBuckets,
    pub queue_pin_buckets: CaseQueuePinBuckets,
    pub watchouts: CaseBriefingWatchouts,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseBriefingMetrics {
    pub actionable: usize,
    pub needs_review: usize,
    pub watchlist: usize,
    pub active_positions: usize,
    pub manual_only: usize,
    pub review_required: usize,
    pub auto_eligible: usize,
    pub queue_pinned: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseBriefingWatchouts {
    pub market_events: Vec<String>,
    pub cross_market: Vec<String>,
    pub anomalies: Vec<String>,
    pub dominant_intents: Vec<String>,
    pub dominant_opportunities: Vec<String>,
    pub learned_archetypes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewResponse {
    pub context: CaseMarketContext,
    pub metrics: CaseReviewMetrics,
    pub buckets: CaseReviewBuckets,
    pub governance_buckets: CaseGovernanceBuckets,
    pub governance_reason_buckets: CaseGovernanceReasonBuckets,
    pub primary_lens_buckets: CasePrimaryLensBuckets,
    pub queue_pin_buckets: CaseQueuePinBuckets,
    pub analytics: CaseReviewAnalytics,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewMetrics {
    pub in_flight: usize,
    pub under_review: usize,
    pub at_risk: usize,
    pub high_conviction: usize,
    pub manual_only: usize,
    pub review_required: usize,
    pub auto_eligible: usize,
    pub queue_pinned: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewBuckets {
    pub in_flight: Vec<CaseSummary>,
    pub under_review: Vec<CaseSummary>,
    pub at_risk: Vec<CaseSummary>,
    pub high_conviction: Vec<CaseSummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseGovernanceBuckets {
    pub manual_only: Vec<CaseSummary>,
    pub review_required: Vec<CaseSummary>,
    pub auto_eligible: Vec<CaseSummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseGovernanceReasonBuckets {
    pub buckets: BTreeMap<ActionGovernanceReasonCode, Vec<CaseSummary>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CasePrimaryLensBuckets {
    pub buckets: BTreeMap<String, Vec<CaseSummary>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseQueuePinBuckets {
    pub pinned: Vec<CaseSummary>,
    pub unpinned: Vec<CaseSummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseReviewAnalytics {
    pub mechanism_stats: Vec<CaseMechanismStat>,
    pub intent_stats: Vec<CaseIntentStat>,
    pub intent_state_stats: Vec<CaseIntentStateStat>,
    pub intent_exit_signal_stats: Vec<CaseIntentExitSignalStat>,
    pub intent_opportunity_stats: Vec<CaseIntentOpportunityStat>,
    pub intent_adjustments: Vec<CaseIntentAdjustmentStat>,
    pub review_required_by_lens: Vec<CaseLensStat>,
    pub human_override_by_lens: Vec<CaseLensStat>,
    pub lens_regime_hit_rates: Vec<CaseLensRegimeHitRateStat>,
    pub archetype_stats: Vec<CaseArchetypeStat>,
    pub discovered_archetype_catalog: Vec<CaseArchetypeCatalogStat>,
    pub signature_stats: Vec<CaseSignatureStat>,
    pub expectation_violation_stats: Vec<CaseExpectationViolationStat>,
    pub intelligence_signals: CaseIntelligenceSignals,
    pub memory_impact: Vec<CaseMemoryImpactStat>,
    pub violation_predictiveness: Vec<CaseViolationPredictivenessStat>,
    pub reviewer_corrections: Vec<CaseReviewerCorrectionStat>,
    pub mechanism_drift: Vec<CaseMechanismDriftPoint>,
    pub mechanism_transition_breakdown: Vec<CaseMechanismTransitionStat>,
    pub transition_by_sector: Vec<CaseMechanismTransitionSliceStat>,
    pub transition_by_regime: Vec<CaseMechanismTransitionSliceStat>,
    pub transition_by_reviewer: Vec<CaseMechanismTransitionSliceStat>,
    pub recent_mechanism_transitions: Vec<CaseMechanismTransitionDigest>,
    pub reviewer_doctrine: Vec<CaseReviewerDoctrineStat>,
    pub human_review_reasons: Vec<CaseHumanReviewReasonStat>,
    pub invalidation_patterns: Vec<CaseInvalidationPatternStat>,
    pub review_reason_feedback: Vec<CaseReviewReasonFeedbackStat>,
    pub review_reason_family_feedback: Vec<CaseReviewReasonFeedbackStat>,
    pub learning_feedback: ReasoningLearningFeedback,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseIntelligenceSignals {
    pub memory_impacted_cases: usize,
    pub reprioritized_cases: usize,
    pub stable_archetypes: usize,
    pub predictive_violation_kinds: usize,
    pub emergent_cases: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMemoryImpactStat {
    pub setup_id: String,
    pub symbol: String,
    pub baseline_rank: usize,
    pub adjusted_rank: usize,
    pub baseline_structure_priority: i32,
    pub adjusted_structure_priority: i32,
    pub confidence_delta: rust_decimal::Decimal,
    pub edge_delta: rust_decimal::Decimal,
    pub archetypes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseViolationPredictivenessStat {
    pub kind: String,
    pub samples: usize,
    pub hits: usize,
    pub hit_rate: rust_decimal::Decimal,
    pub mean_net_return: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseLensStat {
    pub lens: String,
    pub cases: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseLensRegimeHitRateStat {
    pub lens: String,
    pub market_regime: String,
    pub total: usize,
    pub hits: usize,
    pub hit_rate: rust_decimal::Decimal,
    pub mean_net_return: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismStat {
    pub mechanism: String,
    pub cases: usize,
    pub under_review: usize,
    pub at_risk: usize,
    pub high_conviction: usize,
    pub avg_score: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseIntentStat {
    pub intent: String,
    pub cases: usize,
    pub buy_cases: usize,
    pub sell_cases: usize,
    pub mean_confidence: rust_decimal::Decimal,
    pub mean_strength: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseIntentAdjustmentStat {
    pub intent: String,
    pub delta: rust_decimal::Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseIntentStateStat {
    pub state: String,
    pub cases: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseIntentExitSignalStat {
    pub kind: String,
    pub cases: usize,
    pub mean_confidence: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseIntentOpportunityStat {
    pub horizon: String,
    pub bias: String,
    pub cases: usize,
    pub mean_confidence: rust_decimal::Decimal,
    pub mean_alignment: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseArchetypeStat {
    pub archetype: String,
    pub cases: usize,
    pub mean_affinity: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseArchetypeCatalogStat {
    pub archetype: String,
    pub label: String,
    pub samples: u64,
    pub hits: u64,
    pub hit_rate: rust_decimal::Decimal,
    pub mean_net_return: rust_decimal::Decimal,
    pub mean_affinity: rust_decimal::Decimal,
    pub topology: Option<String>,
    pub temporal_shape: Option<String>,
    pub conflict_shape: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseSignatureStat {
    pub topology: String,
    pub temporal_shape: String,
    pub conflict_shape: String,
    pub cases: usize,
    pub mean_novelty: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseExpectationViolationStat {
    pub kind: String,
    pub cases: usize,
    pub mean_magnitude: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewerCorrectionStat {
    pub reviewer: String,
    pub updates: usize,
    pub review_stage_updates: usize,
    pub reflexive_corrections: usize,
    pub narrative_failures: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismDriftPoint {
    pub window_label: String,
    pub top_mechanism: Option<String>,
    pub top_cases: usize,
    pub avg_score: rust_decimal::Decimal,
    pub dominant_factor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewerDoctrineStat {
    pub reviewer: String,
    pub updates: usize,
    pub reflexive_corrections: usize,
    pub narrative_failures: usize,
    pub dominant_mechanism: Option<String>,
    pub dominant_rejection_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseInvalidationPatternStat {
    pub label: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseHumanReviewReasonStat {
    pub reason: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewReasonFeedbackStat {
    pub review_reason_code: String,
    pub blocked_count: usize,
    pub resolved_count: usize,
    pub post_block_hits: usize,
    pub post_block_hit_rate: rust_decimal::Decimal,
    pub invalidation_rate: rust_decimal::Decimal,
    pub mean_net_return: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismTransitionStat {
    pub classification: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismTransitionSliceStat {
    pub key: String,
    pub classification: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismTransitionDigest {
    pub setup_id: String,
    pub symbol: String,
    pub title: String,
    pub sector: Option<String>,
    pub regime: Option<String>,
    pub reviewer: Option<String>,
    pub from_mechanism: Option<String>,
    pub to_mechanism: Option<String>,
    pub classification: String,
    pub confidence: rust_decimal::Decimal,
    pub summary: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMarketContext {
    pub market: LiveMarket,
    pub tick: u64,
    pub timestamp: String,
    pub stock_count: usize,
    pub edge_count: usize,
    pub hypothesis_count: usize,
    pub observation_count: usize,
    pub active_positions: usize,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub scorecard: LiveScorecard,
    pub events: Vec<LiveEvent>,
    pub cross_market_signals: Vec<LiveCrossMarketSignal>,
    pub cross_market_anomalies: Vec<LiveCrossMarketAnomaly>,
    pub lineage: Vec<LiveLineageMetric>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseSummary {
    pub case_id: String,
    pub setup_id: String,
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub queue_pin: Option<String>,
    pub workflow_actor: Option<String>,
    pub workflow_note: Option<String>,
    pub symbol: String,
    pub title: String,
    pub sector: Option<String>,
    pub market: LiveMarket,
    pub recommended_action: String,
    pub workflow_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance: Option<ActionGovernanceContract>,
    pub governance_bucket: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    pub market_regime_bias: String,
    pub market_regime_confidence: rust_decimal::Decimal,
    pub market_breadth_delta: rust_decimal::Decimal,
    pub market_average_return: rust_decimal::Decimal,
    pub market_directional_consensus: Option<rust_decimal::Decimal>,
    pub confidence: rust_decimal::Decimal,
    pub confidence_gap: rust_decimal::Decimal,
    pub heuristic_edge: rust_decimal::Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_family: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reason_subreasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_state: Option<String>,
    /// Tick on which this setup_id first emitted `action=enter`. Derived from
    /// `recent_transitions`. Parallel to `LiveTacticalCase.first_enter_tick`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_enter_tick: Option<u64>,
    /// `current_tick - first_enter_tick` at the moment this summary was built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_since_first_enter: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_since_first_seen: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_position_in_range: Option<rust_decimal::Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state_confidence: Option<rust_decimal::Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_score: Option<rust_decimal::Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_velocity_5t: Option<rust_decimal::Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_fraction_velocity_5t: Option<rust_decimal::Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_confidence: Option<rust_decimal::Decimal>,
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
    pub why_now: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_lens: Option<String>,
    pub primary_driver: Option<String>,
    pub family_label: Option<String>,
    pub counter_label: Option<String>,
    pub hypothesis_status: Option<String>,
    pub current_leader: Option<String>,
    pub flip_count: usize,
    pub leader_streak: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_opportunities: Vec<IntentOpportunityWindow>,
    #[serde(default)]
    pub expectation_binding_count: usize,
    #[serde(default)]
    pub expectation_violation_count: usize,
    pub key_evidence: Vec<CaseEvidence>,
    pub invalidation_rules: Vec<String>,
    pub reasoning_profile: CaseReasoningProfile,
    pub updated_at: String,
    /// Resolution summary projected from the latest persisted `case_resolution`
    /// record. `None` while the case is still open and no horizons have
    /// settled. `finality=Provisional` indicates progressive settlement (some
    /// horizons still pending); `Final` means the resolution is locked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_resolution: Option<crate::ontology::resolution::CaseResolution>,
    /// Horizon-level breakdown, e.g. `"primary Confirmed, 2/3 settled"`.
    /// Populated when horizon_evaluation records exist for the setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_breakdown: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseEvidence {
    pub description: String,
    pub weight: rust_decimal::Decimal,
    pub direction: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseDetail {
    pub summary: CaseSummary,
    pub tactical_case: LiveTacticalCase,
    pub backward_chain: Option<LiveBackwardChain>,
    pub pressure: Option<LivePressure>,
    pub signal: Option<LiveSignal>,
    pub causal_leader: Option<LiveCausalLeader>,
    pub hypothesis_track: Option<LiveHypothesisTrack>,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub lineage: Vec<LiveLineageMetric>,
    pub related_events: Vec<LiveEvent>,
    pub cross_market_signals: Vec<LiveCrossMarketSignal>,
    pub cross_market_anomalies: Vec<LiveCrossMarketAnomaly>,
    pub risk_notes: Vec<String>,
    pub lineage_context: CaseLineageContext,
    pub workflow: Option<CaseWorkflowState>,
    pub workflow_history: Vec<CaseWorkflowEvent>,
    pub reasoning_history: Vec<CaseReasoningAssessmentSnapshot>,
    pub mechanism_story: CaseMechanismStory,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseLineageContext {
    pub based_on: Vec<String>,
    pub blocked_by: Vec<String>,
    pub promoted_by: Vec<String>,
    pub falsified_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseWorkflowState {
    pub workflow_id: String,
    pub stage: String,
    pub execution_policy: ActionExecutionPolicy,
    pub governance: ActionGovernanceContract,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub queue_pin: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseWorkflowEvent {
    pub workflow_id: String,
    pub stage: String,
    pub from_stage: Option<String>,
    pub operator_decision_kind: Option<String>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub queue_pin: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReasoningAssessmentSnapshot {
    pub assessment_id: String,
    pub setup_id: String,
    pub market: String,
    pub symbol: String,
    pub title: String,
    pub family_label: Option<String>,
    pub sector: Option<String>,
    pub recommended_action: String,
    pub source: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub workflow_state: String,
    pub market_regime_bias: Option<String>,
    pub market_regime_confidence: Option<rust_decimal::Decimal>,
    pub market_breadth_delta: Option<rust_decimal::Decimal>,
    pub market_average_return: Option<rust_decimal::Decimal>,
    pub market_directional_consensus: Option<rust_decimal::Decimal>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub actor: Option<String>,
    pub note: Option<String>,
    pub primary_mechanism_kind: Option<String>,
    pub primary_mechanism_score: Option<rust_decimal::Decimal>,
    pub law_kinds: Vec<String>,
    pub predicate_kinds: Vec<String>,
    pub composite_state_kinds: Vec<String>,
    pub competing_mechanism_kinds: Vec<String>,
    pub invalidation_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_opportunities: Vec<IntentOpportunityWindow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_violations: Vec<ExpectationViolation>,
    pub reasoning_profile: CaseReasoningProfile,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseMechanismStory {
    pub current_mechanism: Option<String>,
    pub status: String,
    pub summary: String,
    pub latest_transition: Option<CaseMechanismTransition>,
    pub recent_transitions: Vec<CaseMechanismTransition>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseMechanismTransition {
    #[serde(with = "rfc3339")]
    pub from_recorded_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub to_recorded_at: OffsetDateTime,
    pub from_mechanism: Option<String>,
    pub to_mechanism: Option<String>,
    pub classification: String,
    pub confidence: rust_decimal::Decimal,
    pub summary: String,
    pub regime_change: Option<String>,
    pub regime_evidence: Vec<String>,
    pub decay_evidence: Vec<String>,
    pub emerging_evidence: Vec<String>,
    pub review_evidence: Vec<String>,
}
