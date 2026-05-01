use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use longport::quote::PushEvent;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use serde_json::json;

use super::push_health::{HealthTransition, PushReceiverHealth};
use super::RuntimeInfraConfig;

#[derive(Debug, Clone, Default)]
pub struct RuntimeCounters {
    dropped_push_events: Arc<AtomicU64>,
    rest_updates: Arc<AtomicU64>,
    ndjson_drops: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Copy)]
pub enum RuntimeIssueLevel {
    Warning,
    Error,
}

impl RuntimeIssueLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl RuntimeCounters {
    pub fn record_dropped(&self, count: u64) {
        self.dropped_push_events.fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_rest_update(&self) {
        self.rest_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dropped_push_events(&self) -> u64 {
        self.dropped_push_events.load(Ordering::Relaxed)
    }

    pub fn rest_updates(&self) -> u64 {
        self.rest_updates.load(Ordering::Relaxed)
    }

    /// Shared handle that NdjsonWriter instances bump on `Full` drops.
    /// Spawn sites attach via `NdjsonWriter::with_aggregate_counter` so
    /// the tick summary can surface aggregate ndjson loss without
    /// iterating each writer.
    pub fn ndjson_drops_handle(&self) -> Arc<AtomicU64> {
        self.ndjson_drops.clone()
    }

    pub fn ndjson_drops(&self) -> u64 {
        self.ndjson_drops.load(Ordering::Relaxed)
    }
}

pub fn log_runtime_tick_summary(
    config: &RuntimeInfraConfig,
    tick: u64,
    total_push_events: u64,
    counters: &RuntimeCounters,
    tick_started_at: Instant,
    received_push: bool,
    received_update: bool,
) {
    if tick != 1 && tick % config.metrics_every_ticks != 0 {
        return;
    }

    println!(
        "[runtime:{} summary] tick={} tick_ms={} pushes={} dropped={} rest_updates={} ndjson_drops={} got_push={} got_update={}",
        config.market.slug(),
        tick,
        tick_started_at.elapsed().as_millis(),
        total_push_events,
        counters.dropped_push_events(),
        counters.rest_updates(),
        counters.ndjson_drops(),
        received_push,
        received_update,
    );
    emit_runtime_log(
        config,
        "tick_summary",
        json!({
            "tick": tick,
            "tick_ms": tick_started_at.elapsed().as_millis(),
            "pushes": total_push_events,
            "dropped_push_events": counters.dropped_push_events(),
            "rest_updates": counters.rest_updates(),
            "ndjson_drops": counters.ndjson_drops(),
            "received_push": received_push,
            "received_update": received_update,
        }),
    );
}

pub fn log_runtime_issue(
    config: &RuntimeInfraConfig,
    level: RuntimeIssueLevel,
    code: &str,
    message: impl Into<String>,
    payload: serde_json::Value,
) {
    let message = message.into();
    eprintln!(
        "[runtime:{} {}] {}: {}",
        config.market.slug(),
        level.as_str(),
        code,
        message
    );
    emit_runtime_log(
        config,
        "issue",
        json!({
            "level": level.as_str(),
            "code": code,
            "message": message,
            "payload": payload,
        }),
    );
}

pub fn spawn_push_forwarder(
    mut receiver: mpsc::UnboundedReceiver<PushEvent>,
    capacity: usize,
    counters: RuntimeCounters,
    config: RuntimeInfraConfig,
) -> mpsc::Receiver<PushEvent> {
    let (push_tx, push_rx) = mpsc::channel::<PushEvent>(capacity);
    tokio::spawn(async move {
        let mut dropped_push_events = 0u64;
        while let Some(event) = receiver.recv().await {
            match push_tx.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    dropped_push_events += 1;
                    counters.record_dropped(1);
                    if dropped_push_events == 1 || dropped_push_events % 100 == 0 {
                        log_runtime_issue(
                            &config,
                            RuntimeIssueLevel::Warning,
                            "push_channel_full",
                            format!(
                                "dropped {} {} push events because debounce channel is full.",
                                dropped_push_events,
                                config.market.slug().to_ascii_uppercase()
                            ),
                            json!({ "dropped_push_events": dropped_push_events }),
                        );
                    }
                }
                Err(TrySendError::Closed(_)) => {
                    log_runtime_issue(
                        &config,
                        RuntimeIssueLevel::Error,
                        "push_channel_closed",
                        format!(
                            "{} push event channel closed; stopping forwarder.",
                            config.market.slug().to_ascii_uppercase()
                        ),
                        json!({}),
                    );
                    break;
                }
            }
        }
    });
    push_rx
}

/// Push-event tap. Called once per `PushEvent` *before* batching, so
/// the tap sees every event the longport receiver yields — including
/// events that the bounded batch channel later drops. The C4 fix wires
/// the pressure-event bus through this tap so sub-tick channels keep
/// recomputing under ingest backpressure.
pub type PushTap = Box<dyn Fn(&PushEvent) + Send + Sync + 'static>;

pub fn spawn_batched_push_forwarder(
    mut receiver: mpsc::UnboundedReceiver<PushEvent>,
    capacity: usize,
    batch_size: usize,
    counters: RuntimeCounters,
    config: RuntimeInfraConfig,
    tap: Option<PushTap>,
    health: Option<Arc<PushReceiverHealth>>,
) -> mpsc::Receiver<Vec<PushEvent>> {
    let (push_tx, push_rx) = mpsc::channel::<Vec<PushEvent>>(capacity);
    tokio::spawn(async move {
        let mut dropped_push_events = 0u64;
        while let Some(event) = receiver.recv().await {
            if let Some(h) = health.as_ref() {
                h.record_event();
            }
            if let Some(tap_fn) = tap.as_ref() {
                tap_fn(&event);
            }
            let mut batch = Vec::with_capacity(batch_size);
            batch.push(event);
            while batch.len() < batch_size {
                match receiver.try_recv() {
                    Ok(event) => {
                        if let Some(h) = health.as_ref() {
                            h.record_event();
                        }
                        if let Some(tap_fn) = tap.as_ref() {
                            tap_fn(&event);
                        }
                        batch.push(event);
                    }
                    Err(_) => break,
                }
            }
            match push_tx.try_send(batch) {
                Ok(()) => {}
                Err(TrySendError::Full(batch)) => {
                    dropped_push_events += batch.len() as u64;
                    counters.record_dropped(batch.len() as u64);
                    if dropped_push_events == 1 || dropped_push_events % 100 == 0 {
                        log_runtime_issue(
                            &config,
                            RuntimeIssueLevel::Warning,
                            "push_channel_full",
                            format!(
                                "dropped {} {} push events because debounce channel is full.",
                                dropped_push_events,
                                config.market.slug().to_ascii_uppercase()
                            ),
                            json!({ "dropped_push_events": dropped_push_events }),
                        );
                    }
                }
                Err(TrySendError::Closed(_)) => {
                    log_runtime_issue(
                        &config,
                        RuntimeIssueLevel::Error,
                        "push_channel_closed",
                        format!(
                            "{} push event channel closed; stopping forwarder.",
                            config.market.slug().to_ascii_uppercase()
                        ),
                        json!({}),
                    );
                    break;
                }
            }
        }
    });
    push_rx
}

/// Spawn a periodic poller that watches `health` and emits a runtime
/// log on Healthy↔Stale transitions. Each silence episode produces one
/// "stale" warning when crossed in, and one "recovered" info log when
/// the next event arrives — preventing log spam during sustained
/// outages while still surfacing both edges of the event.
///
/// The poller runs forever (until the runtime tears down). For tests
/// that need to exercise the poll/log loop in isolation, drive
/// [`PushReceiverHealth::poll_transition`] directly — this wrapper is
/// only the spawn-and-forget glue.
pub fn spawn_push_health_monitor(
    health: Arc<PushReceiverHealth>,
    poll_interval: Duration,
    config: RuntimeInfraConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(poll_interval);
        // Skip the immediate first tick so we don't emit on a fresh
        // NeverReceived → NeverReceived "transition" before any event
        // could plausibly have arrived.
        interval.tick().await;
        loop {
            interval.tick().await;
            match health.poll_transition() {
                HealthTransition::Unchanged => {}
                HealthTransition::BecameStale { silent_for } => {
                    log_runtime_issue(
                        &config,
                        RuntimeIssueLevel::Warning,
                        "push_receiver_stale",
                        format!(
                            "{} push receiver silent for {:.1}s — Longport stream may be \
                             reconnecting or upstream is degraded.",
                            config.market.slug().to_ascii_uppercase(),
                            silent_for.as_secs_f64(),
                        ),
                        json!({ "silent_for_secs": silent_for.as_secs_f64() }),
                    );
                }
                HealthTransition::Recovered { last_event_age } => {
                    log_runtime_issue(
                        &config,
                        RuntimeIssueLevel::Warning,
                        "push_receiver_recovered",
                        format!(
                            "{} push receiver recovered; latest event {:.2}s old.",
                            config.market.slug().to_ascii_uppercase(),
                            last_event_age.as_secs_f64(),
                        ),
                        json!({ "last_event_age_secs": last_event_age.as_secs_f64() }),
                    );
                }
            }
        }
    })
}

pub(super) fn load_u64_override(
    market_key: &str,
    global_key: &str,
    default: u64,
) -> Result<u64, String> {
    match std::env::var(market_key)
        .ok()
        .or_else(|| std::env::var(global_key).ok())
    {
        Some(raw) => raw.parse::<u64>().map_err(|error| {
            format!(
                "invalid integer for {} / {}: {}",
                market_key, global_key, error
            )
        }),
        None => Ok(default),
    }
}

pub(super) fn load_string_override(market_key: &str, global_key: &str, default: &str) -> String {
    std::env::var(market_key)
        .ok()
        .or_else(|| std::env::var(global_key).ok())
        .unwrap_or_else(|| default.to_string())
}

pub(super) fn load_optional_string_override(market_key: &str, global_key: &str) -> Option<String> {
    std::env::var(market_key)
        .ok()
        .or_else(|| std::env::var(global_key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn emit_runtime_log(
    config: &RuntimeInfraConfig,
    event: &str,
    payload: serde_json::Value,
) {
    let Some(path) = config.runtime_log_path.as_deref() else {
        return;
    };

    let record = json!({
        "ts": time::OffsetDateTime::now_utc().unix_timestamp(),
        "market": config.market.slug(),
        "event": event,
        "payload": payload,
    });

    if let Err(error) = append_json_line(path, &record) {
        eprintln!(
            "Warning: failed to append runtime log for {} to {}: {}",
            config.market.slug(),
            path,
            error
        );
    }
}

fn append_json_line(path: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let line = serde_json::to_string(value).map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())
}
