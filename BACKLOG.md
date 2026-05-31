# Eden Development Backlog

## 🔴 P0 — Code Review Fixes（先清債）
- [x] resolved_tick/net_return 語義統一 (`src/temporal/lineage/outcomes.rs`)
- [x] 刪除 `evaluation/` 死碼分叉
- [x] `RwLock` unwrap → recoverable handling (`runtime.rs`, `graph.rs`)
- [x] broker_confirms_bias short 方向 (`agent/recommendations/symbol.rs`)
- [ ] governance_reason_code 統一 (`agent/types/recommendation.rs`)
- [x] `query.rs` impl block 斷裂
- [ ] confidence 二次 boost (`us/pipeline/reasoning.rs`)
- [ ] dominant_channels 聚合而非覆蓋 (`vortex.rs` + `convergence_memory.rs`)

## 🟡 P1 — 結構性修復
- [ ] `TacticalSetup.action` `String` → enum
- [ ] `family_key` 從 `risk_notes` 移到正式欄位
- [ ] Pipeline 鏈去重（3次重複 → 1個函數）
- [x] `shadow_scores` 接通（`evolution.rs` 的 safety gate）
- [ ] `baseline_quality` 初始化修正
- [ ] US 加入 `FamilyAlphaGate`
- [ ] `consecutive_misses` 遞增修正

## 🟢 P2 — CLAUDE.md Roadmap
- [ ] Attribution → Template Selection 回饋
- [ ] Case-level Reasoning Narrative
- [ ] Operator Preference Layer
- [ ] Adaptive Attention Rebalancing
- [ ] BrokerBehaviorProfile

## 🔵 P3 — 拓撲湧現
- [ ] Hypothesis generation 從 template-matching 反轉為 convergence-detection
- [ ] 正反饋閉環：outcome 好 → 放大 family / 降低門檻 / 記住漩渦形狀
- [ ] 湧現：新漩渦形狀不屬於任何現有 template → 自動提取為新 pattern

## 🟠 發現的 pre-existing 債（2026-05-31 掃出 → 全數解決）
- [x] 5 個 pre-existing 失敗測試已全部處理（每個都用對抗式診斷：git 考古 + 同類測試交叉比對 + 反方論證，先分類「測試對碼錯」vs「碼對測試過時」再動手）。`cargo test --lib` → **768 passed / 0 failed**。
  - `agent::build_session_creates_thread...` → **碼 bug**（refactor 78f932f 把 `in X` 改成 `(X)`）→ 改 `views.rs`（commit 9ea190e）
  - `agent::wake_suggested_tools_prefer_primary...` → **碼 bug**（工具增多後 truncate(6) 把 primary surface 切掉）→ `attention.rs` 改 8（9ea190e）
  - `core::runtime_loop::bootstrap_does_not_consume_ready_messages` → **碼 bug**（update drain 漏了 `if received_push` 對稱）→ `runtime_loop.rs`（9ea190e）
  - `us::us_market_hours_respect_dst_windows` → **測試過時**（71fe3f0 故意把開盤擴到盤前 04:00，測試的「盤前」時間戳失效）→ `runtime_tests.rs`（9ea190e）
  - `pipeline::signals::corroborated_symbol_pressure_emits_sector_propagation` → **功能未實作**（HK `detect()` 缺板塊傳導）→ 照美股 `propagate_symbol_events_to_sector` 補上（commit f65bca9）

## ✅ 已完成
- 2026-04-03 | resolved_tick/net_return 語義統一 | `src/temporal/lineage/outcomes.rs`, `src/runtime_loop.rs` | adaptive outcome 改成以 peak tick 為邊界重算 outcome，確保 `resolved_tick` 與 `net_return`/`return_pct` 同 horizon；同時修正 runtime loop 測試以移除對 Tokio `test-util` 的隱性依賴，讓驗證鏈可在當前配置下跑通。
- 2026-04-03 | 刪除 `evaluation/` 死碼分叉 | `src/temporal/lineage/outcomes/evaluation/context.rs`, `src/temporal/lineage/outcomes/evaluation/outcome.rs` | 移除未被編譯路徑使用的重複 outcome evaluation 實作，避免未來修改誤落在錯誤分支。
- 2026-04-03 | `RwLock` unwrap → recoverable handling | `src/ontology/store/object_store.rs`, `src/graph/graph.rs`, `src/hk/runtime.rs`, `src/hk/runtime/startup.rs`, `src/us/runtime/startup.rs` | 在 `ObjectStore` 提供 poisoned `RwLock` 恢復入口，並把 graph/HK/US 活躍路徑改成走單一恢復邏輯，避免 poisoned lock 直接把 runtime 打崩。
- 2026-04-03 | `broker_confirms_bias` short 方向修正 | `src/agent/recommendations/symbol.rs`, `src/agent/tests.rs` | broker confirmation 改成只把帶有對應方向持倉的 newly-entered broker 算入確認，不再把 `entered` 本身當成 short confirmation；補了回歸測試保證 directionless entered broker 不會直接觸發 short entry。
- 2026-05-31 | 修復不編譯的 lib build（3 個 pre-existing 路障）| `Cargo.toml`, `Cargo.lock`, `src/persistence/store/query.rs`, `src/cases/review_analytics.rs` | (1) query.rs 雜散 `}` 提前關閉 `impl EdenStore`，孤立 `load_candidate_mechanisms`/`load_causal_schemas`（P0 斷裂）；(2) rust_decimal 缺 `serde-with-arbitrary-precision` feature → `arbitrary_precision_option` 模組找不到；(3) review_analytics 對「已是 Decimal」的值呼叫 `.parse::<Decimal>()`。修完 `cargo check --lib` 0 error。commit `8a56e5a`。
- 2026-05-31 | 補上 5 個測試引用卻從未實作的方法 | `pipeline/reasoning/context.rs`(`record_propagation`), `pipeline/residual.rs`(`residual_adjusted_propagation_strength`), `temporal/buffer.rs`(`graph_edge_transitions_for_id`), `us/graph/graph.rs`(`active_cross_market_pairs`), `us/temporal/causality.rs`(`recent_leaders`) | 皆 test-only caller（所以 `cargo check --lib` 先前仍能綠），依各自測試 spec 實作，無 production 行為改變。`cargo test --lib` → 763 passed / 5 pre-existing failed。commit `68faf86`。
- 2026-05-31 | `shadow_scores` 接通 evolution safety gate | `src/temporal/lineage/evolution.rs`, `src/temporal/lineage.rs`, `src/hk/runtime.rs` | gate 先前查的 `shadow_scores` map 永遠空（unused、never-mutated HashMap）→ `unwrap_or(true)` → 所有 schema 無門檻驗證 = **非收斂根因**。新增 `compute_shadow_scores_from_outcomes`：用 `CaseRealizedOutcome` 既有欄位（regime/session affinity + convergence 門檻）算每個 schema 的反事實命中率；hit-rate < 0.30 且 ≥3 次匹配的 schema 被擋出驗證。刻意不呼叫 `preconditions_met`（其依賴的 `contest_state` 全 codebase 無 production 來源）。+2 測試（皆過）。commit `fac739f`。
- 2026-05-31 | 修復 5 個 pre-existing 失敗測試中的 4 個（3 碼 bug + 1 過時測試）| `agent/views.rs`, `agent/attention.rs`, `core/runtime_loop.rs`, `us/runtime_tests.rs` | 每個先對抗式分類（test-stale vs code-bug）再改：3 個其實是 production 回歸（測試是對的）、1 個是測試時間戳過時。767 passed。commit `9ea190e`。
- 2026-05-31 | 補上 HK 板塊傳導功能（第 5 個失敗測試）| `src/pipeline/signals/events.rs` | `detect()` 本來不發任何板塊範圍信號；照美股 `propagate_symbol_events_to_sector` 把 symbol 級 `SmartMoneyPressure` 事件按板塊分組，同板塊 ≥2 檔佐證 → 發 Sector 範圍 `CompositeAcceleration`（magnitude=均值）。沿用既有 median-cutoff 顯著性門檻，≥2 規則保證孤立單檔不傳導。**768 passed / 0 failed**，persistence build 亦綠。commit `f65bca9`。
