use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;
use time::OffsetDateTime;
use tokio::sync::{Mutex, OwnedMutexGuard};

use super::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
use super::schema;
use super::store_helpers::{take_records, upsert_record_checked, StoreError};

#[path = "store/belief.rs"]
mod belief;
#[path = "store/broker_archetype.rs"]
mod broker_archetype;
#[path = "store/intent_belief.rs"]
mod intent_belief;
#[path = "store/knowledge.rs"]
mod knowledge;
mod query;
#[path = "store/regime_fingerprint.rs"]
mod regime_fingerprint;
#[path = "store/workflow.rs"]
mod workflow;
#[path = "store/write.rs"]
mod write;

#[derive(Clone, Debug)]
pub struct EdenStore {
    db: Surreal<Db>,
    /// Per-table mutexes for the wholesale
    /// `DELETE WHERE market=… ; UPSERT *` syncs. SurrealDB 2.x throws
    /// "read or write conflict" on concurrent overlapping key-range
    /// writes within a single table; conflicts are per-table not
    /// global, so a per-table mutex restores parallelism between
    /// distinct tables (e.g. `knowledge_link_state` and
    /// `symbol_perception_state` no longer wait on each other) while
    /// still serialising within a table. Keys are static table-name
    /// strings, lazily inserted on first lock attempt.
    table_locks: Arc<DashMap<&'static str, Arc<Mutex<()>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchemaMigrationState {
    version: i64,
    name: String,
    updated_at: String,
}

impl EdenStore {
    /// Acquire the wholesale-sync lock for `table`, lazily creating
    /// it on first use. Returned guard serialises any
    /// `DELETE WHERE market=… ; UPSERT *` writes against this exact
    /// table. Cross-table sync paths (knowledge_link vs
    /// symbol_perception, etc.) hold disjoint locks and run
    /// concurrently.
    pub(crate) async fn acquire_table_lock(
        &self,
        table: &'static str,
    ) -> OwnedMutexGuard<()> {
        let lock = self
            .table_locks
            .entry(table)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        lock.lock_owned().await
    }

    /// Open or create the SurrealDB database at the given path.
    pub async fn open(path: &str) -> Result<Self, StoreError> {
        eprintln!("[store] opening {}", path);
        let db = Surreal::new::<RocksDb>(path).await?;
        eprintln!("[store] selecting namespace/database");
        db.use_ns("eden").use_db("market").await?;
        Self::apply_schema_migrations(&db, path).await?;
        eprintln!("[store] ready {}", path);
        Ok(Self {
            db,
            table_locks: Arc::new(DashMap::new()),
        })
    }

    async fn apply_schema_migrations(db: &Surreal<Db>, path: &str) -> Result<(), StoreError> {
        db.query(schema::SCHEMA_VERSION_TABLE).await?.check()?;

        let current_version = Self::stored_schema_version(db).await?;
        eprintln!(
            "[store] {} schema current={:?} target={}",
            path,
            current_version,
            schema::LATEST_SCHEMA_VERSION
        );
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
            let started_at = std::time::Instant::now();
            eprintln!(
                "[store] {} applying migration {} {}",
                path, migration.version, migration.name
            );
            db.query(migration.statements).await?.check()?;
            Self::run_post_migration_hook(db, migration.version).await?;
            Self::write_schema_version(db, migration.version, migration.name).await?;
            eprintln!(
                "[store] {} migration {} {} done in {:?}",
                path,
                migration.version,
                migration.name,
                started_at.elapsed()
            );
        }

        Ok(())
    }

    async fn run_post_migration_hook(db: &Surreal<Db>, version: u32) -> Result<(), StoreError> {
        if matches!(version, 38 | 39) {
            Self::backfill_action_workflow_payload_market(db).await?;
        }
        Ok(())
    }

    async fn backfill_action_workflow_payload_market(db: &Surreal<Db>) -> Result<(), StoreError> {
        let workflow_result = db.query("SELECT * FROM action_workflow").await?;
        let mut workflow_updates = 0usize;
        for mut record in take_records::<ActionWorkflowRecord>(workflow_result)? {
            if backfill_market_payload(&mut record.payload, &record.workflow_id, &record.title) {
                upsert_record_checked(db, "action_workflow", record.record_id(), &record).await?;
                workflow_updates += 1;
            }
        }

        let event_result = db.query("SELECT * FROM action_workflow_event").await?;
        let mut event_updates = 0usize;
        for mut record in take_records::<ActionWorkflowEventRecord>(event_result)? {
            if backfill_market_payload(&mut record.payload, &record.workflow_id, &record.title) {
                upsert_record_checked(db, "action_workflow_event", record.record_id(), &record)
                    .await?;
                event_updates += 1;
            }
        }

        if workflow_updates > 0 || event_updates > 0 {
            eprintln!(
                "[store] backfilled workflow payload market for {} action_workflow rows and {} action_workflow_event rows",
                workflow_updates, event_updates
            );
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

fn backfill_market_payload(payload: &mut Value, workflow_id: &str, title: &str) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    if object
        .get("market")
        .and_then(Value::as_str)
        .is_some_and(|market| !market.trim().is_empty())
    {
        return false;
    }
    let market = infer_market_from_workflow_payload(payload, workflow_id, title);
    let Some(market) = market else {
        return false;
    };
    let Some(object) = payload.as_object_mut() else {
        return false;
    };
    object.insert("market".into(), Value::String(market.into()));
    true
}

fn infer_market_from_workflow_payload(
    payload: &Value,
    workflow_id: &str,
    title: &str,
) -> Option<&'static str> {
    let symbol = payload.get("symbol").and_then(Value::as_str);
    let setup_id = payload.get("setup_id").and_then(Value::as_str);
    let case_id = payload.get("case_id").and_then(Value::as_str);
    let haystacks = [symbol, setup_id, case_id, Some(workflow_id), Some(title)];

    if haystacks
        .into_iter()
        .flatten()
        .any(|value| value.contains(".US"))
    {
        return Some("us");
    }
    if haystacks
        .into_iter()
        .flatten()
        .any(|value| value.contains(".HK"))
    {
        return Some("hk");
    }
    None
}

#[cfg(test)]
#[path = "store/tests.rs"]
mod tests;
