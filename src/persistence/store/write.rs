use serde::Serialize;

use crate::ontology::links::CrossStockPresence;
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
use crate::temporal::record::TickRecord;
use crate::us::temporal::record::UsTickRecord;
use crate::{
    persistence::us_lineage_metric_row::UsLineageMetricRowRecord,
    persistence::us_lineage_snapshot::UsLineageSnapshotRecord,
};

use super::super::store_helpers::{upsert_batch_checked, upsert_record_checked, StoreError};
use super::EdenStore;

impl EdenStore {
    pub async fn write_tick(&self, record: &TickRecord) -> Result<(), StoreError> {
        let id = format!(
            "tick_{}_{}",
            record.timestamp.unix_timestamp(),
            record.tick_number
        );
        upsert_record_checked(&self.db, "tick_record", &id, record).await
    }

    pub async fn write_us_tick(&self, record: &UsTickRecord) -> Result<(), StoreError> {
        let id = format!(
            "us_tick_{}_{}",
            record.timestamp.unix_timestamp(),
            record.tick_number
        );
        upsert_record_checked(&self.db, "us_tick_record", &id, record).await
    }

    pub async fn write_lineage_snapshot(
        &self,
        record: &LineageSnapshotRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "lineage_snapshot", record.record_id(), record).await
    }

    pub async fn write_us_lineage_snapshot(
        &self,
        record: &UsLineageSnapshotRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "us_lineage_snapshot", record.record_id(), record).await
    }

    pub async fn write_lineage_metric_rows(
        &self,
        records: &[LineageMetricRowRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "lineage_metric_row", records, |record| &record.row_id)
            .await
    }

    pub async fn write_us_lineage_metric_rows(
        &self,
        records: &[UsLineageMetricRowRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "us_lineage_metric_row", records, |record| &record.row_id)
            .await
    }

    pub async fn write_institution_states(
        &self,
        presences: &[CrossStockPresence],
        timestamp: time::OffsetDateTime,
    ) -> Result<(), StoreError> {
        if presences.is_empty() {
            return Ok(());
        }

        let ts_str = timestamp
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        #[derive(Serialize)]
        struct InstitutionStateRecord {
            institution_id: i32,
            timestamp: String,
            symbols: Vec<String>,
            ask_symbols: Vec<String>,
            bid_symbols: Vec<String>,
            seat_count: usize,
        }

        let records = presences
            .iter()
            .map(|presence| {
                (
                    format!("{}:{}", presence.institution_id.0, ts_str),
                    InstitutionStateRecord {
                        institution_id: presence.institution_id.0,
                        timestamp: ts_str.clone(),
                        symbols: presence.symbols.iter().map(|s| s.0.clone()).collect(),
                        ask_symbols: presence.ask_symbols.iter().map(|s| s.0.clone()).collect(),
                        bid_symbols: presence.bid_symbols.iter().map(|s| s.0.clone()).collect(),
                        seat_count: presence.symbols.len(),
                    },
                )
            })
            .collect::<Vec<_>>();

        upsert_batch_checked(&self.db, "institution_state", &records, |(record_id, _)| record_id)
            .await
    }
}
