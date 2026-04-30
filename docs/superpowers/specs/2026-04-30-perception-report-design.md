# L4 Perception Report — Opening Eden's Internal World to Y

**Date**: 2026-04-30
**Status**: Design (awaiting user review)
**Author**: claude (with hayhay2323)

---

## Context

Eden has a 4-layer architecture (per `memory/eden_unified_thesis.md`):

- **L1 純感官** — `pipeline/` modules: BP, channels, emergence, contrast, lead-lag, surprise, regime detection
- **L2 反射** — currently almost empty
- **L3 身體記憶** — `persistence/`, `lineage/`, `cases/`
- **L4 內在世界** — graph state per tick: what Y "sees" looking through eden

L1 actively produces rich perception output: 27 NDJSON streams in `.run/eden-*-hk.ndjson` containing emergence clusters, sector-leader contrasts, causal lead-lag chains, surprise outliers, regime analogs, active probe outcomes, and more.

**Y** (the consciousness consuming eden's perception) is currently a hybrid of:
- Human user reading agent_*.json files
- Future LLM agent (Codex / Claude) consuming via `agent/codex.rs` bridge

**The gap**: Y has no read-access to L1's rich output. The current `agent/recommendations.rs` plays a 1990s-quant judge role (`bias=short, action=wait`) using only `breadth_up`, `breadth_down`, `synchrony`, `pressure_consensus` from `AgentSnapshot.market_regime` and `AgentSnapshot.stress`. It does not consume `eden-emergence-hk.ndjson`, `eden-contrast-hk.ndjson`, `eden-lead-lag-hk.ndjson`, `eden-surprise-hk.ndjson`, or `eden-regime-analog-hk.ndjson`.

**Result**: Eden's strongest perceptual signals (e.g., 9/9 semiconductor cluster sync, 6869.HK contrast +7.82 leadership, 6883→2477 causal chain at corr 0.89) are written to disk and never reach Y.

## Problem statement

Build the smallest, lowest-risk surface that makes eden's L1 perception output readable by Y, in a form Y can use to make decisions — without eden itself trying to be Y.

## Design goals

1. **Surface, don't judge** — emit "what eden currently perceives", not "what to do about it"
2. **Multi-modal richness** — preserve emergence + contrast + lead-lag + surprise + regime as parallel perceptual modalities, not collapsed into a single scalar
3. **Backwards-compatible** — keep existing `agent_recommendations.json` working until downstream consumers (frontend, scripts, LLM) migrate
4. **Cheap I/O** — read tail of NDJSON streams, not full file
5. **Schema-versioned** — future-proof the JSON schema
6. **Y-readable** — schema designed for human + LLM consumption (clear field names, no internal hashes)

## Non-goals (explicit scope boundaries)

- Not rewriting BP, substrate, or channels
- Not fixing vol channel bug (separate issue)
- Not solving BP 22min/tick performance (separate issue, "Phase E")
- Not solving lead-lag stale (separate diagnostic)
- Not adding LLM/Codex consumer plumbing (separate milestone — can build on this surface)
- Not changing frontend rendering
- Not deleting `recommendations.rs` (keep for backwards compat, mark deprecated)
- Not introducing reflex layer (L2) — this milestone is purely about L4 surfacing
- Not redesigning AgentSnapshot's existing fields

## Architecture

### Data flow

```
[L1 producers — already running]
    ↓ (writes NDJSON every tick)
.run/eden-emergence-hk.ndjson
.run/eden-contrast-hk.ndjson
.run/eden-lead-lag-hk.ndjson         (currently stale — separate fix)
.run/eden-surprise-hk.ndjson
.run/eden-regime-analog-hk.ndjson
    ↓
[NEW: PerceptionReader in live_snapshot.rs or new file]
    ↓ (tail-reads latest N records per stream)
[NEW: AgentSnapshot.perception field]
    ↓
[NEW: agent/perception.rs — build_perception_report()]
    ↓
[NEW: data/agent_perception.json] — main Y-facing surface
    ↓
[Y reads this file]
```

### File-level changes

| File | Change |
|---|---|
| `src/agent/types/perception.rs` (NEW) | Define `EdenPerception` struct + sub-types (`EmergentCluster`, `SymbolContrast`, `LeadLagEdge`, `SurpriseAlert`, `RegimePerception`) |
| `src/agent/types/snapshot.rs` | Add `pub perception: Option<EdenPerception>` field to `AgentSnapshot` (Option for backwards compat) |
| `src/agent/types/mod.rs` | Add `pub mod perception` |
| `src/live_snapshot.rs` | Add `read_perception_streams(market: &str) -> EdenPerception` function (tail-reads NDJSON streams) |
| `src/live_snapshot.rs` | Modify `build_*_snapshot` callers to populate `perception` field |
| `src/agent/perception.rs` (NEW) | `build_perception_report(snapshot: &AgentSnapshot) -> AgentPerceptionReport` — extract perception from snapshot into Y-facing JSON |
| `src/agent/mod.rs` | Add `pub mod perception` + export |
| `src/agent/io.rs` (or wherever) | Add `write_perception_report(report: &AgentPerceptionReport, path)` |
| Caller of `write_recommendations` | Also call `write_perception_report` to `data/agent_perception.json` |

### NDJSON tail-read approach

Reading 1.5 GB `eden-subkg-hk.ndjson` from start is unacceptable. Approach:

```rust
fn tail_records<T: DeserializeOwned>(path: &Path, max_records: usize) -> Vec<T> {
    // 1. Open file, seek to (file_size - reasonable_buffer)
    // 2. Read from there to end
    // 3. Skip partial first line
    // 4. Parse each subsequent line as T
    // 5. Return up to max_records most-recent
}
```

Buffer size heuristic: 256 KB per stream — enough to capture ~50-200 records at typical line size.

Edge cases:
- File smaller than buffer → seek to 0, read whole file (no partial-line trimming needed).
- File doesn't exist → return empty Vec (fresh runtime, no tick written yet).
- Buffer ends mid-line → drop trailing partial line.
- Buffer starts mid-line → drop leading partial line.
- JSON parse error on a line → log + skip, continue with next line (resilience over fail-fast).

For `eden-visual-graph-frame-hk.ndjson` (20 MB / line), this stream is **not** consumed by perception report (too heavy; consumed only by frontend renderer). Perception report uses lighter streams.

Streams consumed:
- `eden-emergence-hk.ndjson` — cluster sync per tick
- `eden-contrast-hk.ndjson` — symbol-vs-sector leadership
- `eden-lead-lag-hk.ndjson` — causal chains (will be empty until lead-lag stale fixed)
- `eden-surprise-hk.ndjson` — outliers
- `eden-regime-analog-hk.ndjson` — regime + historical outcomes

### Schema design

#### `EdenPerception` (Rust struct)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdenPerception {
    pub schema_version: u32,            // = 1
    pub market: LiveMarket,
    pub tick: u64,
    pub timestamp: String,

    /// Sectors where members move in lock-step. Direct read from
    /// eden-emergence-hk.ndjson at this tick.
    pub emergent_clusters: Vec<EmergentCluster>,

    /// Symbols significantly above their sector activation. Read from
    /// eden-contrast-hk.ndjson; filtered to vs_sector_contrast >= 3.0.
    pub sector_leaders: Vec<SymbolContrast>,

    /// Detected leader→follower temporal chains. Read from
    /// eden-lead-lag-hk.ndjson; filtered to abs(correlation_at_lag) >= 0.5.
    pub causal_chains: Vec<LeadLagEdge>,

    /// Symbols whose channel observation deviates from model expectation.
    /// Read from eden-surprise-hk.ndjson; filtered to total_surprise >= 1.5*floor.
    pub anomaly_alerts: Vec<SurpriseAlert>,

    /// Current macro regime + historical analog outcomes.
    pub regime: Option<RegimePerception>,
}
```

#### Sub-types

```rust
pub struct EmergentCluster {
    pub sector: String,
    pub total_members: u32,
    pub sync_member_count: u32,
    pub sync_ratio: String,             // "9/9" for display
    pub sync_pct: f64,                  // 1.0 for fully synced
    pub strongest_member: String,
    pub strongest_activation: f64,
    pub mean_activation_intent: f64,
    pub mean_activation_pressure: f64,
}

pub struct SymbolContrast {
    pub symbol: String,
    pub sector: Option<String>,
    pub center_activation: f64,
    pub sector_mean: f64,
    pub vs_sector_contrast: f64,        // primary ranking field
    pub node_kind: String,              // "Role" / "Pressure" / etc.
    pub persistence_ticks: Option<u32>, // how many ticks this contrast has held (if computable)
}

pub struct LeadLagEdge {
    pub leader: String,
    pub follower: String,
    pub lag_ticks: i32,
    pub correlation: f64,
    pub n_samples: usize,
    pub direction: String,              // "from_leads" / "to_leads"
}

pub struct SurpriseAlert {
    pub symbol: String,
    pub channel: String,                // e.g. "PressureStructure"
    pub observed: f64,
    pub expected: f64,
    pub squared_error: f64,
    pub total_surprise: f64,
    pub floor: f64,
    pub deviation_kind: String,         // "below_expected" / "above_expected"
}

pub struct RegimePerception {
    pub bucket: String,                 // "stress=4|sync=4|bias=2|act=3|turn=3"
    pub historical_visits: u32,
    pub last_seen_tick: Option<u64>,
    pub forward_outcomes: Vec<RegimeForward>,
}

pub struct RegimeForward {
    pub horizon_ticks: u32,
    pub n_samples: u32,
    pub mean_stress_delta: f64,
    pub mean_synchrony_delta: f64,
    pub mean_bull_bias_delta: f64,
}
```

#### `AgentPerceptionReport` (Y-facing JSON output)

```json
{
  "schema_version": 1,
  "tick": 24,
  "timestamp": "2026-04-30T09:26:29Z",
  "market": "Hk",
  "perception": {
    "emergent_clusters": [
      {
        "sector": "semiconductor",
        "sync_ratio": "9/9",
        "sync_pct": 1.0,
        "strongest_member": "6809.HK",
        "strongest_activation": 0.79,
        "members": ["6082.HK", "1347.HK", "6809.HK", "3896.HK", "1385.HK", "6869.HK", "981.HK", "600.HK", "2518.HK"]
      }
    ],
    "sector_leaders": [
      {
        "symbol": "6869.HK",
        "sector": "semiconductor",
        "vs_sector_contrast": 7.82,
        "center_activation": 13.68,
        "sector_mean": 5.85
      }
    ],
    "causal_chains": [
      {
        "leader": "6883.HK",
        "follower": "2477.HK",
        "lag_ticks": 3,
        "correlation": 0.89
      }
    ],
    "anomaly_alerts": [
      {
        "symbol": "1800.HK",
        "channel": "PressureStructure",
        "observed": 0.68,
        "expected": 1.88,
        "deviation_kind": "below_expected",
        "total_surprise": 1.46
      }
    ],
    "regime": {
      "bucket": "stress=4|sync=4|bias=2|act=3|turn=3",
      "historical_visits": 188,
      "forward_outcomes": [
        {"horizon_ticks": 5, "n_samples": 169, "mean_stress_delta": -0.003},
        {"horizon_ticks": 30, "n_samples": 89, "mean_stress_delta": -0.048},
        {"horizon_ticks": 100, "n_samples": 14, "mean_stress_delta": -0.147}
      ]
    }
  },
  "deprecated": {
    "old_recommendation_engine_still_runs_at": "data/agent_recommendations.json"
  }
}
```

**Key design choice — no `bias` field, no `action` field, no `recommendation` field at top level.** Y reads the perception, decides for itself.

### Filtering / surfacing rules

To avoid noise:
- `emergent_clusters`: only sectors with `sync_pct >= 0.7` (i.e., 70%+ members synced)
- `sector_leaders`: only symbols with `vs_sector_contrast >= 3.0`, sorted descending, top 20
- `causal_chains`: only `abs(correlation) >= 0.5` AND `n_samples >= 10`, sorted by correlation desc, top 30
- `anomaly_alerts`: only `total_surprise >= 1.5 * floor`, sorted by `total_surprise / floor` desc, top 15
- `regime`: always include current regime if available

These thresholds should be CONFIG, not hardcoded:

```rust
pub struct PerceptionFilterConfig {
    pub min_cluster_sync_pct: f64,          // default 0.7
    pub min_leader_contrast: f64,           // default 3.0
    pub max_leaders: usize,                 // default 20
    pub min_chain_correlation: f64,         // default 0.5
    pub min_chain_samples: usize,           // default 10
    pub max_chains: usize,                  // default 30
    pub min_anomaly_surprise_ratio: f64,    // default 1.5
    pub max_anomalies: usize,               // default 15
}
```

Provide `PerceptionFilterConfig::default()` with the values listed above. MVP wires defaults at the call site; env-var / config-file overrides are future work.

## Migration / backwards compatibility

- Existing `agent_recommendations.json` write-path remains unchanged
- New `agent_perception.json` is written in parallel, same call site
- `AgentSnapshot.perception` is `Option<>` so old serialised snapshots still deserialize
- Frontend / scripts continue using `agent_recommendations.json` until they migrate
- `recommendations.rs` gets a `// DEPRECATED: see agent/perception.rs` doc comment, but keeps functioning

## Testing strategy

### Unit tests
- `tail_records` reads correct subset of synthetic NDJSON file
- Filter thresholds correctly drop / retain sample records
- `build_perception_report` output matches expected JSON for a fixture snapshot
- Schema version round-trips through serde

### Integration tests
- Synthetic NDJSON files in tests fixture (small, hand-crafted)
- Verify end-to-end: NDJSON → perception report → JSON output
- Verify empty NDJSON files produce empty (not erroring) perception

### Live verification
- Run HK runtime
- After a few ticks, `cat data/agent_perception.json | jq`
- Confirm:
  - emergent_clusters non-empty for current session (semiconductor 9/9 should appear)
  - sector_leaders non-empty (6869.HK should be top)
  - causal_chains: empty OK if lead-lag stale
  - anomaly_alerts: should include 1800.HK or similar surprise events
  - regime: current bucket present with historical_visits > 0

### Manual / subjective
- User reads `agent_perception.json` and `agent_recommendations.json` side-by-side
- Question: does perception report convey eden's "view of the market" more vividly than recommendations?
- Acceptance: subjectively yes

## Risks and mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| NDJSON tail-read seeks past file end | low | crash | bounds-check seek; if buffer > file size, read whole file |
| Partial first line in tail buffer | high | parse error | skip first line of buffer (incomplete) |
| NDJSON file doesn't exist (fresh runtime) | medium | empty perception | return default `EdenPerception` |
| Lead-lag NDJSON stale → causal_chains always empty | high | reduced perception | acceptable; documented; separate issue |
| Frontend breaks from new field | low | UI bug | field is Option; existing serialisation unchanged |
| LLM-Codex consumer expects old format | medium | bridge breakage | leave `recommendations.rs` running |
| Schema 1 wrong → migration | medium | spec rework | schema_version field allows v1 → v2 evolution |
| Reading large NDJSON on every snapshot build slows down | medium | latency | tail-read with bounded buffer; cache last-read offset |

## Success criteria

**Quantitative**:
- `data/agent_perception.json` written every tick (parallel to recommendations.json)
- Mean file size < 100 KB
- `emergent_clusters` populated for ≥ 8 / 15 sectors during regular HK session
- `sector_leaders` populated with ≥ 5 entries (vs_sector_contrast ≥ 3.0)
- `regime.bucket` present and matches current `regime-analog` ndjson tail
- ≥1414 lib tests pass (current 1412 + new tests covering: tail_records edge cases, filter thresholds, build_perception_report fixture roundtrip)

**Qualitative** (user judgment):
- Reading `agent_perception.json` conveys "what eden currently sees" more vividly than reading `agent_recommendations.json`
- The signals we identified manually (semiconductor 9/9 sync, 6869 leadership, 6883 causal chain) appear in perception report

## Future work (out of scope but enabled by this)

- **L2 reflex layer**: build `agent/reflex.rs` that watches perception stream for known archetypes, emits immediate alerts
- **LLM consumer**: have Codex/Claude subscribe to perception report, generate Y-level deliberation
- **Frontend perception viewer**: web UI rendering perception report directly (not via recommendation)
- **Cross-perception integration**: when perception detects coherent multi-modal pattern (cluster + leader + causal chain align), emit composite alert
- **Perception-driven calibration**: track which perception patterns precede actual moves; use as feedback to L1 (not L4 itself)

## Implementation outline (will be detailed in plan)

1. Add `agent/types/perception.rs` (struct definitions)
2. Add `live_snapshot::tail_records<T>` helper
3. Add `live_snapshot::read_perception_streams("hk")` function
4. Wire into snapshot builder for HK + US
5. Add `agent/perception.rs::build_perception_report`
6. Add `agent/perception.rs::write_perception_report`
7. Wire write into existing per-tick agent IO loop
8. Tests
9. Live smoke test against HK runtime

Estimated: 1 day total work. 1412 → 1413+ tests. Net new code ~600 lines. No deletes.
