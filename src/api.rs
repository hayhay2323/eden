use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::body::Body;
use axum::extract::State;
#[cfg(feature = "persistence")]
use axum::extract::{Path, Query};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
#[cfg(feature = "persistence")]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::serde::rfc3339;
use time::OffsetDateTime;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::external::polymarket::{
    fetch_polymarket_snapshot, load_polymarket_configs, PolymarketMarketConfig, PolymarketSnapshot,
};
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::{
    row_matches_filters, snapshot_records_from_rows, LineageMetricRowRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::temporal::buffer::TickHistory;
#[cfg(feature = "persistence")]
use crate::temporal::causality::{
    compute_causal_timelines, CausalFlipEvent, CausalFlipStyle, CausalTimeline,
};
use crate::temporal::lineage::{
    LineageAlignmentFilter, LineageFilters, LineageSortKey, LineageStats,
};

#[cfg(feature = "persistence")]
const DEFAULT_LIMIT: usize = 120;
#[cfg(feature = "persistence")]
const DEFAULT_TOP: usize = 5;
#[cfg(feature = "persistence")]
const MAX_LIMIT: usize = 2_000;
#[cfg(feature = "persistence")]
const MAX_TOP: usize = 100;
const DEFAULT_API_SCOPE: &str = "frontend:readonly";
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8787";
const API_KEY_PREFIX: &str = "eden_pk_";

#[derive(Clone)]
pub struct ApiState {
    auth: ApiKeyCipher,
    #[cfg(feature = "persistence")]
    store: EdenStore,
}

#[derive(Clone)]
pub struct ApiKeyCipher {
    cipher: Arc<Aes256Gcm>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyClaims {
    pub label: String,
    pub scope: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub token_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MintedApiKey {
    pub api_key: String,
    pub label: String,
    pub scope: String,
    #[serde(with = "rfc3339")]
    pub issued_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub expires_at: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    #[serde(with = "rfc3339")]
    now: OffsetDateTime,
}

#[derive(Debug, Serialize)]
struct PolymarketResponse {
    configs: Vec<PolymarketMarketConfig>,
    snapshot: PolymarketSnapshot,
}

#[derive(Debug, Serialize)]
struct LineageResponse {
    window_size: usize,
    filters: LineageFilters,
    top: usize,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
    stats: LineageStats,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct LineageHistoryResponse {
    requested_snapshots: usize,
    returned_snapshots: usize,
    filters: LineageFilters,
    top: usize,
    latest_only: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
    snapshots: Vec<LineageSnapshotRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct LineageRowsResponse {
    requested_rows: usize,
    returned_rows: usize,
    filters: LineageFilters,
    top: usize,
    latest_only: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
    rows: Vec<LineageMetricRowRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct CausalTimelineResponse {
    window_size: usize,
    timeline: CausalTimeline,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Serialize)]
struct FlatCausalFlip {
    leaf_label: String,
    leaf_scope_key: String,
    event: CausalFlipEvent,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct CausalFlipsResponse {
    window_size: usize,
    total: usize,
    sudden: usize,
    erosion_driven: usize,
    flips: Vec<FlatCausalFlip>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct LineageQuery {
    limit: Option<usize>,
    top: Option<usize>,
    label: Option<String>,
    bucket: Option<String>,
    family: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
    alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct LineageHistoryQuery {
    snapshots: Option<usize>,
    top: Option<usize>,
    latest_only: Option<bool>,
    label: Option<String>,
    bucket: Option<String>,
    family: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
    alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct LineageRowsQuery {
    rows: Option<usize>,
    top: Option<usize>,
    latest_only: Option<bool>,
    label: Option<String>,
    bucket: Option<String>,
    family: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
    alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct CausalQuery {
    limit: Option<usize>,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    #[cfg(feature = "persistence")]
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorBody {
            error: self.message,
        });
        (self.status, body).into_response()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiKeyCipher {
    pub fn from_env() -> Result<Self, ApiError> {
        let secret = env::var("EDEN_API_MASTER_KEY")
            .map_err(|_| ApiError::internal("EDEN_API_MASTER_KEY is not set"))?;
        Self::from_secret(&secret)
    }

    pub fn from_secret(secret: &str) -> Result<Self, ApiError> {
        if secret.trim().is_empty() {
            return Err(ApiError::internal("EDEN_API_MASTER_KEY is empty"));
        }
        let key = Sha256::digest(secret.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|error| {
            ApiError::internal(format!("failed to initialize API key cipher: {error}"))
        })?;
        Ok(Self {
            cipher: Arc::new(cipher),
        })
    }

    pub fn mint_key(
        &self,
        label: &str,
        ttl_hours: u64,
        scope: Option<&str>,
    ) -> Result<MintedApiKey, ApiError> {
        if label.trim().is_empty() {
            return Err(ApiError::bad_request("API key label cannot be empty"));
        }
        if ttl_hours == 0 {
            return Err(ApiError::bad_request("ttl_hours must be greater than 0"));
        }
        if ttl_hours > 24 * 365 {
            return Err(ApiError::bad_request("ttl_hours is too large"));
        }

        let issued_at = OffsetDateTime::now_utc();
        let expires_at = issued_at + time::Duration::hours(ttl_hours as i64);
        let claims = ApiKeyClaims {
            label: label.trim().to_string(),
            scope: scope.unwrap_or(DEFAULT_API_SCOPE).to_string(),
            issued_at: issued_at.unix_timestamp(),
            expires_at: expires_at.unix_timestamp(),
            token_id: random_token_id(),
        };
        let payload = serde_json::to_vec(&claims).map_err(|error| {
            ApiError::internal(format!("failed to encode API key claims: {error}"))
        })?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, payload.as_ref())
            .map_err(|error| ApiError::internal(format!("failed to encrypt API key: {error}")))?;

        let mut token_bytes = nonce_bytes.to_vec();
        token_bytes.extend(ciphertext);
        let api_key = format!("{API_KEY_PREFIX}{}", URL_SAFE_NO_PAD.encode(token_bytes));

        Ok(MintedApiKey {
            api_key,
            label: claims.label,
            scope: claims.scope,
            issued_at,
            expires_at,
        })
    }

    pub fn validate(&self, raw_key: &str) -> Result<ApiKeyClaims, ApiError> {
        let token = raw_key
            .trim()
            .strip_prefix(API_KEY_PREFIX)
            .ok_or_else(|| ApiError::unauthorized("invalid API key prefix"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(token)
            .map_err(|_| ApiError::unauthorized("invalid API key encoding"))?;
        if bytes.len() <= 12 {
            return Err(ApiError::unauthorized("invalid API key payload"));
        }

        let (nonce_bytes, ciphertext) = bytes.split_at(12);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| ApiError::unauthorized("invalid API key"))?;
        let claims: ApiKeyClaims = serde_json::from_slice(&plaintext)
            .map_err(|_| ApiError::unauthorized("invalid API key claims"))?;

        let now = OffsetDateTime::now_utc().unix_timestamp();
        if claims.expires_at <= now {
            return Err(ApiError::unauthorized("API key has expired"));
        }

        Ok(claims)
    }
}

pub fn default_bind_addr() -> Result<SocketAddr, ApiError> {
    let raw = env::var("EDEN_API_BIND").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    raw.parse::<SocketAddr>()
        .map_err(|error| ApiError::bad_request(format!("invalid bind address `{raw}`: {error}")))
}

pub async fn serve(bind_addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let auth = ApiKeyCipher::from_env()?;
    #[cfg(feature = "persistence")]
    let store = {
        let path = env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".to_string());
        EdenStore::open(&path).await?
    };

    let state = ApiState {
        auth,
        #[cfg(feature = "persistence")]
        store,
    };

    let app = build_router(state)?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: ApiState) -> Result<Router, ApiError> {
    let auth_state = state.clone();
    let api_routes = Router::new()
        .route("/live", get(get_live_snapshot))
        .route("/us/live", get(get_us_live_snapshot))
        .route("/polymarket", get(get_polymarket))
        .route("/lineage", get(get_lineage))
        .route("/lineage/history", get(get_lineage_history))
        .route("/lineage/rows", get(get_lineage_rows))
        .route("/causal/flips", get(get_causal_flips))
        .route("/causal/timeline/:leaf_scope_key", get(get_causal_timeline))
        .with_state(state)
        .layer(middleware::from_fn_with_state(auth_state, require_api_key));

    Ok(Router::new()
        .route("/health", get(health))
        .nest("/api", api_routes)
        .layer(build_cors_layer()?))
}

fn build_cors_layer() -> Result<CorsLayer, ApiError> {
    let x_api_key = HeaderName::from_static("x-api-key");
    let mut cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE, x_api_key]);

    let raw = env::var("EDEN_API_ALLOWED_ORIGINS").unwrap_or_else(|_| "*".to_string());
    if raw.trim().is_empty() || raw.trim() == "*" {
        cors = cors.allow_origin(Any);
    } else {
        let origins = raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                HeaderValue::from_str(value).map_err(|error| {
                    ApiError::bad_request(format!(
                        "invalid origin `{value}` in EDEN_API_ALLOWED_ORIGINS: {error}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        cors = cors.allow_origin(AllowOrigin::list(origins));
    }

    Ok(cors)
}

async fn require_api_key(
    State(state): State<ApiState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_api_key(request.headers())
        .ok_or_else(|| ApiError::unauthorized("missing API key"))?;
    state.auth.validate(token)?;
    Ok(next.run(request).await)
}

fn extract_api_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            return Some(token.trim());
        }
        if let Some(token) = value.strip_prefix("bearer ") {
            return Some(token.trim());
        }
    }

    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
}

async fn get_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| ApiError::bad_request("live snapshot not available — is eden running?"))?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ApiError::internal(&format!("invalid snapshot json: {e}")))?;
    Ok(Json(value))
}

async fn get_us_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_US_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/us_live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| ApiError::bad_request("US live snapshot not available — is eden-us running?"))?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| ApiError::internal(&format!("invalid US snapshot json: {e}")))?;
    Ok(Json(value))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eden-api",
        version: env!("CARGO_PKG_VERSION"),
        now: OffsetDateTime::now_utc(),
    })
}

async fn get_polymarket() -> Result<Json<PolymarketResponse>, ApiError> {
    let configs = load_polymarket_configs().map_err(ApiError::bad_request)?;
    let snapshot = fetch_polymarket_snapshot(&configs)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(PolymarketResponse { configs, snapshot }))
}

#[cfg(feature = "persistence")]
async fn get_lineage(
    State(state): State<ApiState>,
    Query(query): Query<LineageQuery>,
) -> Result<Json<LineageResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let filters = filters_from_parts(
        query.label,
        query.bucket,
        query.family,
        query.session,
        query.regime,
    );
    let sort_by = parse_sort_key(query.sort.as_deref())?;
    let alignment = parse_alignment(query.alignment.as_deref())?;

    let stats = state
        .store
        .recent_lineage_stats(limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load lineage stats: {error}")))?;
    let stats = stats
        .filtered(&filters)
        .aligned(alignment)
        .sorted_by(sort_by)
        .truncated(top);

    Ok(Json(LineageResponse {
        window_size: limit,
        filters,
        top,
        sort_by,
        alignment,
        stats,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_lineage() -> Result<Json<LineageResponse>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_lineage_history(
    State(state): State<ApiState>,
    Query(query): Query<LineageHistoryQuery>,
) -> Result<Json<LineageHistoryResponse>, ApiError> {
    let snapshots = bounded(query.snapshots, DEFAULT_LIMIT, MAX_LIMIT, "snapshots")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let latest_only = query.latest_only.unwrap_or(false);
    let filters = filters_from_parts(
        query.label,
        query.bucket,
        query.family,
        query.session,
        query.regime,
    );
    let sort_by = parse_sort_key(query.sort.as_deref())?;
    let alignment = parse_alignment(query.alignment.as_deref())?;

    let rows = state
        .store
        .recent_ranked_lineage_metric_rows(snapshots, top)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to load lineage history rows: {error}"))
        })?;
    let rows = select_lineage_rows(
        &rows,
        &filters,
        snapshots.saturating_mul(top.max(1)),
        latest_only,
        sort_by,
        alignment,
    );
    let records = snapshot_records_from_rows(&rows, &filters, latest_only);

    Ok(Json(LineageHistoryResponse {
        requested_snapshots: snapshots,
        returned_snapshots: records.len(),
        filters,
        top,
        latest_only,
        sort_by,
        alignment,
        snapshots: records,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_lineage_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage history endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_lineage_rows(
    State(state): State<ApiState>,
    Query(query): Query<LineageRowsQuery>,
) -> Result<Json<LineageRowsResponse>, ApiError> {
    let rows_limit = bounded(query.rows, DEFAULT_LIMIT, MAX_LIMIT, "rows")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let latest_only = query.latest_only.unwrap_or(false);
    let filters = filters_from_parts(
        query.label,
        query.bucket,
        query.family,
        query.session,
        query.regime,
    );
    let sort_by = parse_sort_key(query.sort.as_deref())?;
    let alignment = parse_alignment(query.alignment.as_deref())?;

    let ranked_rows = state
        .store
        .recent_ranked_lineage_metric_rows(rows_limit.max(1), top)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load lineage rows: {error}")))?;
    let rows = select_lineage_rows(
        &ranked_rows,
        &filters,
        rows_limit,
        latest_only,
        sort_by,
        alignment,
    );

    Ok(Json(LineageRowsResponse {
        requested_rows: rows_limit,
        returned_rows: rows.len(),
        filters,
        top,
        latest_only,
        sort_by,
        alignment,
        rows,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_lineage_rows() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage row endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_causal_timeline(
    State(state): State<ApiState>,
    Path(leaf_scope_key): Path<String>,
    Query(query): Query<CausalQuery>,
) -> Result<Json<CausalTimelineResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let timeline = state
        .store
        .recent_causal_timeline(&leaf_scope_key, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load causal timeline: {error}")))?;
    let timeline = timeline.ok_or_else(|| {
        ApiError::not_found(format!("no causal timeline found for `{leaf_scope_key}`"))
    })?;

    Ok(Json(CausalTimelineResponse {
        window_size: limit,
        timeline,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_causal_timeline() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "causal timeline endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_causal_flips(
    State(state): State<ApiState>,
    Query(query): Query<CausalQuery>,
) -> Result<Json<CausalFlipsResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let records = state
        .store
        .recent_tick_window(limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load causal flips: {error}")))?;
    let mut history = TickHistory::new(records.len().max(1));
    for record in records {
        history.push(record);
    }
    let timelines = compute_causal_timelines(&history);

    let mut flips = timelines
        .values()
        .flat_map(|timeline| {
            timeline
                .flip_events
                .iter()
                .cloned()
                .map(move |event| FlatCausalFlip {
                    leaf_label: timeline.leaf_label.clone(),
                    leaf_scope_key: timeline.leaf_scope_key.clone(),
                    event,
                })
        })
        .collect::<Vec<_>>();
    flips.sort_by(|a, b| b.event.tick_number.cmp(&a.event.tick_number));

    let sudden = flips
        .iter()
        .filter(|flip| matches!(flip.event.style, CausalFlipStyle::Sudden))
        .count();
    let erosion_driven = flips.len().saturating_sub(sudden);

    Ok(Json(CausalFlipsResponse {
        window_size: limit,
        total: flips.len(),
        sudden,
        erosion_driven,
        flips,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_causal_flips() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "causal flip endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
fn filters_from_parts(
    label: Option<String>,
    bucket: Option<String>,
    family: Option<String>,
    session: Option<String>,
    regime: Option<String>,
) -> LineageFilters {
    LineageFilters {
        label,
        bucket,
        family,
        session,
        market_regime: regime,
    }
}

#[cfg(feature = "persistence")]
fn bounded(
    value: Option<usize>,
    default: usize,
    max: usize,
    name: &str,
) -> Result<usize, ApiError> {
    let value = value.unwrap_or(default);
    if value == 0 {
        return Err(ApiError::bad_request(format!(
            "{name} must be greater than 0"
        )));
    }
    if value > max {
        return Err(ApiError::bad_request(format!("{name} must be <= {max}")));
    }
    Ok(value)
}

#[cfg(feature = "persistence")]
fn parse_sort_key(raw: Option<&str>) -> Result<LineageSortKey, ApiError> {
    match raw.unwrap_or("net") {
        "net" | "net_return" => Ok(LineageSortKey::NetReturn),
        "conv" | "convergence" => Ok(LineageSortKey::ConvergenceScore),
        "external" | "ext" => Ok(LineageSortKey::ExternalDelta),
        value => Err(ApiError::bad_request(format!(
            "invalid sort value `{value}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
fn parse_alignment(raw: Option<&str>) -> Result<LineageAlignmentFilter, ApiError> {
    match raw.unwrap_or("all") {
        "all" => Ok(LineageAlignmentFilter::All),
        "confirm" => Ok(LineageAlignmentFilter::Confirm),
        "contradict" => Ok(LineageAlignmentFilter::Contradict),
        value => Err(ApiError::bad_request(format!(
            "invalid alignment value `{value}`"
        ))),
    }
}

fn random_token_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(feature = "persistence")]
fn select_lineage_rows(
    rows: &[LineageMetricRowRecord],
    filters: &LineageFilters,
    limit: usize,
    latest_only: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
) -> Vec<LineageMetricRowRecord> {
    let mut filtered_rows = rows
        .iter()
        .filter(|row| {
            row_matches_filters(row, filters)
                && matches_lineage_alignment(
                    row.mean_external_delta
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO),
                    alignment,
                )
        })
        .cloned()
        .collect::<Vec<_>>();

    filtered_rows.sort_by(|a, b| {
        lineage_row_metric(b, sort_by)
            .cmp(&lineage_row_metric(a, sort_by))
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| a.label.cmp(&b.label))
    });

    if latest_only {
        if let Some(snapshot_id) = filtered_rows.first().map(|row| row.snapshot_id.clone()) {
            filtered_rows.retain(|row| row.snapshot_id == snapshot_id);
        }
    }

    filtered_rows.truncate(limit);
    filtered_rows
}

#[cfg(feature = "persistence")]
fn lineage_row_metric(row: &LineageMetricRowRecord, sort_by: LineageSortKey) -> Decimal {
    match sort_by {
        LineageSortKey::NetReturn => row.mean_net_return.parse().unwrap_or(Decimal::ZERO),
        LineageSortKey::ConvergenceScore => {
            row.mean_convergence_score.parse().unwrap_or(Decimal::ZERO)
        }
        LineageSortKey::ExternalDelta => row.mean_external_delta.parse().unwrap_or(Decimal::ZERO),
    }
}

#[cfg(feature = "persistence")]
fn matches_lineage_alignment(value: Decimal, alignment: LineageAlignmentFilter) -> bool {
    match alignment {
        LineageAlignmentFilter::All => true,
        LineageAlignmentFilter::Confirm => value > Decimal::ZERO,
        LineageAlignmentFilter::Contradict => value < Decimal::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_api_key_round_trip_works() {
        let cipher = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
        let minted = cipher
            .mint_key("frontend-app", 24, Some("frontend:readonly"))
            .expect("minted");
        assert!(minted.api_key.starts_with(API_KEY_PREFIX));

        let claims = cipher.validate(&minted.api_key).expect("claims");
        assert_eq!(claims.label, "frontend-app");
        assert_eq!(claims.scope, "frontend:readonly");
        assert!(claims.expires_at > claims.issued_at);
    }

    #[test]
    fn invalid_prefix_is_rejected() {
        let cipher = ApiKeyCipher::from_secret("test-master-secret").expect("cipher");
        let error = cipher.validate("not-eden").expect_err("error");
        assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    }
}
