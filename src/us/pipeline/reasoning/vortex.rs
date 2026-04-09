use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope,
};
use crate::us::pipeline::signals::{
    UsDerivedSignalKind, UsDerivedSignalSnapshot, UsEventKind, UsEventSnapshot,
};

use super::{competing_confidence, convert_scope, scope_id, scope_label, summarize_evidence};

const CONVERGENCE_HYPOTHESIS_KEY: &str = "convergence_hypothesis";
const CONVERGENCE_HYPOTHESIS_LABEL: &str = "Convergence Hypothesis";
const LATENT_VORTEX_KEY: &str = "latent_vortex";
const LATENT_VORTEX_LABEL: &str = "Latent Vortex";

#[derive(Clone, Default)]
struct VortexChannelContribution {
    support_weight: Decimal,
    contradict_weight: Decimal,
    support_evidence: Option<ReasoningEvidence>,
    contradict_evidence: Option<ReasoningEvidence>,
}

#[derive(Debug, Clone)]
pub(super) struct VortexCandidate {
    pub vortex_id: String,
    pub scope: ReasoningScope,
    pub local_support_weight: Decimal,
    pub local_contradict_weight: Decimal,
    pub propagated_support_weight: Decimal,
    pub propagated_contradict_weight: Decimal,
    pub evidence: Vec<ReasoningEvidence>,
    pub path_ids: Vec<String>,
    pub dominant_channels: Vec<String>,
    pub channel_diversity: usize,
    pub strength: Decimal,
    pub coherence: Decimal,
}

impl VortexCandidate {
    fn channel_signature(&self) -> String {
        self.dominant_channels.join("|")
    }

    fn provenance_note(&self, family_label: &str) -> String {
        format!(
            "family={}; vortex_strength={}; channel_diversity={}; coherence={}; dominant_channels={}; channel_signature={}",
            family_label,
            self.strength.round_dp(4),
            self.channel_diversity,
            self.coherence.round_dp(4),
            self.channel_signature(),
            self.channel_signature(),
        )
    }
}

pub(super) fn derive_vortex_candidate(
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    relevant_paths: &[&PropagationPath],
) -> Option<VortexCandidate> {
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
                kind: ReasoningEvidenceKind::LocalEvent,
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
                kind: ReasoningEvidenceKind::LocalSignal,
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
                kind: ReasoningEvidenceKind::PropagatedPath,
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

    let summary = summarize_evidence(&evidence);
    Some(VortexCandidate {
        vortex_id: format!("vortex:{}", scope_id(scope)),
        scope: scope.clone(),
        local_support_weight: summary.local_support,
        local_contradict_weight: summary.local_contradict,
        propagated_support_weight: summary.propagated_support,
        propagated_contradict_weight: summary.propagated_contradict,
        evidence,
        path_ids,
        dominant_channels,
        channel_diversity,
        strength,
        coherence,
    })
}

pub(super) fn convergence_hypothesis_from_candidate(
    candidate: &VortexCandidate,
    timestamp: time::OffsetDateTime,
) -> Option<Hypothesis> {
    let min_strength = if candidate.channel_diversity >= 3 {
        Decimal::new(4, 1)
    } else if candidate.channel_diversity == 2 {
        Decimal::new(55, 2)
    } else {
        return None;
    };
    if candidate.strength <= min_strength {
        return None;
    }

    let confidence = (candidate.strength * Decimal::new(6, 1)
        + candidate.coherence * Decimal::new(2, 1)
        + competing_confidence(&candidate.evidence) * Decimal::new(2, 1))
    .clamp(Decimal::ZERO, Decimal::ONE)
    .round_dp(4);
    let hypothesis_id = format!(
        "hyp:{}:{}",
        scope_id(&candidate.scope),
        CONVERGENCE_HYPOTHESIS_KEY
    );

    Some(Hypothesis {
        hypothesis_id: hypothesis_id.clone(),
        family_key: CONVERGENCE_HYPOTHESIS_KEY.into(),
        family_label: CONVERGENCE_HYPOTHESIS_LABEL.into(),
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp)
            .with_trace_id(&hypothesis_id)
            .with_confidence(confidence)
            .with_inputs(
                candidate
                    .evidence
                    .iter()
                    .flat_map(|item| item.references.clone())
                    .chain(candidate.path_ids.iter().cloned())
                    .collect::<Vec<_>>(),
            )
            .with_note(candidate.provenance_note(CONVERGENCE_HYPOTHESIS_LABEL)),
        scope: candidate.scope.clone(),
        statement: format!(
            "{} shows an emergent convergence vortex across {}",
            scope_label(&candidate.scope),
            human_join(&candidate.dominant_channels),
        ),
        confidence,
        local_support_weight: candidate.local_support_weight,
        local_contradict_weight: candidate.local_contradict_weight,
        propagated_support_weight: candidate.propagated_support_weight,
        propagated_contradict_weight: candidate.propagated_contradict_weight,
        evidence: candidate.evidence.clone(),
        invalidation_conditions: vec![InvalidationCondition {
            description:
                "channel diversity falls below 3 or contradicting structure overtakes the vortex"
                    .into(),
            references: vec![],
        }],
        propagation_path_ids: candidate.path_ids.clone(),
        expected_observations: vec![
            "independent channels should keep reinforcing the same scope".into(),
            "diffusion paths should continue feeding the same center".into(),
            "vortex strength should stay above 0.40".into(),
        ],
    })
}

pub(super) fn latent_vortex_hypothesis_from_candidate(
    candidate: &VortexCandidate,
    timestamp: time::OffsetDateTime,
) -> Option<Hypothesis> {
    if candidate.channel_diversity < 2 || candidate.strength < Decimal::new(30, 2) {
        return None;
    }

    let confidence = (candidate.strength * Decimal::new(7, 1)
        + candidate.coherence * Decimal::new(3, 1))
    .clamp(Decimal::ZERO, Decimal::ONE)
    .round_dp(4);
    let hypothesis_id = format!("hyp:{}:{}", scope_id(&candidate.scope), LATENT_VORTEX_KEY);

    Some(Hypothesis {
        hypothesis_id: hypothesis_id.clone(),
        family_key: LATENT_VORTEX_KEY.into(),
        family_label: LATENT_VORTEX_LABEL.into(),
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp)
            .with_trace_id(format!("{}:{}", candidate.vortex_id, LATENT_VORTEX_KEY))
            .with_confidence(confidence)
            .with_inputs(
                candidate
                    .evidence
                    .iter()
                    .flat_map(|item| item.references.clone())
                    .chain(candidate.path_ids.iter().cloned())
                    .collect::<Vec<_>>(),
            )
            .with_note(candidate.provenance_note(LATENT_VORTEX_LABEL)),
        scope: candidate.scope.clone(),
        statement: format!(
            "{} is forming a topology-first vortex across {}, but it does not yet map cleanly to a named family",
            scope_label(&candidate.scope),
            human_join(&candidate.dominant_channels),
        ),
        confidence,
        local_support_weight: candidate.local_support_weight,
        local_contradict_weight: candidate.local_contradict_weight,
        propagated_support_weight: candidate.propagated_support_weight,
        propagated_contradict_weight: candidate.propagated_contradict_weight,
        evidence: candidate.evidence.clone(),
        invalidation_conditions: vec![InvalidationCondition {
            description:
                "named family evidence overtakes the vortex or vortex strength falls below 0.30"
                    .into(),
            references: vec![],
        }],
        propagation_path_ids: candidate.path_ids.clone(),
        expected_observations: vec![
            "channel diversity should expand or get sharper".into(),
            "the same center should keep collecting support across independent paths".into(),
            "a named family should emerge if the vortex matures".into(),
        ],
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
