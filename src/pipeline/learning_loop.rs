use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::math::clamp_unit_interval;
use crate::ontology::{CaseReasoningProfile, HumanReviewReasonKind};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
use crate::pipeline::mechanism_inference::{
    build_reasoning_profile, infer_mechanisms_with_factor_adjustments,
    retain_explanatory_mechanisms,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReasoningLearningFeedback {
    pub paired_examples: usize,
    pub reinforced_examples: usize,
    pub corrected_examples: usize,
    pub mechanism_adjustments: Vec<LearningAdjustment>,
    pub mechanism_factor_adjustments: Vec<MechanismFactorAdjustment>,
    pub predicate_adjustments: Vec<LearningAdjustment>,
    pub conditioned_adjustments: Vec<ConditionedLearningAdjustment>,
    pub outcome_context: OutcomeLearningContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningAdjustment {
    pub label: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionedLearningAdjustment {
    pub mechanism: String,
    pub scope: String,
    pub conditioned_on: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismFactorAdjustment {
    pub mechanism: String,
    pub factor_key: String,
    pub factor_label: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeLearningContext {
    pub source: String,
    pub reward_multiplier: Decimal,
    pub penalty_multiplier: Decimal,
    pub promoted_follow_through: Decimal,
    pub promoted_retention: Decimal,
    pub promoted_mean_net_return: Decimal,
    pub falsified_invalidation: Decimal,
    pub falsified_follow_through: Decimal,
    pub us_hit_rate: Decimal,
    pub us_mean_return: Decimal,
}

impl Default for OutcomeLearningContext {
    fn default() -> Self {
        Self {
            source: "none".into(),
            reward_multiplier: Decimal::ZERO,
            penalty_multiplier: Decimal::ZERO,
            promoted_follow_through: Decimal::ZERO,
            promoted_retention: Decimal::ZERO,
            promoted_mean_net_return: Decimal::ZERO,
            falsified_invalidation: Decimal::ZERO,
            falsified_follow_through: Decimal::ZERO,
            us_hit_rate: Decimal::ZERO,
            us_mean_return: Decimal::ZERO,
        }
    }
}

impl ReasoningLearningFeedback {
    pub fn mechanism_delta(&self, label: &str) -> Decimal {
        self.mechanism_adjustments
            .iter()
            .find(|item| item.label == label)
            .map(|item| item.delta)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn predicate_delta(&self, label: &str) -> Decimal {
        self.predicate_adjustments
            .iter()
            .find(|item| item.label == label)
            .map(|item| item.delta)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn conditioned_delta(
        &self,
        mechanism: &str,
        active_states: &[String],
        active_predicates: &[String],
    ) -> Decimal {
        let total = self
            .conditioned_adjustments
            .iter()
            .filter(|item| item.mechanism == mechanism)
            .filter(|item| match item.scope.as_str() {
                "state" => active_states
                    .iter()
                    .any(|state| state == &item.conditioned_on),
                "predicate" => active_predicates
                    .iter()
                    .any(|predicate| predicate == &item.conditioned_on),
                _ => false,
            })
            .fold(Decimal::ZERO, |acc, item| acc + item.delta);
        clamp_delta(total)
    }

    pub fn mechanism_factor_lookup(&self) -> HashMap<(String, String), Decimal> {
        self.mechanism_factor_adjustments
            .iter()
            .map(|item| {
                (
                    (item.mechanism.clone(), item.factor_key.clone()),
                    item.delta,
                )
            })
            .collect()
    }
}

pub fn derive_learning_feedback(
    assessments: &[CaseReasoningAssessmentRecord],
    outcome_context: &OutcomeLearningContext,
) -> ReasoningLearningFeedback {
    let mut mechanism_totals: HashMap<String, (Decimal, usize)> = HashMap::new();
    let mut mechanism_factor_totals: HashMap<(String, String, String), (Decimal, usize)> =
        HashMap::new();
    let mut predicate_totals: HashMap<String, (Decimal, usize)> = HashMap::new();
    let mut conditioned_totals: HashMap<(String, String, String), (Decimal, usize)> =
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
                .or_insert((Decimal::ZERO, 0));
            entry.0 += delta;
            entry.1 += 1;

            if let Some(runtime_mechanism) =
                example.runtime.reasoning_profile.primary_mechanism.as_ref()
            {
                for factor in &runtime_mechanism.factors {
                    let activation = factor.activation.max(dec!(0.10));
                    let entry = mechanism_factor_totals
                        .entry((mechanism.clone(), factor.key.clone(), factor.label.clone()))
                        .or_insert((Decimal::ZERO, 0));
                    entry.0 += delta * activation;
                    entry.1 += 1;
                }
            }

            for state in &example.runtime.composite_state_kinds {
                let entry = conditioned_totals
                    .entry((mechanism.clone(), "state".into(), state.clone()))
                    .or_insert((Decimal::ZERO, 0));
                entry.0 += delta * dec!(0.8);
                entry.1 += 1;
            }

            for predicate in &example.runtime.predicate_kinds {
                let entry = conditioned_totals
                    .entry((mechanism.clone(), "predicate".into(), predicate.clone()))
                    .or_insert((Decimal::ZERO, 0));
                entry.0 += delta * dec!(0.4);
                entry.1 += 1;
            }
        }

        for predicate in &example.runtime.predicate_kinds {
            let entry = predicate_totals
                .entry(predicate.clone())
                .or_insert((Decimal::ZERO, 0));
            entry.0 += delta * dec!(0.6);
            entry.1 += 1;
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

fn finalize_adjustments(source: HashMap<String, (Decimal, usize)>) -> Vec<LearningAdjustment> {
    let mut items = source
        .into_iter()
        .map(|(label, (total_delta, samples))| LearningAdjustment {
            label,
            delta: clamp_delta(total_delta / Decimal::from(samples as i64)),
            samples,
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
    source: HashMap<(String, String, String), (Decimal, usize)>,
) -> Vec<MechanismFactorAdjustment> {
    let mut items = source
        .into_iter()
        .map(
            |((mechanism, factor_key, factor_label), (total_delta, samples))| {
                MechanismFactorAdjustment {
                    mechanism,
                    factor_key,
                    factor_label,
                    delta: clamp_structural_delta(total_delta / Decimal::from(samples as i64)),
                    samples,
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
    source: HashMap<(String, String, String), (Decimal, usize)>,
) -> Vec<ConditionedLearningAdjustment> {
    let mut items = source
        .into_iter()
        .map(
            |((mechanism, scope, conditioned_on), (total_delta, samples))| {
                ConditionedLearningAdjustment {
                    mechanism,
                    scope,
                    conditioned_on,
                    delta: clamp_delta(total_delta / Decimal::from(samples as i64)),
                    samples,
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
        parse_decimal(example.runtime.primary_mechanism_score.as_deref()),
        parse_decimal(example.workflow.primary_mechanism_score.as_deref()),
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

fn parse_decimal(value: Option<&str>) -> Option<Decimal> {
    value.and_then(|value| value.parse::<Decimal>().ok())
}

fn clamp_delta(value: Decimal) -> Decimal {
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

pub fn derive_outcome_learning_context_from_hk_rows(
    rows: &[LineageMetricRowRecord],
) -> OutcomeLearningContext {
    let promoted = average_bucket_metrics(rows, "promoted_outcomes");
    let falsified = average_bucket_metrics(rows, "falsified_outcomes");
    let blocked = average_bucket_metrics(rows, "blocked_outcomes");

    let reward_strength = mean(&[
        promoted.follow_through_rate,
        promoted.structure_retention_rate,
        normalize_return(promoted.mean_net_return),
    ]);
    let penalty_strength = mean(&[
        falsified.invalidation_rate.max(blocked.invalidation_rate),
        falsified
            .follow_through_rate
            .max(blocked.follow_through_rate),
        normalize_return(falsified.mean_net_return.max(blocked.mean_net_return)),
    ]);

    OutcomeLearningContext {
        source: "hk_lineage".into(),
        reward_multiplier: clamp_multiplier(reward_strength * dec!(0.5)),
        penalty_multiplier: clamp_multiplier(penalty_strength * dec!(0.5)),
        promoted_follow_through: promoted.follow_through_rate,
        promoted_retention: promoted.structure_retention_rate,
        promoted_mean_net_return: promoted.mean_net_return,
        falsified_invalidation: falsified.invalidation_rate.max(blocked.invalidation_rate),
        falsified_follow_through: falsified
            .follow_through_rate
            .max(blocked.follow_through_rate),
        us_hit_rate: Decimal::ZERO,
        us_mean_return: Decimal::ZERO,
    }
}

pub fn derive_outcome_learning_context_from_case_outcomes(
    outcomes: &[CaseRealizedOutcomeRecord],
    market: &str,
) -> OutcomeLearningContext {
    if outcomes.is_empty() {
        return OutcomeLearningContext::default();
    }

    let reward_strength = mean(&[
        rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        rate(
            outcomes
                .iter()
                .filter(|item| item.structure_retained)
                .count(),
            outcomes.len(),
        ),
        normalize_return(mean(
            &outcomes
                .iter()
                .map(|item| parse_decimal(Some(item.net_return.as_str())).unwrap_or(Decimal::ZERO))
                .collect::<Vec<_>>(),
        )),
    ]);
    let penalty_strength = mean(&[
        rate(
            outcomes.iter().filter(|item| item.invalidated).count(),
            outcomes.len(),
        ),
        rate(
            outcomes
                .iter()
                .filter(|item| {
                    parse_decimal(Some(item.net_return.as_str())).unwrap_or(Decimal::ZERO)
                        < Decimal::ZERO
                })
                .count(),
            outcomes.len(),
        ),
        normalize_return(-mean(
            &outcomes
                .iter()
                .map(|item| parse_decimal(Some(item.net_return.as_str())).unwrap_or(Decimal::ZERO))
                .collect::<Vec<_>>(),
        )),
    ]);

    OutcomeLearningContext {
        source: format!("{market}_case_outcomes"),
        reward_multiplier: clamp_multiplier(reward_strength * dec!(0.5)),
        penalty_multiplier: clamp_multiplier(penalty_strength * dec!(0.5)),
        promoted_follow_through: rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        promoted_retention: rate(
            outcomes
                .iter()
                .filter(|item| item.structure_retained)
                .count(),
            outcomes.len(),
        ),
        promoted_mean_net_return: mean(
            &outcomes
                .iter()
                .map(|item| parse_decimal(Some(item.net_return.as_str())).unwrap_or(Decimal::ZERO))
                .collect::<Vec<_>>(),
        ),
        falsified_invalidation: rate(
            outcomes.iter().filter(|item| item.invalidated).count(),
            outcomes.len(),
        ),
        falsified_follow_through: rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        us_hit_rate: Decimal::ZERO,
        us_mean_return: Decimal::ZERO,
    }
}

pub fn derive_outcome_learning_context_from_us_rows(
    rows: &[UsLineageMetricRowRecord],
) -> OutcomeLearningContext {
    if rows.is_empty() {
        return OutcomeLearningContext::default();
    }

    let hit_rate = mean(
        &rows
            .iter()
            .map(|row| parse_decimal(Some(row.hit_rate.as_str())).unwrap_or(Decimal::ZERO))
            .collect::<Vec<_>>(),
    );
    let mean_return = mean(
        &rows
            .iter()
            .map(|row| parse_decimal(Some(row.mean_return.as_str())).unwrap_or(Decimal::ZERO))
            .collect::<Vec<_>>(),
    );

    OutcomeLearningContext {
        source: "us_lineage".into(),
        reward_multiplier: clamp_multiplier(
            mean(&[hit_rate, normalize_return(mean_return)]) * dec!(0.5),
        ),
        penalty_multiplier: clamp_multiplier(
            mean(&[Decimal::ONE - hit_rate, normalize_return(-mean_return)]) * dec!(0.5),
        ),
        promoted_follow_through: Decimal::ZERO,
        promoted_retention: Decimal::ZERO,
        promoted_mean_net_return: Decimal::ZERO,
        falsified_invalidation: Decimal::ZERO,
        falsified_follow_through: Decimal::ZERO,
        us_hit_rate: hit_rate,
        us_mean_return: mean_return,
    }
}

#[derive(Default)]
struct HkBucketMetrics {
    mean_net_return: Decimal,
    follow_through_rate: Decimal,
    invalidation_rate: Decimal,
    structure_retention_rate: Decimal,
}

fn average_bucket_metrics(rows: &[LineageMetricRowRecord], bucket: &str) -> HkBucketMetrics {
    let matched = rows
        .iter()
        .filter(|row| row.bucket == bucket)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return HkBucketMetrics::default();
    }

    HkBucketMetrics {
        mean_net_return: mean(
            &matched
                .iter()
                .map(|row| {
                    parse_decimal(Some(row.mean_net_return.as_str())).unwrap_or(Decimal::ZERO)
                })
                .collect::<Vec<_>>(),
        ),
        follow_through_rate: mean(
            &matched
                .iter()
                .map(|row| {
                    parse_decimal(Some(row.follow_through_rate.as_str())).unwrap_or(Decimal::ZERO)
                })
                .collect::<Vec<_>>(),
        ),
        invalidation_rate: mean(
            &matched
                .iter()
                .map(|row| {
                    parse_decimal(Some(row.invalidation_rate.as_str())).unwrap_or(Decimal::ZERO)
                })
                .collect::<Vec<_>>(),
        ),
        structure_retention_rate: mean(
            &matched
                .iter()
                .map(|row| {
                    parse_decimal(Some(row.structure_retention_rate.as_str()))
                        .unwrap_or(Decimal::ZERO)
                })
                .collect::<Vec<_>>(),
        ),
    }
}

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
}

fn rate(count: usize, total: usize) -> Decimal {
    if total == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(count as i64) / Decimal::from(total as i64)
    }
}

fn normalize_return(value: Decimal) -> Decimal {
    if value <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        clamp_unit_interval(value * dec!(4))
    }
}

fn clamp_multiplier(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        Decimal::ZERO
    } else if value > dec!(0.5) {
        dec!(0.5)
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::cases::{CaseEvidence, CaseSummary};
    use crate::live_snapshot::LiveMarket;
    use crate::ontology::{
        AtomicPredicate, AtomicPredicateKind, CaseReasoningProfile, CompositeState,
        CompositeStateKind, GoverningLawKind, MechanismCandidate, MechanismCandidateKind,
    };
    use crate::persistence::lineage_metric_row::LineageMetricRowRecord;

    #[test]
    fn feedback_penalizes_reviewed_mechanisms() {
        let mut summary = CaseSummary {
            case_id: "setup:1".into(),
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            workflow_actor: Some("actor".into()),
            workflow_note: Some("reject narrative".into()),
            symbol: "A.US".into(),
            title: "Case".into(),
            sector: Some("Technology".into()),
            market: LiveMarket::Us,
            recommended_action: "enter".into(),
            workflow_state: "review".into(),
            market_regime_bias: "neutral".into(),
            market_regime_confidence: dec!(0.40),
            market_breadth_delta: dec!(-0.10),
            market_average_return: dec!(0.01),
            market_directional_consensus: Some(dec!(0.05)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            why_now: "why".into(),
            primary_driver: None,
            family_label: None,
            counter_label: None,
            hypothesis_status: None,
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            key_evidence: vec![CaseEvidence {
                description: "x".into(),
                weight: dec!(0.5),
                direction: dec!(0.5),
            }],
            invalidation_rules: vec![],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![AtomicPredicate {
                    kind: AtomicPredicateKind::CounterevidencePresent,
                    label: "Counterevidence Present".into(),
                    law: GoverningLawKind::Competition,
                    score: dec!(0.6),
                    summary: "s".into(),
                    evidence: vec![],
                }],
                composite_states: vec![CompositeState {
                    kind: CompositeStateKind::ReflexiveCorrection,
                    label: "Reflexive Correction".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    predicates: vec![AtomicPredicateKind::CounterevidencePresent],
                }],
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
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::NarrativeFailure,
                    label: "Narrative Failure".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    supporting_states: vec![CompositeStateKind::ReflexiveCorrection],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![crate::ontology::MechanismFactor {
                        key: "state:reflexive_correction".into(),
                        label: "Reflexive Correction".into(),
                        source: crate::ontology::MechanismFactorSource::State,
                        activation: dec!(0.7),
                        base_weight: dec!(0.5),
                        learned_weight_delta: Decimal::ZERO,
                        effective_weight: dec!(0.5),
                        contribution: dec!(0.35),
                    }],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let runtime = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        summary.workflow_state = "review".into();
        let workflow = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
            "workflow_update",
        );

        let feedback =
            derive_learning_feedback(&[runtime, workflow], &OutcomeLearningContext::default());
        assert_eq!(feedback.paired_examples, 1);
        assert_eq!(feedback.corrected_examples, 1);
        assert!(feedback
            .mechanism_adjustments
            .iter()
            .any(|item| item.label == "Narrative Failure" && item.delta < Decimal::ZERO));
        assert!(feedback
            .mechanism_factor_adjustments
            .iter()
            .any(|item| item.mechanism == "Narrative Failure"));
        assert!(feedback
            .conditioned_adjustments
            .iter()
            .any(|item| item.scope == "state" && item.conditioned_on == "Reflexive Correction"));
    }

    #[test]
    fn feedback_rewards_reinforced_mechanisms() {
        let summary = CaseSummary {
            case_id: "setup:2".into(),
            setup_id: "setup:2".into(),
            workflow_id: Some("wf:2".into()),
            owner: Some("owner".into()),
            reviewer: Some("reviewer".into()),
            workflow_actor: Some("actor".into()),
            workflow_note: Some("confirmed".into()),
            symbol: "B.US".into(),
            title: "Case".into(),
            sector: Some("Financials".into()),
            market: LiveMarket::Us,
            recommended_action: "enter".into(),
            workflow_state: "confirm".into(),
            market_regime_bias: "risk_on".into(),
            market_regime_confidence: dec!(0.70),
            market_breadth_delta: dec!(0.20),
            market_average_return: dec!(0.03),
            market_directional_consensus: Some(dec!(0.18)),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
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
                composite_states: vec![],
                human_review: Some(crate::ontology::HumanReviewContext {
                    verdict: crate::ontology::HumanReviewVerdict::Confirmed,
                    verdict_label: "Confirmed".into(),
                    confidence: dec!(0.6),
                    reasons: vec![],
                    note: Some("confirmed".into()),
                }),
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::MechanicalExecutionSignature,
                    label: "Mechanical Execution Signature".into(),
                    score: dec!(0.7),
                    summary: "s".into(),
                    supporting_states: vec![CompositeStateKind::DirectionalReinforcement],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![crate::ontology::MechanismFactor {
                        key: "state:directional_reinforcement".into(),
                        label: "Directional Reinforcement".into(),
                        source: crate::ontology::MechanismFactorSource::State,
                        activation: dec!(0.7),
                        base_weight: dec!(0.45),
                        learned_weight_delta: Decimal::ZERO,
                        effective_weight: dec!(0.45),
                        contribution: dec!(0.315),
                    }],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
            },
            updated_at: "2026-03-22T00:00:00Z".into(),
        };

        let runtime = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH,
            "runtime",
        );
        let workflow = CaseReasoningAssessmentRecord::from_case_summary(
            &summary,
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
            "workflow_update",
        );

        let feedback =
            derive_learning_feedback(&[runtime, workflow], &OutcomeLearningContext::default());
        assert_eq!(feedback.paired_examples, 1);
        assert_eq!(feedback.reinforced_examples, 1);
        assert!(feedback.mechanism_adjustments.iter().any(|item| item.label
            == "Mechanical Execution Signature"
            && item.delta > Decimal::ZERO));
    }

    #[test]
    fn outcome_context_scales_feedback() {
        let context = OutcomeLearningContext {
            reward_multiplier: dec!(0.50),
            penalty_multiplier: dec!(0.25),
            source: "test".into(),
            ..OutcomeLearningContext::default()
        };

        assert_eq!(
            clamp_delta(dec!(0.04) * (Decimal::ONE + context.reward_multiplier)),
            dec!(0.06)
        );
        assert_eq!(
            clamp_delta(dec!(-0.04) * (Decimal::ONE + context.penalty_multiplier)),
            dec!(-0.05)
        );
    }

    #[test]
    fn hk_outcome_context_reads_follow_through_and_invalidation() {
        let rows = vec![
            LineageMetricRowRecord {
                row_id: "1".into(),
                snapshot_id: "s1".into(),
                tick_number: 1,
                recorded_at: OffsetDateTime::UNIX_EPOCH,
                window_size: 10,
                bucket: "promoted_outcomes".into(),
                rank: 0,
                label: "x".into(),
                family: None,
                session: None,
                market_regime: None,
                total: 10,
                resolved: 8,
                hits: 6,
                hit_rate: "0.75".into(),
                mean_return: "0.03".into(),
                mean_net_return: "0.04".into(),
                mean_mfe: "0.05".into(),
                mean_mae: "-0.02".into(),
                follow_through_rate: "0.70".into(),
                invalidation_rate: "0.10".into(),
                structure_retention_rate: "0.80".into(),
                mean_convergence_score: "0.60".into(),
                mean_external_delta: "0.02".into(),
                external_follow_through_rate: "0.40".into(),
            },
            LineageMetricRowRecord {
                row_id: "2".into(),
                snapshot_id: "s1".into(),
                tick_number: 1,
                recorded_at: OffsetDateTime::UNIX_EPOCH,
                window_size: 10,
                bucket: "falsified_outcomes".into(),
                rank: 0,
                label: "y".into(),
                family: None,
                session: None,
                market_regime: None,
                total: 10,
                resolved: 8,
                hits: 5,
                hit_rate: "0.62".into(),
                mean_return: "0.02".into(),
                mean_net_return: "0.03".into(),
                mean_mfe: "0.04".into(),
                mean_mae: "-0.02".into(),
                follow_through_rate: "0.60".into(),
                invalidation_rate: "0.75".into(),
                structure_retention_rate: "0.30".into(),
                mean_convergence_score: "0.50".into(),
                mean_external_delta: "0.01".into(),
                external_follow_through_rate: "0.20".into(),
            },
        ];

        let context = derive_outcome_learning_context_from_hk_rows(&rows);
        assert_eq!(context.source, "hk_lineage");
        assert!(context.reward_multiplier > Decimal::ZERO);
        assert!(context.penalty_multiplier > Decimal::ZERO);
        assert_eq!(context.promoted_follow_through, dec!(0.70));
        assert_eq!(context.falsified_invalidation, dec!(0.75));
    }
}
