use crate::ontology::objects::Market;
use super::shared::{
    alpha_horizon_label, build_us_invalidation, build_us_structure_state, primary_invalidation_rule,
    setup_family, us_events_by_symbol, us_recent_transitions, us_sector_name, us_signal_state,
};
use super::*;
use crate::pipeline::perception::build_world_state_snapshot;

fn us_structure_status(
    state: Option<&crate::pipeline::state_engine::PersistentSymbolState>,
) -> Option<String> {
    state.map(|item| item.trend.as_str().to_string())
}

fn us_status_streak(
    status: Option<&str>,
    previous_symbol: Option<&AgentSymbolState>,
) -> Option<u64> {
    let status = status?;
    let previous_structure = previous_symbol.and_then(|item| item.structure.as_ref());
    Some(
        if previous_structure.and_then(|item| item.status.as_deref()) == Some(status) {
            previous_structure
                .and_then(|item| item.status_streak)
                .unwrap_or(0)
                + 1
        } else {
            1
        },
    )
}

pub fn build_us_agent_snapshot(
    live: &LiveSnapshot,
    history: &UsTickHistory,
    reasoning: &UsReasoningSnapshot,
    _backward: &UsBackwardSnapshot,
    store: &ObjectStore,
    lineage_stats: &UsLineageStats,
    previous_agent: Option<&AgentSnapshot>,
    perception_graph: &std::sync::RwLock<crate::perception::PerceptionGraph>,
) -> AgentSnapshot {
    let latest = history
        .latest()
        .expect("US agent snapshot requires at least one tick");
    let previous_symbols = previous_agent_symbol_map(previous_agent);

    let setups = latest
        .tactical_setups
        .iter()
        .filter_map(|item| scope_symbol(&item.scope).map(|symbol| (symbol.0.as_str(), item)))
        .collect::<HashMap<_, _>>();
    let hypotheses = latest
        .hypotheses
        .iter()
        .map(|item| (item.hypothesis_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let pressures = live
        .pressures
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let positions = live
        .active_position_nodes
        .iter()
        .map(|item| (item.symbol.0.as_str(), item))
        .collect::<HashMap<_, _>>();
    let causal_by_symbol = live
        .causal_leaders
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let perception_by_symbol = live
        .symbol_states
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let events_by_symbol = us_events_by_symbol(&live.events);

    let mut symbol_keys = BTreeSet::new();
    for symbol in latest.signals.keys() {
        symbol_keys.insert(symbol.0.clone());
    }
    for key in setups.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in pressures.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in positions.keys() {
        symbol_keys.insert((*key).to_string());
    }

    let mut symbols = symbol_keys
        .into_iter()
        .map(|symbol| {
            let signal = latest.signals.get(&Symbol(symbol.clone()));
            let setup = setups.get(symbol.as_str());
            let hypothesis = setup.and_then(|item| hypotheses.get(item.hypothesis_id.as_str()));
            let pressure = pressures.get(symbol.as_str());
            let position = positions.get(symbol.as_str());
            let causal = causal_by_symbol.get(symbol.as_str());
            let persistent = perception_by_symbol.get(symbol.as_str());
            let live_case = live.tactical_cases.iter().find(|c| c.symbol == symbol);
            let backward = live.backward_chains.iter().find(|c| c.symbol == symbol);

            let status = us_structure_status(persistent.copied());
            let previous = previous_symbols.get(symbol.as_str());

            AgentSymbolState {
                symbol: symbol.clone(),
                sector: store.sector_name_for_symbol(&Symbol(symbol.clone())).map(|s| s.to_string()),
                structure: build_us_structure_state(
                    &symbol,
                    store,
                    setup.copied(),
                    hypothesis.copied(),
                    status.clone(),
                    None, // age_ticks
                    us_status_streak(status.as_deref(), previous.copied()),
                    causal.map(|item| item.current_leader.clone()),
                    live_case,
                ),
                signal: signal.map(us_signal_state),
                depth: None,
                brokers: None,
                invalidation: build_us_invalidation(setup.copied(), hypothesis.copied(), backward, live_case),
                pressure: pressure.cloned().cloned(),
                active_position: position.cloned().cloned(),
                latest_events: events_by_symbol.get(symbol.as_str()).cloned().unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();

    sort_symbol_states(&mut symbols);

    let sector_flows = build_sector_flows(&symbols);

    let recent_transitions = us_recent_transitions(history, store, 5);

    let notices = build_us_notices(
        live.tick,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.events,
        &live.cross_market_signals,
    );

    let wake = build_wake_state(
        live.market,
        live.tick,
        &notices,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.cross_market_signals,
    );

    let world_state = build_world_state_snapshot(
        live.market,
        &live.timestamp,
        &live.symbol_states,
        &live.cluster_states,
        live.world_summary.as_ref(),
    );

    let macro_event_candidates = merge_macro_event_candidates(
        build_macro_event_candidates(
            live.tick,
            live.market,
            &live.market_regime,
            &live.stress,
            &wake,
            &notices,
            &sector_flows,
            &symbols,
            &live.cross_market_signals,
        ),
        build_world_monitor_macro_event_candidates(
            live.tick,
            live.market,
            &live.market_regime,
            &live.stress,
            &sector_flows,
            &symbols,
            &live.cross_market_signals,
        ),
    );
    let macro_events = promote_macro_events(&live.market_regime, &live.stress, &macro_event_candidates);
    let knowledge_links = build_macro_event_knowledge_links(&macro_events);

    let world_regime = crate::agent::context::world_state_regime(&world_state).to_string();

    let perception = {
        let graph = perception_graph.read().unwrap();
        let mut report = graph.to_report(
            Market::Us,
            live.tick,
            live.timestamp.clone(),
            &crate::agent::PerceptionFilterConfig::default(),
        );
        // Signature Replay remains file-based for now (historical query)
        report.signature_replays = crate::pipeline::signature_replay::read_latest_signature_replays(
            "us",
            live.tick,
            20,
        );
        report
    };

    AgentSnapshot {
        tick: live.tick,
        timestamp: live.timestamp.clone(),
        market: live.market,
        market_regime: live.market_regime.clone(),
        stress: live.stress.clone(),
        wake,
        world_state: Some(world_state),
        backward_reasoning: None,
        perception: Some(perception),
        notices,
        active_structures: collect_active_structures(&symbols),
        recent_transitions,
        investigation_selections: reasoning.investigation_selections.clone(),
        sector_flows,
        symbols,
        perception_states: live.symbol_states.iter().map(AgentPerceptionState::from_persistent).collect(),
        events: live.events.clone(),
        cross_market_signals: live.cross_market_signals.clone(),
        raw_sources: live.raw_sources.clone(),
        context_priors: current_us_context_priors(lineage_stats, latest.timestamp, &world_regime),
        macro_event_candidates,
        macro_events,
        knowledge_links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::types::state::AgentSymbolState;

    fn previous_symbol_with_status(status: &str, streak: u64) -> AgentSymbolState {
        let mut symbol = AgentSymbolState::new("AAPL.US");
        symbol.structure = Some(AgentStructureState {
            symbol: "AAPL.US".to_string(),
            sector: None,
            setup_id: None,
            title: "".to_string(),
            action: "".to_string(),
            status: Some(status.to_string()),
            status_streak: Some(streak),
            confidence: Decimal::ZERO,
            confidence_change: None,
            confidence_gap: None,
            transition_reason: None,
            current_leader: None,
            leader_streak: None,
            leader_transition_summary: None,
            thesis_family: None,
            action_expectancies: Vec::new(),
            expected_net_alpha: None,
            alpha_horizon: None,
            invalidation_rule: None,
            contest_state: None,
        });
        symbol
    }

    #[test]
    fn us_status_streak_increments_for_matching_status() {
        let previous = previous_symbol_with_status("stable", 4);
        assert_eq!(
            us_status_streak(Some("stable"), Some(&previous)),
            Some(5)
        );
    }

    #[test]
    fn us_status_streak_resets_after_status_change() {
        let previous = previous_symbol_with_status("stable", 4);
        assert_eq!(
            us_status_streak(Some("weakening"), Some(&previous)),
            Some(1)
        );
    }
}
