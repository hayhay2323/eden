use super::*;

fn combined_knowledge_links(
    snapshot: &AgentSnapshot,
    recommendations: &AgentRecommendations,
) -> Vec<AgentKnowledgeLink> {
    let mut seen = std::collections::HashSet::new();
    snapshot
        .knowledge_links
        .iter()
        .cloned()
        .chain(recommendations.knowledge_links.iter().cloned())
        .filter(|item| seen.insert(item.link_id.to_ascii_lowercase()))
        .collect()
}

fn graph_node_endpoint(market: LiveMarket, node_id: &str) -> String {
    format!("/api/ontology/{}/graph/node/{node_id}", market_slug(market))
}

pub(crate) fn object_endpoint(market: LiveMarket, path: &str) -> String {
    format!("/api/ontology/{}/{}", market_slug(market), path)
}

pub(crate) fn symbol_contract_id(market: LiveMarket, source_tick: u64, symbol: &str) -> String {
    format!(
        "symbol_state:{}:{}:{}",
        market_slug(market),
        normalized_symbol_id(symbol),
        source_tick
    )
}

pub(crate) fn symbol_object_ref(
    market: LiveMarket,
    source_tick: u64,
    symbol: &str,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: symbol_contract_id(market, source_tick, symbol),
        kind: OperationalObjectKind::SymbolState,
        endpoint: object_endpoint(market, &format!("symbols/{symbol}")),
        label: Some(symbol.into()),
    }
}

pub(crate) fn case_object_ref(
    market: LiveMarket,
    case_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: case_id.into(),
        kind: OperationalObjectKind::Case,
        endpoint: object_endpoint(market, &format!("cases/{case_id}")),
        label,
    }
}

pub(crate) fn recommendation_object_ref(
    market: LiveMarket,
    recommendation_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: recommendation_id.into(),
        kind: OperationalObjectKind::Recommendation,
        endpoint: object_endpoint(market, &format!("recommendations/{recommendation_id}")),
        label,
    }
}

pub(crate) fn workflow_object_ref(
    market: LiveMarket,
    workflow_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: workflow_id.into(),
        kind: OperationalObjectKind::Workflow,
        endpoint: object_endpoint(market, &format!("workflows/{workflow_id}")),
        label,
    }
}

fn symbol_graph_ref(market: LiveMarket, symbol: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::symbol_node_id(symbol);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Symbol,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn setup_graph_ref(market: LiveMarket, setup_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::setup_node_id(setup_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Setup,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn decision_graph_ref(market: LiveMarket, recommendation_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::decision_node_id(recommendation_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Decision,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn macro_event_graph_ref(market: LiveMarket, event_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::macro_event_node_id(event_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::MacroEvent,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

pub(crate) fn operational_history_ref(
    key: impl Into<String>,
    endpoint: impl Into<String>,
) -> OperationalHistoryRef {
    OperationalHistoryRef {
        key: key.into(),
        endpoint: endpoint.into(),
        count: None,
        latest_at: None,
    }
}

pub(crate) fn case_history_refs(
    market: LiveMarket,
    case_id: &str,
    setup_id: &str,
    workflow_id: Option<&str>,
) -> CaseHistoryRefs {
    let market = market_slug(market);
    CaseHistoryRefs {
        workflow: workflow_id.map(|workflow_id| {
            operational_history_ref(
                workflow_id,
                format!("/api/ontology/{market}/cases/{case_id}/history/workflow"),
            )
        }),
        reasoning: Some(operational_history_ref(
            setup_id,
            format!("/api/ontology/{market}/cases/{case_id}/history/reasoning"),
        )),
        outcomes: Some(operational_history_ref(
            setup_id,
            format!("/api/ontology/{market}/cases/{case_id}/history/outcomes"),
        )),
    }
}

pub(crate) fn recommendation_history_refs(
    market: LiveMarket,
    recommendation_id: &str,
    related_case_id: Option<&str>,
    related_workflow_id: Option<&str>,
) -> RecommendationHistoryRefs {
    let market = market_slug(market);
    RecommendationHistoryRefs {
        journal: Some(operational_history_ref(
            recommendation_id,
            format!("/api/ontology/{market}/recommendations/{recommendation_id}/history"),
        )),
        workflow: related_workflow_id.map(|workflow_id| {
            operational_history_ref(
                workflow_id,
                format!("/api/ontology/{market}/workflows/{workflow_id}/history"),
            )
        }),
        outcomes: related_case_id.map(|case_id| {
            operational_history_ref(
                case_id,
                format!("/api/ontology/{market}/cases/{case_id}/history/outcomes"),
            )
        }),
    }
}

pub(crate) fn workflow_history_refs(
    market: LiveMarket,
    workflow_id: &str,
) -> WorkflowHistoryRefs {
    let market = market_slug(market);
    WorkflowHistoryRefs {
        events: Some(operational_history_ref(
            workflow_id,
            format!("/api/ontology/{market}/workflows/{workflow_id}/history"),
        )),
    }
}

pub fn build_market_session_contract(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    narration: Option<&AgentNarration>,
) -> Result<MarketSessionContract, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    let computed_at = observed_at;
    Ok(MarketSessionContract {
        id: MarketSessionId(format!(
            "market_session:{}:{}",
            market_slug(snapshot.market),
            snapshot.tick
        )),
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        computed_at,
        market_regime: snapshot.market_regime.clone(),
        stress: snapshot.stress.clone(),
        focus_symbols: session
            .map(|item| item.focus_symbols.clone())
            .unwrap_or_else(|| snapshot.wake.focus_symbols.clone()),
        should_speak: session
            .map(|item| item.should_speak)
            .unwrap_or(snapshot.wake.should_speak),
        priority: snapshot.wake.priority,
        active_thread_count: session.map(|item| item.active_thread_count).unwrap_or(0),
        wake_headline: snapshot.wake.headline.clone(),
        wake_summary: snapshot.wake.summary.clone(),
        wake_reasons: snapshot.wake.reasons.clone(),
        suggested_tools: snapshot.wake.suggested_tools.clone(),
        market_summary: narration.and_then(|item| item.market_summary_5m.clone()),
        focus_symbol_refs: session
            .map(|item| {
                item.focus_symbols
                    .iter()
                    .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                    .collect()
            })
            .unwrap_or_else(|| {
                snapshot
                    .wake
                    .focus_symbols
                    .iter()
                    .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                    .collect()
            }),
    })
}

pub fn build_symbol_state_contract(
    snapshot: &AgentSnapshot,
    state: &AgentSymbolState,
) -> Result<SymbolStateContract, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    Ok(SymbolStateContract {
        id: SymbolStateId(format!(
            "symbol_state:{}:{}:{}",
            market_slug(snapshot.market),
            normalized_symbol_id(&state.symbol),
            snapshot.tick
        )),
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        symbol: state.symbol.clone(),
        sector: state.sector.clone(),
        graph_ref: symbol_graph_ref(snapshot.market, &state.symbol),
        state: state.clone(),
    })
}

pub fn build_macro_event_contracts(
    snapshot: &AgentSnapshot,
) -> Result<Vec<MacroEventContract>, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    Ok(snapshot
        .macro_events
        .iter()
        .cloned()
        .map(|event| MacroEventContract {
            id: MacroEventContractId(event.event_id.clone()),
            market: snapshot.market,
            source_tick: snapshot.tick,
            observed_at,
            graph_ref: macro_event_graph_ref(snapshot.market, &event.event_id),
            event,
        })
        .collect())
}

pub fn build_operational_snapshot(
    live_snapshot: &LiveSnapshot,
    snapshot: &AgentSnapshot,
    session: &AgentSession,
    recommendations: &AgentRecommendations,
    narration: Option<&AgentNarration>,
) -> Result<OperationalSnapshot, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    let computed_at = observed_at;

    let market_session = build_market_session_contract(snapshot, Some(session), narration)?;

    let symbols = snapshot
        .symbols
        .iter()
        .map(|state| build_symbol_state_contract(snapshot, state))
        .collect::<Result<Vec<_>, _>>()?;

    let case_summaries = build_case_summaries(live_snapshot);
    let recommendation_links = link_recommendations_to_cases(&case_summaries, &recommendations.items);
    let cases = case_summaries
        .iter()
        .map(|item| CaseContract {
            id: CaseContractId(item.case_id.clone()),
            setup_id: item.setup_id.clone(),
            market: item.market,
            source_tick: snapshot.tick,
            observed_at,
            symbol: item.symbol.clone(),
            sector: item.sector.clone(),
            title: item.title.clone(),
            action: item.recommended_action.clone(),
            workflow_state: item.workflow_state.clone(),
            workflow_id: Some(case_workflow_key(item)),
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
            owner: item.owner.clone(),
            reviewer: item.reviewer.clone(),
            queue_pin: item.queue_pin.clone(),
            confidence: item.confidence,
            confidence_gap: Some(item.confidence_gap),
            thesis_family: item.family_label.clone(),
            current_leader: item.current_leader.clone(),
            invalidation_rule: item.invalidation_rules.first().cloned(),
            expected_net_alpha: None,
            alpha_horizon: None,
            recommendation_ids: recommendation_links
                .iter()
                .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                .map(|(rec_id, _, _, _)| rec_id.clone())
                .collect(),
            symbol_ref: symbol_object_ref(item.market, snapshot.tick, &item.symbol),
            workflow_ref: item
                .workflow_id
                .as_deref()
                .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
            recommendation_refs: recommendation_links
                .iter()
                .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                .map(|(rec_id, _, _, _)| {
                    recommendation_object_ref(
                        item.market,
                        rec_id,
                        Some(format!("{} {}", item.symbol, item.recommended_action)),
                    )
                })
                .collect(),
            graph_ref: setup_graph_ref(item.market, &item.setup_id),
            history_refs: case_history_refs(
                item.market,
                &item.case_id,
                &item.setup_id,
                item.workflow_id.as_deref(),
            ),
        })
        .collect::<Vec<_>>();

    let recommendation_contracts = recommendations
        .items
        .iter()
        .cloned()
        .map(|item| {
            let linkage = recommendation_links
                .iter()
                .find(|(rec_id, _, _, _)| rec_id == &item.recommendation_id);
            RecommendationContract {
                id: RecommendationContractId(item.recommendation_id.clone()),
                market: snapshot.market,
                source_tick: snapshot.tick,
                observed_at,
                symbol: item.symbol.clone(),
                related_case_id: linkage.and_then(|(_, case_id, _, _)| case_id.clone()),
                related_setup_id: linkage.and_then(|(_, _, setup_id, _)| setup_id.clone()),
                related_workflow_id: linkage.and_then(|(_, _, _, workflow_id)| workflow_id.clone()),
                symbol_ref: symbol_object_ref(snapshot.market, snapshot.tick, &item.symbol),
                case_ref: linkage.and_then(|(_, case_id, _, _)| {
                    case_id.as_ref().map(|case_id| {
                        case_object_ref(snapshot.market, case_id, item.title.clone())
                    })
                }),
                workflow_ref: linkage.and_then(|(_, _, _, workflow_id)| {
                    workflow_id
                        .as_ref()
                        .map(|workflow_id| workflow_object_ref(snapshot.market, workflow_id, None))
                }),
                graph_ref: decision_graph_ref(snapshot.market, &item.recommendation_id),
                history_refs: recommendation_history_refs(
                    snapshot.market,
                    &item.recommendation_id,
                    linkage.and_then(|(_, case_id, _, _)| case_id.as_deref()),
                    linkage.and_then(|(_, _, _, workflow_id)| workflow_id.as_deref()),
                ),
                recommendation: item,
            }
        })
        .collect::<Vec<_>>();

    let macro_events = build_macro_event_contracts(snapshot)?;

    let threads = session
        .active_threads
        .iter()
        .cloned()
        .map(|thread| ThreadContract {
            id: ThreadContractId(format!(
                "thread:{}:{}",
                market_slug(snapshot.market),
                normalized_symbol_id(&thread.symbol)
            )),
            market: snapshot.market,
            source_tick: snapshot.tick,
            observed_at,
            thread,
        })
        .collect::<Vec<_>>();

    let workflows = build_workflow_contracts(
        snapshot.market,
        snapshot.tick,
        observed_at,
        &cases,
        &recommendation_contracts,
    );
    let sidecars = OperationalSidecars {
        sector_flows: snapshot.sector_flows.clone(),
        backward_investigations: snapshot
            .backward_reasoning
            .as_ref()
            .map(|item| item.investigations.clone())
            .unwrap_or_default(),
        world_state: snapshot.world_state.clone(),
        macro_event_candidates: snapshot.macro_event_candidates.clone(),
        knowledge_links: combined_knowledge_links(snapshot, recommendations),
    };

    Ok(OperationalSnapshot {
        version: 1,
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        computed_at,
        market_session,
        recent_turns: session.recent_turns.clone(),
        notices: snapshot.notices.clone(),
        recent_transitions: snapshot.recent_transitions.clone(),
        symbols,
        cases,
        market_recommendation: recommendations.market_recommendation.clone(),
        sector_recommendations: recommendations
            .decisions
            .iter()
            .filter_map(|item| {
                if let AgentDecision::Sector(item) = item {
                    Some(item.clone())
                } else {
                    None
                }
            })
            .collect(),
        recommendations: recommendation_contracts,
        macro_events,
        threads,
        workflows,
        sidecars,
        events: snapshot.events.clone(),
    })
}

pub fn rebuild_operational_case_graph(
    snapshot: &mut OperationalSnapshot,
    cases: &[CaseSummary],
) {
    let recommendation_links = link_recommendations_to_cases(
        cases,
        &snapshot
            .recommendations
            .iter()
            .map(|item| item.recommendation.clone())
            .collect::<Vec<_>>(),
    );

    snapshot.cases = cases
        .iter()
        .map(|item| CaseContract {
            id: CaseContractId(item.case_id.clone()),
            setup_id: item.setup_id.clone(),
            market: item.market,
            source_tick: snapshot.source_tick,
            observed_at: snapshot.observed_at,
            symbol: item.symbol.clone(),
            sector: item.sector.clone(),
            title: item.title.clone(),
            action: item.recommended_action.clone(),
            workflow_state: item.workflow_state.clone(),
            workflow_id: Some(case_workflow_key(item)),
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
            owner: item.owner.clone(),
            reviewer: item.reviewer.clone(),
            queue_pin: item.queue_pin.clone(),
            confidence: item.confidence,
            confidence_gap: Some(item.confidence_gap),
            thesis_family: item.family_label.clone(),
            current_leader: item.current_leader.clone(),
            invalidation_rule: item.invalidation_rules.first().cloned(),
            expected_net_alpha: None,
            alpha_horizon: None,
            recommendation_ids: recommendation_links
                .iter()
                .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                .map(|(rec_id, _, _, _)| rec_id.clone())
                .collect(),
            symbol_ref: symbol_object_ref(item.market, snapshot.source_tick, &item.symbol),
            workflow_ref: item
                .workflow_id
                .as_deref()
                .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
            recommendation_refs: recommendation_links
                .iter()
                .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                .map(|(rec_id, _, _, _)| {
                    recommendation_object_ref(
                        item.market,
                        rec_id,
                        Some(format!("{} {}", item.symbol, item.recommended_action)),
                    )
                })
                .collect(),
            graph_ref: setup_graph_ref(item.market, &item.setup_id),
            history_refs: case_history_refs(
                item.market,
                &item.case_id,
                &item.setup_id,
                item.workflow_id.as_deref(),
            ),
        })
        .collect();

    for recommendation in &mut snapshot.recommendations {
        if let Some((_, case_id, setup_id, workflow_id)) = recommendation_links
            .iter()
            .find(|(rec_id, _, _, _)| rec_id == &recommendation.id.0)
        {
            recommendation.related_case_id = case_id.clone();
            recommendation.related_setup_id = setup_id.clone();
            recommendation.related_workflow_id = workflow_id.clone();
            recommendation.case_ref = case_id.as_ref().map(|case_id| {
                case_object_ref(snapshot.market, case_id, recommendation.recommendation.title.clone())
            });
            recommendation.workflow_ref = workflow_id.as_ref().map(|workflow_id| {
                workflow_object_ref(snapshot.market, workflow_id, None)
            });
            recommendation.history_refs = recommendation_history_refs(
                snapshot.market,
                &recommendation.id.0,
                case_id.as_deref(),
                workflow_id.as_deref(),
            );
        }
    }

    snapshot.workflows = build_workflow_contracts(
        snapshot.market,
        snapshot.source_tick,
        snapshot.observed_at,
        &snapshot.cases,
        &snapshot.recommendations,
    );
}
