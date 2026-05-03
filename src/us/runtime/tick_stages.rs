//! Extracted stages of the US tick loop body.
//!
//! Each function here corresponds to one of the `S0*` markers in
//! `us::runtime::run`. Extractions are pure code motion: behaviour is
//! identical to the previous inline form, verified by `cargo check`
//! plus the existing test suite. New stages are added as additional
//! functions; the original inline code is replaced with a call.

use super::view::build_us_bootstrap_snapshot;
use crate::core::runtime::PreparedRuntimeContext;
use crate::live_snapshot::{spawn_write_snapshot, LiveClusterState, LiveWorldSummary};
use crate::ontology::store::ObjectStore;
use crate::pipeline::pressure::reasoning::LifecycleTracker;
use crate::pipeline::state_engine::PersistentSymbolState;
use crate::us::runtime::support::{
    is_us_regular_market_hours, us_market_hours_utc, UsLiveState, UsRestSnapshot,
};
use std::sync::Arc;

/// S02 — after-hours idle branch.
///
/// When the US market is closed (per `is_us_regular_market_hours`),
/// the tick loop still writes an "alive" snapshot and emits a
/// heartbeat, but skips all reasoning. Returns `true` when the caller
/// should `continue` the loop (i.e., the after-hours path was taken).
///
/// Pure code motion of the inline `if !market_open { … continue; }`
/// block from the US tick loop.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_us_after_hours_idle(
    tick: u64,
    now: time::OffsetDateTime,
    store: &Arc<ObjectStore>,
    live: &UsLiveState,
    rest: &UsRestSnapshot,
    workflows_count: usize,
    previous_symbol_states: &mut Vec<PersistentSymbolState>,
    previous_cluster_states: &mut Vec<LiveClusterState>,
    previous_world_summary: &mut Option<LiveWorldSummary>,
    lifecycle_tracker: &mut LifecycleTracker,
    runtime: &PreparedRuntimeContext,
) -> bool {
    if is_us_regular_market_hours(now) {
        return false;
    }
    lifecycle_tracker.decay(tick);
    let timestamp_str = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let idle_snapshot = build_us_bootstrap_snapshot(
        tick,
        timestamp_str,
        store,
        live,
        rest,
        previous_symbol_states,
        previous_cluster_states,
        previous_world_summary.as_ref(),
    );
    *previous_symbol_states = idle_snapshot.symbol_states.clone();
    *previous_cluster_states = idle_snapshot.cluster_states.clone();
    *previous_world_summary = idle_snapshot.world_summary.clone();
    spawn_write_snapshot(runtime.artifacts.live_snapshot_path.clone(), idle_snapshot);
    if tick % 100 == 0 {
        let (open_hour, open_minute, close_hour, close_minute) = us_market_hours_utc(now);
        println!(
            "[US tick {}] after-hours (UTC {:02}:{:02}, session {:02}:{:02}-{:02}:{:02}), skipping reasoning",
            tick,
            now.hour(),
            now.minute(),
            open_hour,
            open_minute,
            close_hour,
            close_minute,
        );
    }
    runtime.runtime_task_heartbeat(
        "us runtime waiting for regular market hours",
        serde_json::json!({
            "market": "us",
            "tick": tick,
            "market_open": false,
            "quotes": live.quotes.len(),
            "candlesticks": live.candlesticks.len(),
            "workflows": workflows_count,
        }),
    );
    true
}
