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
    // The three "*_available" booleans are kept for frontend struct
    // stability (system-status tags read them). Their gated backends
    // were removed as vestigial scaffolding; the fields stay so the
    // typed contract with the frontend doesn't break, but the values
    // are permanently false now.
    Json(ContextStatus {
        runtime_features: config.all_enabled(),
        context_layers_available: false,
        coordinator_available: false,
        task_lifecycle_available: true,
        tool_registry_available: false,
    })
}
