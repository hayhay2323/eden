use axum::response::sse::Event as SseEvent;

use crate::cases::CaseMarket;

use super::super::foundation::ApiError;

pub(in crate::api) fn matches_optional_text(filter: Option<&str>, value: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(filter) => value
            .map(str::trim)
            .map(|value| value.eq_ignore_ascii_case(filter))
            .unwrap_or(false),
    }
}

pub(in crate::api) fn sse_event_from_error(message: &str) -> SseEvent {
    let sanitized = message.replace('\n', " ");
    SseEvent::default().event("stream_error").data(sanitized)
}

pub(in crate::api) fn bounded(
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

pub(in crate::api) fn parse_case_market(raw: &str) -> Result<CaseMarket, ApiError> {
    CaseMarket::parse(raw)
        .ok_or_else(|| ApiError::bad_request(format!("unsupported market `{raw}`")))
}

pub(in crate::api) fn case_market_slug(market: CaseMarket) -> &'static str {
    match market {
        CaseMarket::Hk => "hk",
        CaseMarket::Us => "us",
    }
}

pub(in crate::api) fn ticks_within_window(a: u64, b: u64, window: u64) -> bool {
    if a >= b {
        a - b <= window
    } else {
        b - a <= window
    }
}

pub(in crate::api) fn normalized_query_value(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}
