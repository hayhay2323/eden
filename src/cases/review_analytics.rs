use rust_decimal::Decimal;
use std::collections::HashMap;
#[cfg(feature = "persistence")]
use std::collections::HashSet;

#[cfg(feature = "persistence")]
use crate::live_snapshot::LiveMarket;
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
use crate::pipeline::learning_loop::ReasoningLearningFeedback;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::{
    derive_learning_feedback, derive_outcome_learning_context_from_case_outcomes,
    derive_outcome_learning_context_from_hk_rows, derive_outcome_learning_context_from_us_rows,
};

#[cfg(feature = "persistence")]
use super::builders::{
    case_priority, case_structure_priority, case_structure_priority_with_feedback,
};
#[cfg(feature = "persistence")]
use super::io::CaseError;
#[cfg(feature = "persistence")]
use super::reasoning_story::{build_invalidation_patterns, build_mechanism_transition_analytics};
#[cfg(feature = "persistence")]
use super::types::CaseLensRegimeHitRateStat;
#[cfg(feature = "persistence")]
use super::types::{
    CaseHumanReviewReasonStat, CaseMechanismDriftPoint, CaseReviewReasonFeedbackStat,
    CaseReviewerCorrectionStat, CaseReviewerDoctrineStat,
};
use super::types::{
    CaseIntelligenceSignals, CaseIntentExitSignalStat, CaseIntentOpportunityStat, CaseIntentStat,
    CaseIntentStateStat, CaseLensStat, CaseMechanismStat, CaseReviewAnalytics, CaseSummary,
};
#[cfg(feature = "persistence")]
use super::types::{
    CaseIntentAdjustmentStat, CaseMemoryImpactStat, CaseViolationPredictivenessStat,
};

pub(super) fn build_case_review_analytics(cases: &[CaseSummary]) -> CaseReviewAnalytics {
    CaseReviewAnalytics {
        mechanism_stats: build_mechanism_stats(cases),
        intent_stats: build_intent_stats(cases),
        intent_state_stats: build_intent_state_stats(cases),
        intent_exit_signal_stats: build_intent_exit_signal_stats(cases),
        intent_opportunity_stats: build_intent_opportunity_stats(cases),
        intent_adjustments: Vec::new(),
        review_required_by_lens: build_review_required_by_lens(cases),
        human_override_by_lens: Vec::new(),
        lens_regime_hit_rates: Vec::new(),
        archetype_stats: Vec::new(),
        discovered_archetype_catalog: Vec::new(),
        signature_stats: Vec::new(),
        expectation_violation_stats: Vec::new(),
        intelligence_signals: CaseIntelligenceSignals::default(),
        memory_impact: Vec::new(),
        violation_predictiveness: Vec::new(),
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
        review_reason_feedback: Vec::new(),
        review_reason_family_feedback: Vec::new(),
        learning_feedback: ReasoningLearningFeedback::default(),
    }
}

#[cfg(feature = "persistence")]
pub(super) fn build_case_review_analytics_with_assessments(
    cases: &[CaseSummary],
    assessments: &[CaseReasoningAssessmentRecord],
    case_outcomes: &[CaseRealizedOutcomeRecord],
    discovered_archetypes: &[crate::persistence::discovered_archetype::DiscoveredArchetypeRecord],
    outcome_context: crate::pipeline::learning_loop::OutcomeLearningContext,
) -> CaseReviewAnalytics {
    let learning_feedback = derive_learning_feedback(assessments, &outcome_context);
    let memory_impact = build_memory_impact_stats(cases, &learning_feedback);
    let violation_predictiveness = build_violation_predictiveness(assessments, case_outcomes);
    let (
        mechanism_transition_breakdown,
        transition_by_sector,
        transition_by_regime,
        transition_by_reviewer,
        recent_mechanism_transitions,
    ) = build_mechanism_transition_analytics(cases, assessments);
    CaseReviewAnalytics {
        mechanism_stats: build_mechanism_stats(cases),
        intent_stats: build_intent_stats(cases),
        intent_state_stats: build_intent_state_stats(cases),
        intent_exit_signal_stats: build_intent_exit_signal_stats(cases),
        intent_opportunity_stats: build_intent_opportunity_stats(cases),
        intent_adjustments: build_intent_adjustments(&learning_feedback),
        review_required_by_lens: build_review_required_by_lens(cases),
        human_override_by_lens: build_human_override_by_lens(cases, assessments),
        lens_regime_hit_rates: build_lens_regime_hit_rates(case_outcomes),
        archetype_stats: build_archetype_stats(assessments),
        discovered_archetype_catalog: build_discovered_archetype_catalog(discovered_archetypes),
        signature_stats: build_signature_stats(assessments),
        expectation_violation_stats: build_expectation_violation_stats(assessments),
        intelligence_signals: build_intelligence_signals(
            cases,
            discovered_archetypes,
            &memory_impact,
            &violation_predictiveness,
        ),
        memory_impact,
        violation_predictiveness,
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
        review_reason_feedback: build_review_reason_feedback(assessments, case_outcomes),
        review_reason_family_feedback: build_review_reason_family_feedback(
            assessments,
            case_outcomes,
        ),
        learning_feedback,
    }
}

#[cfg(feature = "persistence")]
fn build_review_reason_feedback(
    assessments: &[CaseReasoningAssessmentRecord],
    case_outcomes: &[CaseRealizedOutcomeRecord],
) -> Vec<CaseReviewReasonFeedbackStat> {
    let mut latest_by_setup_reason =
        HashMap::<(String, String), &CaseReasoningAssessmentRecord>::new();
    for assessment in assessments
        .iter()
        .filter(|item| item.source == "runtime")
        .filter(|item| {
            item.review_reason_code
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some()
        })
    {
        let code = assessment
            .review_reason_code
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        let key = (assessment.setup_id.clone(), code);
        match latest_by_setup_reason.get(&key) {
            Some(existing) if existing.recorded_at >= assessment.recorded_at => {}
            _ => {
                latest_by_setup_reason.insert(key, assessment);
            }
        }
    }

    let mut latest_outcome_by_setup = HashMap::<&str, &CaseRealizedOutcomeRecord>::new();
    for outcome in case_outcomes {
        match latest_outcome_by_setup.get(outcome.setup_id.as_str()) {
            Some(existing) if existing.resolved_at >= outcome.resolved_at => {}
            _ => {
                latest_outcome_by_setup.insert(outcome.setup_id.as_str(), outcome);
            }
        }
    }

    let mut grouped = HashMap::<String, (usize, usize, usize, usize, Decimal)>::new();
    for ((setup_id, reason), _) in latest_by_setup_reason {
        let entry = grouped.entry(reason).or_insert((0, 0, 0, 0, Decimal::ZERO));
        entry.0 += 1; // blocked_count
        if let Some(outcome) = latest_outcome_by_setup.get(setup_id.as_str()) {
            entry.1 += 1; // resolved_count
            if outcome.followed_through && outcome.net_return > Decimal::ZERO {
                entry.2 += 1; // post_block_hits
            }
            if outcome.invalidated {
                entry.3 += 1; // invalidated_count
            }
            entry.4 += outcome.net_return; // total net return
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(
                review_reason_code,
                (blocked_count, resolved_count, post_block_hits, invalidated_count, total_return),
            )| CaseReviewReasonFeedbackStat {
                review_reason_code,
                blocked_count,
                resolved_count,
                post_block_hits,
                post_block_hit_rate: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    Decimal::from(post_block_hits as i64) / Decimal::from(resolved_count as i64)
                },
                invalidation_rate: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    Decimal::from(invalidated_count as i64) / Decimal::from(resolved_count as i64)
                },
                mean_net_return: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    total_return / Decimal::from(resolved_count as i64)
                },
            },
        )
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .blocked_count
            .cmp(&left.blocked_count)
            .then_with(|| right.resolved_count.cmp(&left.resolved_count))
            .then_with(|| right.post_block_hit_rate.cmp(&left.post_block_hit_rate))
            .then_with(|| left.review_reason_code.cmp(&right.review_reason_code))
    });
    stats.truncate(12);
    stats
}

#[cfg(feature = "persistence")]
fn build_review_reason_family_feedback(
    assessments: &[CaseReasoningAssessmentRecord],
    case_outcomes: &[CaseRealizedOutcomeRecord],
) -> Vec<CaseReviewReasonFeedbackStat> {
    let mut latest_by_setup_family =
        HashMap::<(String, String), &CaseReasoningAssessmentRecord>::new();
    for assessment in assessments
        .iter()
        .filter(|item| item.source == "runtime")
        .filter_map(|item| {
            item.review_reason_code
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|code| {
                    (
                        item,
                        crate::live_snapshot::consolidated_review_reason_family(code),
                    )
                })
        })
    {
        let (assessment, family) = assessment;
        let key = (assessment.setup_id.clone(), family.to_string());
        match latest_by_setup_family.get(&key) {
            Some(existing) if existing.recorded_at >= assessment.recorded_at => {}
            _ => {
                latest_by_setup_family.insert(key, assessment);
            }
        }
    }

    let mut latest_outcome_by_setup = HashMap::<&str, &CaseRealizedOutcomeRecord>::new();
    for outcome in case_outcomes {
        match latest_outcome_by_setup.get(outcome.setup_id.as_str()) {
            Some(existing) if existing.resolved_at >= outcome.resolved_at => {}
            _ => {
                latest_outcome_by_setup.insert(outcome.setup_id.as_str(), outcome);
            }
        }
    }

    let mut grouped = HashMap::<String, (usize, usize, usize, usize, Decimal)>::new();
    for ((setup_id, family), _) in latest_by_setup_family {
        let entry = grouped.entry(family).or_insert((0, 0, 0, 0, Decimal::ZERO));
        entry.0 += 1;
        if let Some(outcome) = latest_outcome_by_setup.get(setup_id.as_str()) {
            entry.1 += 1;
            if outcome.followed_through && outcome.net_return > Decimal::ZERO {
                entry.2 += 1;
            }
            if outcome.invalidated {
                entry.3 += 1;
            }
            entry.4 += outcome.net_return;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(
                review_reason_code,
                (blocked_count, resolved_count, post_block_hits, invalidated_count, total_return),
            )| CaseReviewReasonFeedbackStat {
                review_reason_code,
                blocked_count,
                resolved_count,
                post_block_hits,
                post_block_hit_rate: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    Decimal::from(post_block_hits as i64) / Decimal::from(resolved_count as i64)
                },
                invalidation_rate: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    Decimal::from(invalidated_count as i64) / Decimal::from(resolved_count as i64)
                },
                mean_net_return: if resolved_count == 0 {
                    Decimal::ZERO
                } else {
                    total_return / Decimal::from(resolved_count as i64)
                },
            },
        )
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .blocked_count
            .cmp(&left.blocked_count)
            .then_with(|| right.resolved_count.cmp(&left.resolved_count))
            .then_with(|| right.post_block_hit_rate.cmp(&left.post_block_hit_rate))
            .then_with(|| left.review_reason_code.cmp(&right.review_reason_code))
    });
    stats.truncate(12);
    stats
}

#[cfg(feature = "persistence")]
fn build_memory_impact_stats(
    cases: &[CaseSummary],
    feedback: &ReasoningLearningFeedback,
) -> Vec<CaseMemoryImpactStat> {
    let mut baseline = cases.to_vec();
    baseline.sort_by(|left, right| {
        case_priority(left)
            .cmp(&case_priority(right))
            .then_with(|| case_structure_priority(right).cmp(&case_structure_priority(left)))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    let baseline_rank = baseline
        .iter()
        .enumerate()
        .map(|(idx, case)| (case.setup_id.clone(), idx + 1))
        .collect::<HashMap<_, _>>();

    let baseline_case = baseline
        .iter()
        .map(|case| (case.setup_id.clone(), case.clone()))
        .collect::<HashMap<_, _>>();

    let mut adjusted = cases.to_vec();
    // 2026-04-29: removed apply_case_structure_feedback +
    // apply_discovered_archetype_memory. Same 5-channel rogue-modulator
    // pattern as the deleted setup-level apply_feedback_to_tactical_setup
    // — magic 0.3/0.2/0.1/0.2/0.2 weights on case.confidence post-BP.
    // Audit finding CRITICAL #1 + #2 from 漏網之魚 sweep.
    adjusted.sort_by(|left, right| {
        case_priority(left)
            .cmp(&case_priority(right))
            .then_with(|| {
                case_structure_priority_with_feedback(right, Some(feedback))
                    .cmp(&case_structure_priority_with_feedback(left, Some(feedback)))
            })
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    let mut impact = adjusted
        .iter()
        .enumerate()
        .filter_map(|(idx, case)| {
            let baseline = baseline_case.get(&case.setup_id)?;
            let baseline_rank = baseline_rank
                .get(&case.setup_id)
                .copied()
                .unwrap_or(idx + 1);
            let adjusted_rank = idx + 1;
            let confidence_delta = case.confidence - baseline.confidence;
            let edge_delta = case.heuristic_edge - baseline.heuristic_edge;
            let baseline_priority = case_structure_priority(baseline);
            let adjusted_priority = case_structure_priority_with_feedback(case, Some(feedback));

            if baseline_rank == adjusted_rank
                && confidence_delta == Decimal::ZERO
                && edge_delta == Decimal::ZERO
            {
                return None;
            }

            Some(CaseMemoryImpactStat {
                setup_id: case.setup_id.clone(),
                symbol: case.symbol.clone(),
                baseline_rank,
                adjusted_rank,
                baseline_structure_priority: baseline_priority,
                adjusted_structure_priority: adjusted_priority,
                confidence_delta,
                edge_delta,
                archetypes: case
                    .archetype_projections
                    .iter()
                    .map(|projection| projection.label.clone())
                    .collect(),
            })
        })
        .collect::<Vec<_>>();

    impact.sort_by(|left, right| {
        let left_rank_shift = left.baseline_rank.abs_diff(left.adjusted_rank);
        let right_rank_shift = right.baseline_rank.abs_diff(right.adjusted_rank);
        right_rank_shift
            .cmp(&left_rank_shift)
            .then_with(|| {
                right
                    .confidence_delta
                    .abs()
                    .cmp(&left.confidence_delta.abs())
            })
            .then_with(|| right.edge_delta.abs().cmp(&left.edge_delta.abs()))
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    impact.truncate(12);
    impact
}

#[cfg(feature = "persistence")]
fn build_violation_predictiveness(
    assessments: &[CaseReasoningAssessmentRecord],
    outcomes: &[CaseRealizedOutcomeRecord],
) -> Vec<CaseViolationPredictivenessStat> {
    let latest_assessment_by_setup = assessments
        .iter()
        .filter(|assessment| assessment.source == "runtime")
        .fold(
            HashMap::<&str, &CaseReasoningAssessmentRecord>::new(),
            |mut acc, assessment| {
                match acc.get(assessment.setup_id.as_str()) {
                    Some(existing) if existing.recorded_at >= assessment.recorded_at => {}
                    _ => {
                        acc.insert(assessment.setup_id.as_str(), assessment);
                    }
                }
                acc
            },
        );

    let mut grouped = HashMap::<String, (usize, usize, Decimal)>::new();
    for outcome in outcomes {
        let Some(assessment) = latest_assessment_by_setup
            .get(outcome.setup_id.as_str())
            .copied()
        else {
            continue;
        };
        let hit = usize::from(outcome.followed_through && outcome.net_return > Decimal::ZERO);
        for violation in &assessment.expectation_violations {
            let key = format!("{:?}", violation.kind).to_ascii_lowercase();
            let entry = grouped.entry(key).or_insert((0, 0, Decimal::ZERO));
            entry.0 += 1;
            entry.1 += hit;
            entry.2 += outcome.net_return;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(|(kind, (samples, hits, total_net_return))| {
            let denom = Decimal::from(samples.max(1) as i64);
            CaseViolationPredictivenessStat {
                kind,
                samples,
                hits,
                hit_rate: Decimal::from(hits as i64) / denom,
                mean_net_return: total_net_return / denom,
            }
        })
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| right.hit_rate.cmp(&left.hit_rate))
            .then_with(|| right.mean_net_return.cmp(&left.mean_net_return))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    stats.truncate(8);
    stats
}

#[cfg(feature = "persistence")]
fn build_intelligence_signals(
    cases: &[CaseSummary],
    discovered_archetypes: &[crate::persistence::discovered_archetype::DiscoveredArchetypeRecord],
    memory_impact: &[CaseMemoryImpactStat],
    violation_predictiveness: &[CaseViolationPredictivenessStat],
) -> CaseIntelligenceSignals {
    CaseIntelligenceSignals {
        memory_impacted_cases: memory_impact.len(),
        reprioritized_cases: memory_impact
            .iter()
            .filter(|item| item.baseline_rank != item.adjusted_rank)
            .count(),
        stable_archetypes: discovered_archetypes
            .iter()
            .filter(|item| item.samples >= 3)
            .filter(|item| {
                item.hit_rate >= Decimal::new(55, 2) || item.mean_net_return > Decimal::ZERO
            })
            .count(),
        predictive_violation_kinds: violation_predictiveness
            .iter()
            .filter(|item| item.samples >= 3)
            .filter(|item| {
                item.hit_rate >= Decimal::new(55, 2) || item.mean_net_return > Decimal::ZERO
            })
            .count(),
        emergent_cases: cases
            .iter()
            .filter(|case| {
                case.archetype_projections
                    .iter()
                    .any(|projection| projection.archetype_key == "emergent")
            })
            .count(),
    }
}

#[cfg(feature = "persistence")]
pub(super) async fn load_outcome_learning_context(
    store: &EdenStore,
    market: LiveMarket,
) -> Result<crate::pipeline::learning_loop::OutcomeLearningContext, CaseError> {
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

fn build_intent_stats(cases: &[CaseSummary]) -> Vec<CaseIntentStat> {
    let mut grouped: HashMap<String, Vec<&CaseSummary>> = HashMap::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        grouped
            .entry(format!("{:?}", intent.kind).to_ascii_lowercase())
            .or_default()
            .push(case);
    }

    let mut stats = grouped
        .into_iter()
        .map(|(intent, items)| {
            let mut buy_cases = 0usize;
            let mut sell_cases = 0usize;
            let mut total_confidence = Decimal::ZERO;
            let mut total_strength = Decimal::ZERO;

            for case in &items {
                let intent_model = case.inferred_intent.as_ref().expect("grouped with intent");
                match intent_model.direction {
                    crate::ontology::IntentDirection::Buy => buy_cases += 1,
                    crate::ontology::IntentDirection::Sell => sell_cases += 1,
                    _ => {}
                }
                total_confidence += intent_model.confidence;
                total_strength += intent_model.strength.composite;
            }

            let denom = Decimal::from(items.len() as i64);
            CaseIntentStat {
                intent,
                cases: items.len(),
                buy_cases,
                sell_cases,
                mean_confidence: total_confidence / denom,
                mean_strength: total_strength / denom,
            }
        })
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_strength.cmp(&left.mean_strength))
            .then_with(|| left.intent.cmp(&right.intent))
    });
    stats.truncate(8);
    stats
}

fn build_intent_state_stats(cases: &[CaseSummary]) -> Vec<CaseIntentStateStat> {
    let mut counts = HashMap::<String, usize>::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        *counts
            .entry(format!("{:?}", intent.state).to_ascii_lowercase())
            .or_insert(0) += 1;
    }

    let mut stats = counts
        .into_iter()
        .map(|(state, cases)| CaseIntentStateStat { state, cases })
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| left.state.cmp(&right.state))
    });
    stats
}

fn build_intent_exit_signal_stats(cases: &[CaseSummary]) -> Vec<CaseIntentExitSignalStat> {
    let mut grouped = HashMap::<String, (usize, Decimal)>::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        for signal in &intent.exit_signals {
            let key = format!("{:?}", signal.kind).to_ascii_lowercase();
            let entry = grouped.entry(key).or_insert((0, Decimal::ZERO));
            entry.0 += 1;
            entry.1 += signal.confidence;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(kind, (cases, total_confidence))| CaseIntentExitSignalStat {
                kind,
                cases,
                mean_confidence: total_confidence / Decimal::from(cases.max(1) as i64),
            },
        )
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_confidence.cmp(&left.mean_confidence))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    stats.truncate(8);
    stats
}

fn build_intent_opportunity_stats(cases: &[CaseSummary]) -> Vec<CaseIntentOpportunityStat> {
    let mut grouped = HashMap::<(String, String), (usize, Decimal, Decimal)>::new();
    for case in cases {
        let Some(intent) = case.inferred_intent.as_ref() else {
            continue;
        };
        for opportunity in &intent.opportunities {
            let key = (
                opportunity.horizon.clone(),
                format!("{:?}", opportunity.bias).to_ascii_lowercase(),
            );
            let entry = grouped
                .entry(key)
                .or_insert((0, Decimal::ZERO, Decimal::ZERO));
            entry.0 += 1;
            entry.1 += opportunity.confidence;
            entry.2 += opportunity.alignment;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |((horizon, bias), (cases, confidence_sum, alignment_sum))| {
                let denom = Decimal::from(cases.max(1) as i64);
                CaseIntentOpportunityStat {
                    horizon,
                    bias,
                    cases,
                    mean_confidence: confidence_sum / denom,
                    mean_alignment: alignment_sum / denom,
                }
            },
        )
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_confidence.cmp(&left.mean_confidence))
            .then_with(|| left.horizon.cmp(&right.horizon))
            .then_with(|| left.bias.cmp(&right.bias))
    });
    stats.truncate(10);
    stats
}

#[cfg(feature = "persistence")]
fn build_intent_adjustments(feedback: &ReasoningLearningFeedback) -> Vec<CaseIntentAdjustmentStat> {
    let mut stats = feedback
        .intent_adjustments
        .iter()
        .map(|item| CaseIntentAdjustmentStat {
            intent: item.label.clone(),
            delta: item.delta,
            samples: item.samples,
        })
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| right.delta.abs().cmp(&left.delta.abs()))
            .then_with(|| left.intent.cmp(&right.intent))
    });
    stats.truncate(8);
    stats
}

fn build_review_required_by_lens(cases: &[CaseSummary]) -> Vec<CaseLensStat> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for case in cases.iter().filter(|item| item.workflow_state == "review") {
        let lens = case
            .primary_lens
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown");
        *counts.entry(lens.to_string()).or_insert(0) += 1;
    }

    build_lens_stats(counts)
}

#[cfg(feature = "persistence")]
fn build_human_override_by_lens(
    cases: &[CaseSummary],
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<CaseLensStat> {
    let case_lenses = cases
        .iter()
        .map(|case| {
            (
                case.setup_id.clone(),
                case.primary_lens
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut counts: HashMap<String, usize> = HashMap::new();
    for assessment in assessments
        .iter()
        .filter(|item| item.source == "workflow_update")
    {
        let Some(review) = assessment.reasoning_profile.human_review.as_ref() else {
            continue;
        };
        if !matches!(
            review.verdict,
            crate::ontology::HumanReviewVerdict::Rejected
                | crate::ontology::HumanReviewVerdict::Modified
        ) {
            continue;
        }

        let lens = case_lenses
            .get(&assessment.setup_id)
            .cloned()
            .unwrap_or_else(|| "unknown".into());
        *counts.entry(lens).or_insert(0) += 1;
    }

    build_lens_stats(counts)
}

fn build_lens_stats(counts: HashMap<String, usize>) -> Vec<CaseLensStat> {
    let mut stats = counts
        .into_iter()
        .map(|(lens, cases)| CaseLensStat { lens, cases })
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| left.lens.cmp(&right.lens))
    });
    stats.truncate(8);
    stats
}

#[cfg(feature = "persistence")]
fn build_archetype_stats(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<super::types::CaseArchetypeStat> {
    let mut grouped: HashMap<String, (usize, Decimal)> = HashMap::new();
    for assessment in assessments {
        for projection in &assessment.archetype_projections {
            let entry = grouped
                .entry(projection.archetype_key.clone())
                .or_insert((0, Decimal::ZERO));
            entry.0 += 1;
            entry.1 += projection.affinity;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(archetype, (cases, total_affinity))| super::types::CaseArchetypeStat {
                archetype,
                cases,
                mean_affinity: if cases == 0 {
                    Decimal::ZERO
                } else {
                    total_affinity / Decimal::from(cases as i64)
                },
            },
        )
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_affinity.cmp(&left.mean_affinity))
            .then_with(|| left.archetype.cmp(&right.archetype))
    });
    stats.truncate(10);
    stats
}

#[cfg(feature = "persistence")]
fn build_signature_stats(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<super::types::CaseSignatureStat> {
    let mut grouped: HashMap<(String, String, String), (usize, Decimal)> = HashMap::new();
    for assessment in assessments {
        let Some(signature) = assessment.case_signature.as_ref() else {
            continue;
        };
        let key = (
            format!("{:?}", signature.topology).to_ascii_lowercase(),
            format!("{:?}", signature.temporal_shape).to_ascii_lowercase(),
            format!("{:?}", signature.conflict_shape).to_ascii_lowercase(),
        );
        let entry = grouped.entry(key).or_insert((0, Decimal::ZERO));
        entry.0 += 1;
        entry.1 += signature.novelty_score;
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |((topology, temporal_shape, conflict_shape), (cases, novelty_sum))| {
                super::types::CaseSignatureStat {
                    topology,
                    temporal_shape,
                    conflict_shape,
                    cases,
                    mean_novelty: if cases == 0 {
                        Decimal::ZERO
                    } else {
                        novelty_sum / Decimal::from(cases as i64)
                    },
                }
            },
        )
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_novelty.cmp(&left.mean_novelty))
            .then_with(|| left.topology.cmp(&right.topology))
    });
    stats.truncate(12);
    stats
}

#[cfg(feature = "persistence")]
fn build_expectation_violation_stats(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<super::types::CaseExpectationViolationStat> {
    let mut grouped: HashMap<String, (usize, Decimal)> = HashMap::new();
    for assessment in assessments {
        for violation in &assessment.expectation_violations {
            let key = format!("{:?}", violation.kind).to_ascii_lowercase();
            let entry = grouped.entry(key).or_insert((0, Decimal::ZERO));
            entry.0 += 1;
            entry.1 += violation.magnitude;
        }
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |(kind, (cases, magnitude_sum))| super::types::CaseExpectationViolationStat {
                kind,
                cases,
                mean_magnitude: if cases == 0 {
                    Decimal::ZERO
                } else {
                    magnitude_sum / Decimal::from(cases as i64)
                },
            },
        )
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .cases
            .cmp(&left.cases)
            .then_with(|| right.mean_magnitude.cmp(&left.mean_magnitude))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    stats.truncate(12);
    stats
}

#[cfg(feature = "persistence")]
fn build_discovered_archetype_catalog(
    archetypes: &[crate::persistence::discovered_archetype::DiscoveredArchetypeRecord],
) -> Vec<super::types::CaseArchetypeCatalogStat> {
    let mut stats = archetypes
        .iter()
        .map(|record| super::types::CaseArchetypeCatalogStat {
            archetype: record.archetype_key.clone(),
            label: record.label.clone(),
            samples: record.samples,
            hits: record.hits,
            hit_rate: record.hit_rate,
            mean_net_return: record.mean_net_return,
            mean_affinity: record.mean_affinity,
            topology: record.topology.clone(),
            temporal_shape: record.temporal_shape.clone(),
            conflict_shape: record.conflict_shape.clone(),
        })
        .collect::<Vec<_>>();
    stats.sort_by(|left, right| {
        right
            .samples
            .cmp(&left.samples)
            .then_with(|| right.mean_net_return.cmp(&left.mean_net_return))
            .then_with(|| left.archetype.cmp(&right.archetype))
    });
    stats.truncate(24);
    stats
}

#[cfg(feature = "persistence")]
fn build_lens_regime_hit_rates(
    case_outcomes: &[CaseRealizedOutcomeRecord],
) -> Vec<CaseLensRegimeHitRateStat> {
    let mut grouped: HashMap<(String, String), (usize, usize, rust_decimal::Decimal)> =
        HashMap::new();

    for outcome in case_outcomes {
        let lens = outcome
            .primary_lens
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let regime = outcome.market_regime.trim().to_string();
        let net_return = outcome.net_return;
        let entry = grouped
            .entry((lens, regime))
            .or_insert((0, 0, rust_decimal::Decimal::ZERO));
        entry.0 += 1;
        if outcome.followed_through {
            entry.1 += 1;
        }
        entry.2 += net_return;
    }

    let mut stats = grouped
        .into_iter()
        .map(
            |((lens, market_regime), (total, hits, total_net_return))| CaseLensRegimeHitRateStat {
                lens,
                market_regime,
                total,
                hits,
                hit_rate: if total == 0 {
                    rust_decimal::Decimal::ZERO
                } else {
                    rust_decimal::Decimal::from(hits as i64)
                        / rust_decimal::Decimal::from(total as i64)
                },
                mean_net_return: if total == 0 {
                    rust_decimal::Decimal::ZERO
                } else {
                    total_net_return / rust_decimal::Decimal::from(total as i64)
                },
            },
        )
        .collect::<Vec<_>>();

    stats.sort_by(|left, right| {
        right
            .total
            .cmp(&left.total)
            .then_with(|| right.hit_rate.cmp(&left.hit_rate))
            .then_with(|| left.lens.cmp(&right.lens))
            .then_with(|| left.market_regime.cmp(&right.market_regime))
    });
    stats.truncate(12);
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
                if let Some(score) = record.primary_mechanism_score {
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
