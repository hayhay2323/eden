# Eden Metrics & Targets

Eden 的質變不是用感覺判斷的，是用可量化指標。這份文件定義當前在驗證什麼目標、用什麼數字判斷 PASS/FAIL、什麼時候該換目標。

---

## 🎯 當前目標（v1）

**目標**: **Eden `actionable_hit_rate` 是有 edge 的 signal，不是雜訊**。

### PASS 條件（必須全部滿足）
- [ ] 未來 5 個完整 US trading session 跑完
- [ ] Aggregated `actionable_hit_rate >= 55%`（跨 session 加權平均）
- [ ] `actionable_mean_return > 0.002` per trade（跨 session 加權平均）
- [ ] `actionable_resolved >= 2000` total samples（統計最低門檻）
- [ ] 無系統性 bug 導致數字膨脹（e.g. tie-on-zero 被算 miss 或 hit）

### FAIL 情況
如果 5 session 後：
- `actionable_hit_rate < 45%` → Eden 信號品質有根本性問題，**需要重看 reasoning pipeline**
- `actionable_hit_rate 45-55%` 但 `mean_return > 0.002` → edge 存在但小，繼續累積 sample
- `actionable_hit_rate >= 55%` 但 `mean_return < 0` → hit rate 對但每次賺得少於交易成本，**需要檢查持倉時間 / 退場 timing**

### 為什麼這個目標
1. 這是**唯一可以 one-shot 證偽 Eden 是否有 edge** 的單一指標
2. 55% 是一個夠高的門檻（比 coin flip 高 10 pp），也夠低以避免對完美的追求
3. 2000 samples 是 binomial test 下能檢測 55% vs 50% 差異的最低 n
4. 5 session 避免單一 session 的運氣干擾

### Validation calendar

| Session # | 日期（預計） | 當前 AHR | 累計 samples | 判斷 |
|---|---|---|---|---|
| 1 | 2026-04-15 | TBD | TBD | 進行中 |
| 2 | 2026-04-16 | - | - | - |
| 3 | 2026-04-17 | - | - | - |
| 4 | 2026-04-18 | - | - | - |
| 5 | 2026-04-21 | - | - | Final check |

每個 session 結束後填入，週末 audit 時 aggregate。

---

## 📊 Session 內追蹤指標

每個 session 開始和結束都要記錄：

### 信號品質
- `hit_rate`（baseline noise floor）
- `actionable_hit_rate`（headline）
- `actionable_mean_return`
- `actionable_resolved` delta

### rrc 分佈（新 rrc 的 fire 頻率）
- `insufficient_raw_support` count
- `stale_symbol_confirmation` count
- `signal_translation_gap` count
- `orphan_signal_cap` count
- `late_signal_timing` count
- `directional_conflict` count （**新**）
- `freshness_decay_aging` count （**新**）
- `freshness_decay_expired` count （**新**）
- `raw_persistence_insufficient` count （**新**）

### Operator 行為
- Pre-committed triggers 的 total count
- Trigger fired count
- 實際交易 count（entry / exit）
- Discipline saves（我想進場但規則擋下的次數）
- 真實 P&L
- Hypothetical P&L（若所有 triggers 都能完美執行）

### 新 fields 的使用率
- Cases with non-null `first_enter_tick`: count
- Cases with `freshness_state = "fresh"`: count
- Cases with `freshness_state = "aging"` or worse: count
- Max `ticks_since_first_enter` observed: value

---

## 🔮 後續目標（目前 target 達成後的下一個）

以下目標依序排定（只有前一個 PASS 才解鎖下一個）：

### Target 2: Operator OS layer validation
**條件**: pre-committed triggers framework 能 deliver operator discipline
**PASS**: 跨 5 session，operator 實際 trades 的 realized P&L >= hypothetical triggered P&L 的 80%
**意義**: 紀律規則真的被遵守，trigger framework 有實際 alpha 值

### Target 3: Learning loop activation
**條件**: Edge learning feedback loop 啟動後，graph weights 開始自動調整
**PASS**: Week 2 `actionable_hit_rate` > Week 1 `actionable_hit_rate` 且改進可歸因於 edge_learning_ledger 的更新
**意義**: Eden 自己會變聰明，不是靠 operator 手動 calibration

### Target 4: Multi-operator readiness
**條件**: Prompts + rules 能讓另一個 operator（不是原作者）拿來就用
**PASS**: 找 1 個外部 tester 用 prompts v2，跟著 discipline rules 跑 3 個 session，`actionable_hit_rate` 在 50% 以上
**意義**: Eden 成為可 productize 的工具，不是「你的個人工具」

---

## 📐 為什麼用這套指標而不是別的

**為什麼不用「淨 P&L」當主要目標？**
- P&L 受 operator 紀律、position sizing、market regime 影響太大
- 你今天 P&L 好不等於 Eden signal 好，可能是 operator 自律好
- `actionable_hit_rate` 直接量測 Eden 的 output quality，不混 operator factor

**為什麼不用「Sharpe ratio」？**
- Sharpe 需要至少 20-50 個 trade 才有意義，以 Eden 當前的進場頻率需要幾週才累積
- 當前階段 ship velocity 比 risk-adjusted metric 重要

**為什麼不用「單一 session 內的 r33-style alpha moment」？**
- 單個好信號是軼事不是統計
- 需要跨 session 的一致性

**為什麼不用「operator 主觀打分」？**
- operator 有 confirmation bias，看到 r15 6855 winner 就以為 Eden 厲害，看到 r28 whipsaw 就以為 Eden 沒用
- 一個 scalar 數字跨 session 可比

---

## 當前狀態（自動更新）

- Target 1 進行中
- 開始日期: 2026-04-15
- Last session: 2026-04-15（進行中）
- Latest observed `actionable_hit_rate`: ~34-36%（pre-market noise，待開市後真實數字）
- Latest `actionable_resolved`: ~9200
- 距離目標: 需要 `>= 55%` + `>= 2000` samples × 5 sessions
