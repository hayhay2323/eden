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
use super::reasoning_story::{build_invalidation_patterns, build_mechanism_transition_analytics};
use super::types::{
    CaseLensStat, CaseMechanismStat, CaseReviewAnalytics, CaseSummary,
};
#[cfg(feature = "persistence")]
use super::types::CaseLensRegimeHitRateStat;
#[cfg(feature = "persistence")]
use super::io::CaseError;
#[cfg(feature = "persistence")]
use super::types::{
    CaseHumanReviewReasonStat, CaseInvalidationPatternStat, CaseMechanismDriftPoint,
    CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat,
    CaseMechanismTransitionStat, CaseReviewerCorrectionStat, CaseReviewerDoctrineStat,
};

pub(super) fn build_case_review_analytics(cases: &[CaseSummary]) -> CaseReviewAnalytics {
    CaseReviewAnalytics {
        mechanism_stats: build_mechanism_stats(cases),
        review_required_by_lens: build_review_required_by_lens(cases),
        human_override_by_lens: Vec::new(),
        lens_regime_hit_rates: Vec::new(),
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
pub(super) fn build_case_review_analytics_with_assessments(
    cases: &[CaseSummary],
    assessments: &[CaseReasoningAssessmentRecord],
    case_outcomes: &[CaseRealizedOutcomeRecord],
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
        review_required_by_lens: build_review_required_by_lens(cases),
        human_override_by_lens: build_human_override_by_lens(cases, assessments),
        lens_regime_hit_rates: build_lens_regime_hit_rates(case_outcomes),
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
        let net_return = outcome
            .net_return
            .parse::<rust_decimal::Decimal>()
            .unwrap_or(rust_decimal::Decimal::ZERO);
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
        .map(|((lens, market_regime), (total, hits, total_net_return))| {
            CaseLensRegimeHitRateStat {
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
            }
        })
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
