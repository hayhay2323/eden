//! EdenStore persistence methods for IntentBeliefField snapshots.
//!
//! Same pattern as store/belief.rs — `upsert_record_checked` on a
//! stable id derived from (market, snapshot_ts_nanos). Restore on
//! startup is non-fatal: if a row can't deserialize, log-and-continue.

use crate::persistence::intent_belief_snapshot::IntentBeliefSnapshot;

use super::super::store_helpers::{upsert_record_checked, StoreError};
use super::EdenStore;

impl EdenStore {
    pub async fn write_intent_belief_snapshot(
        &self,
        snapshot: &IntentBeliefSnapshot,
    ) -> Result<(), StoreError> {
        let id = format!(
            "{}_{}",
            snapshot.market,
            snapshot.snapshot_ts.timestamp_nanos_opt().unwrap_or(0)
        );
        upsert_record_checked(&self.db, "intent_belief_snapshot", &id, snapshot).await
    }

    pub async fn latest_intent_belief_snapshot(
        &self,
        market: &str,
    ) -> Result<Option<IntentBeliefSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM intent_belief_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC \
                 LIMIT 1",
            )
            .bind(("market", market.to_string()))
            .await?;
        let snaps: Vec<IntentBeliefSnapshot> = result.take(0)?;
        Ok(snaps.into_iter().next())
    }
}
