# Eden — Operator System Handoff

## 用一句話說清楚

Eden 是一個**市場操作系統**（不是 dashboard），目標是：在我創造的 ontology + knowledge graph 世界下，感知數據、推理出究竟發生什麼事情，然後**幫 operator 做決策**。

## 我們現在在哪：重建中

Eden 的終極目標是「一葉知森羅」——看到一片葉子掉了，知道它屬於哪棵樹、為什麼掉、哪些會跟、哪些不會、以及對整片森林意味著什麼。

### 2026-04-10 架構重設計

舊的 template-based hypothesis system（6 families、FamilyAlphaGate、doctrine pressure 等）已刪除（-17,000 行）。
替換為 **壓力場引擎 + 拓撲推理**：

| 層 | 狀態 | 描述 |
|---|------|------|
| **感知：壓力場** | ✅ 完成 | 多時間尺度（tick/minute/hour/day），multi-channel 壓力計算，graph-based propagation，baseline + anomaly detection |
| **推理：Attribution** | 🔄 建設中 | 從壓力場 channel 結構讀「為什麼有 tension」— trade flow / capital flow / institutional / microstructure / broad structural |
| **推理：Absence** | 🔄 建設中 | 從鄰居壓力場讀「同 sector peers 有沒有反應」→ isolated = 個股事件 |
| **推理：Competition** | 🔄 建設中 | 從 channel 數量 + 是否孤立 → 哪個解釋更可信 |
| **推理：Lifecycle** | 🔄 建設中 | 追蹤 tension 的 velocity/acceleration → Growing / Peaking / Fading |
| **學習：Edge Learning** | ✅ 基礎完成 | vortex outcome → credit/debit graph edges → topology adapts |

### 設計哲學

水在地形（ontology + knowledge graph）中流動，漩渦自然湧現。壓力場是眼睛，推理層是腦子 — 兩個都需要。

**不再使用 template 匹配。** Attribution、Absence、Competition 從壓力場的拓撲結構中直接讀取，不需要預定義 pattern。

**關鍵教訓（2026-04-10）：** 砍掉推理層只留壓力場 = 13% hit rate。壓力場能看到異常但不理解異常。需要推理層在壓力場之上做「為什麼」「會不會傳」「哪個解釋更好」的判斷。

## 技術棧

- **後端**: Rust (364 files, ~112k lines), SQLite (persistence), Longport API (real-time HK/US market data)
- **前端**: React + TypeScript + Vite + Blueprint.js
- **雙市場**: HK（主要，broker queue + depth + trade + quote 四通道）和 US（Nasdaq Basic + cross-market）
- **二進位**: `cargo run --bin eden` 啟動 HK live runtime，`cargo run --bin eden -- us` 啟動 US runtime

## 核心架構

```
Market Data (WebSocket + REST)
  ↓
ObjectStore (ontology: symbols, institutions, brokers, sectors)
  ↓
Dimensions (per-symbol multi-channel feature vectors)
  ↓
Pressure Field (multi-scale: tick/minute/hour/day)
  ↓ propagation along graph edges
Vortex Detection (tension = where time scales disagree)
  ↓
Reasoning Layer (on top of pressure field):
  Attribution — WHY is there tension? (which channels drive it)
  Absence — WHO should be reacting but isn't? (neighbor comparison)
  Competition — WHICH explanation is more credible? (channel count)
  Lifecycle — IS the anomaly growing, peaking, or dying? (acceleration)
  ↓
VortexInsight → Tactical Setups → Operator / Claude Code
  ↓
Edge Learning (outcome → update graph weights → better next time)
```

### 關鍵模組

| 模組 | 位置 | 職責 |
|------|------|------|
| Ontology | `src/ontology/` | 語義模型：objects, contracts, store, links, reasoning types |
| Pressure Field | `src/pipeline/pressure.rs` | 壓力場引擎：multi-scale pressure, propagation, vortex detection |
| Pressure Reasoning | `src/pipeline/pressure/reasoning.rs` | 拓撲推理：attribution, absence, competition, lifecycle |
| Pressure Bridge | `src/pipeline/pressure/bridge.rs` | Vortex → TacticalSetup 轉換 |
| Graph | `src/graph/` | 知識圖譜：BrainGraph (HK), UsGraph (US), edge learning |
| Pipeline | `src/pipeline/` | 信號處理：events, dimensions, world state |
| HK Runtime | `src/hk/runtime/` | 港股主循環：tick loop, pressure field, display |
| US Runtime | `src/us/` | 美股主循環：對稱結構，共用壓力場引擎 |
| Temporal | `src/temporal/` | 時間序列：lineage metrics |
| Persistence | `src/persistence/` | SQLite 持久化 |
| API | `src/api/` | HTTP API |
| Agent | `src/agent/` | LLM 分析代理 |

## 完成度

### 基礎設施 (90%)
- ✅ Real-time tick loop (HK + US), debounce, WebSocket subscription
- ✅ ObjectStore: 494 HK symbols, 640 US symbols, 526 institutions, 2725 brokers
- ✅ Optimized REST data acquisition (batch first, per-symbol top 20-40 only)
- ✅ Persistence layer, API, Frontend (React + Blueprint.js)
- ✅ Memory safety: eviction policies, hard caps, query limits
- ✅ Security: API auth, SSE connection limits

### 壓力場感知層 (70%)
- ✅ 6 pressure channels: OrderBook, CapitalFlow, Institutional, Momentum, Volume, Structure
- ✅ Multi-scale accumulation: Tick/Minute/Hour/Day with different decay rates
- ✅ Graph-based propagation along BrainGraph (HK) and UsGraph (US) edges
- ✅ Baseline tracking (slow EMA) + anomaly detection (deviation from baseline)
- ✅ Tension-based vortex detection (tick vs hour divergence)
- ✅ Edge weight learning from vortex outcomes

### 拓撲推理層 (30% — 建設中)
- 🔄 Attribution: 從 channel 結構讀「為什麼有 tension」
- 🔄 Absence: 從鄰居壓力場讀「該傳沒傳」
- 🔄 Competition: 哪個解釋更可信
- 🔄 Lifecycle: tension 的 velocity/acceleration → Growing/Peaking/Fading
- ❌ 尚未接入 runtime tick loop

### 學習閉環 (20%)
- ✅ EdgeLearningLedger: vortex outcome → credit/debit edges
- ✅ Realized outcome persistence
- ❌ 需要更長時間運行來驗證 edge learning 效果
- ❌ 尚未實作 operator preference learning

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
| 壓力場引擎 | `src/pipeline/pressure.rs` |
| 拓撲推理（attribution/absence/competition/lifecycle） | `src/pipeline/pressure/reasoning.rs` |
| Vortex → TacticalSetup 轉換 | `src/pipeline/pressure/bridge.rs` |
| HK 主循環 | `src/hk/runtime.rs` |
| US 主循環 | `src/us/runtime.rs` |
| Event detection | `src/pipeline/signals/events.rs` |
| HK Dimensions | `src/pipeline/dimensions.rs` |
| US Dimensions | `src/us/pipeline/dimensions.rs` |
| Graph（HK） | `src/graph/graph.rs` |
| Graph（US） | `src/us/graph/graph.rs` |
| Edge Learning | `src/graph/edge_learning.rs` |
| Reasoning types（data structs） | `src/ontology/reasoning.rs` |
| Persistence | `src/persistence/store.rs` |

## 核心哲學：拓撲湧現

Eden 的本質不是一個「信號匹配器」，而是一個**拓撲觀察者**。

核心比喻：ontology + knowledge graph 構成地形，數據是水。水在地形中流動、碰撞、匯聚，自然形成漩渦。漩渦就是資訊——不是人預定義的 pattern，而是數據在拓撲結構中互相作用後湧現的。

| | 傳統量化 | 舊 Eden（已刪除） | 新 Eden（壓力場 + 推理） |
|--|---------|-----------|------------|
| 感知 | 價格/量指標 | 6 個固定 template 匹配 | 壓力場：多時間尺度 × 多通道 × 圖譜傳播 |
| 推理 | 統計模型 | 硬編碼 if-else 規則 | 從壓力場拓撲結構直接讀取 attribution/absence/competition |
| 學習 | Backtest + deploy | FamilyAlphaGate（需要 15 resolved） | Edge weight 從每個 vortex outcome 即時更新 |
| Edge 來源 | 歷史 pattern（會衰退） | Template 覆蓋度（固定） | 當下拓撲異常（每天不同，不衰退） |

**已驗證的 edge（實盤）：**
- BKNG: VolumeSpike 15x + Convergence Hypothesis → Claude Code 進場 → +$450 浮盈
- MRVL: 異常偵測正確 → 後漲 10%
- 兩筆交易都確認：**entry 有 edge，exit timing 是缺口**

**關鍵教訓：**
- 壓力場是好的眼睛，但不能取代腦子（推理層）
- 推理的 CONCEPTS（attribution, absence, competition）是正確的
- 推理的 IMPLEMENTATION（templates, if-else）是錯的
- 正確做法：保留概念，用壓力場的結構作為推理的輸入

## 一葉知森羅

最終目標：看到一片葉子掉了，知道它屬於哪棵樹、為什麼掉、哪些其他葉子會跟著掉、哪些不會、以及這對整片森林意味著什麼。

實現路徑：壓力場（感知）→ 推理（理解）→ 生命週期追蹤（時機）

衡量質變的指標：
1. **異常偵測命中率**：壓力場標記的節點，之後 1-2 小時內是否出現超常波動
2. **生命週期信號質量**：Growing/Peaking/Fading 的判斷是否與實際利潤 capture 相關
3. **推理解釋力**：Attribution + Absence 的敘述是否幫助 Claude Code 做出更好的判斷
