#[path = "core/router.rs"]
mod router;
#[path = "core/health.rs"]
mod health;
#[path = "core/auth.rs"]
mod auth;
#[path = "core/common.rs"]
mod common;

#[cfg(test)]
pub(in crate::api) use auth::{extract_api_key, resolve_cors_policy, scope_allows_method};
pub(in crate::api) use common::{
    bounded, case_market_slug, matches_optional_text, normalized_query_value, parse_case_market,
    sse_event_from_error, ticks_within_window,
};
pub(in crate::api) use router::build_router;
