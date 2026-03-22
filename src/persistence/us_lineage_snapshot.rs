use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::us::temporal::lineage::UsLineageStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsLineageSnapshotRecord {
    pub snapshot_id: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub window_size: usize,
    pub resolution_lag: u64,
    pub stats: UsLineageStats,
}

impl UsLineageSnapshotRecord {
    pub fn new(
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        resolution_lag: u64,
        stats: &UsLineageStats,
    ) -> Self {
        Self {
            snapshot_id: format!(
                "us_lineage:{}:{}:{}",
                tick_number, window_size, resolution_lag
            ),
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            stats: stats.clone(),
        }
    }

    pub fn record_id(&self) -> &str {
        &self.snapshot_id
    }
}
