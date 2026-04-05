use super::*;
#[cfg(feature = "persistence")]
use rust_decimal::Decimal;

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_us_lineage(
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
pub(in crate::api) async fn get_us_lineage() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_us_lineage_history(
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
pub(in crate::api) async fn get_us_lineage_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage history endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_us_lineage_rows(
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
pub(in crate::api) async fn get_us_lineage_rows() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US lineage row endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_us_causal_timeline(
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
pub(in crate::api) async fn get_us_causal_timeline() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US causal timeline endpoints require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn get_us_causal_flips(
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
pub(in crate::api) async fn get_us_causal_flips() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "US causal flip endpoints require building with `--features persistence`",
    ))
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
pub(in crate::api) fn parse_us_lineage_sort_key(
    raw: Option<&str>,
) -> Result<UsLineageSortKey, ApiError> {
    match raw.unwrap_or("return") {
        "return" | "mean_return" | "ret" => Ok(UsLineageSortKey::MeanReturn),
        "follow" | "follow_expectancy" => Ok(UsLineageSortKey::FollowExpectancy),
        "fade" | "fade_expectancy" => Ok(UsLineageSortKey::FadeExpectancy),
        "wait" | "wait_expectancy" => Err(ApiError::bad_request(
            "wait_expectancy sort is temporarily unsupported because the metric is not yet meaningfully populated",
        )),
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
        UsLineageSortKey::FollowExpectancy => item.follow_expectancy,
        UsLineageSortKey::FadeExpectancy => item.fade_expectancy,
        UsLineageSortKey::WaitExpectancy => item.wait_expectancy,
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
        UsLineageSortKey::MeanReturn => row.mean_return,
        UsLineageSortKey::FollowExpectancy => row.follow_expectancy,
        UsLineageSortKey::FadeExpectancy => row.fade_expectancy,
        UsLineageSortKey::WaitExpectancy => row.wait_expectancy,
        UsLineageSortKey::HitRate => row.hit_rate,
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
