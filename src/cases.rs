use std::collections::{HashMap, HashSet};

#[cfg(feature = "persistence")]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure,
    LiveScorecard, LiveSignal, LiveSnapshot, LiveStressSnapshot, LiveTacticalCase,
};
use crate::ontology::CaseReasoningProfile;
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
use crate::pipeline::learning_loop::ReasoningLearningFeedback;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::{
    apply_learning_feedback, derive_learning_feedback,
    derive_outcome_learning_context_from_case_outcomes,
    derive_outcome_learning_context_from_hk_rows, derive_outcome_learning_context_from_us_rows,
};
use crate::pipeline::mechanism_inference::build_reasoning_profile as infer_reasoning_profile;
use crate::pipeline::predicate_engine::{
    augment_predicates_with_workflow, derive_atomic_predicates, derive_human_review_context,
    PredicateInputs,
};

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
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseBriefingResponse {
    pub context: CaseMarketContext,
    pub metrics: CaseBriefingMetrics,
    pub priority_cases: Vec<CaseSummary>,
    pub review_cases: Vec<CaseSummary>,
    pub watch_cases: Vec<CaseSummary>,
    pub watchouts: CaseBriefingWatchouts,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseBriefingMetrics {
    pub actionable: usize,
    pub needs_review: usize,
    pub watchlist: usize,
    pub active_positions: usize,
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
    pub analytics: CaseReviewAnalytics,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewMetrics {
    pub in_flight: usize,
    pub under_review: usize,
    pub at_risk: usize,
    pub high_conviction: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReviewBuckets {
    pub in_flight: Vec<CaseSummary>,
    pub under_review: Vec<CaseSummary>,
    pub at_risk: Vec<CaseSummary>,
    pub high_conviction: Vec<CaseSummary>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaseReviewAnalytics {
    pub mechanism_stats: Vec<CaseMechanismStat>,
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
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub workflow_actor: Option<String>,
    pub workflow_note: Option<String>,
    pub symbol: String,
    pub title: String,
    pub sector: Option<String>,
    pub market: LiveMarket,
    pub recommended_action: String,
    pub workflow_state: String,
    pub market_regime_bias: String,
    pub market_regime_confidence: rust_decimal::Decimal,
    pub market_breadth_delta: rust_decimal::Decimal,
    pub market_average_return: rust_decimal::Decimal,
    pub market_directional_consensus: Option<rust_decimal::Decimal>,
    pub confidence: rust_decimal::Decimal,
    pub confidence_gap: rust_decimal::Decimal,
    pub heuristic_edge: rust_decimal::Decimal,
    pub why_now: String,
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
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseWorkflowEvent {
    pub workflow_id: String,
    pub stage: String,
    pub from_stage: Option<String>,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaseReasoningAssessmentSnapshot {
    pub assessment_id: String,
    pub setup_id: String,
    pub market: String,
    pub symbol: String,
    pub title: String,
    pub sector: Option<String>,
    pub recommended_action: String,
    pub source: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub workflow_state: String,
    pub market_regime_bias: Option<String>,
    pub market_regime_confidence: Option<String>,
    pub market_breadth_delta: Option<String>,
    pub market_average_return: Option<String>,
    pub market_directional_consensus: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub actor: Option<String>,
    pub note: Option<String>,
    pub primary_mechanism_kind: Option<String>,
    pub primary_mechanism_score: Option<String>,
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

pub async fn load_snapshot(market: CaseMarket) -> Result<LiveSnapshot, Box<dyn std::error::Error>> {
    let (env_var, default_path) = market.snapshot_path();
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub fn build_case_list(snapshot: &LiveSnapshot) -> CaseListResponse {
    CaseListResponse {
        context: CaseMarketContext {
            market: snapshot.market,
            tick: snapshot.tick,
            timestamp: snapshot.timestamp.clone(),
            stock_count: snapshot.stock_count,
            edge_count: snapshot.edge_count,
            hypothesis_count: snapshot.hypothesis_count,
            observation_count: snapshot.observation_count,
            active_positions: snapshot.active_positions,
            market_regime: snapshot.market_regime.clone(),
            stress: snapshot.stress.clone(),
            scorecard: snapshot.scorecard.clone(),
            events: snapshot.events.clone(),
            cross_market_signals: snapshot.cross_market_signals.clone(),
            cross_market_anomalies: snapshot.cross_market_anomalies.clone(),
            lineage: snapshot.lineage.clone(),
        },
        cases: build_case_summaries(snapshot),
    }
}

pub fn filter_case_list_by_actor(response: &mut CaseListResponse, actor: Option<&str>) {
    let Some(actor) = actor.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    let normalized = actor.to_lowercase();
    response.cases.retain(|item| {
        item.owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase() == normalized)
            .or_else(|| {
                item.workflow_actor
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_lowercase() == normalized)
            })
            .unwrap_or(false)
    });
}

pub fn filter_case_list_by_owner(response: &mut CaseListResponse, owner: Option<&str>) {
    let Some(owner) = owner.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    let normalized = owner.to_lowercase();
    response.cases.retain(|item| {
        item.owner
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase() == normalized)
            .unwrap_or(false)
    });
}

pub fn filter_case_list_by_reviewer(response: &mut CaseListResponse, reviewer: Option<&str>) {
    let Some(reviewer) = reviewer.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    let normalized = reviewer.to_lowercase();
    response.cases.retain(|item| {
        item.reviewer
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_lowercase() == normalized)
            .unwrap_or(false)
    });
}

pub fn build_case_briefing(list: &CaseListResponse) -> CaseBriefingResponse {
    let actionable = list
        .cases
        .iter()
        .filter(|item| item.recommended_action == "enter")
        .cloned()
        .collect::<Vec<_>>();
    let review_cases = list
        .cases
        .iter()
        .filter(|item| item.workflow_state == "review")
        .cloned()
        .collect::<Vec<_>>();
    let watch_cases = list
        .cases
        .iter()
        .filter(|item| item.recommended_action != "enter")
        .cloned()
        .collect::<Vec<_>>();

    CaseBriefingResponse {
        context: list.context.clone(),
        metrics: CaseBriefingMetrics {
            actionable: actionable.len(),
            needs_review: review_cases.len(),
            watchlist: watch_cases.len(),
            active_positions: list.context.active_positions,
        },
        priority_cases: actionable.into_iter().take(6).collect(),
        review_cases: review_cases.into_iter().take(5).collect(),
        watch_cases: watch_cases.into_iter().take(6).collect(),
        watchouts: CaseBriefingWatchouts {
            market_events: list
                .context
                .events
                .iter()
                .take(6)
                .map(|item| item.summary.clone())
                .collect(),
            cross_market: list
                .context
                .cross_market_signals
                .iter()
                .take(6)
                .map(|item| format!("{} ← {}", item.us_symbol, item.hk_symbol))
                .collect(),
            anomalies: list
                .context
                .cross_market_anomalies
                .iter()
                .take(4)
                .map(|item| format!("{} / {} 方向矛盾", item.us_symbol, item.hk_symbol))
                .collect(),
        },
    }
}

pub fn build_case_review(list: &CaseListResponse) -> CaseReviewResponse {
    let in_flight = list
        .cases
        .iter()
        .filter(|item| {
            matches!(
                item.workflow_state.as_str(),
                "confirm" | "execute" | "monitor"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let under_review = list
        .cases
        .iter()
        .filter(|item| item.workflow_state == "review")
        .cloned()
        .collect::<Vec<_>>();
    let at_risk = list
        .cases
        .iter()
        .filter(|item| {
            matches!(
                item.hypothesis_status.as_deref(),
                Some("weakening") | Some("invalidated")
            ) || !item.invalidation_rules.is_empty()
        })
        .cloned()
        .collect::<Vec<_>>();
    let high_conviction = list
        .cases
        .iter()
        .filter(|item| item.recommended_action == "enter" && item.workflow_state != "review")
        .cloned()
        .collect::<Vec<_>>();

    CaseReviewResponse {
        context: list.context.clone(),
        metrics: CaseReviewMetrics {
            in_flight: in_flight.len(),
            under_review: under_review.len(),
            at_risk: at_risk.len(),
            high_conviction: high_conviction.len(),
        },
        buckets: CaseReviewBuckets {
            in_flight,
            under_review,
            at_risk,
            high_conviction,
        },
        analytics: build_case_review_analytics(&list.cases),
    }
}

pub fn build_case_summaries(snapshot: &LiveSnapshot) -> Vec<CaseSummary> {
    let chains = snapshot
        .backward_chains
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let pressures = snapshot
        .pressures
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let signals = snapshot
        .top_signals
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let causals = snapshot
        .causal_leaders
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let tracks = snapshot
        .hypothesis_tracks
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();

    let mut cases = snapshot
        .tactical_cases
        .iter()
        .map(|tactical_case| {
            let symbol = tactical_case.symbol.as_str();
            let chain = chains.get(symbol).copied();
            let pressure = pressures.get(symbol).copied();
            let causal = causals.get(symbol).copied();
            let track = tracks.get(symbol).copied();
            let signal = signals.get(symbol).copied();
            let invalidation_rules =
                default_invalidation_rules(tactical_case, track, causal, pressure);
            let reasoning_profile = build_summary_reasoning_profile(
                snapshot,
                tactical_case,
                chain,
                pressure,
                signal,
                causal,
                track,
                default_workflow_state(&tactical_case.action),
                None,
                &invalidation_rules,
            );

            CaseSummary {
                case_id: tactical_case.setup_id.clone(),
                setup_id: tactical_case.setup_id.clone(),
                workflow_id: None,
                owner: None,
                reviewer: None,
                workflow_actor: None,
                workflow_note: None,
                symbol: tactical_case.symbol.clone(),
                title: tactical_case.title.clone(),
                sector: signal
                    .and_then(|item| item.sector.clone())
                    .or_else(|| pressure.and_then(|item| item.sector.clone())),
                market: snapshot.market,
                recommended_action: tactical_case.action.clone(),
                workflow_state: default_workflow_state(&tactical_case.action).to_string(),
                market_regime_bias: snapshot.market_regime.bias.clone(),
                market_regime_confidence: snapshot.market_regime.confidence,
                market_breadth_delta: snapshot.market_regime.breadth_up
                    - snapshot.market_regime.breadth_down,
                market_average_return: snapshot.market_regime.average_return,
                market_directional_consensus: snapshot.market_regime.directional_consensus,
                confidence: tactical_case.confidence,
                confidence_gap: tactical_case.confidence_gap,
                heuristic_edge: tactical_case.heuristic_edge,
                why_now: derive_why_now(tactical_case, chain, pressure, causal, track, signal),
                primary_driver: chain.map(|item| item.primary_driver.clone()),
                family_label: tactical_case.family_label.clone(),
                counter_label: tactical_case.counter_label.clone(),
                hypothesis_status: track.map(|item| item.status.clone()),
                current_leader: causal.map(|item| item.current_leader.clone()),
                flip_count: causal.map(|item| item.flips).unwrap_or_default(),
                leader_streak: causal.map(|item| item.leader_streak),
                key_evidence: chain
                    .map(|item| {
                        item.evidence
                            .iter()
                            .take(3)
                            .map(|evidence| CaseEvidence {
                                description: evidence.description.clone(),
                                weight: evidence.weight,
                                direction: evidence.direction,
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                invalidation_rules,
                reasoning_profile,
                updated_at: snapshot.timestamp.clone(),
            }
        })
        .collect::<Vec<_>>();

    cases.sort_by(|left, right| {
        case_priority(left)
            .cmp(&case_priority(right))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    cases
}

pub fn build_case_detail(snapshot: &LiveSnapshot, setup_id: &str) -> Option<CaseDetail> {
    let tactical_case = snapshot
        .tactical_cases
        .iter()
        .find(|item| item.setup_id == setup_id)?
        .clone();
    let summary = build_case_summaries(snapshot)
        .into_iter()
        .find(|item| item.setup_id == setup_id)?;

    let backward_chain = snapshot
        .backward_chains
        .iter()
        .find(|item| item.symbol == tactical_case.symbol)
        .cloned();
    let pressure = snapshot
        .pressures
        .iter()
        .find(|item| item.symbol == tactical_case.symbol)
        .cloned();
    let signal = snapshot
        .top_signals
        .iter()
        .find(|item| item.symbol == tactical_case.symbol)
        .cloned();
    let causal_leader = snapshot
        .causal_leaders
        .iter()
        .find(|item| item.symbol == tactical_case.symbol)
        .cloned();
    let hypothesis_track = snapshot
        .hypothesis_tracks
        .iter()
        .find(|item| item.symbol == tactical_case.symbol)
        .cloned();

    let cross_market_signals = snapshot
        .cross_market_signals
        .iter()
        .filter(|item| match snapshot.market {
            LiveMarket::Us => item.us_symbol == tactical_case.symbol,
            LiveMarket::Hk => item.hk_symbol == tactical_case.symbol,
        })
        .cloned()
        .collect::<Vec<_>>();
    let cross_market_anomalies = snapshot
        .cross_market_anomalies
        .iter()
        .filter(|item| match snapshot.market {
            LiveMarket::Us => item.us_symbol == tactical_case.symbol,
            LiveMarket::Hk => item.hk_symbol == tactical_case.symbol,
        })
        .cloned()
        .collect::<Vec<_>>();

    Some(CaseDetail {
        summary,
        tactical_case,
        backward_chain,
        pressure,
        signal,
        causal_leader,
        hypothesis_track,
        market_regime: snapshot.market_regime.clone(),
        stress: snapshot.stress.clone(),
        lineage: snapshot.lineage.clone(),
        related_events: snapshot.events.iter().take(8).cloned().collect(),
        cross_market_signals,
        cross_market_anomalies,
        risk_notes: Vec::new(),
        lineage_context: CaseLineageContext::default(),
        workflow: None,
        workflow_history: Vec::new(),
        reasoning_history: Vec::new(),
        mechanism_story: CaseMechanismStory::default(),
    })
}

#[cfg(feature = "persistence")]
pub async fn enrich_case_summaries(
    store: &EdenStore,
    cases: &mut [CaseSummary],
) -> Result<(), Box<dyn std::error::Error>> {
    for case in cases.iter_mut() {
        let Some(setup) = store.tactical_setup_by_id(&case.setup_id).await? else {
            continue;
        };

        case.workflow_id = setup.workflow_id.clone();
        let invalidation_rules = record_invalidation_rules(&setup);
        if !invalidation_rules.is_empty() {
            case.invalidation_rules = invalidation_rules;
        }

        if let Some(workflow_id) = &setup.workflow_id {
            if let Some(workflow) = store.action_workflow_by_id(workflow_id).await? {
                case.workflow_state = workflow.current_stage.as_str().to_string();
                case.owner = workflow.owner.clone();
                case.reviewer = workflow.reviewer.clone();
                case.workflow_actor = workflow.actor.clone();
                case.workflow_note = workflow.note.clone();
            }
        }

        refresh_reasoning_profile(case);
    }

    if let Some(market) = cases.first().map(|item| item.market) {
        let market_key = match market {
            LiveMarket::Hk => "hk",
            LiveMarket::Us => "us",
        };
        let assessments = store
            .recent_case_reasoning_assessments_by_market(market_key, 240)
            .await?;
        let outcome_context = load_outcome_learning_context(store, market).await?;
        let feedback = derive_learning_feedback(&assessments, &outcome_context);
        for case in cases.iter_mut() {
            case.reasoning_profile = apply_learning_feedback(
                &case.reasoning_profile,
                &case.invalidation_rules,
                &feedback,
            );
        }
    }

    Ok(())
}

#[cfg(feature = "persistence")]
pub async fn enrich_case_detail(
    store: &EdenStore,
    detail: &mut CaseDetail,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(setup) = store.tactical_setup_by_id(&detail.summary.setup_id).await? else {
        return Ok(());
    };

    detail.summary.workflow_id = setup.workflow_id.clone();
    detail.risk_notes = setup.risk_notes.clone();
    detail.lineage_context = CaseLineageContext {
        based_on: setup.based_on.clone(),
        blocked_by: setup.blocked_by.clone(),
        promoted_by: setup.promoted_by.clone(),
        falsified_by: setup.falsified_by.clone(),
    };

    let invalidation_rules = record_invalidation_rules(&setup);
    if !invalidation_rules.is_empty() {
        detail.summary.invalidation_rules = invalidation_rules;
    }

    if let Some(workflow_id) = &setup.workflow_id {
        if let Some(workflow) = store.action_workflow_by_id(workflow_id).await? {
            detail.summary.workflow_state = workflow.current_stage.as_str().to_string();
            detail.workflow = Some(CaseWorkflowState {
                workflow_id: workflow.workflow_id.clone(),
                stage: workflow.current_stage.as_str().to_string(),
                timestamp: workflow.recorded_at,
                actor: workflow.actor.clone(),
                owner: workflow.owner.clone(),
                reviewer: workflow.reviewer.clone(),
                note: workflow.note.clone(),
            });
            detail.summary.owner = workflow.owner.clone();
            detail.summary.reviewer = workflow.reviewer.clone();
            detail.summary.workflow_actor = workflow.actor.clone();
            detail.summary.workflow_note = workflow.note.clone();
        }

        detail.workflow_history = store
            .action_workflow_events(workflow_id)
            .await?
            .into_iter()
            .map(|event| CaseWorkflowEvent {
                workflow_id: event.workflow_id,
                stage: event.to_stage.as_str().to_string(),
                from_stage: event.from_stage.map(|stage| stage.as_str().to_string()),
                timestamp: event.recorded_at,
                actor: event.actor,
                owner: event.owner,
                reviewer: event.reviewer,
                note: event.note,
            })
            .collect();
    }

    detail.reasoning_history = store
        .recent_case_reasoning_assessments(&detail.summary.setup_id, 12)
        .await?
        .into_iter()
        .map(CaseReasoningAssessmentSnapshot::from_record)
        .collect();

    refresh_reasoning_profile(&mut detail.summary);

    let market_key = match detail.summary.market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    };
    let assessments = store
        .recent_case_reasoning_assessments_by_market(market_key, 240)
        .await?;
    let outcome_context = load_outcome_learning_context(store, detail.summary.market).await?;
    let feedback = derive_learning_feedback(&assessments, &outcome_context);
    detail.summary.reasoning_profile = apply_learning_feedback(
        &detail.summary.reasoning_profile,
        &detail.summary.invalidation_rules,
        &feedback,
    );
    detail.mechanism_story = build_case_mechanism_story(detail);

    Ok(())
}

#[cfg(feature = "persistence")]
pub async fn enrich_case_review(
    store: &EdenStore,
    market: CaseMarket,
    review: &mut CaseReviewResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    let market_key = match market {
        CaseMarket::Hk => "hk",
        CaseMarket::Us => "us",
    };
    let assessments = store
        .recent_case_reasoning_assessments_by_market(market_key, 240)
        .await?;
    let outcome_context = load_outcome_learning_context(
        store,
        match market {
            CaseMarket::Hk => LiveMarket::Hk,
            CaseMarket::Us => LiveMarket::Us,
        },
    )
    .await?;
    review.analytics = build_case_review_analytics_with_assessments(
        &review_all_cases(review),
        &assessments,
        outcome_context,
    );
    Ok(())
}

#[cfg(feature = "persistence")]
pub fn workflow_record_payload(setup: &TacticalSetupRecord) -> serde_json::Value {
    serde_json::json!({
        "setup_id": setup.setup_id,
        "title": setup.title,
        "action": setup.action,
        "decision_lineage": {
            "based_on": setup.based_on,
            "blocked_by": setup.blocked_by,
            "promoted_by": setup.promoted_by,
            "falsified_by": setup.falsified_by,
        }
    })
}

fn case_priority(case: &CaseSummary) -> i32 {
    match (
        case.recommended_action.as_str(),
        case.workflow_state.as_str(),
    ) {
        ("enter", "suggest") => 0,
        ("enter", "confirm") => 1,
        (_, "review") => 2,
        ("enter", _) => 3,
        _ => 4,
    }
}

fn build_case_review_analytics(cases: &[CaseSummary]) -> CaseReviewAnalytics {
    CaseReviewAnalytics {
        mechanism_stats: build_mechanism_stats(cases),
        reviewer_corrections: Vec::new(),
        mechanism_drift: Vec::new(),
        mechanism_transition_breakdown: Vec::new(),
        transition_by_sector: Vec::new(),
        transition_by_regime: Vec::new(),
        transition_by_reviewer: Vec::new(),
        recent_mechanism_transitions: Vec::new(),
        reviewer_doctrine: Vec::new(),
        human_review_reasons: Vec::new(),
        invalidation_patterns: Vec::new(),
        learning_feedback: ReasoningLearningFeedback::default(),
    }
}

#[cfg(feature = "persistence")]
fn build_case_review_analytics_with_assessments(
    cases: &[CaseSummary],
    assessments: &[CaseReasoningAssessmentRecord],
    outcome_context: crate::pipeline::learning_loop::OutcomeLearningContext,
) -> CaseReviewAnalytics {
    let (
        mechanism_transition_breakdown,
        transition_by_sector,
        transition_by_regime,
        transition_by_reviewer,
        recent_mechanism_transitions,
    ) = build_mechanism_transition_analytics(cases, assessments);
    CaseReviewAnalytics {
        mechanism_stats: build_mechanism_stats(cases),
        reviewer_corrections: build_reviewer_correction_stats(assessments),
        mechanism_drift: build_mechanism_drift(assessments),
        mechanism_transition_breakdown,
        transition_by_sector,
        transition_by_regime,
        transition_by_reviewer,
        recent_mechanism_transitions,
        reviewer_doctrine: build_reviewer_doctrine(assessments),
        human_review_reasons: build_human_review_reason_stats(assessments),
        invalidation_patterns: build_invalidation_patterns(assessments),
        learning_feedback: derive_learning_feedback(assessments, &outcome_context),
    }
}

#[cfg(feature = "persistence")]
async fn load_outcome_learning_context(
    store: &EdenStore,
    market: LiveMarket,
) -> Result<crate::pipeline::learning_loop::OutcomeLearningContext, Box<dyn std::error::Error>> {
    let market_key = match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    };
    let case_outcomes: Vec<CaseRealizedOutcomeRecord> = store
        .recent_case_realized_outcomes_by_market(market_key, 120)
        .await?;
    if !case_outcomes.is_empty() {
        return Ok(derive_outcome_learning_context_from_case_outcomes(
            &case_outcomes,
            market_key,
        ));
    }

    match market {
        LiveMarket::Hk => {
            let rows: Vec<LineageMetricRowRecord> =
                store.recent_ranked_lineage_metric_rows(12, 5).await?;
            Ok(derive_outcome_learning_context_from_hk_rows(&rows))
        }
        LiveMarket::Us => {
            let rows: Vec<UsLineageMetricRowRecord> =
                store.recent_ranked_us_lineage_metric_rows(12, 5).await?;
            Ok(derive_outcome_learning_context_from_us_rows(&rows))
        }
    }
}

fn build_summary_reasoning_profile(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    chain: Option<&LiveBackwardChain>,
    pressure: Option<&LivePressure>,
    signal: Option<&LiveSignal>,
    causal: Option<&LiveCausalLeader>,
    track: Option<&LiveHypothesisTrack>,
    workflow_state: &str,
    workflow_note: Option<&str>,
    invalidation_rules: &[String],
) -> CaseReasoningProfile {
    let cross_market_signals = relevant_cross_market_signals(snapshot, &tactical_case.symbol);
    let cross_market_anomalies = relevant_cross_market_anomalies(snapshot, &tactical_case.symbol);
    let predicates = derive_atomic_predicates(&PredicateInputs {
        tactical_case,
        chain,
        pressure,
        signal,
        causal,
        track,
        stress: &snapshot.stress,
        market_regime: &snapshot.market_regime,
        all_signals: &snapshot.top_signals,
        all_pressures: &snapshot.pressures,
        events: &snapshot.events,
        cross_market_signals: &cross_market_signals,
        cross_market_anomalies: &cross_market_anomalies,
    });
    let human_review = derive_human_review_context(workflow_state, workflow_note);
    let predicates = augment_predicates_with_workflow(&predicates, workflow_state, workflow_note);
    infer_reasoning_profile(&predicates, invalidation_rules, human_review)
}

#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
fn refresh_reasoning_profile(case: &mut CaseSummary) {
    let human_review =
        derive_human_review_context(&case.workflow_state, case.workflow_note.as_deref());
    let predicates = augment_predicates_with_workflow(
        &case.reasoning_profile.predicates,
        &case.workflow_state,
        case.workflow_note.as_deref(),
    );
    case.reasoning_profile =
        infer_reasoning_profile(&predicates, &case.invalidation_rules, human_review);
}

fn build_mechanism_stats(cases: &[CaseSummary]) -> Vec<CaseMechanismStat> {
    let mut grouped: HashMap<String, Vec<&CaseSummary>> = HashMap::new();
    for case in cases {
        let Some(primary) = case.reasoning_profile.primary_mechanism.as_ref() else {
            continue;
        };
        grouped.entry(primary.label.clone()).or_default().push(case);
    }

    let mut stats = grouped
        .into_iter()
        .map(|(mechanism, items)| {
            let mut total_score = rust_decimal::Decimal::ZERO;
            let mut score_count = 0usize;
            let mut under_review = 0usize;
            let mut at_risk = 0usize;
            let mut high_conviction = 0usize;

            for case in &items {
                if let Some(primary) = case.reasoning_profile.primary_mechanism.as_ref() {
                    total_score += primary.score;
                    score_count += 1;
                }
                if case.workflow_state == "review" {
                    under_review += 1;
                }
                if !case.invalidation_rules.is_empty()
                    || matches!(
                        case.hypothesis_status.as_deref(),
                        Some("weakening") | Some("invalidated")
                    )
                {
                    at_risk += 1;
                }
                if case.recommended_action == "enter" && case.workflow_state != "review" {
                    high_conviction += 1;
                }
            }

            CaseMechanismStat {
                mechanism,
                cases: items.len(),
                under_review,
                at_risk,
                high_conviction,
                avg_score: if score_count == 0 {
                    rust_decimal::Decimal::ZERO
                } else {
                    total_score / rust_decimal::Decimal::from(score_count as i64)
                },
            }
        })
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.avg_score.cmp(&left.avg_score))
            .then_with(|| left.mechanism.cmp(&right.mechanism))
    });
    stats.truncate(6);
    stats
}

#[cfg(feature = "persistence")]
fn build_reviewer_correction_stats(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseReviewerCorrectionStat> {
    let mut grouped: HashMap<String, CaseReviewerCorrectionStat> = HashMap::new();

    for assessment in assessments
        .iter()
        .filter(|item| item.source == "workflow_update")
    {
        let Some(reviewer) = assessment
            .reviewer
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let stat =
            grouped
                .entry(reviewer.to_string())
                .or_insert_with(|| CaseReviewerCorrectionStat {
                    reviewer: reviewer.to_string(),
                    updates: 0,
                    review_stage_updates: 0,
                    reflexive_corrections: 0,
                    narrative_failures: 0,
                });

        stat.updates += 1;
        if assessment.workflow_state == "review" {
            stat.review_stage_updates += 1;
        }
        if assessment
            .composite_state_kinds
            .iter()
            .any(|item| item == "Reflexive Correction")
        {
            stat.reflexive_corrections += 1;
        }
        if assessment.primary_mechanism_kind.as_deref() == Some("Narrative Failure") {
            stat.narrative_failures += 1;
        }
    }

    let mut stats = grouped.into_values().collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .updates
            .cmp(&left.updates)
            .then_with(|| right.reflexive_corrections.cmp(&left.reflexive_corrections))
            .then_with(|| left.reviewer.cmp(&right.reviewer))
    });
    stats.truncate(6);
    stats
}

#[cfg(feature = "persistence")]
fn build_mechanism_drift(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseMechanismDriftPoint> {
    let mut windows: HashMap<String, Vec<&CaseReasoningAssessmentRecord>> = HashMap::new();

    for assessment in assessments.iter().filter(|item| item.source == "runtime") {
        let timestamp = assessment.recorded_at;
        let label = format!(
            "{:02}-{:02} {:02}:00",
            u8::from(timestamp.month()),
            timestamp.day(),
            timestamp.hour()
        );
        windows.entry(label).or_default().push(assessment);
    }

    let mut points = windows
        .into_iter()
        .map(|(window_label, records)| {
            let mut by_mechanism: HashMap<String, (usize, rust_decimal::Decimal, usize)> =
                HashMap::new();
            let mut by_factor: HashMap<String, usize> = HashMap::new();

            for record in records {
                let Some(kind) = record.primary_mechanism_kind.as_ref() else {
                    continue;
                };
                let entry =
                    by_mechanism
                        .entry(kind.clone())
                        .or_insert((0, rust_decimal::Decimal::ZERO, 0));
                entry.0 += 1;
                if let Some(score) = record
                    .primary_mechanism_score
                    .as_deref()
                    .and_then(|value| value.parse::<rust_decimal::Decimal>().ok())
                {
                    entry.1 += score;
                    entry.2 += 1;
                }
                if let Some(factor) = record
                    .reasoning_profile
                    .primary_mechanism
                    .as_ref()
                    .and_then(|mechanism| mechanism.factors.first())
                {
                    *by_factor.entry(factor.label.clone()).or_insert(0) += 1;
                }
            }

            let top = by_mechanism.into_iter().max_by(|left, right| {
                left.1
                     .0
                    .cmp(&right.1 .0)
                    .then_with(|| left.0.cmp(&right.0))
            });

            let dominant_factor = by_factor
                .into_iter()
                .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                .map(|(label, _)| label);

            let (top_mechanism, top_cases, avg_score) = match top {
                Some((mechanism, (cases, total_score, score_count))) => (
                    Some(mechanism),
                    cases,
                    if score_count == 0 {
                        rust_decimal::Decimal::ZERO
                    } else {
                        total_score / rust_decimal::Decimal::from(score_count as i64)
                    },
                ),
                None => (None, 0, rust_decimal::Decimal::ZERO),
            };

            CaseMechanismDriftPoint {
                window_label,
                top_mechanism,
                top_cases,
                avg_score,
                dominant_factor,
            }
        })
        .collect::<Vec<_>>();

    points.sort_by(|left, right| left.window_label.cmp(&right.window_label));
    if points.len() > 8 {
        points = points.split_off(points.len() - 8);
    }
    points
}

#[cfg(feature = "persistence")]
fn build_reviewer_doctrine(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseReviewerDoctrineStat> {
    let mut grouped: HashMap<
        String,
        (
            usize,
            usize,
            usize,
            HashMap<String, usize>,
            HashMap<String, usize>,
        ),
    > = HashMap::new();

    for assessment in assessments
        .iter()
        .filter(|item| item.source == "workflow_update")
    {
        let Some(reviewer) = assessment
            .reviewer
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let entry = grouped
            .entry(reviewer.to_string())
            .or_insert_with(|| (0, 0, 0, HashMap::new(), HashMap::new()));
        entry.0 += 1;
        if assessment
            .composite_state_kinds
            .iter()
            .any(|item| item == "Reflexive Correction")
        {
            entry.1 += 1;
        }
        if assessment.primary_mechanism_kind.as_deref() == Some("Narrative Failure") {
            entry.2 += 1;
        }
        if let Some(mechanism) = assessment.primary_mechanism_kind.as_ref() {
            *entry.3.entry(mechanism.clone()).or_insert(0) += 1;
        }
        if let Some(review) = assessment.reasoning_profile.human_review.as_ref() {
            for reason in &review.reasons {
                *entry.4.entry(reason.label.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(
                reviewer,
                (updates, reflexive_corrections, narrative_failures, mechanisms, reasons),
            )| {
                let dominant_mechanism = mechanisms
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(label, _)| label);
                let dominant_rejection_reason = reasons
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(label, _)| label);
                CaseReviewerDoctrineStat {
                    reviewer,
                    updates,
                    reflexive_corrections,
                    narrative_failures,
                    dominant_mechanism,
                    dominant_rejection_reason,
                }
            },
        )
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .updates
            .cmp(&left.updates)
            .then_with(|| right.reflexive_corrections.cmp(&left.reflexive_corrections))
            .then_with(|| left.reviewer.cmp(&right.reviewer))
    });
    stats.truncate(6);
    stats
}

#[cfg(feature = "persistence")]
fn build_human_review_reason_stats(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseHumanReviewReasonStat> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for assessment in assessments
        .iter()
        .filter(|item| item.source == "workflow_update")
    {
        let mut seen = HashSet::new();
        if let Some(review) = assessment.reasoning_profile.human_review.as_ref() {
            for reason in &review.reasons {
                if seen.insert(reason.label.clone()) {
                    *counts.entry(reason.label.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut items = counts
        .into_iter()
        .map(|(reason, count)| CaseHumanReviewReasonStat { reason, count })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.reason.cmp(&right.reason))
    });
    items.truncate(8);
    items
}

#[cfg(feature = "persistence")]
fn build_mechanism_transition_analytics(
    cases: &[CaseSummary],
    assessments: &[CaseReasoningAssessmentRecord],
) -> (
    Vec<CaseMechanismTransitionStat>,
    Vec<CaseMechanismTransitionSliceStat>,
    Vec<CaseMechanismTransitionSliceStat>,
    Vec<CaseMechanismTransitionSliceStat>,
    Vec<CaseMechanismTransitionDigest>,
) {
    let mut histories: HashMap<String, Vec<CaseReasoningAssessmentSnapshot>> = HashMap::new();
    for assessment in assessments {
        histories
            .entry(assessment.setup_id.clone())
            .or_default()
            .push(CaseReasoningAssessmentSnapshot::from_record(
                assessment.clone(),
            ));
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut sector_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut regime_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut reviewer_counts: HashMap<(String, String), usize> = HashMap::new();
    let mut items = Vec::new();

    for case in cases {
        let entry = histories.entry(case.setup_id.clone()).or_default();
        entry.sort_by(|left, right| left.recorded_at.cmp(&right.recorded_at));
        let current = assessment_snapshot_from_summary(case, None);
        if entry
            .last()
            .map(|last| !snapshot_matches_current(last, &current))
            .unwrap_or(true)
        {
            entry.push(current);
        }

        if entry.len() < 2 {
            continue;
        }

        let transition =
            describe_mechanism_transition(&entry[entry.len() - 2], &entry[entry.len() - 1]);
        if transition.classification == "stable" {
            continue;
        }

        *counts.entry(transition.classification.clone()).or_insert(0) += 1;
        if let Some(sector) = case
            .sector
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *sector_counts
                .entry((sector.to_string(), transition.classification.clone()))
                .or_insert(0) += 1;
        }
        let regime_key = regime_bucket(case);
        *regime_counts
            .entry((regime_key, transition.classification.clone()))
            .or_insert(0) += 1;
        if let Some(reviewer) = case
            .reviewer
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            *reviewer_counts
                .entry((reviewer.to_string(), transition.classification.clone()))
                .or_insert(0) += 1;
        }
        items.push(CaseMechanismTransitionDigest {
            setup_id: case.setup_id.clone(),
            symbol: case.symbol.clone(),
            title: case.title.clone(),
            sector: case.sector.clone(),
            regime: Some(regime_bucket(case)),
            reviewer: case.reviewer.clone(),
            from_mechanism: transition.from_mechanism.clone(),
            to_mechanism: transition.to_mechanism.clone(),
            classification: transition.classification.clone(),
            confidence: transition.confidence,
            summary: transition.summary.clone(),
            recorded_at: transition.to_recorded_at,
        });
    }

    let mut breakdown = counts
        .into_iter()
        .map(|(classification, count)| CaseMechanismTransitionStat {
            classification,
            count,
        })
        .collect::<Vec<_>>();
    breakdown.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.classification.cmp(&right.classification))
    });

    items.sort_by(|left, right| {
        right
            .recorded_at
            .cmp(&left.recorded_at)
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    items.truncate(8);

    (
        breakdown,
        build_transition_slice_stats(sector_counts),
        build_transition_slice_stats(regime_counts),
        build_transition_slice_stats(reviewer_counts),
        items,
    )
}

#[cfg(feature = "persistence")]
fn build_transition_slice_stats(
    counts: HashMap<(String, String), usize>,
) -> Vec<CaseMechanismTransitionSliceStat> {
    let mut items = counts
        .into_iter()
        .map(
            |((key, classification), count)| CaseMechanismTransitionSliceStat {
                key,
                classification,
                count,
            },
        )
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.key.cmp(&right.key))
            .then_with(|| left.classification.cmp(&right.classification))
    });
    items.truncate(8);
    items
}

#[cfg(feature = "persistence")]
fn build_invalidation_patterns(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseInvalidationPatternStat> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for assessment in assessments {
        for rule in &assessment.invalidation_rules {
            let label = normalize_invalidation_label(rule);
            if label.is_empty() {
                continue;
            }
            *counts.entry(label).or_insert(0) += 1;
        }
    }

    let mut items = counts
        .into_iter()
        .map(|(label, count)| CaseInvalidationPatternStat { label, count })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.label.cmp(&right.label))
    });
    items.truncate(8);
    items
}

#[cfg(feature = "persistence")]
fn normalize_invalidation_label(rule: &str) -> String {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .chars()
        .take(48)
        .collect::<String>()
        .trim()
        .to_string()
}

fn derive_why_now(
    tactical_case: &LiveTacticalCase,
    chain: Option<&LiveBackwardChain>,
    pressure: Option<&LivePressure>,
    causal: Option<&LiveCausalLeader>,
    track: Option<&LiveHypothesisTrack>,
    signal: Option<&LiveSignal>,
) -> String {
    if let Some(track) = track {
        if track.status != "stable" {
            return format!(
                "{} 假說{}",
                track.title,
                hypothesis_status_label(&track.status)
            );
        }
    }

    if let Some(pressure) = pressure {
        if pressure.accelerating {
            return format!("{} 資金壓力開始加速", tactical_case.symbol);
        }
    }

    if let Some(causal) = causal {
        if causal.flips > 0 && causal.leader_streak <= 2 {
            return format!("因果主導切換至 {}", causal.current_leader);
        }
    }

    if let Some(chain) = chain {
        return chain.primary_driver.clone();
    }

    if let Some(signal) = signal {
        if signal.pre_post_market_anomaly.abs() > signal.price_momentum.abs() {
            return "盤前異常高於價格動量，優先人工確認".into();
        }
    }

    tactical_case.entry_rationale.clone()
}

fn default_workflow_state(action: &str) -> &'static str {
    match action {
        "enter" => "suggest",
        _ => "review",
    }
}

fn default_invalidation_rules(
    tactical_case: &LiveTacticalCase,
    track: Option<&LiveHypothesisTrack>,
    causal: Option<&LiveCausalLeader>,
    pressure: Option<&LivePressure>,
) -> Vec<String> {
    let mut rules = Vec::new();

    if let Some(counter_label) = &tactical_case.counter_label {
        rules.push(format!("若反向假說「{}」重新主導則撤回", counter_label));
    }
    if let Some(track) = track {
        if matches!(track.status.as_str(), "weakening" | "invalidated") {
            rules.push(format!(
                "當前假說已{}，需要人工複核",
                hypothesis_status_label(&track.status)
            ));
        }
    }
    if let Some(causal) = causal {
        if causal.flips > 0 {
            rules.push(format!("近期已有 {} 次因果翻轉", causal.flips));
        }
    }
    if let Some(pressure) = pressure {
        if pressure.pressure_duration > 0 {
            rules.push(format!(
                "若資金壓力方向翻轉且持續性跌破 {} 次則撤回",
                pressure.pressure_duration
            ));
        }
    }

    ordered_unique(rules)
}

fn hypothesis_status_label(status: &str) -> &'static str {
    match status {
        "strengthening" => "正在增強",
        "weakening" => "正在減弱",
        "invalidated" => "已失效",
        "new" => "剛成立",
        _ => "需關注",
    }
}

fn ordered_unique(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for value in values {
        if seen.insert(value.clone()) {
            result.push(value);
        }
    }

    result
}

fn relevant_cross_market_signals(
    snapshot: &LiveSnapshot,
    symbol: &str,
) -> Vec<LiveCrossMarketSignal> {
    snapshot
        .cross_market_signals
        .iter()
        .filter(|item| match snapshot.market {
            LiveMarket::Us => item.us_symbol == symbol,
            LiveMarket::Hk => item.hk_symbol == symbol,
        })
        .cloned()
        .collect()
}

fn relevant_cross_market_anomalies(
    snapshot: &LiveSnapshot,
    symbol: &str,
) -> Vec<LiveCrossMarketAnomaly> {
    snapshot
        .cross_market_anomalies
        .iter()
        .filter(|item| match snapshot.market {
            LiveMarket::Us => item.us_symbol == symbol,
            LiveMarket::Hk => item.hk_symbol == symbol,
        })
        .cloned()
        .collect()
}

#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
fn review_all_cases(review: &CaseReviewResponse) -> Vec<CaseSummary> {
    let mut cases = Vec::new();
    cases.extend(review.buckets.in_flight.clone());
    cases.extend(review.buckets.under_review.clone());
    cases.extend(review.buckets.at_risk.clone());
    cases.extend(review.buckets.high_conviction.clone());

    let mut seen = HashSet::new();
    cases.retain(|case| seen.insert(case.setup_id.clone()));
    cases
}

#[cfg(feature = "persistence")]
fn build_case_mechanism_story(detail: &CaseDetail) -> CaseMechanismStory {
    let current_mechanism = detail
        .summary
        .reasoning_profile
        .primary_mechanism
        .as_ref()
        .map(|item| item.label.clone());
    let mut history = detail.reasoning_history.clone();
    let current = current_assessment_snapshot(detail);
    if history
        .last()
        .map(|last| !snapshot_matches_current(last, &current))
        .unwrap_or(true)
    {
        history.push(current);
    }
    history.sort_by(|left, right| left.recorded_at.cmp(&right.recorded_at));

    if history.len() < 2 {
        return CaseMechanismStory {
            current_mechanism,
            status: "insufficient_history".into(),
            summary: "history 尚不足以解釋機制如何演化。".into(),
            latest_transition: None,
            recent_transitions: Vec::new(),
        };
    }

    let mut transitions = history
        .windows(2)
        .map(|window| describe_mechanism_transition(&window[0], &window[1]))
        .collect::<Vec<_>>();
    if transitions.len() > 6 {
        transitions = transitions.split_off(transitions.len() - 6);
    }
    let latest_transition = transitions.last().cloned();
    let status = latest_transition
        .as_ref()
        .map(|item| item.classification.clone())
        .unwrap_or_else(|| "stable".into());
    let summary = latest_transition
        .as_ref()
        .map(|item| item.summary.clone())
        .unwrap_or_else(|| "機制目前沒有顯著切換。".into());

    CaseMechanismStory {
        current_mechanism,
        status,
        summary,
        latest_transition,
        recent_transitions: transitions,
    }
}

#[cfg(feature = "persistence")]
fn current_assessment_snapshot(detail: &CaseDetail) -> CaseReasoningAssessmentSnapshot {
    let recorded_at = detail
        .workflow
        .as_ref()
        .map(|workflow| workflow.timestamp)
        .or_else(|| summary_recorded_at(&detail.summary));
    assessment_snapshot_from_summary(&detail.summary, recorded_at)
}

#[cfg(feature = "persistence")]
fn assessment_snapshot_from_summary(
    summary: &CaseSummary,
    recorded_at: Option<OffsetDateTime>,
) -> CaseReasoningAssessmentSnapshot {
    let recorded_at = recorded_at
        .or_else(|| summary_recorded_at(summary))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);

    CaseReasoningAssessmentSnapshot {
        assessment_id: format!("{}:current", summary.setup_id),
        setup_id: summary.setup_id.clone(),
        market: match summary.market {
            LiveMarket::Hk => "hk".into(),
            LiveMarket::Us => "us".into(),
        },
        symbol: summary.symbol.clone(),
        title: summary.title.clone(),
        recommended_action: summary.recommended_action.clone(),
        source: "current".into(),
        recorded_at,
        workflow_state: summary.workflow_state.clone(),
        market_regime_bias: Some(summary.market_regime_bias.clone()),
        market_regime_confidence: Some(summary.market_regime_confidence.to_string()),
        market_breadth_delta: Some(summary.market_breadth_delta.to_string()),
        market_average_return: Some(summary.market_average_return.to_string()),
        market_directional_consensus: summary
            .market_directional_consensus
            .map(|value| value.to_string()),
        owner: summary.owner.clone(),
        reviewer: summary.reviewer.clone(),
        actor: summary.workflow_actor.clone(),
        note: summary.workflow_note.clone(),
        sector: summary.sector.clone(),
        primary_mechanism_kind: summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.label.clone()),
        primary_mechanism_score: summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.score.to_string()),
        law_kinds: summary
            .reasoning_profile
            .laws
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        predicate_kinds: summary
            .reasoning_profile
            .predicates
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        composite_state_kinds: summary
            .reasoning_profile
            .composite_states
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        competing_mechanism_kinds: summary
            .reasoning_profile
            .competing_mechanisms
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        invalidation_rules: summary.invalidation_rules.clone(),
        reasoning_profile: summary.reasoning_profile.clone(),
    }
}

#[cfg(feature = "persistence")]
fn summary_recorded_at(summary: &CaseSummary) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(
        &summary.updated_at,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()
}

#[cfg(feature = "persistence")]
fn regime_bucket(summary: &CaseSummary) -> String {
    let confidence_bucket = if summary.market_regime_confidence >= Decimal::new(65, 2) {
        "high"
    } else if summary.market_regime_confidence >= Decimal::new(35, 2) {
        "medium"
    } else {
        "low"
    };
    format!("{}:{confidence_bucket}", summary.market_regime_bias)
}

#[cfg(feature = "persistence")]
fn snapshot_matches_current(
    existing: &CaseReasoningAssessmentSnapshot,
    current: &CaseReasoningAssessmentSnapshot,
) -> bool {
    existing.workflow_state == current.workflow_state
        && existing.market_regime_bias == current.market_regime_bias
        && existing.primary_mechanism_kind == current.primary_mechanism_kind
        && existing.note == current.note
}

#[cfg(feature = "persistence")]
fn describe_mechanism_transition(
    from: &CaseReasoningAssessmentSnapshot,
    to: &CaseReasoningAssessmentSnapshot,
) -> CaseMechanismTransition {
    let from_factors = mechanism_factor_map(from);
    let to_factors = mechanism_factor_map(to);
    let from_states = state_score_map(from);
    let to_states = state_score_map(to);

    let decay_evidence = factor_delta_strings(&from_factors, &to_factors, true);
    let emerging_evidence = factor_delta_strings(&to_factors, &from_factors, false);
    let mut regime_evidence = regime_delta_strings(&from_states, &to_states);
    let regime_change = match (&from.market_regime_bias, &to.market_regime_bias) {
        (Some(left), Some(right)) if left != right => Some(format!("{left} -> {right}")),
        _ => None,
    };
    if let Some(change) = regime_change.as_ref() {
        regime_evidence.insert(0, format!("market regime {}", change));
    }
    regime_evidence.extend(regime_metric_delta_strings(from, to));
    let review_evidence = to
        .reasoning_profile
        .human_review
        .as_ref()
        .map(|review| {
            review
                .reasons
                .iter()
                .map(|reason| reason.label.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let regime_score = regime_shift_score(&from_states, &to_states, regime_change.is_some());
    let regime_metric_score = regime_metric_shift_score(from, to);
    let decay_score = factor_decay_score(&from_factors, &to_factors);
    let review_score = if review_evidence.is_empty() {
        Decimal::ZERO
    } else {
        Decimal::new(18, 2)
    };
    let combined_regime_score = clamp_decimal(regime_score + regime_metric_score);
    let classification = classify_transition(
        from.primary_mechanism_kind.as_deref(),
        to.primary_mechanism_kind.as_deref(),
        combined_regime_score,
        decay_score,
        review_score,
    );
    let confidence = clamp_decimal(
        combined_regime_score
            .max(decay_score)
            .max(review_score)
            .max(
                if from.primary_mechanism_kind != to.primary_mechanism_kind {
                    Decimal::new(55, 2)
                } else {
                    Decimal::new(35, 2)
                },
            ),
    );
    let summary = transition_summary(
        from.primary_mechanism_kind.as_deref(),
        to.primary_mechanism_kind.as_deref(),
        &classification,
        regime_evidence.first().cloned(),
        decay_evidence.first().cloned(),
        review_evidence.first().cloned(),
    );

    CaseMechanismTransition {
        from_recorded_at: from.recorded_at,
        to_recorded_at: to.recorded_at,
        from_mechanism: from.primary_mechanism_kind.clone(),
        to_mechanism: to.primary_mechanism_kind.clone(),
        classification,
        confidence,
        summary,
        regime_change,
        regime_evidence,
        decay_evidence,
        emerging_evidence,
        review_evidence,
    }
}

#[cfg(feature = "persistence")]
fn mechanism_factor_map(
    snapshot: &CaseReasoningAssessmentSnapshot,
) -> HashMap<String, (String, Decimal)> {
    snapshot
        .reasoning_profile
        .primary_mechanism
        .as_ref()
        .map(|mechanism| {
            mechanism
                .factors
                .iter()
                .map(|factor| {
                    (
                        factor.key.clone(),
                        (factor.label.clone(), factor.contribution),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

#[cfg(feature = "persistence")]
fn state_score_map(snapshot: &CaseReasoningAssessmentSnapshot) -> HashMap<String, Decimal> {
    snapshot
        .reasoning_profile
        .composite_states
        .iter()
        .map(|state| (state.label.clone(), state.score))
        .collect()
}

#[cfg(feature = "persistence")]
fn factor_delta_strings(
    primary: &HashMap<String, (String, Decimal)>,
    reference: &HashMap<String, (String, Decimal)>,
    require_negative: bool,
) -> Vec<String> {
    let mut deltas = primary
        .iter()
        .filter_map(|(key, (label, value))| {
            let other = reference
                .get(key)
                .map(|item| item.1)
                .unwrap_or(Decimal::ZERO);
            let delta = *value - other;
            if require_negative {
                (delta > Decimal::new(8, 2))
                    .then_some(format!("{label} faded {:+}", -delta.round_dp(2)))
            } else {
                (delta > Decimal::new(8, 2))
                    .then_some(format!("{label} rose {:+}", delta.round_dp(2)))
            }
        })
        .collect::<Vec<_>>();
    deltas.sort();
    deltas.truncate(3);
    deltas
}

#[cfg(feature = "persistence")]
fn regime_delta_strings(
    from_states: &HashMap<String, Decimal>,
    to_states: &HashMap<String, Decimal>,
) -> Vec<String> {
    let regime_keys = [
        "Event Catalyst",
        "Cross-market Dislocation",
        "Substitution Flow",
        "Cross-scope Contagion",
        "Structural Fragility",
    ];
    let mut items = regime_keys
        .iter()
        .filter_map(|key| {
            let delta = to_states.get(*key).copied().unwrap_or(Decimal::ZERO)
                - from_states.get(*key).copied().unwrap_or(Decimal::ZERO);
            (delta > Decimal::new(8, 2))
                .then_some(format!("{key} strengthened {:+}", delta.round_dp(2)))
        })
        .collect::<Vec<_>>();
    items.truncate(3);
    items
}

#[cfg(feature = "persistence")]
fn regime_metric_delta_strings(
    from: &CaseReasoningAssessmentSnapshot,
    to: &CaseReasoningAssessmentSnapshot,
) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(delta) = decimal_delta(
        from.market_regime_confidence.as_deref(),
        to.market_regime_confidence.as_deref(),
    ) {
        if delta.abs() >= Decimal::new(10, 2) {
            items.push(format!("regime confidence {:+}", delta.round_dp(2)));
        }
    }
    if let Some(delta) = decimal_delta(
        from.market_breadth_delta.as_deref(),
        to.market_breadth_delta.as_deref(),
    ) {
        if delta.abs() >= Decimal::new(10, 2) {
            items.push(format!("breadth delta {:+}", delta.round_dp(2)));
        }
    }
    if let Some(delta) = decimal_delta(
        from.market_average_return.as_deref(),
        to.market_average_return.as_deref(),
    ) {
        if delta.abs() >= Decimal::new(2, 2) {
            items.push(format!("avg return {:+}", delta.round_dp(2)));
        }
    }
    items.truncate(3);
    items
}

#[cfg(feature = "persistence")]
fn regime_shift_score(
    from_states: &HashMap<String, Decimal>,
    to_states: &HashMap<String, Decimal>,
    market_regime_changed: bool,
) -> Decimal {
    let regime_keys = [
        "Event Catalyst",
        "Cross-market Dislocation",
        "Substitution Flow",
        "Cross-scope Contagion",
        "Structural Fragility",
    ];
    let mut total = regime_keys.iter().fold(Decimal::ZERO, |acc, key| {
        let delta = to_states.get(*key).copied().unwrap_or(Decimal::ZERO)
            - from_states.get(*key).copied().unwrap_or(Decimal::ZERO);
        if delta > Decimal::ZERO {
            acc + delta
        } else {
            acc
        }
    });
    if market_regime_changed {
        total += Decimal::new(18, 2);
    }
    clamp_decimal(total)
}

#[cfg(feature = "persistence")]
fn regime_metric_shift_score(
    from: &CaseReasoningAssessmentSnapshot,
    to: &CaseReasoningAssessmentSnapshot,
) -> Decimal {
    let mut total = Decimal::ZERO;
    if let Some(delta) = decimal_delta(
        from.market_regime_confidence.as_deref(),
        to.market_regime_confidence.as_deref(),
    ) {
        total += delta.abs();
    }
    if let Some(delta) = decimal_delta(
        from.market_breadth_delta.as_deref(),
        to.market_breadth_delta.as_deref(),
    ) {
        total += delta.abs() / Decimal::from(2);
    }
    if let Some(delta) = decimal_delta(
        from.market_average_return.as_deref(),
        to.market_average_return.as_deref(),
    ) {
        total += delta.abs() * Decimal::from(4);
    }
    clamp_decimal(total)
}

#[cfg(feature = "persistence")]
fn factor_decay_score(
    from_factors: &HashMap<String, (String, Decimal)>,
    to_factors: &HashMap<String, (String, Decimal)>,
) -> Decimal {
    clamp_decimal(
        from_factors
            .iter()
            .fold(Decimal::ZERO, |acc, (key, (_, value))| {
                let next = to_factors
                    .get(key)
                    .map(|item| item.1)
                    .unwrap_or(Decimal::ZERO);
                if *value > next {
                    acc + (*value - next)
                } else {
                    acc
                }
            }),
    )
}

#[cfg(feature = "persistence")]
fn classify_transition(
    from_mechanism: Option<&str>,
    to_mechanism: Option<&str>,
    regime_score: Decimal,
    decay_score: Decimal,
    review_score: Decimal,
) -> String {
    let regime_shift = Decimal::new(22, 2);
    let decay_shift = Decimal::new(20, 2);
    let mild = Decimal::new(12, 2);

    if review_score >= Decimal::new(18, 2) && regime_score < mild && decay_score < mild {
        return "review_override".into();
    }
    if regime_score >= regime_shift && decay_score < Decimal::new(16, 2) {
        return "regime_shift".into();
    }
    if decay_score >= decay_shift && regime_score < Decimal::new(15, 2) {
        return "mechanism_decay".into();
    }
    if regime_score >= mild && decay_score >= mild {
        return "mixed".into();
    }
    if from_mechanism != to_mechanism {
        return "mixed".into();
    }
    "stable".into()
}

#[cfg(feature = "persistence")]
fn transition_summary(
    from_mechanism: Option<&str>,
    to_mechanism: Option<&str>,
    classification: &str,
    regime_hint: Option<String>,
    decay_hint: Option<String>,
    review_hint: Option<String>,
) -> String {
    let from = from_mechanism.unwrap_or("Unknown");
    let to = to_mechanism.unwrap_or("Unknown");
    match classification {
        "regime_shift" => format!(
            "{from} -> {to}，主因偏向環境切換：{}。",
            regime_hint.unwrap_or_else(|| "regime-sensitive states strengthened".into())
        ),
        "mechanism_decay" => format!(
            "{from} -> {to}，更像原機制先衰減：{}。",
            decay_hint.unwrap_or_else(|| "old primary factors faded".into())
        ),
        "review_override" => format!(
            "{from} -> {to}，主要由人類校準推動：{}。",
            review_hint.unwrap_or_else(|| "review reasons overrode the prior thesis".into())
        ),
        "mixed" => format!(
            "{from} -> {to}，同時有環境切換與原機制衰減。{}{}",
            regime_hint
                .map(|item| format!("環境面：{item}。"))
                .unwrap_or_default(),
            decay_hint
                .map(|item| format!("結構面：{item}。"))
                .unwrap_or_default()
        ),
        _ => format!("{to} 仍為主機制，近期沒有足夠證據顯示結構性切換。"),
    }
}

#[cfg(feature = "persistence")]
fn clamp_decimal(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        Decimal::ZERO
    } else if value > Decimal::ONE {
        Decimal::ONE
    } else {
        value
    }
}

#[cfg(feature = "persistence")]
fn decimal_delta(from: Option<&str>, to: Option<&str>) -> Option<Decimal> {
    let from = from.and_then(|value| value.parse::<Decimal>().ok())?;
    let to = to.and_then(|value| value.parse::<Decimal>().ok())?;
    Some(to - from)
}

impl CaseReasoningAssessmentSnapshot {
    #[cfg(feature = "persistence")]
    fn from_record(record: CaseReasoningAssessmentRecord) -> Self {
        Self {
            assessment_id: record.assessment_id,
            setup_id: record.setup_id,
            market: record.market,
            symbol: record.symbol,
            title: record.title,
            sector: record.sector,
            recommended_action: record.recommended_action,
            source: record.source,
            recorded_at: record.recorded_at,
            workflow_state: record.workflow_state,
            market_regime_bias: record.market_regime_bias,
            market_regime_confidence: record.market_regime_confidence,
            market_breadth_delta: record.market_breadth_delta,
            market_average_return: record.market_average_return,
            market_directional_consensus: record.market_directional_consensus,
            owner: record.owner,
            reviewer: record.reviewer,
            actor: record.actor,
            note: record.note,
            primary_mechanism_kind: record.primary_mechanism_kind,
            primary_mechanism_score: record.primary_mechanism_score,
            law_kinds: record.law_kinds,
            predicate_kinds: record.predicate_kinds,
            composite_state_kinds: record.composite_state_kinds,
            competing_mechanism_kinds: record.competing_mechanism_kinds,
            invalidation_rules: record.invalidation_rules,
            reasoning_profile: record.reasoning_profile,
        }
    }
}

#[cfg(feature = "persistence")]
fn record_invalidation_rules(setup: &TacticalSetupRecord) -> Vec<String> {
    ordered_unique(
        setup
            .risk_notes
            .iter()
            .chain(setup.falsified_by.iter())
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    #[cfg(feature = "persistence")]
    use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;

    #[test]
    fn actionable_cases_sort_first() {
        let snapshot = LiveSnapshot {
            tick: 1,
            timestamp: "2026-03-22T09:30:00Z".into(),
            market: LiveMarket::Us,
            stock_count: 2,
            edge_count: 1,
            hypothesis_count: 1,
            observation_count: 1,
            active_positions: 0,
            market_regime: LiveMarketRegime {
                bias: "risk_on".into(),
                confidence: dec!(0.6),
                breadth_up: dec!(0.5),
                breadth_down: dec!(0.4),
                average_return: dec!(0.02),
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: dec!(0.1),
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            scorecard: LiveScorecard {
                total_signals: 1,
                resolved_signals: 1,
                hits: 1,
                misses: 0,
                hit_rate: dec!(1),
                mean_return: dec!(0.03),
            },
            tactical_cases: vec![
                LiveTacticalCase {
                    setup_id: "setup:b".into(),
                    symbol: "B.US".into(),
                    title: "Watch B".into(),
                    action: "review".into(),
                    confidence: dec!(0.4),
                    confidence_gap: dec!(0.1),
                    heuristic_edge: dec!(0.02),
                    entry_rationale: "watch".into(),
                    family_label: None,
                    counter_label: None,
                },
                LiveTacticalCase {
                    setup_id: "setup:a".into(),
                    symbol: "A.US".into(),
                    title: "Long A".into(),
                    action: "enter".into(),
                    confidence: dec!(0.7),
                    confidence_gap: dec!(0.2),
                    heuristic_edge: dec!(0.05),
                    entry_rationale: "go".into(),
                    family_label: None,
                    counter_label: None,
                },
            ],
            hypothesis_tracks: vec![],
            top_signals: vec![],
            convergence_scores: vec![],
            pressures: vec![],
            backward_chains: vec![],
            causal_leaders: vec![],
            events: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            lineage: vec![],
        };

        let cases = build_case_summaries(&snapshot);
        assert_eq!(cases[0].setup_id, "setup:a");
        assert!(!cases[0].reasoning_profile.predicates.is_empty());
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn review_analytics_capture_drift_and_invalidation_patterns() {
        let base_case = CaseSummary {
            case_id: "setup:a".into(),
            setup_id: "setup:a".into(),
            workflow_id: Some("wf:a".into()),
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            workflow_actor: Some("actor".into()),
            workflow_note: Some("reject narrative".into()),
            symbol: "A.US".into(),
            title: "Case A".into(),
            sector: Some("Technology".into()),
            market: LiveMarket::Us,
            recommended_action: "enter".into(),
            workflow_state: "review".into(),
            market_regime_bias: "risk_off".into(),
            market_regime_confidence: dec!(0.75),
            market_breadth_delta: dec!(-0.35),
            market_average_return: dec!(-0.04),
            market_directional_consensus: Some(dec!(-0.20)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            why_now: "why".into(),
            primary_driver: None,
            family_label: None,
            counter_label: None,
            hypothesis_status: Some("weakening".into()),
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            key_evidence: vec![],
            invalidation_rules: vec!["若反向假說重新主導則撤回".into()],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![],
                composite_states: vec![],
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
                primary_mechanism: Some(crate::ontology::MechanismCandidate {
                    kind: crate::ontology::MechanismCandidateKind::NarrativeFailure,
                    label: "Narrative Failure".into(),
                    score: dec!(0.71),
                    summary: "s".into(),
                    supporting_states: vec![],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let runtime_1 = CaseReasoningAssessmentRecord::from_case_summary(
            &base_case,
            OffsetDateTime::from_unix_timestamp(1_711_102_000).unwrap(),
            "runtime",
        );
        let mut runtime_2_case = base_case.clone();
        runtime_2_case.reasoning_profile.primary_mechanism =
            Some(crate::ontology::MechanismCandidate {
                kind: crate::ontology::MechanismCandidateKind::FragilityBuildUp,
                label: "Fragility Build-up".into(),
                score: dec!(0.66),
                summary: "s".into(),
                supporting_states: vec![],
                invalidation: vec![],
                human_checks: vec![],
                factors: vec![],
                counterfactuals: vec![],
            });
        runtime_2_case.invalidation_rules = vec!["若 stress 回落則撤回".into()];
        let runtime_2 = CaseReasoningAssessmentRecord::from_case_summary(
            &runtime_2_case,
            OffsetDateTime::from_unix_timestamp(1_711_105_600).unwrap(),
            "runtime",
        );
        let workflow_update = CaseReasoningAssessmentRecord::from_case_summary(
            &base_case,
            OffsetDateTime::from_unix_timestamp(1_711_105_900).unwrap(),
            "workflow_update",
        );

        let analytics = build_case_review_analytics_with_assessments(
            &[base_case],
            &[runtime_1, runtime_2, workflow_update],
            crate::pipeline::learning_loop::OutcomeLearningContext::default(),
        );

        assert!(!analytics.mechanism_drift.is_empty());
        assert!(!analytics.mechanism_transition_breakdown.is_empty());
        assert!(!analytics.transition_by_sector.is_empty());
        assert!(!analytics.transition_by_regime.is_empty());
        assert!(!analytics.transition_by_reviewer.is_empty());
        assert!(!analytics.recent_mechanism_transitions.is_empty());
        assert!(!analytics.reviewer_doctrine.is_empty());
        assert!(!analytics.human_review_reasons.is_empty());
        assert!(!analytics.invalidation_patterns.is_empty());
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn mechanism_transition_story_classifies_regime_shift() {
        let old_case = CaseSummary {
            case_id: "setup:rot".into(),
            setup_id: "setup:rot".into(),
            workflow_id: Some("wf:rot".into()),
            owner: None,
            reviewer: None,
            workflow_actor: None,
            workflow_note: None,
            symbol: "9901.HK".into(),
            title: "Rotation".into(),
            sector: Some("Technology".into()),
            market: LiveMarket::Hk,
            recommended_action: "enter".into(),
            workflow_state: "suggest".into(),
            market_regime_bias: "neutral".into(),
            market_regime_confidence: dec!(0.30),
            market_breadth_delta: dec!(-0.05),
            market_average_return: dec!(0.00),
            market_directional_consensus: Some(dec!(0.01)),
            confidence: dec!(0.55),
            confidence_gap: dec!(0.10),
            heuristic_edge: dec!(0.04),
            why_now: "why".into(),
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
                composite_states: vec![crate::ontology::CompositeState {
                    kind: crate::ontology::CompositeStateKind::DirectionalReinforcement,
                    label: "Directional Reinforcement".into(),
                    score: dec!(0.20),
                    summary: "s".into(),
                    predicates: vec![],
                }],
                human_review: None,
                primary_mechanism: Some(crate::ontology::MechanismCandidate {
                    kind: crate::ontology::MechanismCandidateKind::MechanicalExecutionSignature,
                    label: "Mechanical Execution Signature".into(),
                    score: dec!(0.35),
                    summary: "s".into(),
                    supporting_states: vec![],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![crate::ontology::MechanismFactor {
                        key: "state:directional_reinforcement".into(),
                        label: "Directional Reinforcement".into(),
                        source: crate::ontology::MechanismFactorSource::State,
                        activation: dec!(0.20),
                        base_weight: dec!(0.45),
                        learned_weight_delta: Decimal::ZERO,
                        effective_weight: dec!(0.50),
                        contribution: dec!(0.10),
                    }],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let mut new_case = old_case.clone();
        new_case.market_regime_bias = "risk_off".into();
        new_case.reasoning_profile.composite_states = vec![
            crate::ontology::CompositeState {
                kind: crate::ontology::CompositeStateKind::SubstitutionFlow,
                label: "Substitution Flow".into(),
                score: dec!(0.72),
                summary: "s".into(),
                predicates: vec![],
            },
            crate::ontology::CompositeState {
                kind: crate::ontology::CompositeStateKind::CrossScopeContagion,
                label: "Cross-scope Contagion".into(),
                score: dec!(0.24),
                summary: "s".into(),
                predicates: vec![],
            },
        ];
        new_case.reasoning_profile.primary_mechanism = Some(crate::ontology::MechanismCandidate {
            kind: crate::ontology::MechanismCandidateKind::CapitalRotation,
            label: "Capital Rotation".into(),
            score: dec!(0.68),
            summary: "s".into(),
            supporting_states: vec![],
            invalidation: vec![],
            human_checks: vec![],
            factors: vec![crate::ontology::MechanismFactor {
                key: "state:substitution_flow".into(),
                label: "Substitution Flow".into(),
                source: crate::ontology::MechanismFactorSource::State,
                activation: dec!(0.72),
                base_weight: dec!(0.60),
                learned_weight_delta: Decimal::ZERO,
                effective_weight: dec!(0.60),
                contribution: dec!(0.43),
            }],
            counterfactuals: vec![],
        });

        let old_snapshot = CaseReasoningAssessmentSnapshot::from_record(
            CaseReasoningAssessmentRecord::from_case_summary(
                &old_case,
                OffsetDateTime::from_unix_timestamp(1_711_102_000).unwrap(),
                "runtime",
            ),
        );
        let new_snapshot = CaseReasoningAssessmentSnapshot::from_record(
            CaseReasoningAssessmentRecord::from_case_summary(
                &new_case,
                OffsetDateTime::from_unix_timestamp(1_711_105_600).unwrap(),
                "runtime",
            ),
        );

        let transition = describe_mechanism_transition(&old_snapshot, &new_snapshot);
        assert_eq!(transition.classification, "regime_shift");
        assert!(transition.regime_change.is_some());
        assert!(!transition.regime_evidence.is_empty());
    }
}
