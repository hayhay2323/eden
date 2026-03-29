# Ontology as Runtime: ObjectStore 從靜態配置進化為活的世界模型

## 問題

Eden 的 Knowledge Graph 是「只寫不讀」的系統。每 tick 將知識寫入 SurrealDB（macro events, links, nodes, events），但 tick loop 的推理引擎從未讀取已持久化的知識。所有跨 tick 記憶僅靠記憶體中的 `TickHistory` 環形緩衝。

具體斷層：
- `BrainGraph::compute(narrative, dimensions, links, store)` — `store` 只含靜態配置（股票列表、板塊定義），不含累積知識
- `infer_mechanisms_with_factor_adjustments(states, rules, &HashMap::new())` — 第三個參數永遠是空 HashMap，learning loop 產生的校準權重沒有進入推理
- `ConvergenceScore::compute()` — 不參考歷史機構行為
- SurrealDB 的 14 個讀取方法只被 API 層使用（給前端看），tick loop 內部從未呼叫

系統建了水庫（SurrealDB 知識圖譜）和水管（14 個查詢方法），但沒有接到田裡（推理引擎）。

## 設計原則

遵循 Palantir Foundry 的核心哲學：

1. **Ontology 是唯一真相** — 不存在「計算圖」和「知識圖」的分離。推理引擎讀的和寫的是同一個世界模型
2. **不加新抽象層** — ObjectStore 已經是世界模型，只是以前只有靜態半邊。補上動態半邊，不引入 KnowledgeSpine 或 WorldModel wrapper
3. **Write-through，不是 cache** — 知識在產生的瞬間就活在記憶體裡，下一個 tick 立刻可用。不是「定期從 DB 拉」
4. **啟動時恢復，運行中累積** — SurrealDB 是 durability backend，不是知識的來源

## 架構改動

### 1. 擴展 ObjectStore

```rust
// src/ontology/store/object_store.rs

pub struct ObjectStore {
    // ── 靜態實體（啟動時從 Longport API 載入，運行中不變）──
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,

    // ── 累積知識（每 tick write-through 更新）──
    pub knowledge: AccumulatedKnowledge,
}
```

### 2. AccumulatedKnowledge 結構

```rust
// src/ontology/store/knowledge.rs (新檔案)

pub struct AccumulatedKnowledge {
    /// 機構歷史行為模式
    /// key: (InstitutionId, Symbol)
    /// 每 tick 從 BrainGraph 的 InstitutionToStock 邊沉澱
    pub institutional_memory: HashMap<(InstitutionId, Symbol), InstitutionSymbolProfile>,

    /// 機制歷史表現（按 mechanism_kind × regime 切片）
    /// 從 lineage outcomes 和 case_realized_outcome 沉澱
    pub mechanism_priors: HashMap<String, MechanismPrior>,

    /// Learning loop 校準的全系統權重
    /// 從 ReasoningLearningFeedback 沉澱
    pub calibrated_weights: CalibratedWeights,
}

pub struct InstitutionSymbolProfile {
    /// 機構在這支股票上的歷史操作次數
    pub observation_count: u32,
    /// 方向一致次數（機構方向與後續價格方向一致）
    pub directional_hit_count: u32,
    /// 歷史平均持續 tick 數
    pub avg_presence_ticks: Decimal,
    /// 最近一次操作的 tick 號
    pub last_seen_tick: u64,
    /// 歷史方向偏向（正=傾向買，負=傾向賣）
    pub directional_bias: Decimal,
}

pub struct MechanismPrior {
    /// 歷史命中率（follow_through_rate）
    pub hit_rate: Decimal,
    /// 樣本數
    pub sample_count: u32,
    /// 按 regime 切片的命中率
    pub regime_hit_rates: HashMap<String, Decimal>,
    /// 平均淨回報
    pub mean_net_return: Decimal,
}

pub struct CalibratedWeights {
    /// mechanism factor adjustments（直接從 ReasoningLearningFeedback 拿）
    pub factor_adjustments: HashMap<(String, String), Decimal>,
    /// predicate adjustments
    pub predicate_adjustments: HashMap<String, Decimal>,
    /// conditioned adjustments (mechanism × scope × conditioned_on)
    pub conditioned_adjustments: Vec<ConditionedLearningAdjustment>,
}
```

### 3. accumulate — 每 tick 寫入知識

```rust
impl AccumulatedKnowledge {
    /// 從當前 tick 的計算結果中沉澱知識。
    /// 在 tick loop 末尾、history.push(tick_record) 之後呼叫。
    pub fn accumulate(
        &mut self,
        tick_number: u64,
        brain: &BrainGraph,
        scorecard: &SignalScorecard,
    ) {
        // 1. 從 BrainGraph 的 InstitutionToStock 邊沉澱機構記憶
        self.accumulate_institutional_memory(tick_number, brain);

        // 2. 從 scorecard 的 hit/miss 更新機制先驗
        self.accumulate_mechanism_priors(scorecard);
    }

    /// 從 learning feedback 更新校準權重。
    /// 在 learning feedback 刷新時呼叫（每 N tick）。
    pub fn apply_calibration(&mut self, feedback: &ReasoningLearningFeedback) {
        self.calibrated_weights.factor_adjustments = feedback.mechanism_factor_lookup();
        self.calibrated_weights.predicate_adjustments = feedback
            .predicate_adjustments
            .iter()
            .map(|adj| (adj.label.clone(), adj.delta))
            .collect();
        self.calibrated_weights.conditioned_adjustments =
            feedback.conditioned_adjustments.clone();
    }
}
```

### 4. 消費者改動

#### 4a. mechanism_inference — 接上校準權重（一行改動）

```rust
// 現在（src/hk/runtime.rs 和 src/us/runtime.rs 中）
let (primary, competing) = infer_mechanisms(states, invalidation_rules);

// 之後
let (primary, competing) = infer_mechanisms_with_factor_adjustments(
    states,
    invalidation_rules,
    &store.knowledge.calibrated_weights.factor_adjustments,
);
```

#### 4b. BrainGraph::compute — 機構邊信心調整

```rust
// src/graph/graph.rs — InstitutionToStock 邊計算中

// 現在：每條機構邊的 confidence 只基於當前 tick 的 seat_count
let confidence = Decimal::from(seat_count as i64) / max_seats;

// 之後：查詢機構記憶，有歷史記錄的機構獲得信心加成
let history_bonus = store
    .knowledge
    .institutional_memory
    .get(&(institution_id, symbol.clone()))
    .map(|profile| {
        if profile.observation_count >= 5 {
            let hit_rate = Decimal::from(profile.directional_hit_count)
                / Decimal::from(profile.observation_count);
            // hit_rate > 0.5 → 正向加成，< 0.5 → 負向
            (hit_rate - dec!(0.5)) * dec!(0.2)
        } else {
            Decimal::ZERO // 樣本不足，不調整
        }
    })
    .unwrap_or(Decimal::ZERO);
let confidence = clamp_unit_interval(base_confidence + history_bonus);
```

#### 4c. PredicateInputs — 新增 mechanism_priors

```rust
// src/pipeline/predicate_engine.rs
pub struct PredicateInputs<'a> {
    // ... 現有欄位不變 ...
    pub mechanism_priors: &'a HashMap<String, MechanismPrior>,
}
```

謂詞如 `confidence_builds` 可以參考 `mechanism_priors` 來調整分數：當歷史表明某機制在當前 regime 下表現差時，降低信心建構的評分。

### 5. 啟動時恢復

```rust
// src/ontology/store/knowledge.rs

impl AccumulatedKnowledge {
    pub fn empty() -> Self {
        Self {
            institutional_memory: HashMap::new(),
            mechanism_priors: HashMap::new(),
            calibrated_weights: CalibratedWeights::default(),
        }
    }

    /// 從 SurrealDB 恢復累積知識。
    /// 在 runtime 啟動時呼叫，ObjectStore 初始化之後。
    #[cfg(feature = "persistence")]
    pub async fn restore_from(db: &EdenStore) -> Self {
        let mut knowledge = Self::empty();

        // 1. 從 lineage_metric_row 恢復 mechanism_priors
        if let Ok(rows) = db.recent_lineage_metric_rows(500).await {
            knowledge.restore_mechanism_priors(&rows);
        }

        // 2. 從 case_reasoning_assessment 恢復 calibrated_weights
        if let Ok(assessments) = db.recent_case_reasoning_assessments(200).await {
            let outcome_ctx = OutcomeLearningContext::default();
            let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
            knowledge.apply_calibration(&feedback);
        }

        // 3. institutional_memory 從 knowledge_node_state 恢復
        if let Ok(nodes) = db.current_knowledge_node_state().await {
            knowledge.restore_institutional_memory(&nodes);
        }

        knowledge
    }
}
```

### 6. ObjectStore 的共享模型

```rust
// 現在：Arc<ObjectStore>（不可變）
// 之後：靜態部分不可變，knowledge 部分需要 interior mutability

// 選項 A：整個 ObjectStore 用 Arc<RwLock<ObjectStore>>
//   缺點：每次讀靜態欄位也要拿鎖

// 選項 B（推薦）：拆分
pub struct ObjectStore {
    // 靜態部分
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,
    // 動態知識（interior mutability）
    pub knowledge: RwLock<AccumulatedKnowledge>,
}

// 讀取：store.knowledge.read().unwrap().mechanism_priors.get(...)
// 寫入：store.knowledge.write().unwrap().accumulate(...)
```

使用 `std::sync::RwLock`（不是 tokio 的），因為持鎖時間極短（HashMap lookup / insert），不需要跨 await。

## 不改什麼

- **TickHistory** — 仍是環形緩衝，存高頻 raw tick 資料，不改
- **SurrealDB 寫入路徑** — 現有的 `knowledge_*_state` 寫入繼續，不改
- **API 讀取路徑** — 前端仍從 SurrealDB 讀，不改
- **ObjectStore 的所有現有方法** — `institution_for_broker`, `stocks_in_sector` 等不改
- **TemporalEdgeRegistry / NodeRegistry / BrokerRegistry** — 繼續獨立存在，它們追蹤的是圖拓撲變化，不是累積知識
- **CausalTimeline** — 繼續獨立存在，它追蹤的是因果領袖演變

## 新增檔案

| 檔案 | 用途 |
|------|------|
| `src/ontology/store/knowledge.rs` | `AccumulatedKnowledge` 結構 + accumulate + restore |

## 修改檔案

| 檔案 | 改動 |
|------|------|
| `src/ontology/store/object_store.rs` | 新增 `knowledge: RwLock<AccumulatedKnowledge>` 欄位 |
| `src/ontology/store/mod.rs` | 新增 `mod knowledge` |
| `src/ontology/store/init.rs` | ObjectStore 建構時加 `knowledge: RwLock::new(AccumulatedKnowledge::empty())` |
| `src/hk/runtime.rs` | tick 末尾加 `store.knowledge.write().unwrap().accumulate(...)` + mechanism inference 改用 `calibrated_weights` |
| `src/us/runtime.rs` | 同上 |
| `src/graph/graph.rs` | InstitutionToStock 邊計算時讀 `institutional_memory` |
| `src/pipeline/mechanism_inference.rs` | 不改（已支援 `factor_adjustments` 參數） |
| `src/pipeline/predicate_engine.rs` | `PredicateInputs` 新增 `mechanism_priors` 欄位 |
| `src/hk/runtime/startup.rs` | 啟動時 `restore_from` |
| `src/us/runtime/startup.rs` | 啟動時 `restore_from` |

## 驗證方式

1. `cargo check` + `cargo check --tests` 通過
2. 現有測試全部通過（`ObjectStore::from_parts` 自動帶空的 `AccumulatedKnowledge`）
3. 新增測試：
   - `AccumulatedKnowledge::accumulate` 正確沉澱機構記憶
   - `AccumulatedKnowledge::apply_calibration` 正確更新校準權重
   - `BrainGraph::compute` 在有機構記憶時邊的 confidence 有變化
   - `infer_mechanisms_with_factor_adjustments` 在有校準權重時結果有變化
