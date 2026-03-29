#[cfg(feature = "persistence")]
use rust_decimal::Decimal;

#[cfg(feature = "persistence")]
use super::shared::{
    assessment_snapshot_from_summary, current_assessment_snapshot, decimal_delta,
    factor_decay_score, factor_delta_strings, mechanism_factor_map, regime_delta_strings,
    regime_metric_delta_strings, regime_metric_shift_score, regime_shift_score,
    snapshot_matches_current, state_score_map, classify_transition, transition_summary,
};
#[cfg(feature = "persistence")]
use super::{
    CaseDetail, CaseMechanismStory, CaseMechanismTransition, CaseReasoningAssessmentSnapshot,
    clamp_unit_interval,
};

#[cfg(feature = "persistence")]
pub(in crate::cases) fn build_case_mechanism_story(detail: &CaseDetail) -> CaseMechanismStory {
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
pub(in crate::cases) fn describe_mechanism_transition(
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
    let combined_regime_score = clamp_unit_interval(regime_score + regime_metric_score);
    let classification = classify_transition(
        from.primary_mechanism_kind.as_deref(),
        to.primary_mechanism_kind.as_deref(),
        combined_regime_score,
        decay_score,
        review_score,
    );
    let confidence = clamp_unit_interval(
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
