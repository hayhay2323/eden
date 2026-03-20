use std::collections::HashMap;

use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::graph::decision::DecisionSnapshot;
use crate::graph::insights::GraphInsights;
use crate::ontology::world::{
    BackwardCause, BackwardEvidenceItem, BackwardInvestigation, BackwardReasoningSnapshot,
    CausalContestState, EntityState, WorldLayer, WorldStateSnapshot,
};
use crate::ontology::{
    EvidencePolarity, Hypothesis, ProvenanceMetadata, ProvenanceSource, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope, Symbol,
};
use crate::pipeline::reasoning::ReasoningSnapshot;
use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot, SignalScope};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshots {
    pub world_state: WorldStateSnapshot,
    pub backward_reasoning: BackwardReasoningSnapshot,
}

impl WorldSnapshots {
    pub fn derive(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        reasoning: &ReasoningSnapshot,
        polymarket: Option<&PolymarketSnapshot>,
        previous_backward_reasoning: Option<&BackwardReasoningSnapshot>,
    ) -> Self {
        let world_state =
            derive_world_state(events, derived_signals, insights, decision, reasoning, polymarket);
        let backward_reasoning =
            derive_backward_reasoning(reasoning, &world_state, previous_backward_reasoning);
        Self {
            world_state,
            backward_reasoning,
        }
    }
}

fn derive_world_state(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    insights: &GraphInsights,
    decision: &DecisionSnapshot,
    reasoning: &ReasoningSnapshot,
    polymarket: Option<&PolymarketSnapshot>,
) -> WorldStateSnapshot {
    let mut entities = Vec::new();
    let mut sectors: HashMap<String, Vec<&crate::ontology::Hypothesis>> = HashMap::new();
    let polymarket_priors = polymarket
        .map(|snapshot| {
            snapshot
                .priors
                .iter()
                .filter(|prior| prior.active && !prior.closed && prior.is_material())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let market_driver = strongest_market_prior(&polymarket_priors);
    let market_regime = external_market_regime(&polymarket_priors, !reasoning.case_clusters.is_empty(), insights.stress.composite_stress);

    for hypothesis in &reasoning.hypotheses {
        if let ReasoningScope::Sector(sector) = &hypothesis.scope {
            sectors.entry(sector.clone()).or_default().push(hypothesis);
        }
    }

    entities.push(EntityState {
        entity_id: "world:market".into(),
        scope: ReasoningScope::Market,
        layer: WorldLayer::Forest,
        provenance: world_provenance(
            reasoning.timestamp,
            "world:market",
            [
                format!(
                    "market_stress:{}",
                    insights.stress.composite_stress.round_dp(4)
                ),
                format!("clusters:{}", reasoning.case_clusters.len()),
                market_driver
                    .as_ref()
                    .map(|prior| format!("polymarket:{}", prior.slug))
                    .unwrap_or_else(|| "polymarket:none".into()),
            ],
            "market canopy",
            insights.stress.composite_stress,
        ),
        label: "Market canopy".into(),
        regime: market_regime,
        confidence: insights.stress.composite_stress,
        local_support: derived_signals
            .signals
            .iter()
            .filter(|signal| matches!(signal.value.scope, SignalScope::Market))
            .map(|signal| signal.value.strength.abs())
            .sum::<Decimal>()
            .min(Decimal::ONE),
        propagated_support: reasoning
            .propagation_paths
            .iter()
            .filter(|path| path.steps.len() > 1)
            .map(|path| path.confidence)
            .max()
            .unwrap_or(Decimal::ZERO),
        drivers: vec![
            format!(
                "market stress={}",
                insights.stress.composite_stress.round_dp(3)
            ),
            format!("clusters={}", reasoning.case_clusters.len()),
            market_driver
                .map(|prior| prior.driver_text())
                .unwrap_or_else(|| "polymarket=none".into()),
        ],
    });

    for (sector, hypotheses) in sectors {
        let top = hypotheses
            .iter()
            .max_by(|a, b| a.confidence.cmp(&b.confidence))
            .copied()
            .expect("sector hypothesis");
        entities.push(EntityState {
            entity_id: format!("world:sector:{}", sector),
            scope: ReasoningScope::Sector(sector.clone()),
            layer: WorldLayer::Trunk,
            provenance: top
                .provenance
                .clone()
                .with_trace_id(format!("world:sector:{}", sector))
                .with_note("sector trunk"),
            label: format!("Sector {}", sector),
            regime: top.statement.clone(),
            confidence: top.confidence,
            local_support: top.local_support_weight,
            propagated_support: top.propagated_support_weight,
            drivers: top.expected_observations.clone(),
        });
    }

    for setup in reasoning
        .tactical_setups
        .iter()
        .filter(|setup| matches!(setup.scope, ReasoningScope::Symbol(_)))
        .take(12)
    {
        if let Some(hypothesis) = reasoning
            .hypotheses
            .iter()
            .find(|hypothesis| hypothesis.hypothesis_id == setup.hypothesis_id)
        {
            entities.push(EntityState {
                entity_id: format!("world:{}", setup.setup_id),
                scope: setup.scope.clone(),
                layer: WorldLayer::Leaf,
                provenance: setup
                    .provenance
                    .clone()
                    .with_trace_id(format!("world:{}", setup.setup_id))
                    .with_note("leaf case"),
                label: setup.title.clone(),
                regime: setup.action.clone(),
                confidence: setup.confidence,
                local_support: hypothesis.local_support_weight,
                propagated_support: hypothesis.propagated_support_weight,
                drivers: hypothesis
                    .evidence
                    .iter()
                    .take(2)
                    .map(|item| item.statement.clone())
                    .collect(),
            });
        }
    }

    for rotation in insights.rotations.iter().take(6) {
        entities.push(EntityState {
            entity_id: format!(
                "world:rotation:{}:{}",
                rotation.from_sector, rotation.to_sector
            ),
            scope: ReasoningScope::Custom(format!(
                "{}->{}",
                rotation.from_sector, rotation.to_sector
            )),
            layer: WorldLayer::Branch,
            provenance: world_provenance(
                reasoning.timestamp,
                &format!(
                    "world:rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                ),
                [format!(
                    "rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                )],
                "rotation branch",
                rotation.spread.abs().min(Decimal::ONE),
            ),
            label: format!(
                "Rotation {} -> {}",
                rotation.from_sector, rotation.to_sector
            ),
            regime: if rotation.widening {
                "widening-rotation".into()
            } else {
                "narrowing-rotation".into()
            },
            confidence: rotation.spread.abs().min(Decimal::ONE),
            local_support: Decimal::ZERO,
            propagated_support: rotation.spread.abs().min(Decimal::ONE),
            drivers: vec![format!(
                "spread_delta={}",
                rotation.spread_delta.round_dp(3)
            )],
        });
    }

    append_polymarket_entities(&mut entities, reasoning.timestamp, &polymarket_priors);

    entities.sort_by(|a, b| {
        world_layer_priority(a.layer)
            .cmp(&world_layer_priority(b.layer))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.entity_id.cmp(&b.entity_id))
    });

    let _ = (events, decision); // reserved for richer world-state updates
    WorldStateSnapshot {
        timestamp: reasoning.timestamp,
        entities,
    }
}

fn strongest_market_prior(priors: &[PolymarketPrior]) -> Option<&PolymarketPrior> {
    priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market))
        .max_by(|a, b| a.probability.cmp(&b.probability))
}

fn external_market_regime(
    priors: &[PolymarketPrior],
    has_clusters: bool,
    market_stress: Decimal,
) -> String {
    let default_regime = if market_stress >= Decimal::new(45, 2) {
        "stress-dominant"
    } else if has_clusters {
        "narrative-fragmenting"
    } else {
        "locally-driven"
    };

    let strongest_risk_off = priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market))
        .filter(|prior| prior.bias == PolymarketBias::RiskOff)
        .max_by(|a, b| a.probability.cmp(&b.probability));
    if let Some(prior) = strongest_risk_off {
        if prior.probability >= Decimal::new(65, 2) {
            return format!("event-risk-off ({})", prior.label);
        }
    }

    let strongest_risk_on = priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market))
        .filter(|prior| prior.bias == PolymarketBias::RiskOn)
        .max_by(|a, b| a.probability.cmp(&b.probability));
    if let Some(prior) = strongest_risk_on {
        if prior.probability >= Decimal::new(65, 2) {
            return format!("event-risk-on ({})", prior.label);
        }
    }

    default_regime.into()
}

fn append_polymarket_entities(
    entities: &mut Vec<EntityState>,
    timestamp: time::OffsetDateTime,
    priors: &[PolymarketPrior],
) {
    for prior in priors.iter().take(6) {
        let provenance = ProvenanceMetadata::new(
            ProvenanceSource::External("polymarket".into()),
            timestamp,
        )
        .with_trace_id(format!("world:polymarket:{}", prior.slug))
        .with_inputs([
            format!("polymarket:{}", prior.slug),
            format!("outcome:{}", prior.selected_outcome),
        ])
        .with_note("external event prior");
        entities.push(EntityState {
            entity_id: format!("world:polymarket:{}", prior.slug),
            scope: prior.scope.clone(),
            layer: polymarket_layer(&prior.scope),
            provenance: provenance.clone(),
            label: format!("Polymarket {}", prior.label),
            regime: format!("{} {}", prior.bias.as_str(), prior.selected_outcome),
            confidence: prior.probability,
            local_support: Decimal::ZERO,
            propagated_support: prior.probability,
            drivers: vec![
                prior.question.clone(),
                format!(
                    "probability={:.0}%",
                    (prior.probability * Decimal::new(100, 0)).round_dp(0)
                ),
                prior
                    .category
                    .clone()
                    .map(|category| format!("category={}", category))
                    .unwrap_or_else(|| "category=unknown".into()),
            ],
        });

        for target_scope in prior.parsed_target_scopes() {
            if target_scope == prior.scope {
                continue;
            }
            entities.push(EntityState {
                entity_id: format!(
                    "world:polymarket:{}:{}",
                    prior.slug,
                    scope_key(&target_scope)
                ),
                scope: target_scope.clone(),
                layer: polymarket_layer(&target_scope),
                provenance: provenance
                    .clone()
                    .with_trace_id(format!(
                        "world:polymarket:{}:{}",
                        prior.slug,
                        scope_key(&target_scope)
                    ))
                    .with_note("external event target"),
                label: format!("Polymarket target {}", prior.label),
                regime: format!("{} {}", prior.bias.as_str(), prior.selected_outcome),
                confidence: prior.probability,
                local_support: Decimal::ZERO,
                propagated_support: prior.probability,
                drivers: vec![
                    format!("source={}", prior.slug),
                    format!("target={}", scope_key(&target_scope)),
                    prior.question.clone(),
                ],
            });
        }
    }
}

fn polymarket_layer(scope: &ReasoningScope) -> WorldLayer {
    match scope {
        ReasoningScope::Market => WorldLayer::Branch,
        ReasoningScope::Sector(_) | ReasoningScope::Region(_) => WorldLayer::Trunk,
        ReasoningScope::Theme(_) | ReasoningScope::Custom(_) | ReasoningScope::Institution(_) => {
            WorldLayer::Branch
        }
        ReasoningScope::Symbol(_) => WorldLayer::Leaf,
    }
}

fn derive_backward_reasoning(
    reasoning: &ReasoningSnapshot,
    world_state: &WorldStateSnapshot,
    previous_backward_reasoning: Option<&BackwardReasoningSnapshot>,
) -> BackwardReasoningSnapshot {
    let world_map = world_state
        .entities
        .iter()
        .map(|entity| (scope_key(&entity.scope), entity))
        .collect::<HashMap<_, _>>();
    let hypothesis_map = reasoning
        .hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<HashMap<_, _>>();
    let previous_investigation_map = previous_backward_reasoning
        .map(|snapshot| {
            snapshot
                .investigations
                .iter()
                .map(|investigation| (scope_key(&investigation.leaf_scope), investigation))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let investigations = reasoning
        .tactical_setups
        .iter()
        .filter(|setup| setup.action == "enter" || setup.action == "review")
        .take(6)
        .filter_map(|setup| {
            let leaf_scope_id = scope_key(&setup.scope);
            let hypothesis = hypothesis_map.get(setup.hypothesis_id.as_str()).copied()?;
            let previous_investigation = previous_investigation_map.get(leaf_scope_id.as_str()).copied();
            let relevant_paths = reasoning
                .propagation_paths
                .iter()
                .filter(|path| path.steps.iter().any(|step| step.to == setup.scope))
                .collect::<Vec<_>>();
            let mut candidate_causes = Vec::new();
            let default_local_falsifier = hypothesis
                .invalidation_conditions
                .first()
                .map(|item| item.description.clone())
                .or_else(|| Some(format!("local evidence stops supporting {}", setup.title)));

            for evidence in hypothesis.evidence.iter().take(3) {
                let layer = match evidence.kind {
                    crate::ontology::ReasoningEvidenceKind::PropagatedPath => WorldLayer::Branch,
                    _ => WorldLayer::Leaf,
                };
                let mut cause = BackwardCause {
                    cause_id: format!("cause:local:{}:{}", leaf_scope_id, local_cause_key(evidence)),
                    scope: setup.scope.clone(),
                    layer,
                    depth: 0,
                    provenance: evidence
                        .provenance
                        .clone()
                        .with_trace_id(format!(
                            "cause:local:{}:{}",
                            leaf_scope_id,
                            local_cause_key(evidence)
                        ))
                        .with_note("backward local cause"),
                    explanation: evidence.statement.clone(),
                    chain_summary: None,
                    confidence: evidence.weight,
                    support_weight: Decimal::ZERO,
                    contradict_weight: Decimal::ZERO,
                    net_conviction: Decimal::ZERO,
                    competitive_score: Decimal::ZERO,
                    falsifier: default_local_falsifier.clone(),
                    supporting_evidence: Vec::new(),
                    contradicting_evidence: Vec::new(),
                    references: evidence.references.clone(),
                };
                let mut supporting_evidence =
                    local_evidence_items(hypothesis, EvidencePolarity::Supports);
                if supporting_evidence.is_empty() {
                    supporting_evidence.push(backward_evidence_item(
                        cause.explanation.clone(),
                        cause.confidence,
                        "local-anchor",
                    ));
                }
                let mut contradicting_evidence =
                    local_evidence_items(hypothesis, EvidencePolarity::Contradicts);
                if hypothesis.propagated_support_weight > hypothesis.local_support_weight {
                    contradicting_evidence.push(backward_evidence_item(
                        format!("spillover evidence currently outweighs local tape for {}", setup.title),
                        hypothesis.propagated_support_weight - hypothesis.local_support_weight,
                        "propagated-counter",
                    ));
                }
                attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                candidate_causes.push(cause);
            }

            for path in relevant_paths.iter().take(3) {
                if let Some(origin) = path.steps.first() {
                    let mut cause = BackwardCause {
                        cause_id: format!("cause:path:{}:{}", leaf_scope_id, path.path_id),
                        scope: origin.from.clone(),
                        layer: if matches!(origin.from, ReasoningScope::Market) {
                            WorldLayer::Forest
                        } else {
                            WorldLayer::Branch
                        },
                        depth: path.steps.len() as u8,
                        provenance: world_provenance(
                            reasoning.timestamp,
                            &format!("cause:path:{}:{}", leaf_scope_id, path.path_id),
                            [path.path_id.clone()],
                            "backward path cause",
                            path.confidence,
                        ),
                        explanation: path.summary.clone(),
                        chain_summary: Some(render_path_chain(path)),
                        confidence: path.confidence,
                        support_weight: Decimal::ZERO,
                        contradict_weight: Decimal::ZERO,
                        net_conviction: Decimal::ZERO,
                        competitive_score: Decimal::ZERO,
                        falsifier: Some(format!(
                            "propagation path {} no longer reaches {}",
                            path.path_id, setup.title
                        )),
                        supporting_evidence: Vec::new(),
                        contradicting_evidence: Vec::new(),
                        references: vec![path.path_id.clone()],
                    };
                    let mut supporting_evidence = propagated_evidence_items(
                        hypothesis,
                        EvidencePolarity::Supports,
                        Some(path.path_id.as_str()),
                    );
                    if supporting_evidence.is_empty() {
                        supporting_evidence.push(backward_evidence_item(
                            path.summary.clone(),
                            path.confidence,
                            "path-anchor",
                        ));
                    }
                    let mut contradicting_evidence = propagated_evidence_items(
                        hypothesis,
                        EvidencePolarity::Contradicts,
                        Some(path.path_id.as_str()),
                    );
                    if hypothesis.local_support_weight > hypothesis.propagated_support_weight {
                        contradicting_evidence.push(backward_evidence_item(
                            format!("local tape currently outweighs spillover route {}", path.path_id),
                            hypothesis.local_support_weight - hypothesis.propagated_support_weight,
                            "local-counter",
                        ));
                    }
                    attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                    candidate_causes.push(cause);
                }
            }

            if let Some(market_state) = world_map.get("market") {
                let mut cause = BackwardCause {
                    cause_id: format!("cause:market:{}", leaf_scope_id),
                    scope: market_state.scope.clone(),
                    layer: market_state.layer,
                    depth: 2,
                    provenance: market_state
                        .provenance
                        .clone()
                        .with_trace_id(format!("cause:market:{}", leaf_scope_id))
                        .with_note("backward market cause"),
                    explanation: format!(
                        "market regime {} may be shaping this leaf",
                        market_state.regime
                    ),
                    chain_summary: Some(format!("{} -> {}", setup.title, market_state.label)),
                    confidence: market_state.confidence,
                    support_weight: Decimal::ZERO,
                    contradict_weight: Decimal::ZERO,
                    net_conviction: Decimal::ZERO,
                    competitive_score: Decimal::ZERO,
                    falsifier: Some(format!(
                        "market regime {} stops dominating {}",
                        market_state.regime, setup.title
                    )),
                    supporting_evidence: Vec::new(),
                    contradicting_evidence: Vec::new(),
                    references: market_state.drivers.clone(),
                };
                let market_path_ids = relevant_paths
                    .iter()
                    .filter(|path| {
                        path.steps
                            .first()
                            .map(|step| matches!(step.from, ReasoningScope::Market))
                            .unwrap_or(false)
                    })
                    .map(|path| path.path_id.as_str())
                    .collect::<Vec<_>>();
                let driver_weight = if market_state.drivers.is_empty() {
                    Decimal::ZERO
                } else {
                    market_state.confidence / Decimal::from(market_state.drivers.len() as i64)
                };
                let mut supporting_evidence = market_state
                    .drivers
                    .iter()
                    .map(|driver| backward_evidence_item(driver.clone(), driver_weight, "market-driver"))
                    .collect::<Vec<_>>();
                for path_id in &market_path_ids {
                    supporting_evidence.extend(propagated_evidence_items(
                        hypothesis,
                        EvidencePolarity::Supports,
                        Some(path_id),
                    ));
                }
                if supporting_evidence.is_empty() {
                    supporting_evidence.push(backward_evidence_item(
                        cause.explanation.clone(),
                        cause.confidence,
                        "market-anchor",
                    ));
                }
                let mut contradicting_evidence = market_path_ids
                    .iter()
                    .flat_map(|path_id| {
                        propagated_evidence_items(
                            hypothesis,
                            EvidencePolarity::Contradicts,
                            Some(path_id),
                        )
                    })
                    .collect::<Vec<_>>();
                if hypothesis.local_support_weight > hypothesis.propagated_support_weight {
                    contradicting_evidence.push(backward_evidence_item(
                        format!(
                            "local evidence currently dominates over market spillover for {}",
                            setup.title
                        ),
                        hypothesis.local_support_weight - hypothesis.propagated_support_weight,
                        "local-counter",
                    ));
                }
                attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                candidate_causes.push(cause);
            }

            if let ReasoningScope::Symbol(symbol) = &setup.scope {
                let sector_key = format!("sector:{}", symbol_to_sector_hint(reasoning, symbol));
                if let Some(sector_state) = world_map.get(sector_key.as_str()) {
                    let mut cause = BackwardCause {
                        cause_id: format!(
                            "cause:sector:{}:{}",
                            leaf_scope_id,
                            scope_key(&sector_state.scope)
                        ),
                        scope: sector_state.scope.clone(),
                        layer: sector_state.layer,
                        depth: 1,
                        provenance: sector_state
                            .provenance
                            .clone()
                            .with_trace_id(format!(
                                "cause:sector:{}:{}",
                                leaf_scope_id,
                                scope_key(&sector_state.scope)
                            ))
                            .with_note("backward sector cause"),
                        explanation: format!(
                            "sector regime {} may be framing this leaf",
                            sector_state.regime
                        ),
                        chain_summary: Some(format!("{} -> {}", setup.title, sector_state.label)),
                        confidence: sector_state.confidence,
                        support_weight: Decimal::ZERO,
                        contradict_weight: Decimal::ZERO,
                        net_conviction: Decimal::ZERO,
                        competitive_score: Decimal::ZERO,
                        falsifier: Some(format!(
                            "sector regime {} no longer frames {}",
                            sector_state.regime, setup.title
                        )),
                        supporting_evidence: Vec::new(),
                        contradicting_evidence: Vec::new(),
                        references: sector_state.drivers.clone(),
                    };
                    let sector_path_ids = relevant_paths
                        .iter()
                        .filter(|path| {
                            path.steps.iter().any(|step| step.from == sector_state.scope)
                                || path
                                    .steps
                                    .first()
                                    .map(|step| step.from == sector_state.scope)
                                    .unwrap_or(false)
                        })
                        .map(|path| path.path_id.as_str())
                        .collect::<Vec<_>>();
                    let driver_weight = if sector_state.drivers.is_empty() {
                        Decimal::ZERO
                    } else {
                        sector_state.confidence / Decimal::from(sector_state.drivers.len() as i64)
                    };
                    let mut supporting_evidence = sector_state
                        .drivers
                        .iter()
                        .map(|driver| backward_evidence_item(driver.clone(), driver_weight, "sector-driver"))
                        .collect::<Vec<_>>();
                    for path_id in &sector_path_ids {
                        supporting_evidence.extend(propagated_evidence_items(
                            hypothesis,
                            EvidencePolarity::Supports,
                            Some(path_id),
                        ));
                    }
                    if supporting_evidence.is_empty() {
                        supporting_evidence.push(backward_evidence_item(
                            cause.explanation.clone(),
                            cause.confidence,
                            "sector-anchor",
                        ));
                    }
                    let mut contradicting_evidence = sector_path_ids
                        .iter()
                        .flat_map(|path_id| {
                            propagated_evidence_items(
                                hypothesis,
                                EvidencePolarity::Contradicts,
                                Some(path_id),
                            )
                        })
                        .collect::<Vec<_>>();
                    if hypothesis.local_support_weight > hypothesis.propagated_support_weight {
                        contradicting_evidence.push(backward_evidence_item(
                            format!(
                                "idiosyncratic local tape currently outweighs sector framing for {}",
                                setup.title
                            ),
                            hypothesis.local_support_weight - hypothesis.propagated_support_weight,
                            "local-counter",
                        ));
                    }
                    attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                    candidate_causes.push(cause);
                }
            }

            candidate_causes.sort_by(|a, b| {
                b.competitive_score
                    .cmp(&a.competitive_score)
                    .then_with(|| b.confidence.cmp(&a.confidence))
                    .then_with(|| {
                        backward_layer_priority(a.layer).cmp(&backward_layer_priority(b.layer))
                    })
                    .then_with(|| a.cause_id.cmp(&b.cause_id))
            });
            let leading_cause = candidate_causes.first().cloned();
            let runner_up_cause = candidate_causes.get(1).cloned();
            let cause_gap = match (&leading_cause, &runner_up_cause) {
                (Some(leading), Some(runner_up)) => Some(backward_cause_gap(leading, runner_up)),
                _ => None,
            };
            let previous_leading_cause_id = previous_investigation
                .and_then(|item| item.leading_cause.as_ref().map(|cause| cause.cause_id.clone()));
            let leading_cause_streak =
                leading_cause_streak(previous_investigation, leading_cause.as_ref());
            let (leading_support_delta, leading_contradict_delta) =
                leading_cause_deltas(previous_investigation, leading_cause.as_ref());
            let contest_state = classify_causal_contest(
                previous_investigation,
                leading_cause.as_ref(),
                cause_gap,
                leading_support_delta,
                leading_contradict_delta,
            );
            let leader_transition_summary = render_leader_transition(
                previous_investigation,
                leading_cause.as_ref(),
                cause_gap,
                leading_support_delta,
                leading_contradict_delta,
                contest_state,
            );
            let leading_falsifier = leading_cause
                .as_ref()
                .and_then(|cause| cause.falsifier.clone());

            Some(BackwardInvestigation {
                investigation_id: format!("backward:{}", leaf_scope_id),
                leaf_scope: setup.scope.clone(),
                leaf_label: setup.title.clone(),
                leaf_regime: setup.action.clone(),
                contest_state,
                leading_cause_streak,
                previous_leading_cause_id,
                leading_cause,
                runner_up_cause,
                cause_gap,
                leading_support_delta,
                leading_contradict_delta,
                leader_transition_summary,
                leading_falsifier,
                candidate_causes,
            })
        })
        .collect();

    BackwardReasoningSnapshot {
        timestamp: reasoning.timestamp,
        investigations,
    }
}

fn scope_key(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => format!("sector:{}", sector),
        ReasoningScope::Institution(institution) => format!("institution:{}", institution),
        ReasoningScope::Theme(theme) => format!("theme:{}", theme),
        ReasoningScope::Region(region) => format!("region:{}", region),
        ReasoningScope::Custom(value) => format!("custom:{}", value),
    }
}

fn world_provenance<I, S>(
    observed_at: time::OffsetDateTime,
    trace_id: &str,
    inputs: I,
    note: &str,
    confidence: Decimal,
) -> crate::ontology::ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_inputs(inputs)
    .with_confidence(confidence)
    .with_note(note)
}

fn world_layer_priority(layer: WorldLayer) -> i32 {
    match layer {
        WorldLayer::Forest => 0,
        WorldLayer::Trunk => 1,
        WorldLayer::Branch => 2,
        WorldLayer::Leaf => 3,
    }
}

fn backward_layer_priority(layer: WorldLayer) -> i32 {
    match layer {
        WorldLayer::Forest => 0,
        WorldLayer::Trunk => 1,
        WorldLayer::Branch => 2,
        WorldLayer::Leaf => 3,
    }
}

fn backward_evidence_item(
    statement: impl Into<String>,
    weight: Decimal,
    channel: impl Into<String>,
) -> BackwardEvidenceItem {
    BackwardEvidenceItem {
        statement: statement.into(),
        weight: weight.round_dp(4).clamp(Decimal::ZERO, Decimal::ONE),
        channel: channel.into(),
    }
}

fn evidence_items_by_filter<F>(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
    predicate: F,
) -> Vec<BackwardEvidenceItem>
where
    F: Fn(&ReasoningEvidence) -> bool,
{
    hypothesis
        .evidence
        .iter()
        .filter(|evidence| evidence.polarity == polarity && predicate(evidence))
        .map(|evidence| {
            backward_evidence_item(
                evidence.statement.clone(),
                evidence.weight,
                match evidence.kind {
                    ReasoningEvidenceKind::LocalEvent => "local-event",
                    ReasoningEvidenceKind::LocalSignal => "local-signal",
                    ReasoningEvidenceKind::PropagatedPath => "propagated-path",
                },
            )
        })
        .collect()
}

fn local_evidence_items(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
) -> Vec<BackwardEvidenceItem> {
    evidence_items_by_filter(hypothesis, polarity, |evidence| {
        matches!(
            evidence.kind,
            ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal
        )
    })
}

fn propagated_evidence_items(
    hypothesis: &Hypothesis,
    polarity: EvidencePolarity,
    path_id: Option<&str>,
) -> Vec<BackwardEvidenceItem> {
    evidence_items_by_filter(hypothesis, polarity, |evidence| {
        if !matches!(evidence.kind, ReasoningEvidenceKind::PropagatedPath) {
            return false;
        }
        path_id.is_none_or(|id| evidence.references.iter().any(|reference| reference == id))
    })
}

fn attach_contest_metrics(
    cause: &mut BackwardCause,
    supporting_evidence: Vec<BackwardEvidenceItem>,
    contradicting_evidence: Vec<BackwardEvidenceItem>,
) {
    cause.support_weight = supporting_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE)
        .round_dp(4);
    cause.contradict_weight = contradicting_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE)
        .round_dp(4);
    cause.net_conviction = (cause.support_weight - cause.contradict_weight)
        .clamp(-Decimal::ONE, Decimal::ONE)
        .round_dp(4);
    cause.supporting_evidence = supporting_evidence;
    cause.contradicting_evidence = contradicting_evidence;
    cause.competitive_score = backward_cause_score(cause);
}

fn backward_cause_score(cause: &BackwardCause) -> Decimal {
    let layer_bonus = match cause.layer {
        WorldLayer::Forest => Decimal::new(16, 2),
        WorldLayer::Trunk => Decimal::new(12, 2),
        WorldLayer::Branch => Decimal::new(8, 2),
        WorldLayer::Leaf => Decimal::new(4, 2),
    };
    let reference_bonus = Decimal::from(cause.references.len().min(3) as i64) * Decimal::new(2, 2);
    let chain_bonus = if cause.chain_summary.is_some() {
        Decimal::new(3, 2)
    } else {
        Decimal::ZERO
    };
    let depth_penalty = Decimal::from(cause.depth.min(4) as i64) * Decimal::new(3, 2);
    let support_bonus = cause.support_weight * Decimal::new(28, 2);
    let conviction_bonus = cause.net_conviction.max(Decimal::ZERO) * Decimal::new(18, 2);
    let contradiction_penalty = cause.contradict_weight * Decimal::new(32, 2);

    (cause.confidence * Decimal::new(58, 2)
        + support_bonus
        + conviction_bonus
        + layer_bonus
        + reference_bonus
        + chain_bonus
        - contradiction_penalty
        - depth_penalty)
        .clamp(Decimal::ZERO, Decimal::ONE)
        .round_dp(4)
}

fn backward_cause_gap(leading: &BackwardCause, runner_up: &BackwardCause) -> Decimal {
    let score_gap = leading.competitive_score - runner_up.competitive_score;
    let contradiction_swing = runner_up.contradict_weight - leading.contradict_weight;
    let conviction_swing = leading.net_conviction - runner_up.net_conviction;

    (score_gap + contradiction_swing * Decimal::new(35, 2) + conviction_swing * Decimal::new(25, 2))
        .max(Decimal::ZERO)
        .round_dp(4)
}

fn stable_cause_token(value: &str) -> String {
    let token = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let compact = token
        .split('_')
        .filter(|segment| !segment.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("_");

    if compact.is_empty() {
        "unknown".into()
    } else {
        compact
    }
}

fn local_cause_key(evidence: &ReasoningEvidence) -> String {
    evidence
        .references
        .first()
        .map(|value| stable_cause_token(value))
        .unwrap_or_else(|| stable_cause_token(&evidence.statement))
}

fn previous_cause_by_id<'a>(
    previous_investigation: Option<&'a BackwardInvestigation>,
    cause_id: &str,
) -> Option<&'a BackwardCause> {
    previous_investigation.and_then(|investigation| {
        investigation
            .candidate_causes
            .iter()
            .find(|cause| cause.cause_id == cause_id)
    })
}

fn leading_cause_streak(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
) -> u64 {
    match (
        previous_investigation.and_then(|item| item.leading_cause.as_ref()),
        current_leading,
    ) {
        (_, None) => 0,
        (Some(previous), Some(current)) if previous.cause_id == current.cause_id => {
            previous_investigation
                .map(|item| item.leading_cause_streak.saturating_add(1))
                .unwrap_or(1)
        }
        _ => 1,
    }
}

fn leading_cause_deltas(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
) -> (Option<Decimal>, Option<Decimal>) {
    let Some(current) = current_leading else {
        return (None, None);
    };
    let previous = previous_cause_by_id(previous_investigation, current.cause_id.as_str());
    let support_delta =
        previous.map(|cause| (current.support_weight - cause.support_weight).round_dp(4));
    let contradict_delta =
        previous.map(|cause| (current.contradict_weight - cause.contradict_weight).round_dp(4));
    (support_delta, contradict_delta)
}

fn classify_causal_contest(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
    cause_gap: Option<Decimal>,
    leading_support_delta: Option<Decimal>,
    leading_contradict_delta: Option<Decimal>,
) -> CausalContestState {
    let Some(current) = current_leading else {
        return CausalContestState::Contested;
    };
    let strong_gap = Decimal::new(8, 2);
    let narrow_gap = Decimal::new(3, 2);
    let contradiction_threshold = Decimal::new(5, 2);
    let support_softening_threshold = Decimal::new(-2, 2);
    let previous_leading = previous_investigation.and_then(|item| item.leading_cause.as_ref());

    match previous_leading {
        None => CausalContestState::New,
        Some(previous) if previous.cause_id != current.cause_id => {
            if cause_gap.unwrap_or(Decimal::ZERO) >= strong_gap {
                CausalContestState::Flipped
            } else {
                CausalContestState::Contested
            }
        }
        Some(_) if cause_gap.unwrap_or(Decimal::ZERO) < narrow_gap => CausalContestState::Contested,
        Some(_)
            if leading_contradict_delta.unwrap_or(Decimal::ZERO) >= contradiction_threshold
                && leading_support_delta.unwrap_or(Decimal::ZERO)
                    <= support_softening_threshold =>
        {
            CausalContestState::Eroding
        }
        Some(_) => CausalContestState::Stable,
    }
}

fn render_leader_transition(
    previous_investigation: Option<&BackwardInvestigation>,
    current_leading: Option<&BackwardCause>,
    cause_gap: Option<Decimal>,
    leading_support_delta: Option<Decimal>,
    leading_contradict_delta: Option<Decimal>,
    contest_state: CausalContestState,
) -> Option<String> {
    let current = current_leading?;
    let previous = previous_investigation.and_then(|item| item.leading_cause.as_ref());

    match previous {
        None => Some(format!(
            "new causal contest seeded with {} leading",
            current.explanation
        )),
        Some(previous) if previous.cause_id != current.cause_id => Some(format!(
            "leadership flipped from {} to {} (gap={:+})",
            previous.explanation,
            current.explanation,
            cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        )),
        Some(_) if contest_state == CausalContestState::Eroding => Some(format!(
            "leader {} is eroding (d_support={:+} d_against={:+})",
            current.explanation,
            leading_support_delta.unwrap_or(Decimal::ZERO).round_dp(3),
            leading_contradict_delta
                .unwrap_or(Decimal::ZERO)
                .round_dp(3),
        )),
        Some(_) if contest_state == CausalContestState::Contested => Some(format!(
            "leader {} remains contested (gap={:+})",
            current.explanation,
            cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        )),
        Some(_) => Some(format!(
            "leader {} remains stable with streak continuation",
            current.explanation
        )),
    }
}

fn render_path_chain(path: &crate::ontology::PropagationPath) -> String {
    let mut segments = Vec::new();
    if let Some(first) = path.steps.first() {
        segments.push(scope_key(&first.from));
    }
    for step in &path.steps {
        segments.push(format!("{} via {}", scope_key(&step.to), step.mechanism));
    }
    segments.join(" -> ")
}

fn symbol_to_sector_hint(reasoning: &ReasoningSnapshot, symbol: &Symbol) -> String {
    reasoning
        .propagation_paths
        .iter()
        .flat_map(|path| path.steps.iter())
        .find_map(|step| match (&step.from, &step.to) {
            (ReasoningScope::Sector(sector), ReasoningScope::Symbol(step_symbol))
                if step_symbol == symbol =>
            {
                Some(sector.clone())
            }
            (ReasoningScope::Symbol(step_symbol), ReasoningScope::Sector(sector))
                if step_symbol == symbol =>
            {
                Some(sector.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
    use crate::graph::decision::{ConvergenceScore, OrderDirection, OrderSuggestion};
    use crate::graph::insights::{GraphInsights, MarketStressIndex, RotationPair};
    use crate::ontology::{Hypothesis, HypothesisTrack, HypothesisTrackStatus, TacticalSetup};
    use crate::ontology::{ReasoningEvidence, ReasoningEvidenceKind, Symbol};
    use crate::pipeline::reasoning::ReasoningSnapshot;
    use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot};

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn prov(trace_id: &str) -> crate::ontology::ProvenanceMetadata {
        crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
        )
        .with_trace_id(trace_id)
        .with_inputs([trace_id.to_string()])
    }

    #[test]
    fn world_state_derives_market_and_leaf_entities() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:700.HK:flow"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.64),
            local_support_weight: dec!(0.4),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.2),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![ReasoningEvidence {
                statement: "local flow still leads".into(),
                kind: ReasoningEvidenceKind::LocalEvent,
                polarity: crate::ontology::EvidencePolarity::Supports,
                weight: dec!(0.4),
                references: vec![],
                provenance: crate::ontology::ProvenanceMetadata::new(
                    crate::ontology::ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            }],
            invalidation_conditions: vec![crate::ontology::InvalidationCondition {
                description: "local flow turns net negative".into(),
                references: vec!["flow:700.HK".into()],
            }],
            propagation_path_ids: vec![],
            expected_observations: vec!["flow should persist".into()],
        };
        let reasoning = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![hypothesis.clone()],
            propagation_paths: vec![],
            tactical_setups: vec![TacticalSetup {
                setup_id: "setup:700.HK:review".into(),
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                provenance: prov("setup:700.HK:review"),
                lineage: crate::ontology::DecisionLineage::default(),
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                action: "review".into(),
                time_horizon: "intraday".into(),
                confidence: dec!(0.64),
                confidence_gap: dec!(0.14),
                heuristic_edge: dec!(0.03),
                workflow_id: Some("order:700.HK:buy".into()),
                entry_rationale: "flow leads".into(),
                risk_notes: vec![],
            }],
            hypothesis_tracks: vec![HypothesisTrack {
                track_id: "track:700.HK".into(),
                setup_id: "setup:700.HK:review".into(),
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                action: "review".into(),
                status: HypothesisTrackStatus::Stable,
                age_ticks: 2,
                status_streak: 1,
                confidence: dec!(0.64),
                previous_confidence: Some(dec!(0.62)),
                confidence_change: dec!(0.02),
                confidence_gap: dec!(0.14),
                previous_confidence_gap: Some(dec!(0.12)),
                confidence_gap_change: dec!(0.02),
                heuristic_edge: dec!(0.03),
                policy_reason: "case remains stable".into(),
                transition_reason: None,
                first_seen_at: OffsetDateTime::UNIX_EPOCH,
                last_updated_at: OffsetDateTime::UNIX_EPOCH,
                invalidated_at: None,
            }],
            case_clusters: vec![],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: crate::ontology::SectorId("tech".into()),
                to_sector: crate::ontology::SectorId("finance".into()),
                spread: dec!(0.5),
                spread_delta: dec!(0.1),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.4),
                pressure_consensus: dec!(0.4),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.6),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
        };

        let snapshots = WorldSnapshots::derive(
            &EventSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                events: vec![],
            },
            &DerivedSignalSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                signals: vec![],
            },
            &insights,
            &DecisionSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                convergence_scores: HashMap::from([(
                    sym("700.HK"),
                    ConvergenceScore {
                        symbol: sym("700.HK"),
                        institutional_alignment: dec!(0.4),
                        sector_coherence: Some(dec!(0.2)),
                        cross_stock_correlation: dec!(0.1),
                        composite: dec!(0.5),
                    },
                )]),
                market_regime: crate::graph::decision::MarketRegimeFilter::neutral(),
                order_suggestions: vec![OrderSuggestion {
                    symbol: sym("700.HK"),
                    direction: OrderDirection::Buy,
                    convergence: ConvergenceScore {
                        symbol: sym("700.HK"),
                        institutional_alignment: dec!(0.4),
                        sector_coherence: Some(dec!(0.2)),
                        cross_stock_correlation: dec!(0.1),
                        composite: dec!(0.5),
                    },
                    suggested_quantity: 100,
                    price_low: None,
                    price_high: None,
                    estimated_cost: dec!(0.01),
                    heuristic_edge: dec!(0.04),
                    requires_confirmation: false,
                    convergence_score: dec!(0.5),
                    effective_confidence: dec!(0.5),
                    external_confirmation: None,
                    external_conflict: None,
                    external_support_slug: None,
                    external_support_probability: None,
                    external_conflict_slug: None,
                    external_conflict_probability: None,
                }],
                degradations: HashMap::new(),
            },
            &reasoning,
            None,
            None,
        );

        assert!(!snapshots.world_state.entities.is_empty());
        assert!(snapshots
            .world_state
            .entities
            .iter()
            .any(|entity| entity.layer == WorldLayer::Forest));
        assert!(snapshots
            .backward_reasoning
            .investigations
            .iter()
            .any(|item| item.leaf_label == "Long 700.HK"));
        let investigation = snapshots
            .backward_reasoning
            .investigations
            .iter()
            .find(|item| item.leaf_label == "Long 700.HK")
            .expect("backward investigation");
        assert!(investigation.leading_cause.is_some());
        assert!(investigation.runner_up_cause.is_some());
        assert!(investigation.cause_gap.is_some());
        assert!(investigation.leading_falsifier.is_some());
        assert!(investigation
            .leading_cause
            .as_ref()
            .is_some_and(|cause| !cause.supporting_evidence.is_empty()));
        assert!(investigation
            .leading_cause
            .as_ref()
            .is_some_and(|cause| cause.support_weight >= cause.contradict_weight));
        assert!(investigation
            .candidate_causes
            .windows(2)
            .all(|pair| pair[0].competitive_score >= pair[1].competitive_score));
    }

    #[test]
    fn world_state_adds_polymarket_entities() {
        let reasoning = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.2),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.2),
                composite_stress: dec!(0.2),
            },
            institution_stock_counts: HashMap::new(),
        };
        let polymarket = PolymarketSnapshot {
            fetched_at: OffsetDateTime::UNIX_EPOCH,
            priors: vec![PolymarketPrior {
                slug: "fed-cut".into(),
                label: "Fed cut in September".into(),
                question: "Will the Fed cut in September?".into(),
                scope: ReasoningScope::Market,
                target_scopes: vec![],
                bias: PolymarketBias::RiskOn,
                selected_outcome: "Yes".into(),
                probability: dec!(0.72),
                conviction_threshold: dec!(0.60),
                active: true,
                closed: false,
                category: Some("Macro".into()),
                volume: None,
                liquidity: None,
                end_date: None,
            }],
        };

        let snapshots = WorldSnapshots::derive(
            &EventSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                events: vec![],
            },
            &DerivedSignalSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                signals: vec![],
            },
            &insights,
            &DecisionSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                convergence_scores: HashMap::new(),
                market_regime: crate::graph::decision::MarketRegimeFilter::neutral(),
                order_suggestions: vec![],
                degradations: HashMap::new(),
            },
            &reasoning,
            Some(&polymarket),
            None,
        );

        assert!(snapshots
            .world_state
            .entities
            .iter()
            .any(|entity| entity.entity_id == "world:polymarket:fed-cut"));
        assert!(snapshots
            .world_state
            .entities
            .iter()
            .find(|entity| entity.entity_id == "world:market")
            .is_some_and(|entity| entity.regime.contains("event-risk-on")));
    }

    #[test]
    fn backward_reasoning_demotes_leading_cause_when_contradiction_pressure_rises() {
        let base_provenance = crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
        );
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:contest".into(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:700.HK:review"),
            lineage: crate::ontology::DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.06),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "contest case".into(),
            risk_notes: vec![],
        };
        let market_path = crate::ontology::PropagationPath {
            path_id: "path:market_stress:tech".into(),
            summary: "market stress may propagate into 700.HK".into(),
            confidence: dec!(0.62),
            steps: vec![crate::ontology::PropagationStep {
                from: ReasoningScope::Market,
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "market stress concentration".into(),
                confidence: dec!(0.62),
                references: vec!["graph_stress".into()],
            }],
        };
        let sector_path = crate::ontology::PropagationPath {
            path_id: "path:sector_spill:tech:700.HK".into(),
            summary: "tech regime may propagate into 700.HK".into(),
            confidence: dec!(0.58),
            steps: vec![crate::ontology::PropagationStep {
                from: ReasoningScope::Sector("tech".into()),
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "sector_symbol_spillover".into(),
                confidence: dec!(0.58),
                references: vec!["sector:tech".into()],
            }],
        };
        let world_state = WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![
                EntityState {
                    entity_id: "world:market".into(),
                    scope: ReasoningScope::Market,
                    layer: WorldLayer::Forest,
                    provenance: prov("world:market"),
                    label: "Market canopy".into(),
                    regime: "stress-dominant".into(),
                    confidence: dec!(0.72),
                    local_support: dec!(0.10),
                    propagated_support: dec!(0.64),
                    drivers: vec!["market stress=0.72".into(), "clusters=2".into()],
                },
                EntityState {
                    entity_id: "world:sector:tech".into(),
                    scope: ReasoningScope::Sector("tech".into()),
                    layer: WorldLayer::Trunk,
                    provenance: prov("world:sector:tech"),
                    label: "Sector tech".into(),
                    regime: "tech bid still coherent".into(),
                    confidence: dec!(0.64),
                    local_support: dec!(0.18),
                    propagated_support: dec!(0.52),
                    drivers: vec!["tech leadership persistent".into()],
                },
            ],
        };

        let leading_market_hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:contest".into(),
            family_key: "contest".into(),
            family_label: "Cause Contest".into(),
            provenance: prov("hyp:700.HK:contest"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK is being framed by broader stress".into(),
            confidence: dec!(0.66),
            local_support_weight: dec!(0.18),
            local_contradict_weight: dec!(0.06),
            propagated_support_weight: dec!(0.62),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![
                ReasoningEvidence {
                    statement: "market stress route remains active".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.58),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "tech spillover still supports repricing".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.32),
                    references: vec![sector_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "local bid still absorbs supply".into(),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.18),
                    references: vec!["depth:700.HK".into()],
                    provenance: base_provenance.clone(),
                },
            ],
            invalidation_conditions: vec![crate::ontology::InvalidationCondition {
                description: "market stress route deactivates".into(),
                references: vec![market_path.path_id.clone()],
            }],
            propagation_path_ids: vec![market_path.path_id.clone(), sector_path.path_id.clone()],
            expected_observations: vec![],
        };
        let reasoning_market = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![leading_market_hypothesis.clone()],
            propagation_paths: vec![market_path.clone(), sector_path.clone()],
            tactical_setups: vec![setup.clone()],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let initial = derive_backward_reasoning(&reasoning_market, &world_state, None);
        let initial_investigation = &initial.investigations[0];
        assert_eq!(
            initial_investigation
                .leading_cause
                .as_ref()
                .map(|cause| cause.scope.clone()),
            Some(ReasoningScope::Market)
        );

        let contradicted_market_hypothesis = Hypothesis {
            propagated_contradict_weight: dec!(0.44),
            evidence: vec![
                ReasoningEvidence {
                    statement: "market stress route remains active".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.42),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "tech spillover still supports repricing".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.48),
                    references: vec![sector_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "market path keeps failing follow-through".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Contradicts,
                    weight: dec!(0.44),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "local bid still absorbs supply".into(),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.18),
                    references: vec!["depth:700.HK".into()],
                    provenance: base_provenance,
                },
            ],
            ..leading_market_hypothesis
        };
        let reasoning_contradicted = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![contradicted_market_hypothesis],
            propagation_paths: vec![market_path, sector_path],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let contradicted =
            derive_backward_reasoning(&reasoning_contradicted, &world_state, Some(&initial));
        let contradicted_investigation = &contradicted.investigations[0];
        assert_eq!(
            contradicted_investigation
                .leading_cause
                .as_ref()
                .map(|cause| cause.scope.clone()),
            Some(ReasoningScope::Sector("tech".into()))
        );
        assert!(contradicted_investigation
            .runner_up_cause
            .as_ref()
            .is_some_and(|cause| cause.scope == ReasoningScope::Market));
        assert!(contradicted_investigation
            .runner_up_cause
            .as_ref()
            .is_some_and(|cause| cause.contradict_weight > Decimal::ZERO));
    }
}
