#[cfg(feature = "coordinator")]
use crate::core::coordinator::CoordinatorSnapshot;
#[cfg(feature = "coordinator")]
use axum::Json;

/// Returns the latest coordinator snapshot.
///
/// For now, since the coordinator is not wired to a live runtime yet,
/// this returns an empty snapshot. The endpoint exists so the frontend
/// can start consuming it once the coordinator is running.
#[cfg(feature = "coordinator")]
pub(super) async fn get_coordinator_snapshot() -> Json<CoordinatorSnapshot> {
    Json(CoordinatorSnapshot::empty())
}
