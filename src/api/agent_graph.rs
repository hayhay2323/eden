//! Compatibility wrappers for legacy `/agent/history/*`, `/agent/state/*`, and `/agent/graph/*`
//! routes. New consumers should prefer `/ontology/:market/graph/*`.

#[cfg(feature = "persistence")]
use axum::extract::{Path, Query, State};
#[cfg(not(feature = "persistence"))]
use axum::Json;

#[cfg(feature = "persistence")]
use super::agent_api::AgentFeedQuery;
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
#[cfg(feature = "persistence")]
use super::ontology_graph_api::{
    get_graph_links, get_graph_node, get_knowledge_link_history, get_knowledge_link_state,
    get_macro_event_history, get_macro_event_state, AgentGraphQuery,
};

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_macro_event_history(
    state: State<ApiState>,
    path: Path<String>,
    query: Query<AgentFeedQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_macro_event_history(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_macro_event_history() -> Result<Json<serde_json::Value>, ApiError> {
    super::ontology_graph_api::get_macro_event_history().await
}

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_knowledge_link_history(
    state: State<ApiState>,
    path: Path<String>,
    query: Query<AgentFeedQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_knowledge_link_history(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_knowledge_link_history() -> Result<Json<serde_json::Value>, ApiError>
{
    super::ontology_graph_api::get_knowledge_link_history().await
}

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_macro_event_state(
    state: State<ApiState>,
    path: Path<String>,
    query: Query<AgentFeedQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_macro_event_state(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_macro_event_state() -> Result<Json<serde_json::Value>, ApiError> {
    super::ontology_graph_api::get_macro_event_state().await
}

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_knowledge_link_state(
    state: State<ApiState>,
    path: Path<String>,
    query: Query<AgentFeedQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_knowledge_link_state(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_knowledge_link_state() -> Result<Json<serde_json::Value>, ApiError> {
    super::ontology_graph_api::get_knowledge_link_state().await
}

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_graph_node(
    state: State<ApiState>,
    path: Path<(String, String)>,
    query: Query<AgentGraphQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_graph_node(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_graph_node() -> Result<Json<serde_json::Value>, ApiError> {
    super::ontology_graph_api::get_graph_node().await
}

#[cfg(feature = "persistence")]
pub(super) async fn get_agent_graph_links(
    state: State<ApiState>,
    path: Path<String>,
    query: Query<AgentGraphQuery>,
) -> Result<impl axum::response::IntoResponse, ApiError> {
    get_graph_links(state, path, query).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_agent_graph_links() -> Result<Json<serde_json::Value>, ApiError> {
    super::ontology_graph_api::get_graph_links().await
}
