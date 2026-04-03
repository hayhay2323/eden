use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::graph::decision::{DecisionSnapshot, OrderDirection};
use crate::graph::insights::GraphInsights;
use crate::ontology::reasoning::{
    DecisionLineage, EvidencePolarity, Hypothesis, InvestigationSelection, PropagationPath,
    ReasoningEvidence, ReasoningEvidenceKind, ReasoningScope, TacticalSetup,
};
use crate::pipeline::dimensions::SymbolDimensions;
use crate::pipeline::signals::{
    DerivedSignalKind, DerivedSignalSnapshot, EventSnapshot, MarketEventKind,
};

use super::propagation::{hop_penalty, mechanism_family};
use super::support::{
    build_causal_narrative, competing_hypothesis_confidence, convert_scope, derived_provenance,
    event_polarity, hypothesis_provenance, hypothesis_templates, path_polarity,
    path_relevant_to_scope, scope_id, scope_matches_event, scope_matches_signal_or_market,
    scope_title, setup_provenance, signal_polarity, stable_setup_id, summarize_evidence_weights,
    template_expected_observations, template_invalidation, template_statement, FamilyAlphaGate,
};

pub(super) struct SetupSupportContext<'a> {
    pub events: &'a EventSnapshot,
    pub insights: &'a GraphInsights,
    pub symbol_dimensions: Option<&'a HashMap<crate::ontology::objects::Symbol, SymbolDimensions>>,
    pub convergence_components: &'a HashMap<crate::ontology::objects::Symbol, crate::graph::convergence::ConvergenceScore>,
}

#[derive(Default)]
struct OrderSupportContract {
    symbol_event_count: usize,
    directional_support: Decimal,
    directional_conflict: Decimal,
    flow_direction: Decimal,
    activity_momentum: Decimal,
    candlestick_conviction: Decimal,
    order_book_pressure: Decimal,
    graph_pressure: Decimal,
    fresh_symbol_confirmation: bool,
    directional_conflict_present: bool,
}

const MAX_SYMBOL_HYPOTHESES_PER_SCOPE: usize = 3;
const CONVERGENCE_HYPOTHESIS_KEY: &str = "convergence_hypothesis";
const CONVERGENCE_HYPOTHESIS_LABEL: &str = "Convergence Hypothesis";

fn shared_template_priority(template_key: &str) -> i32 {
    match template_key {
        CONVERGENCE_HYPOTHESIS_KEY => 118,
        "flow" => 120,
        "liquidity" => 115,
        "breakout_contagion" => 110,
        "catalyst_repricing" => 108,
        "institution_reversal" => 104,
        "propagation" => 100,
        "shared_holder_spillover" | "institution_relay" | "sector_symbol_spillover" => 96,
        "sector_rotation_spillover" => 92,
        "stress_feedback_loop" | "stress_concentration" | "risk" => 88,
        "cross_mechanism_chain" => 84,
        _ => 80,
    }
}

fn shared_hypothesis_sort_key(hypothesis: &Hypothesis) -> (i32, Decimal, Decimal, Decimal, String) {
    (
        shared_template_priority(&hypothesis.family_key),
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
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
    family_gate: Option<&FamilyAlphaGate>,
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
        let mut scope_hypotheses = Vec::new();
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
        let templates = hypothesis_templates(
            &relevant_events,
            &relevant_signals,
            &relevant_paths,
            family_gate,
            &crate::pipeline::reasoning::AbsenceMemory::default(),
            None,
            &scope,
        );
        if let Some(hypothesis) = derive_convergence_hypothesis(
            &scope,
            events.timestamp,
            &relevant_events,
            &relevant_signals,
            &relevant_paths,
        ) {
            scope_hypotheses.push(hypothesis);
        }
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

            scope_hypotheses.push(Hypothesis {
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

        scope_hypotheses.sort_by(|left, right| {
            shared_hypothesis_sort_key(right).cmp(&shared_hypothesis_sort_key(left))
        });
        if matches!(scope, ReasoningScope::Symbol(_)) {
            scope_hypotheses.truncate(MAX_SYMBOL_HYPOTHESES_PER_SCOPE);
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
    observed_at: time::OffsetDateTime,
    relevant_events: &[&crate::ontology::domain::Event<
        crate::pipeline::signals::MarketEventRecord,
    >],
    relevant_signals: &[&crate::ontology::domain::DerivedSignal<
        crate::pipeline::signals::DerivedSignalRecord,
    >],
    relevant_paths: &[&PropagationPath],
) -> Option<Hypothesis> {
    let signature = derive_vortex_signature(
        scope,
        observed_at,
        relevant_events,
        relevant_signals,
        relevant_paths,
    )?;
    if signature.channel_diversity < 3 || signature.strength <= Decimal::new(4, 1) {
        return None;
    }

    let evidence_summary = summarize_evidence_weights(&signature.evidence);
    let confidence = signature.strength * Decimal::new(6, 1)
        + signature.coherence * Decimal::new(2, 1)
        + competing_hypothesis_confidence(&signature.evidence) * Decimal::new(2, 1);
    let hypothesis_id = format!("hyp:{}:{}", scope_id(scope), CONVERGENCE_HYPOTHESIS_KEY);
    let channel_summary = human_join(&signature.dominant_channels);

    Some(Hypothesis {
        hypothesis_id: hypothesis_id.clone(),
        family_key: CONVERGENCE_HYPOTHESIS_KEY.into(),
        family_label: CONVERGENCE_HYPOTHESIS_LABEL.into(),
        provenance: hypothesis_provenance(
            observed_at,
            &hypothesis_id,
            CONVERGENCE_HYPOTHESIS_LABEL,
            &signature.evidence,
            &signature.path_ids,
        )
        .with_note(format!(
            "family={}; vortex_strength={}; channel_diversity={}; coherence={}",
            CONVERGENCE_HYPOTHESIS_LABEL,
            signature.strength.round_dp(4),
            signature.channel_diversity,
            signature.coherence.round_dp(4),
        )),
        scope: scope.clone(),
        statement: format!(
            "{} shows an emergent convergence vortex across {}",
            scope_title(scope),
            channel_summary,
        ),
        confidence: confidence.clamp(Decimal::ZERO, Decimal::ONE).round_dp(4),
        local_support_weight: evidence_summary.local_support,
        local_contradict_weight: evidence_summary.local_contradict,
        propagated_support_weight: evidence_summary.propagated_support,
        propagated_contradict_weight: evidence_summary.propagated_contradict,
        evidence: signature.evidence,
        invalidation_conditions: vec![crate::ontology::reasoning::InvalidationCondition {
            description:
                "channel diversity falls below 3 or contradicting structure overtakes the vortex"
                    .into(),
            references: vec![],
        }],
        propagation_path_ids: signature.path_ids,
        expected_observations: vec![
            "independent channels should keep reinforcing the same scope".into(),
            "propagation topology should continue feeding the same center".into(),
            "vortex strength should stay above 0.40".into(),
        ],
    })
}

fn derive_vortex_signature(
    scope: &ReasoningScope,
    observed_at: time::OffsetDateTime,
    relevant_events: &[&crate::ontology::domain::Event<
        crate::pipeline::signals::MarketEventRecord,
    >],
    relevant_signals: &[&crate::ontology::domain::DerivedSignal<
        crate::pipeline::signals::DerivedSignalRecord,
    >],
    relevant_paths: &[&PropagationPath],
) -> Option<VortexSignature> {
    let mut channels: HashMap<String, VortexChannelContribution> = HashMap::new();
    let mut path_ids = Vec::new();

    for event in relevant_events {
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

    for signal in relevant_signals {
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
        let weight = (path.confidence * hop_penalty(path.steps.len()))
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
                statement: format!(
                    "{} via {}",
                    path.summary,
                    channel_display(vortex_path_channel(path))
                ),
                kind: ReasoningEvidenceKind::PropagatedPath,
                polarity: EvidencePolarity::Supports,
                weight,
                references,
                provenance: derived_provenance(observed_at, weight, &[path.path_id.clone()]),
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
    let coherence = support_total.max(contradict_total) / total;
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

    let _ = scope;
    path_ids.sort();
    path_ids.dedup();

    Some(VortexSignature {
        evidence,
        path_ids,
        dominant_channels,
        channel_diversity,
        strength,
        coherence: coherence.round_dp(4),
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

fn vortex_event_channel(kind: &MarketEventKind) -> Option<(&'static str, EvidencePolarity)> {
    match kind {
        MarketEventKind::SmartMoneyPressure
        | MarketEventKind::InstitutionalFlip
        | MarketEventKind::IcebergDetected
        | MarketEventKind::BrokerClusterFormation
        | MarketEventKind::BrokerSideFlip => Some(("broker_flow", EvidencePolarity::Supports)),
        MarketEventKind::OrderBookDislocation => Some(("order_book", EvidencePolarity::Supports)),
        MarketEventKind::VolumeDislocation => Some(("volume", EvidencePolarity::Supports)),
        MarketEventKind::CandlestickBreakout | MarketEventKind::CompositeAcceleration => {
            Some(("price_action", EvidencePolarity::Supports))
        }
        MarketEventKind::SharedHolderAnomaly => {
            Some(("ownership_network", EvidencePolarity::Supports))
        }
        MarketEventKind::CatalystActivation => Some(("catalyst", EvidencePolarity::Supports)),
        MarketEventKind::MarketStressElevated | MarketEventKind::StressRegimeShift => {
            Some(("stress", EvidencePolarity::Supports))
        }
        MarketEventKind::PropagationAbsence => Some(("propagation", EvidencePolarity::Contradicts)),
        MarketEventKind::ManualReviewRequired => {
            Some(("risk_control", EvidencePolarity::Contradicts))
        }
    }
}

fn vortex_signal_channel(kind: &DerivedSignalKind) -> Option<(&'static str, EvidencePolarity)> {
    match kind {
        DerivedSignalKind::StructuralComposite | DerivedSignalKind::Convergence => {
            Some(("structure", EvidencePolarity::Supports))
        }
        DerivedSignalKind::ValuationSupport => Some(("valuation", EvidencePolarity::Supports)),
        DerivedSignalKind::ActivityMomentum | DerivedSignalKind::CandlestickConviction => {
            Some(("price_action", EvidencePolarity::Supports))
        }
        DerivedSignalKind::SmartMoneyPressure => Some(("broker_flow", EvidencePolarity::Supports)),
        DerivedSignalKind::MarketStress => Some(("stress", EvidencePolarity::Supports)),
    }
}

fn vortex_path_channel(path: &PropagationPath) -> &'static str {
    let mut families = path
        .steps
        .iter()
        .map(|step| mechanism_family(&step.mechanism))
        .collect::<Vec<_>>();
    families.sort();
    families.dedup();
    if families.len() > 1 {
        return "cross_mechanism";
    }

    match families.first().copied().unwrap_or("propagation") {
        "shared_holder" => "ownership_network",
        "institution_affinity" | "institution_diffusion" => "broker_flow",
        "rotation" | "sector_symbol_bridge" => "sector_rotation",
        "market_stress" => "stress",
        _ => "propagation",
    }
}

fn channel_display(channel: &str) -> &str {
    match channel {
        "broker_flow" => "broker flow",
        "order_book" => "order book",
        "volume" => "volume",
        "price_action" => "price action",
        "ownership_network" => "ownership network",
        "catalyst" => "catalyst",
        "stress" => "stress",
        "structure" => "structure",
        "valuation" => "valuation",
        "propagation" => "propagation",
        "sector_rotation" => "sector rotation",
        "cross_mechanism" => "cross-mechanism relay",
        "risk_control" => "risk control",
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
            review_reason_code: None,
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
    support_context: SetupSupportContext<'_>,
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
            let support_contract =
                order_support_contract(&suggestion.symbol, suggestion.direction, &support_context);

            let causal_narrative = linked_hypothesis
                .as_ref()
                .map(|(_, _, _, _, family_label)| {
                    let hyp = hypotheses
                        .iter()
                        .find(|h| h.scope == scope && h.family_label == *family_label);
                    match hyp {
                        Some(h) => build_causal_narrative(&scope, family_label, &h.evidence),
                        None => format!(
                            "{} shows structural convergence warranting {} investigation",
                            scope_title(&scope),
                            family_label,
                        ),
                    }
                });

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
                convergence_detail: support_context
                    .convergence_components
                    .get(&suggestion.symbol)
                    .map(crate::pipeline::reasoning::ConvergenceDetail::from_convergence_score),
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
                causal_narrative,
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
                    format!("symbol_event_count={}", support_contract.symbol_event_count),
                    format!(
                        "directional_support={}",
                        support_contract.directional_support.round_dp(4)
                    ),
                    format!(
                        "directional_conflict={}",
                        support_contract.directional_conflict.round_dp(4)
                    ),
                    format!(
                        "fresh_symbol_confirmation={}",
                        support_contract.fresh_symbol_confirmation
                    ),
                    format!(
                        "directional_conflict_present={}",
                        support_contract.directional_conflict_present
                    ),
                    format!(
                        "capital_flow_direction={}",
                        support_contract.flow_direction.round_dp(4)
                    ),
                    format!(
                        "activity_momentum={}",
                        support_contract.activity_momentum.round_dp(4)
                    ),
                    format!(
                        "candlestick_conviction={}",
                        support_contract.candlestick_conviction.round_dp(4)
                    ),
                    format!(
                        "order_book_pressure={}",
                        support_contract.order_book_pressure.round_dp(4)
                    ),
                    format!(
                        "graph_pressure={}",
                        support_contract.graph_pressure.round_dp(4)
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
                review_reason_code: None,
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
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: selection.rationale.clone(),
            causal_narrative: Some(build_causal_narrative(
                &selection.scope,
                &selection.family_label,
                &top.evidence,
            )),
            risk_notes: vec![
                format!("family={}", selection.family_label),
                "scope-level case; requires operator judgement".into(),
                format!("local_support={}", top.local_support_weight.round_dp(4)),
            ],
            review_reason_code: None,
            policy_verdict: None,
        });
    }

    setups
}

fn order_support_contract(
    symbol: &crate::ontology::objects::Symbol,
    direction: OrderDirection,
    support_context: &SetupSupportContext<'_>,
) -> OrderSupportContract {
    let dims = support_context
        .symbol_dimensions
        .and_then(|items| items.get(symbol));
    let flow_direction = dims
        .map(|item| item.capital_flow_direction)
        .unwrap_or(Decimal::ZERO);
    let activity_momentum = dims
        .map(|item| item.activity_momentum)
        .unwrap_or(Decimal::ZERO);
    let candlestick_conviction = dims
        .map(|item| item.candlestick_conviction)
        .unwrap_or(Decimal::ZERO);
    let order_book_pressure = dims
        .map(|item| item.order_book_pressure)
        .unwrap_or(Decimal::ZERO);
    let graph_pressure = support_context
        .insights
        .pressures
        .iter()
        .find(|item| item.symbol == *symbol)
        .map(|item| item.net_pressure)
        .unwrap_or(Decimal::ZERO);
    let symbol_event_count = support_context
        .events
        .events
        .iter()
        .filter(|event| {
            matches!(&event.value.scope, crate::pipeline::signals::SignalScope::Symbol(event_symbol) if event_symbol == symbol)
                && counts_as_symbol_confirmation_event(&event.value.kind)
        })
        .count();
    let direction_sign = match direction {
        OrderDirection::Buy => Decimal::ONE,
        OrderDirection::Sell => -Decimal::ONE,
    };
    let mut directional_support = Decimal::ZERO;
    let mut directional_conflict = Decimal::ZERO;

    for (value, threshold) in [
        (flow_direction, flow_confirmation_threshold()),
        (activity_momentum, momentum_confirmation_threshold()),
        (candlestick_conviction, candlestick_confirmation_threshold()),
        (order_book_pressure, order_book_confirmation_threshold()),
        (graph_pressure, graph_pressure_confirmation_threshold()),
    ] {
        let signed_value = value * direction_sign;
        if signed_value >= threshold {
            directional_support += value.abs();
        } else if signed_value <= -threshold {
            directional_conflict += value.abs();
        }
    }

    let fresh_symbol_confirmation =
        symbol_event_count > 0 || directional_support >= directional_support_trigger();
    let directional_conflict_present = directional_conflict >= directional_conflict_trigger()
        && directional_conflict > directional_support;

    OrderSupportContract {
        symbol_event_count,
        directional_support,
        directional_conflict,
        flow_direction,
        activity_momentum,
        candlestick_conviction,
        order_book_pressure,
        graph_pressure,
        fresh_symbol_confirmation,
        directional_conflict_present,
    }
}

fn counts_as_symbol_confirmation_event(kind: &crate::pipeline::signals::MarketEventKind) -> bool {
    !matches!(
        kind,
        crate::pipeline::signals::MarketEventKind::ManualReviewRequired
    )
}

fn flow_confirmation_threshold() -> Decimal {
    Decimal::new(3, 2)
}

fn momentum_confirmation_threshold() -> Decimal {
    Decimal::new(4, 2)
}

fn candlestick_confirmation_threshold() -> Decimal {
    Decimal::new(4, 2)
}

fn order_book_confirmation_threshold() -> Decimal {
    Decimal::new(8, 2)
}

fn graph_pressure_confirmation_threshold() -> Decimal {
    Decimal::new(8, 2)
}

fn directional_support_trigger() -> Decimal {
    Decimal::new(5, 2)
}

fn directional_conflict_trigger() -> Decimal {
    Decimal::new(10, 2)
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
