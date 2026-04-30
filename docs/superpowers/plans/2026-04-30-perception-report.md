# L4 Perception Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface eden's L1 perception output (emergence, contrast, lead-lag, surprise, regime) to Y via a new `data/agent_perception.json` file, in parallel with existing `data/agent_recommendations.json`. Eden does not judge — Y reads the perception and decides.

**Architecture:** New `EdenPerception` data model + tail-readers for 5 NDJSON streams + new `AgentPerceptionReport` JSON output. Backwards-compatible: existing recommendations engine untouched. New code runs in parallel.

**Tech Stack:** Rust 1.90, serde, tokio, existing eden runtime. No new external deps.

**Spec:** `docs/superpowers/specs/2026-04-30-perception-report-design.md`

---

## File Structure

| File | Status | Purpose |
|---|---|---|
| `src/agent/types/perception.rs` | NEW | Type definitions: `EdenPerception`, `EmergentCluster`, `SymbolContrast`, `LeadLagEdge`, `SurpriseAlert`, `RegimePerception`, `RegimeForward`, `PerceptionFilterConfig` |
| `src/agent/types.rs` | MODIFY | Wire `mod perception` |
| `src/agent/types/snapshot.rs` | MODIFY | Add `pub perception: Option<EdenPerception>` |
| `src/live_snapshot.rs` | MODIFY | Add `tail_records<T>` helper + 5 stream readers + `read_perception_streams` orchestrator |
| `src/agent/builders/hk.rs` | MODIFY | Populate `snapshot.perception` |
| `src/agent/builders/us.rs` | MODIFY | Populate `snapshot.perception` |
| `src/agent/perception.rs` | NEW | `AgentPerceptionReport` type + `build_perception_report` |
| `src/agent/mod.rs` | MODIFY | Wire `pub mod perception` + re-export |
| `src/agent/io.rs` | MODIFY | Add `load_perception` |
| `src/agent/artifacts.rs` | MODIFY | Add `load_perception_path` |
| `src/core/market.rs` | MODIFY | Add `ArtifactKind::Perception` + tuple mappings |
| `src/core/runtime/context.rs` | MODIFY | Add `agent_perception_path: String` to `RuntimeArtifactPaths` |
| `src/core/projection.rs` | MODIFY | Add `agent_perception: AgentPerceptionReport` to `ProjectionBundle`; build it in projection |
| `src/core/runtime/projection.rs` | MODIFY | `push_artifact!` for `agent_perception_path` |

---

## Task 1: Add `EdenPerception` type definitions

**Files:**
- Create: `src/agent/types/perception.rs`
- Modify: `src/agent/types.rs:1-24`

- [ ] **Step 1: Write the test fixture for EdenPerception serde roundtrip**

Create `src/agent/types/perception.rs` with the following content:

```rust
use serde::{Deserialize, Serialize};

use crate::live_snapshot::LiveMarket;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdenPerception {
    pub schema_version: u32,
    pub market: LiveMarket,
    pub tick: u64,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emergent_clusters: Vec<EmergentCluster>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_leaders: Vec<SymbolContrast>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_chains: Vec<LeadLagEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anomaly_alerts: Vec<SurpriseAlert>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<RegimePerception>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmergentCluster {
    pub sector: String,
    pub total_members: u32,
    pub sync_member_count: u32,
    pub sync_ratio: String,
    pub sync_pct: f64,
    pub strongest_member: String,
    pub strongest_activation: f64,
    pub mean_activation_intent: f64,
    pub mean_activation_pressure: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SymbolContrast {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub center_activation: f64,
    pub sector_mean: f64,
    pub vs_sector_contrast: f64,
    pub node_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LeadLagEdge {
    pub leader: String,
    pub follower: String,
    pub lag_ticks: i32,
    pub correlation: f64,
    pub n_samples: usize,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurpriseAlert {
    pub symbol: String,
    pub channel: String,
    pub observed: f64,
    pub expected: f64,
    pub squared_error: f64,
    pub total_surprise: f64,
    pub floor: f64,
    pub deviation_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimePerception {
    pub bucket: String,
    pub historical_visits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_tick: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward_outcomes: Vec<RegimeForward>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimeForward {
    pub horizon_ticks: u32,
    pub n_samples: u32,
    pub mean_stress_delta: f64,
    pub mean_synchrony_delta: f64,
    pub mean_bull_bias_delta: f64,
}

/// Filter thresholds for surfacing perception signals. Defaults set per
/// design spec 2026-04-30-perception-report-design.md.
#[derive(Debug, Clone, Copy)]
pub struct PerceptionFilterConfig {
    pub min_cluster_sync_pct: f64,
    pub min_leader_contrast: f64,
    pub max_leaders: usize,
    pub min_chain_correlation: f64,
    pub min_chain_samples: usize,
    pub max_chains: usize,
    pub min_anomaly_surprise_ratio: f64,
    pub max_anomalies: usize,
}

impl Default for PerceptionFilterConfig {
    fn default() -> Self {
        Self {
            min_cluster_sync_pct: 0.7,
            min_leader_contrast: 3.0,
            max_leaders: 20,
            min_chain_correlation: 0.5,
            min_chain_samples: 10,
            max_chains: 30,
            min_anomaly_surprise_ratio: 1.5,
            max_anomalies: 15,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perception_default_filter_config() {
        let cfg = PerceptionFilterConfig::default();
        assert!((cfg.min_cluster_sync_pct - 0.7).abs() < 1e-9);
        assert!((cfg.min_leader_contrast - 3.0).abs() < 1e-9);
        assert_eq!(cfg.max_leaders, 20);
        assert!((cfg.min_chain_correlation - 0.5).abs() < 1e-9);
        assert_eq!(cfg.min_chain_samples, 10);
        assert_eq!(cfg.max_chains, 30);
        assert!((cfg.min_anomaly_surprise_ratio - 1.5).abs() < 1e-9);
        assert_eq!(cfg.max_anomalies, 15);
    }

    #[test]
    fn perception_serde_roundtrip_empty() {
        let original = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "2026-04-30T09:00:00Z".to_string(),
            emergent_clusters: vec![],
            sector_leaders: vec![],
            causal_chains: vec![],
            anomaly_alerts: vec![],
            regime: None,
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let recovered: EdenPerception = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(original, recovered);
    }

    #[test]
    fn perception_serde_roundtrip_full() {
        let original = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "2026-04-30T09:00:00Z".to_string(),
            emergent_clusters: vec![EmergentCluster {
                sector: "semiconductor".to_string(),
                total_members: 9,
                sync_member_count: 9,
                sync_ratio: "9/9".to_string(),
                sync_pct: 1.0,
                strongest_member: "6809.HK".to_string(),
                strongest_activation: 0.79,
                mean_activation_intent: 0.51,
                mean_activation_pressure: 0.71,
                members: vec!["6809.HK".to_string(), "981.HK".to_string()],
            }],
            sector_leaders: vec![SymbolContrast {
                symbol: "6869.HK".to_string(),
                sector: Some("semiconductor".to_string()),
                center_activation: 13.68,
                sector_mean: 5.85,
                vs_sector_contrast: 7.82,
                node_kind: "Role".to_string(),
            }],
            causal_chains: vec![LeadLagEdge {
                leader: "6883.HK".to_string(),
                follower: "2477.HK".to_string(),
                lag_ticks: 3,
                correlation: 0.89,
                n_samples: 17,
                direction: "from_leads".to_string(),
            }],
            anomaly_alerts: vec![SurpriseAlert {
                symbol: "1800.HK".to_string(),
                channel: "PressureStructure".to_string(),
                observed: 0.68,
                expected: 1.88,
                squared_error: 1.45,
                total_surprise: 1.46,
                floor: 1.22,
                deviation_kind: "below_expected".to_string(),
            }],
            regime: Some(RegimePerception {
                bucket: "stress=4|sync=4|bias=2|act=3|turn=3".to_string(),
                historical_visits: 188,
                last_seen_tick: Some(29),
                forward_outcomes: vec![RegimeForward {
                    horizon_ticks: 30,
                    n_samples: 89,
                    mean_stress_delta: -0.048,
                    mean_synchrony_delta: -0.0001,
                    mean_bull_bias_delta: 0.0,
                }],
            }),
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let recovered: EdenPerception = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(original, recovered);
    }
}
```

- [ ] **Step 2: Wire `perception` into `agent/types.rs`**

Modify `src/agent/types.rs` from:

```rust
use super::*;

#[path = "types/alert.rs"]
mod alert;
#[path = "types/conversation.rs"]
mod conversation;
#[path = "types/investigation.rs"]
mod investigation;
#[path = "types/judgment.rs"]
mod judgment;
#[path = "types/recommendation.rs"]
mod recommendation;
#[path = "types/snapshot.rs"]
mod snapshot;
#[path = "types/state.rs"]
mod state;

pub use alert::*;
pub use conversation::*;
pub use investigation::*;
pub use judgment::*;
pub use recommendation::*;
pub use snapshot::*;
pub use state::*;
```

To:

```rust
use super::*;

#[path = "types/alert.rs"]
mod alert;
#[path = "types/conversation.rs"]
mod conversation;
#[path = "types/investigation.rs"]
mod investigation;
#[path = "types/judgment.rs"]
mod judgment;
#[path = "types/perception.rs"]
mod perception;
#[path = "types/recommendation.rs"]
mod recommendation;
#[path = "types/snapshot.rs"]
mod snapshot;
#[path = "types/state.rs"]
mod state;

pub use alert::*;
pub use conversation::*;
pub use investigation::*;
pub use judgment::*;
pub use perception::*;
pub use recommendation::*;
pub use snapshot::*;
pub use state::*;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --features persistence --lib types::perception 2>&1 | tail -10`

Expected: `test result: ok. 3 passed; 0 failed` (`perception_default_filter_config`, `perception_serde_roundtrip_empty`, `perception_serde_roundtrip_full`)

- [ ] **Step 4: Run full lib test target to confirm no regression**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1415 passed; 0 failed; 0 ignored` (was 1412, +3 from this task)

- [ ] **Step 5: Commit**

```bash
git add src/agent/types/perception.rs src/agent/types.rs
git commit -m "$(cat <<'EOF'
feat(agent): add EdenPerception types

Type-only addition. Defines EdenPerception and sub-types
(EmergentCluster, SymbolContrast, LeadLagEdge, SurpriseAlert,
RegimePerception, RegimeForward, PerceptionFilterConfig) for the
upcoming perception report milestone.

Per spec 2026-04-30-perception-report-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Add `tail_records` helper

**Files:**
- Modify: `src/live_snapshot.rs` (add helper function + tests at end)

- [ ] **Step 1: Write failing tests for tail_records**

Append to `src/live_snapshot.rs` (inside the existing `#[cfg(test)] mod tests { ... }` block, OR create a new test module at the bottom of the file if no test module exists for this purpose):

```rust
#[cfg(test)]
mod perception_reader_tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[derive(Debug, Deserialize, PartialEq)]
    struct ToyRecord {
        id: u32,
        value: String,
    }

    fn make_ndjson(records: &[(u32, &str)]) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp file");
        for (id, value) in records {
            writeln!(f, r#"{{"id":{id},"value":"{value}"}}"#).expect("write");
        }
        f
    }

    #[test]
    fn tail_records_returns_empty_when_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/path/zzz.ndjson");
        let out: Vec<ToyRecord> = tail_records(&path, 1024, 10);
        assert!(out.is_empty());
    }

    #[test]
    fn tail_records_reads_all_when_file_smaller_than_buffer() {
        let f = make_ndjson(&[(1, "a"), (2, "b"), (3, "c")]);
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 10);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].id, 1);
        assert_eq!(out[2].value, "c");
    }

    #[test]
    fn tail_records_caps_at_max_records() {
        let f = make_ndjson(&[(1, "a"), (2, "b"), (3, "c"), (4, "d"), (5, "e")]);
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 2);
        // Most recent 2: ids 4 and 5
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, 4);
        assert_eq!(out[1].id, 5);
    }

    #[test]
    fn tail_records_skips_partial_first_line_in_buffer() {
        // Build a file where the buffer window cuts mid-line.
        let mut f = NamedTempFile::new().expect("temp file");
        // First record is long; next two are short. With small buffer we'll
        // read mid-way through the first record and must drop it.
        let long_value = "x".repeat(500);
        writeln!(f, r#"{{"id":1,"value":"{}"}}"#, long_value).expect("write");
        writeln!(f, r#"{{"id":2,"value":"b"}}"#).expect("write");
        writeln!(f, r#"{{"id":3,"value":"c"}}"#).expect("write");
        let out: Vec<ToyRecord> = tail_records(f.path(), 64, 10);
        // Must NOT include id=1 (partial). Must include id=2 and id=3.
        assert!(out.iter().all(|r| r.id != 1));
        assert!(out.iter().any(|r| r.id == 2));
        assert!(out.iter().any(|r| r.id == 3));
    }

    #[test]
    fn tail_records_skips_unparseable_lines() {
        let mut f = NamedTempFile::new().expect("temp file");
        writeln!(f, r#"{{"id":1,"value":"a"}}"#).expect("write");
        writeln!(f, "not valid json").expect("write");
        writeln!(f, r#"{{"id":3,"value":"c"}}"#).expect("write");
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 10);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, 1);
        assert_eq!(out[1].id, 3);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (function doesn't exist)**

Run: `cargo test --features persistence --lib perception_reader_tests 2>&1 | tail -10`

Expected: `error[E0425]: cannot find function 'tail_records'` — compile failure.

- [ ] **Step 3: Add `tempfile` dev-dependency if missing**

Check `Cargo.toml` for `[dev-dependencies]` `tempfile`:

Run: `grep -A 10 '\[dev-dependencies\]' Cargo.toml | head -10`

If `tempfile` not listed, add it:

```bash
cargo add --dev tempfile
```

Expected: `Adding tempfile vX.Y.Z to dev-dependencies.`

- [ ] **Step 4: Implement `tail_records` in `src/live_snapshot.rs`**

Insert this function near the end of `src/live_snapshot.rs`, before the test modules:

```rust
/// Read the most recent NDJSON records from the tail of a file.
///
/// Reads at most `buffer_bytes` from the end of the file, drops any
/// partial first or last line, parses remaining lines as `T` (skipping
/// JSON parse errors), and returns the most recent `max_records`
/// successfully parsed records (preserving file order, oldest first).
///
/// Returns empty `Vec` if file does not exist. This is intentional:
/// fresh runtime has no perception streams yet — caller should treat
/// missing files as "no perception data" rather than as errors.
pub fn tail_records<T>(path: &std::path::Path, buffer_bytes: u64, max_records: usize) -> Vec<T>
where
    T: serde::de::DeserializeOwned,
{
    use std::io::{Read, Seek, SeekFrom};

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let file_len = metadata.len();
    let read_len = buffer_bytes.min(file_len);
    let seek_from = file_len.saturating_sub(read_len);

    if let Err(_) = file.seek(SeekFrom::Start(seek_from)) {
        return Vec::new();
    }

    let mut buf = Vec::with_capacity(read_len as usize);
    if file.take(read_len).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }

    let text = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // If we did not start at offset 0, the first line is potentially partial.
    let start = if seek_from > 0 { 1 } else { 0 };

    // The trailing newline split produces a final empty element when the
    // file ends with '\n'. If the final element is non-empty, it may be a
    // partial last line (e.g. mid-write); drop it conservatively.
    let end = if lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        lines.len() - 1
    } else if !lines.is_empty() {
        lines.len() - 1
    } else {
        0
    };

    if start >= end {
        return Vec::new();
    }

    let parsed: Vec<T> = lines[start..end]
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<T>(trimmed).ok()
        })
        .collect();

    if parsed.len() <= max_records {
        parsed
    } else {
        parsed.into_iter().rev().take(max_records).rev().collect()
    }
}
```

Note: the function takes `&std::path::Path` but the test uses `f.path()` which returns `&Path`. Compatible.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --features persistence --lib perception_reader_tests 2>&1 | tail -15`

Expected: `test result: ok. 5 passed; 0 failed` (5 test cases)

- [ ] **Step 6: Run full lib test target**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1420 passed; 0 failed` (1415 + 5)

- [ ] **Step 7: Commit**

```bash
git add src/live_snapshot.rs Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add tail_records<T> helper

Generic NDJSON tail reader. Reads at most buffer_bytes from the end
of a file, drops partial first/last lines, parses each line as T
(skipping JSON parse errors), returns the most recent max_records.
Returns empty Vec if file missing — caller treats as "no data" not
"error". Adds tempfile dev-dependency for fixture-based tests.

Per spec 2026-04-30-perception-report-design.md task 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Implement `read_emergent_clusters`

**Files:**
- Modify: `src/live_snapshot.rs` (add reader function + tests)

- [ ] **Step 1: Write the failing test**

Append to the `perception_reader_tests` module in `src/live_snapshot.rs`:

```rust
    #[test]
    fn read_emergent_clusters_filters_by_sync_pct() {
        let mut f = NamedTempFile::new().expect("temp file");
        // 9/9 sync (100%) — included
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"semiconductor","cluster_total_members":9,"sync_member_count":9,"sync_members":["981.HK"],"lit_node_kinds":["Pressure","Intent"],"mean_activation_per_kind":{{"Intent":0.5,"Pressure":0.7}},"strongest_member":"6809.HK","strongest_member_mean_activation":0.79}}"#).expect("write");
        // 3/10 sync (30%) — filtered out (below 70%)
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"toys","cluster_total_members":10,"sync_member_count":3,"sync_members":["x"],"lit_node_kinds":["Intent"],"mean_activation_per_kind":{{"Intent":0.3}},"strongest_member":"x","strongest_member_mean_activation":0.3}}"#).expect("write");
        // 8/10 sync (80%) — included
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"tech","cluster_total_members":10,"sync_member_count":8,"sync_members":["a","b"],"lit_node_kinds":["Pressure"],"mean_activation_per_kind":{{"Pressure":0.6}},"strongest_member":"a","strongest_member_mean_activation":0.6}}"#).expect("write");

        let cfg = crate::agent::PerceptionFilterConfig::default();
        let out = read_emergent_clusters(f.path(), &cfg);
        assert_eq!(out.len(), 2, "expected only sync >= 70%");
        assert!(out.iter().any(|c| c.sector == "semiconductor"));
        assert!(out.iter().any(|c| c.sector == "tech"));
        assert!(!out.iter().any(|c| c.sector == "toys"));
    }
```

- [ ] **Step 2: Run test to confirm failure**

Run: `cargo test --features persistence --lib read_emergent_clusters_filters 2>&1 | tail -5`

Expected: `error[E0425]: cannot find function 'read_emergent_clusters'`

- [ ] **Step 3: Implement `read_emergent_clusters`**

Add near `tail_records` in `src/live_snapshot.rs`:

```rust
/// Internal NDJSON record shape for `eden-emergence-{market}.ndjson`.
/// Mirrors `pipeline::sub_kg_emergence::EmergenceClusterEvent` minus
/// internal fields. We use a private deserialise-only struct rather
/// than depending on the producer struct so the reader is decoupled
/// from upstream schema changes.
#[derive(Debug, serde::Deserialize)]
struct RawEmergenceRecord {
    cluster_key: String,
    cluster_total_members: u32,
    sync_member_count: u32,
    #[serde(default)]
    sync_members: Vec<String>,
    #[serde(default)]
    mean_activation_per_kind: std::collections::HashMap<String, f64>,
    strongest_member: String,
    strongest_member_mean_activation: f64,
}

const PERCEPTION_TAIL_BYTES: u64 = 256 * 1024;
const EMERGENCE_MAX_RECORDS: usize = 30;

/// Read recent emergence cluster records from the NDJSON stream and
/// surface those passing the filter as `EmergentCluster`s.
pub fn read_emergent_clusters(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::EmergentCluster> {
    let raw: Vec<RawEmergenceRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, EMERGENCE_MAX_RECORDS);
    raw.into_iter()
        .filter_map(|rec| {
            let total = rec.cluster_total_members.max(1);
            let sync_pct = rec.sync_member_count as f64 / total as f64;
            if sync_pct < cfg.min_cluster_sync_pct {
                return None;
            }
            Some(crate::agent::EmergentCluster {
                sector: rec.cluster_key,
                total_members: rec.cluster_total_members,
                sync_member_count: rec.sync_member_count,
                sync_ratio: format!("{}/{}", rec.sync_member_count, rec.cluster_total_members),
                sync_pct,
                strongest_member: rec.strongest_member,
                strongest_activation: rec.strongest_member_mean_activation,
                mean_activation_intent: rec.mean_activation_per_kind.get("Intent").copied().unwrap_or(0.0),
                mean_activation_pressure: rec.mean_activation_per_kind.get("Pressure").copied().unwrap_or(0.0),
                members: rec.sync_members,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Run test to verify pass**

Run: `cargo test --features persistence --lib read_emergent_clusters_filters 2>&1 | tail -5`

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 5: Run full lib test target**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1421 passed; 0 failed`

- [ ] **Step 6: Commit**

```bash
git add src/live_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add read_emergent_clusters

Tail-reads .run/eden-emergence-{market}.ndjson, filters records to
sync_pct >= cfg.min_cluster_sync_pct (default 70%), maps to
EmergentCluster with display-friendly sync_ratio + per-kind activation.

Per spec 2026-04-30-perception-report-design.md task 3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Implement `read_sector_leaders`

**Files:**
- Modify: `src/live_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Append to `perception_reader_tests`:

```rust
    #[test]
    fn read_sector_leaders_filters_and_caps() {
        let mut f = NamedTempFile::new().expect("temp file");
        for (sym, contrast) in &[("a", 7.5), ("b", 4.0), ("c", 2.0), ("d", 6.0), ("e", 1.0)] {
            writeln!(
                f,
                r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"{}.HK","node_kind":"Role","center_activation":10.0,"surround_mean":1.0,"surround_count":20,"contrast":9.0,"sector_id":"semiconductor","sector_mean_activation":3.0,"vs_sector_contrast":{}}}"#,
                sym, contrast
            ).expect("write");
        }
        let mut cfg = crate::agent::PerceptionFilterConfig::default();
        cfg.max_leaders = 3;
        let out = read_sector_leaders(f.path(), &cfg);
        // Only a (7.5), d (6.0), b (4.0) pass min_leader_contrast (3.0); c & e filtered
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].symbol, "a.HK");
        assert!((out[0].vs_sector_contrast - 7.5).abs() < 1e-9);
        assert_eq!(out[1].symbol, "d.HK");
        assert_eq!(out[2].symbol, "b.HK");
    }
```

- [ ] **Step 2: Run test to confirm failure**

Run: `cargo test --features persistence --lib read_sector_leaders_filters_and_caps 2>&1 | tail -5`

Expected: `error[E0425]: cannot find function 'read_sector_leaders'`

- [ ] **Step 3: Implement `read_sector_leaders`**

Add to `src/live_snapshot.rs`:

```rust
#[derive(Debug, serde::Deserialize)]
struct RawContrastRecord {
    symbol: String,
    #[serde(default)]
    sector_id: Option<String>,
    center_activation: f64,
    sector_mean_activation: f64,
    vs_sector_contrast: f64,
    node_kind: String,
}

const CONTRAST_MAX_RECORDS: usize = 100;

pub fn read_sector_leaders(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::SymbolContrast> {
    let raw: Vec<RawContrastRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, CONTRAST_MAX_RECORDS);
    let mut filtered: Vec<crate::agent::SymbolContrast> = raw
        .into_iter()
        .filter(|rec| rec.vs_sector_contrast >= cfg.min_leader_contrast)
        .map(|rec| crate::agent::SymbolContrast {
            symbol: rec.symbol,
            sector: rec.sector_id,
            center_activation: rec.center_activation,
            sector_mean: rec.sector_mean_activation,
            vs_sector_contrast: rec.vs_sector_contrast,
            node_kind: rec.node_kind,
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.vs_sector_contrast
            .partial_cmp(&a.vs_sector_contrast)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered.truncate(cfg.max_leaders);
    filtered
}
```

- [ ] **Step 4: Run test to verify pass**

Run: `cargo test --features persistence --lib read_sector_leaders_filters_and_caps 2>&1 | tail -5`

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/live_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add read_sector_leaders

Tail-reads .run/eden-contrast-{market}.ndjson, filters to
vs_sector_contrast >= cfg.min_leader_contrast (default 3.0),
sorts descending by contrast, caps at cfg.max_leaders (default 20).

Per spec 2026-04-30-perception-report-design.md task 4.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Implement `read_causal_chains`

**Files:**
- Modify: `src/live_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Append to `perception_reader_tests`:

```rust
    #[test]
    fn read_causal_chains_filters_and_caps() {
        let mut f = NamedTempFile::new().expect("temp file");
        // pass: corr 0.89, n=17
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","from_symbol":"6883.HK","to_symbol":"2477.HK","edge_weight":1.0,"dominant_lag":3,"correlation_at_lag":0.89,"n_samples":17,"direction":"from_leads"}}"#).expect("write");
        // filtered: corr 0.3 < 0.5
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","from_symbol":"a.HK","to_symbol":"b.HK","edge_weight":1.0,"dominant_lag":1,"correlation_at_lag":0.3,"n_samples":15,"direction":"from_leads"}}"#).expect("write");
        // filtered: n=5 < 10
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","from_symbol":"c.HK","to_symbol":"d.HK","edge_weight":1.0,"dominant_lag":2,"correlation_at_lag":0.7,"n_samples":5,"direction":"from_leads"}}"#).expect("write");
        // pass: corr -0.6 (abs >= 0.5), n=12
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","from_symbol":"e.HK","to_symbol":"f.HK","edge_weight":1.0,"dominant_lag":-2,"correlation_at_lag":-0.6,"n_samples":12,"direction":"to_leads"}}"#).expect("write");

        let cfg = crate::agent::PerceptionFilterConfig::default();
        let out = read_causal_chains(f.path(), &cfg);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].leader, "6883.HK", "highest abs correlation first");
        assert!((out[0].correlation - 0.89).abs() < 1e-9);
        assert!(out.iter().any(|c| c.leader == "e.HK"));
    }

    #[test]
    fn read_causal_chains_returns_empty_when_file_missing() {
        let cfg = crate::agent::PerceptionFilterConfig::default();
        let path = std::path::PathBuf::from("/nonexistent/eden-lead-lag-hk.ndjson");
        let out = read_causal_chains(&path, &cfg);
        assert!(out.is_empty());
    }
```

- [ ] **Step 2: Run tests to confirm failure**

Run: `cargo test --features persistence --lib read_causal_chains 2>&1 | tail -5`

Expected: `error[E0425]: cannot find function 'read_causal_chains'`

- [ ] **Step 3: Implement `read_causal_chains`**

Add to `src/live_snapshot.rs`:

```rust
#[derive(Debug, serde::Deserialize)]
struct RawLeadLagRecord {
    from_symbol: String,
    to_symbol: String,
    dominant_lag: i32,
    correlation_at_lag: f64,
    n_samples: usize,
    direction: String,
}

const LEAD_LAG_MAX_RECORDS: usize = 200;

pub fn read_causal_chains(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::LeadLagEdge> {
    let raw: Vec<RawLeadLagRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, LEAD_LAG_MAX_RECORDS);
    let mut filtered: Vec<crate::agent::LeadLagEdge> = raw
        .into_iter()
        .filter(|rec| {
            rec.correlation_at_lag.abs() >= cfg.min_chain_correlation
                && rec.n_samples >= cfg.min_chain_samples
        })
        .map(|rec| crate::agent::LeadLagEdge {
            leader: rec.from_symbol,
            follower: rec.to_symbol,
            lag_ticks: rec.dominant_lag,
            correlation: rec.correlation_at_lag,
            n_samples: rec.n_samples,
            direction: rec.direction,
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.correlation
            .abs()
            .partial_cmp(&a.correlation.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered.truncate(cfg.max_chains);
    filtered
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --features persistence --lib read_causal_chains 2>&1 | tail -5`

Expected: `test result: ok. 2 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/live_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add read_causal_chains

Tail-reads .run/eden-lead-lag-{market}.ndjson, filters to
abs(correlation_at_lag) >= cfg.min_chain_correlation (default 0.5)
AND n_samples >= cfg.min_chain_samples (default 10), sorts by
abs(correlation) descending, caps at cfg.max_chains (default 30).
Returns empty Vec if file missing (handles current lead-lag stale).

Per spec 2026-04-30-perception-report-design.md task 5.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Implement `read_anomaly_alerts`

**Files:**
- Modify: `src/live_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Append to `perception_reader_tests`:

```rust
    #[test]
    fn read_anomaly_alerts_filters_by_surprise_ratio() {
        let mut f = NamedTempFile::new().expect("temp file");
        // pass: total_surprise 1.46, floor 1.22 → ratio 1.20 < 1.5 → FILTERED
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"borderline.HK","total_surprise":1.46,"floor":1.22,"max_node":"PressureStructure","max_observed":0.68,"max_expected":1.88,"max_squared_error":1.45}}"#).expect("write");
        // pass: ratio 2.0
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"strong.HK","total_surprise":3.0,"floor":1.5,"max_node":"PressureMomentum","max_observed":-2.0,"max_expected":1.0,"max_squared_error":9.0}}"#).expect("write");
        // pass: ratio 1.5 exact
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"edge.HK","total_surprise":3.0,"floor":2.0,"max_node":"IntentDistribution","max_observed":0.5,"max_expected":0.2,"max_squared_error":0.09}}"#).expect("write");

        let cfg = crate::agent::PerceptionFilterConfig::default();
        let out = read_anomaly_alerts(f.path(), &cfg);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|a| a.symbol == "strong.HK"));
        assert!(out.iter().any(|a| a.symbol == "edge.HK"));
        let strong = out.iter().find(|a| a.symbol == "strong.HK").unwrap();
        assert_eq!(strong.deviation_kind, "below_expected");
        let edge = out.iter().find(|a| a.symbol == "edge.HK").unwrap();
        assert_eq!(edge.deviation_kind, "above_expected");
    }
```

- [ ] **Step 2: Run test to confirm failure**

Run: `cargo test --features persistence --lib read_anomaly_alerts 2>&1 | tail -5`

Expected: `error[E0425]`

- [ ] **Step 3: Implement `read_anomaly_alerts`**

Add to `src/live_snapshot.rs`:

```rust
#[derive(Debug, serde::Deserialize)]
struct RawSurpriseRecord {
    symbol: String,
    total_surprise: f64,
    floor: f64,
    max_node: String,
    max_observed: f64,
    max_expected: f64,
    max_squared_error: f64,
}

const SURPRISE_MAX_RECORDS: usize = 100;

pub fn read_anomaly_alerts(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::SurpriseAlert> {
    let raw: Vec<RawSurpriseRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, SURPRISE_MAX_RECORDS);
    let mut filtered: Vec<crate::agent::SurpriseAlert> = raw
        .into_iter()
        .filter(|rec| {
            // Avoid divide-by-zero: floor < 1e-12 means no meaningful expectation;
            // pass through total_surprise > 0 to surface raw anomalies.
            if rec.floor < 1e-12 {
                rec.total_surprise > 0.0
            } else {
                rec.total_surprise / rec.floor >= cfg.min_anomaly_surprise_ratio
            }
        })
        .map(|rec| {
            let deviation_kind = if rec.max_observed < rec.max_expected {
                "below_expected".to_string()
            } else {
                "above_expected".to_string()
            };
            crate::agent::SurpriseAlert {
                symbol: rec.symbol,
                channel: rec.max_node,
                observed: rec.max_observed,
                expected: rec.max_expected,
                squared_error: rec.max_squared_error,
                total_surprise: rec.total_surprise,
                floor: rec.floor,
                deviation_kind,
            }
        })
        .collect();
    filtered.sort_by(|a, b| {
        let a_ratio = if a.floor < 1e-12 { a.total_surprise } else { a.total_surprise / a.floor };
        let b_ratio = if b.floor < 1e-12 { b.total_surprise } else { b.total_surprise / b.floor };
        b_ratio.partial_cmp(&a_ratio).unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered.truncate(cfg.max_anomalies);
    filtered
}
```

- [ ] **Step 4: Run test to verify pass**

Run: `cargo test --features persistence --lib read_anomaly_alerts 2>&1 | tail -5`

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/live_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add read_anomaly_alerts

Tail-reads .run/eden-surprise-{market}.ndjson, filters by
total_surprise / floor >= cfg.min_anomaly_surprise_ratio (default 1.5),
labels deviation_kind from observed-vs-expected sign, caps at
cfg.max_anomalies (default 15).

Per spec 2026-04-30-perception-report-design.md task 6.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Implement `read_regime_perception`

**Files:**
- Modify: `src/live_snapshot.rs`

- [ ] **Step 1: Write the failing test**

Append to `perception_reader_tests`:

```rust
    #[test]
    fn read_regime_perception_returns_latest_record() {
        let mut f = NamedTempFile::new().expect("temp file");
        writeln!(f, r#"{{"ts":"2026-04-30T08:00:00Z","market":"hk","current_tick":1,"current_bucket":"old","historical_visits":0,"last_seen_ts":null,"last_seen_tick":null,"outcomes":{{}}}}"#).expect("write");
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","current_tick":29,"current_bucket":"stress=4|sync=4","historical_visits":188,"last_seen_ts":"2026-04-30T08:55:00Z","last_seen_tick":28,"outcomes":{{"5":{{"n":169,"mean_stress_delta":-0.003,"mean_synchrony_delta":-0.0006,"mean_bull_bias_delta":0.0}},"30":{{"n":89,"mean_stress_delta":-0.048,"mean_synchrony_delta":-0.0001,"mean_bull_bias_delta":0.0}}}}}}"#).expect("write");

        let regime = read_regime_perception(f.path()).expect("regime present");
        assert_eq!(regime.bucket, "stress=4|sync=4");
        assert_eq!(regime.historical_visits, 188);
        assert_eq!(regime.last_seen_tick, Some(28));
        assert_eq!(regime.forward_outcomes.len(), 2);
        let h5 = regime.forward_outcomes.iter().find(|f| f.horizon_ticks == 5).unwrap();
        assert_eq!(h5.n_samples, 169);
        assert!((h5.mean_stress_delta + 0.003).abs() < 1e-6);
    }

    #[test]
    fn read_regime_perception_returns_none_when_missing() {
        let path = std::path::PathBuf::from("/nonexistent/regime.ndjson");
        assert!(read_regime_perception(&path).is_none());
    }
```

- [ ] **Step 2: Run tests to confirm failure**

Run: `cargo test --features persistence --lib read_regime_perception 2>&1 | tail -5`

Expected: `error[E0425]`

- [ ] **Step 3: Implement `read_regime_perception`**

Add to `src/live_snapshot.rs`:

```rust
#[derive(Debug, serde::Deserialize)]
struct RawRegimeRecord {
    current_tick: u64,
    current_bucket: String,
    historical_visits: u32,
    #[serde(default)]
    last_seen_tick: Option<u64>,
    #[serde(default)]
    outcomes: std::collections::HashMap<String, RawRegimeOutcome>,
}

#[derive(Debug, serde::Deserialize)]
struct RawRegimeOutcome {
    n: u32,
    mean_stress_delta: f64,
    mean_synchrony_delta: f64,
    mean_bull_bias_delta: f64,
}

pub fn read_regime_perception(path: &std::path::Path) -> Option<crate::agent::RegimePerception> {
    let raw: Vec<RawRegimeRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, 5);
    let latest = raw.into_iter().last()?;
    let mut forward: Vec<crate::agent::RegimeForward> = latest
        .outcomes
        .into_iter()
        .filter_map(|(horizon, outcome)| {
            let h: u32 = horizon.parse().ok()?;
            Some(crate::agent::RegimeForward {
                horizon_ticks: h,
                n_samples: outcome.n,
                mean_stress_delta: outcome.mean_stress_delta,
                mean_synchrony_delta: outcome.mean_synchrony_delta,
                mean_bull_bias_delta: outcome.mean_bull_bias_delta,
            })
        })
        .collect();
    forward.sort_by_key(|f| f.horizon_ticks);
    Some(crate::agent::RegimePerception {
        bucket: latest.current_bucket,
        historical_visits: latest.historical_visits,
        last_seen_tick: latest.last_seen_tick,
        forward_outcomes: forward,
    })
}
```

Note: `tail_records` uses `outcomes` keys as strings; the rust HashMap from serde will deserialise. We re-parse each key into u32 horizon and discard non-integer keys.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --features persistence --lib read_regime_perception 2>&1 | tail -5`

Expected: `test result: ok. 2 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/live_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot): add read_regime_perception

Tail-reads .run/eden-regime-analog-{market}.ndjson and returns the
latest record's bucket + historical_visits + horizon-keyed outcomes.
Returns None if file missing or empty.

Per spec 2026-04-30-perception-report-design.md task 7.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Combine into `read_perception_streams` + add `perception` field to `AgentSnapshot`

**Files:**
- Modify: `src/live_snapshot.rs`
- Modify: `src/agent/types/snapshot.rs`

- [ ] **Step 1: Write the failing test for orchestrator**

Append to `perception_reader_tests`:

```rust
    #[test]
    fn read_perception_streams_assembles_full_perception() {
        let dir = tempfile::tempdir().expect("dir");
        let market = "hk";
        // Write minimal records to each stream
        std::fs::write(
            dir.path().join(format!("eden-emergence-{market}.ndjson")),
            r#"{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"semiconductor","cluster_total_members":9,"sync_member_count":9,"sync_members":["981.HK"],"lit_node_kinds":["Pressure","Intent"],"mean_activation_per_kind":{"Intent":0.5,"Pressure":0.7},"strongest_member":"6809.HK","strongest_member_mean_activation":0.79}
"#,
        ).expect("write emergence");
        std::fs::write(
            dir.path().join(format!("eden-contrast-{market}.ndjson")),
            r#"{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"6869.HK","node_kind":"Role","center_activation":13.68,"surround_mean":1.45,"surround_count":38,"contrast":12.21,"sector_id":"semiconductor","sector_mean_activation":5.85,"vs_sector_contrast":7.82}
"#,
        ).expect("write contrast");
        // Surprise floor=0 => surface anything > 0; total_surprise=1.4 (above zero)
        std::fs::write(
            dir.path().join(format!("eden-surprise-{market}.ndjson")),
            r#"{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"strong.HK","total_surprise":3.0,"floor":1.5,"max_node":"PressureStructure","max_observed":0.5,"max_expected":2.0,"max_squared_error":2.25}
"#,
        ).expect("write surprise");
        // Regime
        std::fs::write(
            dir.path().join(format!("eden-regime-analog-{market}.ndjson")),
            r#"{"ts":"2026-04-30T09:00:00Z","market":"hk","current_tick":29,"current_bucket":"stress=4","historical_visits":188,"last_seen_ts":null,"last_seen_tick":null,"outcomes":{}}
"#,
        ).expect("write regime");
        // Lead-lag intentionally absent (mirrors current stale state)

        let cfg = crate::agent::PerceptionFilterConfig::default();
        let out = read_perception_streams(
            dir.path(),
            market,
            42,
            "2026-04-30T09:00:00Z",
            LiveMarket::Hk,
            &cfg,
        );
        assert_eq!(out.tick, 42);
        assert_eq!(out.market, LiveMarket::Hk);
        assert_eq!(out.emergent_clusters.len(), 1);
        assert_eq!(out.sector_leaders.len(), 1);
        assert_eq!(out.causal_chains.len(), 0, "lead-lag absent");
        assert_eq!(out.anomaly_alerts.len(), 1);
        assert!(out.regime.is_some());
        assert_eq!(out.schema_version, 1);
    }
```

- [ ] **Step 2: Run test to confirm failure**

Run: `cargo test --features persistence --lib read_perception_streams 2>&1 | tail -5`

Expected: `error[E0425]`

- [ ] **Step 3: Implement `read_perception_streams`**

Add to `src/live_snapshot.rs`:

```rust
/// Orchestrator: tail-reads all 5 perception NDJSON streams from
/// `dir` (typically ".run") and assembles an `EdenPerception` for the
/// given tick / timestamp / market.
pub fn read_perception_streams(
    dir: &std::path::Path,
    market: &str,
    tick: u64,
    timestamp: &str,
    live_market: LiveMarket,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> crate::agent::EdenPerception {
    let emergence_path = dir.join(format!("eden-emergence-{market}.ndjson"));
    let contrast_path = dir.join(format!("eden-contrast-{market}.ndjson"));
    let lead_lag_path = dir.join(format!("eden-lead-lag-{market}.ndjson"));
    let surprise_path = dir.join(format!("eden-surprise-{market}.ndjson"));
    let regime_path = dir.join(format!("eden-regime-analog-{market}.ndjson"));

    crate::agent::EdenPerception {
        schema_version: 1,
        market: live_market,
        tick,
        timestamp: timestamp.to_string(),
        emergent_clusters: read_emergent_clusters(&emergence_path, cfg),
        sector_leaders: read_sector_leaders(&contrast_path, cfg),
        causal_chains: read_causal_chains(&lead_lag_path, cfg),
        anomaly_alerts: read_anomaly_alerts(&surprise_path, cfg),
        regime: read_regime_perception(&regime_path),
    }
}
```

- [ ] **Step 4: Add `perception` field to `AgentSnapshot`**

Modify `src/agent/types/snapshot.rs:4-43` (the `AgentSnapshot` struct). Add the `perception` field anywhere within the struct (suggest after `backward_reasoning` for grouping with other Option-typed fields):

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perception: Option<EdenPerception>,
```

The full struct after edit (showing only the change context):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub wake: AgentWakeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_state: Option<WorldStateSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backward_reasoning: Option<BackwardReasoningSnapshot>,
    /// L4 perception report — eden's currently-perceived market state
    /// (emergence, contrast, lead-lag, surprise, regime). When present,
    /// Y consumes this in preference to the heuristic recommendation.
    /// Optional for backwards compat with older serialised snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perception: Option<EdenPerception>,
    // ... rest unchanged ...
```

- [ ] **Step 5: Run test to verify orchestrator passes**

Run: `cargo test --features persistence --lib read_perception_streams 2>&1 | tail -5`

Expected: `test result: ok. 1 passed; 0 failed`

- [ ] **Step 6: Run full lib test target**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1425 passed; 0 failed`

- [ ] **Step 7: Commit**

```bash
git add src/live_snapshot.rs src/agent/types/snapshot.rs
git commit -m "$(cat <<'EOF'
feat(live_snapshot,agent): orchestrate perception read + add to AgentSnapshot

read_perception_streams() composes the 5 stream readers into a single
EdenPerception for a given (market, tick, timestamp). AgentSnapshot
gains an Option<EdenPerception> field that future builders will
populate.

Per spec 2026-04-30-perception-report-design.md task 8.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Wire perception into HK + US snapshot builders

**Files:**
- Modify: `src/agent/builders/hk.rs`
- Modify: `src/agent/builders/us.rs`

- [ ] **Step 1: Find and modify HK builder return value**

In `src/agent/builders/hk.rs`, find the final `AgentSnapshot { ... }` construction (search for `AgentSnapshot {`). Add the `perception` field to it. Use `.run` as the directory and pull market="hk" + tick + timestamp from existing locals.

The build function signature has `live: &LiveSnapshot, history: &TickHistory, ...`. Use `live.timestamp.clone()` as the timestamp and `latest.tick_number` as the tick.

Find where the function returns its `AgentSnapshot { ... }` literal and add this line inside the struct expression:

```rust
        perception: Some(crate::live_snapshot::read_perception_streams(
            std::path::Path::new(".run"),
            "hk",
            latest.tick_number,
            &live.timestamp,
            live.market,
            &crate::agent::PerceptionFilterConfig::default(),
        )),
```

If `latest` or `live.timestamp` are not available where you need them, ensure you pull them from the local scope (likely both are bound earlier in the function from `history.latest()` and `live` parameter).

- [ ] **Step 2: Build to verify HK builder compiles**

Run: `cargo build --features persistence --bin eden 2>&1 | tail -10`

Expected: build succeeds.

- [ ] **Step 3: Modify US builder symmetrically**

In `src/agent/builders/us.rs`, find the `AgentSnapshot { ... }` literal and add:

```rust
        perception: Some(crate::live_snapshot::read_perception_streams(
            std::path::Path::new(".run"),
            "us",
            // tick variable name in US builder
            <tick_local>,
            &live.timestamp,
            live.market,
            &crate::agent::PerceptionFilterConfig::default(),
        )),
```

Replace `<tick_local>` with the actual tick variable name in the US builder (likely `latest.tick_number` or similar — check the file when implementing).

- [ ] **Step 4: Build to verify US builder compiles**

Run: `cargo build --features persistence --bin eden --bin eden-api 2>&1 | tail -5`

Expected: build succeeds.

- [ ] **Step 5: Run full lib test target**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1425 passed; 0 failed` (no new tests in this task; existing tests still pass).

- [ ] **Step 6: Commit**

```bash
git add src/agent/builders/hk.rs src/agent/builders/us.rs
git commit -m "$(cat <<'EOF'
feat(agent): populate AgentSnapshot.perception in HK + US builders

Both market builders now read the 5 perception NDJSON streams from
.run/ and attach an EdenPerception to the snapshot. Default
PerceptionFilterConfig is used for MVP; env-var / config overrides
are future work.

Per spec 2026-04-30-perception-report-design.md task 9.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Add `Perception` ArtifactKind + path infrastructure

**Files:**
- Modify: `src/core/market.rs`
- Modify: `src/core/runtime/context.rs`

- [ ] **Step 1: Add `Perception` to `ArtifactKind` enum**

In `src/core/market.rs`, find:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    LiveSnapshot,
    BridgeSnapshot,
    AgentSnapshot,
    OperationalSnapshot,
    Briefing,
    Session,
    Watchlist,
    Recommendations,
    RecommendationJournal,
    Scoreboard,
    EodReview,
    Analysis,
    Narration,
    RuntimeNarration,
    AnalystReview,
    AnalystScoreboard,
}
```

Add `Perception,` as the last variant before the closing brace (or anywhere — order is not significant, but put it right after `Recommendations` for readability):

```rust
    Recommendations,
    Perception,
    RecommendationJournal,
```

- [ ] **Step 2: Add tuple mappings for HK + US**

In `src/core/market.rs`, find the `(MarketId::Hk, ArtifactKind::Recommendations)` mapping (around line 290). Add the following two arms right after `(MarketId::Us, ArtifactKind::Recommendations)`:

```rust
            (MarketId::Hk, ArtifactKind::Perception) => ArtifactSpec {
                env_var: "EDEN_AGENT_PERCEPTION_PATH",
                default_path: "data/agent_perception.json",
            },
            (MarketId::Us, ArtifactKind::Perception) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_PERCEPTION_PATH",
                default_path: "data/us_agent_perception.json",
            },
```

- [ ] **Step 3: Add `agent_perception_path` to `RuntimeArtifactPaths`**

In `src/core/runtime/context.rs:162`, find the `RuntimeArtifactPaths` struct. Add this field anywhere within the struct definition (suggested right after `agent_recommendations_path`):

```rust
    pub agent_perception_path: String,
```

- [ ] **Step 4: Initialize `agent_perception_path` in `RuntimeArtifactPaths::new` (the path-resolution block)**

In `src/core/runtime/context.rs:295` area, find where `agent_recommendations_path: resolve_artifact_path(market, ArtifactKind::Recommendations),` is set inside the struct literal. Add right after it:

```rust
            agent_perception_path: resolve_artifact_path(
                market,
                ArtifactKind::Perception,
            ),
```

- [ ] **Step 5: Add `ensure_snapshot_parent` for the new path**

In `src/core/runtime/context.rs:319` area, find `ensure_snapshot_parent(&paths.agent_recommendations_path).await;` and add right after it:

```rust
        ensure_snapshot_parent(&paths.agent_perception_path).await;
```

- [ ] **Step 6: Initialize `agent_perception_path: String::new()` in test/dummy paths**

In `src/core/runtime/context.rs` around lines 2805 and 2901 (the test-only `RuntimeArtifactPaths` constructions which use `String::new()` for paths), add `agent_perception_path: String::new(),` to both literals.

- [ ] **Step 7: Build to verify**

Run: `cargo build --features persistence --bin eden --bin eden-api 2>&1 | tail -5`

Expected: build succeeds.

- [ ] **Step 8: Run full lib tests**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1425 passed; 0 failed`

- [ ] **Step 9: Commit**

```bash
git add src/core/market.rs src/core/runtime/context.rs
git commit -m "$(cat <<'EOF'
feat(core): add Perception ArtifactKind + agent_perception_path

ArtifactKind::Perception variant + (Hk, Perception) and (Us,
Perception) tuple mappings for default paths and env overrides.
RuntimeArtifactPaths gains agent_perception_path field with parent
directory ensured at startup, and test-only literal constructions
include the new field.

Per spec 2026-04-30-perception-report-design.md task 10.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Build + write `AgentPerceptionReport`

**Files:**
- Create: `src/agent/perception.rs`
- Modify: `src/agent/mod.rs`
- Modify: `src/core/projection.rs`
- Modify: `src/core/runtime/projection.rs`

- [ ] **Step 1: Create `src/agent/perception.rs`**

```rust
use serde::{Deserialize, Serialize};

use super::*;

/// Y-facing JSON output for L4 perception. Distinct from EdenPerception
/// (which lives in AgentSnapshot) so future schema evolution of the
/// internal representation can happen without breaking the on-disk
/// surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerceptionReport {
    pub schema_version: u32,
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    /// `None` when the snapshot did not include perception (e.g.
    /// pre-L4 build, no NDJSON streams produced yet). Surface as null
    /// in JSON so downstream clients can detect this state.
    pub perception: Option<EdenPerception>,
}

pub fn build_perception_report(snapshot: &AgentSnapshot) -> AgentPerceptionReport {
    AgentPerceptionReport {
        schema_version: 1,
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        perception: snapshot.perception.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_perception_report_from_snapshot_with_perception() {
        let perception = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "t".to_string(),
            emergent_clusters: vec![],
            sector_leaders: vec![],
            causal_chains: vec![],
            anomaly_alerts: vec![],
            regime: None,
        };
        let snapshot = AgentSnapshot {
            tick: 42,
            timestamp: "ts".to_string(),
            market: LiveMarket::Hk,
            market_regime: Default::default(),
            stress: Default::default(),
            wake: AgentWakeState {
                should_speak: false,
                priority: rust_decimal::Decimal::ZERO,
                headline: None,
                summary: vec![],
                focus_symbols: vec![],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: None,
            backward_reasoning: None,
            perception: Some(perception.clone()),
            notices: vec![],
            active_structures: vec![],
            recent_transitions: vec![],
            investigation_selections: vec![],
            sector_flows: vec![],
            symbols: vec![],
            perception_states: vec![],
            events: vec![],
            cross_market_signals: vec![],
            raw_sources: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![],
            macro_events: vec![],
            knowledge_links: vec![],
        };
        let report = build_perception_report(&snapshot);
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.tick, 42);
        assert_eq!(report.timestamp, "ts");
        assert!(report.perception.is_some());
    }

    #[test]
    fn build_perception_report_from_snapshot_without_perception() {
        let snapshot = AgentSnapshot {
            tick: 0,
            timestamp: "ts".to_string(),
            market: LiveMarket::Hk,
            market_regime: Default::default(),
            stress: Default::default(),
            wake: AgentWakeState {
                should_speak: false,
                priority: rust_decimal::Decimal::ZERO,
                headline: None,
                summary: vec![],
                focus_symbols: vec![],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: None,
            backward_reasoning: None,
            perception: None,
            notices: vec![],
            active_structures: vec![],
            recent_transitions: vec![],
            investigation_selections: vec![],
            sector_flows: vec![],
            symbols: vec![],
            perception_states: vec![],
            events: vec![],
            cross_market_signals: vec![],
            raw_sources: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![],
            macro_events: vec![],
            knowledge_links: vec![],
        };
        let report = build_perception_report(&snapshot);
        assert!(report.perception.is_none());
    }
}
```

Note: if `LiveMarketRegime` and `LiveStressSnapshot` do not implement `Default`, replace `Default::default()` with explicit literal constructions matching their definitions (look in `src/live_snapshot.rs`). The test only needs valid instances — empty/zero values are fine.

- [ ] **Step 2: Wire module + re-export in `src/agent/mod.rs`**

In `src/agent/mod.rs`, find the module list around line 117 (`mod types;`). Add `pub mod perception;` near the other `pub mod` declarations (e.g. after `pub mod codex;`). Then add re-exports near where other types are re-exported:

```rust
pub mod perception;
pub use perception::{build_perception_report, AgentPerceptionReport};
```

- [ ] **Step 3: Run perception unit tests**

Run: `cargo test --features persistence --lib agent::perception 2>&1 | tail -10`

Expected: `test result: ok. 2 passed; 0 failed`

- [ ] **Step 4: Add `agent_perception` to `ProjectionBundle`**

In `src/core/projection.rs:18`, find:

```rust
pub struct ProjectionBundle {
    pub live_snapshot: LiveSnapshot,
    pub agent_snapshot: AgentSnapshot,
    pub agent_briefing: AgentBriefing,
    pub agent_session: AgentSession,
    pub agent_recommendations: AgentRecommendations,
    pub agent_watchlist: AgentWatchlist,
    pub agent_scoreboard: AgentAlertScoreboard,
    pub agent_eod_review: AgentEodReview,
    pub agent_narration: AgentNarration,
}
```

Add `pub agent_perception: AgentPerceptionReport,` at the end (or after `agent_recommendations` for grouping):

```rust
pub struct ProjectionBundle {
    pub live_snapshot: LiveSnapshot,
    pub agent_snapshot: AgentSnapshot,
    pub agent_briefing: AgentBriefing,
    pub agent_session: AgentSession,
    pub agent_recommendations: AgentRecommendations,
    pub agent_perception: AgentPerceptionReport,
    pub agent_watchlist: AgentWatchlist,
    pub agent_scoreboard: AgentAlertScoreboard,
    pub agent_eod_review: AgentEodReview,
    pub agent_narration: AgentNarration,
}
```

- [ ] **Step 5: Build perception report alongside recommendations in projection**

In `src/core/projection.rs:85` (and the symmetric `:136` for US), find:

```rust
    let agent_recommendations = build_recommendations(&agent_snapshot, Some(&agent_session));
```

Add immediately after:

```rust
    let agent_perception = build_perception_report(&agent_snapshot);
```

Then add `agent_perception,` to the `ProjectionBundle { ... }` struct literals at both call sites.

You may need to add `use crate::agent::build_perception_report;` near the top of `src/core/projection.rs` if not already imported.

- [ ] **Step 6: Wire write into `src/core/runtime/projection.rs`**

In `src/core/runtime/projection.rs:92` area, find:

```rust
    push_artifact!(
        paths.agent_recommendations_path.clone(),
        &projection.agent_recommendations,
        "agent_recommendations"
    );
```

Add right after:

```rust
    push_artifact!(
        paths.agent_perception_path.clone(),
        &projection.agent_perception,
        "agent_perception"
    );
```

- [ ] **Step 7: Build to verify**

Run: `cargo build --features persistence --bin eden --bin eden-api 2>&1 | tail -10`

Expected: build succeeds (may have warnings, no errors).

- [ ] **Step 8: Run full lib test target**

Run: `cargo test --features persistence --lib 2>&1 | tail -5`

Expected: `test result: ok. 1427 passed; 0 failed` (1425 + 2 perception tests)

- [ ] **Step 9: Commit**

```bash
git add src/agent/perception.rs src/agent/mod.rs src/core/projection.rs src/core/runtime/projection.rs
git commit -m "$(cat <<'EOF'
feat(agent,core): build + write AgentPerceptionReport per tick

Adds agent/perception.rs with AgentPerceptionReport (Y-facing JSON
schema) and build_perception_report. ProjectionBundle gains
agent_perception field; runtime projection writer pushes a new
artifact every tick to data/agent_perception.json (HK) and
data/us_agent_perception.json (US).

Eden does not judge here — perception is surfaced; Y reads and
decides.

Per spec 2026-04-30-perception-report-design.md task 11.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Live smoke test on HK runtime

**Files:** none modified (verification only)

- [ ] **Step 1: Stop any running HK runtime**

Run:

```bash
[ -f .run/eden-hk.pid ] && kill $(cat .run/eden-hk.pid) 2>/dev/null; sleep 1; ps -p $(cat .run/eden-hk.pid 2>/dev/null) 2>/dev/null || echo "stopped"
```

Expected: `stopped` (or similar — process not running).

- [ ] **Step 2: Build release-quality binary for HK**

Run: `cargo build --features persistence --bin eden 2>&1 | tail -5`

Expected: `Finished `dev` profile [unoptimized + debuginfo]`

- [ ] **Step 3: Start HK runtime in background, capturing logs**

Run:

```bash
EDEN_DB_PATH=.run/stack-hk.db nohup ./target/debug/eden >.run/eden-hk.log 2>&1 &
echo $! > .run/eden-hk.pid
sleep 8
ps -p $(cat .run/eden-hk.pid) -o pid,stat,etime 2>/dev/null | tail -1
```

Expected: process listed, "S" or "R" status.

- [ ] **Step 4: Wait for first tick to produce perception artifact**

Use Monitor (or shell loop) — wait for `data/agent_perception.json` to be created and for size to be > 100 bytes:

```bash
until [ -s data/agent_perception.json ] && [ $(wc -c < data/agent_perception.json) -gt 100 ]; do sleep 5; done
echo "perception file ready"
```

Expected timeout: up to 10 minutes (because BP currently runs slow per current state).

- [ ] **Step 5: Inspect the perception report**

Run:

```bash
cat data/agent_perception.json | jq '{
  tick,
  emergent_cluster_count: (.perception.emergent_clusters | length),
  sector_leader_count: (.perception.sector_leaders | length),
  causal_chain_count: (.perception.causal_chains | length),
  anomaly_count: (.perception.anomaly_alerts | length),
  regime_present: (.perception.regime != null),
  semiconductor_cluster: (.perception.emergent_clusters[] | select(.sector == "semiconductor"))
}'
```

Expected (during HK regular session):
- `emergent_cluster_count >= 5` (multiple sectors hit 70%+ sync)
- `sector_leader_count >= 3` (vs_sector_contrast >= 3.0 surfaces several leaders)
- `causal_chain_count = 0` (lead-lag stale; documented in spec)
- `anomaly_count >= 1`
- `regime_present = true`
- `semiconductor_cluster` shows fields populated correctly

- [ ] **Step 6: Confirm recommendations file still works (backwards compat)**

Run: `[ -s data/agent_recommendations.json ] && jq '.tick' data/agent_recommendations.json`

Expected: prints a tick number — old artifact still being written.

- [ ] **Step 7: Stop runtime**

Run: `kill $(cat .run/eden-hk.pid) 2>/dev/null; sleep 1; ps -p $(cat .run/eden-hk.pid 2>/dev/null) 2>/dev/null || echo "stopped"`

Expected: `stopped`.

- [ ] **Step 8: Confirm test count after smoke**

Run: `cargo test --features persistence --lib 2>&1 | tail -3`

Expected: `test result: ok. 1427 passed; 0 failed`

- [ ] **Step 9: Commit nothing — milestone complete**

This task is verification only. No commit. Update the user with the smoke results.

---

## Self-Review

After writing the plan, walked it back through the spec:

**Spec coverage** — all design goals & non-goals mapped to tasks:
- ✅ Surface, don't judge — Task 11 schema has no bias/action fields
- ✅ Multi-modal richness — Tasks 3-7 cover all 5 modalities separately
- ✅ Backwards-compatible — Task 11 leaves recommendations.rs intact
- ✅ Cheap I/O — Task 2 tail-reads bounded buffer
- ✅ Schema-versioned — `schema_version: u32 = 1` in Task 1 + Task 11
- ✅ Y-readable — Task 11 schema is plain JSON

**Placeholder scan**: Task 9 Step 3 has `<tick_local>` placeholder — but it's followed by an explicit instruction to look up the actual variable name in the US builder file. Acceptable because the engineer needs to inspect existing code; can't be hardcoded across both markets without first reading both. Same applies to "may need to add `use ...`" in Task 11 Step 5 — the build error will guide them.

**Type consistency**: All field names match between Task 1 (definitions) and downstream tasks (`emergent_clusters`, `sector_leaders`, etc.). `EdenPerception` schema_version = 1 in Tasks 1 + 8. `AgentPerceptionReport` schema_version = 1 in Task 11.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-30-perception-report.md`.

**Two execution options:**

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
