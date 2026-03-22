use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "persistence")]
use crate::action::workflow::ActionStage;
use crate::cases::{
    build_case_briefing, build_case_detail, build_case_list, build_case_review,
    filter_case_list_by_actor, filter_case_list_by_owner, filter_case_list_by_reviewer,
    load_snapshot, CaseBriefingResponse, CaseDetail, CaseListResponse, CaseMarket,
    CaseMechanismStory, CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat,
    CaseMechanismTransitionStat, CaseReviewResponse,
};
#[cfg(feature = "persistence")]
use crate::cases::{
    enrich_case_detail, enrich_case_review, enrich_case_summaries, workflow_record_payload,
    CaseWorkflowState,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use futures::{stream, Stream};
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
use crate::persistence::us_lineage_metric_row::{
    us_row_matches_filters, us_snapshot_records_from_rows, UsLineageFilters,
    UsLineageMetricRowRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
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
use crate::us::temporal::buffer::UsTickHistory;
#[cfg(feature = "persistence")]
use crate::us::temporal::causality::{
    compute_causal_timelines as compute_us_causal_timelines, UsCausalFlip, UsCausalTimeline,
};
#[cfg(feature = "persistence")]
use crate::us::temporal::lineage::UsLineageStats;

#[cfg(feature = "persistence")]
const DEFAULT_LIMIT: usize = 120;
#[cfg(feature = "persistence")]
const DEFAULT_TOP: usize = 5;
#[cfg(feature = "persistence")]
const MAX_LIMIT: usize = 2_000;
#[cfg(feature = "persistence")]
const MAX_TOP: usize = 100;
#[cfg(feature = "persistence")]
const DEFAULT_US_RESOLUTION_LAG: u64 = 15;
const DEFAULT_API_SCOPE: &str = "frontend:readonly";
const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8787";
const API_KEY_PREFIX: &str = "eden_pk_";
const CASE_STREAM_INTERVAL_SECS: u64 = 4;

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

type JsonEventStream = Pin<Box<dyn Stream<Item = Result<SseEvent, Infallible>> + Send>>;

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

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct UsLineageResponse {
    window_size: usize,
    resolution_lag: u64,
    filters: UsLineageFilters,
    top: usize,
    sort_by: UsLineageSortKey,
    stats: UsLineageStats,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct UsLineageHistoryResponse {
    requested_snapshots: usize,
    returned_snapshots: usize,
    filters: UsLineageFilters,
    top: usize,
    latest_only: bool,
    sort_by: UsLineageSortKey,
    snapshots: Vec<UsLineageSnapshotRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct UsLineageRowsResponse {
    requested_rows: usize,
    returned_rows: usize,
    filters: UsLineageFilters,
    top: usize,
    latest_only: bool,
    sort_by: UsLineageSortKey,
    rows: Vec<UsLineageMetricRowRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct UsCausalTimelineResponse {
    window_size: usize,
    timeline: UsCausalTimeline,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Serialize)]
struct FlatUsCausalFlip {
    symbol: String,
    event: UsCausalFlip,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
struct UsCausalFlipsResponse {
    window_size: usize,
    total: usize,
    flips: Vec<FlatUsCausalFlip>,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug, Serialize)]
struct CaseTransitionAnalyticsResponse {
    market: String,
    tick: u64,
    timestamp: String,
    filters: CaseTransitionAnalyticsFilters,
    mechanism_transition_breakdown: Vec<CaseMechanismTransitionStat>,
    transition_by_sector: Vec<CaseMechanismTransitionSliceStat>,
    transition_by_regime: Vec<CaseMechanismTransitionSliceStat>,
    transition_by_reviewer: Vec<CaseMechanismTransitionSliceStat>,
    recent_mechanism_transitions: Vec<CaseMechanismTransitionDigest>,
}

#[derive(Debug, Serialize)]
struct CaseTransitionAnalyticsFilters {
    classification: Option<String>,
    limit: usize,
}

#[derive(Debug, Serialize)]
struct CaseMechanismStoryResponse {
    market: String,
    setup_id: String,
    symbol: String,
    title: String,
    workflow_state: String,
    market_regime_bias: String,
    current_mechanism: Option<String>,
    mechanism_story: CaseMechanismStory,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
struct CaseTransitionBody {
    target_stage: String,
    actor: Option<String>,
    note: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
struct CaseAssignBody {
    #[serde(default)]
    owner: Option<Option<String>>,
    #[serde(default)]
    reviewer: Option<Option<String>>,
    actor: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct CaseQuery {
    actor: Option<String>,
    owner: Option<String>,
    reviewer: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct CaseTransitionAnalyticsQuery {
    actor: Option<String>,
    owner: Option<String>,
    reviewer: Option<String>,
    classification: Option<String>,
    limit: Option<usize>,
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

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UsLineageSortKey {
    MeanReturn,
    HitRate,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct UsLineageQuery {
    limit: Option<usize>,
    top: Option<usize>,
    resolution_lag: Option<u64>,
    template: Option<String>,
    bucket: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct UsLineageHistoryQuery {
    snapshots: Option<usize>,
    top: Option<usize>,
    latest_only: Option<bool>,
    template: Option<String>,
    bucket: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
struct UsLineageRowsQuery {
    rows: Option<usize>,
    top: Option<usize>,
    latest_only: Option<bool>,
    template: Option<String>,
    bucket: Option<String>,
    session: Option<String>,
    regime: Option<String>,
    sort: Option<String>,
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

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    #[cfg(not(feature = "persistence"))]
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
        .route("/cases/:market", get(get_cases))
        .route("/briefing/:market", get(get_case_briefing))
        .route("/review/:market", get(get_case_review))
        .route(
            "/review/:market/transitions",
            get(get_case_transition_analytics),
        )
        .route("/stream/:market/cases", get(stream_cases))
        .route("/stream/:market/briefing", get(stream_case_briefing))
        .route("/stream/:market/review", get(stream_case_review))
        .route(
            "/stream/:market/review/transitions",
            get(stream_case_transition_analytics),
        )
        .route("/stream/:market/cases/:setup_id", get(stream_case_detail))
        .route(
            "/stream/:market/cases/:setup_id/mechanism",
            get(stream_case_mechanism_story),
        )
        .route("/cases/:market/:setup_id", get(get_case_detail))
        .route(
            "/cases/:market/:setup_id/mechanism",
            get(get_case_mechanism_story),
        )
        .route("/cases/:market/:setup_id/assign", post(post_case_assign))
        .route(
            "/cases/:market/:setup_id/transition",
            post(post_case_transition),
        )
        .route("/us/live", get(get_us_live_snapshot))
        .route("/us/lineage", get(get_us_lineage))
        .route("/us/lineage/history", get(get_us_lineage_history))
        .route("/us/lineage/rows", get(get_us_lineage_rows))
        .route("/us/causal/flips", get(get_us_causal_flips))
        .route("/us/causal/timeline/:symbol", get(get_us_causal_timeline))
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
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
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
    let claims = state.auth.validate(token)?;
    ensure_scope_allows_method(&claims.scope, request.method())?;
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

fn ensure_scope_allows_method(scope: &str, method: &Method) -> Result<(), ApiError> {
    if scope_allows_method(scope, method) {
        return Ok(());
    }

    Err(ApiError::forbidden(format!(
        "API scope `{scope}` does not allow {} requests",
        method.as_str()
    )))
}

fn scope_allows_method(scope: &str, method: &Method) -> bool {
    let requires_write =
        method != Method::GET && method != Method::HEAD && method != Method::OPTIONS;

    scope
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .any(|token| match token {
            "*" | "frontend:*" | "frontend:write" | "frontend:readwrite" | "frontend:operator" => {
                true
            }
            "frontend:readonly" | "frontend:read" => !requires_write,
            _ => false,
        })
}

async fn get_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| ApiError::bad_request("live snapshot not available — is eden running?"))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(&format!("invalid snapshot json: {e}")))?;
    Ok(Json(value))
}

async fn get_us_live_snapshot() -> Result<Json<serde_json::Value>, ApiError> {
    let path = std::env::var("EDEN_US_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/us_live_snapshot.json".into());
    let content = tokio::fs::read_to_string(&path).await.map_err(|_| {
        ApiError::bad_request("US live snapshot not available — is `eden us` running?")
    })?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ApiError::internal(&format!("invalid US snapshot json: {e}")))?;
    Ok(Json(value))
}

async fn get_cases(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseListResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(load_case_list_response(&state, market, &query).await?))
}

async fn get_case_briefing(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseBriefingResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let response = load_case_list_response(&state, market, &query).await?;
    Ok(Json(build_case_briefing(&response)))
}

async fn get_case_review(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseReviewResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_review_response(&state, market, &query).await?,
    ))
}

async fn get_case_transition_analytics(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseTransitionAnalyticsQuery>,
) -> Result<Json<CaseTransitionAnalyticsResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_transition_analytics_response(&state, market, &query).await?,
    ))
}

async fn stream_cases(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let query = query.clone();
        async move { load_case_list_response(&state, market, &query).await }
    }))
}

async fn stream_case_briefing(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let query = query.clone();
        async move {
            let response = load_case_list_response(&state, market, &query).await?;
            Ok(build_case_briefing(&response))
        }
    }))
}

async fn stream_case_review(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let query = query.clone();
        async move { load_case_review_response(&state, market, &query).await }
    }))
}

async fn stream_case_transition_analytics(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseTransitionAnalyticsQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let query = query.clone();
        async move { load_case_transition_analytics_response(&state, market, &query).await }
    }))
}

async fn get_case_detail(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Json<CaseDetail>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_detail_response(&state, market, &setup_id).await?,
    ))
}

async fn get_case_mechanism_story(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Json<CaseMechanismStoryResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_mechanism_story_response(&state, market, &setup_id).await?,
    ))
}

async fn stream_case_detail(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let setup_id = setup_id.clone();
        async move { load_case_detail_response(&state, market, &setup_id).await }
    }))
}

async fn stream_case_mechanism_story(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(case_json_sse(state.clone(), market, move || {
        let state = state.clone();
        let setup_id = setup_id.clone();
        async move { load_case_mechanism_story_response(&state, market, &setup_id).await }
    }))
}

async fn load_case_list_response(
    state: &ApiState,
    market: CaseMarket,
    query: &CaseQuery,
) -> Result<CaseListResponse, ApiError> {
    #[cfg(not(feature = "persistence"))]
    let _ = state;
    let snapshot = load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load cases snapshot: {error}")))?;
    #[cfg(feature = "persistence")]
    let mut response = build_case_list(&snapshot);
    #[cfg(not(feature = "persistence"))]
    let mut response = build_case_list(&snapshot);

    #[cfg(feature = "persistence")]
    {
        enrich_case_summaries(&state.store, &mut response.cases)
            .await
            .map_err(|error| ApiError::internal(format!("failed to enrich cases: {error}")))?;
    }

    filter_case_list_by_owner(&mut response, query.owner.as_deref());
    filter_case_list_by_reviewer(&mut response, query.reviewer.as_deref());
    filter_case_list_by_actor(&mut response, query.actor.as_deref());

    Ok(response)
}

async fn load_case_detail_response(
    state: &ApiState,
    market: CaseMarket,
    setup_id: &str,
) -> Result<CaseDetail, ApiError> {
    #[cfg(not(feature = "persistence"))]
    let _ = state;
    let snapshot = load_snapshot(market).await.map_err(|error| {
        ApiError::internal(format!("failed to load case detail snapshot: {error}"))
    })?;
    #[cfg(feature = "persistence")]
    let mut detail = build_case_detail(&snapshot, setup_id)
        .ok_or_else(|| ApiError::not_found(format!("case `{setup_id}` not found")))?;
    #[cfg(not(feature = "persistence"))]
    let detail = build_case_detail(&snapshot, setup_id)
        .ok_or_else(|| ApiError::not_found(format!("case `{setup_id}` not found")))?;

    #[cfg(feature = "persistence")]
    {
        enrich_case_detail(&state.store, &mut detail)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to enrich case detail: {error}"))
            })?;
    }

    Ok(detail)
}

async fn load_case_review_response(
    state: &ApiState,
    market: CaseMarket,
    query: &CaseQuery,
) -> Result<CaseReviewResponse, ApiError> {
    let response = load_case_list_response(state, market, query).await?;
    #[cfg(feature = "persistence")]
    let mut review = build_case_review(&response);
    #[cfg(not(feature = "persistence"))]
    let review = build_case_review(&response);

    #[cfg(feature = "persistence")]
    {
        enrich_case_review(&state.store, market, &mut review)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to enrich case review: {error}"))
            })?;
    }

    Ok(review)
}

async fn load_case_transition_analytics_response(
    state: &ApiState,
    market: CaseMarket,
    query: &CaseTransitionAnalyticsQuery,
) -> Result<CaseTransitionAnalyticsResponse, ApiError> {
    let review =
        load_case_review_response(state, market, &case_query_from_transition_query(query)).await?;
    Ok(build_case_transition_analytics_response(&review, query))
}

async fn load_case_mechanism_story_response(
    state: &ApiState,
    market: CaseMarket,
    setup_id: &str,
) -> Result<CaseMechanismStoryResponse, ApiError> {
    let detail = load_case_detail_response(state, market, setup_id).await?;
    Ok(CaseMechanismStoryResponse {
        market: match detail.summary.market {
            crate::live_snapshot::LiveMarket::Hk => "hk".into(),
            crate::live_snapshot::LiveMarket::Us => "us".into(),
        },
        setup_id: detail.summary.setup_id.clone(),
        symbol: detail.summary.symbol.clone(),
        title: detail.summary.title.clone(),
        workflow_state: detail.summary.workflow_state.clone(),
        market_regime_bias: detail.summary.market_regime_bias.clone(),
        current_mechanism: detail.mechanism_story.current_mechanism.clone(),
        mechanism_story: detail.mechanism_story,
    })
}

fn case_query_from_transition_query(query: &CaseTransitionAnalyticsQuery) -> CaseQuery {
    CaseQuery {
        actor: query.actor.clone(),
        owner: query.owner.clone(),
        reviewer: query.reviewer.clone(),
    }
}

fn build_case_transition_analytics_response(
    review: &CaseReviewResponse,
    query: &CaseTransitionAnalyticsQuery,
) -> CaseTransitionAnalyticsResponse {
    let classification = query
        .classification
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = query.limit.unwrap_or(8).clamp(1, 64);

    CaseTransitionAnalyticsResponse {
        market: match review.context.market {
            crate::live_snapshot::LiveMarket::Hk => "hk".into(),
            crate::live_snapshot::LiveMarket::Us => "us".into(),
        },
        tick: review.context.tick,
        timestamp: review.context.timestamp.clone(),
        filters: CaseTransitionAnalyticsFilters {
            classification: classification.clone(),
            limit,
        },
        mechanism_transition_breakdown: filter_transition_stats(
            &review.analytics.mechanism_transition_breakdown,
            classification.as_deref(),
            limit,
        ),
        transition_by_sector: filter_transition_slice_stats(
            &review.analytics.transition_by_sector,
            classification.as_deref(),
            limit,
        ),
        transition_by_regime: filter_transition_slice_stats(
            &review.analytics.transition_by_regime,
            classification.as_deref(),
            limit,
        ),
        transition_by_reviewer: filter_transition_slice_stats(
            &review.analytics.transition_by_reviewer,
            classification.as_deref(),
            limit,
        ),
        recent_mechanism_transitions: review
            .analytics
            .recent_mechanism_transitions
            .iter()
            .filter(|item| {
                matches_optional_text(
                    classification.as_deref(),
                    Some(item.classification.as_str()),
                )
            })
            .take(limit)
            .cloned()
            .collect(),
    }
}

fn filter_transition_stats(
    items: &[CaseMechanismTransitionStat],
    classification: Option<&str>,
    limit: usize,
) -> Vec<CaseMechanismTransitionStat> {
    items
        .iter()
        .filter(|item| matches_optional_text(classification, Some(item.classification.as_str())))
        .take(limit)
        .cloned()
        .collect()
}

fn filter_transition_slice_stats(
    items: &[CaseMechanismTransitionSliceStat],
    classification: Option<&str>,
    limit: usize,
) -> Vec<CaseMechanismTransitionSliceStat> {
    items
        .iter()
        .filter(|item| matches_optional_text(classification, Some(item.classification.as_str())))
        .take(limit)
        .cloned()
        .collect()
}

fn matches_optional_text(filter: Option<&str>, value: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(filter) => value
            .map(str::trim)
            .map(|value| value.eq_ignore_ascii_case(filter))
            .unwrap_or(false),
    }
}

fn case_json_sse<T, F, Fut>(state: ApiState, market: CaseMarket, loader: F) -> Sse<JsonEventStream>
where
    T: Serialize + Send + 'static,
    F: Fn() -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = Result<T, ApiError>> + Send + 'static,
{
    let stream = stream::unfold(
        (None::<String>, None::<String>, true),
        move |(mut last_revision, mut last_payload, first)| {
            let state = state.clone();
            let loader = loader.clone();
            async move {
                let mut first = first;
                loop {
                    if !first {
                        tokio::time::sleep(tokio::time::Duration::from_secs(
                            CASE_STREAM_INTERVAL_SECS,
                        ))
                        .await;
                    }
                    first = false;

                    let revision = match case_stream_revision(&state, market).await {
                        Ok(revision) => revision,
                        Err(error) => {
                            let message = format!("stream_revision:{}", error);
                            if last_payload.as_ref() == Some(&message) {
                                continue;
                            }
                            last_payload = Some(message.clone());
                            return Some((
                                Ok(sse_event_from_error(&message)),
                                (last_revision, last_payload, false),
                            ));
                        }
                    };

                    if last_revision.as_ref() == Some(&revision) {
                        continue;
                    }
                    last_revision = Some(revision);

                    let (event, fingerprint) = match loader().await {
                        Ok(payload) => match serde_json::to_string(&payload) {
                            Ok(json) => {
                                if last_payload.as_ref() == Some(&json) {
                                    continue;
                                }
                                (SseEvent::default().data(json.clone()), json)
                            }
                            Err(error) => {
                                let message = format!("encode_error:{error}");
                                if last_payload.as_ref() == Some(&message) {
                                    continue;
                                }
                                (sse_event_from_error(&message), message)
                            }
                        },
                        Err(error) => {
                            let message = format!("stream_error:{}", error);
                            if last_payload.as_ref() == Some(&message) {
                                continue;
                            }
                            (sse_event_from_error(&message), message)
                        }
                    };

                    last_payload = Some(fingerprint);
                    return Some((Ok(event), (last_revision, last_payload, false)));
                }
            }
        },
    );

    let stream: JsonEventStream = Box::pin(stream);
    Sse::new(stream).keep_alive(
        KeepAlive::default()
            .interval(tokio::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn case_stream_revision(state: &ApiState, market: CaseMarket) -> Result<String, ApiError> {
    let (env_var, default_path) = market.snapshot_path();
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let metadata = tokio::fs::metadata(&path).await.map_err(|error| {
        ApiError::internal(format!("failed to stat snapshot `{path}`: {error}"))
    })?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_else(|| "0".into());

    #[cfg(feature = "persistence")]
    let workflow_revision = state
        .store
        .latest_action_workflow_recorded_at()
        .await
        .map_err(|error| ApiError::internal(format!("failed to query workflow revision: {error}")))?
        .map(|timestamp| timestamp.unix_timestamp_nanos().to_string())
        .unwrap_or_else(|| "none".into());
    #[cfg(not(feature = "persistence"))]
    let workflow_revision = {
        let _ = state;
        "none".to_string()
    };

    Ok(format!(
        "{}:{}:{}",
        metadata.len(),
        modified,
        workflow_revision
    ))
}

fn sse_event_from_error(message: &str) -> SseEvent {
    let sanitized = message.replace('\n', " ");
    SseEvent::default().event("stream_error").data(sanitized)
}

#[cfg(feature = "persistence")]
fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "persistence")]
fn normalize_assignment_update(value: Option<Option<String>>) -> Option<Option<String>> {
    value.map(|next| next.and_then(|value| normalize_optional_string(Some(value))))
}

#[cfg(feature = "persistence")]
fn assignment_note(
    owner: Option<&Option<String>>,
    reviewer: Option<&Option<String>>,
) -> Option<String> {
    let mut parts = Vec::new();

    match owner {
        Some(Some(owner)) => parts.push(format!("assigned owner -> {owner}")),
        Some(None) => parts.push("owner cleared".to_string()),
        None => {}
    }

    match reviewer {
        Some(Some(reviewer)) => parts.push(format!("assigned reviewer -> {reviewer}")),
        Some(None) => parts.push("reviewer cleared".to_string()),
        None => {}
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

#[cfg(feature = "persistence")]
async fn post_case_assign(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
    Json(body): Json<CaseAssignBody>,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let market = parse_case_market(&market)?;
    let store = &state.store;
    let setup = store
        .tactical_setup_by_id(&setup_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load tactical setup: {error}")))?
        .ok_or_else(|| ApiError::not_found(format!("case `{setup_id}` not found")))?;
    let workflow_id = setup
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("setup:{}", setup.setup_id));
    let current = store
        .action_workflow_by_id(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load workflow: {error}")))?;

    let timestamp = OffsetDateTime::now_utc();
    let actor = normalize_optional_string(body.actor.clone()).or(Some("frontend".into()));
    let requested_owner = normalize_assignment_update(body.owner.clone());
    let requested_reviewer = normalize_assignment_update(body.reviewer.clone());
    let owner = match requested_owner.as_ref() {
        Some(next) => next.clone(),
        None => current.as_ref().and_then(|record| record.owner.clone()),
    };
    let reviewer = match requested_reviewer.as_ref() {
        Some(next) => next.clone(),
        None => current.as_ref().and_then(|record| record.reviewer.clone()),
    };
    let note = body
        .note
        .clone()
        .or_else(|| assignment_note(requested_owner.as_ref(), requested_reviewer.as_ref()));
    let stage = current
        .as_ref()
        .map(|record| record.current_stage)
        .unwrap_or(ActionStage::Suggest);
    let title = current
        .as_ref()
        .map(|record| record.title.clone())
        .unwrap_or_else(|| setup.title.clone());
    let payload = current
        .as_ref()
        .map(|record| record.payload.clone())
        .unwrap_or_else(|| workflow_record_payload(&setup));

    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: workflow_id.clone(),
        title: title.clone(),
        payload: payload.clone(),
        current_stage: stage,
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        note: note.clone(),
    };
    let event = crate::persistence::action_workflow::ActionWorkflowEventRecord {
        event_id: crate::persistence::action_workflow::event_id_for(&workflow_id, stage, timestamp),
        workflow_id: workflow_id.clone(),
        title,
        payload,
        from_stage: current.as_ref().map(|item| item.current_stage),
        to_stage: stage,
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        note: note.clone(),
    };

    store
        .write_action_workflow_event(&event)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to write assignment event: {error}"))
        })?;
    store
        .write_action_workflow(&record)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to write assignment state: {error}"))
        })?;
    persist_reasoning_assessment_snapshot(&store, market, &setup_id, timestamp, "workflow_update")
        .await;

    Ok(Json(CaseWorkflowState {
        workflow_id,
        stage: stage.as_str().to_string(),
        timestamp,
        actor,
        owner,
        reviewer,
        note,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn post_case_assign() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "case assignment requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn post_case_transition(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
    Json(body): Json<CaseTransitionBody>,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let target_stage = parse_action_stage(&body.target_stage)?;
    let market = parse_case_market(&market)?;
    let store = &state.store;
    let setup = store
        .tactical_setup_by_id(&setup_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load tactical setup: {error}")))?
        .ok_or_else(|| ApiError::not_found(format!("case `{setup_id}` not found")))?;
    let workflow_id = setup
        .workflow_id
        .clone()
        .unwrap_or_else(|| format!("setup:{}", setup.setup_id));
    let current = store
        .action_workflow_by_id(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load workflow: {error}")))?;
    validate_transition(
        current.as_ref().map(|record| record.current_stage),
        target_stage,
    )?;

    let timestamp = OffsetDateTime::now_utc();
    let actor = normalize_optional_string(body.actor.clone()).or(Some("frontend".into()));
    let note = body.note.clone();
    let title = current
        .as_ref()
        .map(|record| record.title.clone())
        .unwrap_or_else(|| setup.title.clone());
    let owner = current.as_ref().and_then(|record| record.owner.clone());
    let reviewer = current.as_ref().and_then(|record| record.reviewer.clone());
    let payload = current
        .as_ref()
        .map(|record| record.payload.clone())
        .unwrap_or_else(|| workflow_record_payload(&setup));

    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: workflow_id.clone(),
        title: title.clone(),
        payload: payload.clone(),
        current_stage: target_stage,
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        note: note.clone(),
    };
    let event = crate::persistence::action_workflow::ActionWorkflowEventRecord {
        event_id: crate::persistence::action_workflow::event_id_for(
            &workflow_id,
            target_stage,
            timestamp,
        ),
        workflow_id: workflow_id.clone(),
        title,
        payload,
        from_stage: current.as_ref().map(|item| item.current_stage),
        to_stage: target_stage,
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        note: note.clone(),
    };

    store
        .write_action_workflow_event(&event)
        .await
        .map_err(|error| ApiError::internal(format!("failed to write workflow event: {error}")))?;
    store
        .write_action_workflow(&record)
        .await
        .map_err(|error| ApiError::internal(format!("failed to write workflow state: {error}")))?;
    persist_reasoning_assessment_snapshot(&store, market, &setup_id, timestamp, "workflow_update")
        .await;

    Ok(Json(CaseWorkflowState {
        workflow_id,
        stage: target_stage.as_str().to_string(),
        timestamp,
        actor,
        owner,
        reviewer,
        note,
    }))
}

#[cfg(feature = "persistence")]
async fn persist_reasoning_assessment_snapshot(
    store: &EdenStore,
    market: CaseMarket,
    setup_id: &str,
    recorded_at: OffsetDateTime,
    source: &str,
) {
    let Ok(snapshot) = load_snapshot(market).await else {
        eprintln!(
            "Warning: failed to reload snapshot for reasoning assessment {}",
            setup_id
        );
        return;
    };
    let Some(mut detail) = build_case_detail(&snapshot, setup_id) else {
        eprintln!(
            "Warning: failed to rebuild case detail for reasoning assessment {}",
            setup_id
        );
        return;
    };
    if let Err(error) = enrich_case_detail(store, &mut detail).await {
        eprintln!(
            "Warning: failed to enrich case detail for reasoning assessment {}: {}",
            setup_id, error
        );
        return;
    }

    let record =
        CaseReasoningAssessmentRecord::from_case_summary(&detail.summary, recorded_at, source);
    if let Err(error) = store.write_case_reasoning_assessment(&record).await {
        eprintln!(
            "Warning: failed to write reasoning assessment for {}: {}",
            setup_id, error
        );
    }
}

#[cfg(not(feature = "persistence"))]
async fn post_case_transition() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "case transitions require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_us_lineage(
    State(state): State<ApiState>,
    Query(query): Query<UsLineageQuery>,
) -> Result<Json<UsLineageResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let resolution_lag = query.resolution_lag.unwrap_or(DEFAULT_US_RESOLUTION_LAG);
    if resolution_lag == 0 {
        return Err(ApiError::bad_request(
            "resolution_lag must be greater than 0",
        ));
    }
    let filters = us_filters_from_parts(query.template, query.bucket, query.session, query.regime);
    let sort_by = parse_us_lineage_sort_key(query.sort.as_deref())?;

    let stats = state
        .store
        .recent_us_lineage_stats(limit, resolution_lag)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load US lineage stats: {error}")))?;
    let stats = filter_us_lineage_stats(&stats, &filters, top, sort_by);

    Ok(Json(UsLineageResponse {
        window_size: limit,
        resolution_lag,
        filters,
        top,
        sort_by,
        stats,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_us_lineage() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_us_lineage_history(
    State(state): State<ApiState>,
    Query(query): Query<UsLineageHistoryQuery>,
) -> Result<Json<UsLineageHistoryResponse>, ApiError> {
    let snapshots = bounded(query.snapshots, DEFAULT_LIMIT, MAX_LIMIT, "snapshots")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let latest_only = query.latest_only.unwrap_or(false);
    let filters = us_filters_from_parts(query.template, query.bucket, query.session, query.regime);
    let sort_by = parse_us_lineage_sort_key(query.sort.as_deref())?;

    let rows = state
        .store
        .recent_ranked_us_lineage_metric_rows(snapshots, top)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to load US lineage history rows: {error}"))
        })?;
    let rows = select_us_lineage_rows(
        &rows,
        &filters,
        snapshots.saturating_mul(top.max(1)),
        latest_only,
        sort_by,
    );
    let records = us_snapshot_records_from_rows(&rows, &filters, latest_only);

    Ok(Json(UsLineageHistoryResponse {
        requested_snapshots: snapshots,
        returned_snapshots: records.len(),
        filters,
        top,
        latest_only,
        sort_by,
        snapshots: records,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_us_lineage_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage history endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_us_lineage_rows(
    State(state): State<ApiState>,
    Query(query): Query<UsLineageRowsQuery>,
) -> Result<Json<UsLineageRowsResponse>, ApiError> {
    let rows_limit = bounded(query.rows, DEFAULT_LIMIT, MAX_LIMIT, "rows")?;
    let top = bounded(query.top, DEFAULT_TOP, MAX_TOP, "top")?;
    let latest_only = query.latest_only.unwrap_or(false);
    let filters = us_filters_from_parts(query.template, query.bucket, query.session, query.regime);
    let sort_by = parse_us_lineage_sort_key(query.sort.as_deref())?;

    let ranked_rows = state
        .store
        .recent_ranked_us_lineage_metric_rows(rows_limit.max(1), top)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load US lineage rows: {error}")))?;
    let rows = select_us_lineage_rows(&ranked_rows, &filters, rows_limit, latest_only, sort_by);

    Ok(Json(UsLineageRowsResponse {
        requested_rows: rows_limit,
        returned_rows: rows.len(),
        filters,
        top,
        latest_only,
        sort_by,
        rows,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_us_lineage_rows() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage row endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_us_causal_timeline(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<CausalQuery>,
) -> Result<Json<UsCausalTimelineResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let timeline = state
        .store
        .recent_us_causal_timeline(&symbol, limit)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to load US causal timeline: {error}"))
        })?;
    let timeline = timeline.ok_or_else(|| {
        ApiError::not_found(format!("no US causal timeline found for `{symbol}`"))
    })?;

    Ok(Json(UsCausalTimelineResponse {
        window_size: limit,
        timeline,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_us_causal_timeline() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US causal timeline endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn get_us_causal_flips(
    State(state): State<ApiState>,
    Query(query): Query<CausalQuery>,
) -> Result<Json<UsCausalFlipsResponse>, ApiError> {
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let records = state
        .store
        .recent_us_tick_window(limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load US causal flips: {error}")))?;
    let mut history = UsTickHistory::new(records.len().max(1));
    for record in records {
        history.push(record);
    }
    let timelines = compute_us_causal_timelines(&history);

    let mut flips = timelines
        .values()
        .flat_map(|timeline| {
            timeline
                .flips
                .iter()
                .cloned()
                .map(move |event| FlatUsCausalFlip {
                    symbol: timeline.symbol.0.clone(),
                    event,
                })
        })
        .collect::<Vec<_>>();
    flips.sort_by(|a, b| b.event.tick.cmp(&a.event.tick));

    Ok(Json(UsCausalFlipsResponse {
        window_size: limit,
        total: flips.len(),
        flips,
    }))
}

#[cfg(not(feature = "persistence"))]
async fn get_us_causal_flips() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US causal flip endpoints require building with `--features persistence`",
    ))
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
fn us_filters_from_parts(
    template: Option<String>,
    bucket: Option<String>,
    session: Option<String>,
    regime: Option<String>,
) -> UsLineageFilters {
    UsLineageFilters {
        template,
        bucket,
        session,
        market_regime: regime,
    }
}

#[cfg(feature = "persistence")]
fn parse_us_lineage_sort_key(raw: Option<&str>) -> Result<UsLineageSortKey, ApiError> {
    match raw.unwrap_or("return") {
        "return" | "mean_return" | "ret" => Ok(UsLineageSortKey::MeanReturn),
        "hit" | "hit_rate" => Ok(UsLineageSortKey::HitRate),
        value => Err(ApiError::bad_request(format!(
            "invalid US lineage sort value `{value}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
fn filter_us_lineage_stats(
    stats: &UsLineageStats,
    filters: &UsLineageFilters,
    top: usize,
    sort_by: UsLineageSortKey,
) -> UsLineageStats {
    let mut by_template = if filters.session.is_some() || filters.market_regime.is_some() {
        Vec::new()
    } else if us_bucket_matches(filters.bucket.as_deref(), "by_template") {
        stats
            .by_template
            .iter()
            .filter(|item| us_matches_text(filters.template.as_deref(), &item.template))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut by_context = if us_bucket_matches(filters.bucket.as_deref(), "by_context") {
        stats
            .by_context
            .iter()
            .filter(|item| {
                us_matches_text(filters.template.as_deref(), &item.template)
                    && us_matches_text(filters.session.as_deref(), &item.session)
                    && us_matches_text(filters.market_regime.as_deref(), &item.market_regime)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    sort_us_lineage_contexts(&mut by_template, sort_by);
    sort_us_lineage_contexts(&mut by_context, sort_by);
    by_template.truncate(top);
    by_context.truncate(top);

    UsLineageStats {
        by_template,
        by_context,
    }
}

#[cfg(feature = "persistence")]
fn sort_us_lineage_contexts(
    items: &mut [crate::us::temporal::lineage::UsLineageContextStats],
    sort_by: UsLineageSortKey,
) {
    items.sort_by(|a, b| {
        us_lineage_metric_for_stat(b, sort_by)
            .cmp(&us_lineage_metric_for_stat(a, sort_by))
            .then_with(|| a.template.cmp(&b.template))
            .then_with(|| a.session.cmp(&b.session))
    });
}

#[cfg(feature = "persistence")]
fn us_lineage_metric_for_stat(
    item: &crate::us::temporal::lineage::UsLineageContextStats,
    sort_by: UsLineageSortKey,
) -> Decimal {
    match sort_by {
        UsLineageSortKey::MeanReturn => item.mean_return,
        UsLineageSortKey::HitRate => item.hit_rate,
    }
}

#[cfg(feature = "persistence")]
fn select_us_lineage_rows(
    rows: &[UsLineageMetricRowRecord],
    filters: &UsLineageFilters,
    limit: usize,
    latest_only: bool,
    sort_by: UsLineageSortKey,
) -> Vec<UsLineageMetricRowRecord> {
    let mut filtered_rows = rows
        .iter()
        .filter(|row| us_row_matches_filters(row, filters))
        .cloned()
        .collect::<Vec<_>>();

    filtered_rows.sort_by(|a, b| {
        us_lineage_row_metric(b, sort_by)
            .cmp(&us_lineage_row_metric(a, sort_by))
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| a.template.cmp(&b.template))
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
fn us_lineage_row_metric(row: &UsLineageMetricRowRecord, sort_by: UsLineageSortKey) -> Decimal {
    match sort_by {
        UsLineageSortKey::MeanReturn => row.mean_return.parse().unwrap_or(Decimal::ZERO),
        UsLineageSortKey::HitRate => row.hit_rate.parse().unwrap_or(Decimal::ZERO),
    }
}

#[cfg(feature = "persistence")]
fn us_bucket_matches(filter: Option<&str>, bucket: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => filter.eq_ignore_ascii_case(bucket),
    }
}

#[cfg(feature = "persistence")]
fn us_matches_text(filter: Option<&str>, value: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => value
            .to_ascii_lowercase()
            .contains(&filter.to_ascii_lowercase()),
    }
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

fn parse_case_market(raw: &str) -> Result<CaseMarket, ApiError> {
    CaseMarket::parse(raw)
        .ok_or_else(|| ApiError::bad_request(format!("unsupported market `{raw}`")))
}

#[cfg(feature = "persistence")]
fn parse_action_stage(raw: &str) -> Result<ActionStage, ApiError> {
    match raw {
        "suggest" => Ok(ActionStage::Suggest),
        "confirm" => Ok(ActionStage::Confirm),
        "execute" => Ok(ActionStage::Execute),
        "monitor" => Ok(ActionStage::Monitor),
        "review" => Ok(ActionStage::Review),
        _ => Err(ApiError::bad_request(format!(
            "unsupported action stage `{raw}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
fn validate_transition(current: Option<ActionStage>, target: ActionStage) -> Result<(), ApiError> {
    match current {
        None if matches!(target, ActionStage::Suggest | ActionStage::Confirm | ActionStage::Review) => Ok(()),
        None => Err(ApiError::bad_request(
            "workflow does not exist yet; first transition must be `suggest`, `confirm`, or `review`",
        )),
        Some(stage) if stage == target => Err(ApiError::bad_request(
            "workflow is already in the requested stage",
        )),
        Some(_) if target == ActionStage::Review => Ok(()),
        Some(stage) if stage.next() == Some(target) => Ok(()),
        Some(stage) => Err(ApiError::bad_request(format!(
            "invalid transition from `{}` to `{}`",
            stage.as_str(),
            target.as_str()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cases::{
        CaseHumanReviewReasonStat, CaseMechanismDriftPoint, CaseMechanismStat,
        CaseMechanismTransitionDigest, CaseMechanismTransitionSliceStat,
        CaseMechanismTransitionStat, CaseReviewAnalytics, CaseReviewBuckets, CaseReviewMetrics,
        CaseReviewResponse,
    };
    use crate::live_snapshot::{LiveMarket, LiveMarketRegime, LiveScorecard, LiveStressSnapshot};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

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

    #[test]
    fn readonly_scope_blocks_mutations() {
        assert!(scope_allows_method("frontend:readonly", &Method::GET));
        assert!(!scope_allows_method("frontend:readonly", &Method::POST));
    }

    #[test]
    fn write_scope_allows_mutations() {
        assert!(scope_allows_method("frontend:write", &Method::POST));
        assert!(scope_allows_method("frontend:write", &Method::GET));
    }

    #[test]
    fn transition_analytics_response_filters_and_limits() {
        let review = CaseReviewResponse {
            context: crate::cases::CaseMarketContext {
                market: LiveMarket::Us,
                tick: 42,
                timestamp: "2026-03-22T00:00:00Z".into(),
                stock_count: 0,
                edge_count: 0,
                hypothesis_count: 0,
                observation_count: 0,
                active_positions: 0,
                market_regime: LiveMarketRegime {
                    bias: "risk_off".into(),
                    confidence: dec!(0.7),
                    breadth_up: dec!(0.2),
                    breadth_down: dec!(0.6),
                    average_return: dec!(-0.03),
                    directional_consensus: Some(dec!(-0.1)),
                    pre_market_sentiment: None,
                },
                stress: LiveStressSnapshot {
                    composite_stress: dec!(0.5),
                    sector_synchrony: None,
                    pressure_consensus: None,
                    momentum_consensus: None,
                    pressure_dispersion: None,
                    volume_anomaly: None,
                },
                scorecard: LiveScorecard {
                    total_signals: 0,
                    resolved_signals: 0,
                    hits: 0,
                    misses: 0,
                    hit_rate: dec!(0),
                    mean_return: dec!(0),
                },
                events: vec![],
                cross_market_signals: vec![],
                cross_market_anomalies: vec![],
                lineage: vec![],
            },
            metrics: CaseReviewMetrics {
                in_flight: 0,
                under_review: 0,
                at_risk: 0,
                high_conviction: 0,
            },
            buckets: CaseReviewBuckets {
                in_flight: vec![],
                under_review: vec![],
                at_risk: vec![],
                high_conviction: vec![],
            },
            analytics: CaseReviewAnalytics {
                mechanism_stats: vec![CaseMechanismStat {
                    mechanism: "Capital Rotation".into(),
                    cases: 1,
                    under_review: 0,
                    at_risk: 0,
                    high_conviction: 1,
                    avg_score: dec!(0.6),
                }],
                reviewer_corrections: vec![],
                mechanism_drift: vec![CaseMechanismDriftPoint {
                    window_label: "03-22 10:00".into(),
                    top_mechanism: Some("Capital Rotation".into()),
                    top_cases: 1,
                    avg_score: dec!(0.6),
                    dominant_factor: Some("Substitution Flow".into()),
                }],
                mechanism_transition_breakdown: vec![
                    CaseMechanismTransitionStat {
                        classification: "regime_shift".into(),
                        count: 2,
                    },
                    CaseMechanismTransitionStat {
                        classification: "mechanism_decay".into(),
                        count: 1,
                    },
                ],
                transition_by_sector: vec![
                    CaseMechanismTransitionSliceStat {
                        key: "Technology".into(),
                        classification: "regime_shift".into(),
                        count: 2,
                    },
                    CaseMechanismTransitionSliceStat {
                        key: "Financials".into(),
                        classification: "mechanism_decay".into(),
                        count: 1,
                    },
                ],
                transition_by_regime: vec![
                    CaseMechanismTransitionSliceStat {
                        key: "risk_off:high".into(),
                        classification: "regime_shift".into(),
                        count: 2,
                    },
                    CaseMechanismTransitionSliceStat {
                        key: "neutral:low".into(),
                        classification: "mechanism_decay".into(),
                        count: 1,
                    },
                ],
                transition_by_reviewer: vec![CaseMechanismTransitionSliceStat {
                    key: "reviewer-a".into(),
                    classification: "regime_shift".into(),
                    count: 1,
                }],
                recent_mechanism_transitions: vec![
                    CaseMechanismTransitionDigest {
                        setup_id: "setup:1".into(),
                        symbol: "A.US".into(),
                        title: "A".into(),
                        sector: Some("Technology".into()),
                        regime: Some("risk_off:high".into()),
                        reviewer: Some("reviewer-a".into()),
                        from_mechanism: Some("Mechanical Execution Signature".into()),
                        to_mechanism: Some("Capital Rotation".into()),
                        classification: "regime_shift".into(),
                        confidence: dec!(0.82),
                        summary: "shift".into(),
                        recorded_at: OffsetDateTime::UNIX_EPOCH,
                    },
                    CaseMechanismTransitionDigest {
                        setup_id: "setup:2".into(),
                        symbol: "B.US".into(),
                        title: "B".into(),
                        sector: Some("Financials".into()),
                        regime: Some("neutral:low".into()),
                        reviewer: Some("reviewer-b".into()),
                        from_mechanism: Some("Narrative Failure".into()),
                        to_mechanism: Some("Fragility Build-up".into()),
                        classification: "mechanism_decay".into(),
                        confidence: dec!(0.61),
                        summary: "decay".into(),
                        recorded_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
                    },
                ],
                reviewer_doctrine: vec![],
                human_review_reasons: vec![CaseHumanReviewReasonStat {
                    reason: "Mechanism Mismatch".into(),
                    count: 1,
                }],
                invalidation_patterns: vec![],
                learning_feedback:
                    crate::pipeline::learning_loop::ReasoningLearningFeedback::default(),
            },
        };

        let response = build_case_transition_analytics_response(
            &review,
            &CaseTransitionAnalyticsQuery {
                classification: Some("regime_shift".into()),
                limit: Some(1),
                ..CaseTransitionAnalyticsQuery::default()
            },
        );

        assert_eq!(response.market, "us");
        assert_eq!(response.mechanism_transition_breakdown.len(), 1);
        assert_eq!(response.transition_by_sector.len(), 1);
        assert_eq!(response.transition_by_regime.len(), 1);
        assert_eq!(response.transition_by_reviewer.len(), 1);
        assert_eq!(response.recent_mechanism_transitions.len(), 1);
        assert_eq!(
            response.recent_mechanism_transitions[0].classification,
            "regime_shift"
        );
    }
}
