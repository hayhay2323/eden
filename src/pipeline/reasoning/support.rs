use std::collections::HashSet;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::reasoning::{
    EvidencePolarity, InvalidationCondition, PropagationPath, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope,
};
use crate::pipeline::signals::{
    event_driver_kind, event_propagation_scope, DerivedSignalKind, EventDriverKind,
    EventPropagationScope, MarketEventKind, SignalScope,
};
pub(super) use super::family_gate::FamilyAlphaGate;

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

pub struct HypothesisTemplate {
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
    family_gate: Option<&FamilyAlphaGate>,
    absence_memory: &super::context::AbsenceMemory,
    world_state: Option<&crate::ontology::world::WorldStateSnapshot>,
    current_scope: &ReasoningScope,
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
        let template = HypothesisTemplate {
            key: "shared_holder_spillover".into(),
            family_label: "Shared-Holder Spillover".into(),
            thesis: "shared-holder spillover".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_family("institution_affinity") || has_family("institution_diffusion") {
        let template = HypothesisTemplate {
            key: "institution_relay".into(),
            family_label: "Institution Relay".into(),
            thesis: "institution relay".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_family("rotation") {
        let template = HypothesisTemplate {
            key: "sector_rotation_spillover".into(),
            family_label: "Sector Rotation Spillover".into(),
            thesis: "sector rotation spillover".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_family("market_stress") && has_family("rotation") {
        let template = HypothesisTemplate {
            key: "stress_feedback_loop".into(),
            family_label: "Stress Feedback Loop".into(),
            thesis: "stress feedback loop".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_family("market_stress")
        || has_signal(|kind| matches!(kind, DerivedSignalKind::MarketStress))
    {
        let template = HypothesisTemplate {
            key: "stress_concentration".into(),
            family_label: "Stress Concentration".into(),
            thesis: "market stress concentration".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_family("sector_symbol_bridge") {
        let template = HypothesisTemplate {
            key: "sector_symbol_spillover".into(),
            family_label: "Sector-Symbol Spillover".into(),
            thesis: "sector-symbol spillover".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
    }
    if has_mixed {
        let template = HypothesisTemplate {
            key: "cross_mechanism_chain".into(),
            family_label: "Cross-Mechanism Chain".into(),
            thesis: "cross-mechanism chain".into(),
        };
        if attribution_allows_template(relevant_events, template.key.as_str()) {
            templates.push(template);
        }
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
    if has_event(|kind| matches!(kind, MarketEventKind::CatalystActivation)) {
        templates.push(HypothesisTemplate {
            key: "catalyst_repricing".into(),
            family_label: "Catalyst Repricing".into(),
            thesis: "thematic catalyst-driven repricing".into(),
        });
    }
    if let Some(gate) = family_gate {
        templates.retain(|template| gate.allows(&template.family_label));
    }

    // Suppress propagation/spillover templates for sectors with repeated absence
    if let ReasoningScope::Sector(sector_id) = current_scope {
        templates.retain(|template| {
            let is_propagation = matches!(
                template.key.as_str(),
                "propagation"
                    | "shared_holder_spillover"
                    | "institution_relay"
                    | "sector_rotation_spillover"
                    | "sector_symbol_spillover"
                    | "cross_mechanism_chain"
                    | "stress_feedback_loop"
            );
            if is_propagation {
                !absence_memory.should_suppress(sector_id, &template.family_label)
            } else {
                true
            }
        });
    }

    // Block stress_feedback_loop in stabilizing regime
    if let Some(ws) = world_state {
        let is_stabilizing = ws.entities.iter().any(|e| {
            matches!(e.scope, ReasoningScope::Market(_)) && e.regime == "stabilizing"
        });
        if is_stabilizing {
            templates.retain(|t| t.key != "stress_feedback_loop");
        }
    }

    let mut seen = HashSet::new();
    templates.retain(|template| seen.insert(template.key.clone()));
    templates
}


fn attribution_allows_template(
    relevant_events: &[&crate::ontology::Event<crate::pipeline::signals::MarketEventRecord>],
    template_key: &str,
) -> bool {
    let strongest_scope = relevant_events
        .iter()
        .filter_map(|event| event_propagation_scope(event))
        .max();

    let Some(scope) = strongest_scope else {
        // No attribution data → allow everything (cold start).
        return true;
    };

    let scope_allowed = match template_key {
        // Pure local templates: always allowed regardless of attribution.
        "flow"
        | "liquidity"
        | "risk"
        | "catalyst_repricing"
        | "institution_reversal"
        | "breakout_contagion" => true,

        // Sector-level templates: need at least Sector attribution.
        "sector_rotation_spillover"
        | "sector_symbol_spillover"
        | "stress_concentration"
        | "stress_feedback_loop" => {
            matches!(
                scope,
                EventPropagationScope::Sector | EventPropagationScope::Market
            )
        }

        // Cross-scope / institutional templates: need at least Sector attribution.
        "shared_holder_spillover"
        | "institution_relay"
        | "cross_mechanism_chain"
        | "propagation" => {
            matches!(
                scope,
                EventPropagationScope::Sector | EventPropagationScope::Market
            )
        }

        _ => true,
    };

    if !scope_allowed {
        return false;
    }

    // Driver-kind gate: if ALL events are company_specific, block cross-scope templates.
    let all_company_specific = relevant_events
        .iter()
        .filter_map(|e| event_driver_kind(e))
        .all(|dk| dk == EventDriverKind::CompanySpecific);
    let has_any_driver = relevant_events
        .iter()
        .any(|e| event_driver_kind(e).is_some());

    if has_any_driver && all_company_specific {
        let is_cross_scope = matches!(
            template_key,
            "shared_holder_spillover"
                | "institution_relay"
                | "cross_mechanism_chain"
                | "propagation"
                | "sector_rotation_spillover"
                | "sector_symbol_spillover"
                | "stress_feedback_loop"
                | "stress_concentration"
        );
        if is_cross_scope {
            return false;
        }
    }

    true
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
        ("catalyst_repricing", K::CatalystActivation) => S,
        ("catalyst_repricing", K::ManualReviewRequired) => C,
        ("shared_holder_spillover", K::SharedHolderAnomaly) => S,
        ("shared_holder_spillover", K::InstitutionalFlip) => C,
        ("institution_relay", K::InstitutionalFlip | K::SharedHolderAnomaly) => S,
        ("institution_relay", K::ManualReviewRequired) => C,
        ("sector_rotation_spillover", K::StressRegimeShift | K::CompositeAcceleration) => S,
        ("sector_rotation_spillover", K::ManualReviewRequired) => C,
        ("stress_feedback_loop", K::MarketStressElevated | K::StressRegimeShift) => S,
        ("stress_feedback_loop", K::CandlestickBreakout) => C,
        ("stress_concentration", K::MarketStressElevated | K::StressRegimeShift) => S,
        ("stress_concentration", K::CandlestickBreakout) => C,
        ("sector_symbol_spillover", K::SharedHolderAnomaly | K::VolumeDislocation) => S,
        ("sector_symbol_spillover", K::ManualReviewRequired) => C,
        (
            "propagation"
            | "sector_rotation_spillover"
            | "sector_symbol_spillover"
            | "cross_mechanism_chain"
            | "catalyst_repricing",
            K::PropagationAbsence,
        ) => C,
        (
            "cross_mechanism_chain",
            K::SharedHolderAnomaly | K::StressRegimeShift | K::CompositeAcceleration,
        ) => S,
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
    use DerivedSignalKind as K;
    use EvidencePolarity::{Contradicts as C, Supports as S};

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
        ("institution_relay", K::SmartMoneyPressure | K::Convergence) => S,
        ("institution_relay", K::MarketStress) => C,
        ("sector_rotation_spillover", K::Convergence | K::StructuralComposite) => S,
        ("sector_rotation_spillover", K::MarketStress) => C,
        ("stress_feedback_loop", K::MarketStress | K::StructuralComposite) => S,
        ("stress_feedback_loop", K::CandlestickConviction) => C,
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
        | "institution_relay"
        | "sector_rotation_spillover"
        | "stress_feedback_loop"
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
        ReasoningScope::Symbol(symbol) => {
            format!("{} may currently reflect {}", symbol, template.thesis)
        }
        ReasoningScope::Sector(sector) => format!(
            "sector {} may currently reflect {}",
            sector, template.thesis
        ),
        ReasoningScope::Institution(institution) => {
            format!(
                "institution {} may currently reflect {}",
                institution, template.thesis
            )
        }
        ReasoningScope::Theme(theme) => {
            format!("theme {} may currently reflect {}", theme, template.thesis)
        }
        ReasoningScope::Region(region) => format!(
            "region {} may currently reflect {}",
            region, template.thesis
        ),
        ReasoningScope::Custom(value) => {
            format!("{} may currently reflect {}", value, template.thesis)
        }
    }
}

pub(super) fn template_invalidation(template: &HypothesisTemplate) -> Vec<InvalidationCondition> {
    let description = match template.key.as_str() {
        "flow" => "directional flow evidence reverses or weakens",
        "liquidity" => "depth asymmetry and candle stress normalize",
        "propagation" => "connected scopes stop co-moving or the path breaks",
        "risk" => "market stress and risk-sensitive events revert",
        "shared_holder_spillover" => "shared-holder crowding link weakens or peers decouple",
        "institution_relay" => "institution relay loses synchronization or affinity breaks",
        "sector_rotation_spillover" => "sector rotation stalls or reverses",
        "stress_feedback_loop" => "stress stops feeding back through the rotation complex",
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
        "shared_holder_spillover" => {
            vec!["peer names should move with shared-holder pressure".into()]
        }
        "institution_relay" => {
            vec!["institution-linked scopes should relay the move in sequence".into()]
        }
        "sector_rotation_spillover" => {
            vec!["sector beneficiaries and victims should diverge further".into()]
        }
        "stress_feedback_loop" => {
            vec!["stress and rotation should keep reinforcing each other".into()]
        }
        "stress_concentration" => {
            vec!["market stress should cluster into the same vulnerable sectors".into()]
        }
        "sector_symbol_spillover" => vec!["sector move should leak into linked symbols".into()],
        "cross_mechanism_chain" => {
            vec!["multiple mechanisms should reinforce the same direction".into()]
        }
        "institution_reversal" => {
            vec!["institutional flow should continue flipping the same way".into()]
        }
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

/// Build a one-sentence causal narrative: why does this case exist at the reasoning level?
///
/// Distinct from entry_rationale (policy justification). This answers: what causal chain
/// produced this hypothesis? It considers:
/// - The trigger event (strongest supporting evidence)
/// - The driver kind (company-specific vs sector-wide vs macro, from attribution provenance)
/// - Whether propagation paths reinforce the thesis
/// - The diversity of evidence channels
pub(super) fn build_causal_narrative(
    scope: &ReasoningScope,
    family_label: &str,
    evidence: &[ReasoningEvidence],
) -> String {
    let supporting: Vec<_> = evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Supports)
        .collect();

    if supporting.is_empty() {
        return format!(
            "{} is under investigation for {} based on structural signals",
            scope_title(scope),
            family_label
        );
    }

    let strongest = supporting
        .iter()
        .max_by(|a, b| a.weight.cmp(&b.weight))
        .unwrap();

    // Extract driver kind from evidence provenance
    let driver_phrase = supporting
        .iter()
        .flat_map(|e| e.references.iter())
        .find_map(|r| r.strip_prefix("attr:driver="))
        .map(|driver| match driver {
            "company_specific" => "a company-specific event",
            "sector_wide" => "a sector-wide dynamic",
            "macro_wide" => "a macro-level shift",
            _ => "an observed signal",
        })
        .unwrap_or("observed market activity");

    // Check if propagation paths reinforce
    let has_propagation = supporting
        .iter()
        .any(|e| e.kind == ReasoningEvidenceKind::PropagatedPath);

    // Count distinct evidence channels
    let local_event_count = supporting
        .iter()
        .filter(|e| e.kind == ReasoningEvidenceKind::LocalEvent)
        .count();
    let local_signal_count = supporting
        .iter()
        .filter(|e| e.kind == ReasoningEvidenceKind::LocalSignal)
        .count();

    let scope_name = scope_title(scope);

    // Build narrative from components
    let trigger = &strongest.statement;
    let propagation_clause = if has_propagation {
        ", reinforced by cross-scope propagation paths"
    } else {
        ""
    };
    let channel_clause = if local_event_count + local_signal_count > 2 {
        format!(
            " ({} events + {} signals converging)",
            local_event_count, local_signal_count
        )
    } else {
        String::new()
    };

    format!(
        "{scope_name}: {driver_phrase} — {trigger} — suggests {family}{propagation}{channels}",
        scope_name = scope_name,
        driver_phrase = driver_phrase,
        trigger = trigger,
        family = family_label.to_ascii_lowercase(),
        propagation = propagation_clause,
        channels = channel_clause,
    )
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
        SignalScope::Theme(theme) => ReasoningScope::Theme(theme.clone()),
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
            | (ReasoningScope::Theme(_), SignalScope::Theme(_))
    ) && scope_id(scope) == scope_id(&convert_scope(signal_scope))
}

pub(super) fn path_relevant_to_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}

pub(crate) fn hk_session_label(timestamp: OffsetDateTime) -> &'static str {
    let hk = timestamp.to_offset(time::UtcOffset::from_hms(8, 0, 0).expect("valid hk offset"));
    let minutes = u16::from(hk.hour()) * 60 + u16::from(hk.minute());
    match minutes {
        570..=630 => "opening",
        631..=870 => "midday",
        871..=970 => "closing",
        _ => "offhours",
    }
}

