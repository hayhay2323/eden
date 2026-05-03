// 2026-05-01: DEPRECATED. Produces legacy agent_recommendations.json
// using 1990s breadth + sector-synchrony heuristics. Does NOT consume
// eden's emergence/contrast/lead-lag/surprise streams.
//
// Per the eden thesis (memory/eden_unified_thesis.md): eden does NOT
// decide. Eden perceives; Y decides. ANY function that emits a
// "recommendation" is architecturally misplaced. This file is the most
// flagrant violation; `ontology::contracts::derive::derive_agent_recommendations`
// is the still-load-bearing "new" path that also violates FP2 but is
// migrated incrementally.
//
// Canonical Y read surfaces (use these instead):
//   * HTTP:  GET /perception/:market   -> EdenPerception JSON
//   * CLI:   eden perception <hk|us>   -> EdenPerception JSON to stdout
//   * Code:  graph.to_report(market, tick, ts, &cfg)  in PerceptionGraph
//
// Retained only for backwards-compat with existing frontend/script
// consumers. Schedule for removal once consumers migrate to /perception.
// Sync contract: docs/architecture/perception-graph-sync-contract.md.
use super::*;
mod decision_model;
mod market;
mod outcomes;
mod symbol;
use market::{build_market_recommendation, build_sector_recommendations};
pub(crate) use outcomes::{
    best_counterfactual_action, counterfactual_regret, realized_return_for_action,
    recommendation_resolution_status, resolve_market_recommendation_outcome,
    resolve_recommendation_outcome, resolve_sector_recommendation_outcome, sector_reference_value,
    symbol_mark_price,
};
use symbol::build_symbol_recommendation;
pub(crate) use symbol::{agent_bias_for_symbol, decision_alert_record, symbol_status};

pub fn build_recommendations(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
) -> AgentRecommendations {
    let has_fresh_transition = snapshot
        .recent_transitions
        .iter()
        .any(|item| item.to_tick == snapshot.tick);
    let has_fresh_notice = snapshot
        .notices
        .iter()
        .any(|item| item.tick == snapshot.tick);
    let has_active_position = snapshot
        .symbols
        .iter()
        .any(|item| item.active_position.is_some());
    let has_symbol_structure = snapshot.symbols.iter().any(|item| item.structure.is_some());
    if snapshot.active_structures.is_empty()
        && !has_symbol_structure
        && !has_fresh_transition
        && !has_fresh_notice
        && !has_active_position
    {
        let market_recommendation = build_market_recommendation(snapshot, &[]);
        let sector_recommendations = build_sector_recommendations(snapshot);
        let mut recommendations = AgentRecommendations {
            tick: snapshot.tick,
            timestamp: snapshot.timestamp.clone(),
            market: snapshot.market,
            regime_bias: snapshot.market_regime.bias.clone(),
            total: 0,
            decisions: recommendation_decisions(
                market_recommendation.clone(),
                &[],
                &sector_recommendations,
            ),
            market_recommendation,
            items: vec![],
            knowledge_links: vec![],
        };
        sync_recommendation_views(&mut recommendations);
        recommendations.knowledge_links =
            build_decision_knowledge_links(snapshot, &recommendations.decisions);
        return recommendations;
    }

    let focus_symbols = recommendation_focus_symbols(snapshot, session);
    let mut items = focus_symbols
        .into_iter()
        .filter_map(|symbol| snapshot.symbol(&symbol))
        .filter_map(|state| build_symbol_recommendation(snapshot, state))
        .collect::<Vec<_>>();

    items.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.symbol.cmp(&b.symbol)));
    let has_actionable = items.iter().any(|item| {
        !matches!(item.action.as_str(), "watch" | "ignore") || item.best_action != "wait"
    });
    if has_actionable {
        items.retain(|item| {
            !matches!(item.action.as_str(), "watch" | "ignore") || item.best_action != "wait"
        });
    }
    items.truncate(6);

    let market_recommendation = build_market_recommendation(snapshot, &items);
    let sector_recommendations = build_sector_recommendations(snapshot);
    let mut recommendations = AgentRecommendations {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
        total: 0,
        decisions: recommendation_decisions(
            market_recommendation.clone(),
            &items,
            &sector_recommendations,
        ),
        market_recommendation,
        items,
        knowledge_links: vec![],
    };
    sync_recommendation_views(&mut recommendations);
    recommendations.knowledge_links =
        build_decision_knowledge_links(snapshot, &recommendations.decisions);
    recommendations
}

fn recommendation_focus_symbols(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
) -> Vec<String> {
    let mut focus_symbols = session
        .map(|session| session.focus_symbols.clone())
        .unwrap_or_else(|| snapshot.wake.focus_symbols.clone());

    for transition in snapshot
        .recent_transitions
        .iter()
        .filter(|item| item.to_tick == snapshot.tick)
        .take(6)
    {
        push_unique(&mut focus_symbols, transition.symbol.clone());
    }
    for notice in snapshot
        .notices
        .iter()
        .filter(|item| item.tick == snapshot.tick)
        .take(6)
    {
        if let Some(symbol) = &notice.symbol {
            push_unique(&mut focus_symbols, symbol.clone());
        }
    }
    for symbol in snapshot
        .symbols
        .iter()
        .filter(|item| item.active_position.is_some())
        .take(6)
        .map(|item| item.symbol.clone())
    {
        push_unique(&mut focus_symbols, symbol);
    }
    for symbol in snapshot
        .active_structures
        .iter()
        .filter(|item| item.action != "observe")
        .take(6)
        .map(|item| item.symbol.clone())
    {
        push_unique(&mut focus_symbols, symbol);
    }

    focus_symbols.truncate(8);
    focus_symbols
}
