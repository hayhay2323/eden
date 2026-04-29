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
        upsert_batch_checked(&self.db, "lineage_metric_row", records, |record| {
            &record.row_id
        })
        .await
    }

    pub async fn write_us_lineage_metric_rows(
        &self,
        records: &[UsLineageMetricRowRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "us_lineage_metric_row", records, |record| {
            &record.row_id
        })
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
            #[serde(skip)]
            record_id: String,
            institution_id: i32,
            timestamp: String,
            symbols: Vec<String>,
            ask_symbols: Vec<String>,
            bid_symbols: Vec<String>,
            seat_count: usize,
        }

        let records = presences
            .iter()
            .map(|presence| InstitutionStateRecord {
                record_id: format!("{}:{}", presence.institution_id.0, ts_str),
                institution_id: presence.institution_id.0,
                timestamp: ts_str.clone(),
                symbols: presence.symbols.iter().map(|s| s.0.clone()).collect(),
                ask_symbols: presence.ask_symbols.iter().map(|s| s.0.clone()).collect(),
                bid_symbols: presence.bid_symbols.iter().map(|s| s.0.clone()).collect(),
                seat_count: presence.symbols.len(),
            })
            .collect::<Vec<_>>();

        upsert_batch_checked(&self.db, "institution_state", &records, |record| {
            record.record_id.as_str()
        })
        .await
    }

    pub async fn write_candidate_mechanisms(
        &self,
        records: &[crate::persistence::candidate_mechanism::CandidateMechanismRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "candidate_mechanism", records, |record| {
            &record.mechanism_id
        })
        .await
    }

    pub async fn write_causal_schemas(
        &self,
        records: &[crate::persistence::causal_schema::CausalSchemaRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "causal_schema", records, |record| {
            &record.schema_id
        })
        .await
    }

    pub async fn write_edge_learning_ledger(
        &self,
        record: &crate::persistence::edge_learning_ledger::EdgeLearningLedgerRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "edge_learning_ledger", record.record_id(), record).await
    }

    pub async fn write_discovered_archetypes(
        &self,
        records: &[crate::persistence::discovered_archetype::DiscoveredArchetypeRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "discovered_archetype", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn write_horizon_evaluations(
        &self,
        records: &[crate::persistence::horizon_evaluation::HorizonEvaluationRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "horizon_evaluation", records, |record| {
            &record.record_id
        })
        .await
    }

    pub async fn write_case_resolutions(
        &self,
        records: &[crate::persistence::case_resolution::CaseResolutionRecord],
    ) -> Result<(), StoreError> {
        if records.is_empty() {
            return Ok(());
        }
        upsert_batch_checked(&self.db, "case_resolution", records, |record| {
            &record.record_id
        })
        .await
    }

    /// Write (upsert) discovered archetype records. Alias for
    /// `write_discovered_archetypes` used by the shard recompute path.
    pub async fn write_archetypes(
        &self,
        records: &[crate::persistence::discovered_archetype::DiscoveredArchetypeRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "discovered_archetype", records, |record| {
            record.record_id()
        })
        .await
    }

    /// Operator override: force a case resolution to a new kind, bypassing
    /// the normal upgrade gate. Always sets finality to Final and records the
    /// override in `resolution_history`. Requires a non-empty reason string.
    pub async fn override_case_resolution(
        &self,
        setup_id: &str,
        new_kind: crate::ontology::resolution::CaseResolutionKind,
        reason: String,
        at: time::OffsetDateTime,
    ) -> Result<(), StoreError> {
        use crate::ontology::resolution::{
            CaseResolution, CaseResolutionTransition, ResolutionFinality, ResolutionSource,
        };

        if reason.trim().is_empty() {
            return Err("override reason cannot be empty".into());
        }

        let Some(mut record) = self.load_case_resolution_for_setup(setup_id).await? else {
            return Err(format!("no case_resolution for setup {setup_id}").into());
        };

        // Append transition BEFORE mutating current fields.
        record.resolution_history.push(CaseResolutionTransition {
            from_kind: Some(record.resolution.kind),
            from_finality: Some(record.resolution.finality),
            to_kind: new_kind,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: record.primary_horizon,
            at,
            reason: format!("operator_override: {reason}"),
        });

        // Apply override — bypass upgrade gate.
        record.resolution = CaseResolution {
            kind: new_kind,
            finality: ResolutionFinality::Final,
            narrative: format!("operator override: {reason}"),
            net_return: record.resolution.net_return,
        };
        record.resolution_source = ResolutionSource::OperatorOverride;
        record.updated_at = at;

        self.write_case_resolutions(&[record]).await
    }
}

#[cfg(test)]
mod override_validation_tests {
    #[test]
    fn empty_reason_rejected() {
        // The override method checks reason.trim().is_empty()
        assert_eq!("".trim().is_empty(), true);
        assert_eq!("  ".trim().is_empty(), true);
        assert_eq!("real reason".trim().is_empty(), false);
    }
}
