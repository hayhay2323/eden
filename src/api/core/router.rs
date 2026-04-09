use axum::middleware;
use axum::routing::{get, post};
use axum::Router;

use super::super::agent_api::{
    get_agent_analysis, get_agent_analyst_review, get_agent_analyst_scoreboard, get_agent_backward,
    get_agent_briefing, get_agent_brokers, get_agent_depth, get_agent_eod_review,
    get_agent_invalidation, get_agent_narration, get_agent_notices, get_agent_query,
    get_agent_recommendations, get_agent_scoreboard, get_agent_sector_flows, get_agent_session,
    get_agent_snapshot, get_agent_structure, get_agent_structures, get_agent_symbol,
    get_agent_thread, get_agent_threads, get_agent_tools, get_agent_transitions, get_agent_turns,
    get_agent_wake, get_agent_watchlist, get_agent_world, post_agent_analyze,
    post_agent_analyze_codex_cli,
};
use super::super::agent_graph::{
    get_agent_graph_links, get_agent_graph_node, get_agent_knowledge_link_history,
    get_agent_knowledge_link_state, get_agent_macro_event_history, get_agent_macro_event_state,
};
use super::super::agent_surface::{
    stream_agent_analysis, stream_agent_analyst_review, stream_agent_analyst_scoreboard,
    stream_agent_briefing, stream_agent_eod_review, stream_agent_narration,
    stream_agent_recommendations, stream_agent_scoreboard, stream_agent_session,
    stream_agent_snapshot, stream_agent_threads, stream_agent_turns, stream_agent_wake,
    stream_agent_watchlist,
};
use super::super::archive::{
    get_archive_capital_flows, get_archive_order_books, get_archive_trades, get_symbol_history,
};
use super::super::case_api::{
    get_case_briefing, get_case_detail, get_case_mechanism_story, get_case_review,
    get_case_transition_analytics, get_cases, stream_case_briefing, stream_case_detail,
    stream_case_mechanism_story, stream_case_review, stream_case_transition_analytics,
    stream_cases,
};
use super::super::case_workflow_api::{
    post_case_assign, post_case_queue_pin, post_case_transition,
};
use super::super::context_api::get_context_status;
#[cfg(feature = "coordinator")]
use super::super::coordinator_api::get_coordinator_snapshot;
use super::super::feed_api::{get_feed_notices, get_feed_transitions};
use super::super::feed_surface::{stream_feed_notices, stream_feed_transitions};
use super::super::foundation::{ApiError, ApiState};
use super::super::lineage_api::{
    get_causal_flips, get_causal_timeline, get_lineage, get_lineage_history, get_lineage_rows,
    get_us_causal_flips, get_us_causal_timeline, get_us_lineage, get_us_lineage_history,
    get_us_lineage_rows,
};
use super::super::ontology_api::{
    get_backward_investigation_sidecar, get_case_contract, get_case_contracts,
    get_macro_event_contract, get_macro_event_contracts, get_market_session_contract,
    get_operational_navigation, get_operational_neighborhood, get_operational_snapshot,
    get_operator_work_item_sidecars, get_recommendation_contract, get_recommendation_contracts,
    get_sector_flow_sidecars, get_symbol_state_contract, get_symbol_state_contracts,
    get_thread_contract, get_workflow_contract,
};
use super::super::ontology_graph_api::{
    get_graph_links, get_graph_node, get_knowledge_link_history, get_knowledge_link_state,
    get_macro_event_history, get_macro_event_state,
};
use super::super::ontology_history_api::{
    get_case_outcome_history, get_case_reasoning_history, get_case_workflow_history,
    get_recommendation_journal_history, get_workflow_event_history,
};
use super::super::ontology_query_api::{
    get_ontology_knowledge_links, get_ontology_macro_event_candidates, get_ontology_world,
};
use super::super::ontology_query_surface::stream_ontology_world;
use super::super::runtime_tasks_api::{
    get_runtime_task, get_runtime_tasks, post_runtime_task, post_runtime_task_status,
};
use super::auth::{audit_request, build_cors_layer, require_api_key};
use super::health::{
    get_live_snapshot, get_polymarket, get_us_live_snapshot, health, health_report,
};

pub(in crate::api) fn build_router(state: ApiState) -> Result<Router, ApiError> {
    let auth_state = state.clone();
    let root_state = state.clone();
    let api_routes = Router::new()
        .route("/live", get(get_live_snapshot))
        .route("/agent/:market/live", get(get_agent_snapshot))
        .route("/agent/:market/tools", get(get_agent_tools))
        .route("/agent/:market/wake", get(get_agent_wake))
        .route("/agent/:market/briefing", get(get_agent_briefing))
        .route("/agent/:market/analysis", get(get_agent_analysis))
        .route("/agent/:market/narration", get(get_agent_narration))
        .route(
            "/agent/:market/analyst-review",
            get(get_agent_analyst_review),
        )
        .route(
            "/agent/:market/analyst-scoreboard",
            get(get_agent_analyst_scoreboard),
        )
        .route("/agent/:market/session", get(get_agent_session))
        .route("/agent/:market/watchlist", get(get_agent_watchlist))
        .route(
            "/agent/:market/recommendations",
            get(get_agent_recommendations),
        )
        .route("/agent/:market/scoreboard", get(get_agent_scoreboard))
        .route("/agent/:market/eod-review", get(get_agent_eod_review))
        .route("/agent/:market/threads", get(get_agent_threads))
        .route("/agent/:market/threads/:symbol", get(get_agent_thread))
        .route("/agent/:market/turns", get(get_agent_turns))
        .route("/agent/:market/query", get(get_agent_query))
        .route(
            "/agent/:market/history/macro-events",
            get(get_agent_macro_event_history),
        )
        .route(
            "/agent/:market/history/knowledge-links",
            get(get_agent_knowledge_link_history),
        )
        .route(
            "/agent/:market/state/macro-events",
            get(get_agent_macro_event_state),
        )
        .route(
            "/agent/:market/state/knowledge-links",
            get(get_agent_knowledge_link_state),
        )
        .route(
            "/agent/:market/graph/node/:node_id",
            get(get_agent_graph_node),
        )
        .route("/agent/:market/graph/links", get(get_agent_graph_links))
        .route("/agent/:market/analyze", post(post_agent_analyze))
        .route(
            "/agent/:market/analyze/codex-cli",
            post(post_agent_analyze_codex_cli),
        )
        .route("/agent/:market/world", get(get_agent_world))
        .route("/agent/:market/notices", get(get_agent_notices))
        .route("/agent/:market/transitions", get(get_agent_transitions))
        .route("/feed/:market/notices", get(get_feed_notices))
        .route("/feed/:market/transitions", get(get_feed_transitions))
        .route(
            "/runtime/tasks",
            get(get_runtime_tasks).post(post_runtime_task),
        )
        .route("/runtime/tasks/:task_id", get(get_runtime_task))
        .route(
            "/runtime/tasks/:task_id/status",
            post(post_runtime_task_status),
        )
        .route("/status/context", get(get_context_status))
        .route("/agent/:market/structures", get(get_agent_structures))
        .route(
            "/agent/:market/structures/:symbol",
            get(get_agent_structure),
        )
        .route("/agent/:market/symbol/:symbol", get(get_agent_symbol))
        .route("/agent/:market/depth/:symbol", get(get_agent_depth))
        .route("/agent/:market/brokers/:symbol", get(get_agent_brokers))
        .route(
            "/agent/:market/invalidation/:symbol",
            get(get_agent_invalidation),
        )
        .route("/agent/:market/sector-flow", get(get_agent_sector_flows))
        .route("/agent/:market/backward/:symbol", get(get_agent_backward))
        .route(
            "/ontology/:market/operational-snapshot",
            get(get_operational_snapshot),
        )
        .route(
            "/ontology/:market/navigation/:kind/:id",
            get(get_operational_navigation),
        )
        .route(
            "/ontology/:market/neighborhood/:kind/:id",
            get(get_operational_neighborhood),
        )
        .route(
            "/ontology/:market/market-session",
            get(get_market_session_contract),
        )
        .route("/ontology/:market/world", get(get_ontology_world))
        .route(
            "/ontology/:market/macro-event-candidates",
            get(get_ontology_macro_event_candidates),
        )
        .route(
            "/ontology/:market/knowledge-links",
            get(get_ontology_knowledge_links),
        )
        .route("/ontology/:market/symbols", get(get_symbol_state_contracts))
        .route(
            "/ontology/:market/symbols/:symbol",
            get(get_symbol_state_contract),
        )
        .route("/ontology/:market/cases", get(get_case_contracts))
        .route("/ontology/:market/cases/:case_id", get(get_case_contract))
        .route(
            "/ontology/:market/cases/:case_id/history/workflow",
            get(get_case_workflow_history),
        )
        .route(
            "/ontology/:market/cases/:case_id/history/reasoning",
            get(get_case_reasoning_history),
        )
        .route(
            "/ontology/:market/cases/:case_id/history/outcomes",
            get(get_case_outcome_history),
        )
        .route(
            "/ontology/:market/recommendations",
            get(get_recommendation_contracts),
        )
        .route(
            "/ontology/:market/recommendations/:recommendation_id",
            get(get_recommendation_contract),
        )
        .route(
            "/ontology/:market/recommendations/:recommendation_id/history",
            get(get_recommendation_journal_history),
        )
        .route(
            "/ontology/:market/macro-events",
            get(get_macro_event_contracts),
        )
        .route(
            "/ontology/:market/macro-events/:event_id",
            get(get_macro_event_contract),
        )
        .route(
            "/ontology/:market/graph/history/macro-events",
            get(get_macro_event_history),
        )
        .route(
            "/ontology/:market/graph/history/knowledge-links",
            get(get_knowledge_link_history),
        )
        .route(
            "/ontology/:market/graph/state/macro-events",
            get(get_macro_event_state),
        )
        .route(
            "/ontology/:market/graph/state/knowledge-links",
            get(get_knowledge_link_state),
        )
        .route("/ontology/:market/graph/node/:node_id", get(get_graph_node))
        .route("/ontology/:market/graph/links", get(get_graph_links))
        .route(
            "/ontology/:market/threads/:thread_id",
            get(get_thread_contract),
        )
        .route(
            "/ontology/:market/workflows/:workflow_id",
            get(get_workflow_contract),
        )
        .route(
            "/ontology/:market/workflows/:workflow_id/history",
            get(get_workflow_event_history),
        )
        .route(
            "/ontology/:market/sector-flows",
            get(get_sector_flow_sidecars),
        )
        .route(
            "/ontology/:market/operator-work-items",
            get(get_operator_work_item_sidecars),
        )
        .route(
            "/ontology/:market/backward/:symbol",
            get(get_backward_investigation_sidecar),
        )
        .route("/stream/agent/:market/live", get(stream_agent_snapshot))
        .route("/stream/agent/:market/wake", get(stream_agent_wake))
        .route("/stream/agent/:market/briefing", get(stream_agent_briefing))
        .route("/stream/agent/:market/analysis", get(stream_agent_analysis))
        .route(
            "/stream/agent/:market/narration",
            get(stream_agent_narration),
        )
        .route(
            "/stream/agent/:market/analyst-review",
            get(stream_agent_analyst_review),
        )
        .route(
            "/stream/agent/:market/analyst-scoreboard",
            get(stream_agent_analyst_scoreboard),
        )
        .route("/stream/agent/:market/session", get(stream_agent_session))
        .route(
            "/stream/agent/:market/watchlist",
            get(stream_agent_watchlist),
        )
        .route(
            "/stream/agent/:market/recommendations",
            get(stream_agent_recommendations),
        )
        .route(
            "/stream/agent/:market/scoreboard",
            get(stream_agent_scoreboard),
        )
        .route(
            "/stream/agent/:market/eod-review",
            get(stream_agent_eod_review),
        )
        .route("/stream/agent/:market/threads", get(stream_agent_threads))
        .route("/stream/agent/:market/turns", get(stream_agent_turns))
        .route("/stream/feed/:market/notices", get(stream_feed_notices))
        .route(
            "/stream/feed/:market/transitions",
            get(stream_feed_transitions),
        )
        .route("/stream/ontology/:market/world", get(stream_ontology_world))
        .route("/cases/:market", get(get_cases))
        .route("/briefing/:market", get(get_case_briefing))
        .route("/review/:market", get(get_case_review))
        .route(
            "/review/:market/transitions",
            get(get_case_transition_analytics),
        )
        .route("/stream/:market/cases", get(stream_cases))
        .route("/stream/:market/briefing", get(stream_case_briefing))
        .route("/stream/:market/review", get(stream_case_review))
        .route(
            "/stream/:market/review/transitions",
            get(stream_case_transition_analytics),
        )
        .route("/stream/:market/cases/:setup_id", get(stream_case_detail))
        .route(
            "/stream/:market/cases/:setup_id/mechanism",
            get(stream_case_mechanism_story),
        )
        .route("/cases/:market/:setup_id", get(get_case_detail))
        .route(
            "/cases/:market/:setup_id/mechanism",
            get(get_case_mechanism_story),
        )
        .route("/cases/:market/:setup_id/assign", post(post_case_assign))
        .route(
            "/cases/:market/:setup_id/queue-pin",
            post(post_case_queue_pin),
        )
        .route(
            "/cases/:market/:setup_id/transition",
            post(post_case_transition),
        )
        .route("/us/live", get(get_us_live_snapshot))
        .route("/us/lineage", get(get_us_lineage))
        .route("/us/lineage/history", get(get_us_lineage_history))
        .route("/us/lineage/rows", get(get_us_lineage_rows))
        .route("/us/causal/flips", get(get_us_causal_flips))
        .route("/us/causal/timeline/:symbol", get(get_us_causal_timeline))
        .route("/polymarket", get(get_polymarket))
        .route("/lineage", get(get_lineage))
        .route("/lineage/history", get(get_lineage_history))
        .route("/lineage/rows", get(get_lineage_rows))
        .route("/causal/flips", get(get_causal_flips))
        .route("/causal/timeline/:leaf_scope_key", get(get_causal_timeline))
        .route("/archive/order-books/:symbol", get(get_archive_order_books))
        .route("/archive/trades/:symbol", get(get_archive_trades))
        .route(
            "/archive/capital-flows/:symbol",
            get(get_archive_capital_flows),
        )
        .route("/history/symbol/:symbol", get(get_symbol_history));

    #[cfg(feature = "coordinator")]
    let api_routes = api_routes.route("/coordinator/snapshot", get(get_coordinator_snapshot));

    let api_routes = api_routes
        .route("/health/report", get(health_report))
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_api_key));

    Ok(Router::new()
        .route("/health", get(health))
        .nest("/api", api_routes)
        .with_state(root_state)
        .layer(middleware::from_fn(audit_request))
        .layer(build_cors_layer()?))
}
