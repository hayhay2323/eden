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
use crate::ontology::CaseReasoningProfile;
use crate::pipeline::learning_loop::ReasoningLearningFeedback;

pub(super) struct SnapshotCaseLookups<'a> {
    pub(super) chains: HashMap<&'a str, &'a LiveBackwardChain>,
    pub(super) pressures: HashMap<&'a str, &'a LivePressure>,
    pub(super) signals: HashMap<&'a str, &'a LiveSignal>,
    pub(super) causals: HashMap<&'a str, &'a LiveCausalLeader>,
    pub(super) tracks: HashMap<&'a str, &'a LiveHypothesisTrack>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
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
    pub review_required_by_lens: Vec<CaseLensStat>,
    pub human_override_by_lens: Vec<CaseLensStat>,
    pub lens_regime_hit_rates: Vec<CaseLensRegimeHitRateStat>,
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
    pub learning_feedback: ReasoningLearningFeedback,
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
    pub key_evidence: Vec<CaseEvidence>,
    pub invalidation_rules: Vec<String>,
    pub reasoning_profile: CaseReasoningProfile,
    pub updated_at: String,
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
