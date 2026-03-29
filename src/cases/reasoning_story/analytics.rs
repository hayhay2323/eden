#[cfg(feature = "persistence")]
use std::collections::HashMap;

#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;

#[cfg(feature = "persistence")]
use super::shared::{
    assessment_snapshot_from_summary, regime_bucket, snapshot_matches_current,
};
#[cfg(feature = "persistence")]
use super::story::describe_mechanism_transition;
#[cfg(feature = "persistence")]
use super::{
    CaseInvalidationPatternStat, CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat,
    CaseMechanismTransitionStat, CaseReasoningAssessmentSnapshot, CaseSummary,
};

#[cfg(feature = "persistence")]
pub(in crate::cases) fn build_mechanism_transition_analytics(
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
pub(in crate::cases) fn build_invalidation_patterns(
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
