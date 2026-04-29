use axum::extract::{Path, Query, State};
use axum::Json;

use crate::core::runtime_tasks::{
    RuntimeTaskCreateRequest, RuntimeTaskFilter, RuntimeTaskRecord, RuntimeTaskStatusUpdateRequest,
};

use super::foundation::{ApiError, ApiState};

pub(super) async fn get_runtime_tasks(
    State(state): State<ApiState>,
    Query(filter): Query<RuntimeTaskFilter>,
) -> Result<Json<Vec<RuntimeTaskRecord>>, ApiError> {
    Ok(Json(state.runtime_tasks.list(&filter)))
}

pub(super) async fn get_runtime_task(
    State(state): State<ApiState>,
    Path(task_id): Path<String>,
) -> Result<Json<RuntimeTaskRecord>, ApiError> {
    state
        .runtime_tasks
        .get(&task_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("runtime task not found: {task_id}")))
}

pub(super) async fn post_runtime_task(
    State(state): State<ApiState>,
    Json(body): Json<RuntimeTaskCreateRequest>,
) -> Result<Json<RuntimeTaskRecord>, ApiError> {
    state
        .runtime_tasks
        .create(body)
        .map(Json)
        .map_err(ApiError::bad_request)
}

pub(super) async fn post_runtime_task_status(
    State(state): State<ApiState>,
    Path(task_id): Path<String>,
    Json(body): Json<RuntimeTaskStatusUpdateRequest>,
) -> Result<Json<RuntimeTaskRecord>, ApiError> {
    state
        .runtime_tasks
        .update_status(&task_id, body)
        .map(Json)
        .map_err(|error| {
            if error.contains("not found") {
                ApiError::not_found(error)
            } else {
                ApiError::bad_request(error)
            }
        })
}
