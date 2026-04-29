use axum::extract::{Path, Query, State};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::Json;
use futures::stream;
use serde::{Deserialize, Serialize};

use crate::cases::{
    build_case_briefing, build_case_detail, build_case_list, build_case_review,
    filter_case_list_by_actor, filter_case_list_by_governance_reason_code,
    filter_case_list_by_mechanism, filter_case_list_by_opportunity, filter_case_list_by_owner,
    filter_case_list_by_primary_lens, filter_case_list_by_queue_pin, filter_case_list_by_reviewer,
    load_snapshot, refresh_case_list_governance, CaseBriefingResponse, CaseDetail,
    CaseListResponse, CaseMarket, CaseMechanismStory, CaseMechanismTransitionDigest,
    CaseMechanismTransitionSliceStat, CaseMechanismTransitionStat, CaseReviewResponse,
};
#[cfg(feature = "persistence")]
use crate::cases::{enrich_case_detail, enrich_case_review, enrich_case_summaries};

use super::constants::CASE_STREAM_INTERVAL_SECS;
use super::core::{matches_optional_text, parse_case_market, sse_event_from_error};
use super::foundation::{ApiError, ApiState, JsonEventStream};

#[derive(Debug, Serialize)]
pub(super) struct CaseTransitionAnalyticsResponse {
    pub(super) market: String,
    pub(super) tick: u64,
    pub(super) timestamp: String,
    pub(super) filters: CaseTransitionAnalyticsFilters,
    pub(super) mechanism_transition_breakdown: Vec<CaseMechanismTransitionStat>,
    pub(super) transition_by_sector: Vec<CaseMechanismTransitionSliceStat>,
    pub(super) transition_by_regime: Vec<CaseMechanismTransitionSliceStat>,
    pub(super) transition_by_reviewer: Vec<CaseMechanismTransitionSliceStat>,
    pub(super) recent_mechanism_transitions: Vec<CaseMechanismTransitionDigest>,
}

#[derive(Debug, Serialize)]
pub(super) struct CaseTransitionAnalyticsFilters {
    pub(super) classification: Option<String>,
    pub(super) queue_pin: Option<String>,
    pub(super) primary_lens: Option<String>,
    pub(super) opportunity_horizon: Option<String>,
    pub(super) opportunity_bias: Option<String>,
    pub(super) governance_reason_code: Option<crate::action::workflow::ActionGovernanceReasonCode>,
    pub(super) limit: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct CaseMechanismStoryResponse {
    pub(super) market: String,
    pub(super) setup_id: String,
    pub(super) symbol: String,
    pub(super) title: String,
    pub(super) workflow_state: String,
    pub(super) execution_policy: Option<crate::action::workflow::ActionExecutionPolicy>,
    pub(super) governance: Option<crate::action::workflow::ActionGovernanceContract>,
    pub(super) governance_reason_code: Option<crate::action::workflow::ActionGovernanceReasonCode>,
    pub(super) governance_reason: Option<String>,
    pub(super) market_regime_bias: String,
    pub(super) current_mechanism: Option<String>,
    pub(super) mechanism_story: CaseMechanismStory,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct CaseQuery {
    pub(super) actor: Option<String>,
    pub(super) owner: Option<String>,
    pub(super) reviewer: Option<String>,
    pub(super) queue_pin: Option<String>,
    pub(super) primary_lens: Option<String>,
    pub(super) mechanism: Option<String>,
    pub(super) opportunity_horizon: Option<String>,
    pub(super) opportunity_bias: Option<String>,
    pub(super) governance_reason_code: Option<crate::action::workflow::ActionGovernanceReasonCode>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct CaseTransitionAnalyticsQuery {
    pub(super) actor: Option<String>,
    pub(super) owner: Option<String>,
    pub(super) reviewer: Option<String>,
    pub(super) queue_pin: Option<String>,
    pub(super) primary_lens: Option<String>,
    pub(super) opportunity_horizon: Option<String>,
    pub(super) opportunity_bias: Option<String>,
    pub(super) governance_reason_code: Option<crate::action::workflow::ActionGovernanceReasonCode>,
    pub(super) classification: Option<String>,
    pub(super) limit: Option<usize>,
}

pub(super) async fn get_cases(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseListResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(load_case_list_response(&state, market, &query).await?))
}

pub(super) async fn get_case_briefing(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseBriefingResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let response = load_case_list_response(&state, market, &query).await?;
    Ok(Json(build_case_briefing(&response)))
}

pub(super) async fn get_case_review(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseQuery>,
) -> Result<Json<CaseReviewResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_review_response(&state, market, &query).await?,
    ))
}

pub(super) async fn get_case_transition_analytics(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<CaseTransitionAnalyticsQuery>,
) -> Result<Json<CaseTransitionAnalyticsResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_transition_analytics_response(&state, market, &query).await?,
    ))
}

pub(super) async fn stream_cases(
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

pub(super) async fn stream_case_briefing(
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

pub(super) async fn stream_case_review(
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

pub(super) async fn stream_case_transition_analytics(
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

pub(super) async fn get_case_detail(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Json<CaseDetail>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_detail_response(&state, market, &setup_id).await?,
    ))
}

pub(super) async fn get_case_mechanism_story(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
) -> Result<Json<CaseMechanismStoryResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(Json(
        load_case_mechanism_story_response(&state, market, &setup_id).await?,
    ))
}

pub(super) async fn stream_case_detail(
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

pub(super) async fn stream_case_mechanism_story(
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

pub(super) async fn load_case_list_response(
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
    filter_case_list_by_queue_pin(&mut response, query.queue_pin.as_deref());
    filter_case_list_by_primary_lens(&mut response, query.primary_lens.as_deref());
    filter_case_list_by_mechanism(&mut response, query.mechanism.as_deref());
    filter_case_list_by_opportunity(
        &mut response,
        query.opportunity_horizon.as_deref(),
        query.opportunity_bias.as_deref(),
    );
    filter_case_list_by_governance_reason_code(&mut response, query.governance_reason_code);
    refresh_case_list_governance(&mut response);

    Ok(response)
}

pub(super) async fn load_case_detail_response(
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

pub(super) async fn load_case_review_response(
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

pub(super) async fn load_case_transition_analytics_response(
    state: &ApiState,
    market: CaseMarket,
    query: &CaseTransitionAnalyticsQuery,
) -> Result<CaseTransitionAnalyticsResponse, ApiError> {
    let review =
        load_case_review_response(state, market, &case_query_from_transition_query(query)).await?;
    Ok(build_case_transition_analytics_response(&review, query))
}

pub(super) async fn load_case_mechanism_story_response(
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
        execution_policy: detail.summary.execution_policy,
        governance: detail.summary.governance.clone(),
        governance_reason_code: detail.summary.governance_reason_code,
        governance_reason: detail.summary.governance_reason.clone(),
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
        queue_pin: query.queue_pin.clone(),
        primary_lens: query.primary_lens.clone(),
        mechanism: None,
        opportunity_horizon: query.opportunity_horizon.clone(),
        opportunity_bias: query.opportunity_bias.clone(),
        governance_reason_code: query.governance_reason_code,
    }
}

pub(super) fn build_case_transition_analytics_response(
    review: &CaseReviewResponse,
    query: &CaseTransitionAnalyticsQuery,
) -> CaseTransitionAnalyticsResponse {
    let classification = query
        .classification
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let queue_pin = query
        .queue_pin
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let primary_lens = query
        .primary_lens
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let opportunity_horizon = query
        .opportunity_horizon
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let opportunity_bias = query
        .opportunity_bias
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let governance_reason_code = query.governance_reason_code;
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
            queue_pin,
            primary_lens,
            opportunity_horizon,
            opportunity_bias,
            governance_reason_code,
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
