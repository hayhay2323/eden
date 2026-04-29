//! EdenStore persistence methods for PressureBeliefField snapshots.
//!
//! Pattern follows other write_* methods in store/write.rs —
//! `upsert_record_checked` against a stable record id derived from
//! (market, snapshot_ts).
//!
//! Caller should log-and-continue on Err; belief snapshots are not
//! golden data — the field can rebuild from scratch.

use crate::persistence::belief_snapshot::BeliefSnapshot;

use super::super::store_helpers::{upsert_record_checked, StoreError};
use super::EdenStore;

impl EdenStore {
    /// Persist a belief snapshot. Record id is "{market}_{unix_ts_nanos}"
    /// so multiple snapshots per market coexist, and the index on
    /// (market, snapshot_ts) keeps the "latest" query cheap.
    pub async fn write_belief_snapshot(&self, snapshot: &BeliefSnapshot) -> Result<(), StoreError> {
        let id = format!(
            "{}_{}",
            snapshot.market,
            snapshot.snapshot_ts.timestamp_nanos_opt().unwrap_or(0)
        );
        upsert_record_checked(&self.db, "belief_snapshot", &id, snapshot).await
    }

    /// Load the most recent belief snapshot for the given market, or
    /// None if no snapshot exists. Returns Err on SurrealDB failure.
    pub async fn latest_belief_snapshot(
        &self,
        market: &str,
    ) -> Result<Option<BeliefSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM belief_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC \
                 LIMIT 1",
            )
            .bind(("market", market.to_string()))
            .await?;

        let snaps: Vec<BeliefSnapshot> = result.take(0)?;
        Ok(snaps.into_iter().next())
    }

    /// List all belief snapshots for a market within a timestamp range,
    /// ordered ascending by snapshot_ts. Used by the dreaming binary
    /// to pick morning + evening snapshots on a given date.
    pub async fn belief_snapshots_in_range(
        &self,
        market: &str,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<BeliefSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM belief_snapshot \
                 WHERE market = $market \
                   AND snapshot_ts >= $from \
                   AND snapshot_ts <= $to \
                 ORDER BY snapshot_ts ASC",
            )
            .bind(("market", market.to_string()))
            .bind(("from", from))
            .bind(("to", to))
            .await?;

        let snaps: Vec<BeliefSnapshot> = result.take(0)?;
        Ok(snaps)
    }
}
