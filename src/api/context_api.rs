use axum::Json;
use serde::Serialize;

use crate::core::feature_gates::RuntimeFeatureConfig;

/// Reports the current feature-gate status and context-layer availability.
#[derive(Serialize)]
pub struct ContextStatus {
    pub runtime_features: Vec<String>,
    pub context_layers_available: bool,
    pub coordinator_available: bool,
    pub task_lifecycle_available: bool,
    pub tool_registry_available: bool,
}

pub(super) async fn get_context_status() -> Json<ContextStatus> {
    let config = RuntimeFeatureConfig::load();
    Json(ContextStatus {
        runtime_features: config.all_enabled(),
        context_layers_available: cfg!(feature = "context-layers"),
        coordinator_available: cfg!(feature = "coordinator"),
        task_lifecycle_available: true,
        tool_registry_available: true,
    })
}
