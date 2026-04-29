# Operator Discipline Rules (v2 — 2026-04-15 post-session-1)

**v1 廢棄**：12 條 AND gate 導致 120 rounds 0 trades。v2 砍到 3 條，目標是允許交易而不只是擋交易。

---

## Rule 1 — Entry

**要進場，以下 3 條必須同時滿足**:

1. `action == "enter"` 且 `confidence >= 0.7`
2. `raw_disagreement.support_fraction >= 0.67`（不是 0.85，放寬）
3. `review_reason_code == null` OR rrc ∈ {`stale_symbol_confirmation`}（僅這一個 rrc 可 override，因它 midday 無差別擋 signal）

**強制硬擋 rrc**（永遠不 override）:
- `directional_conflict` — Eden 自己打架
- `late_signal_timing` — timing 錯（position_in_range 在 Long 信號的低端或 Short 信號的高端）
- `raw_persistence_insufficient` — 少於 2 tick persistence
- `freshness_decay_aging` / `freshness_decay_expired` — 信號老了

## Rule 2 — Sizing

- Single position: **$3k USD or 1,000 HKD equivalent**（小額試倉）
- Max concurrent positions: **3**
- Max session total risk: **$10k USD notional**

## Rule 3 — Exit

**任一觸發即平倉（OR logic）**:

1. Eden signal 消失（case 從 roster removed, 或 action 降到 observe）
2. `composite_score` 方向翻轉（Long → composite < 0, Short → composite > 0）
3. Unrealized P&L < **-$100 USD** per position（hard stop）
4. `raw_disagreement.support_fraction < 0.50`（raw 反轉）

---

## 改什麼 vs v1

| v1 | v2 | 為什麼 |
|---|---|---|
| 12 條 AND gate | 3 條 (entry/size/exit) | 簡單 |
| raw support ≥ 85% | ≥ 67% | 85% 讓 120 rounds 0 trades |
| `stale_symbol_confirmation` 硬擋 | 可 override | midday 80% roster 都是這個，擋等於 0 交易 |
| Pre-commit trigger 框架 | 刪 | Over-engineering |
| Attention budget 分層 | 刪 | Ritual |
| Headline metric `AHR >= 55%` | `excess_over_baseline >= 15pp` | Regime-independent |
| 每輪 3 個必答問題 | 刪 | Ritual |
| 禁止項 8 條 | 只保留「不追高追低」 | 用 `late_signal_timing` rrc 代替 |

## v2 的設計原則

**"positive expected value 機器" 而不是 "zero false-positive 機器"**。v1 把自己設計成永遠不會錯，代價是永遠不下單。v2 允許下錯單，只要期望值正。

每 session 至少要產生 **1 筆 trade outcome data** — 如果到 R50 還 0 enters，強制下最強 conf=1 case $2k USD（experimental budget）。

---

## Session 1 (2026-04-15) 學到什麼

- 120 rounds / 0 trades = 紀律失控，不是紀律勝利
- AHR 34% → 43% 是 Eden 自己 grind 出來的，不需要 operator 行動
- 真正的 operator value 在 trade outcome，不在 observation log
- `late_signal_timing` (ANET R110 案例) 是 valid guardrail，保留
- `raw_persistence_insufficient` 過擋 large-cap，需要 audit gate
- `stale_symbol_confirmation` midday 佔 80% roster，是最該放寬的 rrc

v2 的 bet 是：放寬 `stale_symbol_confirmation` override，保留其他硬 rrc，就能在下 session 實際產生 entries。若驗證 false positive rate 太高，v3 再收緊。
