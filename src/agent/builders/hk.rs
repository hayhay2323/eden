use crate::ontology::objects::Market;
use super::shared::{
    build_hk_depth_state, build_hk_invalidation, build_hk_structure_state, hk_events_by_symbol,
    hk_recent_transitions, hk_signal_state, institutions_by_symbol,
};
use super::*;
use crate::pipeline::perception::build_world_state_snapshot;

pub fn build_hk_agent_snapshot(
    live: &LiveSnapshot,
    history: &TickHistory,
    links: &LinkSnapshot,
    store: &ObjectStore,
    lineage_priors: &[FamilyContextLineageOutcome],
    previous_agent: Option<&AgentSnapshot>,
    perception_graph: &std::sync::RwLock<crate::perception::PerceptionGraph>,
) -> AgentSnapshot {
    let latest = history
        .latest()
        .expect("HK agent snapshot requires at least one tick");
    let world_state = build_world_state_snapshot(
        live.market,
        &live.timestamp,
        &live.symbol_states,
        &live.cluster_states,
        live.world_summary.as_ref(),
    );
    let world_regime = crate::agent::context::world_state_regime(&world_state).to_string();
    let previous_tick = history.latest_n(2).into_iter().rev().nth(1);
    let previous_symbols = previous_agent_symbol_map(previous_agent);
    let live_cases = live
        .tactical_cases
        .iter()
        .map(|item| (item.setup_id.as_str(), item))
        .collect::<HashMap<_, _>>();

    let hypotheses = latest
        .hypotheses
        .iter()
        .map(|item| (item.hypothesis_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let setups = latest
        .tactical_setups
        .iter()
        .filter_map(|item| scope_symbol(&item.scope).map(|symbol| (symbol.0.as_str(), item)))
        .collect::<HashMap<_, _>>();
    let tracks = latest
        .hypothesis_tracks
        .iter()
        .filter_map(|item| scope_symbol(&item.scope).map(|symbol| (symbol.0.as_str(), item)))
        .collect::<HashMap<_, _>>();
    let backward = latest
        .backward_reasoning
        .investigations
        .iter()
        .filter_map(|item| scope_symbol(&item.leaf_scope).map(|symbol| (symbol.0.as_str(), item)))
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
    let order_books = links
        .order_books
        .iter()
        .map(|item| (item.symbol.0.as_str(), item))
        .collect::<HashMap<_, _>>();
    let institutions = institutions_by_symbol(&links.institution_activities, store);
    let events_by_symbol = hk_events_by_symbol(&live.events);

    let mut symbol_keys = BTreeSet::new();
    for symbol in latest.signals.keys() {
        symbol_keys.insert(symbol.0.clone());
    }
    for key in setups.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in tracks.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in backward.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in pressures.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in positions.keys() {
        symbol_keys.insert((*key).to_string());
    }
    for key in institutions.keys() {
        symbol_keys.insert(key.clone());
    }

    let mut symbols = symbol_keys
        .into_iter()
        .map(|symbol| {
            let current_signal = latest.signals.get(&Symbol(symbol.clone()));
            let previous_signal =
                previous_tick.and_then(|tick| tick.signals.get(&Symbol(symbol.clone())));
            let prev_symbol = previous_symbols.get(symbol.as_str()).copied();
            let setup = setups.get(symbol.as_str()).copied();
            let track = tracks.get(symbol.as_str()).copied();
            let backward = backward.get(symbol.as_str()).copied();
            let hypothesis =
                setup.and_then(|item| hypotheses.get(item.hypothesis_id.as_str()).copied());
            let live_case = setup.and_then(|item| live_cases.get(item.setup_id.as_str()).copied());
            let context_prior = hypothesis.and_then(|item| {
                best_hk_context_prior(
                    item.family_label.as_str(),
                    latest.timestamp,
                    &world_regime,
                    lineage_priors,
                )
            });
            let depth = match (current_signal, order_books.get(symbol.as_str()).copied()) {
                (Some(signal), Some(order_book)) => Some(build_hk_depth_state(
                    signal,
                    previous_signal,
                    order_book,
                    prev_symbol.and_then(|item| item.depth.as_ref()),
                )),
                _ => None,
            };
            let brokers = institutions.get(symbol.as_str()).map(|current| {
                build_broker_state(current, prev_symbol.and_then(|item| item.brokers.as_ref()))
            });

            AgentSymbolState {
                sector: store
                    .sector_name_for_symbol(&Symbol(symbol.clone()))
                    .map(str::to_string),
                structure: build_hk_structure_state(
                    &symbol,
                    store,
                    setup,
                    track,
                    backward,
                    hypothesis,
                    context_prior.as_ref(),
                    live_case,
                ),
                signal: current_signal.map(hk_signal_state),
                depth,
                brokers,
                invalidation: build_hk_invalidation(track, hypothesis, backward, setup, live_case),
                pressure: pressures.get(symbol.as_str()).cloned().cloned(),
                active_position: positions.get(symbol.as_str()).cloned().cloned(),
                latest_events: events_by_symbol
                    .get(symbol.as_str())
                    .cloned()
                    .unwrap_or_default(),
                symbol,
            }
        })
        .collect::<Vec<_>>();

    sort_symbol_states(&mut symbols);
    let active_structures = collect_active_structures(&symbols);
    let recent_transitions = hk_recent_transitions(history, store, 32);
    let sector_flows = build_sector_flows(&symbols);
    let notices = build_hk_notices(
        live.tick,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.events,
    );
    let perception_states = live
        .symbol_states
        .iter()
        .map(AgentPerceptionState::from_persistent)
        .collect::<Vec<_>>();
    let mut wake = build_wake_state(
        live.market,
        live.tick,
        &notices,
        &recent_transitions,
        &symbols,
        &sector_flows,
        &live.cross_market_signals,
    );
    // Surface Y#2 (cluster/world persistent state) and Y#3 (absence first-class)
    // signals into the wake reasons so they reach operator/LLM. Without this the
    // whole persistent-state engine is invisible above the case layer.
    let perception_reasons = crate::agent::attention::derive_perception_reasons(
        live.world_summary.as_ref(),
        &live.cluster_states,
        &perception_states,
    );
    wake.reasons.extend(perception_reasons);
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

    let perception = {
        let graph = perception_graph.read().unwrap();
        let mut report = graph.to_report(
            Market::Hk,
            live.tick,
            live.timestamp.clone(),
            &crate::agent::PerceptionFilterConfig::default(),
        );
        // Signature Replay remains file-based for now (historical query)
        report.signature_replays = crate::pipeline::signature_replay::read_latest_signature_replays(
            "hk",
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
        backward_reasoning: Some(latest.backward_reasoning.clone()),
        perception: Some(perception),
        notices,
        active_structures,
        recent_transitions,
        investigation_selections: vec![],
        sector_flows,
        symbols,
        perception_states,
        events: live.events.clone(),
        cross_market_signals: live.cross_market_signals.clone(),
        raw_sources: live.raw_sources.clone(),
        context_priors: current_hk_context_priors(lineage_priors, latest.timestamp, &world_regime),
        macro_event_candidates,
        macro_events,
        knowledge_links,
    }
}
