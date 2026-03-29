use rust_decimal::Decimal;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningScope,
};
use crate::us::pipeline::signals::{UsDerivedSignalSnapshot, UsEventSnapshot};

use super::{
    competing_confidence, convert_scope, path_relevant_to_scope, scope_id, scope_label,
    summarize_evidence, TEMPLATES,
};
use super::support::{gather_evidence, template_applicable};

pub(super) fn derive_hypotheses(
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
) -> Vec<Hypothesis> {
    let mut scopes: Vec<ReasoningScope> = Vec::new();
    for event in &events.events {
        let scope = convert_scope(&event.value.scope);
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    for signal in &derived_signals.signals {
        let scope = convert_scope(&signal.value.scope);
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    for path in propagation_paths {
        for step in &path.steps {
            if !scopes.contains(&step.from) {
                scopes.push(step.from.clone());
            }
            if !scopes.contains(&step.to) {
                scopes.push(step.to.clone());
            }
        }
    }

    let mut hypotheses = Vec::new();

    for scope in &scopes {
        let relevant_paths = propagation_paths
            .iter()
            .filter(|path| path_relevant_to_scope(path, scope))
            .collect::<Vec<_>>();
        for template in TEMPLATES {
            if !template_applicable(template, scope, events, derived_signals, &relevant_paths) {
                continue;
            }

            let (evidence, path_ids) =
                gather_evidence(template, scope, events, derived_signals, &relevant_paths);
            let support_count = evidence
                .iter()
                .filter(|e| e.polarity == EvidencePolarity::Supports)
                .count();
            if support_count == 0 {
                continue;
            }

            let summary = summarize_evidence(&evidence);
            let confidence = competing_confidence(&evidence);

            hypotheses.push(Hypothesis {
                hypothesis_id: format!("hyp:{}:{}", scope_id(scope), template.key),
                family_key: template.key.to_string(),
                family_label: template.family_label.to_string(),
                provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, events.timestamp)
                    .with_trace_id(format!("hyp:{}:{}", scope_id(scope), template.key))
                    .with_inputs(
                        evidence
                            .iter()
                            .flat_map(|e| e.references.clone())
                            .collect::<Vec<_>>(),
                    ),
                scope: scope.clone(),
                statement: format!("{} {}", scope_label(scope), template.thesis),
                confidence,
                local_support_weight: summary.local_support,
                local_contradict_weight: summary.local_contradict,
                propagated_support_weight: summary.propagated_support,
                propagated_contradict_weight: summary.propagated_contradict,
                evidence: evidence.clone(),
                invalidation_conditions: vec![InvalidationCondition {
                    description: template.invalidation.to_string(),
                    references: vec![],
                }],
                propagation_path_ids: path_ids.clone(),
                expected_observations: template
                    .expected_observations
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            });

            let independent_counter: Vec<ReasoningEvidence> = evidence
                .iter()
                .filter(|e| e.polarity == EvidencePolarity::Contradicts)
                .map(|e| ReasoningEvidence {
                    polarity: EvidencePolarity::Supports,
                    ..e.clone()
                })
                .collect();
            if !independent_counter.is_empty() {
                let counter_summary = summarize_evidence(&independent_counter);
                let contradict_weight: Decimal = evidence
                    .iter()
                    .filter(|e| e.polarity == EvidencePolarity::Contradicts)
                    .map(|e| e.weight.abs())
                    .sum();
                let total_weight: Decimal = evidence.iter().map(|e| e.weight.abs()).sum();
                let counter_confidence = if total_weight > Decimal::ZERO {
                    (contradict_weight / total_weight).min(confidence)
                } else {
                    Decimal::ZERO
                };
                hypotheses.push(Hypothesis {
                    hypothesis_id: format!("hyp:{}:{}:counter", scope_id(scope), template.key),
                    family_key: format!("{}_reversal", template.key),
                    family_label: format!("{} Reversal", template.family_label),
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        events.timestamp,
                    )
                    .with_trace_id(format!(
                        "hyp:{}:{}:counter",
                        scope_id(scope),
                        template.key
                    )),
                    scope: scope.clone(),
                    statement: format!(
                        "{} counter-thesis: {} may reverse",
                        scope_label(scope),
                        template.family_label,
                    ),
                    confidence: counter_confidence,
                    local_support_weight: counter_summary.local_support,
                    local_contradict_weight: counter_summary.local_contradict,
                    propagated_support_weight: counter_summary.propagated_support,
                    propagated_contradict_weight: counter_summary.propagated_contradict,
                    evidence: independent_counter,
                    invalidation_conditions: vec![InvalidationCondition {
                        description: format!(
                            "{} thesis holds — no reversal",
                            template.family_label
                        ),
                        references: vec![],
                    }],
                    propagation_path_ids: path_ids.clone(),
                    expected_observations: vec![format!(
                        "{} signal should weaken",
                        template.family_label
                    )],
                });
            }
        }
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
    });
    hypotheses
}
