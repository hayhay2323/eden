use std::collections::HashMap;

use crate::ontology::world::{
    BackwardCause, BackwardInvestigation, BackwardReasoningSnapshot, WorldLayer, WorldStateSnapshot,
};
use crate::ontology::{EvidencePolarity, ReasoningScope};
use crate::pipeline::reasoning::ReasoningSnapshot;
use rust_decimal::Decimal;
#[path = "backward_helpers.rs"]
mod backward_helpers;
use backward_helpers::*;
pub(crate) use backward_helpers::{
    scope_key, select_backward_investigation_targets, world_layer_priority, world_provenance,
};

pub(super) fn derive_backward_reasoning(
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

    let investigation_targets = select_backward_investigation_targets(
        reasoning,
        &hypothesis_map,
        &previous_investigation_map,
    );

    let investigations = investigation_targets
        .into_iter()
        .filter_map(|selection| {
            let leaf_scope_id = scope_key(&selection.scope);
            let hypothesis = hypothesis_map.get(selection.hypothesis_id.as_str()).copied()?;
            let previous_investigation = previous_investigation_map.get(leaf_scope_id.as_str()).copied();
            let relevant_paths = reasoning
                .propagation_paths
                .iter()
                .filter(|path| path.steps.iter().any(|step| step.to == selection.scope))
                .collect::<Vec<_>>();
            let mut candidate_causes = Vec::new();
            let default_local_falsifier = hypothesis
                .invalidation_conditions
                .first()
                .map(|item| item.description.clone())
                .or_else(|| Some(format!("local evidence stops supporting {}", selection.title)));

            for evidence in hypothesis.evidence.iter().take(3) {
                let layer = match evidence.kind {
                    crate::ontology::ReasoningEvidenceKind::PropagatedPath => WorldLayer::Branch,
                    _ => WorldLayer::Leaf,
                };
                let mut cause = BackwardCause {
                    cause_id: format!("cause:local:{}:{}", leaf_scope_id, local_cause_key(evidence)),
                    scope: selection.scope.clone(),
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
                            format!(
                                "spillover evidence currently outweighs local tape for {}",
                                selection.title
                            ),
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
                        layer: if matches!(origin.from, ReasoningScope::Market(_)) {
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
                            path.path_id, selection.title
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
                            format!(
                                "local tape currently outweighs spillover route {}",
                                path.path_id
                            ),
                            hypothesis.local_support_weight - hypothesis.propagated_support_weight,
                            "local-counter",
                        ));
                    }
                    attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                    candidate_causes.push(cause);
                }
            }

            if let Some(market_state) = world_map.get(scope_key(&ReasoningScope::market()).as_str()) {
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
                    chain_summary: Some(format!("{} -> {}", selection.title, market_state.label)),
                    confidence: market_state.confidence,
                    support_weight: Decimal::ZERO,
                    contradict_weight: Decimal::ZERO,
                    net_conviction: Decimal::ZERO,
                    competitive_score: Decimal::ZERO,
                    falsifier: Some(format!(
                        "market regime {} stops dominating {}",
                        market_state.regime, selection.title
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
                            .map(|step| matches!(step.from, ReasoningScope::Market(_)))
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
                            selection.title
                        ),
                        hypothesis.local_support_weight - hypothesis.propagated_support_weight,
                        "local-counter",
                    ));
                }
                attach_contest_metrics(&mut cause, supporting_evidence, contradicting_evidence);
                candidate_causes.push(cause);
            }

            if let ReasoningScope::Symbol(symbol) = &selection.scope {
                let sector_key = format!("sector:{}", symbol_to_sector_hint(reasoning, &symbol));
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
                        chain_summary: Some(format!("{} -> {}", selection.title, sector_state.label)),
                        confidence: sector_state.confidence,
                        support_weight: Decimal::ZERO,
                        contradict_weight: Decimal::ZERO,
                        net_conviction: Decimal::ZERO,
                        competitive_score: Decimal::ZERO,
                        falsifier: Some(format!(
                            "sector regime {} no longer frames {}",
                            sector_state.regime, selection.title
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
                                selection.title
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
            let mut cause_iter = candidate_causes.into_iter();
            let leading_cause = cause_iter.next();
            let runner_up_cause = cause_iter.next();
            let candidate_causes: Vec<_> = cause_iter.collect();
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
                investigation_id: selection.investigation_id.clone(),
                leaf_scope: selection.scope.clone(),
                leaf_label: selection.title.clone(),
                leaf_regime: selection.attention_hint.clone(),
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
