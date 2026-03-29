use super::*;
use super::shared::{
    alpha_horizon_label, build_us_invalidation, setup_family, us_events_by_symbol,
    us_recent_transitions, us_sector_name, us_signal_state, primary_invalidation_rule,
};

pub fn build_us_agent_snapshot(
    live: &LiveSnapshot,
    history: &UsTickHistory,
    reasoning: &UsReasoningSnapshot,
    _backward: &UsBackwardSnapshot,
    store: &ObjectStore,
    lineage_stats: &UsLineageStats,
    previous_agent: Option<&AgentSnapshot>,
) -> AgentSnapshot {
    let latest = history
        .latest()
        .expect("US agent snapshot requires at least one tick");
    let _previous_tick = history.latest_n(2).into_iter().rev().nth(1);
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
    let backward_by_symbol = live
        .backward_chains
        .iter()
        .map(|item| (item.symbol.as_str(), item))
        .collect::<HashMap<_, _>>();
    let causal_by_symbol = live
        .causal_leaders
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
    for key in backward_by_symbol.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in causal_by_symbol.keys() {
        symbol_keys.insert((*key).to_string());
    }

    let mut symbols = symbol_keys
        .into_iter()
        .map(|symbol| {
            let current_signal = latest.signals.get(&Symbol(symbol.clone()));
            let _prev_symbol = previous_symbols.get(symbol.as_str()).copied();
            let setup = setups.get(symbol.as_str()).copied();
            let hypothesis =
                setup.and_then(|item| hypotheses.get(item.hypothesis_id.as_str()).copied());
            let context_prior = hypothesis.and_then(|item| {
                best_us_context_prior(
                    item.family_key.as_str(),
                    latest.timestamp,
                    latest.market_regime.as_str(),
                    lineage_stats,
                )
            });
            let structure = setup.map(|item| AgentStructureState {
                symbol: symbol.clone(),
                sector: us_sector_name(store, &symbol),
                setup_id: Some(item.setup_id.clone()),
                title: item.title.clone(),
                action: item.action.clone(),
                status: None,
                age_ticks: None,
                status_streak: None,
                confidence: item.confidence,
                confidence_change: None,
                confidence_gap: Some(item.confidence_gap),
                transition_reason: None,
                contest_state: None,
                current_leader: causal_by_symbol
                    .get(symbol.as_str())
                    .map(|item| item.current_leader.clone()),
                leader_streak: causal_by_symbol
                    .get(symbol.as_str())
                    .map(|item| item.leader_streak),
                leader_transition_summary: backward_by_symbol
                    .get(symbol.as_str())
                    .map(|item| item.primary_driver.clone()),
                thesis_family: hypothesis
                    .map(|item| item.family_label.clone())
                    .or_else(|| setup_family(item)),
                action_expectancies: context_prior
                    .as_ref()
                    .map(|item| item.action_expectancies.clone())
                    .unwrap_or_default(),
                expected_net_alpha: context_prior.as_ref().map(|item| item.expected_net_alpha),
                alpha_horizon: Some(alpha_horizon_label(item.time_horizon.as_str(), 10)),
                invalidation_rule: primary_invalidation_rule(
                    hypothesis,
                    backward_by_symbol
                        .get(symbol.as_str())
                        .map(|item| item.primary_driver.as_str()),
                    Some(item),
                ),
            });

            AgentSymbolState {
                symbol: symbol.clone(),
                sector: us_sector_name(store, &symbol),
                structure,
                signal: current_signal.map(us_signal_state),
                depth: None,
                brokers: None,
                invalidation: build_us_invalidation(
                    setup,
                    hypothesis,
                    backward_by_symbol.get(symbol.as_str()).copied(),
                ),
                pressure: pressures.get(symbol.as_str()).cloned().cloned(),
                active_position: positions.get(symbol.as_str()).cloned().cloned(),
                latest_events: events_by_symbol
                    .get(symbol.as_str())
                    .cloned()
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();

    let _ = reasoning;

    sort_symbol_states(&mut symbols);
    let active_structures = collect_active_structures(&symbols);
    let recent_transitions = us_recent_transitions(history, store, 32);
    let sector_flows = build_sector_flows(&symbols);
    let notices = build_us_notices(
        live.tick,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.events,
        &live.cross_market_signals,
    );
    let wake = build_wake_state(
        live.tick,
        &notices,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.cross_market_signals,
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
    let macro_events =
        promote_macro_events(&live.market_regime, &live.stress, &macro_event_candidates);
    let knowledge_links = build_macro_event_knowledge_links(&macro_events);

    AgentSnapshot {
        tick: live.tick,
        timestamp: live.timestamp.clone(),
        market: live.market,
        market_regime: live.market_regime.clone(),
        stress: live.stress.clone(),
        wake,
        world_state: None,
        backward_reasoning: None,
        notices,
        active_structures,
        recent_transitions,
        sector_flows,
        symbols,
        events: live.events.clone(),
        cross_market_signals: live.cross_market_signals.clone(),
        context_priors: current_us_context_priors(
            lineage_stats,
            latest.timestamp,
            latest.market_regime.as_str(),
        ),
        macro_event_candidates,
        macro_events,
        knowledge_links,
    }
}
