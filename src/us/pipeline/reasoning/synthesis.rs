use rust_decimal::Decimal;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningScope,
};
use crate::us::pipeline::signals::{UsDerivedSignalSnapshot, UsEventSnapshot};

use super::support::{gather_evidence, template_applicable};
use super::vortex::{
    convergence_hypothesis_from_candidate, derive_vortex_candidate,
    latent_vortex_hypothesis_from_candidate,
};
use super::{
    competing_confidence, convert_scope, path_relevant_to_scope, scope_id, scope_label,
    summarize_evidence, TEMPLATES,
};

const MAX_US_SYMBOL_HYPOTHESES_PER_SCOPE: usize = 3;

/// Refine catalyst_repricing family_key by inspecting the driver attribution
/// embedded in CatalystActivation event provenance. This splits one noisy family
/// into three sub-families with independent lineage tracking.
fn refine_catalyst_family(template_key: &str, evidence: &[ReasoningEvidence]) -> (String, String) {
    if template_key != super::TEMPLATE_CATALYST_REPRICING {
        return (template_key.to_string(), String::new());
    }
    // Look for attr:driver= in evidence references
    let driver = evidence
        .iter()
        .flat_map(|e| e.references.iter())
        .find_map(|r| r.strip_prefix("attr:driver="))
        .unwrap_or("unknown");

    match driver {
        "company_specific" => (
            "catalyst_repricing_company".into(),
            "Catalyst Repricing (Company)".into(),
        ),
        "sector_wide" => (
            "catalyst_repricing_sector".into(),
            "Catalyst Repricing (Sector)".into(),
        ),
        "macro_wide" => (
            "catalyst_repricing_macro".into(),
            "Catalyst Repricing (Macro)".into(),
        ),
        _ => ("catalyst_repricing".into(), "Catalyst Repricing".into()),
    }
}

fn us_template_priority(family_key: &str) -> i32 {
    match family_key {
        super::TEMPLATE_MOMENTUM_CONTINUATION => 120,
        "convergence_hypothesis" => 119,
        super::TEMPLATE_CROSS_MARKET_ARBITRAGE => 118,
        super::TEMPLATE_CATALYST_REPRICING
        | "catalyst_repricing_company"
        | "catalyst_repricing_sector"
        | "catalyst_repricing_macro" => 116,
        "latent_vortex" => 112,
        super::TEMPLATE_PRE_MARKET_POSITIONING => 114,
        super::TEMPLATE_SECTOR_ROTATION => 108,
        super::TEMPLATE_CROSS_MARKET_DIFFUSION => 104,
        super::TEMPLATE_SECTOR_DIFFUSION => 100,
        super::TEMPLATE_PEER_RELAY => 98,
        super::TEMPLATE_STRUCTURAL_DIFFUSION => 96,
        super::TEMPLATE_CROSS_MECHANISM_CHAIN => 92,
        _ if family_key.ends_with("_reversal") => 40,
        _ => 80,
    }
}

fn us_hypothesis_sort_key(hypothesis: &Hypothesis) -> (i32, Decimal, Decimal, Decimal, String) {
    (
        us_template_priority(&hypothesis.family_key),
        hypothesis.confidence,
        hypothesis.local_support_weight + hypothesis.propagated_support_weight,
        Decimal::ZERO
            - (hypothesis.local_contradict_weight + hypothesis.propagated_contradict_weight),
        hypothesis.hypothesis_id.clone(),
    )
}

pub(super) fn derive_hypotheses(
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
    family_gate: Option<&crate::pipeline::reasoning::family_gate::FamilyAlphaGate>,
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
        let mut scope_hypotheses = Vec::new();
        let relevant_paths = propagation_paths
            .iter()
            .filter(|path| path_relevant_to_scope(path, scope))
            .collect::<Vec<_>>();
        let vortex_candidate =
            derive_vortex_candidate(scope, events, derived_signals, &relevant_paths);
        // Vortex candidates now come first. Templates still exist, but they refine or
        // name an already detected topology instead of being the only generation path.
        let convergence_hypothesis = vortex_candidate.as_ref().and_then(|candidate| {
            convergence_hypothesis_from_candidate(candidate, events.timestamp)
        });
        let convergence_supersedes = convergence_hypothesis
            .as_ref()
            .map(|h| h.confidence >= Decimal::new(45, 2))
            .unwrap_or(false);
        if let Some(hypothesis) = convergence_hypothesis {
            scope_hypotheses.push(hypothesis);
        } else if let Some(hypothesis) = vortex_candidate.as_ref().and_then(|candidate| {
            latent_vortex_hypothesis_from_candidate(candidate, events.timestamp)
        }) {
            scope_hypotheses.push(hypothesis);
        }
        if convergence_supersedes {
            scope_hypotheses.truncate(MAX_US_SYMBOL_HYPOTHESES_PER_SCOPE);
            hypotheses.extend(scope_hypotheses);
            continue;
        }
        for template in TEMPLATES {
            // FamilyAlphaGate: skip templates from blocked families
            if let Some(gate) = family_gate {
                if !gate.allows(template.key) {
                    continue;
                }
            }
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

            let (refined_family_key, refined_family_label) =
                refine_catalyst_family(template.key, &evidence);
            let family_key = if refined_family_label.is_empty() {
                template.key.to_string()
            } else {
                refined_family_key
            };
            let family_label = if refined_family_label.is_empty() {
                template.family_label.to_string()
            } else {
                refined_family_label
            };

            scope_hypotheses.push(Hypothesis {
                hypothesis_id: format!("hyp:{}:{}", scope_id(scope), family_key),
                family_key,
                family_label,
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
                scope_hypotheses.push(Hypothesis {
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

        scope_hypotheses.sort_by(|left, right| {
            us_hypothesis_sort_key(right).cmp(&us_hypothesis_sort_key(left))
        });
        if matches!(scope, ReasoningScope::Symbol(_)) {
            scope_hypotheses.truncate(MAX_US_SYMBOL_HYPOTHESES_PER_SCOPE);
        }
        hypotheses.extend(scope_hypotheses);
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
    });
    hypotheses
}
