# Eden Development Backlog

## 🔴 P0 — Code Review Fixes（先清債）
- [x] resolved_tick/net_return 語義統一 (`src/temporal/lineage/outcomes.rs`)
- [x] 刪除 `evaluation/` 死碼分叉
- [ ] `RwLock` unwrap → recoverable handling (`runtime.rs`, `graph.rs`)
- [ ] broker_confirms_bias short 方向 (`agent/recommendations/symbol.rs`)
- [ ] governance_reason_code 統一 (`agent/types/recommendation.rs`)
- [ ] `query.rs` impl block 斷裂
- [ ] confidence 二次 boost (`us/pipeline/reasoning.rs`)
- [ ] dominant_channels 聚合而非覆蓋 (`vortex.rs` + `convergence_memory.rs`)

## 🟡 P1 — 結構性修復
- [ ] `TacticalSetup.action` `String` → enum
- [ ] `family_key` 從 `risk_notes` 移到正式欄位
- [ ] Pipeline 鏈去重（3次重複 → 1個函數）
- [ ] `shadow_scores` 接通（`evolution.rs` 的 safety gate）
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

## ✅ 已完成
- 2026-04-03 | resolved_tick/net_return 語義統一 | `src/temporal/lineage/outcomes.rs`, `src/runtime_loop.rs` | adaptive outcome 改成以 peak tick 為邊界重算 outcome，確保 `resolved_tick` 與 `net_return`/`return_pct` 同 horizon；同時修正 runtime loop 測試以移除對 Tokio `test-util` 的隱性依賴，讓驗證鏈可在當前配置下跑通。
- 2026-04-03 | 刪除 `evaluation/` 死碼分叉 | `src/temporal/lineage/outcomes/evaluation/context.rs`, `src/temporal/lineage/outcomes/evaluation/outcome.rs` | 移除未被編譯路徑使用的重複 outcome evaluation 實作，避免未來修改誤落在錯誤分支。
