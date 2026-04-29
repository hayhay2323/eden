# Attention Wake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface Eden's current per-symbol state uncertainty as a ranked wake line `attention: X state_entropy=N nats (n=k, PP% of max)`, top-5 symbols by `CategoricalBelief<PersistentStateKind>.entropy()` descending.

**Architecture:** Add `AttentionItem` + `top_attention(k)` method + `format_attention_line` helper directly on `PressureBeliefField` in `belief_field.rs` (parallel to existing `NotableBelief` + `top_notable_beliefs` + `format_wake_line`). HK and US runtimes each emit their own `attention:` block after the existing belief-notable + prior-decisions blocks.

**Tech Stack:** Rust + existing `CategoricalBelief.entropy()` primitive. No new deps.

**Spec:** `docs/superpowers/specs/2026-04-19-attention-wake-design.md`

---

## File Structure

**Modified files:**
- `src/pipeline/belief_field.rs` — add `AttentionItem`, `top_attention`, `format_attention_line`, 7 new unit tests
- `src/hk/runtime.rs` — insert attention wake loop after notable/prior-decisions block
- `src/us/runtime.rs` — symmetric
- `tests/belief_field_integration.rs` — append `attention_ranking_survives_snapshot_restore`

**No new files.** B is entirely additive to existing A1 module.

**Concrete types referenced (verified from A1 / A2 commits):**
- `PressureBeliefField` with `categorical: HashMap<Symbol, CategoricalBelief<PersistentStateKind>>` (private field; iterated via `categorical_iter()` which is already pub)
- `CategoricalBelief.entropy() -> Option<f64>` (src/pipeline/belief.rs:291)
- `CategoricalBelief.sample_count: u32` (pub field)
- `PERSISTENT_STATE_VARIANTS: &[PersistentStateKind]` with length 5 (defined in belief_field.rs by A1)

---

## Branch + env

All commits on current branch (`codex/polymarket-convergence`). Additive.

```bash
export CARGO_TARGET_DIR=/tmp/eden-target
```

---

## Task 1: `AttentionItem` + `top_attention` + formatter + tests

**Files:**
- Modify: `src/pipeline/belief_field.rs`

- [ ] **Step 1: Add `AttentionItem` struct + constant**

Locate the section in `src/pipeline/belief_field.rs` that ends the `top_notable_beliefs` impl block (look for `candidates.truncate(k); candidates }`). After the closing `}` of `impl PressureBeliefField`, add:

```rust
/// Maximum entropy of a CategoricalBelief over PERSISTENT_STATE_VARIANTS
/// (5 variants). Equals ln(5). Exported so consumers can compute the
/// percent-of-max ratio cheaply.
pub const MAX_STATE_ENTROPY_NATS: f64 = 1.6094379124341003; // ln(5)

/// One symbol's attention score for the wake surface — how uncertain
/// Eden's current categorical posterior is for that symbol.
#[derive(Debug, Clone)]
pub struct AttentionItem {
    pub symbol: Symbol,
    pub state_entropy: f64,
    pub sample_count: u32,
    /// Upper bound of state_entropy for this belief (= MAX_STATE_ENTROPY_NATS).
    /// Included so consumers don't need to re-import the const.
    pub max_entropy: f64,
}
```

- [ ] **Step 2: Add `top_attention` method on PressureBeliefField**

Re-open `impl PressureBeliefField` (add a new `impl` block at the end of the file, below `format_wake_line`):

```rust
impl PressureBeliefField {
    /// Rank symbols by CategoricalBelief entropy descending, cap at `k`.
    /// Only symbols with sample_count >= 1 are considered; symbols whose
    /// entropy() returns None are silently dropped.
    ///
    /// Used by HK/US runtimes to produce `attention:` wake lines.
    pub fn top_attention(&self, k: usize) -> Vec<AttentionItem> {
        let mut items: Vec<AttentionItem> = self
            .categorical_iter()
            .filter(|(_, cat)| cat.sample_count >= 1)
            .filter_map(|(symbol, cat)| {
                cat.entropy().map(|h| AttentionItem {
                    symbol: symbol.clone(),
                    state_entropy: h,
                    sample_count: cat.sample_count,
                    max_entropy: MAX_STATE_ENTROPY_NATS,
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
}
```

- [ ] **Step 3: Add `format_attention_line` helper**

After the `format_wake_line` function and before the existing `#[cfg(test)]` module, add:

```rust
/// Format an AttentionItem as a single wake line.
///
/// Shape: `attention: SYMBOL state_entropy=V.VV nats (n=N, PP% of max)`
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

- [ ] **Step 4: Add 7 unit tests**

Append to the existing `#[cfg(test)] mod tests { ... }` block in `src/pipeline/belief_field.rs`:

```rust
    #[test]
    fn top_attention_empty_field_returns_empty() {
        let field = PressureBeliefField::new(Market::Hk);
        let attention = field.top_attention(5);
        assert!(attention.is_empty());
    }

    #[test]
    fn top_attention_uniform_has_near_max_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // One observation of each variant → as close to uniform as
        // Dirichlet-K=5 smoothing allows after equal-count updates.
        for variant in PERSISTENT_STATE_VARIANTS {
            field.record_state_sample(&s, *variant);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 1);
        let h = items[0].state_entropy;
        // Five equal counts → ~ uniform → entropy near ln(5).
        assert!(
            h > 0.9 * MAX_STATE_ENTROPY_NATS,
            "expected near-max entropy, got {} (max {})",
            h,
            MAX_STATE_ENTROPY_NATS
        );
        assert_eq!(items[0].sample_count, 5);
    }

    #[test]
    fn top_attention_point_mass_has_low_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // 30 Continuation samples → posterior should concentrate
        // strongly on Continuation, entropy should be well below max.
        for _ in 0..30 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 1);
        let h = items[0].state_entropy;
        assert!(
            h < 0.5 * MAX_STATE_ENTROPY_NATS,
            "expected low entropy after 30 continuation samples, got {}",
            h
        );
    }

    #[test]
    fn top_attention_orders_descending_by_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);

        // sym_certain: 30 of one variant → low entropy
        let sym_certain = Symbol("C.HK".to_string());
        for _ in 0..30 {
            field.record_state_sample(&sym_certain, PersistentStateKind::Continuation);
        }

        // sym_mixed: 10 each of two variants → moderate entropy
        let sym_mixed = Symbol("M.HK".to_string());
        for _ in 0..10 {
            field.record_state_sample(&sym_mixed, PersistentStateKind::Continuation);
            field.record_state_sample(&sym_mixed, PersistentStateKind::TurningPoint);
        }

        // sym_uniform: 1 each of all 5 variants → highest entropy
        let sym_uniform = Symbol("U.HK".to_string());
        for variant in PERSISTENT_STATE_VARIANTS {
            field.record_state_sample(&sym_uniform, *variant);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 3);
        // Uniform first, mixed second, certain last.
        assert_eq!(items[0].symbol.0, "U.HK");
        assert_eq!(items[1].symbol.0, "M.HK");
        assert_eq!(items[2].symbol.0, "C.HK");
        assert!(items[0].state_entropy > items[1].state_entropy);
        assert!(items[1].state_entropy > items[2].state_entropy);
    }

    #[test]
    fn top_attention_honors_cap() {
        let mut field = PressureBeliefField::new(Market::Hk);
        for i in 0..10 {
            let s = Symbol(format!("S{:02}.HK", i));
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }
        let items = field.top_attention(3);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn format_attention_line_shows_percent_of_max() {
        let item = AttentionItem {
            symbol: Symbol("0700.HK".to_string()),
            state_entropy: 1.43,
            sample_count: 487,
            max_entropy: MAX_STATE_ENTROPY_NATS,
        };
        let line = format_attention_line(&item);
        // 1.43 / 1.6094 ≈ 0.8885 → 89%
        assert_eq!(
            line,
            "attention: 0700.HK state_entropy=1.43 nats (n=487, 89% of max)"
        );
    }

    #[test]
    fn max_entropy_constant_matches_variant_count() {
        let expected = (PERSISTENT_STATE_VARIANTS.len() as f64).ln();
        assert!(
            (MAX_STATE_ENTROPY_NATS - expected).abs() < 1e-9,
            "MAX_STATE_ENTROPY_NATS ({}) drifted from ln(variant_count={}) = {}",
            MAX_STATE_ENTROPY_NATS,
            PERSISTENT_STATE_VARIANTS.len(),
            expected
        );
    }
```

- [ ] **Step 5: Compile + run all tests**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib -q
cargo test --lib -q belief_field
```

Expected: compile clean; 18 tests pass (11 from A1 + 7 new).

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs
git commit -m "$(cat <<'EOF'
feat(belief_field): top_attention + AttentionItem + format_attention_line

Ranks symbols by CategoricalBelief entropy descending. AttentionItem
carries symbol + state_entropy + sample_count + max_entropy (= ln 5).
format_attention_line produces "attention: X state_entropy=V.VV nats
(n=N, PP% of max)" wake shape.

7 unit tests: empty, near-max uniform, low point-mass, descending
ordering, cap, format golden, and a guardrail asserting
MAX_STATE_ENTROPY_NATS stays consistent with PERSISTENT_STATE_VARIANTS
length.

Spec: docs/superpowers/specs/2026-04-19-attention-wake-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: HK runtime integration

**Files:**
- Modify: `src/hk/runtime.rs`

- [ ] **Step 1: Locate insertion point**

Run:
```bash
grep -n "eden::pipeline::decision_ledger::wake_format::format_prior_decisions_line" src/hk/runtime.rs | head -2
```

This gives two line numbers: the first is where `format_prior_decisions_line` is called inside the for-loop over notable beliefs. Find the end of the entire `for notable in belief_field.top_notable_beliefs(5) { ... }` block (its closing `}`).

- [ ] **Step 2: Insert attention loop after notable loop**

Using that location, add immediately after the closing `}` of the notable loop, **before** the `// 60s rescan — picks up new decisions` block:

```rust
                for item in belief_field.top_attention(5) {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(eden::pipeline::belief_field::format_attention_line(&item));
                }
```

Indentation must match the surrounding `for notable in ...` block (which was 16 spaces / 4 levels deep in HK runtime).

- [ ] **Step 3: Compile check**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
```

Expected: both compile clean.

- [ ] **Step 4: Re-run test suites for regression**

Run:
```bash
cargo test --lib -q belief_field
cargo test --lib -q belief_snapshot
cargo test --lib -q decision_ledger
```

Expected: all previously-passing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/hk/runtime.rs
git commit -m "$(cat <<'EOF'
feat(hk): attention wake — top-5 entropy symbols per tick

Inserts a new loop after the belief-notable + prior-decisions wake
loop: for each of the top 5 symbols ranked by state-posterior
entropy, emit an "attention:" wake line. Independent of persistence
feature and independent of decisions tree.

Compiles clean both with and without persistence; all prior tests pass.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: US runtime integration (symmetric)

**Files:**
- Modify: `src/us/runtime.rs`

- [ ] **Step 1: Locate insertion point**

Run:
```bash
grep -n "crate::pipeline::decision_ledger::wake_format::format_prior_decisions_line" src/us/runtime.rs | head -2
```

This gives the US wake emission line. Find the closing `}` of the US `for notable in belief_field.top_notable_beliefs(5) { ... }` block.

- [ ] **Step 2: Insert attention loop**

Add immediately after the closing `}` of the notable loop, **before** the `// 60s rescan — picks up new decisions` block:

```rust
            for item in belief_field.top_attention(5) {
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(crate::pipeline::belief_field::format_attention_line(&item));
            }
```

Note: US runtime uses `crate::` (not `eden::`) for pipeline paths, and its indent level is typically 12 spaces / 3 levels deep — match the surrounding `for notable in ...` block exactly.

- [ ] **Step 3: Compile check**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
```

Expected: both compile clean.

- [ ] **Step 4: Commit**

```bash
git add src/us/runtime.rs
git commit -m "$(cat <<'EOF'
feat(us): attention wake (symmetric)

Mirror of HK attention wake using crate::-prefixed paths.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Integration test + final acceptance

**Files:**
- Modify: `tests/belief_field_integration.rs`

- [ ] **Step 1: Append integration test**

Append to the end of `tests/belief_field_integration.rs`:

```rust
#[test]
fn attention_ranking_survives_snapshot_restore() {
    use eden::pipeline::belief_field::{PERSISTENT_STATE_VARIANTS, MAX_STATE_ENTROPY_NATS};

    let mut field = PressureBeliefField::new(Market::Hk);

    // Three symbols with increasing uncertainty:
    //   C.HK  → low entropy (single variant × 20)
    //   M.HK  → medium entropy (two variants × 10 each)
    //   U.HK  → high entropy (all 5 variants × 1)
    let c = Symbol("C.HK".to_string());
    for _ in 0..20 {
        field.record_state_sample(&c, PersistentStateKind::Continuation);
    }

    let m = Symbol("M.HK".to_string());
    for _ in 0..10 {
        field.record_state_sample(&m, PersistentStateKind::Continuation);
        field.record_state_sample(&m, PersistentStateKind::TurningPoint);
    }

    let u = Symbol("U.HK".to_string());
    for variant in PERSISTENT_STATE_VARIANTS {
        field.record_state_sample(&u, *variant);
    }

    let before = field.top_attention(3);
    assert_eq!(before.len(), 3);
    let before_order: Vec<String> = before.iter().map(|i| i.symbol.0.clone()).collect();

    // Snapshot + restore.
    let snap = serialize_field(&field, chrono::Utc::now());
    let restored = restore_field(&snap).expect("restore ok");

    let after = restored.top_attention(3);
    assert_eq!(after.len(), 3);
    let after_order: Vec<String> = after.iter().map(|i| i.symbol.0.clone()).collect();

    assert_eq!(before_order, after_order, "attention order should survive restart");

    // Also assert entropy values match within f64↔Decimal tolerance.
    for (b, a) in before.iter().zip(after.iter()) {
        assert!(
            (b.state_entropy - a.state_entropy).abs() < 1e-6,
            "entropy drift after restore: {} → {} for {}",
            b.state_entropy,
            a.state_entropy,
            b.symbol.0
        );
        // max_entropy constant is identical on both sides.
        assert!((b.max_entropy - MAX_STATE_ENTROPY_NATS).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: Run integration test**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo test --test belief_field_integration -q
```

Expected: 4 tests pass (3 original + 1 new).

- [ ] **Step 3: Full acceptance run**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
echo "AC1 cargo check persistence:"; cargo check --lib --features persistence -q && echo PASS
echo "AC2 cargo check no-default:"; cargo check --lib --no-default-features -q && echo PASS
echo "AC3 belief_field unit tests (should be 18):"; cargo test --lib -q belief_field 2>&1 | tail -2
echo "AC4 belief_snapshot (should be 7):"; cargo test --lib -q belief_snapshot 2>&1 | tail -2
echo "AC5 decision_ledger (should be 13):"; cargo test --lib -q decision_ledger 2>&1 | tail -2
echo "AC6 integration tests (should be 4):"; cargo test --test belief_field_integration -q 2>&1 | tail -2
```

Expected: all AC lines PASS; counts match.

- [ ] **Step 4: Commit**

```bash
git add tests/belief_field_integration.rs
git commit -m "$(cat <<'EOF'
test(belief_field): attention_ranking_survives_snapshot_restore

Build field with low/medium/high entropy symbols, serialize, restore,
re-rank — assert ordering preserved and entropy values match within
f64↔Decimal roundtrip tolerance (1e-6).

Closes B (MI Attention Wake) implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage:**

| Spec requirement | Task |
|------------------|------|
| `AttentionItem` struct | Task 1 Step 1 |
| `MAX_STATE_ENTROPY_NATS` const | Task 1 Step 1 |
| `top_attention(k)` method | Task 1 Step 2 |
| `format_attention_line` helper | Task 1 Step 3 |
| 7 unit tests | Task 1 Step 4 |
| HK runtime wake loop | Task 2 |
| US runtime wake loop | Task 3 |
| Roundtrip integration test | Task 4 Step 1 |
| AC1 cargo check persistence | Task 4 Step 3 |
| AC2 cargo check no-default | Task 4 Step 3 |
| AC3-5 regression on existing test suites | Task 4 Step 3 |
| AC6 integration test | Task 4 Step 3 |

All spec requirements mapped to tasks.

**Placeholder scan:** No TBDs, TODOs, or "similar to" references. Every step has complete code. Every test has a concrete assertion.

**Type consistency:**
- `AttentionItem { symbol, state_entropy, sample_count, max_entropy }` — consistent across Task 1 definition, Task 1 tests, Task 2/3 format calls, Task 4 integration test
- `MAX_STATE_ENTROPY_NATS` — const consistent across Task 1, 4
- `top_attention(k: usize) -> Vec<AttentionItem>` — consistent signature
- `format_attention_line(item: &AttentionItem) -> String` — consistent signature
- HK uses `eden::pipeline::belief_field::` paths; US uses `crate::pipeline::belief_field::` — matches existing project conventions

No drift.
