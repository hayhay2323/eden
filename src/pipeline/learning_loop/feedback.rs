use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LearningOutcome {
    Reinforced,
    Corrected,
    Unresolved,
}

#[derive(Debug, Clone)]
struct PairedLearningExample {
    runtime: CaseReasoningAssessmentRecord,
    workflow: CaseReasoningAssessmentRecord,
    outcome: LearningOutcome,
}

pub fn derive_learning_feedback(
    assessments: &[CaseReasoningAssessmentRecord],
    outcome_context: &OutcomeLearningContext,
) -> ReasoningLearningFeedback {
    let mut mechanism_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut mechanism_factor_totals: HashMap<(String, String, String), (Decimal, Decimal)> =
        HashMap::new();
    let mut predicate_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut conditioned_totals: HashMap<(String, String, String), (Decimal, Decimal)> =
        HashMap::new();
    let paired_examples = pair_learning_examples(assessments);
    let reinforced_examples = paired_examples
        .iter()
        .filter(|example| example.outcome == LearningOutcome::Reinforced)
        .count();
    let corrected_examples = paired_examples
        .iter()
        .filter(|example| example.outcome == LearningOutcome::Corrected)
        .count();

    for (index, example) in paired_examples.iter().enumerate() {
        let recency_weight = if index < 24 {
            Decimal::ONE
        } else if index < 72 {
            dec!(0.5)
        } else {
            dec!(0.25)
        };
        let delta = learning_delta(example, outcome_context) * recency_weight;

        if let Some(mechanism) = example.runtime.primary_mechanism_kind.as_ref() {
            let entry = mechanism_totals
                .entry(mechanism.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta;
            entry.1 += recency_weight;

            if let Some(runtime_mechanism) =
                example.runtime.reasoning_profile.primary_mechanism.as_ref()
            {
                for factor in &runtime_mechanism.factors {
                    let activation = factor.activation.max(dec!(0.10));
                    let entry = mechanism_factor_totals
                        .entry((mechanism.clone(), factor.key.clone(), factor.label.clone()))
                        .or_insert((Decimal::ZERO, Decimal::ZERO));
                    entry.0 += delta * activation;
                    entry.1 += recency_weight * activation;
                }
            }

            for state in &example.runtime.composite_state_kinds {
                let entry = conditioned_totals
                    .entry((mechanism.clone(), "state".into(), state.clone()))
                    .or_insert((Decimal::ZERO, Decimal::ZERO));
                entry.0 += delta * dec!(0.8);
                entry.1 += recency_weight * dec!(0.8);
            }

            for predicate in &example.runtime.predicate_kinds {
                let entry = conditioned_totals
                    .entry((mechanism.clone(), "predicate".into(), predicate.clone()))
                    .or_insert((Decimal::ZERO, Decimal::ZERO));
                entry.0 += delta * dec!(0.4);
                entry.1 += recency_weight * dec!(0.4);
            }
        }

        for predicate in &example.runtime.predicate_kinds {
            let entry = predicate_totals
                .entry(predicate.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * dec!(0.6);
            entry.1 += recency_weight * dec!(0.6);
        }
    }

    ReasoningLearningFeedback {
        paired_examples: paired_examples.len(),
        reinforced_examples,
        corrected_examples,
        mechanism_adjustments: finalize_adjustments(mechanism_totals),
        mechanism_factor_adjustments: finalize_factor_adjustments(mechanism_factor_totals),
        predicate_adjustments: finalize_adjustments(predicate_totals),
        conditioned_adjustments: finalize_conditioned_adjustments(conditioned_totals),
        outcome_context: outcome_context.clone(),
    }
}

pub fn apply_learning_feedback(
    profile: &CaseReasoningProfile,
    invalidation_rules: &[String],
    feedback: &ReasoningLearningFeedback,
) -> CaseReasoningProfile {
    let mut predicates = profile.predicates.clone();
    for predicate in &mut predicates {
        let delta = feedback.predicate_delta(&predicate.label);
        if delta != Decimal::ZERO {
            predicate.score = clamp_unit_interval(predicate.score + delta);
            predicate.evidence.push(format!(
                "learning feedback {}{}",
                if delta >= Decimal::ZERO { "+" } else { "" },
                delta.round_dp(3)
            ));
        }
    }

    let mut next = build_reasoning_profile(
        &predicates,
        invalidation_rules,
        profile.human_review.clone(),
    );
    let factor_lookup = feedback.mechanism_factor_lookup();
    let (primary, competing) = infer_mechanisms_with_factor_adjustments(
        &next.composite_states,
        invalidation_rules,
        &factor_lookup,
    );
    next.primary_mechanism = primary;
    next.competing_mechanisms = competing;
    let mut candidates = Vec::new();
    if let Some(primary) = next.primary_mechanism.take() {
        candidates.push(primary);
    }
    candidates.extend(next.competing_mechanisms.drain(..));
    let active_states = next
        .composite_states
        .iter()
        .map(|state| state.label.clone())
        .collect::<Vec<_>>();
    let active_predicates = next
        .predicates
        .iter()
        .map(|predicate| predicate.label.clone())
        .collect::<Vec<_>>();

    for candidate in &mut candidates {
        let delta = feedback.mechanism_delta(&candidate.label)
            + feedback.conditioned_delta(&candidate.label, &active_states, &active_predicates);
        if delta != Decimal::ZERO {
            candidate.score = clamp_unit_interval(candidate.score + delta);
        }
    }

    retain_explanatory_mechanisms(&mut candidates);

    next.primary_mechanism = candidates.first().cloned();
    next.competing_mechanisms = candidates.into_iter().skip(1).take(3).collect();
    next
}

fn finalize_adjustments(source: HashMap<String, (Decimal, Decimal)>) -> Vec<LearningAdjustment> {
    let mut items = source
        .into_iter()
        .map(|(label, (total_delta, weight_sum))| LearningAdjustment {
            label,
            delta: clamp_delta(if weight_sum > Decimal::ZERO {
                total_delta / weight_sum
            } else {
                Decimal::ZERO
            }),
            samples: decimal_ceil_usize(weight_sum),
        })
        .filter(|item| item.delta.abs() >= dec!(0.005))
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .delta
            .abs()
            .cmp(&left.delta.abs())
            .then_with(|| right.samples.cmp(&left.samples))
            .then_with(|| left.label.cmp(&right.label))
    });
    items.truncate(8);
    items
}

fn finalize_factor_adjustments(
    source: HashMap<(String, String, String), (Decimal, Decimal)>,
) -> Vec<MechanismFactorAdjustment> {
    let mut items = source
        .into_iter()
        .map(
            |((mechanism, factor_key, factor_label), (total_delta, weight_sum))| {
                MechanismFactorAdjustment {
                    mechanism,
                    factor_key,
                    factor_label,
                    delta: clamp_structural_delta(if weight_sum > Decimal::ZERO {
                        total_delta / weight_sum
                    } else {
                        Decimal::ZERO
                    }),
                    samples: weight_sum
                        .to_string()
                        .parse::<f64>()
                        .map(|v| v.ceil() as usize)
                        .unwrap_or(0),
                }
            },
        )
        .filter(|item| item.delta.abs() >= dec!(0.003))
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .delta
            .abs()
            .cmp(&left.delta.abs())
            .then_with(|| right.samples.cmp(&left.samples))
            .then_with(|| left.mechanism.cmp(&right.mechanism))
            .then_with(|| left.factor_key.cmp(&right.factor_key))
    });
    items.truncate(24);
    items
}

fn finalize_conditioned_adjustments(
    source: HashMap<(String, String, String), (Decimal, Decimal)>,
) -> Vec<ConditionedLearningAdjustment> {
    let mut items = source
        .into_iter()
        .map(
            |((mechanism, scope, conditioned_on), (total_delta, weight_sum))| {
                ConditionedLearningAdjustment {
                    mechanism,
                    scope,
                    conditioned_on,
                    delta: clamp_delta(if weight_sum > Decimal::ZERO {
                        total_delta / weight_sum
                    } else {
                        Decimal::ZERO
                    }),
                    samples: weight_sum
                        .to_string()
                        .parse::<f64>()
                        .map(|v| v.ceil() as usize)
                        .unwrap_or(0),
                }
            },
        )
        .filter(|item| item.delta.abs() >= dec!(0.005))
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .delta
            .abs()
            .cmp(&left.delta.abs())
            .then_with(|| right.samples.cmp(&left.samples))
            .then_with(|| left.mechanism.cmp(&right.mechanism))
            .then_with(|| left.conditioned_on.cmp(&right.conditioned_on))
    });
    items.truncate(12);
    items
}

fn pair_learning_examples(
    assessments: &[CaseReasoningAssessmentRecord],
) -> Vec<PairedLearningExample> {
    let mut ordered = assessments.to_vec();
    ordered.sort_by(|left, right| {
        left.recorded_at
            .cmp(&right.recorded_at)
            .then_with(|| source_rank(&left.source).cmp(&source_rank(&right.source)))
    });

    let mut latest_runtime_by_key: HashMap<String, CaseReasoningAssessmentRecord> = HashMap::new();
    let mut examples = Vec::new();

    for assessment in ordered {
        let key = assessment
            .workflow_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| assessment.setup_id.clone());

        match assessment.source.as_str() {
            "runtime" => {
                latest_runtime_by_key.insert(key, assessment);
            }
            "workflow_update" => {
                if let Some(runtime) = latest_runtime_by_key.get(&key).cloned() {
                    let outcome = learning_outcome(&runtime, &assessment);
                    examples.push(PairedLearningExample {
                        runtime,
                        workflow: assessment,
                        outcome,
                    });
                }
            }
            _ => {}
        }
    }

    examples.reverse();
    examples
}

fn source_rank(source: &str) -> u8 {
    match source {
        "runtime" => 0,
        "workflow_update" => 1,
        _ => 2,
    }
}

fn learning_outcome(
    runtime: &CaseReasoningAssessmentRecord,
    workflow: &CaseReasoningAssessmentRecord,
) -> LearningOutcome {
    let same_mechanism = runtime.primary_mechanism_kind == workflow.primary_mechanism_kind;
    let review_like = workflow.workflow_state == "review";
    let reflexive = workflow
        .composite_state_kinds
        .iter()
        .any(|item| item == "Reflexive Correction");
    let narrative_failure = workflow.primary_mechanism_kind.as_deref() == Some("Narrative Failure");
    let mechanism_reject = has_review_reason(workflow, HumanReviewReasonKind::MechanismMismatch)
        || note_rejected(workflow);
    let timing_reject = has_review_reason(workflow, HumanReviewReasonKind::TimingMismatch);
    let risk_reject = has_review_reason(workflow, HumanReviewReasonKind::RiskTooHigh)
        || has_review_reason(workflow, HumanReviewReasonKind::EventRisk);

    if matches!(
        workflow.workflow_state.as_str(),
        "confirm" | "execute" | "monitor"
    ) && same_mechanism
    {
        LearningOutcome::Reinforced
    } else if review_like
        || !same_mechanism
        || reflexive
        || narrative_failure
        || mechanism_reject
        || timing_reject
        || risk_reject
    {
        LearningOutcome::Corrected
    } else {
        LearningOutcome::Unresolved
    }
}

fn learning_delta(
    example: &PairedLearningExample,
    outcome_context: &OutcomeLearningContext,
) -> Decimal {
    let mut delta = match example.outcome {
        LearningOutcome::Reinforced => dec!(0.05),
        LearningOutcome::Corrected => dec!(-0.06),
        LearningOutcome::Unresolved => Decimal::ZERO,
    };

    if example
        .workflow
        .composite_state_kinds
        .iter()
        .any(|item| item == "Reflexive Correction")
    {
        delta -= dec!(0.03);
    }

    if example.workflow.primary_mechanism_kind.as_deref() == Some("Narrative Failure") {
        delta -= dec!(0.01);
    }

    if has_review_reason(&example.workflow, HumanReviewReasonKind::MechanismMismatch)
        || note_rejected(&example.workflow)
    {
        delta -= dec!(0.03);
    }
    if has_review_reason(&example.workflow, HumanReviewReasonKind::TimingMismatch) {
        delta -= dec!(0.02);
    }
    if has_review_reason(&example.workflow, HumanReviewReasonKind::RiskTooHigh)
        || has_review_reason(&example.workflow, HumanReviewReasonKind::EventRisk)
    {
        delta -= dec!(0.015);
    }

    if let (Some(runtime_score), Some(workflow_score)) = (
        example.runtime.primary_mechanism_score,
        example.workflow.primary_mechanism_score,
    ) {
        let score_delta = clamp_delta((workflow_score - runtime_score) * dec!(0.5));
        delta += score_delta;
    }

    if delta > Decimal::ZERO {
        delta = clamp_delta(delta * (Decimal::ONE + outcome_context.reward_multiplier));
    } else if delta < Decimal::ZERO {
        delta = clamp_delta(delta * (Decimal::ONE + outcome_context.penalty_multiplier));
    }

    clamp_delta(delta)
}

fn has_review_reason(
    workflow: &CaseReasoningAssessmentRecord,
    kind: HumanReviewReasonKind,
) -> bool {
    workflow
        .reasoning_profile
        .human_review
        .as_ref()
        .map(|review| review.reasons.iter().any(|reason| reason.kind == kind))
        .unwrap_or(false)
}

fn note_rejected(workflow: &CaseReasoningAssessmentRecord) -> bool {
    workflow
        .note
        .as_deref()
        .map(str::to_lowercase)
        .map(|note| {
            [
                "reject",
                "dismiss",
                "ignore",
                "撤回",
                "否決",
                "駁回",
                "忽略",
                "不成立",
                "mismatch",
            ]
            .iter()
            .any(|keyword| note.contains(keyword))
        })
        .unwrap_or(false)
}

fn decimal_ceil_usize(value: Decimal) -> usize {
    value.ceil().to_f64().map(|v| v as usize).unwrap_or(0)
}

pub(super) fn clamp_delta(value: Decimal) -> Decimal {
    if value < dec!(-0.12) {
        dec!(-0.12)
    } else if value > dec!(0.12) {
        dec!(0.12)
    } else {
        value
    }
}

fn clamp_structural_delta(value: Decimal) -> Decimal {
    if value < dec!(-0.08) {
        dec!(-0.08)
    } else if value > dec!(0.08) {
        dec!(0.08)
    } else {
        value
    }
}
