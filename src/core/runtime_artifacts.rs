use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::core::market::MarketId;

pub const RUNTIME_ARTIFACT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeArtifactKind {
    RuntimeStageTrace,
    VisualGraphFrame,
    EncodedTickFrame,
    BpMarginals,
    BpMessageTrace,
    SubKgSnapshot,
    RuntimeHealthTick,
}

impl RuntimeArtifactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RuntimeStageTrace => "runtime_stage_trace",
            Self::VisualGraphFrame => "visual_graph_frame",
            Self::EncodedTickFrame => "encoded_tick_frame",
            Self::BpMarginals => "bp_marginals",
            Self::BpMessageTrace => "bp_message_trace",
            Self::SubKgSnapshot => "subkg_snapshot",
            Self::RuntimeHealthTick => "runtime_health_tick",
        }
    }

    fn file_stem(self) -> &'static str {
        match self {
            Self::RuntimeStageTrace => "eden-runtime-stage",
            Self::VisualGraphFrame => "eden-visual-graph-frame",
            Self::EncodedTickFrame => "eden-encoded-tick-frame",
            Self::BpMarginals => "eden-bp-marginals",
            Self::BpMessageTrace => "eden-bp-message-trace",
            Self::SubKgSnapshot => "eden-subkg",
            Self::RuntimeHealthTick => "eden-runtime-health",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeArtifactWriteError {
    pub artifact: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeHealthTick {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub stage_count: usize,
    pub stage_plan: String,
    pub stage_plan_expected_count: usize,
    pub stage_plan_covered: bool,
    pub bp_iterations: usize,
    pub bp_converged: bool,
    pub bp_nodes: usize,
    pub bp_edges: usize,
    pub bp_master_graph_edges: usize,
    pub bp_master_runtime_edges: usize,
    pub bp_build_inputs_ms: u64,
    pub bp_run_ms: u64,
    pub bp_message_trace_write_ms: u64,
    pub bp_marginals_write_ms: u64,
    pub bp_shadow_observed_incident_edges: usize,
    pub bp_shadow_low_weight_edges: usize,
    pub bp_shadow_retained_edges: usize,
    pub bp_shadow_pruned_edges: usize,
    pub bp_shadow_stock_to_stock_edges: usize,
    pub bp_shadow_unknown_edges: usize,
    pub observed_priors: usize,
    pub frontier_symbols: usize,
    pub frontier_nodes: usize,
    pub frontier_edges: usize,
    pub frontier_hops: usize,
    pub frontier_candidates: usize,
    pub frontier_dry_run_updates: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier_dry_run_mean_abs_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier_dry_run_max_abs_delta: Option<f64>,
    pub frontier_pressure_cache_updates: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier_pressure_cache_mean_abs_delta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier_pressure_cache_max_abs_delta: Option<f64>,
    pub frontier_pressure_gate_passed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontier_pressure_gate_noise_floor: Option<f64>,
    pub frontier_next_proposals: usize,
    pub frontier_loop_rounds: usize,
    pub frontier_loop_final_proposals: usize,
    pub lead_lag_events: usize,
    pub probe_emitted: usize,
    pub probe_evaluated: usize,
    pub probe_pending: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_mean_accuracy: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_write_errors: Vec<RuntimeArtifactWriteError>,
}

pub fn record_artifact_result<T>(
    errors: &mut Vec<RuntimeArtifactWriteError>,
    artifact: &'static str,
    result: io::Result<T>,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push(RuntimeArtifactWriteError {
                artifact: artifact.to_string(),
                error: error.to_string(),
            });
            None
        }
    }
}

pub fn write_runtime_health_tick(
    market: MarketId,
    row: &RuntimeHealthTick,
) -> io::Result<RuntimeArtifactWrite> {
    RuntimeArtifactStore::default().append_json_line(
        RuntimeArtifactKind::RuntimeHealthTick,
        market,
        row,
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeArtifactEnvelope<T> {
    pub schema_version: u16,
    pub kind: RuntimeArtifactKind,
    pub market: String,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeArtifactWrite {
    pub schema_version: u16,
    pub kind: RuntimeArtifactKind,
    pub market: MarketId,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeArtifactStore {
    root: PathBuf,
}

impl RuntimeArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn path(&self, kind: RuntimeArtifactKind, market: MarketId) -> PathBuf {
        self.root
            .join(".run")
            .join(format!("{}-{}.ndjson", kind.file_stem(), market.slug()))
    }

    pub fn append_json_line<T: Serialize>(
        &self,
        kind: RuntimeArtifactKind,
        market: MarketId,
        payload: &T,
    ) -> io::Result<RuntimeArtifactWrite> {
        let path = self.path(kind, market);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let envelope = RuntimeArtifactEnvelope {
            schema_version: RUNTIME_ARTIFACT_SCHEMA_VERSION,
            kind,
            market: market.slug().to_string(),
            payload,
        };
        let line = serde_json::to_string(&envelope)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{line}")?;
        Ok(RuntimeArtifactWrite {
            schema_version: RUNTIME_ARTIFACT_SCHEMA_VERSION,
            kind,
            market,
            path: path.display().to_string(),
        })
    }

    pub fn read_latest_json_line<T: DeserializeOwned>(
        &self,
        kind: RuntimeArtifactKind,
        market: MarketId,
    ) -> io::Result<Option<RuntimeArtifactEnvelope<T>>> {
        let path = self.path(kind, market);
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let Some(line) = content.lines().rev().find(|line| !line.trim().is_empty()) else {
            return Ok(None);
        };
        serde_json::from_str::<RuntimeArtifactEnvelope<T>>(line)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    pub fn read_latest_json_payload<T: DeserializeOwned>(
        &self,
        kind: RuntimeArtifactKind,
        market: MarketId,
    ) -> io::Result<Option<T>> {
        let path = self.path(kind, market);
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let Some(line) = content.lines().rev().find(|line| !line.trim().is_empty()) else {
            return Ok(None);
        };
        if let Ok(envelope) = serde_json::from_str::<RuntimeArtifactEnvelope<T>>(line) {
            return Ok(Some(envelope.payload));
        }
        serde_json::from_str::<T>(line)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}

impl Default for RuntimeArtifactStore {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Row {
        tick: u64,
        label: String,
    }

    #[test]
    fn store_appends_schema_envelope_and_reads_latest_row() {
        let root = unique_temp_dir();
        let store = RuntimeArtifactStore::new(&root);

        let first = store
            .append_json_line(
                RuntimeArtifactKind::VisualGraphFrame,
                MarketId::Us,
                &Row {
                    tick: 1,
                    label: "old".to_string(),
                },
            )
            .expect("append first row");
        let second = store
            .append_json_line(
                RuntimeArtifactKind::VisualGraphFrame,
                MarketId::Us,
                &Row {
                    tick: 2,
                    label: "new".to_string(),
                },
            )
            .expect("append second row");

        assert_eq!(first.schema_version, RUNTIME_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(second.kind, RuntimeArtifactKind::VisualGraphFrame);
        assert!(second
            .path
            .ends_with(".run/eden-visual-graph-frame-us.ndjson"));

        let latest: RuntimeArtifactEnvelope<Row> = store
            .read_latest_json_line(RuntimeArtifactKind::VisualGraphFrame, MarketId::Us)
            .expect("read latest row")
            .expect("latest row exists");

        assert_eq!(latest.schema_version, RUNTIME_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(latest.payload.tick, 2);
        assert_eq!(latest.payload.label, "new");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn store_ignores_blank_lines_when_reading_latest_row() {
        let root = unique_temp_dir();
        let store = RuntimeArtifactStore::new(&root);
        fs::create_dir_all(root.join(".run")).expect("create run dir");
        fs::write(
            store.path(RuntimeArtifactKind::RuntimeStageTrace, MarketId::Hk),
            "\n\n",
        )
        .expect("write blank artifact");

        let latest: Option<RuntimeArtifactEnvelope<Row>> = store
            .read_latest_json_line(RuntimeArtifactKind::RuntimeStageTrace, MarketId::Hk)
            .expect("read blank artifact");

        assert!(latest.is_none());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn store_can_read_legacy_raw_latest_payload() {
        let root = unique_temp_dir();
        let store = RuntimeArtifactStore::new(&root);
        fs::create_dir_all(root.join(".run")).expect("create run dir");
        fs::write(
            store.path(RuntimeArtifactKind::VisualGraphFrame, MarketId::Hk),
            r#"{"tick":7,"label":"legacy"}"#,
        )
        .expect("write legacy artifact");

        let latest: Row = store
            .read_latest_json_payload(RuntimeArtifactKind::VisualGraphFrame, MarketId::Hk)
            .expect("read legacy artifact")
            .expect("legacy row exists");

        assert_eq!(latest.tick, 7);
        assert_eq!(latest.label, "legacy");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn runtime_health_tick_records_bp_probe_and_artifact_errors() {
        let root = unique_temp_dir();
        let store = RuntimeArtifactStore::new(&root);
        let row = RuntimeHealthTick {
            ts: Utc::now(),
            market: "us".to_string(),
            tick: 42,
            stage_count: 14,
            stage_plan: "subkg_bp_probe_artifact_health".to_string(),
            stage_plan_expected_count: 16,
            stage_plan_covered: true,
            bp_iterations: 8,
            bp_converged: true,
            bp_nodes: 11,
            bp_edges: 17,
            bp_master_graph_edges: 17,
            bp_master_runtime_edges: 17,
            bp_build_inputs_ms: 3,
            bp_run_ms: 5,
            bp_message_trace_write_ms: 7,
            bp_marginals_write_ms: 11,
            bp_shadow_observed_incident_edges: 4,
            bp_shadow_low_weight_edges: 6,
            bp_shadow_retained_edges: 13,
            bp_shadow_pruned_edges: 4,
            bp_shadow_stock_to_stock_edges: 15,
            bp_shadow_unknown_edges: 2,
            observed_priors: 3,
            frontier_symbols: 2,
            frontier_nodes: 5,
            frontier_edges: 8,
            frontier_hops: 4,
            frontier_candidates: 3,
            frontier_dry_run_updates: 3,
            frontier_dry_run_mean_abs_delta: Some(0.25),
            frontier_dry_run_max_abs_delta: Some(0.5),
            frontier_pressure_cache_updates: 2,
            frontier_pressure_cache_mean_abs_delta: Some(0.2),
            frontier_pressure_cache_max_abs_delta: Some(0.4),
            frontier_pressure_gate_passed: 1,
            frontier_pressure_gate_noise_floor: Some(0.2),
            frontier_next_proposals: 1,
            frontier_loop_rounds: 2,
            frontier_loop_final_proposals: 1,
            lead_lag_events: 2,
            probe_emitted: 3,
            probe_evaluated: 1,
            probe_pending: 5,
            probe_mean_accuracy: Some(0.75),
            artifact_write_errors: vec![RuntimeArtifactWriteError {
                artifact: "visual_graph_frame".to_string(),
                error: "disk full".to_string(),
            }],
        };

        store
            .append_json_line(RuntimeArtifactKind::RuntimeHealthTick, MarketId::Us, &row)
            .expect("write health tick");
        let latest: RuntimeHealthTick = store
            .read_latest_json_payload(RuntimeArtifactKind::RuntimeHealthTick, MarketId::Us)
            .expect("read health tick")
            .expect("health tick exists");

        assert_eq!(latest.tick, 42);
        assert_eq!(latest.stage_plan_expected_count, 16);
        assert!(latest.stage_plan_covered);
        assert_eq!(latest.bp_iterations, 8);
        assert_eq!(latest.bp_master_graph_edges, 17);
        assert_eq!(latest.bp_master_runtime_edges, 17);
        assert_eq!(latest.bp_build_inputs_ms, 3);
        assert_eq!(latest.bp_run_ms, 5);
        assert_eq!(latest.bp_message_trace_write_ms, 7);
        assert_eq!(latest.bp_marginals_write_ms, 11);
        assert_eq!(latest.bp_shadow_observed_incident_edges, 4);
        assert_eq!(latest.bp_shadow_low_weight_edges, 6);
        assert_eq!(latest.bp_shadow_retained_edges, 13);
        assert_eq!(latest.bp_shadow_pruned_edges, 4);
        assert_eq!(latest.bp_shadow_stock_to_stock_edges, 15);
        assert_eq!(latest.bp_shadow_unknown_edges, 2);
        assert_eq!(latest.frontier_nodes, 5);
        assert_eq!(latest.frontier_hops, 4);
        assert_eq!(latest.frontier_candidates, 3);
        assert_eq!(latest.frontier_dry_run_updates, 3);
        assert_eq!(latest.frontier_dry_run_max_abs_delta, Some(0.5));
        assert_eq!(latest.frontier_pressure_cache_updates, 2);
        assert_eq!(latest.frontier_pressure_cache_max_abs_delta, Some(0.4));
        assert_eq!(latest.frontier_pressure_gate_passed, 1);
        assert_eq!(latest.frontier_pressure_gate_noise_floor, Some(0.2));
        assert_eq!(latest.frontier_next_proposals, 1);
        assert_eq!(latest.frontier_loop_rounds, 2);
        assert_eq!(latest.frontier_loop_final_proposals, 1);
        assert_eq!(latest.probe_pending, 5);
        assert_eq!(
            latest.artifact_write_errors[0].artifact,
            "visual_graph_frame"
        );

        fs::remove_dir_all(root).ok();
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("eden-runtime-artifacts-{nanos}"))
    }
}
