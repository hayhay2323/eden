# Prompts Changelog

每次 prompt 改動都要在這裡 append 一筆。包含 why、what、validate by。

**規則**:
- 改動要引用具體 audit 或 session 觀察當理由
- 每個改動要寫 validate metric（下一週怎麼判斷有效）
- 連續 2 次同方向放寬 → 停，檢討是否 operator rationalization

---

## v1 (2026-04-15, baseline)

**狀態**: Initial version. 沒有前置版本可比較。

**包含**:
- `operator_discipline_rules.md` v1 — 12 條底線規則
- `operator_live_prompt_v1.md` — 平日 live session 流程
- `operator_weekend_prompt_v1.md` — 週末 4 段 audit/iteration
- `operator_scratch.md` — stateful session scratch template
- `metrics_targets.md` — Target 1: `actionable_hit_rate >= 55% over 5 sessions`

**Rationale**（為什麼要 v1 這個 shape）:

基於 2026-04-14 US session (144 rounds) 和 2026-04-15 HK session (~48 rounds) 的實戰觀察，我識別出 10 個當前 operator prompt 的問題：

1. "有 edge 就下單" 是假指令 — 沒強制 pre-commit
2. 沒有 low-novelty short-circuit
3. 沒指定 headline metric
4. 無 market open 特殊協議
5. 無 pre-session ritual
6. 無 post-session retrospective
7. Trader + analyst + product tester role 混合
8. 無 hypothesis accumulator
9. 無 attention budget
10. 無 weekend / 非交易日協議

v1 主要 address #1, #2, #3, #5, #6, #7 (live session)，#10 (weekend)，部分 #4, #8, #9。

**這週要驗證的假設**:
- H1: Pre-commit trigger framework 會讓 operator 減少 reactive 修改（discipline_overrides == 0）
- H2: Low-novelty short-circuit 讓 pre-market session 至少節省 50% 時間
- H3: `actionable_hit_rate` 作為 headline metric 能避免 operator 被 baseline `hit_rate` 誤導

**首週 audit 的 3 個問題**:
1. 有多少輪真的用了 short-circuit？佔 pre-market 輪次 %？
2. Discipline_overrides 實際次數（應該為 0）？
3. `actionable_hit_rate` 是否實際成為 operator 日常注意力的錨點？

---

## 迭代方向 backlog（等 audit 後考慮）

以下是我還沒納入 v1 的 ideas，等第一次 weekend audit 後決定是否加：

- **Round-hash based skip**: 計算 roster + scorecard 的 hash，一致就自動 skip（比當前 "novelty check" 更嚴格）
- **Per-cluster attention weighting**: 根據 cluster_states direction 強度決定某 symbol 值不值得逐個看
- **Exit timing optimization**: 目前 exit_triggers 是 OR logic，可能太寬，看 session 數據再調
- **Longer cron interval midday**: 盤中穩定期改成 5 min cadence，close 前恢復 2 min
- **Hypothesis template**: 強制 session 開始寫 3 個 hypothesis（minimum），不要 optional
- **Paper trade counter**: 區分 real trades 和 hypothetical（當 MCP 不可用時），分開統計

以上每一個都要有 pattern 證據才能進 v2 — 沒 audit 證據的直覺不進 prompt。

---

## v{N+1} template

未來版本的 entry 模板：

```
## v{N+1} (YYYY-MM-DD)

**Based on audit**: `docs/weekly_audit_YYYY-MM-DD.md`
**Based on patterns**: `docs/weekly_patterns_YYYY-MM-DD.md`

**Changes**:
1. [file] [rule X] → [new behavior]
   - Rationale: [pattern Z from Phase 2 audit]
   - Validate by: [metric W in next week]
2. ...

**Rollback criteria**: 如果 [metric V > threshold]，下週 weekend 回滾這個改動
```
