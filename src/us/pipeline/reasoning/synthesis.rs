use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningScope,
};
use crate::us::pipeline::signals::{
    UsDerivedSignalKind, UsDerivedSignalSnapshot, UsEventKind, UsEventSnapshot,
};

use super::support::{gather_evidence, template_applicable};
use super::{
    competing_confidence, convert_scope, path_relevant_to_scope, scope_id, scope_label,
    summarize_evidence, TEMPLATES,
};

const MAX_US_SYMBOL_HYPOTHESES_PER_SCOPE: usize = 3;
const CONVERGENCE_HYPOTHESIS_KEY: &str = "convergence_hypothesis";
const CONVERGENCE_HYPOTHESIS_LABEL: &str = "Convergence Hypothesis";

fn us_template_priority(family_key: &str) -> i32 {
    match family_key {
        super::TEMPLATE_MOMENTUM_CONTINUATION => 120,
        CONVERGENCE_HYPOTHESIS_KEY => 119,
        super::TEMPLATE_CROSS_MARKET_ARBITRAGE => 118,
        super::TEMPLATE_CATALYST_REPRICING => 116,
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

#[derive(Clone, Default)]
struct VortexChannelContribution {
    support_weight: Decimal,
    contradict_weight: Decimal,
    support_evidence: Option<ReasoningEvidence>,
    contradict_evidence: Option<ReasoningEvidence>,
}

struct VortexSignature {
    evidence: Vec<ReasoningEvidence>,
    path_ids: Vec<String>,
    dominant_channels: Vec<String>,
    channel_diversity: usize,
    strength: Decimal,
    coherence: Decimal,
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
        // Convergence hypothesis takes priority (same as HK).
        let convergence_hypothesis =
            derive_convergence_hypothesis(scope, events, derived_signals, &relevant_paths);
        let convergence_supersedes = convergence_hypothesis
            .as_ref()
            .map(|h| h.confidence >= Decimal::new(45, 2))
            .unwrap_or(false);
        if let Some(hypothesis) = convergence_hypothesis {
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
                if !gate.allows(template.family_label) {
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

            scope_hypotheses.push(Hypothesis {
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

fn derive_convergence_hypothesis(
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    relevant_paths: &[&PropagationPath],
) -> Option<Hypothesis> {
    let signature = derive_vortex_signature(scope, events, derived_signals, relevant_paths)?;
    if signature.channel_diversity < 3 || signature.strength <= Decimal::new(4, 1) {
        return None;
    }

    let summary = summarize_evidence(&signature.evidence);
    let confidence = (signature.strength * Decimal::new(6, 1)
        + signature.coherence * Decimal::new(2, 1)
        + competing_confidence(&signature.evidence) * Decimal::new(2, 1))
    .clamp(Decimal::ZERO, Decimal::ONE)
    .round_dp(4);
    let hypothesis_id = format!("hyp:{}:{}", scope_id(scope), CONVERGENCE_HYPOTHESIS_KEY);

    Some(Hypothesis {
        hypothesis_id: hypothesis_id.clone(),
        family_key: CONVERGENCE_HYPOTHESIS_KEY.into(),
        family_label: CONVERGENCE_HYPOTHESIS_LABEL.into(),
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, events.timestamp)
            .with_trace_id(&hypothesis_id)
            .with_confidence(confidence)
            .with_inputs(
                signature
                    .evidence
                    .iter()
                    .flat_map(|item| item.references.clone())
                    .chain(signature.path_ids.iter().cloned())
                    .collect::<Vec<_>>(),
            )
            .with_note(format!(
                "family={}; vortex_strength={}; channel_diversity={}; coherence={}; dominant_channels={}; channel_signature={}",
                CONVERGENCE_HYPOTHESIS_LABEL,
                signature.strength.round_dp(4),
                signature.channel_diversity,
                signature.coherence.round_dp(4),
                signature.dominant_channels.join("|"),
                signature.dominant_channels.join("|"),
            )),
        scope: scope.clone(),
        statement: format!(
            "{} shows an emergent convergence vortex across {}",
            scope_label(scope),
            human_join(&signature.dominant_channels),
        ),
        confidence,
        local_support_weight: summary.local_support,
        local_contradict_weight: summary.local_contradict,
        propagated_support_weight: summary.propagated_support,
        propagated_contradict_weight: summary.propagated_contradict,
        evidence: signature.evidence,
        invalidation_conditions: vec![InvalidationCondition {
            description:
                "channel diversity falls below 3 or contradicting structure overtakes the vortex"
                    .into(),
            references: vec![],
        }],
        propagation_path_ids: signature.path_ids,
        expected_observations: vec![
            "independent channels should keep reinforcing the same scope".into(),
            "diffusion paths should continue feeding the same center".into(),
            "vortex strength should stay above 0.40".into(),
        ],
    })
}

fn derive_vortex_signature(
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    relevant_paths: &[&PropagationPath],
) -> Option<VortexSignature> {
    let mut channels = HashMap::<String, VortexChannelContribution>::new();
    let mut path_ids = Vec::new();

    for event in events
        .events
        .iter()
        .filter(|event| scope == &convert_scope(&event.value.scope))
    {
        let Some((channel, polarity)) = vortex_event_channel(&event.value.kind) else {
            continue;
        };
        let weight = event.value.magnitude.min(Decimal::ONE);
        if weight <= Decimal::ZERO {
            continue;
        }
        register_vortex_evidence(
            &mut channels,
            channel,
            ReasoningEvidence {
                statement: event.value.summary.clone(),
                kind: crate::ontology::reasoning::ReasoningEvidenceKind::LocalEvent,
                polarity,
                weight,
                references: event.provenance.inputs.clone(),
                provenance: event.provenance.clone(),
            },
        );
    }

    for signal in derived_signals
        .signals
        .iter()
        .filter(|signal| scope == &convert_scope(&signal.value.scope))
    {
        let Some((channel, polarity)) = vortex_signal_channel(&signal.value.kind) else {
            continue;
        };
        let weight = signal.value.strength.abs().min(Decimal::ONE);
        if weight <= Decimal::ZERO {
            continue;
        }
        register_vortex_evidence(
            &mut channels,
            channel,
            ReasoningEvidence {
                statement: signal.value.summary.clone(),
                kind: crate::ontology::reasoning::ReasoningEvidenceKind::LocalSignal,
                polarity,
                weight,
                references: signal.provenance.inputs.clone(),
                provenance: signal.provenance.clone(),
            },
        );
    }

    for path in relevant_paths {
        let channel = vortex_path_channel(path);
        let weight = (path.confidence * vortex_hop_penalty(path.steps.len()))
            .round_dp(4)
            .min(Decimal::ONE);
        if weight <= Decimal::ZERO {
            continue;
        }
        let mut references = path
            .steps
            .iter()
            .flat_map(|step| step.references.clone())
            .collect::<Vec<_>>();
        references.push(path.path_id.clone());
        register_vortex_evidence(
            &mut channels,
            channel,
            ReasoningEvidence {
                statement: format!("{} via {}", path.summary, channel_display(channel)),
                kind: crate::ontology::reasoning::ReasoningEvidenceKind::PropagatedPath,
                polarity: EvidencePolarity::Supports,
                weight,
                references,
                provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, events.timestamp)
                    .with_trace_id(format!(
                        "us-vortex-path:{}:{}",
                        scope_id(scope),
                        path.path_id
                    ))
                    .with_inputs([path.path_id.clone()]),
            },
        );
        path_ids.push(path.path_id.clone());
    }

    if path_ids.is_empty() {
        return None;
    }

    let support_total = channels
        .values()
        .map(|channel| channel.support_weight)
        .sum::<Decimal>();
    let contradict_total = channels
        .values()
        .map(|channel| channel.contradict_weight)
        .sum::<Decimal>();
    let total = support_total + contradict_total;
    if support_total <= Decimal::ZERO || total <= Decimal::ZERO {
        return None;
    }

    let channel_diversity = channels
        .values()
        .filter(|channel| channel.support_weight > Decimal::ZERO)
        .count();
    let coherence = (support_total.max(contradict_total) / total).round_dp(4);
    let strength = ((support_total / Decimal::from(3))
        * ((coherence + Decimal::ONE) / Decimal::TWO))
        .clamp(Decimal::ZERO, Decimal::ONE)
        .round_dp(4);

    let mut dominant_channels = channels
        .iter()
        .filter(|(_, channel)| channel.support_weight > Decimal::ZERO)
        .map(|(name, channel)| (name.clone(), channel.support_weight))
        .collect::<Vec<_>>();
    dominant_channels.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let dominant_channels = dominant_channels
        .into_iter()
        .take(3)
        .map(|(name, _)| channel_display(&name).to_string())
        .collect::<Vec<_>>();

    let mut evidence = channels
        .into_values()
        .flat_map(|channel| {
            channel
                .support_evidence
                .into_iter()
                .chain(channel.contradict_evidence)
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|a, b| {
        b.weight
            .cmp(&a.weight)
            .then_with(|| a.statement.cmp(&b.statement))
    });

    path_ids.sort();
    path_ids.dedup();

    Some(VortexSignature {
        evidence,
        path_ids,
        dominant_channels,
        channel_diversity,
        strength,
        coherence,
    })
}

fn register_vortex_evidence(
    channels: &mut HashMap<String, VortexChannelContribution>,
    channel: &'static str,
    evidence: ReasoningEvidence,
) {
    let entry = channels.entry(channel.into()).or_default();
    match evidence.polarity {
        EvidencePolarity::Supports => {
            if evidence.weight > entry.support_weight {
                entry.support_weight = evidence.weight;
                entry.support_evidence = Some(evidence);
            }
        }
        EvidencePolarity::Contradicts => {
            if evidence.weight > entry.contradict_weight {
                entry.contradict_weight = evidence.weight;
                entry.contradict_evidence = Some(evidence);
            }
        }
    }
}

fn vortex_event_channel(kind: &UsEventKind) -> Option<(&'static str, EvidencePolarity)> {
    match kind {
        UsEventKind::PreMarketDislocation | UsEventKind::GapOpen => {
            Some(("pre_market", EvidencePolarity::Supports))
        }
        UsEventKind::CapitalFlowReversal => Some(("flow", EvidencePolarity::Contradicts)),
        UsEventKind::VolumeSpike => Some(("volume", EvidencePolarity::Supports)),
        UsEventKind::CrossMarketDivergence => Some(("cross_market", EvidencePolarity::Supports)),
        UsEventKind::SectorMomentumShift => Some(("sector_rotation", EvidencePolarity::Supports)),
        UsEventKind::CatalystActivation => Some(("catalyst", EvidencePolarity::Supports)),
        UsEventKind::PropagationAbsence => Some(("propagation", EvidencePolarity::Contradicts)),
    }
}

fn vortex_signal_channel(kind: &UsDerivedSignalKind) -> Option<(&'static str, EvidencePolarity)> {
    match kind {
        UsDerivedSignalKind::StructuralComposite => Some(("structure", EvidencePolarity::Supports)),
        UsDerivedSignalKind::PreMarketConviction => {
            Some(("pre_market", EvidencePolarity::Supports))
        }
        UsDerivedSignalKind::CrossMarketPropagation => {
            Some(("cross_market", EvidencePolarity::Supports))
        }
        UsDerivedSignalKind::ValuationExtreme => Some(("valuation", EvidencePolarity::Supports)),
    }
}

fn vortex_path_channel(path: &PropagationPath) -> &'static str {
    let mechanisms = path
        .steps
        .iter()
        .map(|step| step.mechanism.as_str())
        .collect::<Vec<_>>();
    let has_cross_market = mechanisms
        .iter()
        .any(|mechanism| mechanism.contains("cross-market"));
    let has_sector = mechanisms
        .iter()
        .any(|mechanism| mechanism.contains("sector"));
    let has_stock = mechanisms
        .iter()
        .any(|mechanism| mechanism.contains("stock"));

    match (has_cross_market, has_sector, has_stock) {
        (true, true, _) | (true, _, true) | (_, true, true) => "cross_mechanism",
        (true, false, false) => "cross_market",
        (false, true, false) => "sector_rotation",
        (false, false, true) => "peer_diffusion",
        _ => "propagation",
    }
}

fn vortex_hop_penalty(hops: usize) -> Decimal {
    match hops {
        0 | 1 => Decimal::ONE,
        2 => Decimal::new(85, 2),
        _ => Decimal::new(70, 2),
    }
}

fn channel_display(channel: &str) -> &str {
    match channel {
        "pre_market" => "pre-market",
        "flow" => "flow",
        "volume" => "volume",
        "cross_market" => "cross-market",
        "sector_rotation" => "sector rotation",
        "catalyst" => "catalyst",
        "propagation" => "propagation",
        "structure" => "structure",
        "valuation" => "valuation",
        "peer_diffusion" => "peer diffusion",
        "cross_mechanism" => "cross-mechanism relay",
        _ => channel,
    }
}

fn human_join(items: &[String]) -> String {
    match items {
        [] => "multiple channels".into(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [head @ .., last] => format!("{}, and {}", head.join(", "), last),
    }
}
