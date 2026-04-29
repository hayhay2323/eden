# Eden Operator Prompts

這個資料夾是 Eden 的 **operator OS source code**。每份檔案都是 operator 在某個情境下該怎麼使用 Eden 的規則集。

跟 Eden 的 Rust code 一樣嚴肅 — 這些 prompts 決定了信號能不能被正確抽取成 alpha。Eden 本身只是 reasoning engine，真正產生交易 edge 的是 **Eden 的信號 + operator 的紀律 framework** 這個組合。

## 檔案索引

| 檔案 | 用途 | 何時讀 |
|---|---|---|
| `operator_discipline_rules.md` | 跨 session 不變的紀律底線 | 所有其他 prompt 的 import 依賴 |
| `operator_live_prompt_vN.md` | 平日 live session（有開市時）| US/HK 交易時段 |
| `operator_weekend_prompt_vN.md` | 週末 / 非交易日 | 週六日、節假日 |
| `operator_scratch.md` | 當前 session 的可變 state（由 operator 讀寫）| 每輪 cron fire 時 |
| `metrics_targets.md` | 短期目標和驗證指標 | 每次 session 開始 + 每週 audit |
| `prompts_changelog.md` | Prompt 版本歷史和改動理由 | 每次改 prompt 前看 |

## 迭代原則

1. **一次最多改一條**。無法歸因是最大的陷阱。
2. **每次改動要寫 hypothesis + validation metric**。
3. **保留版本歷史**（`_v1.md` → `_v2.md`，不要覆蓋）。
4. **紀律只能變嚴不能變鬆**。放寬規則是最危險的迭代方向。
5. **Bug fix** 和 **philosophy change** 要分開節奏 — 前者當日迭代，後者至少 2-3 個 session 驗證。

## Prompt 對應的 role

一個 operator 同時有三個 role，不同 prompt 會 prioritize 不同 role：

| Role | 關心 | 偏誤 |
|---|---|---|
| **Trader** | 實際 P&L，position safety | 保守，規則導向 |
| **Analyst** | 找信號 edge，驗證假設 | 客觀，量化優先 |
| **Product tester** | 找 bug、UX 缺陷 | 挑剔，批判 |

**Live session prompt** 優先 Trader，次 Analyst，Product tester 的觀察放 end-of-session。
**Weekend prompt** 優先 Analyst，次 Product tester，Trader 不動作（市場關閉）。

## 當前版本狀態

- `operator_discipline_rules.md` — v1（基準，後續變動要謹慎）
- `operator_live_prompt_v1.md` — v1
- `operator_weekend_prompt_v1.md` — v1
- `metrics_targets.md` — 第一個目標正在進行驗證（actionable_hit_rate ≥ 55% over 5 US sessions）
