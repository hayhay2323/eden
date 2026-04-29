use std::collections::{BTreeMap, HashSet};

use crate::action::workflow::{
    governance_reason, governance_reason_code, ActionExecutionPolicy, ActionGovernanceContract,
    ActionGovernanceReasonCode,
};
use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal,
    LiveHypothesisTrack, LiveMarket, LivePressure, LiveSignal, LiveSnapshot, LiveTacticalCase,
};
use crate::math::clamp_unit_interval;
use crate::ontology::horizon::{HorizonBucket, Urgency};
use crate::ontology::{
    ArchetypeProjection, CaseChannel, CaseReasoningProfile, CaseSignature, CaseTemporalShape,
    CaseTopology, ConflictShape, IntentDirection, IntentHypothesis, IntentKind,
    IntentOpportunityBias, IntentOpportunityWindow,
};
use crate::pipeline::learning_loop::{apply_learning_feedback, ReasoningLearningFeedback};
use crate::pipeline::mechanism_inference::build_reasoning_profile_with_adjustments;
use crate::pipeline::predicate_engine::{
    augment_predicates_with_workflow, derive_atomic_predicates, derive_human_review_context,
    PredicateInputs,
};

use super::review_analytics::build_case_review_analytics;
use super::types::{
    CaseBriefingMetrics, CaseBriefingResponse, CaseBriefingWatchouts, CaseDetail, CaseEvidence,
    CaseGovernanceBuckets, CaseGovernanceReasonBuckets, CaseLineageContext, CaseListResponse,
    CaseMarketContext, CaseMechanismStory, CasePrimaryLensBuckets, CaseQueuePinBuckets,
    CaseReviewBuckets, CaseReviewMetrics, CaseReviewResponse, CaseSummary, SnapshotCaseLookups,
};

pub fn build_case_list(snapshot: &LiveSnapshot) -> CaseListResponse {
    build_case_list_with_feedback(snapshot, None)
}

/// Build case list with optional pre-computed learning feedback applied to reasoning profiles.
/// When feedback is provided, predicate and mechanism scores are adjusted before case ranking.
pub fn build_case_list_with_feedback(
    snapshot: &LiveSnapshot,
    feedback: Option<&ReasoningLearningFeedback>,
) -> CaseListResponse {
    let mut cases = build_case_summaries(snapshot);
    if let Some(fb) = feedback {
        for case in &mut cases {
            case.reasoning_profile = apply_learning_feedback(
                &case.reasoning_profile,
                &case.invalidation_rules,
                fb,
                None,
            );
            // 2026-04-29: removed apply_case_structure_feedback —
            // mirror of the deleted setup-level rogue modulator
            // (5-channel weighted sum on case.confidence). Audit
            // finding CRITICAL #1 from 漏網之魚 sweep.
        }
    }
    cases.sort_by(|left, right| {
        case_priority(left)
            .cmp(&case_priority(right))
            .then_with(|| {
                case_structure_priority_with_feedback(right, feedback)
                    .cmp(&case_structure_priority_with_feedback(left, feedback))
            })
            .then_with(|| case_intent_priority(right).cmp(&case_intent_priority(left)))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
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
        governance_buckets: build_case_governance_buckets(&cases),
        governance_reason_buckets: build_case_governance_reason_buckets(&cases),
        primary_lens_buckets: build_case_primary_lens_buckets(&cases),
        queue_pin_buckets: build_case_queue_pin_buckets(&cases),
        cases,
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

pub fn filter_case_list_by_queue_pin(response: &mut CaseListResponse, queue_pin: Option<&str>) {
    let Some(queue_pin) = queue_pin.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    let normalized = queue_pin.to_lowercase();
    response.cases.retain(|item| {
        let value = item
            .queue_pin
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match normalized.as_str() {
            "any" => value.is_some(),
            "none" => value.is_none(),
            _ => value
                .map(|value| value.to_lowercase() == normalized)
                .unwrap_or(false),
        }
    });
}

pub fn filter_case_list_by_primary_lens(
    response: &mut CaseListResponse,
    primary_lens: Option<&str>,
) {
    let Some(primary_lens) = primary_lens
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    let normalized = primary_lens.to_lowercase();
    response.cases.retain(|item| {
        let value = item
            .primary_lens
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match normalized.as_str() {
            "any" => value.is_some(),
            "none" | "unknown" => value.is_none(),
            _ => value
                .map(|value| value.eq_ignore_ascii_case(&normalized))
                .unwrap_or(false),
        }
    });
}

pub fn filter_case_list_by_mechanism(response: &mut CaseListResponse, mechanism: Option<&str>) {
    let Some(mechanism) = mechanism.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    let normalized = normalize_mechanism_key(mechanism);
    response
        .cases
        .retain(|item| case_matches_mechanism(item, &normalized));
}

pub fn filter_case_list_by_opportunity(
    response: &mut CaseListResponse,
    opportunity_horizon: Option<&str>,
    opportunity_bias: Option<&str>,
) {
    let horizon = opportunity_horizon
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let bias = opportunity_bias
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());

    if horizon.is_none() && bias.is_none() {
        return;
    }

    response.cases.retain(|item| {
        item.intent_opportunities.iter().any(|opportunity| {
            let horizon_match = horizon
                .as_deref()
                .map(|value| opportunity.horizon.eq_ignore_ascii_case(value))
                .unwrap_or(true);
            let bias_match = bias
                .as_deref()
                .map(|value| format!("{:?}", opportunity.bias).to_ascii_lowercase() == value)
                .unwrap_or(true);
            horizon_match && bias_match
        })
    });
}

pub fn filter_case_list_by_governance_reason_code(
    response: &mut CaseListResponse,
    governance_reason_code: Option<ActionGovernanceReasonCode>,
) {
    let Some(governance_reason_code) = governance_reason_code else {
        return;
    };

    response
        .cases
        .retain(|item| inferred_governance_reason_code(item) == governance_reason_code);
}

pub fn refresh_case_list_governance(response: &mut CaseListResponse) {
    response.governance_buckets = build_case_governance_buckets(&response.cases);
    response.governance_reason_buckets = build_case_governance_reason_buckets(&response.cases);
    response.primary_lens_buckets = build_case_primary_lens_buckets(&response.cases);
    response.queue_pin_buckets = build_case_queue_pin_buckets(&response.cases);
}

fn case_matches_mechanism(case: &CaseSummary, normalized_query: &str) -> bool {
    case.reasoning_profile
        .primary_mechanism
        .as_ref()
        .map(|mechanism| normalize_mechanism_key(&mechanism.label) == normalized_query)
        .unwrap_or(false)
        || case
            .reasoning_profile
            .competing_mechanisms
            .iter()
            .any(|mechanism| normalize_mechanism_key(&mechanism.label) == normalized_query)
}

fn normalize_mechanism_key(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn summary_is_actionable(case: &CaseSummary) -> bool {
    matches!(case.actionability_state.as_deref(), Some("actionable"))
        || (case.actionability_state.is_none() && case.recommended_action == "enter")
}

fn summary_is_watch(case: &CaseSummary) -> bool {
    !summary_is_actionable(case)
}

pub fn build_case_briefing(list: &CaseListResponse) -> CaseBriefingResponse {
    let actionable = list
        .cases
        .iter()
        .filter(|item| summary_is_actionable(item))
        .cloned()
        .collect::<Vec<_>>();
    let mut review_cases = list
        .cases
        .iter()
        .filter(|item| item.workflow_state == "review")
        .cloned()
        .collect::<Vec<_>>();
    review_cases.sort_by(|left, right| {
        case_governance_reason_priority(left, right)
            .then_with(|| case_intent_priority(right).cmp(&case_intent_priority(left)))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
    });
    let watch_cases = list
        .cases
        .iter()
        .filter(|item| summary_is_watch(item))
        .cloned()
        .collect::<Vec<_>>();

    CaseBriefingResponse {
        context: list.context.clone(),
        metrics: CaseBriefingMetrics {
            actionable: actionable.len(),
            needs_review: review_cases.len(),
            watchlist: watch_cases.len(),
            active_positions: list.context.active_positions,
            manual_only: count_cases_by_policy(&list.cases, ActionExecutionPolicy::ManualOnly),
            review_required: count_cases_by_policy(
                &list.cases,
                ActionExecutionPolicy::ReviewRequired,
            ),
            auto_eligible: count_cases_by_policy(&list.cases, ActionExecutionPolicy::AutoEligible),
            queue_pinned: count_queue_pinned_cases(&list.cases),
        },
        priority_cases: actionable.into_iter().take(6).collect(),
        review_cases: review_cases.into_iter().take(5).collect(),
        watch_cases: watch_cases.into_iter().take(6).collect(),
        governance_buckets: build_case_governance_buckets(&list.cases),
        governance_reason_buckets: build_case_governance_reason_buckets(&list.cases),
        primary_lens_buckets: build_case_primary_lens_buckets(&list.cases),
        queue_pin_buckets: build_case_queue_pin_buckets(&list.cases),
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
            dominant_intents: summarize_briefing_intents(&list.cases),
            dominant_opportunities: summarize_briefing_opportunities(&list.cases),
            learned_archetypes: summarize_briefing_archetypes(&list.cases),
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
    let mut under_review = list
        .cases
        .iter()
        .filter(|item| item.workflow_state == "review")
        .cloned()
        .collect::<Vec<_>>();
    under_review.sort_by(|left, right| {
        case_governance_reason_priority(left, right)
            .then_with(|| case_intent_priority(right).cmp(&case_intent_priority(left)))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
    });
    let at_risk = list
        .cases
        .iter()
        .filter(|item| {
            matches!(
                item.hypothesis_status.as_deref(),
                Some("weakening") | Some("invalidated")
            ) || !item.invalidation_rules.is_empty()
                || item.inferred_intent.as_ref().is_some_and(|intent| {
                    matches!(
                        intent.state,
                        crate::ontology::IntentState::AtRisk
                            | crate::ontology::IntentState::Exhausted
                            | crate::ontology::IntentState::Invalidated
                    )
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let high_conviction = list
        .cases
        .iter()
        .filter(|item| summary_is_actionable(item) && item.workflow_state != "review")
        .cloned()
        .collect::<Vec<_>>();

    CaseReviewResponse {
        context: list.context.clone(),
        metrics: CaseReviewMetrics {
            in_flight: in_flight.len(),
            under_review: under_review.len(),
            at_risk: at_risk.len(),
            high_conviction: high_conviction.len(),
            manual_only: count_cases_by_policy(&list.cases, ActionExecutionPolicy::ManualOnly),
            review_required: count_cases_by_policy(
                &list.cases,
                ActionExecutionPolicy::ReviewRequired,
            ),
            auto_eligible: count_cases_by_policy(&list.cases, ActionExecutionPolicy::AutoEligible),
            queue_pinned: count_queue_pinned_cases(&list.cases),
        },
        buckets: CaseReviewBuckets {
            in_flight,
            under_review,
            at_risk,
            high_conviction,
        },
        governance_buckets: build_case_governance_buckets(&list.cases),
        governance_reason_buckets: build_case_governance_reason_buckets(&list.cases),
        primary_lens_buckets: build_case_primary_lens_buckets(&list.cases),
        queue_pin_buckets: build_case_queue_pin_buckets(&list.cases),
        analytics: build_case_review_analytics(&list.cases),
    }
}

pub fn build_case_summaries(snapshot: &LiveSnapshot) -> Vec<CaseSummary> {
    let lookups = snapshot_case_lookups(snapshot);

    let mut cases = snapshot
        .tactical_cases
        .iter()
        .map(|tactical_case| build_case_summary(snapshot, &lookups, tactical_case))
        .collect::<Vec<_>>();

    cases.sort_by(|left, right| {
        case_priority(left)
            .cmp(&case_priority(right))
            .then_with(|| case_structure_priority(right).cmp(&case_structure_priority(left)))
            .then_with(|| case_intent_priority(right).cmp(&case_intent_priority(left)))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    cases
}

pub fn build_case_detail(snapshot: &LiveSnapshot, setup_id: &str) -> Option<CaseDetail> {
    let lookups = snapshot_case_lookups(snapshot);
    let tactical_case = snapshot
        .tactical_cases
        .iter()
        .find(|item| item.setup_id == setup_id)?
        .clone();
    let summary = build_case_summary(snapshot, &lookups, &tactical_case);
    let symbol = tactical_case.symbol.as_str();

    let backward_chain = lookups.chains.get(symbol).map(|item| (*item).clone());
    let pressure = lookups.pressures.get(symbol).map(|item| (*item).clone());
    let signal = lookups.signals.get(symbol).map(|item| (*item).clone());
    let causal_leader = lookups.causals.get(symbol).map(|item| (*item).clone());
    let hypothesis_track = lookups.tracks.get(symbol).map(|item| (*item).clone());

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

    let mut risk_notes = Vec::new();
    if let Some(intent) = summary.inferred_intent.as_ref() {
        risk_notes.push(format!(
            "intent_state={:?} strength={:.3} conflict={:.3}",
            intent.state, intent.strength.composite, intent.conflict_score
        ));
        risk_notes.extend(intent.opportunities.iter().map(|window| {
            format!(
                "opportunity:{}:{:?}:conf={:.3}:align={:.3}",
                window.horizon, window.bias, window.confidence, window.alignment
            )
        }));
    }

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
        risk_notes,
        lineage_context: CaseLineageContext::default(),
        workflow: None,
        workflow_history: Vec::new(),
        reasoning_history: Vec::new(),
        mechanism_story: CaseMechanismStory::default(),
    })
}

fn snapshot_case_lookups(snapshot: &LiveSnapshot) -> SnapshotCaseLookups<'_> {
    SnapshotCaseLookups {
        chains: snapshot
            .backward_chains
            .iter()
            .map(|item| (item.symbol.as_str(), item))
            .collect(),
        pressures: snapshot
            .pressures
            .iter()
            .map(|item| (item.symbol.as_str(), item))
            .collect(),
        signals: snapshot
            .top_signals
            .iter()
            .map(|item| (item.symbol.as_str(), item))
            .collect(),
        causals: snapshot
            .causal_leaders
            .iter()
            .map(|item| (item.symbol.as_str(), item))
            .collect(),
        tracks: snapshot
            .hypothesis_tracks
            .iter()
            .map(|item| (item.symbol.as_str(), item))
            .collect(),
    }
}

fn build_case_summary(
    snapshot: &LiveSnapshot,
    lookups: &SnapshotCaseLookups<'_>,
    tactical_case: &LiveTacticalCase,
) -> CaseSummary {
    let symbol = tactical_case.symbol.as_str();
    let chain = lookups.chains.get(symbol).copied();
    let pressure = lookups.pressures.get(symbol).copied();
    let causal = lookups.causals.get(symbol).copied();
    let track = lookups.tracks.get(symbol).copied();
    let signal = lookups.signals.get(symbol).copied();
    let invalidation_rules = default_invalidation_rules(tactical_case, track, causal, pressure);
    let case_signature = live_case_signature(tactical_case);
    let archetype_projections = live_case_archetype_projections(tactical_case);
    let inferred_intent = tactical_case
        .inferred_intent
        .as_ref()
        .map(|intent| enrich_intent_with_temporal_opportunities(snapshot, tactical_case, intent));
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
        workflow_id: Some(
            crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                &tactical_case.setup_id,
            ),
        ),
        execution_policy: Some(default_execution_policy(&tactical_case.action)),
        owner: None,
        reviewer: None,
        queue_pin: None,
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
        governance: Some(ActionGovernanceContract::for_recommendation(
            default_execution_policy(&tactical_case.action),
        )),
        governance_bucket: governance_bucket_label(default_execution_policy(&tactical_case.action))
            .into(),
        governance_reason_code: Some(governance_reason_code(
            None,
            default_execution_policy(&tactical_case.action),
        )),
        governance_reason: Some(governance_reason(
            None,
            default_execution_policy(&tactical_case.action),
        )),
        market_regime_bias: snapshot.market_regime.bias.clone(),
        market_regime_confidence: snapshot.market_regime.confidence,
        market_breadth_delta: snapshot.market_regime.breadth_up
            - snapshot.market_regime.breadth_down,
        market_average_return: snapshot.market_regime.average_return,
        market_directional_consensus: snapshot.market_regime.directional_consensus,
        confidence: tactical_case.confidence,
        confidence_gap: tactical_case.confidence_gap,
        heuristic_edge: tactical_case.heuristic_edge,
        review_reason_code: tactical_case.review_reason_code.clone(),
        review_reason_family: tactical_case.review_reason_family.clone(),
        review_reason_subreasons: tactical_case.review_reason_subreasons.clone(),
        freshness_state: tactical_case.freshness_state.clone(),
        first_enter_tick: tactical_case.first_enter_tick,
        ticks_since_first_enter: tactical_case.ticks_since_first_enter,
        ticks_since_first_seen: tactical_case.ticks_since_first_seen,
        timing_state: tactical_case.timing_state.clone(),
        timing_position_in_range: tactical_case.timing_position_in_range,
        local_state: tactical_case.local_state.clone(),
        local_state_confidence: tactical_case.local_state_confidence,
        actionability_score: tactical_case.actionability_score,
        actionability_state: tactical_case.actionability_state.clone(),
        confidence_velocity_5t: tactical_case.confidence_velocity_5t,
        support_fraction_velocity_5t: tactical_case.support_fraction_velocity_5t,
        driver_confidence: tactical_case.driver_confidence,
        absence_summary: tactical_case.absence_summary.clone(),
        competition_summary: tactical_case.competition_summary.clone(),
        competition_winner: tactical_case.competition_winner.clone(),
        competition_runner_up: tactical_case.competition_runner_up.clone(),
        priority_rank: tactical_case.priority_rank,
        state_persistence_ticks: tactical_case.state_persistence_ticks,
        direction_stability_rounds: tactical_case.direction_stability_rounds,
        state_reason_codes: tactical_case.state_reason_codes.clone(),
        why_now: derive_why_now(tactical_case, chain, pressure, causal, track, signal),
        primary_lens: derive_primary_lens(
            snapshot,
            tactical_case,
            chain,
            pressure,
            causal,
            track,
            signal,
        ),
        primary_driver: chain.map(|item| item.primary_driver.clone()),
        family_label: tactical_case.family_label.clone(),
        counter_label: tactical_case.counter_label.clone(),
        hypothesis_status: track.map(|item| item.status.clone()),
        current_leader: causal.map(|item| item.current_leader.clone()),
        flip_count: causal.map(|item| item.flips).unwrap_or_default(),
        leader_streak: causal.map(|item| item.leader_streak),
        case_signature: case_signature.clone(),
        archetype_projections,
        intent_opportunities: inferred_intent
            .as_ref()
            .map(|intent| intent.opportunities.clone())
            .unwrap_or_default(),
        inferred_intent,
        expectation_binding_count: case_signature
            .as_ref()
            .map(|item| item.expectation_support)
            .unwrap_or(0),
        expectation_violation_count: case_signature
            .as_ref()
            .map(|item| item.expectation_violations)
            .unwrap_or(0),
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
        case_resolution: None,
        horizon_breakdown: None,
    }
}

fn live_case_signature(tactical_case: &LiveTacticalCase) -> Option<CaseSignature> {
    let mut channels = Vec::new();
    if tactical_case
        .entry_rationale
        .to_ascii_lowercase()
        .contains("volume")
    {
        channels.push(CaseChannel::Volume);
    }
    if tactical_case
        .entry_rationale
        .to_ascii_lowercase()
        .contains("flow")
    {
        channels.push(CaseChannel::CapitalFlow);
    }
    if tactical_case.tension_driver.is_some() {
        channels.push(CaseChannel::Propagation);
    }
    channels.sort_by_key(|channel| *channel as u8);
    channels.dedup();

    Some(CaseSignature {
        active_channels: channels,
        topology: if tactical_case.is_isolated == Some(true) {
            CaseTopology::Isolated
        } else {
            CaseTopology::SectorLinked
        },
        temporal_shape: match tactical_case.lifecycle_phase.as_deref() {
            Some("Growing") => CaseTemporalShape::Persistent,
            Some("Peaking") => CaseTemporalShape::Burst,
            Some("Fading") => CaseTemporalShape::Reversal,
            Some("New") => CaseTemporalShape::Drift,
            _ => CaseTemporalShape::Unknown,
        },
        conflict_shape: if tactical_case.is_isolated == Some(true) {
            ConflictShape::Contradictory
        } else {
            ConflictShape::Aligned
        },
        expectation_support: usize::from(tactical_case.tension_driver.is_some()),
        expectation_violations: usize::from(tactical_case.is_isolated == Some(true)),
        novelty_score: if tactical_case.is_isolated == Some(true) {
            rust_decimal::Decimal::new(7, 1)
        } else {
            rust_decimal::Decimal::new(4, 1)
        },
        notes: tactical_case
            .tension_driver
            .as_ref()
            .map(|driver| vec![format!("driver={driver}")])
            .unwrap_or_default(),
    })
}

fn live_case_archetype_projections(tactical_case: &LiveTacticalCase) -> Vec<ArchetypeProjection> {
    let mut projections = Vec::new();
    if let Some(label) = tactical_case.family_label.as_ref() {
        projections.push(ArchetypeProjection {
            archetype_key: label.to_ascii_lowercase().replace(' ', "_"),
            label: label.clone(),
            affinity: rust_decimal::Decimal::new(6, 1),
            rationale: "projected from tactical case family label".into(),
        });
    }
    if tactical_case.is_isolated == Some(true) {
        projections.push(ArchetypeProjection {
            archetype_key: "emergent".into(),
            label: "emergent pattern".into(),
            affinity: rust_decimal::Decimal::new(7, 1),
            rationale: "isolated tactical case behavior suggests an emergent archetype".into(),
        });
    }
    projections
}

fn enrich_intent_with_temporal_opportunities(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    intent: &IntentHypothesis,
) -> IntentHypothesis {
    let mut intent = intent.clone();
    intent.opportunities = build_intent_opportunities(snapshot, tactical_case, &intent);
    intent
}

fn build_intent_opportunities(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    intent: &IntentHypothesis,
) -> Vec<IntentOpportunityWindow> {
    let mut items = snapshot
        .temporal_bars
        .iter()
        .filter(|bar| bar.symbol == tactical_case.symbol)
        .filter(|bar| matches!(bar.horizon.as_str(), "5m" | "30m"))
        .map(|bar| opportunity_from_bar(bar, intent))
        .collect::<Vec<_>>();

    items.push(session_opportunity(snapshot, tactical_case, intent));
    items
}

fn opportunity_from_bar(
    bar: &crate::live_snapshot::LiveTemporalBar,
    intent: &IntentHypothesis,
) -> IntentOpportunityWindow {
    let directional_alignment = match intent.direction {
        IntentDirection::Buy => bar.composite_close,
        IntentDirection::Sell => -bar.composite_close,
        IntentDirection::Mixed => bar.composite_close.abs() / rust_decimal::Decimal::new(2, 0),
        IntentDirection::Neutral => rust_decimal::Decimal::ZERO,
    };
    let flow_alignment = match intent.direction {
        IntentDirection::Buy => bar.capital_flow_delta,
        IntentDirection::Sell => -bar.capital_flow_delta,
        IntentDirection::Mixed => bar.capital_flow_delta.abs() / rust_decimal::Decimal::new(2, 0),
        IntentDirection::Neutral => rust_decimal::Decimal::ZERO,
    };
    let persistence = rust_decimal::Decimal::from(bar.signal_persistence.min(6) as i64)
        / rust_decimal::Decimal::new(10, 1);
    let alignment =
        (directional_alignment + flow_alignment + persistence) / rust_decimal::Decimal::new(3, 0);
    let confidence =
        clamp_unit_interval(intent.confidence + alignment / rust_decimal::Decimal::new(2, 0));

    let bias = if alignment <= rust_decimal::Decimal::new(-2, 1) {
        IntentOpportunityBias::Exit
    } else if alignment >= rust_decimal::Decimal::new(5, 1) {
        IntentOpportunityBias::Enter
    } else if alignment >= rust_decimal::Decimal::new(2, 1) {
        IntentOpportunityBias::Hold
    } else {
        IntentOpportunityBias::Watch
    };

    {
        let bucket = HorizonBucket::from_legacy_string(&bar.horizon);
        let mut w = IntentOpportunityWindow::new(
            bucket,
            Urgency::Normal,
            bias,
            confidence,
            alignment,
            format!(
                "{} alignment {:.3}, persistence {}",
                bar.horizon, alignment, bar.signal_persistence
            ),
        );
        // Preserve the original bar horizon string for backward-compat filtering
        // (e.g., "5m", "30m") which may not round-trip through HorizonBucket.
        w.horizon = bar.horizon.clone();
        w
    }
}

fn session_opportunity(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    intent: &IntentHypothesis,
) -> IntentOpportunityWindow {
    let market_alignment = match intent.direction {
        IntentDirection::Buy => {
            snapshot.market_regime.breadth_up - snapshot.market_regime.breadth_down
        }
        IntentDirection::Sell => {
            snapshot.market_regime.breadth_down - snapshot.market_regime.breadth_up
        }
        IntentDirection::Mixed => rust_decimal::Decimal::ZERO,
        IntentDirection::Neutral => rust_decimal::Decimal::ZERO,
    };
    let intent_penalty = match intent.state {
        crate::ontology::IntentState::Invalidated => rust_decimal::Decimal::new(8, 1),
        crate::ontology::IntentState::Exhausted => rust_decimal::Decimal::new(5, 1),
        crate::ontology::IntentState::AtRisk => rust_decimal::Decimal::new(3, 1),
        _ => rust_decimal::Decimal::ZERO,
    };
    let alignment = market_alignment + intent.strength.composite - intent_penalty;
    let bias = if matches!(
        intent.state,
        crate::ontology::IntentState::Invalidated | crate::ontology::IntentState::Exhausted
    ) || alignment <= rust_decimal::Decimal::new(-2, 1)
    {
        IntentOpportunityBias::Exit
    } else if alignment >= rust_decimal::Decimal::new(6, 1) {
        IntentOpportunityBias::Enter
    } else if alignment >= rust_decimal::Decimal::new(3, 1) {
        IntentOpportunityBias::Hold
    } else {
        IntentOpportunityBias::Watch
    };

    IntentOpportunityWindow::new(
        HorizonBucket::Session,
        Urgency::Normal,
        bias,
        clamp_unit_interval(intent.confidence + alignment / rust_decimal::Decimal::new(3, 0)),
        alignment,
        format!(
            "session regime {} for {} with intent state {:?}",
            snapshot.market_regime.bias, tactical_case.symbol, intent.state
        ),
    )
}

pub(crate) fn case_priority(case: &CaseSummary) -> i32 {
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

pub(crate) fn case_structure_priority(case: &CaseSummary) -> i32 {
    let novelty = case
        .case_signature
        .as_ref()
        .map(|signature| signature.novelty_score)
        .unwrap_or(rust_decimal::Decimal::ZERO);
    let violation_count = case.expectation_violation_count as i32;
    let emergent_bonus = case
        .archetype_projections
        .iter()
        .any(|projection| projection.archetype_key == "emergent") as i32;

    let novelty_bucket = if novelty >= rust_decimal::Decimal::new(7, 1) {
        3
    } else if novelty >= rust_decimal::Decimal::new(5, 1) {
        2
    } else if novelty > rust_decimal::Decimal::ZERO {
        1
    } else {
        0
    };

    emergent_bonus * 10 + violation_count * 3 + novelty_bucket
}

pub(crate) fn case_structure_priority_with_feedback(
    case: &CaseSummary,
    feedback: Option<&ReasoningLearningFeedback>,
) -> i32 {
    let mut score = case_structure_priority(case);
    let Some(feedback) = feedback else {
        return score;
    };

    for projection in &case.archetype_projections {
        let delta = feedback.archetype_delta(&projection.archetype_key);
        if delta > rust_decimal::Decimal::ZERO {
            score += 2;
        } else if delta < rust_decimal::Decimal::ZERO {
            score -= 2;
        }
    }

    if let Some(signature) = case.case_signature.as_ref() {
        let signature_match = feedback
            .signature_adjustments
            .iter()
            .find(|item| {
                item.topology == format!("{:?}", signature.topology).to_ascii_lowercase()
                    && item.temporal_shape
                        == format!("{:?}", signature.temporal_shape).to_ascii_lowercase()
                    && item.conflict_shape
                        == format!("{:?}", signature.conflict_shape).to_ascii_lowercase()
            })
            .map(|item| item.delta)
            .unwrap_or(rust_decimal::Decimal::ZERO);
        if signature_match > rust_decimal::Decimal::ZERO {
            score += 1;
        } else if signature_match < rust_decimal::Decimal::ZERO {
            score -= 1;
        }
    }

    if case.expectation_violation_count > 0 {
        let violation_delta = feedback.expectation_violation_delta("missingpropagation")
            + feedback.expectation_violation_delta("unexpectedpropagation")
            + feedback.expectation_violation_delta("failedconfirmation")
            + feedback.expectation_violation_delta("modalconflict")
            + feedback.expectation_violation_delta("timingmismatch");
        if violation_delta > rust_decimal::Decimal::ZERO {
            score += 1;
        } else if violation_delta < rust_decimal::Decimal::ZERO {
            score -= 1;
        }
    }

    score
}

fn case_intent_priority(case: &CaseSummary) -> i32 {
    let Some(intent) = case.inferred_intent.as_ref() else {
        return 0;
    };

    let state_score = match intent.state {
        crate::ontology::IntentState::Active => 4,
        crate::ontology::IntentState::Forming => 3,
        crate::ontology::IntentState::AtRisk => 1,
        crate::ontology::IntentState::Exhausted => -2,
        crate::ontology::IntentState::Invalidated => -4,
        crate::ontology::IntentState::Fulfilled => -3,
        crate::ontology::IntentState::Unknown => 0,
    };

    let kind_score = match intent.kind {
        IntentKind::FailedPropagation => 12,
        IntentKind::CrossMarketLead => 10,
        IntentKind::EventRepricing => 9,
        IntentKind::ForcedUnwind => 8,
        IntentKind::Absorption => 7,
        IntentKind::Accumulation | IntentKind::Distribution => 6,
        IntentKind::PassiveRebalance => 3,
        IntentKind::Unknown => 0,
    };

    let strength_score = if intent.strength.composite >= rust_decimal::Decimal::new(8, 1) {
        4
    } else if intent.strength.composite >= rust_decimal::Decimal::new(6, 1) {
        3
    } else if intent.strength.composite >= rust_decimal::Decimal::new(4, 1) {
        2
    } else if intent.strength.composite > rust_decimal::Decimal::ZERO {
        1
    } else {
        0
    };

    let conflict_score = if intent.conflict_score >= rust_decimal::Decimal::new(7, 1) {
        2
    } else if intent.conflict_score >= rust_decimal::Decimal::new(4, 1) {
        1
    } else {
        0
    };

    kind_score + strength_score + conflict_score + state_score + intent_opportunity_priority(intent)
}

fn summarize_briefing_archetypes(cases: &[CaseSummary]) -> Vec<String> {
    let mut grouped = std::collections::HashMap::<String, (usize, rust_decimal::Decimal)>::new();
    for case in cases {
        for projection in &case.archetype_projections {
            let entry = grouped
                .entry(projection.label.clone())
                .or_insert((0, rust_decimal::Decimal::ZERO));
            entry.0 += 1;
            entry.1 += projection.affinity;
        }
    }

    let mut items = grouped
        .into_iter()
        .map(|(label, (count, affinity_sum))| {
            let mean_affinity = if count == 0 {
                rust_decimal::Decimal::ZERO
            } else {
                affinity_sum / rust_decimal::Decimal::from(count as i64)
            };
            format!("{} ×{} (affinity {:.2})", label, count, mean_affinity)
        })
        .collect::<Vec<_>>();
    items.sort();
    items.truncate(5);
    items
}

fn summarize_briefing_intents(cases: &[CaseSummary]) -> Vec<String> {
    let mut grouped = std::collections::HashMap::<
        String,
        (usize, rust_decimal::Decimal, rust_decimal::Decimal),
    >::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        let label = format!("{:?}", intent.kind)
            .to_ascii_lowercase()
            .replace('_', " ");
        let entry = grouped.entry(label).or_insert((
            0,
            rust_decimal::Decimal::ZERO,
            rust_decimal::Decimal::ZERO,
        ));
        entry.0 += 1;
        entry.1 += intent.strength.composite;
        entry.2 += intent.conflict_score;
    }

    let mut items = grouped
        .into_iter()
        .map(|(label, (count, strength_sum, conflict_sum))| {
            let denom = rust_decimal::Decimal::from(count as i64);
            let mean_strength = strength_sum / denom;
            let mean_conflict = conflict_sum / denom;
            format!(
                "{} ×{} (strength {:.2}, conflict {:.2})",
                label, count, mean_strength, mean_conflict
            )
        })
        .collect::<Vec<_>>();
    items.sort();
    items.truncate(5);
    items
}

fn summarize_briefing_opportunities(cases: &[CaseSummary]) -> Vec<String> {
    let mut grouped = std::collections::HashMap::<
        (String, String),
        (usize, rust_decimal::Decimal, rust_decimal::Decimal),
    >::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        for opportunity in &intent.opportunities {
            let key = (
                opportunity.horizon.clone(),
                format!("{:?}", opportunity.bias).to_ascii_lowercase(),
            );
            let entry = grouped.entry(key).or_insert((
                0,
                rust_decimal::Decimal::ZERO,
                rust_decimal::Decimal::ZERO,
            ));
            entry.0 += 1;
            entry.1 += opportunity.confidence;
            entry.2 += opportunity.alignment;
        }
    }

    let mut items = grouped
        .into_iter()
        .map(
            |((horizon, bias), (count, confidence_sum, alignment_sum))| {
                let denom = rust_decimal::Decimal::from(count as i64);
                (
                    count,
                    format!(
                        "{} {} ×{} (conf {:.2}, align {:.2})",
                        horizon,
                        bias,
                        count,
                        confidence_sum / denom,
                        alignment_sum / denom
                    ),
                )
            },
        )
        .collect::<Vec<_>>();
    items.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    items.into_iter().map(|(_, label)| label).take(5).collect()
}

fn intent_opportunity_priority(intent: &crate::ontology::IntentHypothesis) -> i32 {
    intent
        .opportunities
        .iter()
        .map(|window| {
            let bias_score = match window.bias {
                IntentOpportunityBias::Enter => 4,
                IntentOpportunityBias::Hold => 2,
                IntentOpportunityBias::Watch => 0,
                IntentOpportunityBias::Exit => -3,
            };
            let horizon_score = match window.horizon.as_str() {
                "5m" => 2,
                "30m" => 1,
                "session" => 1,
                _ => 0,
            };
            let confidence_score = if window.confidence >= rust_decimal::Decimal::new(8, 1) {
                2
            } else if window.confidence >= rust_decimal::Decimal::new(6, 1) {
                1
            } else {
                0
            };
            bias_score + horizon_score + confidence_score
        })
        .max()
        .unwrap_or(0)
}

// 2026-04-29: deleted `apply_case_structure_feedback` and
// `apply_discovered_archetype_memory`. Both were case-level mirrors of
// the deleted setup-level `apply_feedback_to_tactical_setup` rogue
// modulator — same 5-channel weighted-sum pattern with magic constants
// (0.3 / 0.2 / 0.1 / 0.2 / 0.2 + 0.6 hit-rate threshold + 0.2 cap)
// overwriting `case.confidence` after BP fusion. They escaped the
// architecture invariants test because:
//   1. The test only scans `pipeline/` + `runtime/`, not `cases/`.
//   2. The test bans by NAME (`apply_*_modulation`), not by BEHAVIOR.
// Audit findings CRITICAL #1 + #2 from 漏網之魚 sweep 2026-04-29.

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
    build_summary_reasoning_profile_with_adjustments(
        snapshot,
        tactical_case,
        chain,
        pressure,
        signal,
        causal,
        track,
        workflow_state,
        workflow_note,
        invalidation_rules,
        &std::collections::HashMap::new(),
    )
}

fn build_summary_reasoning_profile_with_adjustments(
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
    factor_adjustments: &std::collections::HashMap<(String, String), rust_decimal::Decimal>,
) -> CaseReasoningProfile {
    let cross_market_signals = relevant_cross_market_signals(snapshot, &tactical_case.symbol);
    let cross_market_anomalies = relevant_cross_market_anomalies(snapshot, &tactical_case.symbol);
    let predicates = derive_atomic_predicates(&PredicateInputs {
        tactical_case,
        active_positions: &snapshot.active_position_nodes,
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
    build_reasoning_profile_with_adjustments(
        &predicates,
        invalidation_rules,
        human_review,
        factor_adjustments,
    )
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

fn default_execution_policy(action: &str) -> ActionExecutionPolicy {
    match action {
        "enter" => ActionExecutionPolicy::ReviewRequired,
        _ => ActionExecutionPolicy::ManualOnly,
    }
}

fn count_cases_by_policy(cases: &[CaseSummary], policy: ActionExecutionPolicy) -> usize {
    cases
        .iter()
        .filter(|case| case.execution_policy == Some(policy))
        .count()
}

fn build_case_governance_buckets(cases: &[CaseSummary]) -> CaseGovernanceBuckets {
    let mut buckets = CaseGovernanceBuckets::default();
    for case in cases {
        match case
            .execution_policy
            .unwrap_or(ActionExecutionPolicy::ReviewRequired)
        {
            ActionExecutionPolicy::ManualOnly => buckets.manual_only.push(case.clone()),
            ActionExecutionPolicy::ReviewRequired => buckets.review_required.push(case.clone()),
            ActionExecutionPolicy::AutoEligible => buckets.auto_eligible.push(case.clone()),
        }
    }
    buckets
}

fn build_case_governance_reason_buckets(cases: &[CaseSummary]) -> CaseGovernanceReasonBuckets {
    let mut buckets = BTreeMap::new();
    for case in cases {
        let code = case
            .governance_reason_code
            .unwrap_or_else(|| inferred_governance_reason_code(case));
        buckets
            .entry(code)
            .or_insert_with(Vec::new)
            .push(case.clone());
    }

    CaseGovernanceReasonBuckets { buckets }
}

fn build_case_primary_lens_buckets(cases: &[CaseSummary]) -> CasePrimaryLensBuckets {
    let mut buckets = BTreeMap::new();
    for case in cases {
        let key = case
            .primary_lens
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "unknown".into());
        buckets
            .entry(key)
            .or_insert_with(Vec::new)
            .push(case.clone());
    }

    CasePrimaryLensBuckets { buckets }
}

fn build_case_queue_pin_buckets(cases: &[CaseSummary]) -> CaseQueuePinBuckets {
    let mut buckets = CaseQueuePinBuckets::default();
    for case in cases {
        if case
            .queue_pin
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
        {
            buckets.pinned.push(case.clone());
        } else {
            buckets.unpinned.push(case.clone());
        }
    }
    buckets
}

fn governance_bucket_label(policy: ActionExecutionPolicy) -> &'static str {
    match policy {
        ActionExecutionPolicy::ManualOnly => "manual_only",
        ActionExecutionPolicy::ReviewRequired => "review_required",
        ActionExecutionPolicy::AutoEligible => "auto_eligible",
    }
}

fn count_queue_pinned_cases(cases: &[CaseSummary]) -> usize {
    cases
        .iter()
        .filter(|case| {
            case.queue_pin
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some()
        })
        .count()
}

fn inferred_governance_reason_code(case: &CaseSummary) -> ActionGovernanceReasonCode {
    case.governance_reason_code.unwrap_or_else(|| {
        let policy = case
            .execution_policy
            .unwrap_or(ActionExecutionPolicy::ReviewRequired);
        governance_reason_code(None, policy)
    })
}

fn derive_primary_lens(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    chain: Option<&LiveBackwardChain>,
    pressure: Option<&LivePressure>,
    causal: Option<&LiveCausalLeader>,
    track: Option<&LiveHypothesisTrack>,
    signal: Option<&LiveSignal>,
) -> Option<String> {
    let symbol = tactical_case.symbol.as_str();
    if snapshot
        .events
        .iter()
        .any(|event| event.kind == "IcebergDetected" && event.symbol.as_deref() == Some(symbol))
    {
        return Some("iceberg".into());
    }
    if chain.is_some() {
        return Some("causal".into());
    }
    if track.is_some() || pressure.is_some() || causal.is_some() || signal.is_some() {
        return Some("structural".into());
    }
    if tactical_case.family_label.is_some() && !snapshot.lineage.is_empty() {
        return Some("lineage_prior".into());
    }
    None
}

fn case_governance_reason_priority(left: &CaseSummary, right: &CaseSummary) -> std::cmp::Ordering {
    governance_reason_rank(inferred_governance_reason_code(left))
        .cmp(&governance_reason_rank(inferred_governance_reason_code(
            right,
        )))
        .then_with(|| right.confidence.cmp(&left.confidence))
        .then_with(|| left.symbol.cmp(&right.symbol))
}

fn governance_reason_rank(code: ActionGovernanceReasonCode) -> i32 {
    match code {
        ActionGovernanceReasonCode::SeverityRequiresReview => 0,
        ActionGovernanceReasonCode::InvalidationRuleMissing => 1,
        ActionGovernanceReasonCode::NonPositiveExpectedAlpha => 2,
        ActionGovernanceReasonCode::AssignmentLockedDuringExecution => 3,
        ActionGovernanceReasonCode::WorkflowTransitionWindow => 4,
        ActionGovernanceReasonCode::TerminalReviewStage => 5,
        ActionGovernanceReasonCode::OperatorActionRequired => 6,
        ActionGovernanceReasonCode::AdvisoryAction => 7,
        ActionGovernanceReasonCode::AutoExecutionEligible => 8,
        ActionGovernanceReasonCode::WorkflowNotCreated => 9,
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

pub(super) fn ordered_unique(values: Vec<String>) -> Vec<String> {
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
