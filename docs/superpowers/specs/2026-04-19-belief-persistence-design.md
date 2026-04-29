# Belief Persistence — Eden's First Cross-Tick Memory Trace

**Date**: 2026-04-19
**Author**: Claude Code (with operator)
**Status**: Design for review

## 為什麼做

Eden 的 `src/pipeline/belief.rs` 已經有 GaussianBelief + CategoricalBelief + Welford + KL，但是**無狀態 helper** — 每次呼叫都從 raw samples 重建 belief。這意味著：

- **Eden 沒有跨 tick 的世界模型**。每 tick 重跑 pipeline → 輸出 wake，中間沒有持續演化的 belief。
- **MI / do-calculus / intervention** 每次都要帶 prior，但 prior 本身每 tick 重建，不是累積的。
- **時間不讓 Eden 變聰明**（Y 最核心的屬性）。Eden 坐一整天跟坐一分鐘，感知能力一樣。

這是 CLAUDE.md P0 #1 的工作：把 stateless helper 升級成持續演化的 `PressureBeliefField`，跨 tick 在記憶體內維護，定期 snapshot 到 SurrealDB，重啟時 restore。

這是 A 路線的第一塊（A2 decisions ingestor 在後續 spec）。

## 北極星：這是「Eden 第一次有世界模型」

不是 feature。是**架構層表徵轉變** — 從「每 tick 建構一次 snapshot」變成「持續演化的 belief，wake 是從它 query 的視圖」。這個轉變解鎖後續所有 Y-式應用（dreaming / ontology emergence / active probing）。

## 本 spec 的 scope（嚴格 bounded）

**In**：
- 新模組 `src/pipeline/belief_field.rs` — `PressureBeliefField` 結構
- 新模組 `src/persistence/belief_snapshot.rs` — save/load 邏輯
- SurrealDB 新 migration — `belief_snapshot` table
- HK + US runtime 整合（對稱、各自一個實例）
- Wake line 新表面（per-tick top 5, informed + notable only）
- Restore-on-startup
- Tests: unit + integration (snapshot roundtrip + restart continuity)

**Out（明確不做）**：
- Decisions/ ingestor（A2，下一個 spec）
- Dreaming runner（A3，需要 A1 + replay）
- 跨 HK/US 共享 belief（multi-market coupling，獨立 spec）
- Belief compaction / retention policy
- Variance floor tuning / hyperparam sweep
- Dashboard / frontend 顯示（純 wake surface）
- MI ranking attention wake（P0 #2，獨立 spec）
- Intervention wake（已有，不動）

## 架構

### 模組結構

```
src/pipeline/belief_field.rs          # 新 — 核心結構
src/persistence/belief_snapshot.rs    # 新 — save / load / restore
src/persistence/schema.rs             # 改 — append MIGRATION_NNN
src/hk/runtime.rs                     # 改 — 整合
src/us/runtime.rs                     # 改 — 對稱整合
tests/belief_field_integration.rs     # 新 — restart continuity test
```

### 資料模型

```rust
// src/pipeline/belief_field.rs

pub struct PressureBeliefField {
    // Per-(symbol, channel) continuous distribution of pressure values.
    // Updated every tick; survives tick-to-tick in memory.
    gaussian: HashMap<(SymbolId, ChannelKind), GaussianBelief>,

    // Per-symbol categorical distribution over state_kind variants.
    // (Continuation, TurningPoint, LowInformation, Conflicted, Latent)
    categorical: HashMap<SymbolId, CategoricalBelief<StateKind>>,

    // Bookkeeping
    last_tick: u64,
    last_snapshot_ts: Option<DateTime<Utc>>,
    market: Market, // HK or US — one field per market, not shared
}

impl PressureBeliefField {
    pub fn new(market: Market) -> Self { ... }

    pub fn update_from_pressure(&mut self, pressure: &PressureField, tick: u64) { ... }

    pub fn update_state(&mut self, symbol: SymbolId, state: StateKind) { ... }

    pub fn query_gaussian(&self, symbol: SymbolId, channel: ChannelKind) -> Option<&GaussianBelief> { ... }

    pub fn query_state_posterior(&self, symbol: SymbolId) -> Option<&CategoricalBelief<StateKind>> { ... }

    pub fn top_notable_beliefs(&self, k: usize) -> Vec<NotableBelief> { ... }

    pub fn informed_count(&self) -> usize { ... }
}
```

### Snapshot 格式（序列化到 SurrealDB）

```rust
// src/persistence/belief_snapshot.rs

#[derive(Serialize, Deserialize)]
pub struct BeliefSnapshot {
    pub market: String,       // "hk" or "us"
    pub snapshot_ts: DateTime<Utc>,
    pub tick: u64,
    pub gaussian: Vec<GaussianSnapshotRow>,
    pub categorical: Vec<CategoricalSnapshotRow>,
}

#[derive(Serialize, Deserialize)]
pub struct GaussianSnapshotRow {
    pub symbol: String,       // SymbolId as string
    pub channel: String,      // ChannelKind as string
    pub mean: f64,
    pub variance: f64,
    pub sample_count: u32,
    pub m2: f64,              // Welford internal, needed for merge-correctness
}

#[derive(Serialize, Deserialize)]
pub struct CategoricalSnapshotRow {
    pub symbol: String,
    pub distribution: Vec<(String, f64)>, // (state_kind_name, probability_mass)
    pub sample_count: u32,
}
```

### SurrealDB Migration

```sql
-- MIGRATION_NNN: Belief snapshot table
DEFINE TABLE belief_snapshot SCHEMAFULL;
DEFINE FIELD market ON belief_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON belief_snapshot TYPE datetime;
DEFINE FIELD tick ON belief_snapshot TYPE int;
DEFINE FIELD gaussian ON belief_snapshot TYPE array;
DEFINE FIELD categorical ON belief_snapshot TYPE array;
DEFINE INDEX idx_belief_market_ts ON belief_snapshot FIELDS market, snapshot_ts;
```

Append-only migration；不影響現有 33 個 migration。

## 資料流

### 每 tick（hot path）

```
┌─────────────────────────────────────────────────────────┐
│ 1. Market data pushes → tick aggregation (現有)           │
│ 2. Pressure field built (現有 pipeline::pressure)         │
│ 3. State classification per symbol (現有 state_engine)   │
│ ───────────────────────────────────────────────────── ✨ │
│ 4. **新**: belief_field.update_from_pressure(&pf, tick)  │
│    - 遍歷 (symbol, channel) → Welford update             │
│    - 遍歷 symbol → Categorical update from state         │
│    - O(symbols × channels) = ~6900 O(1) ops, <1ms         │
│ 5. **新**: belief_field.update_state(symbol, state) [x5] │
│ ───────────────────────────────────────────────────── ✨ │
│ 6. Reasoning / vortex / setup (現有)                      │
│ 7. Wake output (現有 + 新增 belief: ... lines)            │
│ 8. Tick persistence (現有)                                │
│    如果距離上次 snapshot >= 60s：snapshot belief_field      │
└─────────────────────────────────────────────────────────┘
```

Belief update **在 reasoning 之前** — 讓下游能用最新 belief。但 reasoning 本身不強制改 — 這個 spec 只建 belief field，不改 reasoning 邏輯；future spec 才會把 MI / KL-tension 接到 belief_field。

### Snapshot（每 60s）

```
tick 結束 → 檢查 (now - last_snapshot_ts) >= 60s
  是 → serialize field → INSERT INTO belief_snapshot
     → update last_snapshot_ts
     → log "[belief] snapshot written: N gaussian, M categorical"
  否 → skip
```

**只存 informed beliefs**（sample_count >= 1）— uninformed 是語義空的，不浪費 IO。

### Restore（startup）

```
Eden 啟動（pressure field init 完之後）
  → EdenStore::latest_belief_snapshot(market=hk)
    → SELECT * FROM belief_snapshot WHERE market = 'hk'
       ORDER BY snapshot_ts DESC LIMIT 1
  → Some(snapshot) → deserialize → seed PressureBeliefField
                    → log "[belief] restored N beliefs from ts=X"
  → None → fresh field
          → log "[belief] starting with uninformed prior"
  → Error → fresh field + warning
           → log "[belief] restore failed: {err}; starting fresh"
```

Graceful degrade：beliefs 是**可重建的**（跑幾天就有 prior），不是 golden data，restore 失敗不影響 eden 主流程。

## Wake Surface

每 tick（HK 和 US 獨立），在現有 wake lines 後加：

```
belief: 0700.HK orderbook μ=1.23 σ²=0.08 n=5840 informed (KL vs 30min=0.82)
belief: 0700.HK state_posterior=turning_point 0.62, latent 0.28, continuation 0.10 (n=3421)
belief: 3690.HK capital_flow μ=-0.41 σ²=0.91 n=12 prior-heavy
```

**輸出規則**：
- `top_notable_beliefs(5)` — 每 tick 最多 5 行，避免洗螢幕
- **Notable Gaussian**：`sample_count >= BELIEF_INFORMED_MIN_SAMPLES (= 5)` **且**符合下列任一：
  - `|KL divergence vs 上一 tick 同 (symbol, channel) 的 belief quick snapshot| > 0.5`（需維護 `previous_gaussian: HashMap<(SymbolId, ChannelKind), GaussianBelief>` 作輕量 diff buffer，只存上一 tick，每 tick 覆寫）
  - `sample_count` 剛從 `< 5` 跨越到 `>= 5`（belief 剛「夠用」的 milestone 事件）
- **Notable Categorical**：符合下列任一：
  - `max posterior probability < 0.5`（significant uncertainty — 沒主導 state）
  - `sum of |p_i_now - p_i_prev_tick| > 0.3`（state posterior 整體 shift）
- **Prior-heavy** 標記：當 `sample_count < BELIEF_INFORMED_MIN_SAMPLES` 時在 wake line 尾端加 `prior-heavy`；告訴 operator 不要賦予太大權重
- 排序：Gaussian 按 `KL` 降冪、Categorical 按 `state shift` 降冪，兩類各選 top 2-3 組成 top 5

格式 follow 現有 wake line 風格（低噪音、grep-able）。

## 測試

### Unit（`src/pipeline/belief_field.rs` 內）

- `update_from_pressure_creates_belief_per_channel` — 新 (symbol, channel) 首次 update → 產生 from_first_sample belief
- `update_is_welford_correct` — 已有 belief 再 update → mean/variance 符合 Welford
- `top_notable_beliefs_returns_sorted_by_kl` — KL 大小排序
- `informed_count_only_counts_n_geq_5` — 明確 boundary
- `update_state_updates_categorical_posterior` — CategoricalBelief 更新正確

### Unit（`src/persistence/belief_snapshot.rs` 內）

- `snapshot_roundtrip_preserves_all_beliefs` — field → serialize → deserialize → same beliefs
- `snapshot_skips_uninformed_beliefs` — sample_count=0 不被寫入
- `load_returns_none_when_no_snapshot` — 空 DB → None
- `load_returns_latest_on_multiple_snapshots` — 多個 snapshot 時取最新

### Integration（`tests/belief_field_integration.rs`）

- `belief_survives_restart` — run 100 fake ticks → snapshot → reload → run 100 more → 驗證 belief 連續性（sample_count 總共 200）
- `snapshot_cadence_writes_every_60s` — 模擬 90s 的 tick stream → 驗證至少 1 次 snapshot 寫入，至多 2 次
- `hk_and_us_fields_are_independent` — 同時跑 HK + US → 各自 snapshot 不互相污染

### Benchmark（不 block merge 但要量）

- 測量 `update_from_pressure` 在 1150 symbols × 6 channels 的延遲 → 要求 < 2ms
- 測量 tick 端到端延遲增加 < 5%（vs. baseline 無 belief update）

## 風險與 Mitigation

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Hot path latency 超標 | Low | High | Benchmark first; 如果超標 defer update 到 tick 尾端 batch |
| SurrealDB IO 壓力（60s × 6900 beliefs）| Medium | Low | 只存 informed；monitor write latency；未來加 batch |
| Migration 失敗（33 現有）| Very Low | High | Append-only ADD TABLE，不動既有 schema；local test 先跑 |
| Restore 反序列化失敗 | Medium | Low | Graceful degrade → fresh field；不 panic |
| Belief field memory footprint | Low | Low | 6900 × ~30 bytes = ~200KB per market × 2 = ~400KB total；不足道 |
| 序列化鎖影響 tick | Medium | Medium | Snapshot 寫入 async/spawn；tick loop 不等 IO |

## 驗證 (Acceptance Criteria)

1. ✅ `cargo check --lib --features persistence -q` 通過
2. ✅ 所有新 unit tests 通過（≥ 8 個）
3. ✅ Integration test `belief_survives_restart` 通過
4. ✅ Tick latency 增加 < 5%（benchmark 前後對比）
5. ✅ HK + US runtime 啟動後，wake 中出現 `belief:` lines
6. ✅ 停 Eden → 等 70s → 重啟 → log 有 `[belief] restored N beliefs from ts=X`
7. ✅ SurrealDB `belief_snapshot` table 存在，每 60s ±10s 有新 row
8. ✅ `cargo check --lib -q --no-default-features` 仍通過（persistence 是 feature flag）

## 已決定（原開放問題）

1. **Granularity**：per-(symbol, channel) Gaussian + per-symbol Categorical<StateKind>
2. **HK/US**：各自獨立 field，不 shared（跨市場 coupling 是後續 spec）
3. **Update point**：pressure field built 之後、reasoning 之前
4. **Snapshot cadence**：60s，async write
5. **Storage**：SurrealDB `belief_snapshot` table, append-only, SCHEMAFULL
6. **Format**：SurrealDB object, not bincode（方便 debug + query）
7. **Restore**：load latest snapshot on startup，graceful degrade on failure
8. **Wake cap**：top 5 per tick, notable only
9. **Schema mismatch**：fresh field（beliefs 可重建）
10. **變動容忍**：不做 variance floor 動態調整 / 不做 retention — 未來 spec

## Out of Scope 備忘（將來 specs 會接）

| 未來 spec | 依賴 | 內容 |
|-----------|------|------|
| `2026-04-XX-decisions-ingestor-design.md` | 本 spec | Rust 讀 `decisions/` tree，update belief field（或另一 per-wake-shape belief）|
| `2026-04-XX-dreaming-runner-design.md` | 本 spec + decisions ingestor | replay historical ticks with current belief prior，produce delta report |
| `2026-04-XX-mi-attention-wake-design.md` | 本 spec | Information gain ranking 接 wake `attention: X gain=0.82 bits` |
| `2026-04-XX-multi-market-coupling-design.md` | 本 spec | HK + US belief fields 合體，covariance 作為 coupling signal |
| `2026-04-XX-ontology-emergence-design.md` | 本 spec + 長期 belief accumulation | 從持續殘差 pattern 提案新 entity type (Y#0) |

## Scope 備注

這個 spec **刻意聚焦在 A1**（Belief 持久化），不碰下游應用。理由：

- 地基先蓋，上面的樓層各自獨立
- 讓這一個 spec 的改動範圍可控（5 files，~1000-1200 LOC）
- 對稱 HK/US 但不合體，保持 T28 既有對稱模式不打破
- 不改 reasoning / wake logic，只新增 `belief:` line — 風險面最小

Spec 完成後 → `writing-plans` → implementation。
