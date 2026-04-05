use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::world::{
    BackwardCause, BackwardEvidenceItem, BackwardInvestigation, CausalContestState, WorldLayer,
};
use crate::ontology::{
    scope_node_id, EvidencePolarity, Hypothesis, InvestigationSelection, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope, Symbol,
};
use crate::pipeline::reasoning::ReasoningSnapshot;

pub(crate) fn select_backward_investigation_targets<'a>(
    reasoning: &'a ReasoningSnapshot,
    hypothesis_map: &HashMap<&'a str, &'a Hypothesis>,
    previous_investigation_map: &HashMap<String, &'a BackwardInvestigation>,
) -> Vec<&'a InvestigationSelection> {
    const MAX_BACKWARD_INVESTIGATIONS: usize = 6;
    let enter_selection_count = reasoning
        .investigation_selections
        .iter()
        .filter(|selection| selection.attention_hint == "enter")
        .count();
    let backward_budget = MAX_BACKWARD_INVESTIGATIONS.max(enter_selection_count.saturating_add(2));

    let mut candidates = reasoning
        .investigation_selections
        .iter()
        .filter_map(|selection| {
            let hypothesis = hypothesis_map
                .get(selection.hypothesis_id.as_str())
                .copied()?;
            let previous = previous_investigation_map
                .get(scope_key(&selection.scope).as_str())
                .copied();
            let propagated_signal = !hypothesis.propagation_path_ids.is_empty()
                || hypothesis.propagated_support_weight > Decimal::ZERO
                || hypothesis.propagated_contradict_weight > Decimal::ZERO;
            let meaningful_observe = selection.attention_hint == "observe"
                && (propagated_signal
                    || selection.priority_score >= Decimal::new(5, 2)
                    || selection.confidence_gap >= Decimal::new(5, 2));

            if !matches!(selection.attention_hint.as_str(), "enter" | "review")
                && !meaningful_observe
            {
                return None;
            }

            let mut score = selection.priority_score.max(Decimal::ZERO)
                + selection.confidence_gap.max(Decimal::ZERO)
                + hypothesis.propagated_support_weight
                + (hypothesis.local_support_weight * Decimal::new(5, 1));
            if selection.attention_hint == "enter" {
                score += Decimal::new(20, 2);
            } else if selection.attention_hint == "review" {
                score += Decimal::new(10, 2);
            }
            if propagated_signal {
                score += Decimal::new(15, 2);
            }
            if previous.is_some() {
                score += Decimal::new(8, 2);
            }

            Some((score, selection))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(score_a, selection_a), (score_b, selection_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| {
                backward_action_priority(selection_a.attention_hint.as_str()).cmp(
                    &backward_action_priority(selection_b.attention_hint.as_str()),
                )
            })
            .then_with(|| selection_b.confidence.cmp(&selection_a.confidence))
            .then_with(|| {
                selection_a
                    .investigation_id
                    .cmp(&selection_b.investigation_id)
            })
    });

    candidates
        .into_iter()
        .take(backward_budget)
        .map(|(_, selection)| selection)
        .collect()
}

pub(super) fn backward_action_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

pub(crate) fn scope_key(scope: &ReasoningScope) -> String {
    scope_node_id(scope)
}

pub(crate) fn world_provenance<I, S>(
    observed_at: time::OffsetDateTime,
    trace_id: &str,
    inputs: I,
    note: &str,
    confidence: Decimal,
) -> crate::ontology::ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_inputs(inputs)
    .with_confidence(confidence)
    .with_note(note)
}

pub(crate) fn world_layer_priority(layer: WorldLayer) -> i32 {
    match layer {
        WorldLayer::Forest => 0,
        WorldLayer::Trunk => 1,
        WorldLayer::Branch => 2,
        WorldLayer::Leaf => 3,
    }
}

pub(super) fn backward_layer_priority(layer: WorldLayer) -> i32 {
    match layer {
        WorldLayer::Forest => 0,
        WorldLayer::Trunk => 1,
        WorldLayer::Branch => 2,
        WorldLayer::Leaf => 3,
    }
}

pub(super) fn backward_evidence_item(
    statement: impl Into<String>,
    weight: Decimal,
    channel: impl Into<String>,
) -> BackwardEvidenceItem {
    BackwardEvidenceItem {
        statement: statement.into(),
        weight: weight.round_dp(4).clamp(Decimal::ZERO, Decimal::ONE),
        channel: channel.into(),
    }
}

pub(super) fn evidence_items_by_filter<F>(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
    predicate: F,
) -> Vec<BackwardEvidenceItem>
where
    F: Fn(&ReasoningEvidence) -> bool,
{
    hypothesis
        .evidence
        .iter()
        .filter(|evidence| evidence.polarity == polarity && predicate(evidence))
        .map(|evidence| {
            backward_evidence_item(
                evidence.statement.clone(),
                evidence.weight,
                match evidence.kind {
                    ReasoningEvidenceKind::LocalEvent => "local-event",
                    ReasoningEvidenceKind::LocalSignal => "local-signal",
                    ReasoningEvidenceKind::PropagatedPath => "propagated-path",
                },
            )
        })
        .collect()
}

pub(super) fn local_evidence_items(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
) -> Vec<BackwardEvidenceItem> {
    evidence_items_by_filter(hypothesis, polarity, |evidence| {
        matches!(
            evidence.kind,
            ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal
        )
    })
}

pub(super) fn propagated_evidence_items(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
    path_id: Option<&str>,
) -> Vec<BackwardEvidenceItem> {
    evidence_items_by_filter(hypothesis, polarity, |evidence| {
        if !matches!(evidence.kind, ReasoningEvidenceKind::PropagatedPath) {
            return false;
        }
        path_id.is_none_or(|id| evidence.references.iter().any(|reference| reference == id))
    })
}

pub(super) fn attach_contest_metrics(
    cause: &mut BackwardCause,
    supporting_evidence: Vec<BackwardEvidenceItem>,
    contradicting_evidence: Vec<BackwardEvidenceItem>,
) {
    cause.support_weight = supporting_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE)
        .round_dp(4);
    cause.contradict_weight = contradicting_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE)
        .round_dp(4);
    cause.net_conviction = (cause.support_weight - cause.contradict_weight)
        .clamp(-Decimal::ONE, Decimal::ONE)
        .round_dp(4);
    cause.supporting_evidence = supporting_evidence;
    cause.contradicting_evidence = contradicting_evidence;
    cause.competitive_score = backward_cause_score(cause);
}

pub(super) fn backward_cause_score(cause: &BackwardCause) -> Decimal {
    let layer_bonus = match cause.layer {
        WorldLayer::Forest => Decimal::new(16, 2),
        WorldLayer::Trunk => Decimal::new(12, 2),
        WorldLayer::Branch => Decimal::new(8, 2),
        WorldLayer::Leaf => Decimal::new(4, 2),
    };
    let reference_bonus = Decimal::from(cause.references.len().min(3) as i64) * Decimal::new(2, 2);
    let chain_bonus = if cause.chain_summary.is_some() {
        Decimal::new(3, 2)
    } else {
        Decimal::ZERO
    };
    let depth_penalty = Decimal::from(cause.depth.min(4) as i64) * Decimal::new(3, 2);
    let support_bonus = cause.support_weight * Decimal::new(28, 2);
    let conviction_bonus = cause.net_conviction.max(Decimal::ZERO) * Decimal::new(18, 2);
    let contradiction_penalty = cause.contradict_weight * Decimal::new(32, 2);

    (cause.confidence * Decimal::new(58, 2)
        + support_bonus
        + conviction_bonus
        + layer_bonus
        + reference_bonus
        + chain_bonus
        - contradiction_penalty
        - depth_penalty)
        .clamp(Decimal::ZERO, Decimal::ONE)
        .round_dp(4)
}

pub(super) fn backward_cause_gap(leading: &BackwardCause, runner_up: &BackwardCause) -> Decimal {
    let score_gap = leading.competitive_score - runner_up.competitive_score;
    let contradiction_swing = runner_up.contradict_weight - leading.contradict_weight;
    let conviction_swing = leading.net_conviction - runner_up.net_conviction;

    (score_gap + contradiction_swing * Decimal::new(35, 2) + conviction_swing * Decimal::new(25, 2))
        .max(Decimal::ZERO)
        .round_dp(4)
}

pub(super) fn stable_cause_token(value: &str) -> String {
    let token = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let compact = token
        .split('_')
        .filter(|segment| !segment.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("_");

    if compact.is_empty() {
        "unknown".into()
    } else {
        compact
    }
}

pub(super) fn local_cause_key(evidence: &ReasoningEvidence) -> String {
    evidence
        .references
        .first()
        .map(|value| stable_cause_token(value))
        .unwrap_or_else(|| stable_cause_token(&evidence.statement))
}

pub(super) fn previous_cause_by_id<'a>(
    previous_investigation: Option<&'a BackwardInvestigation>,
    cause_id: &str,
) -> Option<&'a BackwardCause> {
    previous_investigation.and_then(|investigation| {
        investigation
            .candidate_causes
            .iter()
            .find(|cause| cause.cause_id == cause_id)
    })
}

pub(super) fn leading_cause_streak(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
) -> u64 {
    match (
        previous_investigation.and_then(|item| item.leading_cause.as_ref()),
        current_leading,
    ) {
        (_, None) => 0,
        (Some(previous), Some(current)) if previous.cause_id == current.cause_id => {
            previous_investigation
                .map(|item| item.leading_cause_streak.saturating_add(1))
                .unwrap_or(1)
        }
        _ => 1,
    }
}

pub(super) fn leading_cause_deltas(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
) -> (Option<Decimal>, Option<Decimal>) {
    let Some(current) = current_leading else {
        return (None, None);
    };
    let previous = previous_cause_by_id(previous_investigation, current.cause_id.as_str());
    let support_delta =
        previous.map(|cause| (current.support_weight - cause.support_weight).round_dp(4));
    let contradict_delta =
        previous.map(|cause| (current.contradict_weight - cause.contradict_weight).round_dp(4));
    (support_delta, contradict_delta)
}

pub(super) fn classify_causal_contest(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
    cause_gap: Option<Decimal>,
    leading_support_delta: Option<Decimal>,
    leading_contradict_delta: Option<Decimal>,
) -> CausalContestState {
    let Some(current) = current_leading else {
        return CausalContestState::Contested;
    };
    let strong_gap = Decimal::new(8, 2);
    let narrow_gap = Decimal::new(3, 2);
    let contradiction_threshold = Decimal::new(5, 2);
    let support_softening_threshold = Decimal::new(-2, 2);
    let previous_leading = previous_investigation.and_then(|item| item.leading_cause.as_ref());

    match previous_leading {
        None => CausalContestState::New,
        Some(previous) if previous.cause_id != current.cause_id => {
            if cause_gap.unwrap_or(Decimal::ZERO) >= strong_gap {
                CausalContestState::Flipped
            } else {
                CausalContestState::Contested
            }
        }
        Some(_) if cause_gap.unwrap_or(Decimal::ZERO) < narrow_gap => CausalContestState::Contested,
        Some(_)
            if leading_contradict_delta.unwrap_or(Decimal::ZERO) >= contradiction_threshold
                && leading_support_delta.unwrap_or(Decimal::ZERO)
                    <= support_softening_threshold =>
        {
            CausalContestState::Eroding
        }
        Some(_) => CausalContestState::Stable,
    }
}

pub(super) fn render_leader_transition(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
    cause_gap: Option<Decimal>,
    leading_support_delta: Option<Decimal>,
    leading_contradict_delta: Option<Decimal>,
    contest_state: CausalContestState,
) -> Option<String> {
    let current = current_leading?;
    let previous = previous_investigation.and_then(|item| item.leading_cause.as_ref());

    match previous {
        None => Some(format!(
            "new causal contest seeded with {} leading",
            current.explanation
        )),
        Some(previous) if previous.cause_id != current.cause_id => Some(format!(
            "leadership flipped from {} to {} (gap={:+})",
            previous.explanation,
            current.explanation,
            cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        )),
        Some(_) if contest_state == CausalContestState::Eroding => Some(format!(
            "leader {} is eroding (d_support={:+} d_against={:+})",
            current.explanation,
            leading_support_delta.unwrap_or(Decimal::ZERO).round_dp(3),
            leading_contradict_delta
                .unwrap_or(Decimal::ZERO)
                .round_dp(3),
        )),
        Some(_) if contest_state == CausalContestState::Contested => Some(format!(
            "leader {} remains contested (gap={:+})",
            current.explanation,
            cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        )),
        Some(_) => Some(format!(
            "leader {} remains stable with streak continuation",
            current.explanation
        )),
    }
}

pub(super) fn render_path_chain(path: &crate::ontology::PropagationPath) -> String {
    let mut segments = Vec::new();
    if let Some(first) = path.steps.first() {
        segments.push(scope_key(&first.from));
    }
    for step in &path.steps {
        segments.push(format!("{} via {}", scope_key(&step.to), step.mechanism));
    }
    segments.join(" -> ")
}

pub(super) fn symbol_to_sector_hint(reasoning: &ReasoningSnapshot, symbol: &Symbol) -> String {
    reasoning
        .propagation_paths
        .iter()
        .flat_map(|path| path.steps.iter())
        .find_map(|step| match (&step.from, &step.to) {
            (ReasoningScope::Sector(sector), ReasoningScope::Symbol(step_symbol))
                if step_symbol == symbol =>
            {
                Some(sector.to_string())
            }
            (ReasoningScope::Symbol(step_symbol), ReasoningScope::Sector(sector))
                if step_symbol == symbol =>
            {
                Some(sector.to_string())
            }
            _ => None,
        })
        .unwrap_or_else(|| "unknown".into())
}
