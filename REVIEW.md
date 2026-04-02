# Eden Code Review Findings

本文件整理 2026-04-03 的 repo-level review 結果，作為 Codex 持續修復的來源。

## Summary

- P0: 8
- P1: 22
- P2: 38
- P3: 24
- Total: 92

## P0

- P0-1: `compute_case_realized_outcomes_adaptive` 的 `resolved_tick` 用峰值，但 `net_return` 用最終值，語義混亂。
- P0-2: `src/temporal/lineage/outcomes/evaluation/` 下有死碼分叉，`setup_direction`、`fade_return`、`estimated_execution_cost` 與活躍版邏輯不同。
- P0-3: `knowledge.write().unwrap()` 在 tick loop / runtime path，poisoned lock 會直接 crash。
- P0-4: `broker_confirms_bias` 對 short 用 `entered` broker 作確認，與方向無關。
- P0-5: `governance_reason_code` 在 `ReviewRequired` 分支重走條件鏈，可能回傳錯誤 code。
- P0-6: `market_why_not_single_name` 在空 items 時 ratio=1，產生誤導性解釋。
- P0-7: `store/query.rs` 的 `load_candidate_mechanisms` / `load_causal_schemas` 脫離 `impl EdenStore`。
- P0-8: `causal_scope_key` 在 `store.rs` 與 `query.rs` 重複。

## P1

- P1-1: US `confidence` 被 boost 兩次。
- P1-2: US 缺少 `FamilyAlphaGate`。
- P1-3: `evaluate_us_candidate_mechanisms` 的 `consecutive_misses` 在 pattern 仍存在時不遞增。
- P1-4: `setup_direction` 依賴 `title.starts_with("Short ")`。
- P1-5: `TacticalSetup.action` 是 `String`，且有兩套 priority 體系。
- P1-6: `family_key` 透過 `risk_notes` 字串前綴傳遞。
- P1-7: `extract_sectors_from_fingerprints` 不看 parent-child 關係。
- P1-8: `evaluate_candidate_mechanisms` 的 promote 和 decay 可能同 tick 連續觸發。
- P1-9: `BrainGraph::compute` 中 `store.knowledge.read().unwrap()`。
- P1-10: `case_realized_outcome` schema 缺少 `primary_lens`。

## Cross-module Themes

- Stringly typed fields 過多，編譯器無法捕捉 typo / 狀態衝突。
- Pipeline 鏈路有重複實現。
- `dominant_channels` 聚合被覆蓋而非累積。
- event attribution 在 HK / US 都有雙次調用。
- `shadow_scores` 未接通，演化 safety gate 被短路。

## P2 Highlights

- Graph 構建存在 O(n^2) 熱路徑。
- `attention_for()` 每次重算分配。
- `TickRecord::capture` 每 tick 深拷貝大量資料。
- `compute_case_realized_outcomes_adaptive` 同 tick 被多次調用。
- `baseline_quality` 在已有 live mechanisms 時可能永不初始化。
- 前端有 401 遞迴 prompt / 類型缺失問題。

## Notes

- 原始完整 review 報告來自 2026-04-03 對話內容。
- 本文件先作為修復清單索引；修復時應以當前 code 為準，逐條再驗證。
