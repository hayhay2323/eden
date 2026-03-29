use serde::{Deserialize, Serialize};
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;
use time::OffsetDateTime;

use crate::ontology::links::CrossStockPresence;
use crate::ontology::{scope_node_id, ReasoningScope, Symbol};
use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
use crate::persistence::agent_graph::{
    KnowledgeEventHistoryRecord, KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord,
    KnowledgeLinkStateRecord, KnowledgeNodeHistoryRecord, KnowledgeNodeStateRecord,
    MacroEventHistoryRecord, MacroEventStateRecord,
};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
use crate::persistence::tactical_setup::TacticalSetupRecord;
use crate::temporal::buffer::TickHistory;
use crate::temporal::causality::{compute_causal_timelines, CausalTimeline};
use crate::temporal::lineage::{compute_lineage_stats, LineageStats};
use crate::temporal::record::TickRecord;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::{
    compute_causal_timelines as compute_us_causal_timelines, UsCausalTimeline,
};
use crate::us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use crate::us::temporal::record::UsTickRecord;
use crate::{
    persistence::us_lineage_metric_row::UsLineageMetricRowRecord,
    persistence::us_lineage_snapshot::UsLineageSnapshotRecord,
};

use super::schema;
use super::store_helpers::{
    exec_query_checked, fetch_latest_timestamp_field, fetch_market_history_records,
    fetch_market_history_records_for_node, fetch_market_state_records,
    fetch_market_state_records_for_node, fetch_optional_market_record_by_field,
    fetch_optional_record_by_field, fetch_ordered_records, fetch_ordered_records_custom,
    fetch_ranked_records, fetch_recent_tick_window, fetch_records_by_field_order,
    fetch_records_by_ids, fetch_records_in_time_range, fetch_tick_archives_in_range,
    sync_market_state_checked, take_records, upsert_batch_checked, upsert_record_checked,
    StoreError,
};

mod query;
#[path = "store/knowledge.rs"]
mod knowledge;
#[path = "store/workflow.rs"]
mod workflow;
#[path = "store/write.rs"]
mod write;

#[derive(Clone, Debug)]
pub struct EdenStore {
    db: Surreal<Db>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaMigrationState {
    version: i64,
    name: String,
    updated_at: String,
}

impl EdenStore {
    /// Open or create the SurrealDB database at the given path.
    pub async fn open(path: &str) -> Result<Self, StoreError> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("eden").use_db("market").await?;
        Self::apply_schema_migrations(&db).await?;
        Ok(Self { db })
    }

    async fn apply_schema_migrations(db: &Surreal<Db>) -> Result<(), StoreError> {
        db.query(schema::SCHEMA_VERSION_TABLE).await?.check()?;

        let current_version = Self::stored_schema_version(db).await?;
        if let Some(version) = current_version {
            if version > schema::LATEST_SCHEMA_VERSION {
                return Err(format!(
                    "database schema version {} is newer than supported version {}",
                    version,
                    schema::LATEST_SCHEMA_VERSION
                )
                .into());
            }
        }

        for migration in schema::pending_migrations(current_version) {
            db.query(migration.statements).await?.check()?;
            Self::write_schema_version(db, migration.version, migration.name).await?;
        }

        Ok(())
    }

    async fn stored_schema_version(db: &Surreal<Db>) -> Result<Option<u32>, StoreError> {
        let state: Option<SchemaMigrationState> =
            db.select(("schema_migration_state", "eden")).await?;
        Ok(state.map(|state| state.version.max(0) as u32))
    }

    async fn write_schema_version(
        db: &Surreal<Db>,
        version: u32,
        name: &str,
    ) -> Result<(), StoreError> {
        let state = SchemaMigrationState {
            version: i64::from(version),
            name: name.to_string(),
            updated_at: OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| OffsetDateTime::now_utc().to_string()),
        };
        let _: Option<SchemaMigrationState> = db
            .upsert(("schema_migration_state", "eden"))
            .content(state)
            .await?;
        Ok(())
    }


}
#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;

fn causal_scope_key(scope: &ReasoningScope) -> String {
    scope_node_id(scope)
}
