use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::runtime_loop::{next_tick, TickAdvance, TickState, spawn_periodic_fetch};

use super::{RuntimeCounters, RuntimeInfraConfig};
use super::telemetry::{emit_runtime_log, log_runtime_issue, RuntimeIssueLevel};

pub fn log_runtime_monitoring_active(runtime_config: &RuntimeInfraConfig, label: &str) {
    println!(
        "\n{} (debounce: {}ms)\n",
        label,
        runtime_config.debounce_ms,
    );
    emit_runtime_log(
        runtime_config,
        "monitoring_active",
        serde_json::json!({
            "label": label,
            "debounce_ms": runtime_config.debounce_ms,
        }),
    );
}

pub fn spawn_runtime_rest_refresh<U, F, Fut>(
    runtime_config: &RuntimeInfraConfig,
    capacity: usize,
    fetcher: F,
) -> mpsc::Receiver<U>
where
    U: Send + 'static,
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = U> + Send + 'static,
{
    spawn_periodic_fetch(capacity, runtime_config.rest_refresh_duration(), fetcher)
}

pub struct RuntimeTickBoundary {
    pub advance: TickAdvance,
    pub started_at: Instant,
}

pub async fn advance_runtime_tick<P, U, S>(
    bootstrap_pending: &mut bool,
    push_rx: &mut mpsc::Receiver<P>,
    update_rx: &mut mpsc::Receiver<U>,
    runtime_config: &RuntimeInfraConfig,
    debounce: Duration,
    state: &mut S,
    tick: &mut u64,
) -> Option<TickAdvance>
where
    S: TickState<P, U>,
{
    match next_tick(
        bootstrap_pending,
        push_rx,
        update_rx,
        debounce,
        state,
        tick,
    )
    .await
    {
        Ok(result) => result,
        Err(()) => {
            log_runtime_issue(
                runtime_config,
                RuntimeIssueLevel::Warning,
                "push_channel_closed",
                "push channel closed; runtime loop exiting",
                serde_json::json!({}),
            );
            None
        }
    }
}

pub async fn begin_runtime_tick<P, U, S>(
    bootstrap_pending: &mut bool,
    push_rx: &mut mpsc::Receiver<P>,
    update_rx: &mut mpsc::Receiver<U>,
    runtime_config: &RuntimeInfraConfig,
    runtime_counters: &RuntimeCounters,
    debounce: Duration,
    state: &mut S,
    tick: &mut u64,
) -> Option<RuntimeTickBoundary>
where
    S: TickState<P, U>,
{
    let advance = advance_runtime_tick(
        bootstrap_pending,
        push_rx,
        update_rx,
        runtime_config,
        debounce,
        state,
        tick,
    )
    .await?;
    let started_at = Instant::now();
    if advance.received_update {
        runtime_counters.record_rest_update();
    }
    Some(RuntimeTickBoundary { advance, started_at })
}
