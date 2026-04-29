# Attention Wake — Entropy-Ranked Symbol Attention

**Date**: 2026-04-19
**Author**: Claude Code (with operator)
**Status**: Design for review

## 為什麼做

A1 已 land persistent `PressureBeliefField`；A2 已 land decision ingestor。第一個真正把 A1 belief 變成 operator-visible 推理訊號的 spec：**每 tick 輸出 Eden 此刻對哪些 symbol 最不確定**。

對應 CLAUDE.md P0 #2「Information gain attention ranking 接 wake」。

**Why entropy (not full MI)**：
- `CategoricalBelief<PersistentStateKind>.entropy()` = `H(state)` = **MI upper bound** with a hypothetical noiseless observation (information-theoretic correctness)
- 無需決定 candidate observation 的 noise variance（真 MI 需要）
- Operator 意義直觀：「這支股票 Eden 的 state 模型最接近 uniform」= 最應該多看
- Parameter-free；可以後續 spec 加 Gaussian variance 或真 MI rank_candidates

## 本 spec scope

**In**：
- 新 `AttentionItem` struct + `PressureBeliefField::top_attention(k)` method
- 新 `format_attention_line` helper
- HK + US runtime wake 輸出（在 belief notable block 之後）
- Unit tests + roundtrip integration test
- 跟既有 `belief:` notable 行並列，不取代

**Out（留給後續 spec）**：
- Gaussian variance-weighted attention（B.5）
- 真正 MI rank_candidates 呼叫（B.6，需要 channel noise variance 參數化）
- Cross-tick attention drift tracking
- Attention → intervention 自動觸發
- Per-channel attention（"NVDA.US orderbook is most informative"）

## 資料模型

```rust
// 新增到 src/pipeline/belief_field.rs

#[derive(Debug, Clone)]
pub struct AttentionItem {
    pub symbol: Symbol,
    pub state_entropy: f64,
    pub sample_count: u32,
    /// Entropy of a uniform 5-state belief = ln 5 ≈ 1.609 nats.
    /// Included so consumers can compute `state_entropy / max_entropy`
    /// without re-computing.
    pub max_entropy: f64,
}
```

`max_entropy` 是常量 `ln(5)` — CategoricalBelief 用 5 個 PersistentStateKind variants。對每個 item 重複放同值看起來多餘，但簡化 formatter API（不需要另傳）+ 保留擴展性（未來若加別的 variant 數）。

## API

```rust
impl PressureBeliefField {
    /// Rank symbols by CategoricalBelief<PersistentStateKind> entropy
    /// descending. Cap at k. Only symbols with sample_count >= 1 are
    /// eligible. Returns empty if no categorical beliefs.
    ///
    /// Unranked (entropy returns None) symbols are silently dropped —
    /// shouldn't happen since probabilities always normalize, but the
    /// entropy() API is Option<f64> so we honor it.
    pub fn top_attention(&self, k: usize) -> Vec<AttentionItem>;
}

/// Format an AttentionItem as a single wake line.
///
/// Shape: `attention: SYMBOL state_entropy=V.VV nats (n=N, PP% of max)`
pub fn format_attention_line(item: &AttentionItem) -> String;
```

**Implementation**：

```rust
pub fn top_attention(&self, k: usize) -> Vec<AttentionItem> {
    const MAX_ENTROPY: f64 = 1.6094379124341003; // ln(5)
    let mut items: Vec<AttentionItem> = self
        .categorical
        .iter()
        .filter(|(_, cat)| cat.sample_count >= 1)
        .filter_map(|(symbol, cat)| {
            cat.entropy().map(|h| AttentionItem {
                symbol: symbol.clone(),
                state_entropy: h,
                sample_count: cat.sample_count,
                max_entropy: MAX_ENTROPY,
            })
        })
        .collect();
    items.sort_by(|a, b| {
        b.state_entropy
            .partial_cmp(&a.state_entropy)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items.truncate(k);
    items
}

pub fn format_attention_line(item: &AttentionItem) -> String {
    let pct = if item.max_entropy > 0.0 {
        (item.state_entropy / item.max_entropy * 100.0).round() as i64
    } else {
        0
    };
    format!(
        "attention: {} state_entropy={:.2} nats (n={}, {}% of max)",
        item.symbol.0, item.state_entropy, item.sample_count, pct
    )
}
```

**Note on MAX_ENTROPY constant**: hardcoded `ln(5)` because `PersistentStateKind` has exactly 5 variants, guarded by `PERSISTENT_STATE_VARIANTS` const in same file. If that const changes length, this constant must update too — plan includes a test that asserts this consistency.

## Runtime Integration

### HK + US symmetric

**HK (`src/hk/runtime.rs`)**: In the same `{ ... }` scope that already holds the belief notable block + decision ledger wake loop + snapshot cadence, add a new loop **after** the notable-beliefs loop and decision-summary emission, but **before** the 60s rescan block (ordering is cosmetic; this just follows the "wake emissions first, then timers" convention).

```rust
// After existing notable/prior-decisions loops:
for item in belief_field.top_attention(5) {
    artifact_projection
        .agent_snapshot
        .wake
        .reasons
        .push(eden::pipeline::belief_field::format_attention_line(&item));
}
```

**US (`src/us/runtime.rs`)**: mirror with `crate::` paths instead of `eden::`.

### Ordering in wake

Per-tick wake will contain (in order):
1. Existing narrative / inference / institution lines (unchanged)
2. `belief:` notable lines (top 5 from A1)
3. `prior decisions:` lines for any notable symbol with history (from A2)
4. **NEW**: `attention:` lines (top 5 by entropy)

Step 4 is in addition to step 2; the same symbol can appear in both (it just had a shift **and** is currently uncertain). That's fine — they convey different information.

## Tests

### Unit (all in `src/pipeline/belief_field.rs`)

1. `top_attention_empty_field_returns_empty` — trivial
2. `top_attention_uniform_has_near_max_entropy` — create categorical belief with uniform update pattern, assert entropy > 0.9 × max_entropy
3. `top_attention_point_mass_has_low_entropy` — 30 samples all on Continuation, assert entropy < 0.2 × max_entropy
4. `top_attention_orders_descending_by_entropy` — 3 symbols with different certainty levels, verify ranking
5. `top_attention_honors_cap` — 10 symbols, k=3 → len 3
6. `format_attention_line_shows_percent_of_max` — golden output check
7. `max_entropy_constant_matches_variant_count` — assert `ln(PERSISTENT_STATE_VARIANTS.len() as f64) == MAX_ENTROPY` (within 1e-9) so a future variant add/remove will fail a test

### Integration (append to `tests/belief_field_integration.rs`)

1. `attention_ranking_survives_snapshot_restore` — build field with varied categoricals → `top_attention(3)` → snapshot → restore → `top_attention(3)` → assert same symbols in same order

## 驗收

1. ✅ `cargo check --lib -q` + `--features persistence -q` + `--no-default-features -q` 全通過
2. ✅ ≥ 7 new unit tests + 1 integration test 通過
3. ✅ 既有 tests 全過（belief_field 原有 11 + belief_snapshot 7 + decision_ledger 13）
4. ✅ Live HK/US session wake 看得到 `attention:` 行

## 風險

| Risk | Mitigation |
|------|-----------|
| CategoricalBelief.entropy() 回傳 None 的場景 | API 允許 None；filter_map silently drops — 安全 |
| max_entropy 硬編碼漂移 | Test 7 強制一致性 |
| 5 個 attention 行 × 2 市場 = 每 tick 多 10 行 wake | Top-5 cap + per-market；接受。若未來太吵可降到 3 |
| 同 symbol 在 notable + attention 雙顯 | Spec 接受（正交訊號）；operator 有兩種視角合理 |

## Out of Scope 備忘

| Future spec | 依賴 |
|-------------|------|
| B.5 Gaussian-variance attention | 本 spec |
| B.6 真 MI rank_candidates wake | 本 spec + 每通道 noise variance 參數化 |
| Attention drift cross-tick tracker | 本 spec |

## Scope 備注

**刻意小**：~80 LOC 新增 + ~120 LOC tests。跟 A1/A2 一樣是「加一層 observable 不動現有邏輯」的 spec。完成後 Eden 第一次有「operator-facing attention surface」— 從「這裡有事」（notable, 事件） + 「這裡有歷史」（prior decisions） 進展到「這裡最該多看」（attention, 狀態）。

Plan next: `writing-plans` → `executing-plans` inline。
