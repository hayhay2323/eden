use std::time::SystemTime;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use serde_json::Value;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::core::artifact_repository::resolve_artifact_path;
use crate::core::market::{ArtifactKind, MarketId};
use crate::core::runtime::RuntimeInfraConfig;
use crate::core::settings::ApiInfraConfig;
use crate::external::polymarket::{
    fetch_polymarket_snapshot, load_polymarket_configs, PolymarketMarketConfig, PolymarketSnapshot,
};

use super::super::foundation::{ApiError, ApiState};
use super::auth::resolve_cors_policy;

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(with = "rfc3339")]
    now: OffsetDateTime,
}

#[derive(Debug, Serialize)]
pub(super) struct HealthReportResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(with = "rfc3339")]
    now: OffsetDateTime,
    api: ApiHealthSummary,
    runtimes: Vec<RuntimeHealthSummary>,
}

#[derive(Debug, Serialize)]
struct ApiHealthSummary {
    status: &'static str,
    bind_addr: String,
    db_path: String,
    runtime_tasks_path: String,
    runtime_task_count: usize,
    persistence_enabled: bool,
    query_auth_enabled: bool,
    cors_mode: &'static str,
    allowed_origins: Vec<String>,
    revocation_path: String,
    revoked_token_count: usize,
}

#[derive(Debug, Serialize)]
struct RuntimeHealthSummary {
    status: &'static str,
    market: String,
    debounce_ms: u64,
    rest_refresh_secs: u64,
    metrics_every_ticks: u64,
    db_path: String,
    runtime_log_path: Option<String>,
    artifacts: Vec<ArtifactHealth>,
    issue_summary: RuntimeIssueSummary,
    recent_runtime_events: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct ArtifactHealth {
    kind: &'static str,
    status: &'static str,
    path: String,
    exists: bool,
    size_bytes: Option<u64>,
    modified_at: Option<String>,
    age_secs: Option<i64>,
}

#[derive(Debug, Serialize, Default)]
struct RuntimeIssueSummary {
    warning_count: usize,
    error_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    last_issue_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct PolymarketResponse {
    configs: Vec<PolymarketMarketConfig>,
    snapshot: PolymarketSnapshot,
}

pub(super) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eden-api",
        version: env!("CARGO_PKG_VERSION"),
        now: OffsetDateTime::now_utc(),
    })
}

pub(super) async fn health_report(
    State(state): State<ApiState>,
) -> Result<Json<HealthReportResponse>, ApiError> {
    let api_config = ApiInfraConfig::load().map_err(ApiError::internal)?;
    let cors_policy = resolve_cors_policy()?;
    let runtimes = vec![
        build_runtime_health_summary(MarketId::Hk).await?,
        build_runtime_health_summary(MarketId::Us).await?,
    ];
    let overall_status = aggregate_overall_status(&runtimes);

    Ok(Json(HealthReportResponse {
        status: overall_status,
        service: "eden-api",
        version: env!("CARGO_PKG_VERSION"),
        now: OffsetDateTime::now_utc(),
        api: ApiHealthSummary {
            status: "ok",
            bind_addr: state.bind_addr.to_string(),
            db_path: api_config.db_path,
            runtime_tasks_path: state.runtime_tasks.path().display().to_string(),
            runtime_task_count: state.runtime_tasks.task_count(),
            persistence_enabled: cfg!(feature = "persistence"),
            query_auth_enabled: false,
            cors_mode: cors_policy.mode,
            allowed_origins: cors_policy.origins,
            revocation_path: state.revocations.path().to_string(),
            revoked_token_count: state.revocations.revoked_count(),
        },
        runtimes,
    }))
}

pub(super) async fn get_polymarket() -> Result<Json<PolymarketResponse>, ApiError> {
    let configs = load_polymarket_configs().map_err(ApiError::bad_request)?;
    let snapshot = fetch_polymarket_snapshot(&configs)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(PolymarketResponse { configs, snapshot }))
}

pub(super) async fn get_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path).await.map_err(|_| {
        ApiError::service_unavailable("live snapshot not available — is eden running?")
    })?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(&format!("invalid snapshot json: {e}")))?;
    Ok(Json(value))
}

pub(super) async fn get_us_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_US_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/us_live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path).await.map_err(|_| {
        ApiError::service_unavailable("US live snapshot not available — is `eden us` running?")
    })?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(&format!("invalid US snapshot json: {e}")))?;
    Ok(Json(value))
}

async fn build_runtime_health_summary(market: MarketId) -> Result<RuntimeHealthSummary, ApiError> {
    let runtime_config = RuntimeInfraConfig::load(market).map_err(ApiError::internal)?;
    let artifacts = load_runtime_artifacts(market).await;
    let issue_events = load_runtime_log_tail(runtime_config.runtime_log_path.as_deref(), 50).await;
    let issue_summary = summarize_runtime_issues(&issue_events);
    let recent_runtime_events = issue_events
        .iter()
        .rev()
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let status = aggregate_runtime_status(&artifacts, &issue_summary);

    Ok(RuntimeHealthSummary {
        status,
        market: market.slug().to_string(),
        debounce_ms: runtime_config.debounce_ms,
        rest_refresh_secs: runtime_config.rest_refresh_secs,
        metrics_every_ticks: runtime_config.metrics_every_ticks,
        db_path: runtime_config.db_path,
        runtime_log_path: runtime_config.runtime_log_path,
        artifacts,
        issue_summary,
        recent_runtime_events,
    })
}

async fn load_runtime_artifacts(market: MarketId) -> Vec<ArtifactHealth> {
    let artifact_specs = match market {
        MarketId::Hk => vec![
            ("live_snapshot", ArtifactKind::LiveSnapshot),
            ("bridge_snapshot", ArtifactKind::BridgeSnapshot),
            ("agent_snapshot", ArtifactKind::AgentSnapshot),
            ("briefing", ArtifactKind::Briefing),
            ("session", ArtifactKind::Session),
            ("watchlist", ArtifactKind::Watchlist),
            ("recommendations", ArtifactKind::Recommendations),
            ("scoreboard", ArtifactKind::Scoreboard),
            ("eod_review", ArtifactKind::EodReview),
            ("analysis", ArtifactKind::Analysis),
            ("narration", ArtifactKind::Narration),
            ("runtime_narration", ArtifactKind::RuntimeNarration),
        ],
        MarketId::Us => vec![
            ("live_snapshot", ArtifactKind::LiveSnapshot),
            ("bridge_snapshot", ArtifactKind::BridgeSnapshot),
            ("agent_snapshot", ArtifactKind::AgentSnapshot),
            ("briefing", ArtifactKind::Briefing),
            ("session", ArtifactKind::Session),
            ("watchlist", ArtifactKind::Watchlist),
            ("recommendations", ArtifactKind::Recommendations),
            ("scoreboard", ArtifactKind::Scoreboard),
            ("eod_review", ArtifactKind::EodReview),
            ("analysis", ArtifactKind::Analysis),
            ("narration", ArtifactKind::Narration),
            ("runtime_narration", ArtifactKind::RuntimeNarration),
        ],
    };

    let mut items = Vec::with_capacity(artifact_specs.len());
    for (label, kind) in artifact_specs {
        let path = resolve_artifact_path(market, kind);
        items.push(build_artifact_health(label, &path).await);
    }
    items
}

async fn build_artifact_health(kind: &'static str, path: &str) -> ArtifactHealth {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => {
            let modified_at = metadata.modified().ok().and_then(system_time_to_rfc3339);
            let age_secs = metadata.modified().ok().and_then(system_time_to_age_secs);
            ArtifactHealth {
                kind,
                status: classify_artifact_status(kind, age_secs),
                path: path.to_string(),
                exists: true,
                size_bytes: Some(metadata.len()),
                modified_at,
                age_secs,
            }
        }
        Err(_) => ArtifactHealth {
            kind,
            status: "missing",
            path: path.to_string(),
            exists: false,
            size_bytes: None,
            modified_at: None,
            age_secs: None,
        },
    }
}

async fn load_runtime_log_tail(path: Option<&str>, limit: usize) -> Vec<Value> {
    let Some(path) = path else {
        return Vec::new();
    };
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };
    content
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(limit)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn classify_artifact_status(kind: &str, age_secs: Option<i64>) -> &'static str {
    let Some(age_secs) = age_secs else {
        return "missing";
    };
    let budget = artifact_freshness_budget_secs(kind);
    if age_secs <= budget {
        "fresh"
    } else {
        "stale"
    }
}

fn artifact_freshness_budget_secs(kind: &str) -> i64 {
    match kind {
        "live_snapshot" | "agent_snapshot" | "bridge_snapshot" => 180,
        "briefing" | "session" | "watchlist" | "recommendations" | "scoreboard" => 600,
        "eod_review" | "analysis" | "narration" | "runtime_narration" => 1800,
        _ => 600,
    }
}

fn summarize_runtime_issues(events: &[Value]) -> RuntimeIssueSummary {
    let mut summary = RuntimeIssueSummary::default();
    for event in events {
        let payload = event.get("payload");
        let level = payload
            .and_then(|payload| payload.get("level"))
            .and_then(|value| value.as_str());
        let code = payload
            .and_then(|payload| payload.get("code"))
            .and_then(|value| value.as_str());
        match level {
            Some("warning") => summary.warning_count += 1,
            Some("error") => summary.error_count += 1,
            _ => {}
        }
        if let Some(code) = code {
            if !summary.last_issue_codes.iter().any(|item| item == code) {
                summary.last_issue_codes.push(code.to_string());
            }
        }
    }
    summary.last_issue_codes.truncate(5);
    summary
}

fn aggregate_runtime_status(
    artifacts: &[ArtifactHealth],
    issue_summary: &RuntimeIssueSummary,
) -> &'static str {
    if issue_summary.error_count > 0 {
        return "error";
    }

    let critical_missing = artifacts.iter().any(|artifact| {
        matches!(
            artifact.kind,
            "live_snapshot" | "agent_snapshot" | "briefing" | "session"
        ) && artifact.status == "missing"
    });
    if critical_missing {
        return "error";
    }

    let critical_stale = artifacts.iter().any(|artifact| {
        matches!(
            artifact.kind,
            "live_snapshot" | "agent_snapshot" | "briefing" | "session"
        ) && artifact.status == "stale"
    });
    if critical_stale || issue_summary.warning_count > 0 {
        return "warn";
    }

    "ok"
}

fn aggregate_overall_status(runtimes: &[RuntimeHealthSummary]) -> &'static str {
    if runtimes.iter().any(|runtime| runtime.status == "error") {
        "error"
    } else if runtimes.iter().any(|runtime| runtime.status == "warn") {
        "warn"
    } else {
        "ok"
    }
}

fn system_time_to_rfc3339(value: SystemTime) -> Option<String> {
    let duration = value.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let timestamp = OffsetDateTime::from_unix_timestamp_nanos(duration.as_nanos() as i128).ok()?;
    timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

fn system_time_to_age_secs(value: SystemTime) -> Option<i64> {
    let modified = value.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(OffsetDateTime::now_utc().unix_timestamp() - modified)
}
