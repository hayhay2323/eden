use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::temporal::lineage::LineageStats;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageSnapshotRecord {
    pub snapshot_id: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub window_size: usize,
    pub stats: LineageStats,
}

impl LineageSnapshotRecord {
    pub fn new(
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        stats: &LineageStats,
    ) -> Self {
        Self {
            snapshot_id: format!("lineage:{}:{}", tick_number, window_size),
            tick_number,
            recorded_at,
            window_size,
            stats: stats.clone(),
        }
    }

    pub fn record_id(&self) -> &str {
        &self.snapshot_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_snapshot_record_uses_tick_and_window_in_id() {
        let record = LineageSnapshotRecord::new(
            42,
            OffsetDateTime::UNIX_EPOCH,
            50,
            &LineageStats::default(),
        );
        assert_eq!(record.record_id(), "lineage:42:50");
        assert_eq!(record.tick_number, 42);
        assert_eq!(record.window_size, 50);
    }
}
