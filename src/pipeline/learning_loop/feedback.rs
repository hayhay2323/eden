use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizonLearningMode {
    /// < 50 samples: log only, no feedback effect
    Diagnostics,
    /// 50-99 samples: adjustment recorded with shadow=true
    Shadow,
    /// >= 100 samples: adjustment recorded with shadow=false (live learning)
    Full,
}

/// Single choke point for the supplemental-horizon sample gate.
/// Wave 3 spec rule: <50 / 50-99 / >=100.
pub fn supplemental_horizon_learning_mode(samples: usize) -> HorizonLearningMode {
    match samples {
        0..=49 => HorizonLearningMode::Diagnostics,
        50..=99 => HorizonLearningMode::Shadow,
        _ => HorizonLearningMode::Full,
    }
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

pub fn derive_learning_feedback(
    assessments: &[CaseReasoningAssessmentRecord],
    outcome_context: &OutcomeLearningContext,
) -> ReasoningLearningFeedback {
    let mut mechanism_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut mechanism_factor_totals: HashMap<(String, String, String), (Decimal, Decimal)> =
        HashMap::new();
    let mut predicate_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut intent_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut archetype_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
    let mut signature_totals: HashMap<(String, String, String), (Decimal, Decimal)> =
        HashMap::new();
    let mut expectation_violation_totals: HashMap<String, (Decimal, Decimal)> = HashMap::new();
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
    let deferred_examples = paired_examples
        .iter()
        .filter(|example| example.workflow.operator_decision_kind.as_deref() == Some("defer"))
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

            // Regime fingerprint bucket: when the runtime captured the
            // active regime fingerprint at assessment time, learn how
            // this mechanism performs in that regime. New scope as of
            // 2026-04-23 (regime-conditional learning is the
            // architectural fix for operator's manually-derived rule
            // v0.21 "ban SHORT in reversal_prone"). Weighted heavier
            // than predicate (0.7 vs 0.4) since regime is a stronger
            // gating context than any single predicate.
            if let Some(bucket) = example.runtime.regime_bucket.as_ref() {
                let entry = conditioned_totals
                    .entry((mechanism.clone(), "regime_bucket".into(), bucket.clone()))
                    .or_insert((Decimal::ZERO, Decimal::ZERO));
                entry.0 += delta * dec!(0.7);
                entry.1 += recency_weight * dec!(0.7);
            }
        }

        for predicate in &example.runtime.predicate_kinds {
            let entry = predicate_totals
                .entry(predicate.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * dec!(0.6);
            entry.1 += recency_weight * dec!(0.6);
        }

        if let Some(intent) = example.runtime.inferred_intent.as_ref() {
            let key = format!("{:?}", intent.kind).to_ascii_lowercase();
            let weight = intent.strength.composite.max(dec!(0.10));
            let entry = intent_totals
                .entry(key)
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * weight;
            entry.1 += recency_weight * weight;
        }

        for projection in &example.runtime.archetype_projections {
            let entry = archetype_totals
                .entry(projection.archetype_key.clone())
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * projection.affinity.max(dec!(0.10));
            entry.1 += recency_weight * projection.affinity.max(dec!(0.10));
        }

        if let Some(signature) = example.runtime.case_signature.as_ref() {
            let key = (
                format!("{:?}", signature.topology).to_ascii_lowercase(),
                format!("{:?}", signature.temporal_shape).to_ascii_lowercase(),
                format!("{:?}", signature.conflict_shape).to_ascii_lowercase(),
            );
            let novelty_weight = signature.novelty_score.max(dec!(0.10));
            let entry = signature_totals
                .entry(key)
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * novelty_weight;
            entry.1 += recency_weight * novelty_weight;
        }

        for violation in &example.runtime.expectation_violations {
            let key = format!("{:?}", violation.kind).to_ascii_lowercase();
            let weight = violation.magnitude.max(dec!(0.10));
            let entry = expectation_violation_totals
                .entry(key)
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += delta * weight;
            entry.1 += recency_weight * weight;
        }
    }

    ReasoningLearningFeedback {
        paired_examples: paired_examples.len(),
        reinforced_examples,
        corrected_examples,
        deferred_examples,
        mechanism_adjustments: finalize_adjustments(mechanism_totals),
        mechanism_factor_adjustments: finalize_factor_adjustments(mechanism_factor_totals),
        predicate_adjustments: finalize_adjustments(predicate_totals),
        intent_adjustments: finalize_adjustments(intent_totals),
        archetype_adjustments: finalize_adjustments(archetype_totals),
        signature_adjustments: finalize_signature_adjustments(signature_totals),
        expectation_violation_adjustments: finalize_adjustments(expectation_violation_totals),
        conditioned_adjustments: finalize_conditioned_adjustments(conditioned_totals),
        outcome_context: outcome_context.clone(),
        horizon_adjustments: vec![],
    }
}

pub fn apply_learning_feedback(
    profile: &CaseReasoningProfile,
    invalidation_rules: &[String],
    feedback: &ReasoningLearningFeedback,
    active_regime_bucket: Option<&str>,
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
            + feedback.conditioned_delta(
                &candidate.label,
                &active_states,
                &active_predicates,
                active_regime_bucket,
            );
        if delta != Decimal::ZERO {
            candidate.score = clamp_unit_interval(candidate.score + delta);
        }
    }

    retain_explanatory_mechanisms(&mut candidates);

    next.primary_mechanism = candidates.first().cloned();
    next.competing_mechanisms = candidates.into_iter().skip(1).take(3).collect();
    next
}

pub fn apply_feedback_to_hypothesis(
    hypothesis: &mut crate::ontology::Hypothesis,
    feedback: &ReasoningLearningFeedback,
) {
    let intent = hypothesis.intent_hypothesis();
    let signature = hypothesis.case_signature();
    let signature_delta = feedback.signature_delta(
        &format!("{:?}", signature.topology).to_ascii_lowercase(),
        &format!("{:?}", signature.temporal_shape).to_ascii_lowercase(),
        &format!("{:?}", signature.conflict_shape).to_ascii_lowercase(),
    );
    let intent_delta = feedback.intent_delta(&format!("{:?}", intent.kind).to_ascii_lowercase());
    // V2 Pass 2: family_key removed from Hypothesis. Use family_label
    // (still on Hypothesis as operator-facing display) as the
    // archetype-delta lookup key.
    let archetype_delta = feedback.archetype_delta(&hypothesis.family_label);
    let violation_delta =
        hypothesis
            .expectation_violations()
            .iter()
            .fold(Decimal::ZERO, |acc, violation| {
                acc + feedback.expectation_violation_delta(
                    &format!("{:?}", violation.kind).to_ascii_lowercase(),
                )
            });

    let total_delta = clamp_delta(
        intent_delta * dec!(0.7)
            + archetype_delta * dec!(0.6)
            + signature_delta * dec!(0.4)
            + violation_delta * dec!(0.3),
    );
    if total_delta != Decimal::ZERO {
        hypothesis.confidence = clamp_unit_interval(hypothesis.confidence + total_delta);
        hypothesis.local_support_weight =
            clamp_unit_interval(hypothesis.local_support_weight + total_delta.max(Decimal::ZERO));
        if total_delta < Decimal::ZERO {
            hypothesis.local_contradict_weight =
                clamp_unit_interval(hypothesis.local_contradict_weight + total_delta.abs());
        }
    }
}

// 2026-04-29: deleted `apply_feedback_to_tactical_setup`. It was a 5-channel
// magic-weighted modulator (intent_delta * 0.6 + archetype_delta * 0.4 +
// signature_delta * 0.3 + violation_delta * 0.2 + conditioned_delta * 0.5)
// that overwrote `setup.confidence` after BP had already set it from the
// posterior, breaking the "BP posterior is single source of truth" contract.
// It slipped through the architecture invariants test because the test only
// blacklists the named-deleted `apply_*_modulation` functions; this one lived
// under a different name. Audit finding CRITICAL #1 from the 2026-04-29
// legacy sweep. Future per-tick post-BP confidence adjustments must enter
// BP via NodeId activations, not via a direct overwrite.

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

fn finalize_signature_adjustments(
    source: HashMap<(String, String, String), (Decimal, Decimal)>,
) -> Vec<SignatureLearningAdjustment> {
    let mut items = source
        .into_iter()
        .map(
            |((topology, temporal_shape, conflict_shape), (total_delta, weight_sum))| {
                SignatureLearningAdjustment {
                    topology,
                    temporal_shape,
                    conflict_shape,
                    delta: clamp_delta(if weight_sum > Decimal::ZERO {
                        total_delta / weight_sum
                    } else {
                        Decimal::ZERO
                    }),
                    samples: decimal_ceil_usize(weight_sum),
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
            .then_with(|| left.topology.cmp(&right.topology))
            .then_with(|| left.temporal_shape.cmp(&right.temporal_shape))
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
    match workflow.operator_decision_kind.as_deref() {
        Some("accept") => return LearningOutcome::Reinforced,
        Some("reject") => return LearningOutcome::Corrected,
        Some("defer") => return LearningOutcome::Unresolved,
        _ => {}
    }

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
    if example.workflow.operator_decision_kind.as_deref() == Some("defer") {
        delta -= dec!(0.02);
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

/// Resolution kind → learning delta policy.
///
/// Locked rules (Wave 4):
/// - Confirmed + Final       → +1.0 full credit
/// - Confirmed + Provisional → +0.5 half credit
/// - Invalidated + Final     → −1.0 full debit
/// - Invalidated + Provisional → 0.0 (wait for upgrade)
/// - Exhausted               → 0.0
/// - ProfitableButLate       → +0.3 intent credit (bucket debit handled separately
///                             via `profitable_but_late_bucket_deltas`)
/// - PartiallyConfirmed + Final     → +0.5 partial credit
/// - PartiallyConfirmed + Provisional → +0.25 partial provisional credit
/// - EarlyExited             → 0.0
/// - StructurallyRightButUntradeable → 0.0
///
/// **Primary path.** Call this when a `case_resolution` record exists for the
/// case. Fall back to `legacy_delta_from_booleans` only when no resolution
/// record exists.
///
/// CRITICAL: never merge both paths. Read one or the other, not both.
#[cfg(feature = "persistence")]
pub fn delta_from_case_resolution(
    resolution: &crate::ontology::resolution::CaseResolution,
) -> rust_decimal::Decimal {
    use crate::ontology::resolution::{CaseResolutionKind, ResolutionFinality};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    match (resolution.kind, resolution.finality) {
        (CaseResolutionKind::Confirmed, ResolutionFinality::Final) => dec!(1.0),
        (CaseResolutionKind::Confirmed, ResolutionFinality::Provisional) => dec!(0.5),
        (CaseResolutionKind::Invalidated, ResolutionFinality::Final) => dec!(-1.0),
        (CaseResolutionKind::Invalidated, ResolutionFinality::Provisional) => Decimal::ZERO,
        (CaseResolutionKind::Exhausted, _) => Decimal::ZERO,
        // Intent credit only — bucket debit handled separately by
        // `profitable_but_late_bucket_deltas`.
        (CaseResolutionKind::ProfitableButLate, _) => dec!(0.3),
        (CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Final) => dec!(0.5),
        (CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Provisional) => dec!(0.25),
        (CaseResolutionKind::EarlyExited, _) => Decimal::ZERO,
        (CaseResolutionKind::StructurallyRightButUntradeable, _) => Decimal::ZERO,
    }
}

/// Legacy learning delta path: computes delta from the paired (runtime /
/// workflow) assessment comparison — does NOT read `CaseRealizedOutcomeRecord`
/// booleans directly.
///
/// In the original codebase, the delta was always derived from human review
/// workflow state transitions (`Reinforced` / `Corrected` / `Unresolved`),
/// not from persisted boolean outcome fields. This function documents that
/// the legacy path is the `learning_delta` computation on paired examples,
/// which still runs via `derive_learning_feedback`.
///
/// **Fallback path.** Use only when no `case_resolution` record exists.
///
/// CRITICAL: never merge with `delta_from_case_resolution`. Read one or
/// the other, not both.
///
/// # Note on the legacy "boolean path"
/// The plan references `CaseRealizedOutcomeRecord.followed_through /
/// invalidated / structure_retained`. In this codebase those booleans are
/// used in `outcome_context.rs` to compute `OutcomeLearningContext`
/// multipliers (penalty / reward scaling), NOT to compute per-case deltas.
/// The actual per-case delta was always computed from workflow state pairs
/// via `learning_delta(example, outcome_context)`. That remains unchanged
/// under this function name.
/// This function is `fn` (private to feedback.rs) because `PairedLearningExample`
/// is a private type. External callers use `delta_from_case_resolution`
/// (primary path) or `derive_learning_feedback` (which internally dispatches
/// to this via `learning_delta`).
#[allow(dead_code)]
fn legacy_delta_from_booleans(
    example: &PairedLearningExample,
    outcome_context: &OutcomeLearningContext,
) -> Decimal {
    learning_delta(example, outcome_context)
}

/// For ProfitableButLate: the bucket that was chosen (primary) gets a
/// debit (horizon selection was wrong), while the bucket that actually
/// confirmed gets a credit.
///
/// This function produces *additional* bucket-level delta pairs that should
/// flow into `horizon_adjustments`. It is called **only** when the case
/// resolution is `ProfitableButLate`.
///
/// The intent-level credit (+0.3) is applied independently via
/// `delta_from_case_resolution`. These two functions compose — they are NOT
/// alternatives to each other.
///
/// Returns a vec of `(bucket, delta)` pairs. When `confirming_bucket` is the
/// same as `primary_bucket` (degenerate case), only the debit is returned.
#[cfg(feature = "persistence")]
pub fn profitable_but_late_bucket_deltas(
    primary_bucket: crate::ontology::horizon::HorizonBucket,
    confirming_bucket: Option<crate::ontology::horizon::HorizonBucket>,
) -> Vec<(
    crate::ontology::horizon::HorizonBucket,
    rust_decimal::Decimal,
)> {
    use rust_decimal_macros::dec;

    let mut out = vec![(primary_bucket, dec!(-0.3))];
    if let Some(confirming) = confirming_bucket {
        if confirming != primary_bucket {
            out.push((confirming, dec!(0.3)));
        }
    }
    out
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

/// Tests for Tasks 19 and 20: delta_from_case_resolution + bucket debit split.
/// The entire module is gated on `feature = "persistence"` because the
/// functions under test are only compiled with that feature.
#[cfg(all(test, feature = "persistence"))]
mod resolution_delta_tests {
    use super::*;
    use crate::ontology::horizon::HorizonBucket;
    use crate::ontology::resolution::{CaseResolution, CaseResolutionKind, ResolutionFinality};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn make(kind: CaseResolutionKind, finality: ResolutionFinality) -> CaseResolution {
        CaseResolution {
            kind,
            finality,
            narrative: "test".into(),
            net_return: Decimal::ZERO,
        }
    }

    // --- Task 19: delta_from_case_resolution policy ---

    #[test]
    fn confirmed_final_full_credit() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, dec!(1.0));
    }

    #[test]
    fn confirmed_provisional_half_credit() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::Confirmed,
            ResolutionFinality::Provisional,
        ));
        assert_eq!(d, dec!(0.5));
    }

    #[test]
    fn invalidated_final_full_debit() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::Invalidated,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, dec!(-1.0));
    }

    #[test]
    fn invalidated_provisional_neutral() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::Invalidated,
            ResolutionFinality::Provisional,
        ));
        assert_eq!(d, Decimal::ZERO);
    }

    #[test]
    fn exhausted_zero() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::Exhausted,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, Decimal::ZERO);
    }

    #[test]
    fn profitable_but_late_intent_credit() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::ProfitableButLate,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, dec!(0.3));
    }

    #[test]
    fn structurally_right_zero() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::StructurallyRightButUntradeable,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, Decimal::ZERO);
    }

    #[test]
    fn partially_confirmed_final_half_credit() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::PartiallyConfirmed,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, dec!(0.5));
    }

    #[test]
    fn early_exited_zero() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::EarlyExited,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, Decimal::ZERO);
    }

    // --- Task 20: profitable_but_late_bucket_deltas ---

    #[test]
    fn profitable_but_late_debits_primary_credits_confirming() {
        let deltas =
            profitable_but_late_bucket_deltas(HorizonBucket::Fast5m, Some(HorizonBucket::Mid30m));
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0], (HorizonBucket::Fast5m, dec!(-0.3)));
        assert_eq!(deltas[1], (HorizonBucket::Mid30m, dec!(0.3)));
    }

    #[test]
    fn profitable_but_late_same_bucket_is_noop_credit() {
        // Edge case: confirming_bucket == primary_bucket (shouldn't happen
        // in practice but defend the contract).
        let deltas =
            profitable_but_late_bucket_deltas(HorizonBucket::Fast5m, Some(HorizonBucket::Fast5m));
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].1, dec!(-0.3));
    }

    #[test]
    fn profitable_but_late_no_confirming_bucket_debit_only() {
        let deltas = profitable_but_late_bucket_deltas(HorizonBucket::Session, None);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], (HorizonBucket::Session, dec!(-0.3)));
    }
}

#[cfg(test)]
mod horizon_gate_tests {
    use super::*;
    use crate::ontology::horizon::HorizonBucket;
    use crate::pipeline::learning_loop::types::{
        HorizonLearningAdjustment, ReasoningLearningFeedback,
    };
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    #[test]
    fn gate_below_50_is_diagnostics() {
        assert_eq!(
            supplemental_horizon_learning_mode(0),
            HorizonLearningMode::Diagnostics
        );
        assert_eq!(
            supplemental_horizon_learning_mode(49),
            HorizonLearningMode::Diagnostics
        );
    }

    #[test]
    fn gate_50_to_99_is_shadow() {
        assert_eq!(
            supplemental_horizon_learning_mode(50),
            HorizonLearningMode::Shadow
        );
        assert_eq!(
            supplemental_horizon_learning_mode(99),
            HorizonLearningMode::Shadow
        );
    }

    #[test]
    fn gate_100_plus_is_full() {
        assert_eq!(
            supplemental_horizon_learning_mode(100),
            HorizonLearningMode::Full
        );
        assert_eq!(
            supplemental_horizon_learning_mode(250),
            HorizonLearningMode::Full
        );
        assert_eq!(
            supplemental_horizon_learning_mode(usize::MAX),
            HorizonLearningMode::Full
        );
    }

    #[test]
    fn horizon_delta_ignores_shadow_adjustments() {
        let feedback = ReasoningLearningFeedback {
            horizon_adjustments: vec![
                HorizonLearningAdjustment {
                    intent_kind: "directional_accumulation".into(),
                    bucket: HorizonBucket::Fast5m,
                    delta: dec!(0.10),
                    samples: 75,
                    shadow: true, // shadow → must NOT count
                },
                HorizonLearningAdjustment {
                    intent_kind: "directional_accumulation".into(),
                    bucket: HorizonBucket::Mid30m,
                    delta: dec!(0.05),
                    samples: 150,
                    shadow: false, // full → counts
                },
            ],
            ..ReasoningLearningFeedback::default()
        };
        // Fast5m is shadow → returns 0
        assert_eq!(
            feedback.horizon_delta("directional_accumulation", HorizonBucket::Fast5m),
            Decimal::ZERO,
        );
        // Mid30m is full → returns 0.05
        assert_eq!(
            feedback.horizon_delta("directional_accumulation", HorizonBucket::Mid30m),
            dec!(0.05),
        );
    }

    #[test]
    fn horizon_delta_zero_when_no_adjustments() {
        let feedback = ReasoningLearningFeedback::default();
        assert_eq!(
            feedback.horizon_delta("any_intent", HorizonBucket::Session),
            Decimal::ZERO,
        );
    }

    #[test]
    fn workflow_operator_decision_kinds_drive_learning_outcome() {
        let runtime = CaseReasoningAssessmentRecord {
            assessment_id: "a".into(),
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            market: "us".into(),
            symbol: "AAPL.US".into(),
            title: "runtime".into(),
            family_label: None,
            sector: None,
            recommended_action: "enter".into(),
            workflow_state: "suggest".into(),
            market_regime_bias: None,
            market_regime_confidence: None,
            market_breadth_delta: None,
            market_average_return: None,
            market_directional_consensus: None,
            source: "runtime".into(),
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            owner: None,
            reviewer: None,
            actor: None,
            note: None,
            operator_decision_kind: None,
            freshness_state: None,
            timing_state: None,
            timing_position_in_range: None,
            local_state: None,
            local_state_confidence: None,
            actionability_score: None,
            actionability_state: None,
            state_persistence_ticks: None,
            direction_stability_rounds: None,
            state_reason_codes: vec![],
            law_kinds: vec![],
            predicate_kinds: vec![],
            composite_state_kinds: vec![],
            primary_mechanism_kind: Some("Trade Flow".into()),
            primary_mechanism_score: None,
            competing_mechanism_kinds: vec![],
            invalidation_rules: vec![],
            case_signature: None,
            archetype_projections: vec![],
            inferred_intent: None,
            primary_horizon: None,
            expectation_bindings: vec![],
            expectation_violations: vec![],
            reasoning_profile: CaseReasoningProfile::default(),
            regime_bucket: None,
        };
        let mut accept = runtime.clone();
        accept.source = "workflow_update".into();
        accept.operator_decision_kind = Some("accept".into());
        let mut reject = accept.clone();
        reject.operator_decision_kind = Some("reject".into());
        let mut defer = accept.clone();
        defer.operator_decision_kind = Some("defer".into());

        assert!(matches!(
            learning_outcome(&runtime, &accept),
            LearningOutcome::Reinforced
        ));
        assert!(matches!(
            learning_outcome(&runtime, &reject),
            LearningOutcome::Corrected
        ));
        assert!(matches!(
            learning_outcome(&runtime, &defer),
            LearningOutcome::Unresolved
        ));
        assert!(
            learning_delta(
                &PairedLearningExample {
                    runtime,
                    workflow: defer,
                    outcome: LearningOutcome::Unresolved,
                },
                &OutcomeLearningContext::default(),
            ) < Decimal::ZERO
        );
    }
}
