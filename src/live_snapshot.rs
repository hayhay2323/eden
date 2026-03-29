use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::ontology::reasoning::ActionNode;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum LiveMarket {
    #[serde(alias = "hk", alias = "HK")]
    Hk,
    #[serde(alias = "us", alias = "US")]
    Us,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSnapshot {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub stock_count: usize,
    pub edge_count: usize,
    pub hypothesis_count: usize,
    pub observation_count: usize,
    pub active_positions: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_position_nodes: Vec<ActionNode>,
    #[serde(deserialize_with = "deserialize_market_regime")]
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub scorecard: LiveScorecard,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tactical_cases: Vec<LiveTacticalCase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypothesis_tracks: Vec<LiveHypothesisTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_signals: Vec<LiveSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub convergence_scores: Vec<LiveSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pressures: Vec<LivePressure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backward_chains: Vec<LiveBackwardChain>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_leaders: Vec<LiveCausalLeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LiveEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_market_signals: Vec<LiveCrossMarketSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_market_anomalies: Vec<LiveCrossMarketAnomaly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structural_deltas: Vec<LiveStructuralDelta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_senses: Vec<LivePropagationSense>,
    #[serde(default, deserialize_with = "deserialize_lineage")]
    pub lineage: Vec<LiveLineageMetric>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveMarketRegime {
    pub bias: String,
    pub confidence: Decimal,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub average_return: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directional_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_market_sentiment: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveStressSnapshot {
    pub composite_stress: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector_synchrony: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub momentum_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_dispersion: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_anomaly: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveScorecard {
    pub total_signals: usize,
    pub resolved_signals: usize,
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveTacticalCase {
    pub setup_id: String,
    #[serde(default)]
    pub symbol: String,
    pub title: String,
    pub action: String,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub heuristic_edge: Decimal,
    pub entry_rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveHypothesisTrack {
    pub symbol: String,
    pub title: String,
    pub status: String,
    pub age_ticks: u64,
    pub confidence: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSignal {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub composite: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_composite: Option<Decimal>,
    #[serde(default)]
    pub capital_flow_direction: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub volume_profile: Decimal,
    #[serde(default)]
    pub pre_post_market_anomaly: Decimal,
    #[serde(default)]
    pub valuation: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_stock_correlation: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector_coherence: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_market_propagation: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveStructuralDelta {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub composite_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub capital_flow_delta: Decimal,
    pub flow_persistence: u64,
    pub flow_reversal: bool,
    pub pre_market_trend: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LivePropagationSense {
    pub source_symbol: String,
    pub target_symbol: String,
    pub channel: String,
    pub propagation_strength: Decimal,
    pub target_momentum: Decimal,
    pub lag_gap: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LivePressure {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default)]
    pub capital_flow_pressure: Decimal,
    pub momentum: Decimal,
    pub pressure_delta: Decimal,
    pub pressure_duration: u64,
    pub accelerating: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveBackwardChain {
    pub symbol: String,
    pub conclusion: String,
    pub primary_driver: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<LiveEvidence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveEvidence {
    pub source: String,
    pub description: String,
    pub weight: Decimal,
    pub direction: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCausalLeader {
    pub symbol: String,
    pub current_leader: String,
    pub leader_streak: u64,
    pub flips: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveEvent {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub magnitude: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCrossMarketSignal {
    pub us_symbol: String,
    pub hk_symbol: String,
    pub propagation_confidence: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_since_hk_close_minutes: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCrossMarketAnomaly {
    pub us_symbol: String,
    pub hk_symbol: String,
    pub expected_direction: Decimal,
    pub actual_direction: Decimal,
    pub divergence: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveLineageMetric {
    pub template: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

/// Accepts either a full `LiveMarketRegime` object (HK format)
/// or a plain string like `"neutral"` (US format).
fn deserialize_market_regime<'de, D>(deserializer: D) -> Result<LiveMarketRegime, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        Full(LiveMarketRegime),
        Short(String),
    }
    match Helper::deserialize(deserializer)? {
        Helper::Full(r) => Ok(r),
        Helper::Short(bias) => Ok(LiveMarketRegime {
            bias,
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        }),
    }
}

/// Accepts either a `Vec<LiveLineageMetric>` or `{"by_template": [...]}`.
fn deserialize_lineage<'de, D>(deserializer: D) -> Result<Vec<LiveLineageMetric>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        Vec(Vec<LiveLineageMetric>),
        Map { by_template: Vec<LiveLineageMetric> },
    }
    match Helper::deserialize(deserializer)? {
        Helper::Vec(v) => Ok(v),
        Helper::Map { by_template } => Ok(by_template),
    }
}

pub fn snapshot_path(env_var: &str, default_path: &str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
}

pub async fn ensure_snapshot_parent(path: &str) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
}

async fn write_snapshot_atomic(path: &str, payload: &str) -> std::io::Result<()> {
    let path = Path::new(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot.json");
    let temp_path = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let mut file = tokio::fs::File::create(&temp_path).await?;
    file.write_all(payload.as_bytes()).await?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    tokio::fs::rename(&temp_path, path).await
}

async fn append_jsonl_line(path: &str, line: &str) -> std::io::Result<()> {
    ensure_snapshot_parent(path).await;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    file.sync_data().await
}

fn snapshot_group_locks() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snapshot_group_latest_ticks() -> &'static Mutex<HashMap<String, u64>> {
    static LATEST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    LATEST.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snapshot_group_lock(group: &str) -> Arc<AsyncMutex<()>> {
    let mut locks = snapshot_group_locks()
        .lock()
        .expect("snapshot group lock poisoned");
    locks
        .entry(group.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub fn spawn_write_json_snapshot<T>(path: String, snapshot: T)
where
    T: Serialize + Send + 'static,
{
    tokio::spawn(async move {
        let payload = serde_json::to_string(&snapshot).unwrap_or_default();
        if let Err(error) = write_snapshot_atomic(&path, &payload).await {
            eprintln!(
                "Warning: failed to write snapshot {} atomically: {}",
                path, error
            );
        }
    });
}

pub fn spawn_write_json_snapshots_batch(
    group: String,
    tick: u64,
    snapshots: Vec<(String, String)>,
) {
    {
        let mut latest = snapshot_group_latest_ticks()
            .lock()
            .expect("snapshot latest tick lock poisoned");
        latest.insert(group.clone(), tick);
    }

    let lock = snapshot_group_lock(&group);
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        let latest_tick = snapshot_group_latest_ticks()
            .lock()
            .expect("snapshot latest tick lock poisoned")
            .get(&group)
            .copied()
            .unwrap_or_default();
        if latest_tick != tick {
            return;
        }

        for (path, payload) in snapshots {
            if let Err(error) = write_snapshot_atomic(&path, &payload).await {
                eprintln!(
                    "Warning: failed to write snapshot batch {}:{} atomically for {}: {}",
                    group, tick, path, error
                );
                return;
            }
        }
    });
}

pub fn json_payload<T>(snapshot: &T) -> String
where
    T: Serialize,
{
    serde_json::to_string(snapshot).unwrap_or_default()
}

pub fn spawn_write_snapshot(path: String, snapshot: LiveSnapshot) {
    spawn_write_json_snapshot(path, snapshot);
}

pub fn spawn_append_jsonl_line(group: String, path: String, line: String) {
    let lock = snapshot_group_lock(&format!("append:{group}"));
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        if let Err(error) = append_jsonl_line(&path, &line).await {
            eprintln!("Warning: failed to append journal line {}: {}", path, error);
        }
    });
}

pub fn spawn_mutate_text_file<F>(group: String, path: String, transform: F)
where
    F: FnOnce(String) -> String + Send + 'static,
{
    let lock = snapshot_group_lock(&format!("append:{group}"));
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let updated = transform(existing);
        if let Err(error) = write_snapshot_atomic(&path, &updated).await {
            eprintln!("Warning: failed to mutate text file {}: {}", path, error);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_snapshot() -> LiveSnapshot {
        LiveSnapshot {
            tick: 1,
            timestamp: "2026-03-22T00:00:00Z".into(),
            market: LiveMarket::Us,
            stock_count: 1,
            edge_count: 2,
            hypothesis_count: 3,
            observation_count: 4,
            active_positions: 0,
            active_position_nodes: vec![],
            market_regime: LiveMarketRegime {
                bias: "neutral".into(),
                confidence: Decimal::ZERO,
                breadth_up: Decimal::ZERO,
                breadth_down: Decimal::ZERO,
                average_return: Decimal::ZERO,
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: Decimal::ZERO,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            scorecard: LiveScorecard {
                total_signals: 0,
                resolved_signals: 0,
                hits: 0,
                misses: 0,
                hit_rate: Decimal::ZERO,
                mean_return: Decimal::ZERO,
            },
            tactical_cases: vec![],
            hypothesis_tracks: vec![],
            top_signals: vec![],
            convergence_scores: vec![],
            pressures: vec![],
            backward_chains: vec![],
            causal_leaders: vec![],
            events: vec![],
            lineage: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            structural_deltas: vec![],
            propagation_senses: vec![],
        }
    }

    #[tokio::test]
    async fn writes_snapshot_atomically() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-live-snapshot-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let path = dir.join("snapshot.json");
        let payload = serde_json::to_string(&test_snapshot()).unwrap();
        write_snapshot_atomic(path.to_str().unwrap(), &payload)
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(written, payload);

        let temp_files = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count();
        assert_eq!(temp_files, 0);

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn batch_writer_keeps_latest_tick_per_group() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-live-batch-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path_a = dir.join("a.json");
        let path_b = dir.join("b.json");
        let group = format!("test-group-{}", dir.display());

        spawn_write_json_snapshots_batch(
            group.clone(),
            1,
            vec![
                (path_a.to_string_lossy().to_string(), "{\"tick\":1}".into()),
                (path_b.to_string_lossy().to_string(), "{\"tick\":1}".into()),
            ],
        );
        spawn_write_json_snapshots_batch(
            group,
            2,
            vec![
                (path_a.to_string_lossy().to_string(), "{\"tick\":2}".into()),
                (path_b.to_string_lossy().to_string(), "{\"tick\":2}".into()),
            ],
        );

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let written_a = tokio::fs::read_to_string(&path_a).await.unwrap();
        let written_b = tokio::fs::read_to_string(&path_b).await.unwrap();
        assert_eq!(written_a, "{\"tick\":2}");
        assert_eq!(written_b, "{\"tick\":2}");

        let _ = tokio::fs::remove_file(&path_a).await;
        let _ = tokio::fs::remove_file(&path_b).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn append_jsonl_is_serialized_per_group() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-jsonl-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal.jsonl");

        spawn_append_jsonl_line(
            "test-journal".into(),
            path.to_string_lossy().to_string(),
            "{\"tick\":1}\n".into(),
        );
        spawn_append_jsonl_line(
            "test-journal".into(),
            path.to_string_lossy().to_string(),
            "{\"tick\":2}\n".into(),
        );

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let written = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(written.contains("{\"tick\":1}\n"));
        assert!(written.contains("{\"tick\":2}\n"));

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }
}
