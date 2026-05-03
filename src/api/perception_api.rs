//! Y reader — the canonical perception query surface.
//!
//! Per the eden thesis: eden is Y's sensory organ, not Y's decider.
//! This endpoint exposes the structured perception report (emergence,
//! sector leaders, causal chains, anomalies, regime, belief kinetics,
//! sensory & thematic vortices) so that an external Y — human, Codex
//! analyst, future autonomous AI — can read what eden currently
//! perceives without parsing the heavier `AgentSnapshot` envelope.
//!
//! The report is produced by `PerceptionGraph::to_report()` inside the
//! runtime (see `src/agent/builders/hk.rs` and `us.rs`) and persisted
//! as part of `AgentSnapshot.perception`. This handler reads that
//! persisted snapshot and returns just the perception slice.
//!
//! Architectural reference:
//! `docs/architecture/perception-graph-sync-contract.md`.
use axum::extract::Path;
use axum::Json;

use crate::agent::{self, EdenPerception};

use super::core::parse_case_market;
use super::foundation::ApiError;

/// `GET /perception/:market` — return the latest perception report for
/// `market`. Returns 404 if no perception has been generated yet (e.g.
/// runtime hasn't finished its first tick after start-up).
pub(super) async fn get_perception(
    Path(market): Path<String>,
) -> Result<Json<EdenPerception>, ApiError> {
    let market = parse_case_market(&market)?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    snapshot.perception.map(Json).ok_or_else(|| {
        ApiError::not_found(format!(
            "no perception report yet for market `{market:?}` — runtime may not have ticked"
        ))
    })
}
