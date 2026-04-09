# Eden — Operator System Handoff

## 用一句話說清楚

Eden 是一個**市場操作系統**（不是 dashboard），目標是：在我創造的 ontology + knowledge graph 世界下，感知數據、推理出究竟發生什麼事情，然後**幫 operator 做決策**。

## 我們現在在哪：一葉知林

Eden 的終極目標是「一葉知森羅」——看到一片葉子掉了，知道它屬於哪棵樹、為什麼掉、哪些會跟、哪些不會、以及對整片森林意味著什麼。

當前進度：

| 階段 | 完成度 | 含義 |
|------|--------|------|
| **一葉知三葉** | 100% | symbol 動了 → 知道 peer 會不會動（knowledge graph + propagation） |
| **一葉知枝** | 90% | symbol 動了 → 知道 sector 整體狀態，有因果 narrative |
| **一葉知樹** | 70% | 知道 **為什麼** 動（attribution）、**該不該傳**（conditional propagation）、**沒傳代表什麼**（PropagationAbsence）、自己判斷在**惡化**（mid-flight health）、哪個**解釋更好**（competition narrative） |
| **一葉知林** | 40% | 知道歷史上這類 pattern 有沒有賺過（FamilyAlphaGate）、有初步自我修正（auto-assessment → doctrine） |
| **一葉知森羅** | 15% | 缺：attribution→template 回饋、operator 偏好學習、catalyst 自動識別、case-level 因果敘述、adaptive attention |

最大缺口不是技術，是**閉環**——Eden 能「看到」和「判斷」，但還不能真正「從結果學到教訓再改變看法」。

## 技術棧

- **後端**: Rust (364 files, ~112k lines), SQLite (persistence), Longport API (real-time HK/US market data)
- **前端**: React + TypeScript + Vite + Blueprint.js
- **雙市場**: HK（主要，broker queue + depth + trade + quote 四通道）和 US（Nasdaq Basic + cross-market）
- **二進位**: `cargo run --bin eden` 啟動 HK live runtime，`cargo run --bin eden -- us` 啟動 US runtime

## 核心架構

```
Market Data (WebSocket)
  ↓
ObjectStore (ontology: symbols, institutions, brokers, sectors, themes)
  ↓
Pipeline:
  Observations → Events → Derived Signals → Hypotheses → Tactical Setups → Tracks
  ↓                                                        ↓
  World State (backward reasoning, causal timelines)    Policy Layer (action decisions)
  ↓                                                        ↓
  Operator Work Items ← ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
  ↓
  Persistence (SQLite) + API (Axum HTTP) + Console Display
```

### 關鍵模組

| 模組 | 位置 | 職責 |
|------|------|------|
| Ontology | `src/ontology/` | 語義模型：objects, contracts, store, links, reasoning types |
| Pipeline | `src/pipeline/` | 推理引擎：signals → events → reasoning → world state |
| Graph | `src/graph/` | 知識圖譜：institutional alignment, convergence, decision |
| HK Runtime | `src/hk/runtime/` | 港股主循環：tick loop, display, persistence, snapshot |
| US Runtime | `src/us/` | 美股主循環：對稱結構，但有獨立的 signals/reasoning/policy |
| Temporal | `src/temporal/` | 時間序列：lineage metrics, multi-horizon gate |
| Persistence | `src/persistence/` | SQLite 持久化：tracks, outcomes, assessments, workflows |
| Cases | `src/cases/` | Case 管理：reasoning story, review analytics, enrichment |
| API | `src/api/` | HTTP API：ontology, agent, case, feed, lineage surfaces |
| Agent | `src/agent/` | LLM 分析代理：attention, lens, recommendations |

## 我們已經完成了什麼（完成度估計）

### 基礎設施 (80%)
- ✅ Real-time tick loop (HK + US), debounce, WebSocket subscription
- ✅ ObjectStore: 493 symbols, 526 institutions, 2725 brokers, 17 sectors
- ✅ RuntimeTask lifecycle management
- ✅ Persistence layer: tracks, outcomes, assessments, workflows
- ✅ OperatorWorkItem 作為唯一對外 canonical DTO（OperatorWorkflowSurface 已降級為內部層）
- ✅ Frontend 已收斂到 OperatorWorkItem grammar

### 感知層 (75%)
- ✅ 四通道數據: price, volume, order book (depth), broker queue
- ✅ Temporal enrichment: broker perception, iceberg detection, regime transitions
- ✅ Cross-stock presence (broker concordance)
- ✅ Convergence scores: institutional alignment + sector coherence + cross-stock correlation
- ✅ Attention budget: Deep/Standard/Scan/Skip 四層 symbol 分類

### 推理層 (60%)
- ✅ Hypothesis template system: 6 families (Directed Flow, Stress Concentration, Institution Relay, Propagation Chain, Breakout Contagion, Catalyst Repricing)
- ✅ FamilyAlphaGate: zero-follow-through 硬性淘汰 + tiered net penalty scoring
- ✅ MultiHorizonGate: 區分 cold start vs attempted-but-failed
- ✅ Event attribution: driver_kind (company_specific / sector_wide / macro_wide) + propagation_scope
- ✅ Evidence-based attribution enrichment (broker concordance + macro event alignment)
- ✅ PropagationAbsence detection: 預期傳導未發生 → 反證
- ✅ Symbol hypothesis cap (max 3 per symbol)
- ✅ Observe budget cap (max 20 observe setups)
- ✅ Observe pruning: fast-expire low-quality + TTL
- ✅ Mid-flight health check: enter/review case 證據惡化 → 自動降級
- ✅ Competition narrative: 每個 case 解釋 winner 為什麼贏了 runner_up

### 學習閉環 (35%)
- ✅ Realized outcome persistence
- ✅ Auto-assessment: realized outcome → CaseReasoningAssessmentRecord (source="outcome_auto")
- ✅ ReviewerDoctrinePressure: 聚合 reviewer feedback → 動態調整 promotion threshold
- ✅ Family-aware pressure overrides
- ✅ Family Performance Summary: 每 tick 輸出 family 級別成績單
- ⚠️ 但 doctrine 資料目前偏少，需要更多 live cycle 累積
- ❌ 尚未實作 operator preference layer（operator 審核偏好學習）

### 因果歸因 (40%)
- ✅ Event attribution (why this happened)
- ✅ Conditional propagation (should this spread)
- ✅ PropagationAbsence (who should have reacted but didn't)
- ✅ World State narrative enrichment (sector regime 有因果描述)
- ✅ Causal Memory / Causal Timelines (flip events, leader sequence)
- ❌ 尚未做到 case-level 因果敘述（為什麼這個 case 被生成）
- ❌ Attribution 尚未反饋到 hypothesis generation（知道是 company_specific 但不會因此調整 template 選擇）

## 當前 Live 運行指標

```
tick range: 30-34
hypotheses per tick: 51-71 (down from 62-186 in earlier versions)
setups: ~57 (capped, down from 60-141)
signals: ~2480
avg_tick_ms: ~4500ms
attention budget: 24 Deep / 98 Standard / 371 Scan / 0 Skip
families active: Directed Flow, Stress Concentration, Institution Relay, Propagation Chain, Breakout Contagion
Propagation Chain: completely eliminated from hypothesis generation (zero-follow-through gate)
```

## Operator 實盤觀察（2026-04-06 US session）

以下是用 Claude Code 作為 operator 實際跟 Eden 信號交易一個 session 後的觀察。這些不是理論分析，是實際下單、管理持倉、跟信號進出場的體感。

### Eden 有 edge 的地方

1. **退出紀律是最大優勢** — DDOG 和 DOW 空頭在 Eden 信號消失/反轉後平倉，避免了更大不確定性。基於微觀結構的退出比任何固定止損都精準。
2. **跨市場信號最可靠** — ZTO 的 HK→US conf 和 inst 是整個 session 最穩定的信號。結構性信息不對稱是真實 edge。
3. **VolumeSpike 是硬數據** — BKNG 的 VolumeSpike 從 14.09x 持續加速到 14.83x，和價格走勢完全一致。
4. **Pipeline 分級（observe→review→enter）是好的風控框架** — 防止在低確信度時衝動進場。
5. **Convergence 聚類捕捉是真的** — crypto 族群（MSTR/BITO/IBIT/CLSK/RIOT）的共振是真實的市場結構。

### Eden 需要改進的地方

1. **~~Lineage 評估窗口太短，gate 被噪音主導~~** ✅ 已修復（2026-04-07）
   - tick_history 從 120 → 500（中期穩定）
   - 新增 `UsLineageFamilyAccumulator` 累積長期 family 統計，不受窗口輪轉影響
   - 改了：`src/us/temporal/lineage.rs` + `src/us/runtime/startup.rs` + `src/us/runtime.rs`

2. **~~Catalyst Repricing 信噪比太低~~** ✅ 已修復（2026-04-07）
   - 根據 event driver 自動拆分為 3 個子 family：`catalyst_repricing_company`、`catalyst_repricing_sector`、`catalyst_repricing_macro`
   - 每個子 family 有獨立的 lineage 追蹤、alpha_boost、gate 判斷
   - 改了：`src/us/pipeline/reasoning/synthesis.rs`（新增 `refine_catalyst_family()`）

3. **~~正反饋閉環不存在~~** ✅ 已修復（2026-04-07）
   - `compute_us_alpha_boost` / `compute_alpha_boost`：門檻從 resolved≥15 降到 resolved≥1（80%+ hit_rate 時）
   - Boost 範圍從 0.2~1.0 擴大到 0.3~1.5，乘數從 2~3% 加大到 4~5%
   - Elite family（100+ resolved, 60%+ hit_rate）可降低 threshold 最多 ~7.5%
   - 改了：`src/us/pipeline/reasoning/policy.rs` + `src/pipeline/reasoning/policy.rs`（HK+US 對稱）

4. **~~Scorecard 一直 0/0，學習閉環斷了~~** ✅ 已修復（2026-04-07）
   - 根因：每 tick ~400 個 symbol 產生 signal_record，10 ticks 爆 4000 cap，resolved 的記錄被立刻 prune，scorecard 永遠 0
   - 修法：新增 `UsSignalScorecardAccumulator`，resolve 時立刻累積到獨立計數器，不依賴 buffer 存活
   - 改了：`src/us/graph/decision/scorecard.rs` + `src/us/runtime/startup.rs` + `src/us/runtime.rs`

5. **~~review_gate 不過濾無 lineage 的 family~~** ✅ 已修復（2026-04-07）
   - `classify_us_lineage_prior()` 和 `classify_lineage_prior()` 中 resolved=0 → Negative
   - Negative signal 的 family 被 policy 自動擋在 review/enter 外
   - 改了：`src/us/pipeline/reasoning/policy.rs` + `src/pipeline/reasoning/policy.rs`（HK+US 對稱）

6. **~~Convergence 分數 vs Convergence Hypothesis 的斷裂~~** ✅ 已修復（2026-04-07）
   - Vortex channel_diversity 門檻從 3 降到 2（2 channel 時要求 strength > 0.55）
   - 解決了真實 convergence 信號（如 volume + structure）因 channel 數不足被擋的問題
   - 改了：`src/us/pipeline/reasoning/synthesis.rs`（`derive_convergence_hypothesis` 門檻邏輯）

### 優化優先級（從實盤體感排序）

| 優先級 | 問題 | 為什麼重要 |
|--------|------|-----------|
| 優先級 | 問題 | 為什麼重要 | 狀態 |
|--------|------|-----------|------|
| 優先級 | 問題 | 為什麼重要 | 狀態 |
|--------|------|-----------|------|
| ~~P0~~ | ~~Scorecard 0/0~~ | ~~沒有閉環~~ | ✅ 已修復 |
| ~~P0~~ | ~~Lineage 評估窗口太短~~ | ~~Gate 被噪音主導~~ | ✅ 已修復 |
| ~~P1~~ | ~~review_gate 過濾無 lineage family~~ | ~~減少 operator 認知負荷~~ | ✅ 已修復 |
| ~~P0~~ | ~~正反饋閉環~~ | ~~好 family 得不到放大~~ | ✅ 已修復 |
| ~~P1~~ | ~~Catalyst Repricing 拆分子類型~~ | ~~最大噪音源~~ | ✅ 已修復 |
| ~~P2~~ | ~~Convergence → Hypothesis 轉化~~ | ~~真實信號被稀釋~~ | ✅ 已修復 |
| **P0** | 信號二階導數（Palantir 方式） | 知道「風還在吹」但不知道「風在變弱」，BKNG +$450 未止盈 | ✅ 基礎設施已建（2026-04-07） |

### 信號動量追蹤（2026-04-07 實盤教訓）

**問題**：BKNG 昨天浮盈 +$450，今天平倉只拿了 +$67。Eden 只追蹤信號的「有沒有」和「多大」，不追蹤信號的「變化率」和「加速度」。VolumeSpike 14→15→16→17→17→17 的加速度從正轉零就是見頂信號，但 Eden 看不到。

**解法（Palantir 方式）**：不加止盈線，而是追蹤信號的二階導數。
- `SignalMomentumTracker`：per-symbol 追蹤 convergence 和 VolumeSpike 的歷史值
- `velocity()`：一階導數，信號在變強還是變弱
- `acceleration()`：二階導數，變化在加速還是減速
- `is_peaking()`：值為正但加速度為負 = 見頂
- `is_collapsing()`：速度和加速度都為負 = 崩潰
- `SignalHealth` enum：Healthy → Weakening → Peaking → Collapsing

**已完成**：
- `src/us/temporal/lineage.rs`：新增 `SignalMomentumTracker` + `SignalMomentumEntry` + `SignalHealth`
- `src/us/runtime/startup.rs`：初始化 tracker
- `src/us/runtime.rs`：每 tick 餵入 convergence 數據

**待完成**：
- 在 policy 層使用 `signal_health()` 收緊 peaking 信號的 review/enter 門檻
- 追蹤 VolumeSpike 的 ratio 變化（需要從 events 中提取）
- 在 console display 中顯示信號動量狀態

## 還差什麼（按優先級排序）

### P0: 從「知林」到「知森」的關鍵跨越

1. **Attribution → Template Selection 回饋**（知道風向就該決定往哪看）
   - 現在 attribution 只用在 propagation gating 和 World State narrative
   - 應該：如果 event attribution = company_specific，就不要生成 sector propagation hypothesis
   - 這一步做到，Eden 就不只是「知道風從哪來」，而是「會根據風向決定往哪看」
   - 位置：`src/pipeline/reasoning/support.rs` 的 `template_applicable` + `src/us/pipeline/reasoning/support.rs`

2. **Case-level Reasoning Narrative**（看到果實要說得出它從哪朵花來）
   - 每個 TacticalSetup 應該有一句人話總結「為什麼這個 case 存在」
   - 不是 entry_rationale（那是 policy 層面），而是因果層面
   - 位置：`src/pipeline/reasoning/synthesis.rs` 的 `derive_hypotheses`

3. **Doctrine 資料豐富度**（學習閉環要轉起來）
   - auto-assessment 已接好，但需要更多 live cycle 累積
   - 可以考慮 backfill 歷史 realized outcomes 作為 seed
   - 位置：`src/persistence/case_reasoning_assessment.rs`

### P1: 從「知森」到「知森羅」的深度

4. **Operator Preference Layer**（知道森林但也要知道誰在看）
   - 最簡單的版本：記錄 operator 點開了哪些 case、忽略了哪些
   - 用 click-through rate per family 作為 preference signal
   - 調整 promotion threshold（類似 ReviewerDoctrinePressure 但基於行為）
   - 位置：需要前端 + API + 新的 persistence table

5. **Adaptive Attention Rebalancing**（看到好樹要會走近）
   - 現在 attention budget 是靜態的（Deep/Standard/Scan/Skip 基於 activity）
   - 應該：如果某個 symbol 連續產生好的 case，升級它的 attention tier
   - 位置：`src/pipeline/attention_budget.rs`

6. **Catalyst / Narrative Sensing 深化**（知道樹在搖但也要知道是什麼風）
   - 現在只有 AgentMacroEvent → CatalystActivation 的基礎投射
   - 缺少：自動識別題材（目前依賴 LLM agent 提供 macro events）
   - 長期：需要 news/social 數據源

### P2: 感知與整合

7. **BrokerBehaviorProfile**
   - 識別 execution algo / liquidity provider / directional desk
   - 已有 broker queue 數據和 cross-stock presence
   - 缺少：profile 建構邏輯和 classification
   - 位置：`src/ontology/links/broker.rs` 需要擴展

8. **Operator Shell**
   - 類似 Claude Code 的 command/tool/task 模式
   - 讓 operator 用自然語言查詢和操作 Eden
   - `src/operator_commands.rs` 已有骨架
   - 位置：`src/cli/` + `src/operator_commands.rs`

9. **US Pipeline 對稱性**
   - US 大部分邏輯已對稱實作，但少數功能（如 broker concordance）只有 HK 有
   - 需要逐步補齊

## 開發約定

- **用中文溝通**，code 和 comments 用英文
- **優化現有代碼**，不要創建新程式。傾向於修改而非新建模組
- **SOTA 方式解決問題**，不做最小 MVP
- 所有改動要 `cargo check --lib -q` 通過
- 重要改動要寫 `#[test]` 並通過
- HK 和 US 保持對稱：HK 做的改動通常也需要在 US 做
- Policy layer 的閾值調整要小心，避免過擬合
- 不要刪除已有的兼容性欄位（`object_ref`, `case_ref`, `workflow_ref`），它們在 migration 期間需要保留

## 關鍵文件速查

| 要改什麼 | 看哪裡 |
|----------|--------|
| Hypothesis 生成邏輯 | `src/pipeline/reasoning/synthesis.rs` |
| Template 適用性 / 極性 | `src/pipeline/reasoning/support.rs` |
| Action policy (promote/observe/enter) | `src/pipeline/reasoning/policy.rs` |
| HK 主循環 | `src/hk/runtime.rs` |
| US 主循環 | `src/us/runtime.rs` |
| Event detection | `src/pipeline/signals/events.rs` |
| World State 推導 | `src/pipeline/world.rs` |
| Console display | `src/hk/runtime/display/console/reasoning.rs` |
| Reasoning types | `src/ontology/reasoning.rs` |
| Contract DTOs | `src/ontology/contracts/types.rs` |
| Persistence | `src/persistence/store.rs` |
| 已有 reasoning tests | `src/pipeline/reasoning/tests.rs` |
| US reasoning tests | `src/us/pipeline/reasoning_tests.rs` |

## 核心哲學：拓撲湧現

Eden 的本質不是一個「信號匹配器」，而是一個**拓撲觀察者**。

核心比喻：ontology + knowledge graph 構成地形，數據是水。水在地形中流動、碰撞、匯聚，自然形成漩渦。漩渦就是資訊——不是人預定義的 pattern，而是數據在拓撲結構中互相作用後湧現的。

| | 傳統量化 | 現在的 Eden | 終極的 Eden |
|--|---------|-----------|------------|
| 思路來源 | 人寫因子 | 6 個固定 template | 圖拓撲 + 數據流動自然湧現 |
| 發現能力 | 只能找已知的 | 只能匹配已定義的 | 能觀察到未預期的 |
| Edge 演化 | 衰減 | 衰減較慢（殺壞的） | 自我生長（好的放大，新的湧現） |
| 上限 | 人的想像力 | template 的覆蓋度 | ontology 拓撲的豐富度 |

**質變的關鍵**：把 hypothesis generation 從 template-matching 反轉為 convergence-detection。不是問「這個事件符合哪個 template」，而是讓 event 在圖裡傳播，觀察哪裡有多條獨立路徑匯聚——那就是漩渦，那就是 hypothesis。

現有的 6 個 template 不用刪，它們是人類先驗知識注入的初始漩渦形狀（cold start）。但系統不再被限制在這 6 個形狀裡。

學習閉環必須雙向：
- **負反饋**（已有）：outcome 差 → 殺掉 family / 收緊門檻
- **正反饋**（需要建）：outcome 好 → 放大 family / 降低門檻 / 記住漩渦形狀
- **湧現**（終極目標）：新的漩渦形狀不屬於任何現有 template → 自動提取為新 pattern

## 一葉知森羅

最終目標：看到一片葉子掉了，知道它屬於哪棵樹、為什麼掉、哪些其他葉子會跟著掉、哪些不會、以及這對整片森林意味著什麼。

當前位置：一葉知林。差的不是更多 signal，是**正反饋閉環**和**拓撲湧現**。

衡量質變的兩個指標：
1. **operator 點開率**：operator 每天真正點開的 case 數 / 系統產生的 case 數。上升 = Eden 學會少說廢話
2. **template 外 hypothesis 佔比**：不屬於任何固定 template 的成功 hypothesis 數量。> 0 = 湧現開始了
