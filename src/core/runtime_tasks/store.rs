use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RuntimeTaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RuntimeTaskStatus::Completed | RuntimeTaskStatus::Failed | RuntimeTaskStatus::Cancelled
        )
    }
}

impl std::fmt::Display for RuntimeTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            RuntimeTaskStatus::Pending => "pending",
            RuntimeTaskStatus::Running => "running",
            RuntimeTaskStatus::Completed => "completed",
            RuntimeTaskStatus::Failed => "failed",
            RuntimeTaskStatus::Cancelled => "cancelled",
        };
        f.write_str(value)
    }
}

impl FromStr for RuntimeTaskStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" | "complete" => Ok(Self::Completed),
            "failed" | "error" => Ok(Self::Failed),
            "cancelled" | "canceled" => Ok(Self::Cancelled),
            other => Err(format!("invalid runtime task status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTaskKind {
    RuntimeLoop,
    Analysis,
    Projection,
    Workflow,
    Backfill,
    Operator,
}

impl std::fmt::Display for RuntimeTaskKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            RuntimeTaskKind::RuntimeLoop => "runtime_loop",
            RuntimeTaskKind::Analysis => "analysis",
            RuntimeTaskKind::Projection => "projection",
            RuntimeTaskKind::Workflow => "workflow",
            RuntimeTaskKind::Backfill => "backfill",
            RuntimeTaskKind::Operator => "operator",
        };
        f.write_str(value)
    }
}

impl FromStr for RuntimeTaskKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "runtime_loop" | "runtime-loop" | "runtime" => Ok(Self::RuntimeLoop),
            "analysis" => Ok(Self::Analysis),
            "projection" => Ok(Self::Projection),
            "workflow" => Ok(Self::Workflow),
            "backfill" => Ok(Self::Backfill),
            "operator" => Ok(Self::Operator),
            other => Err(format!("invalid runtime task kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTaskRecord {
    pub id: String,
    pub label: String,
    pub kind: RuntimeTaskKind,
    pub status: RuntimeTaskStatus,
    pub market: Option<String>,
    pub owner: Option<String>,
    pub detail: Option<String>,
    pub metadata: Option<Value>,
    pub last_error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RuntimeTaskFilter {
    pub status: Option<RuntimeTaskStatus>,
    pub kind: Option<RuntimeTaskKind>,
    pub market: Option<String>,
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeTaskCreateRequest {
    pub label: String,
    pub kind: RuntimeTaskKind,
    #[serde(default)]
    pub market: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeTaskStatusUpdateRequest {
    pub status: RuntimeTaskStatus,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RuntimeTaskStore {
    path: Arc<PathBuf>,
    records: Arc<RwLock<BTreeMap<String, RuntimeTaskRecord>>>,
}

#[derive(Debug, Clone)]
pub struct RuntimeTaskHandle {
    store: RuntimeTaskStore,
    task_id: String,
}

impl RuntimeTaskStore {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        let records = load_records_from_path(&path)?;
        let map = records
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect::<BTreeMap<_, _>>();
        Ok(Self {
            path: Arc::new(path),
            records: Arc::new(RwLock::new(map)),
        })
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub fn task_count(&self) -> usize {
        self.records
            .read()
            .map(|records| records.len())
            .unwrap_or(0)
    }

    pub fn list(&self, filter: &RuntimeTaskFilter) -> Vec<RuntimeTaskRecord> {
        let mut records = self
            .records
            .read()
            .map(|records| records.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        records.retain(|record| matches_filter(record, filter));
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        records
    }

    pub fn get(&self, task_id: &str) -> Option<RuntimeTaskRecord> {
        self.records
            .read()
            .ok()
            .and_then(|records| records.get(task_id).cloned())
    }

    pub fn create(&self, request: RuntimeTaskCreateRequest) -> Result<RuntimeTaskRecord, String> {
        let label = request.label.trim();
        if label.is_empty() {
            return Err("runtime task label cannot be empty".into());
        }

        let now = OffsetDateTime::now_utc();
        let record = RuntimeTaskRecord {
            id: generate_task_id(),
            label: label.to_string(),
            kind: request.kind,
            status: RuntimeTaskStatus::Pending,
            market: normalize_optional_text(request.market),
            owner: normalize_optional_text(request.owner),
            detail: normalize_optional_text(request.detail),
            metadata: request.metadata,
            last_error: None,
            created_at: now,
            updated_at: now,
            started_at: None,
            completed_at: None,
        };

        self.write_records(|records| {
            records.insert(record.id.clone(), record.clone());
            Ok(record.clone())
        })
    }

    pub fn create_handle(
        &self,
        request: RuntimeTaskCreateRequest,
    ) -> Result<(RuntimeTaskRecord, RuntimeTaskHandle), String> {
        let record = self.create(request)?;
        let handle = RuntimeTaskHandle {
            store: self.clone(),
            task_id: record.id.clone(),
        };
        Ok((record, handle))
    }

    pub fn update_status(
        &self,
        task_id: &str,
        request: RuntimeTaskStatusUpdateRequest,
    ) -> Result<RuntimeTaskRecord, String> {
        self.write_records(|records| {
            let record = records
                .get_mut(task_id)
                .ok_or_else(|| format!("runtime task not found: {task_id}"))?;
            let now = OffsetDateTime::now_utc();

            record.status = request.status;
            record.updated_at = now;

            if request.status == RuntimeTaskStatus::Running && record.started_at.is_none() {
                record.started_at = Some(now);
            }
            if request.status.is_terminal() {
                record.completed_at = Some(now);
            } else {
                record.completed_at = None;
            }
            if let Some(detail) = request.detail {
                record.detail = normalize_optional_text(Some(detail));
            }
            if let Some(metadata) = request.metadata {
                record.metadata = Some(metadata);
            }

            if request.status == RuntimeTaskStatus::Failed {
                if let Some(error) = request.error {
                    record.last_error = normalize_optional_text(Some(error));
                }
            } else {
                record.last_error = None;
            }

            Ok(record.clone())
        })
    }

    fn write_records<T>(
        &self,
        updater: impl FnOnce(&mut BTreeMap<String, RuntimeTaskRecord>) -> Result<T, String>,
    ) -> Result<T, String> {
        let result;
        let snapshot;
        let disk_records = load_records_from_path(self.path())?
            .into_iter()
            .map(|record| (record.id.clone(), record))
            .collect::<BTreeMap<_, _>>();
        {
            let mut records = self
                .records
                .write()
                .map_err(|_| "runtime task registry lock poisoned".to_string())?;
            let mut merged = disk_records;
            for (id, local) in records.iter() {
                match merged.get(id) {
                    Some(disk) if disk.updated_at > local.updated_at => {}
                    _ => {
                        merged.insert(id.clone(), local.clone());
                    }
                }
            }
            result = updater(&mut merged)?;
            snapshot = merged.values().cloned().collect::<Vec<_>>();
            *records = merged;
        }
        persist_records(self.path(), &snapshot)?;
        Ok(result)
    }
}

impl RuntimeTaskHandle {
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn heartbeat(
        &self,
        detail: impl Into<String>,
        metadata: Value,
    ) -> Result<RuntimeTaskRecord, String> {
        self.store.update_status(
            &self.task_id,
            RuntimeTaskStatusUpdateRequest {
                status: RuntimeTaskStatus::Running,
                detail: Some(detail.into()),
                error: None,
                metadata: Some(metadata),
            },
        )
    }

    pub fn complete(
        &self,
        detail: impl Into<String>,
        metadata: Value,
    ) -> Result<RuntimeTaskRecord, String> {
        self.store.update_status(
            &self.task_id,
            RuntimeTaskStatusUpdateRequest {
                status: RuntimeTaskStatus::Completed,
                detail: Some(detail.into()),
                error: None,
                metadata: Some(metadata),
            },
        )
    }

    pub fn fail(
        &self,
        detail: impl Into<String>,
        error: impl Into<String>,
        metadata: Value,
    ) -> Result<RuntimeTaskRecord, String> {
        self.store.update_status(
            &self.task_id,
            RuntimeTaskStatusUpdateRequest {
                status: RuntimeTaskStatus::Failed,
                detail: Some(detail.into()),
                error: Some(error.into()),
                metadata: Some(metadata),
            },
        )
    }
}

pub fn default_runtime_tasks_path() -> String {
    std::env::var("EDEN_API_RUNTIME_TASKS_PATH")
        .ok()
        .or_else(|| std::env::var("EDEN_RUNTIME_TASKS_PATH").ok())
        .unwrap_or_else(|| "data/runtime_tasks.json".to_string())
}

fn load_records_from_path(path: &Path) -> Result<Vec<RuntimeTaskRecord>, String> {
    match fs::read_to_string(path) {
        Ok(content) => {
            if content.trim().is_empty() {
                Ok(Vec::new())
            } else {
                serde_json::from_str::<Vec<RuntimeTaskRecord>>(&content).map_err(|error| {
                    format!(
                        "failed to decode runtime tasks `{}`: {error}",
                        path.display()
                    )
                })
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!(
            "failed to read runtime tasks `{}`: {error}",
            path.display()
        )),
    }
}

fn persist_records(path: &Path, records: &[RuntimeTaskRecord]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create runtime task directory `{}`: {error}",
                parent.display()
            )
        })?;
    }

    let payload = serde_json::to_string_pretty(records)
        .map_err(|error| format!("failed to encode runtime tasks: {error}"))?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, payload).map_err(|error| {
        format!(
            "failed to write runtime tasks temp file `{}`: {error}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|error| {
        format!(
            "failed to replace runtime tasks file `{}`: {error}",
            path.display()
        )
    })?;
    Ok(())
}

fn matches_filter(record: &RuntimeTaskRecord, filter: &RuntimeTaskFilter) -> bool {
    if let Some(status) = filter.status {
        if record.status != status {
            return false;
        }
    }
    if let Some(kind) = filter.kind {
        if record.kind != kind {
            return false;
        }
    }
    if let Some(market) = filter.market.as_deref() {
        if record.market.as_deref() != Some(market) {
            return false;
        }
    }
    if let Some(owner) = filter.owner.as_deref() {
        if record.owner.as_deref() != Some(owner) {
            return false;
        }
    }
    true
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn generate_task_id() -> String {
    let mut random = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut random);
    format!(
        "task-{}-{:02x}{:02x}{:02x}{:02x}",
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        random[0],
        random[1],
        random[2],
        random[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_runtime_tasks_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "eden-runtime-tasks-{label}-{}-{}.json",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ))
    }

    #[test]
    fn runtime_task_store_round_trip_and_filters() {
        let path = temp_runtime_tasks_path("round-trip");
        let store = RuntimeTaskStore::load(path.clone()).expect("store");
        let created = store
            .create(RuntimeTaskCreateRequest {
                label: "HK operator review".into(),
                kind: RuntimeTaskKind::Operator,
                market: Some("hk".into()),
                owner: Some("ops".into()),
                detail: Some("review open alerts".into()),
                metadata: Some(serde_json::json!({ "priority": "high" })),
            })
            .expect("create task");

        let listed = store.list(&RuntimeTaskFilter {
            kind: Some(RuntimeTaskKind::Operator),
            market: Some("hk".into()),
            ..RuntimeTaskFilter::default()
        });
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(listed[0].status, RuntimeTaskStatus::Pending);

        let updated = store
            .update_status(
                &created.id,
                RuntimeTaskStatusUpdateRequest {
                    status: RuntimeTaskStatus::Running,
                    detail: Some("worker claimed task".into()),
                    error: None,
                    metadata: None,
                },
            )
            .expect("update task");
        assert_eq!(updated.status, RuntimeTaskStatus::Running);
        assert!(updated.started_at.is_some());

        let reloaded = RuntimeTaskStore::load(path.clone()).expect("reload store");
        let fetched = reloaded.get(&created.id).expect("task exists after reload");
        assert_eq!(fetched.status, RuntimeTaskStatus::Running);
        assert_eq!(fetched.detail.as_deref(), Some("worker claimed task"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn runtime_task_handle_updates_lifecycle() {
        let path = temp_runtime_tasks_path("handle");
        let store = RuntimeTaskStore::load(path.clone()).expect("store");
        let (created, handle) = store
            .create_handle(RuntimeTaskCreateRequest {
                label: "HK runtime loop".into(),
                kind: RuntimeTaskKind::RuntimeLoop,
                market: Some("hk".into()),
                owner: Some("runtime".into()),
                detail: Some("starting".into()),
                metadata: None,
            })
            .expect("create handle");

        assert_eq!(created.status, RuntimeTaskStatus::Pending);
        handle
            .heartbeat("running", serde_json::json!({ "tick": 1 }))
            .expect("heartbeat");
        handle
            .complete("finished", serde_json::json!({ "tick": 2 }))
            .expect("complete");

        let fetched = store.get(handle.task_id()).expect("stored task");
        assert_eq!(fetched.status, RuntimeTaskStatus::Completed);
        assert_eq!(fetched.detail.as_deref(), Some("finished"));
        assert!(fetched.completed_at.is_some());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn write_records_merges_disk_updates_from_other_handles() {
        let path = temp_runtime_tasks_path("merge");
        let store_a = RuntimeTaskStore::load(path.clone()).expect("store a");
        let store_b = RuntimeTaskStore::load(path.clone()).expect("store b");

        let created = store_a
            .create(RuntimeTaskCreateRequest {
                label: "HK runtime loop".into(),
                kind: RuntimeTaskKind::RuntimeLoop,
                market: Some("hk".into()),
                owner: Some("runtime".into()),
                detail: Some("starting".into()),
                metadata: None,
            })
            .expect("create task");

        let _analysis = store_b
            .create(RuntimeTaskCreateRequest {
                label: "Longport / Trade feed closure".into(),
                kind: RuntimeTaskKind::Analysis,
                market: None,
                owner: Some("raw-data".into()),
                detail: Some("tracking".into()),
                metadata: None,
            })
            .expect("create analysis task");

        store_a
            .update_status(
                &created.id,
                RuntimeTaskStatusUpdateRequest {
                    status: RuntimeTaskStatus::Running,
                    detail: Some("heartbeat".into()),
                    error: None,
                    metadata: None,
                },
            )
            .expect("heartbeat update");

        let reloaded = RuntimeTaskStore::load(path.clone()).expect("reload");
        let tasks = reloaded.list(&RuntimeTaskFilter::default());
        assert_eq!(tasks.len(), 2);
        assert!(tasks
            .iter()
            .any(|task| task.label == "Longport / Trade feed closure"));

        let _ = fs::remove_file(path);
    }
}
