use super::*;

fn scope_evidence_ref(scope: &crate::ontology::ReasoningScope) -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceRefKind::Scope,
        ref_id: scope_node_id(scope),
        label: Some(scope_node_label(scope)),
    }
}

pub fn reasoning_knowledge_links(
    hypotheses: &[Hypothesis],
    setups: &[TacticalSetup],
    cases: &[CaseSummary],
) -> Vec<AgentKnowledgeLink> {
    let mut links = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let hypothesis_map = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<std::collections::HashMap<_, _>>();

    for hypothesis in hypotheses {
        let scope_kind = hypothesis.scope.kind_slug();
        let source = knowledge_node_ref(
            "hypothesis",
            crate::ontology::hypothesis_node_id(&hypothesis.hypothesis_id),
            hypothesis.family_label.clone(),
        );
        let target = knowledge_node_ref(
            hypothesis.scope.kind_slug(),
            scope_node_id(&hypothesis.scope),
            scope_node_label(&hypothesis.scope),
        );
        let link_id = format!(
            "hypothesis_targets_scope:{}:{}",
            source.node_id, target.node_id
        );
        if seen.insert(link_id.clone()) {
            links.push(AgentKnowledgeLink {
                link_id,
                relation: KnowledgeRelation::TargetsScope,
                source,
                target,
                confidence: hypothesis.confidence,
                attributes: KnowledgeLinkAttributes::TargetsScope {
                    source_kind: "hypothesis".into(),
                    scope_kind: scope_kind.into(),
                },
                rationale: Some(hypothesis.statement.clone()),
            });
        }
    }

    for setup in setups {
        let scope_kind = setup.scope.kind_slug();
        let setup_node =
            knowledge_node_ref("setup", setup_node_id(&setup.setup_id), setup.title.clone());
        let scope_node = knowledge_node_ref(
            setup.scope.kind_slug(),
            scope_node_id(&setup.scope),
            scope_node_label(&setup.scope),
        );
        let scope_link_id = format!(
            "setup_targets_scope:{}:{}",
            setup_node.node_id, scope_node.node_id
        );
        if seen.insert(scope_link_id.clone()) {
            links.push(AgentKnowledgeLink {
                link_id: scope_link_id,
                relation: KnowledgeRelation::TargetsScope,
                source: setup_node.clone(),
                target: scope_node,
                confidence: setup.confidence,
                attributes: KnowledgeLinkAttributes::TargetsScope {
                    source_kind: "setup".into(),
                    scope_kind: scope_kind.into(),
                },
                rationale: Some(setup.entry_rationale.clone()),
            });
        }

        if let Some(hypothesis) = hypothesis_map.get(setup.hypothesis_id.as_str()) {
            let hypothesis_node = knowledge_node_ref(
                "hypothesis",
                crate::ontology::hypothesis_node_id(&hypothesis.hypothesis_id),
                hypothesis.family_label.clone(),
            );
            let link_id = format!(
                "setup_instantiates_hypothesis:{}:{}",
                setup_node.node_id, hypothesis_node.node_id
            );
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::InstantiatesHypothesis,
                    source: setup_node.clone(),
                    target: hypothesis_node,
                    confidence: setup.confidence,
                    attributes: KnowledgeLinkAttributes::InstantiatesHypothesis {
                        action: setup.action.clone(),
                        confidence_gap: setup.confidence_gap,
                    },
                    rationale: Some(setup.entry_rationale.clone()),
                });
            }
        }
    }

    for case in cases {
        let setup_node =
            knowledge_node_ref("setup", setup_node_id(&case.setup_id), case.title.clone());
        if let Some(primary) = case.reasoning_profile.primary_mechanism.as_ref() {
            let mechanism_node = knowledge_node_ref(
                "mechanism",
                mechanism_node_id(&primary.label),
                primary.label.clone(),
            );
            let link_id = format!(
                "setup_primary_mechanism:{}:{}",
                setup_node.node_id, mechanism_node.node_id
            );
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::PrimaryMechanism,
                    source: setup_node.clone(),
                    target: mechanism_node,
                    confidence: primary.score,
                    attributes: KnowledgeLinkAttributes::PrimaryMechanism {
                        mechanism_score: primary.score,
                        case_action: case.recommended_action.clone(),
                    },
                    rationale: Some(primary.summary.clone()),
                });
            }
        }
        for mechanism in &case.reasoning_profile.competing_mechanisms {
            let mechanism_node = knowledge_node_ref(
                "mechanism",
                mechanism_node_id(&mechanism.label),
                mechanism.label.clone(),
            );
            let link_id = format!(
                "setup_competing_mechanism:{}:{}",
                setup_node.node_id, mechanism_node.node_id
            );
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::CompetingMechanism,
                    source: setup_node.clone(),
                    target: mechanism_node,
                    confidence: mechanism.score,
                    attributes: KnowledgeLinkAttributes::CompetingMechanism {
                        mechanism_score: mechanism.score,
                        case_action: case.recommended_action.clone(),
                    },
                    rationale: Some(mechanism.summary.clone()),
                });
            }
        }
    }

    links
}

pub fn reasoning_knowledge_events(
    hypotheses: &[Hypothesis],
    setups: &[TacticalSetup],
    cases: &[CaseSummary],
) -> Vec<AgentKnowledgeEvent> {
    let mut events = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let hypothesis_map = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<std::collections::HashMap<_, _>>();

    for setup in setups {
        let Some(hypothesis) = hypothesis_map.get(setup.hypothesis_id.as_str()) else {
            continue;
        };
        let setup_node =
            knowledge_node_ref("setup", setup_node_id(&setup.setup_id), setup.title.clone());
        let hypothesis_node = knowledge_node_ref(
            "hypothesis",
            crate::ontology::hypothesis_node_id(&hypothesis.hypothesis_id),
            hypothesis.family_label.clone(),
        );
        let event_id = format!(
            "hypothesis_instantiation:{}:{}",
            setup_node.node_id, hypothesis_node.node_id
        );
        if seen.insert(event_id.clone()) {
            events.push(AgentKnowledgeEvent {
                event_id,
                kind: KnowledgeEventKind::HypothesisInstantiation,
                subject: setup_node.clone(),
                object: Some(hypothesis_node),
                confidence: setup.confidence,
                evidence: vec![
                    EvidenceRef {
                        kind: EvidenceRefKind::Setup,
                        ref_id: setup.setup_id.clone(),
                        label: Some(setup.title.clone()),
                    },
                    EvidenceRef {
                        kind: EvidenceRefKind::Hypothesis,
                        ref_id: hypothesis.hypothesis_id.clone(),
                        label: Some(hypothesis.family_label.clone()),
                    },
                    scope_evidence_ref(&setup.scope),
                ],
                attributes: KnowledgeEventAttributes::HypothesisInstantiation {
                    action: setup.action.clone(),
                    confidence_gap: setup.confidence_gap,
                    scope_kind: setup.scope.kind_slug().into(),
                },
                rationale: Some(setup.entry_rationale.clone()),
            });
        }
    }

    for case in cases {
        let setup_node =
            knowledge_node_ref("setup", setup_node_id(&case.setup_id), case.title.clone());
        if let Some(primary) = case.reasoning_profile.primary_mechanism.as_ref() {
            let mechanism_node = knowledge_node_ref(
                "mechanism",
                mechanism_node_id(&primary.label),
                primary.label.clone(),
            );
            let event_id = format!(
                "mechanism_assessment:primary:{}:{}",
                setup_node.node_id, mechanism_node.node_id
            );
            if seen.insert(event_id.clone()) {
                events.push(AgentKnowledgeEvent {
                    event_id,
                    kind: KnowledgeEventKind::MechanismAssessment,
                    subject: setup_node.clone(),
                    object: Some(mechanism_node),
                    confidence: primary.score,
                    evidence: vec![
                        EvidenceRef {
                            kind: EvidenceRefKind::Setup,
                            ref_id: case.setup_id.clone(),
                            label: Some(case.title.clone()),
                        },
                        EvidenceRef {
                            kind: EvidenceRefKind::Mechanism,
                            ref_id: mechanism_node_id(&primary.label),
                            label: Some(primary.label.clone()),
                        },
                    ],
                    attributes: KnowledgeEventAttributes::MechanismAssessment {
                        role: "primary".into(),
                        mechanism_score: primary.score,
                        case_action: case.recommended_action.clone(),
                    },
                    rationale: Some(primary.summary.clone()),
                });
            }
        }
        for mechanism in &case.reasoning_profile.competing_mechanisms {
            let mechanism_node = knowledge_node_ref(
                "mechanism",
                mechanism_node_id(&mechanism.label),
                mechanism.label.clone(),
            );
            let event_id = format!(
                "mechanism_assessment:competing:{}:{}",
                setup_node.node_id, mechanism_node.node_id
            );
            if seen.insert(event_id.clone()) {
                events.push(AgentKnowledgeEvent {
                    event_id,
                    kind: KnowledgeEventKind::MechanismAssessment,
                    subject: setup_node.clone(),
                    object: Some(mechanism_node),
                    confidence: mechanism.score,
                    evidence: vec![
                        EvidenceRef {
                            kind: EvidenceRefKind::Setup,
                            ref_id: case.setup_id.clone(),
                            label: Some(case.title.clone()),
                        },
                        EvidenceRef {
                            kind: EvidenceRefKind::Mechanism,
                            ref_id: mechanism_node_id(&mechanism.label),
                            label: Some(mechanism.label.clone()),
                        },
                    ],
                    attributes: KnowledgeEventAttributes::MechanismAssessment {
                        role: "competing".into(),
                        mechanism_score: mechanism.score,
                        case_action: case.recommended_action.clone(),
                    },
                    rationale: Some(mechanism.summary.clone()),
                });
            }
        }
    }

    events
}
