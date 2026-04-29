# Decisions Ingestor — Eden 讀回 Claude Code 的行為

**Date**: 2026-04-19
**Author**: Claude Code (with operator)
**Status**: Design for review

## 為什麼做

今天早上做完了 decision log schema（`decisions/` 樹 + JSON）和 belief persistence (A1)。Decision log 目前是 **write-only** — Claude Code 寫 JSON 到 disk，但 Eden 不讀。這個 spec 讓 Eden 把 `decisions/` 讀進 runtime，建成 queryable 結構，並在 wake 裡顯示 operator 當下看到的 symbol 的 prior decisions 摘要。

**Y-principled：observation first, interpretation second。**

這個 spec 只做 **observation**（decisions → Rust data → wake line）。**Interpretation**（decisions → belief field update）是完全獨立的設計問題，留給 A2.5 另一 spec。

Spec chain:
- decision-log-design (2026-04-18) ✅ schema + backfill
- belief-persistence-design (2026-04-19) ✅ A1 cross-tick belief
- **this spec (A2)** — Rust 讀 decisions/
- A2.5 (future) — decisions → belief update 的 interpretation
- A3 (future) — dreaming with decisions as labels

## 本 spec scope（嚴格 bounded）

**In**：
- 新模組 `src/pipeline/decision_ledger.rs` + 子 module（scanner, wake_format）
- Startup scan + 60s rescan（piggyback belief snapshot cadence）
- HK + US 各自一個 DecisionLedger 實例
- Per-symbol 索引 + 預算 summary
- Wake emission：對當前 tick 的 notable symbols 輸出 `prior decisions:` 行
- Unit + integration tests

**Out（明確不做）**：
- Decision → belief update（A2.5 獨立 spec）
- Filesystem watcher (inotify/fsevents) — 60s rescan 夠用
- Old decision pruning / retention
- 前端 UI
- Wake shape feature extraction / clustering
- Mirror 到 SurrealDB（純 in-memory from JSON）
- Index 跨 market 聚合

## 資料模型

```rust
// src/pipeline/decision_ledger.rs

pub struct DecisionLedger {
    per_symbol: HashMap<Symbol, Vec<DecisionRecord>>,
    summaries: HashMap<Symbol, SymbolDecisionSummary>,
    market: Market,
    last_scan_ts: Option<DateTime<Utc>>,
    /// Paths already ingested — avoids duplicate parse + index
    ingested_paths: HashSet<PathBuf>,
    /// Counters surfaced in log
    ingested_count: usize,
    skipped_count: usize,
}

#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub timestamp: DateTime<Utc>,
    pub symbol: Symbol,
    pub action: DecisionAction,
    pub direction: Option<TradeDirection>,
    pub confidence: f64,
    pub linked_entry_id: Option<String>,
    pub outcome: Option<OutcomeSummary>,
    pub eden_gap: Option<String>,
    pub backfilled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionAction { Entry, Exit, Skip, SizeChange }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeDirection { Long, Short }

#[derive(Debug, Clone)]
pub struct OutcomeSummary {
    pub pnl_bps: f64,
    pub hold_duration_sec: u64,
    pub closing_reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolDecisionSummary {
    pub total_decisions: usize,
    pub entries: usize,
    pub exits: usize,
    pub skips: usize,
    pub size_changes: usize,
    pub net_pnl_bps: f64,
    pub last_action: Option<DecisionAction>,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub last_pnl_bps: Option<f64>,
    /// Unique, de-duped eden_gap values, most-recent first. Cap 3.
    pub unique_eden_gaps: Vec<String>,
}
```

**Rationale for summary predigestion**：per-tick wake emission 需要快速 lookup；eagerly 算一次 summary 在 ingest 時，wake 層就是 `O(1)` query。每次 rescan 可能只有 0-1 新檔加入，summary update 成本 tiny.

## Ingestion 策略

### Startup scan

```rust
pub fn scan_directory(root: &Path, market: Market) -> DecisionLedger;
```

從 `decisions/YYYY/MM/DD/*.json` 全樹 scan：
- 排除 `index.jsonl`
- 排除 `session-recap.json`
- 排除 `schemas/` 子樹（本身不是 decision 檔）
- 逐檔 parse JSON，filter by `market` field → 分到對應 ledger
- Bad JSON / wrong schema_version / missing required fields → `tracing::warn!` 一行 + skip（其他檔照讀）

### Per-tick rescan

每 60s（piggyback belief snapshot cadence 的 timer）：
- **只** scan `decisions/YYYY/MM/DD/` for 今天 + 昨天
- 透過 `ingested_paths: HashSet<PathBuf>` 擋重複
- 新檔 → parse → update per_symbol + summaries
- Log: `[decisions] rescan: N new, M skipped, K total`（N>0 時才 log，避免屏幕噪音）

**為什麼只掃 today + yesterday**：避免全 glob 數萬筆 — 掃描成本 O(files)，但新檔只會在近期。

### Error handling

所有錯誤都 **graceful degrade**，不 crash runtime：
- Missing `decisions/` 目錄 → 空 ledger + `tracing::info!("no decisions directory yet")`
- Bad JSON → `tracing::warn!` + skip
- schema_version ≠ 1 → `tracing::warn!` + skip（未來 migration 再處理）
- Market mismatch（HK ledger 遇到 US record）→ silently skip（正常情況；市場 split）

## Wake Emission

### 條件

Runtime 每 tick 呼叫 `belief_field.top_notable_beliefs(5)` 之後：
1. 對每個 notable 的 symbol，query `ledger.summary_for(&symbol)`
2. 若 summary 存在 **且** `total_decisions >= 1` → emit 一行

### 格式

```
prior decisions: 0700.HK 2 (1 entry @2026-04-15 +82bps, 1 skip @2026-04-18); eden_gap: peer_synchronization missing
```

**Break down**：
- Symbol + 總 decisions 數
- 最近的 action type + date + pnl_bps（若有 outcome）
- 若 summary.unique_eden_gaps 非空，追加 `; eden_gap: <top 1>`

**多 symbol 排序**：跟著 `top_notable_beliefs` 的順序（已經按 importance 排了）。

**頻率控制**：與 `belief:` 行同一次 top_notable 輸出，自動跟著 cap=5 走。

### Implementation

```rust
// src/pipeline/decision_ledger/wake_format.rs

pub fn format_prior_decisions_line(
    symbol: &Symbol,
    summary: &SymbolDecisionSummary,
) -> String;
```

HK / US runtime 都呼叫同一個 helper（跟 format_wake_line for belief 一致風格）。

## 模組結構

```
src/pipeline/decision_ledger.rs               # 核心 struct + query API
src/pipeline/decision_ledger/
  ├── scanner.rs                              # file glob + JSON parsing
  └── wake_format.rs                          # format_prior_decisions_line
src/pipeline/mod.rs                           # +pub mod decision_ledger
src/hk/runtime.rs                             # 啟動 scan + rescan + wake emit
src/us/runtime.rs                             # 對稱
tests/decision_ledger_integration.rs          # end-to-end: 讀 decisions/2026/04/15
```

**不動** `belief_field.rs`、`belief_snapshot.rs`。

## 資料流

```
┌─────────────────────────────────────────────────────────┐
│ Startup:                                                 │
│   DecisionLedger::scan_directory("decisions/", Hk)       │
│   → parse all JSON, build per_symbol + summaries         │
│   → log "[decisions] ingested N for market=hk"           │
├─────────────────────────────────────────────────────────┤
│ Per tick (後接 A1 belief block):                         │
│   1. belief_field update + top_notable_beliefs(5)         │
│   2. 新增：for each notable symbol:                      │
│      summary = ledger.summary_for(symbol)                │
│      if summary.total_decisions >= 1:                    │
│        wake.reasons.push(format_prior_decisions_line(..))│
│   3. 現有 snapshot cadence                                │
│   4. 每 60s：ledger.rescan(today, yesterday)              │
└─────────────────────────────────────────────────────────┘
```

## 測試

### Unit tests（`decision_ledger.rs` 內）

1. `empty_directory_produces_empty_ledger`
2. `single_entry_decision_indexed_per_symbol`
3. `exit_linked_to_entry_updates_summary_pnl`
4. `skip_decision_counted_but_no_pnl_impact`
5. `schema_version_mismatch_skipped_with_warning`
6. `duplicate_path_ingest_is_idempotent`
7. `market_split_hk_only_for_hk_ledger`
8. `summary_unique_eden_gaps_deduped_cap_3`

### Unit tests（`scanner.rs` 內）

1. `scan_skips_index_jsonl_and_session_recap`
2. `scan_skips_schemas_subtree`
3. `rescan_with_paths_only_returns_new_files`

### Unit tests（`wake_format.rs` 內）

1. `format_shows_entry_and_pnl`
2. `format_shows_skip_without_pnl`
3. `format_appends_top_eden_gap_if_present`

### Integration test（`tests/decision_ledger_integration.rs`）

1. `ingests_three_decisions_from_2026_04_15`：
   - 讀 `decisions/2026/04/15/`
   - 驗證 HK ledger 空、US ledger 有 2 個 symbols（KC.US + HUBS.US）
   - KC.US summary: 1 entry + 1 exit linked, net_pnl_bps = -18
   - HUBS.US summary: 1 entry, no exit, net_pnl_bps = 0

2. `rescan_picks_up_new_decision`：
   - 啟始 ledger 讀 2026/04/15
   - 手動寫一個新 decision 到 2026/04/15/
   - `ledger.rescan(...)` → 新 decision 進 ledger

## 風險與 Mitigation

| Risk | Mitigation |
|------|-----------|
| 啟動時 scan 慢（萬筆 decisions）| 目前 6 筆 + backfill 少，未來考慮並行 scan；現在不做 |
| 60s rescan 命中大量新檔案 | 今天+昨天目錄 bound 檔案數；scan 時間可忽略 |
| JSON schema 演化（v2）| 現在 hard-reject schema_version!=1，future spec 加 migration |
| Wake 行爆量（operator 每次看 10+ 行）| top_notable cap=5 天然限制 |
| Duplicate ingest 造成 summary 雙倍 | HashSet<PathBuf> 擋 |
| Market tag 錯誤 | scanner filter by `market` field；不 throw |

## 驗收

1. ✅ `cargo check --lib -q` + `--features persistence` 通過
2. ✅ All new unit tests 通過（≥ 14 個）
3. ✅ Integration test `ingests_three_decisions_from_2026_04_15` 通過
4. ✅ Startup log：`[decisions] ingested N for market=hk`（HK 會是 0，US 會是 3）
5. ✅ Live wake 中出現 `prior decisions:` 行（notable symbol 有 prior 時）
6. ✅ Belief field tests 仍然全過（0 regression）
7. ✅ `cargo check --lib --no-default-features -q` 仍通過

## Out of Scope 備忘（後續 specs）

| Future spec | 依賴 |
|-------------|------|
| A2.5 decision→belief update | 本 spec + A1 |
| A3 dreaming with decisions | A1 + A2 + tick archive replay |
| Decision filesystem watcher | 本 spec |
| Decision retention / pruning | 本 spec |
| Multi-market aggregate index | 本 spec |

## Scope 備注

**刻意窄**：本 spec 只做 observation + wake。不碰 belief update（interpretation），不碰 pruning（retention），不碰前端。Single focused delivery。

Plan next：`writing-plans` → `executing-plans`（inline；scope 小，grounding pass 後可以一口氣做完）。
