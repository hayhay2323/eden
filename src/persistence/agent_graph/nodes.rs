use super::*;

fn market_label(market: LiveMarket) -> String {
    match market {
        LiveMarket::Hk => "HK".into(),
        LiveMarket::Us => "US".into(),
    }
}

fn decision_node_ref(decision: &AgentDecision) -> AgentKnowledgeNodeRef {
    match decision {
        AgentDecision::Market(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", market_label(item.market), item.best_action),
        ),
        AgentDecision::Sector(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", item.sector, item.best_action),
        ),
        AgentDecision::Symbol(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", item.symbol, item.best_action),
        ),
    }
}

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

pub fn build_knowledge_node_records(
    market: LiveMarket,
    tick_number: u64,
    recorded_at: OffsetDateTime,
    macro_events: &[AgentMacroEvent],
    decisions: &[AgentDecision],
    hypotheses: &[Hypothesis],
    setups: &[TacticalSetup],
    cases: &[CaseSummary],
    world_state: Option<&WorldStateSnapshot>,
    backward_reasoning: Option<&BackwardReasoningSnapshot>,
    active_positions: &[ActionNode],
    links: &[AgentKnowledgeLink],
) -> (
    Vec<KnowledgeNodeHistoryRecord>,
    Vec<KnowledgeNodeStateRecord>,
) {
    let mut nodes = std::collections::BTreeMap::<
        String,
        (AgentKnowledgeNodeRef, KnowledgeNodeAttributes),
    >::new();

    let mut insert_node =
        |node: AgentKnowledgeNodeRef, attributes: KnowledgeNodeAttributes| match nodes
            .get_mut(&node.node_id)
        {
            Some((existing_node, existing_attributes))
                if matches!(existing_attributes, KnowledgeNodeAttributes::Generic)
                    && !matches!(attributes, KnowledgeNodeAttributes::Generic) =>
            {
                *existing_node = node;
                *existing_attributes = attributes;
            }
            None => {
                nodes.insert(node.node_id.clone(), (node, attributes));
            }
            _ => {}
        };

    for link in links {
        insert_node(link.source.clone(), KnowledgeNodeAttributes::Generic);
        insert_node(link.target.clone(), KnowledgeNodeAttributes::Generic);
    }

    for event in macro_events {
        insert_node(
            crate::ontology::macro_event_knowledge_node_ref(&event.event_id, &event.headline),
            KnowledgeNodeAttributes::MacroEvent {
                event_type: event.event_type.clone(),
                authority_level: event.authority_level.clone(),
                confidence: event.confidence,
                confirmation_state: event.confirmation_state.clone(),
                primary_scope: event.impact.primary_scope.clone(),
                preferred_expression: event.impact.preferred_expression.clone(),
                requires_market_confirmation: event.impact.requires_market_confirmation,
                affected_markets: event.impact.affected_markets.clone(),
                affected_sectors: event.impact.affected_sectors.clone(),
                affected_symbols: event.impact.affected_symbols.clone(),
                decisive_factors: event.impact.decisive_factors.clone(),
            },
        );
    }

    for decision in decisions {
        match decision {
            AgentDecision::Market(item) => insert_node(
                decision_node_ref(decision),
                    KnowledgeNodeAttributes::Decision {
                        scope_kind: "market".into(),
                        title: format!("{} {}", market_label(item.market), item.best_action),
                        bias: item.bias.clone(),
                        best_action: item.best_action.clone(),
                        regime_bias: item.regime_bias.clone(),
                        alpha_horizon: item.alpha_horizon.clone(),
                        confidence: item.market_impulse_score,
                        score: item.market_impulse_score,
                        preferred_expression: Some(item.preferred_expression.clone()),
                        reference_symbols: item.reference_symbols.clone(),
                    },
            ),
            AgentDecision::Sector(item) => {
                insert_node(
                    decision_node_ref(decision),
                    KnowledgeNodeAttributes::Decision {
                        scope_kind: "sector".into(),
                        title: format!("{} {}", item.sector, item.best_action),
                        bias: item.bias.clone(),
                        best_action: item.best_action.clone(),
                        regime_bias: item.regime_bias.clone(),
                        alpha_horizon: item.alpha_horizon.clone(),
                        confidence: item.sector_impulse_score,
                        score: item.sector_impulse_score,
                        preferred_expression: Some(item.preferred_expression.clone()),
                        reference_symbols: item.reference_symbols.clone(),
                    },
                );
                insert_node(
                    sector_knowledge_node_ref(&item.sector),
                    KnowledgeNodeAttributes::Scope {
                        scope_kind: "sector".into(),
                        scope_label: item.sector.clone(),
                    },
                );
            }
            AgentDecision::Symbol(item) => {
                insert_node(
                    decision_node_ref(decision),
                    KnowledgeNodeAttributes::Decision {
                        scope_kind: "symbol".into(),
                        title: item.title.clone().unwrap_or_else(|| item.symbol.clone()),
                        bias: item.bias.clone(),
                        best_action: item.best_action.clone(),
                        regime_bias: item.regime_bias.clone(),
                        alpha_horizon: item.alpha_horizon.clone(),
                        confidence: item.confidence,
                        score: item.score,
                        preferred_expression: None,
                        reference_symbols: vec![],
                    },
                );
                insert_node(
                    symbol_knowledge_node_ref(&item.symbol),
                    KnowledgeNodeAttributes::Scope {
                        scope_kind: "symbol".into(),
                        scope_label: item.symbol.clone(),
                    },
                );
            }
        }
    }

    for hypothesis in hypotheses {
        let scope_kind = hypothesis.scope.kind_slug();
        insert_node(
            knowledge_node_ref(
                "hypothesis",
                crate::ontology::hypothesis_node_id(&hypothesis.hypothesis_id),
                hypothesis.family_label.clone(),
            ),
            KnowledgeNodeAttributes::Hypothesis {
                family_key: hypothesis.family_key.clone(),
                family_label: hypothesis.family_label.clone(),
                statement: hypothesis.statement.clone(),
                confidence: hypothesis.confidence,
                local_support_weight: hypothesis.local_support_weight,
                local_contradict_weight: hypothesis.local_contradict_weight,
                propagated_support_weight: hypothesis.propagated_support_weight,
                propagated_contradict_weight: hypothesis.propagated_contradict_weight,
                propagation_path_ids: hypothesis.propagation_path_ids.clone(),
                expected_observations: hypothesis.expected_observations.clone(),
            },
        );
        insert_node(
            knowledge_node_ref(
                scope_kind,
                scope_node_id(&hypothesis.scope),
                scope_node_label(&hypothesis.scope),
            ),
            KnowledgeNodeAttributes::Scope {
                scope_kind: scope_kind.into(),
                scope_label: scope_node_label(&hypothesis.scope),
            },
        );
    }

    for setup in setups {
        let scope_kind = setup.scope.kind_slug();
        insert_node(
            knowledge_node_ref("setup", setup_node_id(&setup.setup_id), setup.title.clone()),
            KnowledgeNodeAttributes::Setup {
                action: setup.action.clone(),
                time_horizon: setup.time_horizon.clone(),
                confidence: setup.confidence,
                confidence_gap: setup.confidence_gap,
                heuristic_edge: setup.heuristic_edge,
                workflow_id: setup.workflow_id.clone(),
                entry_rationale: setup.entry_rationale.clone(),
                risk_notes: setup.risk_notes.clone(),
            },
        );
        insert_node(
            knowledge_node_ref(
                scope_kind,
                scope_node_id(&setup.scope),
                scope_node_label(&setup.scope),
            ),
            KnowledgeNodeAttributes::Scope {
                scope_kind: scope_kind.into(),
                scope_label: scope_node_label(&setup.scope),
            },
        );
    }

    for case in cases {
        if let Some(primary) = case.reasoning_profile.primary_mechanism.as_ref() {
            insert_node(
                knowledge_node_ref(
                    "mechanism",
                    mechanism_node_id(&primary.label),
                    primary.label.clone(),
                ),
                KnowledgeNodeAttributes::Mechanism {
                    label: primary.label.clone(),
                    summary: primary.summary.clone(),
                    invalidation: primary.invalidation.clone(),
                    human_checks: primary.human_checks.clone(),
                },
            );
        }
        for mechanism in &case.reasoning_profile.competing_mechanisms {
            insert_node(
                knowledge_node_ref(
                    "mechanism",
                    mechanism_node_id(&mechanism.label),
                    mechanism.label.clone(),
                ),
                KnowledgeNodeAttributes::Mechanism {
                    label: mechanism.label.clone(),
                    summary: mechanism.summary.clone(),
                    invalidation: mechanism.invalidation.clone(),
                    human_checks: mechanism.human_checks.clone(),
                },
            );
        }
    }

    if let Some(world_state) = world_state {
        for entity in &world_state.entities {
            insert_node(
                world_entity_knowledge_node_ref(&entity.entity_id, entity.label.clone()),
                KnowledgeNodeAttributes::WorldEntity {
                    layer: entity.layer.to_string(),
                    regime: entity.regime.clone(),
                    confidence: entity.confidence,
                    local_support: entity.local_support,
                    propagated_support: entity.propagated_support,
                    drivers: entity.drivers.clone(),
                },
            );
        }
    }

    if let Some(backward_reasoning) = backward_reasoning {
        for investigation in &backward_reasoning.investigations {
            if let Some(cause) = investigation.leading_cause.as_ref() {
                insert_node(
                    backward_cause_knowledge_node_ref(&cause.cause_id, cause.explanation.clone()),
                    KnowledgeNodeAttributes::BackwardCause {
                        layer: cause.layer.to_string(),
                        depth: cause.depth,
                        explanation: cause.explanation.clone(),
                        chain_summary: cause.chain_summary.clone(),
                        confidence: cause.confidence,
                        support_weight: cause.support_weight,
                        contradict_weight: cause.contradict_weight,
                        net_conviction: cause.net_conviction,
                        competitive_score: cause.competitive_score,
                        falsifier: cause.falsifier.clone(),
                        references: cause.references.clone(),
                    },
                );
            }
            for cause in &investigation.candidate_causes {
                insert_node(
                    backward_cause_knowledge_node_ref(&cause.cause_id, cause.explanation.clone()),
                    KnowledgeNodeAttributes::BackwardCause {
                        layer: cause.layer.to_string(),
                        depth: cause.depth,
                        explanation: cause.explanation.clone(),
                        chain_summary: cause.chain_summary.clone(),
                        confidence: cause.confidence,
                        support_weight: cause.support_weight,
                        contradict_weight: cause.contradict_weight,
                        net_conviction: cause.net_conviction,
                        competitive_score: cause.competitive_score,
                        falsifier: cause.falsifier.clone(),
                        references: cause.references.clone(),
                    },
                );
            }
        }
    }

    for position in active_positions {
        insert_node(
            position_knowledge_node_ref(
                &position.workflow_id,
                format!(
                    "{} {}",
                    position.symbol.0,
                    action_direction_label(position.direction)
                ),
            ),
            KnowledgeNodeAttributes::Position {
                market: match position.market {
                    Market::Hk => "hk".into(),
                    Market::Us => "us".into(),
                },
                symbol: position.symbol.0.clone(),
                sector: position.sector.clone(),
                stage: action_stage_label(position.stage).into(),
                direction: action_direction_label(position.direction).into(),
                entry_confidence: position.entry_confidence,
                current_confidence: position.current_confidence,
                entry_price: position.entry_price,
                pnl: position.pnl,
                age_ticks: position.age_ticks,
                degradation_score: position.degradation_score,
                exit_forming: position.exit_forming,
            },
        );
    }

    let history = nodes
        .values()
        .map(|(node, attributes)| {
            knowledge_node_history_record(
                market,
                tick_number,
                recorded_at,
                node,
                attributes.clone(),
            )
        })
        .collect::<Vec<_>>();
    let state = nodes
        .values()
        .map(|(node, attributes)| {
            knowledge_node_state_record(market, tick_number, recorded_at, node, attributes.clone())
        })
        .collect::<Vec<_>>();

    (history, state)
}
