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

// ---------------------------------------------------------------------------
// Template metadata registry — single source of truth for all template
// polarity, invalidation, expected-observation, and priority data.
// ---------------------------------------------------------------------------

struct TemplateMetadata {
    key: &'static str,
    /// Events that SUPPORT this template's thesis
    supporting_events: &'static [MarketEventKind],
    /// Events that CONTRADICT this template's thesis
    contradicting_events: &'static [MarketEventKind],
    /// Signal kinds that SUPPORT this template's thesis
    supporting_signals: &'static [DerivedSignalKind],
    /// Signal kinds that CONTRADICT this template's thesis
    contradicting_signals: &'static [DerivedSignalKind],
    /// Whether propagation paths support (true) or contradict (false)
    path_supports: bool,
    /// Description of what would invalidate the hypothesis
    invalidation: &'static str,
    /// Expected observations if hypothesis is correct
    expected_observations: &'static [&'static str],
    /// Priority score for ranking
    priority: i32,
}

static TEMPLATE_REGISTRY: &[TemplateMetadata] = &[
    TemplateMetadata {
        key: "flow",
        supporting_events: &[
            MarketEventKind::SmartMoneyPressure,
            MarketEventKind::VolumeDislocation,
            MarketEventKind::CompositeAcceleration,
        ],
        contradicting_events: &[
            MarketEventKind::ManualReviewRequired,
            MarketEventKind::InstitutionalFlip,
        ],
        supporting_signals: &[
            DerivedSignalKind::Convergence,
            DerivedSignalKind::SmartMoneyPressure,
            DerivedSignalKind::ActivityMomentum,
        ],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: false,
        invalidation: "directional flow evidence reverses or weakens",
        expected_observations: &["directional participation should persist"],
        priority: 120,
    },
    TemplateMetadata {
        key: "liquidity",
        supporting_events: &[
            MarketEventKind::OrderBookDislocation,
            MarketEventKind::CandlestickBreakout,
        ],
        contradicting_events: &[
            MarketEventKind::SharedHolderAnomaly,
            MarketEventKind::StressRegimeShift,
        ],
        supporting_signals: &[DerivedSignalKind::CandlestickConviction, DerivedSignalKind::StructuralComposite],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: false,
        invalidation: "depth asymmetry and candle stress normalize",
        expected_observations: &[
            "local imbalance should remain visible in depth or candles",
        ],
        priority: 115,
    },
    TemplateMetadata {
        key: "propagation",
        supporting_events: &[
            MarketEventKind::SharedHolderAnomaly,
            MarketEventKind::StressRegimeShift,
        ],
        contradicting_events: &[
            MarketEventKind::OrderBookDislocation,
            MarketEventKind::PropagationAbsence,
        ],
        supporting_signals: &[DerivedSignalKind::MarketStress, DerivedSignalKind::Convergence],
        contradicting_signals: &[DerivedSignalKind::CandlestickConviction],
        path_supports: true,
        invalidation: "connected scopes stop co-moving or the path breaks",
        expected_observations: &[
            "linked scopes should start repricing in sequence",
        ],
        priority: 100,
    },
    TemplateMetadata {
        key: "risk",
        supporting_events: &[
            MarketEventKind::MarketStressElevated,
            MarketEventKind::StressRegimeShift,
            MarketEventKind::InstitutionalFlip,
        ],
        contradicting_events: &[MarketEventKind::CandlestickBreakout],
        supporting_signals: &[DerivedSignalKind::MarketStress],
        contradicting_signals: &[DerivedSignalKind::ActivityMomentum],
        path_supports: true,
        invalidation: "market stress and risk-sensitive events revert",
        expected_observations: &[
            "stress-sensitive assets should move coherently",
        ],
        priority: 88,
    },
    TemplateMetadata {
        key: "catalyst_repricing",
        supporting_events: &[MarketEventKind::CatalystActivation],
        contradicting_events: &[
            MarketEventKind::ManualReviewRequired,
            MarketEventKind::PropagationAbsence,
        ],
        supporting_signals: &[],
        contradicting_signals: &[],
        path_supports: false,
        invalidation: "the core supporting evidence disappears",
        expected_observations: &["supporting evidence should persist"],
        priority: 108,
    },
    TemplateMetadata {
        key: "shared_holder_spillover",
        supporting_events: &[MarketEventKind::SharedHolderAnomaly],
        contradicting_events: &[MarketEventKind::InstitutionalFlip],
        supporting_signals: &[DerivedSignalKind::Convergence, DerivedSignalKind::SmartMoneyPressure],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "shared-holder crowding link weakens or peers decouple",
        expected_observations: &[
            "peer names should move with shared-holder pressure",
        ],
        priority: 96,
    },
    TemplateMetadata {
        key: "institution_relay",
        supporting_events: &[
            MarketEventKind::InstitutionalFlip,
            MarketEventKind::SharedHolderAnomaly,
        ],
        contradicting_events: &[MarketEventKind::ManualReviewRequired],
        supporting_signals: &[DerivedSignalKind::SmartMoneyPressure, DerivedSignalKind::Convergence],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "institution relay loses synchronization or affinity breaks",
        expected_observations: &[
            "institution-linked scopes should relay the move in sequence",
        ],
        priority: 96,
    },
    TemplateMetadata {
        key: "sector_rotation_spillover",
        supporting_events: &[
            MarketEventKind::StressRegimeShift,
            MarketEventKind::CompositeAcceleration,
        ],
        contradicting_events: &[
            MarketEventKind::ManualReviewRequired,
            MarketEventKind::PropagationAbsence,
        ],
        supporting_signals: &[DerivedSignalKind::Convergence, DerivedSignalKind::StructuralComposite],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "sector rotation stalls or reverses",
        expected_observations: &[
            "sector beneficiaries and victims should diverge further",
        ],
        priority: 92,
    },
    TemplateMetadata {
        key: "stress_feedback_loop",
        supporting_events: &[
            MarketEventKind::MarketStressElevated,
            MarketEventKind::StressRegimeShift,
        ],
        contradicting_events: &[MarketEventKind::CandlestickBreakout],
        supporting_signals: &[DerivedSignalKind::MarketStress, DerivedSignalKind::StructuralComposite],
        contradicting_signals: &[DerivedSignalKind::CandlestickConviction],
        path_supports: true,
        invalidation: "stress stops feeding back through the rotation complex",
        expected_observations: &[
            "stress and rotation should keep reinforcing each other",
        ],
        priority: 88,
    },
    TemplateMetadata {
        key: "stress_concentration",
        supporting_events: &[
            MarketEventKind::MarketStressElevated,
            MarketEventKind::StressRegimeShift,
        ],
        contradicting_events: &[MarketEventKind::CandlestickBreakout],
        supporting_signals: &[DerivedSignalKind::MarketStress],
        contradicting_signals: &[DerivedSignalKind::ActivityMomentum],
        path_supports: true,
        invalidation: "market stress diffuses and sectors decouple",
        expected_observations: &[
            "market stress should cluster into the same vulnerable sectors",
        ],
        priority: 88,
    },
    TemplateMetadata {
        key: "sector_symbol_spillover",
        supporting_events: &[
            MarketEventKind::SharedHolderAnomaly,
            MarketEventKind::VolumeDislocation,
        ],
        contradicting_events: &[
            MarketEventKind::ManualReviewRequired,
            MarketEventKind::PropagationAbsence,
        ],
        supporting_signals: &[DerivedSignalKind::StructuralComposite, DerivedSignalKind::Convergence],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "sector-symbol spillover stops transmitting",
        expected_observations: &["sector move should leak into linked symbols"],
        priority: 96,
    },
    TemplateMetadata {
        key: "cross_mechanism_chain",
        supporting_events: &[
            MarketEventKind::SharedHolderAnomaly,
            MarketEventKind::StressRegimeShift,
            MarketEventKind::CompositeAcceleration,
        ],
        contradicting_events: &[
            MarketEventKind::ManualReviewRequired,
            MarketEventKind::PropagationAbsence,
        ],
        supporting_signals: &[DerivedSignalKind::Convergence, DerivedSignalKind::MarketStress],
        contradicting_signals: &[DerivedSignalKind::CandlestickConviction],
        path_supports: true,
        invalidation: "one leg of the cross-mechanism chain breaks",
        expected_observations: &[
            "multiple mechanisms should reinforce the same direction",
        ],
        priority: 84,
    },
    TemplateMetadata {
        key: "institution_reversal",
        supporting_events: &[
            MarketEventKind::InstitutionalFlip,
            MarketEventKind::ManualReviewRequired,
        ],
        contradicting_events: &[MarketEventKind::CandlestickBreakout],
        supporting_signals: &[DerivedSignalKind::SmartMoneyPressure, DerivedSignalKind::Convergence],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "institutional reversal no longer persists",
        expected_observations: &[
            "institutional flow should continue flipping the same way",
        ],
        priority: 104,
    },
    TemplateMetadata {
        key: "breakout_contagion",
        supporting_events: &[
            MarketEventKind::CandlestickBreakout,
            MarketEventKind::SharedHolderAnomaly,
        ],
        contradicting_events: &[MarketEventKind::MarketStressElevated],
        supporting_signals: &[DerivedSignalKind::CandlestickConviction, DerivedSignalKind::ActivityMomentum],
        contradicting_signals: &[DerivedSignalKind::MarketStress],
        path_supports: true,
        invalidation: "breakout loses follow-through or contagion stops",
        expected_observations: &[
            "breakout leaders should drag peers along",
        ],
        priority: 110,
    },
];

fn lookup_template(key: &str) -> Option<&'static TemplateMetadata> {
    TEMPLATE_REGISTRY.iter().find(|m| m.key == key)
}

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
    let meta = lookup_template(template.key.as_str())?;
    if meta.supporting_events.contains(kind) {
        Some(EvidencePolarity::Supports)
    } else if meta.contradicting_events.contains(kind) {
        Some(EvidencePolarity::Contradicts)
    } else {
        None
    }
}

pub(super) fn signal_polarity(
    template: &HypothesisTemplate,
    kind: &DerivedSignalKind,
) -> Option<EvidencePolarity> {
    let meta = lookup_template(template.key.as_str())?;
    if meta.supporting_signals.contains(kind) {
        Some(EvidencePolarity::Supports)
    } else if meta.contradicting_signals.contains(kind) {
        Some(EvidencePolarity::Contradicts)
    } else {
        None
    }
}

pub(super) fn path_polarity(template: &HypothesisTemplate) -> EvidencePolarity {
    match lookup_template(template.key.as_str()) {
        Some(meta) if meta.path_supports => EvidencePolarity::Supports,
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
    let description = lookup_template(template.key.as_str())
        .map(|m| m.invalidation)
        .unwrap_or("the core supporting evidence disappears");

    vec![InvalidationCondition {
        description: description.into(),
        references: Vec::new(),
    }]
}

pub(super) fn template_expected_observations(template: &HypothesisTemplate) -> Vec<String> {
    match lookup_template(template.key.as_str()) {
        Some(meta) if !meta.expected_observations.is_empty() => meta
            .expected_observations
            .iter()
            .map(|s| (*s).into())
            .collect(),
        _ => vec!["supporting evidence should persist".into()],
    }
}

/// Look up the priority score for a template key. Used by synthesis.rs.
pub(super) fn template_priority(key: &str) -> i32 {
    lookup_template(key).map(|m| m.priority).unwrap_or(80)
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

