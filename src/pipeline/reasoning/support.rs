use std::collections::HashSet;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::reasoning::{
    EvidencePolarity, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope,
};
use crate::pipeline::signals::{DerivedSignalKind, MarketEventKind, SignalScope};

use super::propagation::{path_has_family, path_is_mixed_multi_hop};

pub(super) struct EvidenceWeightSummary {
    pub local_support: Decimal,
    pub local_contradict: Decimal,
    pub propagated_support: Decimal,
    pub propagated_contradict: Decimal,
}

pub(super) fn summarize_evidence_weights(evidence: &[ReasoningEvidence]) -> EvidenceWeightSummary {
    let mut summary = EvidenceWeightSummary {
        local_support: Decimal::ZERO,
        local_contradict: Decimal::ZERO,
        propagated_support: Decimal::ZERO,
        propagated_contradict: Decimal::ZERO,
    };

    for item in evidence {
        match (item.kind, item.polarity) {
            (
                ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal,
                EvidencePolarity::Supports,
            ) => summary.local_support += item.weight,
            (
                ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal,
                EvidencePolarity::Contradicts,
            ) => summary.local_contradict += item.weight,
            (ReasoningEvidenceKind::PropagatedPath, EvidencePolarity::Supports) => {
                summary.propagated_support += item.weight
            }
            (ReasoningEvidenceKind::PropagatedPath, EvidencePolarity::Contradicts) => {
                summary.propagated_contradict += item.weight
            }
        }
    }

    summary
}

pub(super) fn hypothesis_provenance(
    observed_at: OffsetDateTime,
    trace_id: &str,
    family_label: &str,
    evidence: &[ReasoningEvidence],
    path_ids: &[String],
) -> crate::ontology::ProvenanceMetadata {
    let mut inputs = evidence
        .iter()
        .flat_map(|item| item.provenance.inputs.clone())
        .collect::<Vec<_>>();
    inputs.extend(path_ids.iter().cloned());
    inputs.sort();
    inputs.dedup();

    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_confidence(competing_hypothesis_confidence(evidence))
    .with_inputs(inputs)
    .with_note(format!("family={}", family_label))
}

pub(super) fn setup_provenance<I, S>(
    observed_at: OffsetDateTime,
    trace_id: &str,
    hypothesis_id: Option<&str>,
    runner_up_hypothesis_id: Option<&str>,
    inputs: I,
) -> crate::ontology::ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut lineage = inputs.into_iter().map(Into::into).collect::<Vec<_>>();
    if let Some(hypothesis_id) = hypothesis_id {
        lineage.push(hypothesis_id.to_string());
    }
    if let Some(runner_up_hypothesis_id) = runner_up_hypothesis_id {
        lineage.push(runner_up_hypothesis_id.to_string());
    }
    lineage.sort();
    lineage.dedup();

    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_inputs(lineage)
}

pub(super) struct HypothesisTemplate {
    pub key: String,
    pub family_label: String,
    pub thesis: String,
}

pub(super) fn hypothesis_templates(
    relevant_events: &[&crate::ontology::Event<crate::pipeline::signals::MarketEventRecord>],
    relevant_signals: &[&crate::ontology::DerivedSignal<
        crate::pipeline::signals::DerivedSignalRecord,
    >],
    relevant_paths: &[&PropagationPath],
) -> Vec<HypothesisTemplate> {
    let mut templates = vec![
        HypothesisTemplate {
            key: "flow".into(),
            family_label: "Directed Flow".into(),
            thesis: "directed flow repricing".into(),
        },
        HypothesisTemplate {
            key: "liquidity".into(),
            family_label: "Liquidity Dislocation".into(),
            thesis: "local liquidity dislocation".into(),
        },
        HypothesisTemplate {
            key: "propagation".into(),
            family_label: "Propagation Chain".into(),
            thesis: "cross-scope propagation".into(),
        },
        HypothesisTemplate {
            key: "risk".into(),
            family_label: "Risk Repricing".into(),
            thesis: "risk repricing".into(),
        },
    ];

    let has_family = |family: &str| {
        relevant_paths
            .iter()
            .any(|path| path_has_family(path, family))
    };
    let has_mixed = relevant_paths
        .iter()
        .any(|path| path_is_mixed_multi_hop(path));
    let has_event = |predicate: fn(&MarketEventKind) -> bool| {
        relevant_events
            .iter()
            .any(|event| predicate(&event.value.kind))
    };
    let has_signal = |predicate: fn(&DerivedSignalKind) -> bool| {
        relevant_signals
            .iter()
            .any(|signal| predicate(&signal.value.kind))
    };

    if has_family("shared_holder") {
        templates.push(HypothesisTemplate {
            key: "shared_holder_spillover".into(),
            family_label: "Shared-Holder Spillover".into(),
            thesis: "shared-holder spillover".into(),
        });
    }
    if has_family("rotation") {
        templates.push(HypothesisTemplate {
            key: "sector_rotation_spillover".into(),
            family_label: "Sector Rotation Spillover".into(),
            thesis: "sector rotation spillover".into(),
        });
    }
    if has_family("market_stress") || has_signal(|kind| matches!(kind, DerivedSignalKind::MarketStress)) {
        templates.push(HypothesisTemplate {
            key: "stress_concentration".into(),
            family_label: "Stress Concentration".into(),
            thesis: "market stress concentration".into(),
        });
    }
    if has_family("sector_symbol_bridge") {
        templates.push(HypothesisTemplate {
            key: "sector_symbol_spillover".into(),
            family_label: "Sector-Symbol Spillover".into(),
            thesis: "sector-symbol spillover".into(),
        });
    }
    if has_mixed {
        templates.push(HypothesisTemplate {
            key: "cross_mechanism_chain".into(),
            family_label: "Cross-Mechanism Chain".into(),
            thesis: "cross-mechanism chain".into(),
        });
    }
    if has_event(|kind| matches!(kind, MarketEventKind::InstitutionalFlip)) {
        templates.push(HypothesisTemplate {
            key: "institution_reversal".into(),
            family_label: "Institution Reversal".into(),
            thesis: "institution reversal".into(),
        });
    }
    if has_event(|kind| matches!(kind, MarketEventKind::CandlestickBreakout))
        || has_signal(|kind| matches!(kind, DerivedSignalKind::CandlestickConviction))
    {
        templates.push(HypothesisTemplate {
            key: "breakout_contagion".into(),
            family_label: "Breakout Contagion".into(),
            thesis: "breakout-led contagion".into(),
        });
    }

    let mut seen = HashSet::new();
    templates.retain(|template| seen.insert(template.key.clone()));
    templates
}

pub(super) fn scope_matches_event(scope: &ReasoningScope, event_scope: &SignalScope) -> bool {
    let converted = convert_scope(event_scope);
    converted == *scope || matches!(event_scope, SignalScope::Market)
}

pub(super) fn scope_matches_signal_or_market(
    scope: &ReasoningScope,
    signal_scope: &SignalScope,
) -> bool {
    scope_matches_signal(scope, signal_scope) || matches!(signal_scope, SignalScope::Market)
}

pub(super) fn event_polarity(
    template: &HypothesisTemplate,
    kind: &MarketEventKind,
) -> Option<EvidencePolarity> {
    use EvidencePolarity::{Contradicts as C, Supports as S};
    use MarketEventKind as K;

    let polarity = match (template.key.as_str(), kind) {
        ("flow", K::SmartMoneyPressure | K::VolumeDislocation | K::CompositeAcceleration) => S,
        ("flow", K::ManualReviewRequired | K::InstitutionalFlip) => C,
        ("liquidity", K::OrderBookDislocation | K::CandlestickBreakout) => S,
        ("liquidity", K::SharedHolderAnomaly | K::StressRegimeShift) => C,
        ("propagation", K::SharedHolderAnomaly | K::StressRegimeShift) => S,
        ("propagation", K::OrderBookDislocation) => C,
        ("risk", K::MarketStressElevated | K::StressRegimeShift | K::InstitutionalFlip) => S,
        ("risk", K::CandlestickBreakout) => C,
        ("shared_holder_spillover", K::SharedHolderAnomaly) => S,
        ("shared_holder_spillover", K::InstitutionalFlip) => C,
        ("sector_rotation_spillover", K::StressRegimeShift | K::CompositeAcceleration) => S,
        ("sector_rotation_spillover", K::ManualReviewRequired) => C,
        ("stress_concentration", K::MarketStressElevated | K::StressRegimeShift) => S,
        ("stress_concentration", K::CandlestickBreakout) => C,
        ("sector_symbol_spillover", K::SharedHolderAnomaly | K::VolumeDislocation) => S,
        ("sector_symbol_spillover", K::ManualReviewRequired) => C,
        ("cross_mechanism_chain", K::SharedHolderAnomaly | K::StressRegimeShift | K::CompositeAcceleration) => S,
        ("cross_mechanism_chain", K::ManualReviewRequired) => C,
        ("institution_reversal", K::InstitutionalFlip | K::ManualReviewRequired) => S,
        ("institution_reversal", K::CandlestickBreakout) => C,
        ("breakout_contagion", K::CandlestickBreakout | K::SharedHolderAnomaly) => S,
        ("breakout_contagion", K::MarketStressElevated) => C,
        _ => return None,
    };
    Some(polarity)
}

pub(super) fn signal_polarity(
    template: &HypothesisTemplate,
    kind: &DerivedSignalKind,
) -> Option<EvidencePolarity> {
    use EvidencePolarity::{Contradicts as C, Supports as S};
    use DerivedSignalKind as K;

    let polarity = match (template.key.as_str(), kind) {
        ("flow", K::Convergence | K::SmartMoneyPressure | K::ActivityMomentum) => S,
        ("flow", K::MarketStress) => C,
        ("liquidity", K::CandlestickConviction | K::StructuralComposite) => S,
        ("liquidity", K::MarketStress) => C,
        ("propagation", K::MarketStress | K::Convergence) => S,
        ("propagation", K::CandlestickConviction) => C,
        ("risk", K::MarketStress) => S,
        ("risk", K::ActivityMomentum) => C,
        ("shared_holder_spillover", K::Convergence | K::SmartMoneyPressure) => S,
        ("shared_holder_spillover", K::MarketStress) => C,
        ("sector_rotation_spillover", K::Convergence | K::StructuralComposite) => S,
        ("sector_rotation_spillover", K::MarketStress) => C,
        ("stress_concentration", K::MarketStress) => S,
        ("stress_concentration", K::ActivityMomentum) => C,
        ("sector_symbol_spillover", K::StructuralComposite | K::Convergence) => S,
        ("sector_symbol_spillover", K::MarketStress) => C,
        ("cross_mechanism_chain", K::Convergence | K::MarketStress) => S,
        ("cross_mechanism_chain", K::CandlestickConviction) => C,
        ("institution_reversal", K::SmartMoneyPressure | K::Convergence) => S,
        ("institution_reversal", K::MarketStress) => C,
        ("breakout_contagion", K::CandlestickConviction | K::ActivityMomentum) => S,
        ("breakout_contagion", K::MarketStress) => C,
        _ => return None,
    };
    Some(polarity)
}

pub(super) fn path_polarity(template: &HypothesisTemplate) -> EvidencePolarity {
    match template.key.as_str() {
        "propagation"
        | "risk"
        | "shared_holder_spillover"
        | "sector_rotation_spillover"
        | "stress_concentration"
        | "sector_symbol_spillover"
        | "cross_mechanism_chain"
        | "breakout_contagion" => EvidencePolarity::Supports,
        _ => EvidencePolarity::Contradicts,
    }
}

pub(super) fn template_statement(template: &HypothesisTemplate, scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => format!("the market may be governed by {}", template.thesis),
        ReasoningScope::Symbol(symbol) => format!("{} may currently reflect {}", symbol, template.thesis),
        ReasoningScope::Sector(sector) => format!("sector {} may currently reflect {}", sector, template.thesis),
        ReasoningScope::Institution(institution) => {
            format!("institution {} may currently reflect {}", institution, template.thesis)
        }
        ReasoningScope::Theme(theme) => format!("theme {} may currently reflect {}", theme, template.thesis),
        ReasoningScope::Region(region) => format!("region {} may currently reflect {}", region, template.thesis),
        ReasoningScope::Custom(value) => format!("{} may currently reflect {}", value, template.thesis),
    }
}

pub(super) fn template_invalidation(template: &HypothesisTemplate) -> Vec<InvalidationCondition> {
    let description = match template.key.as_str() {
        "flow" => "directional flow evidence reverses or weakens",
        "liquidity" => "depth asymmetry and candle stress normalize",
        "propagation" => "connected scopes stop co-moving or the path breaks",
        "risk" => "market stress and risk-sensitive events revert",
        "shared_holder_spillover" => "shared-holder crowding link weakens or peers decouple",
        "sector_rotation_spillover" => "sector rotation stalls or reverses",
        "stress_concentration" => "market stress diffuses and sectors decouple",
        "sector_symbol_spillover" => "sector-symbol spillover stops transmitting",
        "cross_mechanism_chain" => "one leg of the cross-mechanism chain breaks",
        "institution_reversal" => "institutional reversal no longer persists",
        "breakout_contagion" => "breakout loses follow-through or contagion stops",
        _ => "the core supporting evidence disappears",
    };

    vec![InvalidationCondition {
        description: description.into(),
        references: Vec::new(),
    }]
}

pub(super) fn template_expected_observations(template: &HypothesisTemplate) -> Vec<String> {
    match template.key.as_str() {
        "flow" => vec!["directional participation should persist".into()],
        "liquidity" => vec!["local imbalance should remain visible in depth or candles".into()],
        "propagation" => vec!["linked scopes should start repricing in sequence".into()],
        "risk" => vec!["stress-sensitive assets should move coherently".into()],
        "shared_holder_spillover" => vec!["peer names should move with shared-holder pressure".into()],
        "sector_rotation_spillover" => vec!["sector beneficiaries and victims should diverge further".into()],
        "stress_concentration" => vec!["market stress should cluster into the same vulnerable sectors".into()],
        "sector_symbol_spillover" => vec!["sector move should leak into linked symbols".into()],
        "cross_mechanism_chain" => vec!["multiple mechanisms should reinforce the same direction".into()],
        "institution_reversal" => vec!["institutional flow should continue flipping the same way".into()],
        "breakout_contagion" => vec!["breakout leaders should drag peers along".into()],
        _ => vec!["supporting evidence should persist".into()],
    }
}

pub(super) fn competing_hypothesis_confidence(evidence: &[ReasoningEvidence]) -> Decimal {
    let support = evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Supports)
        .map(|item| item.weight)
        .sum::<Decimal>();
    let contradict = evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Contradicts)
        .map(|item| item.weight)
        .sum::<Decimal>();

    let total = support + contradict;
    if total == Decimal::ZERO {
        Decimal::ZERO
    } else {
        ((support - contradict) / total + Decimal::ONE) / Decimal::TWO
    }
    .clamp(Decimal::ZERO, Decimal::ONE)
}

pub(super) fn derived_provenance(
    observed_at: OffsetDateTime,
    confidence: Decimal,
    inputs: &[String],
) -> crate::ontology::ProvenanceMetadata {
    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_confidence(confidence)
    .with_inputs(inputs.iter().cloned())
}

pub(super) fn convert_scope(scope: &SignalScope) -> ReasoningScope {
    match scope {
        SignalScope::Market => ReasoningScope::market(),
        SignalScope::Symbol(symbol) => ReasoningScope::Symbol(symbol.clone()),
        SignalScope::Institution(institution) => ReasoningScope::Institution(institution.clone()),
        SignalScope::Sector(sector) => ReasoningScope::Sector(sector.clone()),
    }
}

pub(super) fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => sector.to_string(),
        ReasoningScope::Institution(institution) => institution.to_string(),
        ReasoningScope::Theme(theme) => theme.to_string(),
        ReasoningScope::Region(region) => region.to_string(),
        ReasoningScope::Custom(value) => value.to_string(),
    }
}

pub(crate) fn track_id_for_scope(scope: &ReasoningScope) -> String {
    format!("track:{}", scope_id(scope))
}

pub(super) fn stable_setup_id(scope: &ReasoningScope) -> String {
    format!("setup:{}", scope_id(scope))
}

pub(super) fn scope_title(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "Market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => format!("Sector {}", sector),
        ReasoningScope::Institution(institution) => format!("Institution {}", institution),
        ReasoningScope::Theme(theme) => format!("Theme {}", theme),
        ReasoningScope::Region(region) => format!("Region {}", region),
        ReasoningScope::Custom(value) => value.to_string(),
    }
}

pub(super) fn scope_matches_signal(scope: &ReasoningScope, signal_scope: &SignalScope) -> bool {
    matches!(
        (scope, signal_scope),
        (ReasoningScope::Market(_), SignalScope::Market)
            | (ReasoningScope::Symbol(_), SignalScope::Symbol(_))
            | (ReasoningScope::Institution(_), SignalScope::Institution(_))
            | (ReasoningScope::Sector(_), SignalScope::Sector(_))
    ) && scope_id(scope) == scope_id(&convert_scope(signal_scope))
}

pub(super) fn path_relevant_to_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}
