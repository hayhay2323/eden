use rust_decimal::Decimal;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, PropagationPath, ReasoningEvidence, ReasoningEvidenceKind, ReasoningScope,
};
use crate::us::pipeline::signals::{
    UsDerivedSignalKind, UsDerivedSignalSnapshot, UsEventKind, UsEventSnapshot,
};

use super::{
    convert_scope, scope_id, scope_label, scope_matches, HypothesisTemplate,
    TEMPLATE_CROSS_MARKET_ARBITRAGE, TEMPLATE_MOMENTUM_CONTINUATION,
    TEMPLATE_PRE_MARKET_POSITIONING, TEMPLATE_SECTOR_ROTATION,
    TEMPLATE_STRUCTURAL_DIFFUSION,
};

pub(crate) fn template_applicable(
    template: &HypothesisTemplate,
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    relevant_paths: &[&PropagationPath],
) -> bool {
    match template.key {
        TEMPLATE_PRE_MARKET_POSITIONING => has_event_for_scope(
            events,
            scope,
            &[UsEventKind::PreMarketDislocation, UsEventKind::GapOpen],
        ),
        TEMPLATE_CROSS_MARKET_ARBITRAGE => {
            has_event_for_scope(events, scope, &[UsEventKind::CrossMarketDivergence])
                || has_signal_for_scope(
                    derived_signals,
                    scope,
                    &[UsDerivedSignalKind::CrossMarketPropagation],
                )
                || !relevant_paths_for_template(template.key, relevant_paths).is_empty()
        }
        TEMPLATE_MOMENTUM_CONTINUATION => {
            let has_event = has_event_for_scope(
                events,
                scope,
                &[UsEventKind::CapitalFlowReversal, UsEventKind::VolumeSpike],
            );
            let has_signal = has_signal_for_scope(
                derived_signals,
                scope,
                &[UsDerivedSignalKind::StructuralComposite],
            );
            (has_event && has_signal)
                || !relevant_paths_for_template(template.key, relevant_paths).is_empty()
        }
        TEMPLATE_SECTOR_ROTATION => {
            has_event_for_scope(events, scope, &[UsEventKind::SectorMomentumShift])
                || matches!(scope, ReasoningScope::Sector(_))
                || !relevant_paths_for_template(template.key, relevant_paths).is_empty()
        }
        TEMPLATE_STRUCTURAL_DIFFUSION => {
            !relevant_paths_for_template(template.key, relevant_paths).is_empty()
        }
        _ => false,
    }
}

fn has_event_for_scope(
    events: &UsEventSnapshot,
    scope: &ReasoningScope,
    kinds: &[UsEventKind],
) -> bool {
    events.events.iter().any(|e| {
        scope_matches(&convert_scope(&e.value.scope), scope) && event_kind_in(&e.value.kind, kinds)
    })
}

fn has_signal_for_scope(
    signals: &UsDerivedSignalSnapshot,
    scope: &ReasoningScope,
    kinds: &[UsDerivedSignalKind],
) -> bool {
    signals.signals.iter().any(|s| {
        scope_matches(&convert_scope(&s.value.scope), scope) && signal_kind_in(&s.value.kind, kinds)
    })
}

fn event_kind_in(kind: &UsEventKind, kinds: &[UsEventKind]) -> bool {
    kinds
        .iter()
        .any(|k| std::mem::discriminant(k) == std::mem::discriminant(kind))
}

fn signal_kind_in(kind: &UsDerivedSignalKind, kinds: &[UsDerivedSignalKind]) -> bool {
    kinds
        .iter()
        .any(|k| std::mem::discriminant(k) == std::mem::discriminant(kind))
}

pub(super) fn gather_evidence(
    template: &HypothesisTemplate,
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
    relevant_paths: &[&PropagationPath],
) -> (Vec<ReasoningEvidence>, Vec<String>) {
    let mut evidence = Vec::new();

    for event in &events.events {
        if !scope_matches(&convert_scope(&event.value.scope), scope) {
            continue;
        }
        if let Some(polarity) = event_polarity(template.key, &event.value.kind) {
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

    for signal in &derived_signals.signals {
        if !scope_matches(&convert_scope(&signal.value.scope), scope) {
            continue;
        }
        if let Some(polarity) = signal_polarity(template.key, &signal.value.kind) {
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

    let path_refs = relevant_paths_for_template(template.key, relevant_paths);
    let (path_weight, path_ids) = propagated_path_evidence(&evidence, &path_refs);
    if path_weight > Decimal::ZERO {
        let polarity = path_polarity(template.key);
        evidence.push(ReasoningEvidence {
            statement: diffusion_path_statement(template.key, scope),
            kind: ReasoningEvidenceKind::PropagatedPath,
            polarity,
            weight: path_weight,
            references: path_ids.clone(),
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, events.timestamp)
                .with_trace_id(format!("us-path:{}:{}", scope_id(scope), template.key))
                .with_inputs(path_ids.clone()),
        });
    }

    (evidence, path_ids)
}

pub(crate) fn event_polarity(template_key: &str, kind: &UsEventKind) -> Option<EvidencePolarity> {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => match kind {
            UsEventKind::PreMarketDislocation | UsEventKind::GapOpen => {
                Some(EvidencePolarity::Supports)
            }
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_CROSS_MARKET_ARBITRAGE => match kind {
            UsEventKind::CrossMarketDivergence => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_MOMENTUM_CONTINUATION => match kind {
            UsEventKind::VolumeSpike => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            UsEventKind::PreMarketDislocation | UsEventKind::GapOpen => {
                Some(EvidencePolarity::Contradicts)
            }
            _ => None,
        },
        TEMPLATE_SECTOR_ROTATION => match kind {
            UsEventKind::SectorMomentumShift => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn signal_polarity(template_key: &str, kind: &UsDerivedSignalKind) -> Option<EvidencePolarity> {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => match kind {
            UsDerivedSignalKind::PreMarketConviction => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_CROSS_MARKET_ARBITRAGE => match kind {
            UsDerivedSignalKind::CrossMarketPropagation => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_MOMENTUM_CONTINUATION => match kind {
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            UsDerivedSignalKind::PreMarketConviction => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_SECTOR_ROTATION => match kind {
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_STRUCTURAL_DIFFUSION => match kind {
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::CrossMarketPropagation => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        _ => None,
    }
}

fn path_family(mechanism: &str) -> &'static str {
    if mechanism.contains("cross-market diffusion") {
        "cross_market_diffusion"
    } else if mechanism.contains("sector diffusion") {
        "sector_diffusion"
    } else if mechanism.contains("stock diffusion") {
        "stock_diffusion"
    } else {
        "other"
    }
}

fn relevant_paths_for_template<'a>(
    template_key: &str,
    relevant_paths: &[&'a PropagationPath],
) -> Vec<&'a PropagationPath> {
    relevant_paths
        .iter()
        .copied()
        .filter(|path| match template_key {
            TEMPLATE_PRE_MARKET_POSITIONING => path
                .steps
                .iter()
                .any(|step| path_family(&step.mechanism) != "other"),
            TEMPLATE_CROSS_MARKET_ARBITRAGE => path
                .steps
                .iter()
                .any(|step| path_family(&step.mechanism) == "cross_market_diffusion"),
            TEMPLATE_MOMENTUM_CONTINUATION => path.steps.iter().any(|step| {
                matches!(
                    path_family(&step.mechanism),
                    "stock_diffusion" | "cross_market_diffusion"
                )
            }),
            TEMPLATE_SECTOR_ROTATION => path.steps.iter().any(|step| {
                matches!(
                    path_family(&step.mechanism),
                    "sector_diffusion" | "stock_diffusion"
                )
            }),
            TEMPLATE_STRUCTURAL_DIFFUSION => path
                .steps
                .iter()
                .any(|step| path_family(&step.mechanism) != "other"),
            _ => false,
        })
        .collect()
}

fn path_polarity(template_key: &str) -> EvidencePolarity {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => EvidencePolarity::Contradicts,
        TEMPLATE_CROSS_MARKET_ARBITRAGE
        | TEMPLATE_MOMENTUM_CONTINUATION
        | TEMPLATE_SECTOR_ROTATION
        | TEMPLATE_STRUCTURAL_DIFFUSION => EvidencePolarity::Supports,
        _ => EvidencePolarity::Supports,
    }
}

fn diffusion_path_statement(template_key: &str, scope: &ReasoningScope) -> String {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => format!(
            "{} is being absorbed into a broader structural move, not just isolated pre-market positioning",
            scope_label(scope)
        ),
        TEMPLATE_CROSS_MARKET_ARBITRAGE => format!(
            "{} is receiving cross-market diffusion before full price convergence",
            scope_label(scope)
        ),
        TEMPLATE_MOMENTUM_CONTINUATION => format!(
            "{} is being reinforced by diffusion through connected names",
            scope_label(scope)
        ),
        TEMPLATE_SECTOR_ROTATION => format!(
            "{} is participating in sector-level structural diffusion",
            scope_label(scope)
        ),
        TEMPLATE_STRUCTURAL_DIFFUSION => format!(
            "{} is being touched by graph diffusion before the move is fully priced",
            scope_label(scope)
        ),
        _ => format!("{} is influenced by propagated structural change", scope_label(scope)),
    }
}

fn propagation_hop_penalty(hops: usize) -> Decimal {
    match hops {
        0 | 1 => Decimal::ONE,
        2 => Decimal::new(80, 2),
        3 => Decimal::new(60, 2),
        _ => Decimal::new(50, 2),
    }
}

pub(super) fn propagated_path_evidence(
    local_evidence: &[ReasoningEvidence],
    relevant_paths: &[&PropagationPath],
) -> (Decimal, Vec<String>) {
    if relevant_paths.is_empty() {
        return (Decimal::ZERO, Vec::new());
    }

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
        Decimal::ONE + local_support * Decimal::new(20, 2)
    } else {
        Decimal::new(35, 2)
    };
    let contradiction_penalty = Decimal::ONE - local_contradict * Decimal::new(35, 2);

    let mut scored = relevant_paths
        .iter()
        .map(|path| {
            let effective = (path.confidence
                * propagation_hop_penalty(path.steps.len())
                * local_bonus
                * contradiction_penalty)
                .round_dp(4)
                .clamp(Decimal::ZERO, Decimal::ONE);
            (effective, path.path_id.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let best_weight = scored.first().map(|item| item.0).unwrap_or(Decimal::ZERO);
    let path_ids = scored.into_iter().take(3).map(|item| item.1).collect();
    (best_weight, path_ids)
}
