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

fn perceptual_state_node_ref(state_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_state",
        format!("perceptual_state:{state_id}"),
        label,
    )
}

fn perceptual_evidence_node_ref(evidence_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_evidence",
        format!("perceptual_evidence:{evidence_id}"),
        label,
    )
}

fn perceptual_expectation_node_ref(expectation_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_expectation",
        format!("perceptual_expectation:{expectation_id}"),
        label,
    )
}

fn attention_allocation_node_ref(allocation_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "attention_allocation",
        format!("attention_allocation:{allocation_id}"),
        label,
    )
}

fn perceptual_uncertainty_node_ref(uncertainty_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        "perceptual_uncertainty",
        format!("perceptual_uncertainty:{uncertainty_id}"),
        label,
    )
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
                // V2 Pass 2: family_key removed from Hypothesis. Persist
                // family_label as the bucket key for downstream consumers
                // (still operator-readable; matches the deprecated
                // family_key semantically when no other key is available).
                family_key: hypothesis.family_label.clone(),
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
                action: setup.action.to_string(),
                time_horizon: setup.horizon.primary.to_legacy_string().to_string(),
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
        for state in &world_state.perceptual_states {
            insert_node(
                perceptual_state_node_ref(&state.state_id, state.label.clone()),
                KnowledgeNodeAttributes::PerceptualState {
                    state_kind: state.state_kind.clone(),
                    trend: state.trend.clone(),
                    direction: state.direction.clone(),
                    age_ticks: state.age_ticks,
                    persistence_ticks: state.persistence_ticks,
                    direction_continuity_ticks: state.direction_continuity_ticks,
                    confidence: state.confidence,
                    strength: state.strength,
                    weighted_support_fraction: state.weighted_support_fraction,
                    count_support_fraction: state.count_support_fraction,
                    support_weight: state.support_weight,
                    contradict_weight: state.contradict_weight,
                    conflict_age_ticks: state.conflict_age_ticks,
                },
            );
            for evidence in state
                .supporting_evidence
                .iter()
                .chain(state.opposing_evidence.iter())
                .chain(state.missing_evidence.iter())
            {
                insert_node(
                    perceptual_evidence_node_ref(&evidence.evidence_id, evidence.rationale.clone()),
                    KnowledgeNodeAttributes::PerceptualEvidence {
                        channel: evidence.channel.clone(),
                        polarity: evidence.polarity.to_string(),
                        weight: evidence.weight,
                        rationale: evidence.rationale.clone(),
                    },
                );
            }
            for expectation in &state.expectations {
                insert_node(
                    perceptual_expectation_node_ref(
                        &expectation.expectation_id,
                        expectation.rationale.clone(),
                    ),
                    KnowledgeNodeAttributes::PerceptualExpectation {
                        expectation_kind: expectation.kind.to_string(),
                        expectation_status: expectation.status.to_string(),
                        pending_ticks: expectation.pending_ticks,
                        rationale: expectation.rationale.clone(),
                    },
                );
            }
            for allocation in &state.attention_allocations {
                insert_node(
                    attention_allocation_node_ref(
                        &allocation.allocation_id,
                        format!("{} {}", state.label, allocation.channel),
                    ),
                    KnowledgeNodeAttributes::AttentionAllocation {
                        channel: allocation.channel.clone(),
                        weight: allocation.weight,
                        rationale: allocation.rationale.clone(),
                    },
                );
            }
            for uncertainty in &state.uncertainties {
                insert_node(
                    perceptual_uncertainty_node_ref(
                        &uncertainty.uncertainty_id,
                        uncertainty.rationale.clone(),
                    ),
                    KnowledgeNodeAttributes::PerceptualUncertainty {
                        level: uncertainty.level,
                        rationale: uncertainty.rationale.clone(),
                        degraded_channels: uncertainty.degraded_channels.clone(),
                    },
                );
            }
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
