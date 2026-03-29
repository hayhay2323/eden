#[cfg(feature = "persistence")]
use axum::extract::{Path, Query, State};
use axum::Json;

#[cfg(feature = "persistence")]
use super::constants::{
    DEFAULT_LIMIT, DEFAULT_TOP, DEFAULT_US_RESOLUTION_LAG, MAX_LIMIT, MAX_TOP,
};
#[cfg(feature = "persistence")]
use super::core::bounded;
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;

#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::{
    row_matches_filters, snapshot_records_from_rows, LineageMetricRowRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
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
#[cfg(feature = "persistence")]
use crate::us::temporal::buffer::UsTickHistory;
#[cfg(feature = "persistence")]
use crate::us::temporal::causality::{
    compute_causal_timelines as compute_us_causal_timelines, UsCausalFlip, UsCausalTimeline,
};
#[cfg(feature = "persistence")]
use crate::us::temporal::lineage::UsLineageStats;

#[path = "lineage_api/types.rs"]
mod types;
#[path = "lineage_api/us.rs"]
mod us;
#[path = "lineage_api/hk.rs"]
mod hk;

pub(super) use hk::{
    get_causal_flips, get_causal_timeline, get_lineage, get_lineage_history, get_lineage_rows,
};
pub(super) use us::{
    get_us_causal_flips, get_us_causal_timeline, get_us_lineage, get_us_lineage_history,
    get_us_lineage_rows,
};
#[cfg(feature = "persistence")]
pub(in crate::api) use hk::parse_sort_key;
#[cfg(feature = "persistence")]
pub(in crate::api) use us::parse_us_lineage_sort_key;
use types::*;
