#[cfg(feature = "persistence")]
use serde::Deserialize;
use serde::Serialize;

#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::{UsLineageFilters, UsLineageMetricRowRecord};
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::temporal::causality::{CausalFlipEvent, CausalTimeline};
use crate::temporal::lineage::{
    LineageAlignmentFilter, LineageFilters, LineageSortKey, LineageStats,
};
#[cfg(feature = "persistence")]
use crate::us::temporal::causality::{UsCausalFlip, UsCausalTimeline};
#[cfg(feature = "persistence")]
use crate::us::temporal::lineage::UsLineageStats;

#[derive(Debug, Serialize)]
pub(in crate::api) struct LineageResponse {
    pub window_size: usize,
    pub filters: LineageFilters,
    pub top: usize,
    pub sort_by: LineageSortKey,
    pub alignment: LineageAlignmentFilter,
    pub stats: LineageStats,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct LineageHistoryResponse {
    pub requested_snapshots: usize,
    pub returned_snapshots: usize,
    pub filters: LineageFilters,
    pub top: usize,
    pub latest_only: bool,
    pub sort_by: LineageSortKey,
    pub alignment: LineageAlignmentFilter,
    pub snapshots: Vec<LineageSnapshotRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct LineageRowsResponse {
    pub requested_rows: usize,
    pub returned_rows: usize,
    pub filters: LineageFilters,
    pub top: usize,
    pub latest_only: bool,
    pub sort_by: LineageSortKey,
    pub alignment: LineageAlignmentFilter,
    pub rows: Vec<LineageMetricRowRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct CausalTimelineResponse {
    pub window_size: usize,
    pub timeline: CausalTimeline,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Serialize)]
pub(in crate::api) struct FlatCausalFlip {
    pub leaf_label: String,
    pub leaf_scope_key: String,
    pub event: CausalFlipEvent,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct CausalFlipsResponse {
    pub window_size: usize,
    pub total: usize,
    pub sudden: usize,
    pub erosion_driven: usize,
    pub flips: Vec<FlatCausalFlip>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct UsLineageResponse {
    pub window_size: usize,
    pub resolution_lag: u64,
    pub filters: UsLineageFilters,
    pub top: usize,
    pub sort_by: UsLineageSortKey,
    pub stats: UsLineageStats,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct UsLineageHistoryResponse {
    pub requested_snapshots: usize,
    pub returned_snapshots: usize,
    pub filters: UsLineageFilters,
    pub top: usize,
    pub latest_only: bool,
    pub sort_by: UsLineageSortKey,
    pub snapshots: Vec<UsLineageSnapshotRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct UsLineageRowsResponse {
    pub requested_rows: usize,
    pub returned_rows: usize,
    pub filters: UsLineageFilters,
    pub top: usize,
    pub latest_only: bool,
    pub sort_by: UsLineageSortKey,
    pub rows: Vec<UsLineageMetricRowRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct UsCausalTimelineResponse {
    pub window_size: usize,
    pub timeline: UsCausalTimeline,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Serialize)]
pub(in crate::api) struct FlatUsCausalFlip {
    pub symbol: String,
    pub event: UsCausalFlip,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(in crate::api) struct UsCausalFlipsResponse {
    pub window_size: usize,
    pub total: usize,
    pub flips: Vec<FlatUsCausalFlip>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct LineageQuery {
    pub limit: Option<usize>,
    pub top: Option<usize>,
    pub label: Option<String>,
    pub bucket: Option<String>,
    pub family: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
    pub alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct LineageHistoryQuery {
    pub snapshots: Option<usize>,
    pub top: Option<usize>,
    pub latest_only: Option<bool>,
    pub label: Option<String>,
    pub bucket: Option<String>,
    pub family: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
    pub alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct LineageRowsQuery {
    pub rows: Option<usize>,
    pub top: Option<usize>,
    pub latest_only: Option<bool>,
    pub label: Option<String>,
    pub bucket: Option<String>,
    pub family: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
    pub alignment: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct CausalQuery {
    pub limit: Option<usize>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(in crate::api) enum UsLineageSortKey {
    MeanReturn,
    FollowExpectancy,
    FadeExpectancy,
    WaitExpectancy,
    HitRate,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct UsLineageQuery {
    pub limit: Option<usize>,
    pub top: Option<usize>,
    pub resolution_lag: Option<u64>,
    pub template: Option<String>,
    pub bucket: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct UsLineageHistoryQuery {
    pub snapshots: Option<usize>,
    pub top: Option<usize>,
    pub latest_only: Option<bool>,
    pub template: Option<String>,
    pub bucket: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default)]
pub(in crate::api) struct UsLineageRowsQuery {
    pub rows: Option<usize>,
    pub top: Option<usize>,
    pub latest_only: Option<bool>,
    pub template: Option<String>,
    pub bucket: Option<String>,
    pub session: Option<String>,
    pub regime: Option<String>,
    pub sort: Option<String>,
}
