# Decision Log — Eden ↔ Claude Code 閉環的第一塊磚

**Date**: 2026-04-18
**Author**: Claude Code (with operator)
**Status**: Design for review

## 為什麼做

Eden 的 Y 化目標需要一個 bidirectional loop：

```
Longport raw → Eden(感知腦) → wake text → Claude Code(推理腦) → trade
                  ↑                                              ↓
                  └──── decisions/ (本 spec 要做的) ─────────────┘
```

現況：Eden 每 tick emit wake。Claude Code 讀、推理、下單。這條鏈的**回饋路徑是空的** — Eden 不知道：
- 哪些 wake line 真的驅動了決策
- 哪些 wake line 被 Claude Code 覺得是噪音跳過
- 決策背後的 reasoning chain 是什麼
- 事後 retrospective 如何

沒這個 log，Eden 永遠在對空無輸出，學不到「怎麼溝通對 Claude Code 有效」。

現在紀錄存在 `docs/operator_session_*.md`，但是敘事 markdown，**機器不可解析**，Eden 無法 ingest。

## 本 spec 的 scope（嚴格 bounded）

**In**：
- 定義 structured JSON schema for decisions
- 定義檔案儲存佈局
- 回補（backfill）現有 `operator_session_*.md` 裡能抽取的決策
- 定義 Eden 未來 ingestor 會讀取的介面（schema contract）

**Out（刻意不做）**：
- Eden 端 ingestor（屬於下一個 spec — Belief 持久化 + Dreaming 那包）
- UI 或 API（Claude Code 直接寫 JSON 檔即可）
- 自動化決策記錄（這個 spec 完成後，Claude Code 每次下單手動寫一個 JSON）
- Trade execution 整合（Longport 下單 API）

## Schema

每個決策是一個 JSON 檔。Schema v1：

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-18T09-31-47Z-HK-0700.HK-entry",
  "timestamp": "2026-04-18T09:31:47Z",
  "market": "HK",
  "symbol": "0700.HK",
  "action": "entry",
  "direction": "long",
  "eden_session": {
    "binary": "hk",
    "tick_seq": 12,
    "stress_composite": 0.48,
    "wake_excerpt": "inference: 0700.HK broad institutional deployment conf=0.72",
    "wake_context": [
      "inference: 0700.HK broad institutional deployment conf=0.72",
      "institution rotation: BOCI +0.21, JPM +0.18",
      "hidden forces confirmed: 47 symbols"
    ],
    "supporting_evidence": ["BOCI rotation", "broker 157/26 buy skew"],
    "opposing_evidence": ["peer 3690.HK no sync move"],
    "missing_evidence": ["no matching option flow HK-side"]
  },
  "claude": {
    "reasoning": "T22 chain at 0.72 + institutional rotation confirms buy side. Broker queue 157/26 is raw evidence of distribution → accumulation flip.",
    "concerns": ["3690.HK lag suggests single-name story not sector"],
    "confidence": 0.78,
    "prior_knowledge_used": ["HK edge in raw_microstructure (CLAUDE.md)", "T22 conf>0.7 historically 65%+ hit"],
    "alternatives_considered": ["wait for 3690.HK confirmation", "size half"],
    "decision_rationale": "T22 信號 + 機構輪動雙驗 > 單股疑慮，size 減半作 concession"
  },
  "execution": {
    "price": "342.80",
    "size_bps": 25,
    "size_notional_hkd": 25000,
    "linked_entry_id": null,
    "broker_order_id": null,
    "paper_or_real": "paper"
  },
  "outcome": null,
  "retrospective": null,
  "metadata": {
    "backfilled": false,
    "backfill_source": null,
    "created_at": "2026-04-18T09:31:47Z",
    "updated_at": "2026-04-18T09:31:47Z"
  }
}
```

### Schema 欄位解釋

| Section | Purpose |
|---------|---------|
| `eden_session` | Eden 當時 emit 了什麼 — 這是 Claude Code 看到的**輸入** |
| `claude` | Claude Code 怎麼推理 — 這是**處理過程**，未來 Eden 學習的 label |
| `execution` | 實際下單參數 — 這是**動作** |
| `outcome` | 事後（退出時填） — 這是**結果** |
| `retrospective` | Claude Code 事後反思（退出後） — 這是**ground truth for perception learning** |

### Schema — outcome 和 retrospective

退出時 append 到同一個 decision_id 或寫新檔。建議**寫新 exit decision**，透過 `linked_entry_id` 串聯。Exit 的 outcome 欄位：

```json
"outcome": {
  "exit_timestamp": "2026-04-18T10:15:33Z",
  "exit_price": "345.60",
  "hold_duration_sec": 2626,
  "pnl_bps": 82,
  "pnl_abs_hkd": 2050,
  "closing_reason": "eden_signal_faded"
}
```

### Schema — retrospective

```json
"retrospective": {
  "what_worked": "T22 信號方向正確，broker queue dominance 是有效的 early read",
  "what_didnt": "沒等 3690.HK 確認是對的，但 entry timing 早了 5 分鐘",
  "would_do_differently": "T22 first fire 可以等一個 tick 的 persistence 再進",
  "new_pattern_observed": "BOCI rotation 領先 JPM rotation ~3 ticks，之前沒注意到",
  "eden_gap": "missing_evidence 裡沒標示『peer synchronization』也算 opposing — Eden 該提升這類缺失維度的 surface 權重"
}
```

`retrospective.eden_gap` 是最重要的欄位 — 這是 Claude Code 告訴 Eden「你漏了什麼」的 first-class signal。

## 儲存佈局

```
decisions/
├── 2026/
│   └── 04/
│       └── 18/
│           ├── 093147Z-HK-0700.HK-entry.json
│           ├── 101533Z-HK-0700.HK-exit.json
│           └── index.jsonl          # 每日所有 decision 的 flat index
├── schemas/
│   └── v1.json                       # JSON Schema validator
└── README.md                         # 給 Claude Code 看的 usage guide
```

- **為什麼用檔案 + 目錄**：
  - 可讀性（grep-able、human-inspectable）
  - 無資料庫依賴（Eden ingestor 以後再接 SQLite）
  - 版本控制友善（可以選擇性 commit，或 gitignore）
  - 平行寫入無衝突（每個 decision 獨立檔）

- **為什麼額外有 `index.jsonl`**：快速日 scan 用，不用 glob 全目錄

- **schemas/v1.json**：JSON Schema validator，未來 Eden ingestor 可以驗證

## Backfill 計畫

既有 `docs/operator_session_*.md` 有以下可抽取決策（初估）：

| 檔案 | 推測 decision 數 |
|------|---|
| `operator_session_2026-04-14.md` | ~5-8 |
| `operator_session_hk_2026-04-15.md` | ~3-5 |
| `operator_session_us_2026-04-15_v2.md` | ~15+ (KC/HUBS/others) |
| `operator_session_hk_2026-04-16.md` | ~3-5 |
| `operator_session_us_2026-04-15_postfix.md` | ~5-10 |
| `session_2026-04-17_overnight.md` | ~varies |

Backfill 原則：
- `metadata.backfilled = true`
- `metadata.backfill_source = "docs/operator_session_hk_2026-04-15.md"`
- 缺失欄位填 `null`（不要瞎猜）
- `claude.confidence` 只有原文明確寫了才填；否則 null
- `retrospective` 只有原文有明確反思段落才填

Backfill 不是這個 spec 本身要實作的 — **用 Claude Code 逐檔讀、逐決策抽**。可以批次但要人工核對。

## Eden ingestor hook（描述，不實作）

下一個 spec（2026-04-19-belief-persistence-design.md）會加 Eden 端：

```rust
// src/ingestor/decision_log.rs (future)
pub struct DecisionLogIngestor {
    root: PathBuf,  // e.g. "decisions/"
    last_ingested: DateTime<Utc>,
}

impl DecisionLogIngestor {
    pub fn new_since(root: PathBuf, since: DateTime<Utc>) -> Self { ... }
    pub fn ingest_pending(&mut self, belief_field: &mut PressureBeliefField) { ... }
    pub fn scan_daily(&self, date: NaiveDate) -> Vec<DecisionRecord> { ... }
}
```

Contract（這個 spec 要保證）：
- decision_id 是 unique + sortable
- timestamp 是 ISO8601 + Z (UTC)
- schema_version 明示（未來 breaking change 改版本）
- eden_session.tick_seq 可以跟 tick archive 對齊

## 驗證

Spec 完成的 acceptance criteria：

1. ✅ JSON schema `decisions/schemas/v1.json` 存在且 validate 通過樣本
2. ✅ `decisions/README.md` 寫清楚 Claude Code 怎麼用
3. ✅ 至少一個樣本 entry decision + 一個樣本 exit decision 展示 full lifecycle
4. ✅ 至少從一份 operator_session_*.md 成功 backfill ≥ 3 個 decisions 當作 proof of concept
5. ✅ `decisions/index.jsonl` auto-generated from per-day files

## 明確 out of scope

- **不**自動抓取 Eden wake → 手動貼進 `eden_session.wake_excerpt`
- **不**整合 Longport trading API
- **不**做前端 UI
- **不**做 Eden 端 ingestor
- **不**做 batch validation runner
- **不**實作 paper trading simulator（`execution.paper_or_real` 欄位只是佔位）

## 已決定（原 Open Questions）

1. **Retrospective timing — 兩段式**
   - **退出即時**：`retrospective` 欄位在 exit decision JSON 裡填（hot reflection — Eden gap 當下最敏銳）
   - **收盤批次**：另一份 `decisions/YYYY/MM/DD/session-recap.json`，把當日所有 decisions cross-reference（哪些主題重複出現？哪些 retrospective.eden_gap 是同一個 pattern？）
   - 兩個都是 structured JSON，schema 分離但互相引用

2. **Skip decisions — 要記**
   - `action: "skip"`，`execution` 整段變 null
   - `claude.reasoning` 必填（解釋為何沒交易）
   - `claude.confidence` 仍填（「我有 40% 覺得該做，但 60% 覺得該跳 → 跳」）
   - 這對 Eden 學「哪條 wake 沒價值」至少跟 entry 一樣重要

3. **Confidence calibration — v1 不做**
   - 記 Claude Code 主觀 confidence
   - 未來累積足夠 (decisions, outcomes) 之後 fit isotonic regression 做 post-hoc calibration
   - v2 spec 再處理

4. **Git 版本控制 — commit 全部**
   - `decisions/` 整棵 commit
   - 理由：推理歷史有版本化價值（未來 blame 特定決策邏輯的演化）
   - `broker_order_id`：現在只有 paper trading，不敏感。未來有 real trading 時改為 redacted-at-write（寫入時就存 hash，不存 raw ID）。v1 直接存 null 即可，paper 沒有真 order ID
   - 不另做 `.gitignore` 例外

5. **Paper vs real enum — 保持開放**
   - v1 只接受 `paper`
   - 未來擴增：`real` / `simulated` / `backtest`
   - JSON schema 用 `enum` 而非 fixed 字串，方便向後相容

## Scope 備注

這個 spec 刻意**極小**。它的價值不是自己解決問題，是：
- 建立一個**結構化 contract**，未來 Belief 持久化 + Dreaming + Eden ingestor 有東西 ingest
- 讓 Claude Code **現在就開始積累 training data**，Eden 端以後再接管
- 零 Eden 改動（不動 Rust code）

如果這個 spec 通過，下一步是：
1. 實作（寫 schema + README + backfill 3 個樣本）
2. 下一個 spec：`2026-04-19-belief-persistence-design.md` — 真的 Eden 側的 Belief 持久化 + ingestor
