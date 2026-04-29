use super::*;

fn action_stage_label(stage: ActionNodeStage) -> &'static str {
    match stage {
        ActionNodeStage::Suggested => "suggested",
        ActionNodeStage::Confirmed => "confirmed",
        ActionNodeStage::Executed => "executed",
        ActionNodeStage::Monitoring => "monitoring",
        ActionNodeStage::Reviewed => "reviewed",
    }
}

fn action_direction_label(direction: ActionDirection) -> &'static str {
    match direction {
        ActionDirection::Long => "long",
        ActionDirection::Short => "short",
        ActionDirection::Neutral => "neutral",
    }
}

fn scope_evidence_ref(scope: &crate::ontology::ReasoningScope) -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceRefKind::Scope,
        ref_id: scope_node_id(scope),
        label: Some(scope_node_label(scope)),
    }
}

fn perceptual_state_node(state_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_state",
        format!("perceptual_state:{state_id}"),
        label,
    )
}

fn perceptual_evidence_node(evidence_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_evidence",
        format!("perceptual_evidence:{evidence_id}"),
        label,
    )
}

fn perceptual_expectation_node(expectation_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_expectation",
        format!("perceptual_expectation:{expectation_id}"),
        label,
    )
}

fn attention_allocation_node(allocation_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "attention_allocation",
        format!("attention_allocation:{allocation_id}"),
        label,
    )
}

fn perceptual_uncertainty_node(uncertainty_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_uncertainty",
        format!("perceptual_uncertainty:{uncertainty_id}"),
        label,
    )
}

pub fn build_runtime_knowledge_links(
    world_state: Option<&WorldStateSnapshot>,
    backward_reasoning: Option<&BackwardReasoningSnapshot>,
    active_positions: &[ActionNode],
) -> Vec<AgentKnowledgeLink> {
    let mut links = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    if let Some(world_state) = world_state {
        for entity in &world_state.entities {
            let source = world_entity_knowledge_node_ref(&entity.entity_id, entity.label.clone());
            let scope_kind = entity.scope.kind_slug();
            let target = knowledge_node_ref(
                scope_kind,
                scope_node_id(&entity.scope),
                scope_node_label(&entity.scope),
            );
            let link_id = format!(
                "world_entity_describes_scope:{}:{}",
                source.node_id, target.node_id
            );
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::DescribesScope,
                    source,
                    target,
                    confidence: entity.confidence,
                    attributes: KnowledgeLinkAttributes::DescribesScope {
                        layer: entity.layer.to_string(),
                        regime: entity.regime.clone(),
                    },
                    rationale: Some(entity.regime.clone()),
                });
            }
        }
        for state in &world_state.perceptual_states {
            let state_node = perceptual_state_node(&state.state_id, state.label.clone());
            let scope_kind = state.scope.kind_slug();
            let scope_node = knowledge_node_ref(
                scope_kind,
                scope_node_id(&state.scope),
                scope_node_label(&state.scope),
            );
            let state_link_id = format!(
                "perceptual_state_describes_scope:{}:{}",
                state_node.node_id, scope_node.node_id
            );
            if seen.insert(state_link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id: state_link_id,
                    relation: KnowledgeRelation::DescribesCurrentStateOf,
                    source: state_node.clone(),
                    target: scope_node,
                    confidence: state.confidence,
                    attributes: KnowledgeLinkAttributes::DescribesCurrentStateOf {
                        state_kind: state.state_kind.clone(),
                        trend: state.trend.clone(),
                    },
                    rationale: Some(state.state_kind.clone()),
                });
            }

            for evidence in &state.supporting_evidence {
                let evidence_node =
                    perceptual_evidence_node(&evidence.evidence_id, evidence.rationale.clone());
                let link_id = format!(
                    "perceptual_state_supports:{}:{}",
                    state_node.node_id, evidence_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::SupportedByEvidence,
                        source: state_node.clone(),
                        target: evidence_node,
                        confidence: evidence.weight,
                        attributes: KnowledgeLinkAttributes::SupportedByEvidence {
                            channel: evidence.channel.clone(),
                            polarity: evidence.polarity.to_string(),
                        },
                        rationale: Some(evidence.rationale.clone()),
                    });
                }
            }
            for evidence in &state.opposing_evidence {
                let evidence_node =
                    perceptual_evidence_node(&evidence.evidence_id, evidence.rationale.clone());
                let link_id = format!(
                    "perceptual_state_contradicts:{}:{}",
                    state_node.node_id, evidence_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::ContradictedByEvidence,
                        source: state_node.clone(),
                        target: evidence_node,
                        confidence: evidence.weight,
                        attributes: KnowledgeLinkAttributes::ContradictedByEvidence {
                            channel: evidence.channel.clone(),
                            polarity: evidence.polarity.to_string(),
                        },
                        rationale: Some(evidence.rationale.clone()),
                    });
                }
            }
            for evidence in &state.missing_evidence {
                let evidence_node =
                    perceptual_evidence_node(&evidence.evidence_id, evidence.rationale.clone());
                let link_id = format!(
                    "perceptual_state_missing:{}:{}",
                    state_node.node_id, evidence_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::MissingExpectedEvidenceFor,
                        source: evidence_node,
                        target: state_node.clone(),
                        confidence: evidence.weight,
                        attributes: KnowledgeLinkAttributes::MissingExpectedEvidenceFor {
                            channel: evidence.channel.clone(),
                            polarity: evidence.polarity.to_string(),
                        },
                        rationale: Some(evidence.rationale.clone()),
                    });
                }
            }
            for expectation in &state.expectations {
                let expectation_node = perceptual_expectation_node(
                    &expectation.expectation_id,
                    expectation.rationale.clone(),
                );
                let link_id = format!(
                    "perceptual_state_expectation:{}:{}",
                    state_node.node_id, expectation_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::ExpectsConfirmationFrom,
                        source: state_node.clone(),
                        target: expectation_node,
                        confidence: state.confidence,
                        attributes: KnowledgeLinkAttributes::ExpectsConfirmationFrom {
                            expectation_kind: expectation.kind.to_string(),
                            expectation_status: expectation.status.to_string(),
                        },
                        rationale: Some(expectation.rationale.clone()),
                    });
                }
            }
            for allocation in &state.attention_allocations {
                let allocation_node = attention_allocation_node(
                    &allocation.allocation_id,
                    format!("{} {}", state.label, allocation.channel),
                );
                let link_id = format!(
                    "perceptual_state_attention:{}:{}",
                    state_node.node_id, allocation_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::AttentionAllocatedTo,
                        source: state_node.clone(),
                        target: allocation_node,
                        confidence: allocation.weight,
                        attributes: KnowledgeLinkAttributes::AttentionAllocatedTo {
                            channel: allocation.channel.clone(),
                        },
                        rationale: Some(allocation.rationale.clone()),
                    });
                }
            }
            for uncertainty in &state.uncertainties {
                let uncertainty_node = perceptual_uncertainty_node(
                    &uncertainty.uncertainty_id,
                    uncertainty.rationale.clone(),
                );
                let link_id = format!(
                    "perceptual_state_uncertainty:{}:{}",
                    state_node.node_id, uncertainty_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::UncertainDueTo,
                        source: state_node.clone(),
                        target: uncertainty_node,
                        confidence: uncertainty.level,
                        attributes: KnowledgeLinkAttributes::UncertainDueTo {
                            uncertainty_level: uncertainty.level,
                        },
                        rationale: Some(uncertainty.rationale.clone()),
                    });
                }
            }
        }
    }

    if let Some(backward_reasoning) = backward_reasoning {
        for investigation in &backward_reasoning.investigations {
            let leaf_kind = investigation.leaf_scope.kind_slug();
            let leaf_node = knowledge_node_ref(
                leaf_kind,
                scope_node_id(&investigation.leaf_scope),
                scope_node_label(&investigation.leaf_scope),
            );

            for cause in &investigation.candidate_causes {
                let cause_node =
                    backward_cause_knowledge_node_ref(&cause.cause_id, cause.explanation.clone());
                let scope_kind = cause.scope.kind_slug();
                let scope_node = knowledge_node_ref(
                    scope_kind,
                    scope_node_id(&cause.scope),
                    scope_node_label(&cause.scope),
                );
                let scope_link_id = format!(
                    "backward_cause_targets_scope:{}:{}",
                    cause_node.node_id, scope_node.node_id
                );
                if seen.insert(scope_link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id: scope_link_id,
                        relation: KnowledgeRelation::TargetsScope,
                        source: cause_node.clone(),
                        target: scope_node,
                        confidence: cause.confidence,
                        attributes: KnowledgeLinkAttributes::TargetsScope {
                            source_kind: "backward_cause".into(),
                            scope_kind: scope_kind.into(),
                        },
                        rationale: Some(cause.explanation.clone()),
                    });
                }
                let leaf_link_id = format!(
                    "backward_cause_candidate_leaf:{}:{}",
                    cause_node.node_id, leaf_node.node_id
                );
                if seen.insert(leaf_link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id: leaf_link_id,
                        relation: KnowledgeRelation::CandidateForLeaf,
                        source: cause_node.clone(),
                        target: leaf_node.clone(),
                        confidence: cause.confidence,
                        attributes: KnowledgeLinkAttributes::CandidateForLeaf {
                            leaf_regime: investigation.leaf_regime.clone(),
                            contest_state: investigation.contest_state.to_string(),
                        },
                        rationale: Some(cause.explanation.clone()),
                    });
                }
            }

            if let Some(cause) = investigation.leading_cause.as_ref() {
                let cause_node =
                    backward_cause_knowledge_node_ref(&cause.cause_id, cause.explanation.clone());
                let link_id = format!(
                    "backward_cause_leading_leaf:{}:{}",
                    cause_node.node_id, leaf_node.node_id
                );
                if seen.insert(link_id.clone()) {
                    links.push(AgentKnowledgeLink {
                        link_id,
                        relation: KnowledgeRelation::LeadingCauseForLeaf,
                        source: cause_node,
                        target: leaf_node.clone(),
                        confidence: cause.confidence,
                        attributes: KnowledgeLinkAttributes::LeadingCauseForLeaf {
                            leaf_regime: investigation.leaf_regime.clone(),
                            contest_state: investigation.contest_state.to_string(),
                            leader_streak: investigation.leading_cause_streak,
                            cause_gap: investigation.cause_gap,
                        },
                        rationale: Some(cause.explanation.clone()),
                    });
                }
            }
        }
    }

    for position in active_positions {
        let position_node = position_knowledge_node_ref(
            &position.workflow_id,
            format!(
                "{} {}",
                position.symbol.0,
                action_direction_label(position.direction)
            ),
        );
        let symbol_node = symbol_knowledge_node_ref(&position.symbol.0);
        let link_id = format!(
            "position_tracks_symbol:{}:{}",
            position_node.node_id, symbol_node.node_id
        );
        if seen.insert(link_id.clone()) {
            links.push(AgentKnowledgeLink {
                link_id,
                relation: KnowledgeRelation::TracksSymbol,
                source: position_node.clone(),
                target: symbol_node,
                confidence: position.current_confidence,
                attributes: KnowledgeLinkAttributes::TracksSymbol {
                    stage: action_stage_label(position.stage).into(),
                    direction: action_direction_label(position.direction).into(),
                    age_ticks: position.age_ticks,
                    exit_forming: position.exit_forming,
                },
                rationale: Some(action_stage_label(position.stage).into()),
            });
        }
        if let Some(sector) = position.sector.as_ref() {
            let sector_node = sector_knowledge_node_ref(sector);
            let link_id = format!(
                "position_tracks_sector:{}:{}",
                position_node.node_id, sector_node.node_id
            );
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::TracksSector,
                    source: position_node.clone(),
                    target: sector_node,
                    confidence: position.current_confidence,
                    attributes: KnowledgeLinkAttributes::TracksSector {
                        stage: action_stage_label(position.stage).into(),
                        direction: action_direction_label(position.direction).into(),
                        age_ticks: position.age_ticks,
                        exit_forming: position.exit_forming,
                    },
                    rationale: Some(action_stage_label(position.stage).into()),
                });
            }
        }
    }

    links
}

pub fn build_runtime_knowledge_events(
    backward_reasoning: Option<&BackwardReasoningSnapshot>,
    active_positions: &[ActionNode],
) -> Vec<AgentKnowledgeEvent> {
    let mut events = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    if let Some(backward_reasoning) = backward_reasoning {
        for investigation in &backward_reasoning.investigations {
            let Some(cause) = investigation.leading_cause.as_ref() else {
                continue;
            };
            let cause_node =
                backward_cause_knowledge_node_ref(&cause.cause_id, cause.explanation.clone());
            let leaf_node = knowledge_node_ref(
                investigation.leaf_scope.kind_slug(),
                scope_node_id(&investigation.leaf_scope),
                scope_node_label(&investigation.leaf_scope),
            );
            let event_id = format!(
                "leading_cause_assessment:{}:{}",
                cause_node.node_id, leaf_node.node_id
            );
            if seen.insert(event_id.clone()) {
                events.push(AgentKnowledgeEvent {
                    event_id,
                    kind: KnowledgeEventKind::LeadingCauseAssessment,
                    subject: cause_node.clone(),
                    object: Some(leaf_node),
                    confidence: cause.confidence,
                    evidence: vec![
                        EvidenceRef {
                            kind: EvidenceRefKind::BackwardCause,
                            ref_id: cause.cause_id.clone(),
                            label: Some(cause.explanation.clone()),
                        },
                        scope_evidence_ref(&investigation.leaf_scope),
                    ],
                    attributes: KnowledgeEventAttributes::LeadingCauseAssessment {
                        leaf_regime: investigation.leaf_regime.clone(),
                        contest_state: investigation.contest_state.to_string(),
                        leader_streak: investigation.leading_cause_streak,
                        cause_gap: investigation.cause_gap,
                    },
                    rationale: Some(cause.explanation.clone()),
                });
            }
        }
    }

    for position in active_positions {
        let position_node = position_knowledge_node_ref(
            &position.workflow_id,
            format!(
                "{} {}",
                position.symbol.0,
                action_direction_label(position.direction)
            ),
        );
        let symbol_node = symbol_knowledge_node_ref(&position.symbol.0);
        let symbol_event_id = format!(
            "position_tracking:{}:{}",
            position_node.node_id, symbol_node.node_id
        );
        if seen.insert(symbol_event_id.clone()) {
            events.push(AgentKnowledgeEvent {
                event_id: symbol_event_id,
                kind: KnowledgeEventKind::PositionTracking,
                subject: position_node.clone(),
                object: Some(symbol_node),
                confidence: position.current_confidence,
                evidence: vec![EvidenceRef {
                    kind: EvidenceRefKind::Workflow,
                    ref_id: position.workflow_id.clone(),
                    label: Some(position.workflow_id.clone()),
                }],
                attributes: KnowledgeEventAttributes::PositionTracking {
                    scope_kind: "symbol".into(),
                    stage: action_stage_label(position.stage).into(),
                    direction: action_direction_label(position.direction).into(),
                    age_ticks: position.age_ticks,
                    exit_forming: position.exit_forming,
                },
                rationale: Some(action_stage_label(position.stage).into()),
            });
        }
        if let Some(sector) = position.sector.as_ref() {
            let sector_node = sector_knowledge_node_ref(sector);
            let sector_event_id = format!(
                "position_tracking:{}:{}",
                position_node.node_id, sector_node.node_id
            );
            if seen.insert(sector_event_id.clone()) {
                events.push(AgentKnowledgeEvent {
                    event_id: sector_event_id,
                    kind: KnowledgeEventKind::PositionTracking,
                    subject: position_node.clone(),
                    object: Some(sector_node),
                    confidence: position.current_confidence,
                    evidence: vec![EvidenceRef {
                        kind: EvidenceRefKind::Workflow,
                        ref_id: position.workflow_id.clone(),
                        label: Some(position.workflow_id.clone()),
                    }],
                    attributes: KnowledgeEventAttributes::PositionTracking {
                        scope_kind: "sector".into(),
                        stage: action_stage_label(position.stage).into(),
                        direction: action_direction_label(position.direction).into(),
                        age_ticks: position.age_ticks,
                        exit_forming: position.exit_forming,
                    },
                    rationale: Some(action_stage_label(position.stage).into()),
                });
            }
        }
    }

    events
}
