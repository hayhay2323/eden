use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::graph::decision::{DecisionSnapshot, OrderDirection};
use crate::ontology::reasoning::{
    DecisionLineage, EvidencePolarity, Hypothesis, InvestigationSelection,
    PropagationPath, ReasoningEvidence, ReasoningEvidenceKind, ReasoningScope,
    TacticalSetup,
};
use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot};

use super::propagation::hop_penalty;
use super::support::{
    competing_hypothesis_confidence, convert_scope, derived_provenance,
    event_polarity, hypothesis_provenance, hypothesis_templates, path_polarity,
    path_relevant_to_scope, scope_id, scope_matches_event,
    scope_matches_signal_or_market, scope_title, setup_provenance,
    signal_polarity, stable_setup_id, summarize_evidence_weights,
    template_expected_observations, template_invalidation, template_statement,
};

pub(super) fn derive_hypotheses(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
) -> Vec<Hypothesis> {
    let mut scopes = events
        .events
        .iter()
        .map(|event| convert_scope(&event.value.scope))
        .collect::<Vec<_>>();
    for path in propagation_paths {
        for step in &path.steps {
            scopes.push(step.from.clone());
            scopes.push(step.to.clone());
        }
    }
    scopes.sort_by_key(scope_id);
    scopes.dedup();

    let mut hypotheses = Vec::new();

    for scope in scopes {
        let relevant_events = events
            .events
            .iter()
            .filter(|event| scope_matches_event(&scope, &event.value.scope))
            .collect::<Vec<_>>();
        let relevant_signals = derived_signals
            .signals
            .iter()
            .filter(|signal| scope_matches_signal_or_market(&scope, &signal.value.scope))
            .collect::<Vec<_>>();
        let relevant_paths = propagation_paths
            .iter()
            .filter(|path| path_relevant_to_scope(path, &scope))
            .collect::<Vec<_>>();
        let templates =
            hypothesis_templates(&relevant_events, &relevant_signals, &relevant_paths);
        for template in &templates {
            let mut evidence = Vec::new();

            for event in &relevant_events {
                if let Some(polarity) = event_polarity(template, &event.value.kind) {
                    evidence.push(ReasoningEvidence {
                        statement: event.value.summary.clone(),
                        kind: ReasoningEvidenceKind::LocalEvent,
                        polarity,
                        weight: event.value.magnitude.min(Decimal::ONE),
                        references: event.provenance.inputs.clone(),
                        provenance: event.provenance.clone(),
                    });
                }
            }

            for signal in &relevant_signals {
                if let Some(polarity) = signal_polarity(template, &signal.value.kind) {
                    evidence.push(ReasoningEvidence {
                        statement: signal.value.summary.clone(),
                        kind: ReasoningEvidenceKind::LocalSignal,
                        polarity,
                        weight: signal.value.strength.abs().min(Decimal::ONE),
                        references: signal.provenance.inputs.clone(),
                        provenance: signal.provenance.clone(),
                    });
                }
            }

            let (path_weight, path_ids) =
                propagated_path_evidence(&scope, &evidence, propagation_paths);
            if path_weight > Decimal::ZERO {
                let polarity = path_polarity(template);
                evidence.push(ReasoningEvidence {
                    statement: if polarity == EvidencePolarity::Supports {
                        format!("propagation paths align with {}", template.thesis)
                    } else {
                        format!("propagation paths do not support {}", template.thesis)
                    },
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity,
                    weight: path_weight,
                    references: path_ids.clone(),
                    provenance: derived_provenance(events.timestamp, path_weight, &path_ids),
                });
            }

            let evidence_summary = summarize_evidence_weights(&evidence);
            let support_count = evidence
                .iter()
                .filter(|item| item.polarity == EvidencePolarity::Supports)
                .count();
            if support_count == 0 {
                continue;
            }

            hypotheses.push(Hypothesis {
                hypothesis_id: format!("hyp:{}:{}", scope_id(&scope), template.key),
                family_key: template.key.clone(),
                family_label: template.family_label.clone(),
                provenance: hypothesis_provenance(
                    events.timestamp,
                    &format!("hyp:{}:{}", scope_id(&scope), template.key),
                    &template.family_label,
                    &evidence,
                    &path_ids,
                ),
                scope: scope.clone(),
                statement: template_statement(template, &scope),
                confidence: competing_hypothesis_confidence(&evidence),
                local_support_weight: evidence_summary.local_support,
                local_contradict_weight: evidence_summary.local_contradict,
                propagated_support_weight: evidence_summary.propagated_support,
                propagated_contradict_weight: evidence_summary.propagated_contradict,
                evidence,
                invalidation_conditions: template_invalidation(template),
                propagation_path_ids: path_ids.clone(),
                expected_observations: template_expected_observations(template),
            });
        }
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
    });
    hypotheses
}

pub(super) fn derive_investigation_selections(
    decision: &DecisionSnapshot,
    hypotheses: &[Hypothesis],
) -> Vec<InvestigationSelection> {
    let suggestion_map = decision
        .order_suggestions
        .iter()
        .map(|suggestion| {
            (
                scope_id(&ReasoningScope::Symbol(suggestion.symbol.clone())),
                suggestion,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut hypotheses_by_scope: HashMap<ReasoningScope, Vec<&Hypothesis>> = HashMap::new();
    for hypothesis in hypotheses {
        hypotheses_by_scope
            .entry(hypothesis.scope.clone())
            .or_default()
            .push(hypothesis);
    }

    let mut selections = Vec::new();
    for (scope, mut ranked) in hypotheses_by_scope {
        ranked.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
        });
        let top = ranked[0];
        let runner_up = ranked.get(1).copied();
        let gap = runner_up
            .map(|item| top.confidence - item.confidence)
            .unwrap_or(Decimal::ONE);
        let scope_key = scope_id(&scope);
        let suggestion = suggestion_map.get(scope_key.as_str()).copied();
        let propagated_signal = !top.propagation_path_ids.is_empty()
            || top.propagated_support_weight > Decimal::ZERO
            || top.propagated_contradict_weight > Decimal::ZERO;
        let attention_hint = suggestion
            .map(|item| {
                if item.requires_confirmation || gap < Decimal::new(1, 1) {
                    "review"
                } else if item.heuristic_edge > Decimal::ZERO {
                    "enter"
                } else {
                    "observe"
                }
            })
            .unwrap_or_else(|| {
                if top.confidence >= Decimal::new(7, 1) && gap >= Decimal::new(15, 2) {
                    "review"
                } else {
                    "observe"
                }
            });
        let mut priority_score = suggestion
            .map(|item| item.heuristic_edge.max(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO)
            + gap.max(Decimal::ZERO)
            + top.propagated_support_weight
            + (top.local_support_weight * Decimal::new(5, 1));
        if attention_hint == "enter" {
            priority_score += Decimal::new(20, 2);
        } else if attention_hint == "review" {
            priority_score += Decimal::new(10, 2);
        }
        if propagated_signal {
            priority_score += Decimal::new(15, 2);
        }
        let title = suggestion
            .map(|item| {
                format!(
                    "{} {}",
                    match item.direction {
                        OrderDirection::Buy => "Long",
                        OrderDirection::Sell => "Short",
                    },
                    item.symbol
                )
            })
            .unwrap_or_else(|| format!("{} investigation", scope_title(&scope)));
        let mut notes = vec![
            format!("family={}", top.family_label),
            format!("local_support={}", top.local_support_weight.round_dp(4)),
            format!(
                "propagated_support={}",
                top.propagated_support_weight.round_dp(4)
            ),
            format!(
                "propagated_contradict={}",
                top.propagated_contradict_weight.round_dp(4)
            ),
        ];
        if let Some(item) = suggestion {
            notes.push(format!(
                "heuristic_edge={}",
                item.heuristic_edge.round_dp(4)
            ));
            notes.push(format!(
                "convergence_score={}",
                item.convergence_score.round_dp(4)
            ));
            notes.push(format!(
                "effective_confidence={}",
                item.effective_confidence.round_dp(4)
            ));
        }
        if propagated_signal {
            notes.push("investigation_channel=propagated".into());
        }

        selections.push(InvestigationSelection {
            investigation_id: format!("investigation:{}", scope_key),
            hypothesis_id: top.hypothesis_id.clone(),
            runner_up_hypothesis_id: runner_up.map(|item| item.hypothesis_id.clone()),
            provenance: top
                .provenance
                .clone()
                .with_trace_id(format!("investigation:{}", scope_key))
                .with_note("investigation selection"),
            scope,
            title,
            family_key: top.family_key.clone(),
            family_label: top.family_label.clone(),
            confidence: top.confidence,
            confidence_gap: gap,
            priority_score: priority_score.round_dp(4),
            attention_hint: attention_hint.into(),
            rationale: top.statement.clone(),
            notes,
        });
    }

    selections.sort_by(|a, b| {
        b.priority_score
            .cmp(&a.priority_score)
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.investigation_id.cmp(&b.investigation_id))
    });
    selections
}

pub(super) fn derive_tactical_setups(
    decision: &DecisionSnapshot,
    hypotheses: &[Hypothesis],
    investigation_selections: &[InvestigationSelection],
) -> Vec<TacticalSetup> {
    let selection_map = investigation_selections
        .iter()
        .map(|selection| (scope_id(&selection.scope), selection))
        .collect::<HashMap<_, _>>();
    let mut setups = decision
        .order_suggestions
        .iter()
        .map(|suggestion| {
            let scope = ReasoningScope::Symbol(suggestion.symbol.clone());
            let selection = selection_map.get(scope_id(&scope).as_str()).copied();
            let linked_hypothesis = selection.and_then(|selection| {
                hypotheses
                    .iter()
                    .find(|hypothesis| hypothesis.hypothesis_id == selection.hypothesis_id)
                    .map(|hypothesis| {
                        (
                            hypothesis.hypothesis_id.clone(),
                            hypothesis.statement.clone(),
                            selection.confidence,
                            hypothesis.local_support_weight,
                            hypothesis.family_label.clone(),
                        )
                    })
            });
            let runner_up_hypothesis =
                selection.and_then(|selection| selection.runner_up_hypothesis_id.clone());
            let hypothesis_margin = selection
                .map(|selection| selection.confidence_gap)
                .unwrap_or(Decimal::ONE);

            let action =
                if suggestion.requires_confirmation || hypothesis_margin < Decimal::new(1, 1) {
                    "review"
                } else if suggestion.heuristic_edge > Decimal::ZERO {
                    "enter"
                } else {
                    "observe"
                };
            let title = format!(
                "{} {}",
                match suggestion.direction {
                    OrderDirection::Buy => "Long",
                    OrderDirection::Sell => "Short",
                },
                suggestion.symbol
            );

            TacticalSetup {
                setup_id: stable_setup_id(&scope),
                hypothesis_id: linked_hypothesis
                    .as_ref()
                    .map(|(id, _, _, _, _)| id.clone())
                    .unwrap_or_else(|| format!("hyp:{}:convergence", suggestion.symbol)),
                runner_up_hypothesis_id: runner_up_hypothesis.clone(),
                provenance: setup_provenance(
                    decision.timestamp,
                    &stable_setup_id(&scope),
                    linked_hypothesis
                        .as_ref()
                        .map(|(id, _, _, _, _)| id.as_str()),
                    runner_up_hypothesis.as_deref(),
                    [
                        format!("order_suggestion:{}", suggestion.symbol),
                        format!("heuristic_edge:{}", suggestion.heuristic_edge.round_dp(4)),
                        format!("estimated_cost:{}", suggestion.estimated_cost.round_dp(4)),
                    ],
                ),
                lineage: DecisionLineage::default(),
                scope,
                title,
                action: action.into(),
                time_horizon: "intraday".into(),
                confidence: linked_hypothesis
                    .as_ref()
                    .map(|(_, _, confidence, _, _)| *confidence)
                    .unwrap_or(suggestion.effective_confidence),
                confidence_gap: hypothesis_margin,
                heuristic_edge: suggestion.heuristic_edge,
                convergence_score: Some(suggestion.convergence_score.round_dp(4)),
                workflow_id: Some(format!(
                    "order:{}:{}",
                    suggestion.symbol,
                    match suggestion.direction {
                        OrderDirection::Buy => "buy",
                        OrderDirection::Sell => "sell",
                    }
                )),
                entry_rationale: linked_hypothesis
                    .as_ref()
                    .map(|(_, statement, _, _, _)| statement.clone())
                    .unwrap_or_else(|| "structural convergence without explicit hypothesis".into()),
                risk_notes: vec![
                    linked_hypothesis
                        .as_ref()
                        .map(|(_, _, _, _, family_label)| format!("family={}", family_label))
                        .unwrap_or_else(|| "family=convergence".into()),
                    format!(
                        "estimated execution cost={}",
                        suggestion.estimated_cost.round_dp(4)
                    ),
                    format!(
                        "convergence_score={}",
                        suggestion.convergence_score.round_dp(4)
                    ),
                    format!(
                        "effective_confidence={}",
                        suggestion.effective_confidence.round_dp(4)
                    ),
                    format!("hypothesis_margin={}", hypothesis_margin.round_dp(4)),
                    format!(
                        "local_support={}",
                        linked_hypothesis
                            .as_ref()
                            .map(|(_, _, _, local_support, _)| local_support.round_dp(4))
                            .unwrap_or(Decimal::ZERO)
                    ),
                    suggestion
                        .external_confirmation
                        .as_ref()
                        .map(|value| format!("external_confirmation={}", value))
                        .unwrap_or_else(|| "external_confirmation=".into()),
                    suggestion
                        .external_support_slug
                        .as_ref()
                        .map(|value| format!("external_support_slug={}", value))
                        .unwrap_or_else(|| "external_support_slug=".into()),
                    suggestion
                        .external_support_probability
                        .map(|value| format!("external_support_probability={}", value.round_dp(4)))
                        .unwrap_or_else(|| "external_support_probability=".into()),
                    suggestion
                        .external_conflict
                        .as_ref()
                        .map(|value| format!("external_conflict={}", value))
                        .unwrap_or_else(|| "external_conflict=".into()),
                    suggestion
                        .external_conflict_slug
                        .as_ref()
                        .map(|value| format!("external_conflict_slug={}", value))
                        .unwrap_or_else(|| "external_conflict_slug=".into()),
                    suggestion
                        .external_conflict_probability
                        .map(|value| format!("external_conflict_probability={}", value.round_dp(4)))
                        .unwrap_or_else(|| "external_conflict_probability=".into()),
                ],
                policy_verdict: None,
            }
        })
        .collect::<Vec<_>>();

    let symbol_scope_setups: HashMap<ReasoningScope, String> = setups
        .iter()
        .map(|setup| (setup.scope.clone(), setup.setup_id.clone()))
        .collect();

    for selection in investigation_selections {
        if symbol_scope_setups.contains_key(&selection.scope) {
            continue;
        }

        let Some(top) = hypotheses
            .iter()
            .find(|hypothesis| hypothesis.hypothesis_id == selection.hypothesis_id)
        else {
            continue;
        };

        let action = if selection.confidence >= Decimal::new(7, 1)
            && selection.confidence_gap >= Decimal::new(15, 2)
        {
            "review"
        } else {
            "observe"
        };

        setups.push(TacticalSetup {
            setup_id: stable_setup_id(&selection.scope),
            hypothesis_id: top.hypothesis_id.clone(),
            runner_up_hypothesis_id: selection.runner_up_hypothesis_id.clone(),
            provenance: setup_provenance(
                decision.timestamp,
                &stable_setup_id(&selection.scope),
                Some(top.hypothesis_id.as_str()),
                selection.runner_up_hypothesis_id.as_deref(),
                [format!("scope_case:{}", scope_id(&selection.scope))],
            ),
            lineage: DecisionLineage::default(),
            scope: selection.scope.clone(),
            title: selection.title.clone(),
            action: action.into(),
            time_horizon: "intraday".into(),
            confidence: selection.confidence,
            confidence_gap: selection.confidence_gap,
            heuristic_edge: selection.priority_score.clamp(Decimal::ZERO, Decimal::ONE),
            convergence_score: None,
            workflow_id: None,
            entry_rationale: selection.rationale.clone(),
            risk_notes: vec![
                format!("family={}", selection.family_label),
                "scope-level case; requires operator judgement".into(),
                format!("local_support={}", top.local_support_weight.round_dp(4)),
            ],
            policy_verdict: None,
        });
    }

    setups
}

pub(crate) fn propagated_path_evidence(
    scope: &ReasoningScope,
    local_evidence: &[ReasoningEvidence],
    propagation_paths: &[PropagationPath],
) -> (Decimal, Vec<String>) {
    let local_support = local_evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Supports)
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE);
    let local_contradict = local_evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Contradicts)
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE);

    let local_bonus = if local_support > Decimal::ZERO {
        Decimal::ONE + local_support * Decimal::new(25, 2)
    } else {
        Decimal::new(35, 2)
    };
    let contradiction_penalty = Decimal::ONE - local_contradict * Decimal::new(40, 2);

    let relevant = propagation_paths
        .iter()
        .filter(|path| path_relevant_to_scope(path, scope))
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return (Decimal::ZERO, Vec::new());
    }

    let mut scored = relevant
        .into_iter()
        .map(|path| {
            let hop_penalty = hop_penalty(path.steps.len());
            let effective = (path.confidence * hop_penalty * local_bonus * contradiction_penalty)
                .round_dp(4)
                .clamp(Decimal::ZERO, Decimal::ONE);
            (effective, path.path_id.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let best_weight = scored.first().map(|item| item.0).unwrap_or(Decimal::ZERO);
    let path_ids = scored
        .into_iter()
        .take(3)
        .map(|item| item.1)
        .collect::<Vec<_>>();
    (best_weight, path_ids)
}
