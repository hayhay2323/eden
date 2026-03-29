use super::*;

pub fn build_operational_snapshot(
    live_snapshot: &LiveSnapshot,
    snapshot: &AgentSnapshot,
    session: &AgentSession,
    recommendations: &AgentRecommendations,
    narration: Option<&AgentNarration>,
) -> Result<OperationalSnapshot, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    let computed_at = observed_at;

    let market_session = MarketSessionContract {
        id: MarketSessionId(format!("market_session:{}:{}", market_slug(snapshot.market), snapshot.tick)),
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        computed_at,
        market_regime: snapshot.market_regime.clone(),
        stress: snapshot.stress.clone(),
        focus_symbols: session.focus_symbols.clone(),
        should_speak: session.should_speak,
        priority: snapshot.wake.priority,
        active_thread_count: session.active_thread_count,
        wake_headline: snapshot.wake.headline.clone(),
        wake_summary: snapshot.wake.summary.clone(),
        wake_reasons: snapshot.wake.reasons.clone(),
        suggested_tools: snapshot.wake.suggested_tools.clone(),
        market_summary: narration.and_then(|item| item.market_summary_5m.clone()),
    };

    let symbols = snapshot
        .symbols
        .iter()
        .cloned()
        .map(|state| SymbolStateContract {
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
            state,
        })
        .collect::<Vec<_>>();

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
                recommendation: item,
            }
        })
        .collect::<Vec<_>>();

    let macro_events = snapshot
        .macro_events
        .iter()
        .cloned()
        .map(|event| MacroEventContract {
            id: MacroEventContractId(event.event_id.clone()),
            market: snapshot.market,
            source_tick: snapshot.tick,
            observed_at,
            event,
        })
        .collect::<Vec<_>>();

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
