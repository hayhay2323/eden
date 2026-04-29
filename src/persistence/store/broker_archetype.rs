//! EdenStore persistence methods for BrokerArchetypeBeliefField.
//! Same pattern as store/intent_belief.rs.

use crate::persistence::broker_archetype_snapshot::BrokerArchetypeSnapshot;

use super::super::store_helpers::{upsert_record_checked, StoreError};
use super::EdenStore;

impl EdenStore {
    pub async fn write_broker_archetype_snapshot(
        &self,
        snapshot: &BrokerArchetypeSnapshot,
    ) -> Result<(), StoreError> {
        let id = format!(
            "{}_{}",
            snapshot.market,
            snapshot.snapshot_ts.timestamp_nanos_opt().unwrap_or(0)
        );
        upsert_record_checked(&self.db, "broker_archetype_snapshot", &id, snapshot).await
    }

    pub async fn latest_broker_archetype_snapshot(
        &self,
        market: &str,
    ) -> Result<Option<BrokerArchetypeSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM broker_archetype_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC \
                 LIMIT 1",
            )
            .bind(("market", market.to_string()))
            .await?;
        let snaps: Vec<BrokerArchetypeSnapshot> = result.take(0)?;
        Ok(snaps.into_iter().next())
    }
}
