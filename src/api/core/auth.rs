use std::env;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use axum::middleware::Next;
use axum::response::Response;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use super::super::foundation::{ApiError, ApiState};

pub(super) fn build_cors_layer() -> Result<CorsLayer, ApiError> {
    let x_api_key = HeaderName::from_static("x-api-key");
    let mut cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([AUTHORIZATION, CONTENT_TYPE, x_api_key]);

    let policy = resolve_cors_policy()?;
    if policy.allow_any {
        cors = cors.allow_origin(Any);
    } else {
        let origins = policy
            .origins
            .iter()
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

pub(super) async fn require_api_key(
    State(state): State<ApiState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_api_key(request.headers())
        .ok_or_else(|| ApiError::unauthorized("missing API key"))?;
    let claims = state.auth.validate(&token)?;
    if state.revocations.is_revoked(&claims.token_id) {
        return Err(ApiError::unauthorized("API key has been revoked"));
    }
    ensure_scope_allows_method(&claims.scope, request.method())?;
    Ok(next.run(request).await)
}

pub(in crate::api) fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        if let Some(token) = value.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
        if let Some(token) = value.strip_prefix("bearer ") {
            return Some(token.trim().to_string());
        }
    }

    if let Some(value) = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
    {
        return Some(value.trim().to_string());
    }

    None
}

pub(super) async fn audit_request(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let started_at = Instant::now();
    let response = next.run(request).await;
    println!(
        "[api audit] method={} path={} status={} duration_ms={}",
        method,
        path,
        response.status().as_u16(),
        started_at.elapsed().as_millis()
    );
    response
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

pub(in crate::api) fn scope_allows_method(scope: &str, method: &Method) -> bool {
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

#[derive(Debug, Clone)]
pub(in crate::api) struct CorsPolicy {
    pub(in crate::api) mode: &'static str,
    pub(in crate::api) allow_any: bool,
    pub(in crate::api) origins: Vec<String>,
}

pub(in crate::api) fn resolve_cors_policy() -> Result<CorsPolicy, ApiError> {
    let raw = env::var("EDEN_API_ALLOWED_ORIGINS").unwrap_or_default();
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        if trimmed == "*" {
            return Ok(CorsPolicy {
                mode: "explicit_env",
                allow_any: true,
                origins: vec!["*".to_string()],
            });
        }
        let origins = trimmed
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if origins.is_empty() {
            return Err(ApiError::bad_request(
                "EDEN_API_ALLOWED_ORIGINS was set but no valid origins were found",
            ));
        }
        return Ok(CorsPolicy {
            mode: "explicit_env",
            allow_any: false,
            origins,
        });
    }

    Ok(CorsPolicy {
        mode: "default_local_whitelist",
        allow_any: false,
        origins: vec![
            "http://127.0.0.1:3000".into(),
            "http://localhost:3000".into(),
            "http://127.0.0.1:3001".into(),
            "http://localhost:3001".into(),
            "http://127.0.0.1:4173".into(),
            "http://localhost:4173".into(),
            "http://127.0.0.1:5173".into(),
            "http://localhost:5173".into(),
            "http://127.0.0.1:8080".into(),
            "http://localhost:8080".into(),
            "http://127.0.0.1:8788".into(),
            "http://localhost:8788".into(),
        ],
    })
}
