use super::*;
#[cfg(feature = "persistence")]
use crate::temporal::lineage::{LineageAlignmentFilter, LineageFilters, LineageSortKey};
#[cfg(feature = "persistence")]
use rust_decimal::Decimal;

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_lineage(
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
pub(in crate::api) async fn get_lineage() -> Result<Json<LineageResponse>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_lineage_history(
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
pub(in crate::api) async fn get_lineage_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage history endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_lineage_rows(
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
pub(in crate::api) async fn get_lineage_rows() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "lineage row endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_causal_timeline(
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
pub(in crate::api) async fn get_causal_timeline() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "causal timeline endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_causal_flips(
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
pub(in crate::api) async fn get_causal_flips() -> Result<Json<serde_json::Value>, ApiError> {
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
pub(in crate::api) fn parse_sort_key(raw: Option<&str>) -> Result<LineageSortKey, ApiError> {
    match raw.unwrap_or("net") {
        "net" | "net_return" => Ok(LineageSortKey::NetReturn),
        "follow" | "follow_expectancy" => Ok(LineageSortKey::FollowExpectancy),
        "fade" | "fade_expectancy" => Ok(LineageSortKey::FadeExpectancy),
        "wait" | "wait_expectancy" => Err(ApiError::bad_request(
            "wait_expectancy sort is temporarily unsupported because the metric is not yet meaningfully populated",
        )),
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
                && matches_lineage_alignment(row.mean_external_delta, alignment)
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
        LineageSortKey::NetReturn => row.mean_net_return,
        LineageSortKey::FollowExpectancy => row.follow_expectancy,
        LineageSortKey::FadeExpectancy => row.fade_expectancy,
        LineageSortKey::WaitExpectancy => row.wait_expectancy,
        LineageSortKey::ConvergenceScore => row.mean_convergence_score,
        LineageSortKey::ExternalDelta => row.mean_external_delta,
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
