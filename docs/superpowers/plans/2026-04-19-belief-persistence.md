# Belief Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Eden a cross-tick memory trace — `PressureBeliefField` that persists beliefs per (symbol, channel) and per-symbol state posterior, snapshots every 60s to SurrealDB, and restores on startup.

**Architecture:** New `pipeline::belief_field` module holds the in-memory field. New `persistence::belief_snapshot` module handles serialize/save/load. SurrealDB migration adds `belief_snapshot` table. HK and US runtimes each own an independent instance (symmetric, not shared).

**Tech Stack:** Rust + SurrealDB + existing `pipeline::belief` primitives (GaussianBelief, CategoricalBelief, Welford).

**Spec:** `docs/superpowers/specs/2026-04-19-belief-persistence-design.md`

---

## File Structure

**New files:**
- `src/pipeline/belief_field.rs` (~400 LOC) — core struct + update + query
- `src/persistence/belief_snapshot.rs` (~250 LOC) — serialize/deserialize + save/load
- `tests/belief_field_integration.rs` (~150 LOC) — restart continuity

**Modified files:**
- `src/pipeline/mod.rs` — `pub mod belief_field;`
- `src/persistence/mod.rs` — `pub mod belief_snapshot;`
- `src/persistence/schema.rs` — add MIGRATION_035 + bump LATEST_SCHEMA_VERSION
- `src/persistence/store.rs` — add `save_belief_snapshot` + `load_latest_belief_snapshot` methods on EdenStore
- `src/hk/runtime.rs` — integrate update + snapshot + restore + wake line
- `src/us/runtime.rs` — symmetric integration

**Concrete types referenced** (verified from codebase):
- `Symbol(String)` — `src/ontology/objects.rs:16`
- `Market::{Hk, Us}` — `src/ontology/objects.rs:20`
- `PressureChannel::{OrderBook, CapitalFlow, Institutional, Momentum, Volume, Structure}` — `src/pipeline/pressure.rs:31`
- `PersistentStateKind::{Continuation, TurningPoint, LowInformation, Conflicted, Latent}` — `src/pipeline/state_engine.rs:76`
- `GaussianBelief`, `CategoricalBelief<K>` — `src/pipeline/belief.rs`
- `LATEST_SCHEMA_VERSION: u32 = 34` — `src/persistence/schema.rs:944` (bump to 35)

---

## Branch

Commits go on current branch (`codex/polymarket-convergence`). Work is additive + contained to listed files.

**Verification commands used throughout:**
- `cargo check --lib -q` — basic compile
- `cargo check --lib --features persistence -q` — with SurrealDB path
- `cargo test --lib -q belief_field` — scoped tests (avoids OOM linker)

---

## Task 1: Scaffold `PressureBeliefField` struct

**Files:**
- Create: `src/pipeline/belief_field.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Create skeleton file**

Create `src/pipeline/belief_field.rs`:

```rust
//! Persistent belief field — Eden's first cross-tick memory trace.
//!
//! Holds per-(symbol, channel) GaussianBelief over pressure values and
//! per-symbol CategoricalBelief over PersistentStateKind. Survives tick
//! to tick via in-memory accumulation, snapshots periodically to SurrealDB
//! via `persistence::belief_snapshot`.
//!
//! See docs/superpowers/specs/2026-04-19-belief-persistence-design.md.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::{CategoricalBelief, GaussianBelief};
use crate::pipeline::pressure::PressureChannel;
use crate::pipeline::state_engine::PersistentStateKind;

/// Key for Gaussian belief map: per (symbol, pressure channel).
pub type GaussianKey = (Symbol, PressureChannel);

/// Persistent belief field — cross-tick state that survives restart via snapshot.
#[derive(Debug, Clone)]
pub struct PressureBeliefField {
    /// Continuous distribution of pressure per (symbol, channel).
    gaussian: HashMap<GaussianKey, GaussianBelief>,

    /// Snapshot of `gaussian` from the previous tick, used for KL-diff
    /// notable detection. Overwritten each tick after `update_from_pressure`.
    previous_gaussian: HashMap<GaussianKey, GaussianBelief>,

    /// Per-symbol posterior over the 5 persistent-state variants.
    categorical: HashMap<Symbol, CategoricalBelief<PersistentStateKind>>,

    /// Previous-tick snapshot of `categorical` for posterior-shift detection.
    previous_categorical: HashMap<Symbol, CategoricalBelief<PersistentStateKind>>,

    /// Which market this field tracks. HK and US each own an independent field.
    market: Market,

    /// Tick of the most recent `update_from_pressure` call. Zero until first update.
    last_tick: u64,

    /// Timestamp of the most recent snapshot write. None until first snapshot.
    last_snapshot_ts: Option<DateTime<Utc>>,
}

impl PressureBeliefField {
    /// Construct an empty field for the given market.
    pub fn new(market: Market) -> Self {
        Self {
            gaussian: HashMap::new(),
            previous_gaussian: HashMap::new(),
            categorical: HashMap::new(),
            previous_categorical: HashMap::new(),
            market,
            last_tick: 0,
            last_snapshot_ts: None,
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn last_tick(&self) -> u64 {
        self.last_tick
    }

    pub fn last_snapshot_ts(&self) -> Option<DateTime<Utc>> {
        self.last_snapshot_ts
    }

    pub fn set_last_snapshot_ts(&mut self, ts: DateTime<Utc>) {
        self.last_snapshot_ts = Some(ts);
    }

    /// Number of (symbol, channel) Gaussian beliefs with at least one sample.
    pub fn gaussian_count(&self) -> usize {
        self.gaussian
            .values()
            .filter(|b| b.sample_count >= 1)
            .count()
    }

    /// Number of symbols with a categorical belief (≥1 state sample).
    pub fn categorical_count(&self) -> usize {
        self.categorical
            .values()
            .filter(|b| b.sample_count() >= 1)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_field_is_empty() {
        let field = PressureBeliefField::new(Market::Hk);
        assert_eq!(field.gaussian_count(), 0);
        assert_eq!(field.categorical_count(), 0);
        assert_eq!(field.last_tick(), 0);
        assert!(field.last_snapshot_ts().is_none());
        assert_eq!(field.market(), Market::Hk);
    }

    #[test]
    fn market_tag_preserved() {
        let hk_field = PressureBeliefField::new(Market::Hk);
        let us_field = PressureBeliefField::new(Market::Us);
        assert_eq!(hk_field.market(), Market::Hk);
        assert_eq!(us_field.market(), Market::Us);
    }
}
```

- [ ] **Step 2: Wire into pipeline mod.rs**

Read `src/pipeline/mod.rs`, find the section listing `pub mod` declarations, add:

```rust
pub mod belief_field;
```

(Add alphabetically between `pub mod belief;` and the next module.)

- [ ] **Step 3: Check CategoricalBelief API for `sample_count()`**

Run:
```bash
grep -n "pub fn sample_count\|sample_count:" src/pipeline/belief.rs | head -5
```

Expected: confirms `sample_count()` method or public field exists on `CategoricalBelief`. If field is pub, `b.sample_count` is correct (no parens). If method, keep parens. **Adapt the field scaffolding code above to match.**

- [ ] **Step 4: Compile check**

Run: `cargo check --lib -q`
Expected: compiles clean (no errors).

- [ ] **Step 5: Run tests**

Run: `cargo test --lib -q belief_field`
Expected: 2 tests pass (`new_field_is_empty`, `market_tag_preserved`).

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs src/pipeline/mod.rs
git commit -m "$(cat <<'EOF'
feat(belief_field): scaffold PressureBeliefField struct

Empty field with market tag, gaussian/categorical HashMaps, and diff
buffers for notable-detection. No update logic yet — structure only.

Spec: docs/superpowers/specs/2026-04-19-belief-persistence-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Gaussian update from pressure field

**Files:**
- Modify: `src/pipeline/belief_field.rs`

- [ ] **Step 1: Inspect PressureField API to determine iteration**

Run:
```bash
grep -n "pub fn\|pub struct PressureField" src/pipeline/pressure.rs | head -30
```

Find methods that expose per-symbol per-channel pressure values. Likely candidates: `nodes()`, `iter_nodes()`, `channels_for(symbol)`, or a `NodePressure` struct containing a channel map. **Record the exact method signature for use below.**

- [ ] **Step 2: Write failing test for update**

Append to `tests` mod in `src/pipeline/belief_field.rs`:

```rust
#[test]
fn update_creates_gaussian_per_channel() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let symbol = Symbol("0700.HK".to_string());

    // Feed a pressure sample on OrderBook channel directly.
    field.record_gaussian_sample(&symbol, PressureChannel::OrderBook, dec!(1.2), 1);

    assert_eq!(field.gaussian_count(), 1);
    let belief = field
        .query_gaussian(&symbol, PressureChannel::OrderBook)
        .expect("belief exists");
    assert_eq!(belief.sample_count, 1);
    assert_eq!(belief.mean, dec!(1.2));
    assert_eq!(field.last_tick(), 1);
}

#[test]
fn update_is_welford_correct_over_multiple_samples() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let symbol = Symbol("0700.HK".to_string());

    for (i, v) in [dec!(1.0), dec!(2.0), dec!(3.0), dec!(4.0), dec!(5.0)].iter().enumerate() {
        field.record_gaussian_sample(&symbol, PressureChannel::OrderBook, *v, (i + 1) as u64);
    }

    let belief = field
        .query_gaussian(&symbol, PressureChannel::OrderBook)
        .unwrap();
    assert_eq!(belief.sample_count, 5);
    assert_eq!(belief.mean, dec!(3.0));
    // Variance of {1..5} = 2.5 (sample variance, divisor n-1)
    assert!((belief.variance.to_string().parse::<f64>().unwrap() - 2.5).abs() < 1e-6);
}
```

- [ ] **Step 3: Run test — expect FAIL (methods not defined)**

Run: `cargo test --lib -q belief_field -- --nocapture`
Expected: compile errors referencing `record_gaussian_sample` and `query_gaussian`.

- [ ] **Step 4: Implement update + query methods**

Add to `impl PressureBeliefField` block in `src/pipeline/belief_field.rs`:

```rust
    /// Record a single pressure observation on a (symbol, channel) belief.
    /// Creates the belief if absent (from_first_sample), otherwise Welford-updates.
    /// Bumps `last_tick` and copies the pre-update belief into `previous_gaussian`
    /// so KL-diff notables can be computed later.
    pub fn record_gaussian_sample(
        &mut self,
        symbol: &Symbol,
        channel: PressureChannel,
        value: rust_decimal::Decimal,
        tick: u64,
    ) {
        let key = (symbol.clone(), channel);

        // Snapshot the pre-update belief for diff.
        if let Some(existing) = self.gaussian.get(&key) {
            self.previous_gaussian.insert(key.clone(), existing.clone());
        }

        self.gaussian
            .entry(key)
            .and_modify(|b| b.update(value))
            .or_insert_with(|| GaussianBelief::from_first_sample(value));

        if tick > self.last_tick {
            self.last_tick = tick;
        }
    }

    /// Read a Gaussian belief for (symbol, channel). Returns None if never
    /// observed.
    pub fn query_gaussian(
        &self,
        symbol: &Symbol,
        channel: PressureChannel,
    ) -> Option<&GaussianBelief> {
        self.gaussian.get(&(symbol.clone(), channel))
    }

    /// Read the previous-tick Gaussian belief for (symbol, channel). Used
    /// by `top_notable_beliefs` to compute KL since last update.
    pub fn query_previous_gaussian(
        &self,
        symbol: &Symbol,
        channel: PressureChannel,
    ) -> Option<&GaussianBelief> {
        self.previous_gaussian.get(&(symbol.clone(), channel))
    }
```

- [ ] **Step 5: Run tests — expect PASS**

Run: `cargo test --lib -q belief_field`
Expected: 4 tests pass (prior 2 + new 2).

- [ ] **Step 6: Add bulk-update helper for runtime integration**

Append to the impl:

```rust
    /// Convenience bulk update: accept an iterator of (symbol, channel, value)
    /// triples and apply them at the given tick. Used from runtime to update
    /// the full pressure field in one call after it's built.
    ///
    /// Caller is responsible for providing all (symbol, channel) pairs —
    /// absence of a pair means "no observation this tick", not "zero".
    pub fn update_from_pressure_samples<I>(&mut self, samples: I, tick: u64)
    where
        I: IntoIterator<Item = (Symbol, PressureChannel, rust_decimal::Decimal)>,
    {
        for (symbol, channel, value) in samples {
            self.record_gaussian_sample(&symbol, channel, value, tick);
        }
    }
```

- [ ] **Step 7: Test bulk update**

Append test:

```rust
#[test]
fn update_from_pressure_samples_processes_all_triples() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let s1 = Symbol("0700.HK".to_string());
    let s2 = Symbol("0005.HK".to_string());

    let samples = vec![
        (s1.clone(), PressureChannel::OrderBook, dec!(1.0)),
        (s1.clone(), PressureChannel::CapitalFlow, dec!(0.5)),
        (s2.clone(), PressureChannel::OrderBook, dec!(-0.3)),
    ];
    field.update_from_pressure_samples(samples, 42);

    assert_eq!(field.gaussian_count(), 3);
    assert_eq!(field.last_tick(), 42);
    assert_eq!(
        field
            .query_gaussian(&s1, PressureChannel::OrderBook)
            .unwrap()
            .mean,
        dec!(1.0)
    );
}
```

- [ ] **Step 8: Run all belief_field tests**

Run: `cargo test --lib -q belief_field`
Expected: 5 tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/pipeline/belief_field.rs
git commit -m "$(cat <<'EOF'
feat(belief_field): Gaussian update + query + bulk helper

record_gaussian_sample creates/updates per-(symbol, channel) belief via
Welford. previous_gaussian snapshots pre-update belief for later KL-diff.
update_from_pressure_samples is the runtime-facing bulk entry point.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Categorical update from state

**Files:**
- Modify: `src/pipeline/belief_field.rs`

- [ ] **Step 1: Verify CategoricalBelief update signature**

Run:
```bash
grep -n "impl.*CategoricalBelief\|pub fn" src/pipeline/belief.rs | grep -i "categorical\|update\|observe" | head -10
```

Expected: find the update/observe method on CategoricalBelief. Record exact name (likely `observe` or `update`). **Adapt code below to match.**

- [ ] **Step 2: Write failing test**

Append to `tests` mod:

```rust
#[test]
fn record_state_creates_categorical_belief() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let symbol = Symbol("0700.HK".to_string());

    field.record_state_sample(&symbol, PersistentStateKind::TurningPoint);

    assert_eq!(field.categorical_count(), 1);
    let cat = field.query_state_posterior(&symbol).expect("belief exists");
    assert_eq!(cat.sample_count(), 1);
}

#[test]
fn state_samples_accumulate_posterior_mass() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let symbol = Symbol("0700.HK".to_string());

    for _ in 0..7 {
        field.record_state_sample(&symbol, PersistentStateKind::Continuation);
    }
    for _ in 0..3 {
        field.record_state_sample(&symbol, PersistentStateKind::TurningPoint);
    }

    let cat = field.query_state_posterior(&symbol).unwrap();
    assert_eq!(cat.sample_count(), 10);
    let p_cont = cat.probability(&PersistentStateKind::Continuation);
    let p_tp = cat.probability(&PersistentStateKind::TurningPoint);
    assert!((p_cont - 0.7).abs() < 1e-6, "continuation={}", p_cont);
    assert!((p_tp - 0.3).abs() < 1e-6, "turning_point={}", p_tp);
}
```

- [ ] **Step 3: Run — expect FAIL (methods undefined)**

Run: `cargo test --lib -q belief_field`
Expected: compile errors for `record_state_sample`, `query_state_posterior`, `cat.probability`.

- [ ] **Step 4: Implement + verify CategoricalBelief.probability exists**

Append to `impl PressureBeliefField`:

```rust
    /// Record a single state observation on the symbol's categorical belief.
    /// Creates the belief if absent, otherwise increments the count on the
    /// matching variant.
    pub fn record_state_sample(
        &mut self,
        symbol: &Symbol,
        state: PersistentStateKind,
    ) {
        // Snapshot the pre-update posterior for diff.
        if let Some(existing) = self.categorical.get(symbol) {
            self.previous_categorical.insert(symbol.clone(), existing.clone());
        }

        self.categorical
            .entry(symbol.clone())
            .and_modify(|c| c.observe(state))
            .or_insert_with(|| {
                let mut c = CategoricalBelief::<PersistentStateKind>::uninformed();
                c.observe(state);
                c
            });
    }

    /// Read the categorical posterior for a symbol.
    pub fn query_state_posterior(
        &self,
        symbol: &Symbol,
    ) -> Option<&CategoricalBelief<PersistentStateKind>> {
        self.categorical.get(symbol)
    }

    /// Read the previous-tick categorical posterior.
    pub fn query_previous_state_posterior(
        &self,
        symbol: &Symbol,
    ) -> Option<&CategoricalBelief<PersistentStateKind>> {
        self.previous_categorical.get(symbol)
    }
```

If `observe` is not the actual method name, substitute whatever the Step 1 grep surfaced. If `CategoricalBelief::uninformed()` does not exist, use whichever constructor builds an empty belief (e.g. `::new()` or `::default()`).

- [ ] **Step 5: Run — expect PASS**

Run: `cargo test --lib -q belief_field`
Expected: 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs
git commit -m "$(cat <<'EOF'
feat(belief_field): Categorical update per symbol over PersistentStateKind

record_state_sample bumps the correct variant; query_state_posterior
returns the 5-class distribution. previous_categorical enables posterior-
shift detection for notable wake lines.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `top_notable_beliefs` for wake surface

**Files:**
- Modify: `src/pipeline/belief_field.rs`

- [ ] **Step 1: Define NotableBelief enum**

Append to `src/pipeline/belief_field.rs` above the tests mod:

```rust
/// One line's worth of notable belief for the wake surface.
/// Produced by `PressureBeliefField::top_notable_beliefs`.
#[derive(Debug, Clone)]
pub enum NotableBelief {
    /// A Gaussian belief moved significantly vs its previous-tick snapshot,
    /// or just crossed the informed threshold.
    Gaussian {
        symbol: Symbol,
        channel: PressureChannel,
        mean: rust_decimal::Decimal,
        variance: rust_decimal::Decimal,
        sample_count: u32,
        kl_since_last: Option<f64>,
        just_became_informed: bool,
    },
    /// A Categorical belief either has significant uncertainty (max < 0.5)
    /// or has shifted significantly from last tick (total |delta| > 0.3).
    Categorical {
        symbol: Symbol,
        distribution: Vec<(PersistentStateKind, f64)>,
        sample_count: u32,
        posterior_shift: Option<f64>,
        max_probability: f64,
    },
}

impl NotableBelief {
    /// Numeric importance used to sort. Higher = more notable.
    /// Gaussian: KL vs previous; newly-informed gets a fixed boost.
    /// Categorical: posterior shift; or uncertainty penalty (1 - max_prob).
    pub fn importance(&self) -> f64 {
        match self {
            NotableBelief::Gaussian {
                kl_since_last,
                just_became_informed,
                ..
            } => {
                let base = kl_since_last.unwrap_or(0.0);
                if *just_became_informed {
                    base + 0.5
                } else {
                    base
                }
            }
            NotableBelief::Categorical {
                posterior_shift,
                max_probability,
                ..
            } => posterior_shift
                .unwrap_or_else(|| 1.0 - max_probability.min(1.0)),
        }
    }
}
```

- [ ] **Step 2: Write failing tests**

Append to tests mod:

```rust
#[test]
fn top_notable_beliefs_returns_significant_kl_movers() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0700.HK".to_string());

    // Build an informed belief (≥5 samples) with tight mean around 1.0.
    for _ in 0..6 {
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
    }
    // Snapshot of previous now captured. Shock it with a very different value.
    field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(5.0), 2);

    let notable = field.top_notable_beliefs(5);
    assert!(!notable.is_empty(), "expected at least one notable belief");
    match &notable[0] {
        NotableBelief::Gaussian {
            symbol, channel, kl_since_last, ..
        } => {
            assert_eq!(symbol.0, "0700.HK");
            assert_eq!(*channel, PressureChannel::OrderBook);
            assert!(kl_since_last.unwrap_or(0.0) > 0.5,
                "expected KL > 0.5, got {:?}", kl_since_last);
        }
        other => panic!("expected Gaussian notable, got {:?}", other),
    }
}

#[test]
fn top_notable_beliefs_skips_uninformed_gaussians() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0005.HK".to_string());

    // Only 2 samples — below BELIEF_INFORMED_MIN_SAMPLES (=5).
    field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
    field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.5), 2);

    let notable = field.top_notable_beliefs(5);
    // Either zero notables, or only categorical ones (we have none). Should be zero.
    assert_eq!(notable.len(), 0);
}

#[test]
fn top_notable_beliefs_reports_posterior_shift() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0700.HK".to_string());

    // Build strong continuation posterior (10 samples).
    for _ in 0..10 {
        field.record_state_sample(&s, PersistentStateKind::Continuation);
    }
    // Now flip to turning_point.
    field.record_state_sample(&s, PersistentStateKind::TurningPoint);

    let notable = field.top_notable_beliefs(5);
    let has_categorical = notable.iter().any(|n| matches!(n, NotableBelief::Categorical { .. }));
    assert!(has_categorical, "expected a categorical notable after state flip");
}

#[test]
fn top_notable_beliefs_honors_cap() {
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    for i in 0..20 {
        let s = Symbol(format!("{:04}.HK", i));
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(10.0), 2);
    }

    let notable = field.top_notable_beliefs(5);
    assert!(notable.len() <= 5);
}
```

- [ ] **Step 3: Run — expect FAIL**

Run: `cargo test --lib -q belief_field`
Expected: compile errors on `top_notable_beliefs` and `NotableBelief` patterns.

- [ ] **Step 4: Implement top_notable_beliefs**

Append to `impl PressureBeliefField`:

```rust
    /// Compute notable beliefs this tick (cap at `k`). Notable criteria:
    ///
    /// - Gaussian: sample_count ≥ BELIEF_INFORMED_MIN_SAMPLES (5) AND
    ///   (KL vs previous-tick > 0.5 OR just crossed the informed threshold).
    /// - Categorical: sample_count ≥ 1 AND
    ///   (posterior_shift > 0.3 OR max probability < 0.5).
    ///
    /// Sorted by `NotableBelief::importance` descending. Used by runtime
    /// to emit wake lines.
    pub fn top_notable_beliefs(&self, k: usize) -> Vec<NotableBelief> {
        use crate::pipeline::belief::BELIEF_INFORMED_MIN_SAMPLES;

        let mut candidates: Vec<NotableBelief> = Vec::new();

        // Gaussians
        for ((symbol, channel), belief) in &self.gaussian {
            if belief.sample_count < BELIEF_INFORMED_MIN_SAMPLES {
                continue;
            }

            let prev = self.previous_gaussian.get(&(symbol.clone(), *channel));

            let just_became_informed = match prev {
                Some(p) => p.sample_count < BELIEF_INFORMED_MIN_SAMPLES,
                None => false,
            };

            let kl_since_last = prev
                .and_then(|p| p.kl_divergence(belief))
                .filter(|kl| kl.is_finite());

            let significant_kl = kl_since_last.map(|kl| kl > 0.5).unwrap_or(false);

            if significant_kl || just_became_informed {
                candidates.push(NotableBelief::Gaussian {
                    symbol: symbol.clone(),
                    channel: *channel,
                    mean: belief.mean,
                    variance: belief.variance,
                    sample_count: belief.sample_count,
                    kl_since_last,
                    just_became_informed,
                });
            }
        }

        // Categoricals
        for (symbol, cat) in &self.categorical {
            if cat.sample_count() == 0 {
                continue;
            }

            let distribution: Vec<(PersistentStateKind, f64)> = [
                PersistentStateKind::Continuation,
                PersistentStateKind::TurningPoint,
                PersistentStateKind::LowInformation,
                PersistentStateKind::Conflicted,
                PersistentStateKind::Latent,
            ]
            .iter()
            .map(|k| (*k, cat.probability(k)))
            .collect();

            let max_probability = distribution
                .iter()
                .map(|(_, p)| *p)
                .fold(0.0_f64, f64::max);

            let posterior_shift = self
                .previous_categorical
                .get(symbol)
                .map(|prev| {
                    distribution
                        .iter()
                        .map(|(k, p_now)| (p_now - prev.probability(k)).abs())
                        .sum::<f64>()
                })
                .filter(|s| s.is_finite());

            let significant_shift = posterior_shift.map(|s| s > 0.3).unwrap_or(false);
            let significant_uncertainty = max_probability < 0.5;

            if significant_shift || significant_uncertainty {
                candidates.push(NotableBelief::Categorical {
                    symbol: symbol.clone(),
                    distribution,
                    sample_count: cat.sample_count(),
                    posterior_shift,
                    max_probability,
                });
            }
        }

        candidates.sort_by(|a, b| {
            b.importance()
                .partial_cmp(&a.importance())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(k);
        candidates
    }
```

- [ ] **Step 5: Run — expect PASS**

Run: `cargo test --lib -q belief_field`
Expected: all tests pass (11 total).

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs
git commit -m "$(cat <<'EOF'
feat(belief_field): top_notable_beliefs for wake surface

NotableBelief enum + importance sort; Gaussian notable requires
informed + significant KL, Categorical notable requires posterior shift
or significant uncertainty. Cap at k=5 for wake.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Snapshot module — types + serialize

**Files:**
- Create: `src/persistence/belief_snapshot.rs`
- Modify: `src/persistence/mod.rs`

- [ ] **Step 1: Create belief_snapshot.rs with types + serialize**

Create `src/persistence/belief_snapshot.rs`:

```rust
//! Serialize/deserialize helpers for PressureBeliefField snapshots.
//!
//! Invariants:
//!   - Only informed beliefs (sample_count >= 1) are written.
//!   - Market tag is stored as a lowercase string ("hk" or "us").
//!   - Channel / State enums round-trip via Debug representation.
//!
//! See docs/superpowers/specs/2026-04-19-belief-persistence-design.md.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::{CategoricalBelief, GaussianBelief};
use crate::pipeline::belief_field::PressureBeliefField;
use crate::pipeline::pressure::PressureChannel;
use crate::pipeline::state_engine::PersistentStateKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSnapshot {
    pub market: String,
    pub snapshot_ts: DateTime<Utc>,
    pub tick: u64,
    pub gaussian: Vec<GaussianSnapshotRow>,
    pub categorical: Vec<CategoricalSnapshotRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianSnapshotRow {
    pub symbol: String,
    pub channel: String,
    pub mean: f64,
    pub variance: f64,
    pub m2: f64,
    pub sample_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoricalSnapshotRow {
    pub symbol: String,
    /// Pairs of (state_name, probability_mass) summing to 1.0. State names
    /// match PersistentStateKind Debug representation.
    pub distribution: Vec<(String, f64)>,
    pub sample_count: u32,
}

fn market_to_str(m: Market) -> &'static str {
    match m {
        Market::Hk => "hk",
        Market::Us => "us",
    }
}

fn channel_to_str(c: PressureChannel) -> &'static str {
    match c {
        PressureChannel::OrderBook => "order_book",
        PressureChannel::CapitalFlow => "capital_flow",
        PressureChannel::Institutional => "institutional",
        PressureChannel::Momentum => "momentum",
        PressureChannel::Volume => "volume",
        PressureChannel::Structure => "structure",
    }
}

fn state_to_str(s: PersistentStateKind) -> &'static str {
    match s {
        PersistentStateKind::Continuation => "continuation",
        PersistentStateKind::TurningPoint => "turning_point",
        PersistentStateKind::LowInformation => "low_information",
        PersistentStateKind::Conflicted => "conflicted",
        PersistentStateKind::Latent => "latent",
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

/// Serialize a field to a snapshot. Only informed (sample_count >= 1) beliefs
/// are written.
pub fn serialize_field(
    field: &PressureBeliefField,
    now: DateTime<Utc>,
) -> BeliefSnapshot {
    let mut gaussian = Vec::new();
    for ((symbol, channel), belief) in field.gaussian_iter() {
        if belief.sample_count == 0 {
            continue;
        }
        gaussian.push(GaussianSnapshotRow {
            symbol: symbol.0.clone(),
            channel: channel_to_str(*channel).to_string(),
            mean: decimal_to_f64(belief.mean),
            variance: decimal_to_f64(belief.variance),
            m2: decimal_to_f64(belief.m2_internal()),
            sample_count: belief.sample_count,
        });
    }

    let mut categorical = Vec::new();
    for (symbol, cat) in field.categorical_iter() {
        if cat.sample_count() == 0 {
            continue;
        }
        let distribution: Vec<(String, f64)> = [
            PersistentStateKind::Continuation,
            PersistentStateKind::TurningPoint,
            PersistentStateKind::LowInformation,
            PersistentStateKind::Conflicted,
            PersistentStateKind::Latent,
        ]
        .iter()
        .map(|k| (state_to_str(*k).to_string(), cat.probability(k)))
        .collect();

        categorical.push(CategoricalSnapshotRow {
            symbol: symbol.0.clone(),
            distribution,
            sample_count: cat.sample_count(),
        });
    }

    BeliefSnapshot {
        market: market_to_str(field.market()).to_string(),
        snapshot_ts: now,
        tick: field.last_tick(),
        gaussian,
        categorical,
    }
}
```

- [ ] **Step 2: Wire into persistence mod.rs**

Read `src/persistence/mod.rs`. Add:

```rust
pub mod belief_snapshot;
```

(in the `pub mod` block, alphabetically).

- [ ] **Step 3: Expose field iterators + m2**

Modify `src/pipeline/belief_field.rs` — append to `impl PressureBeliefField`:

```rust
    /// Iterator over all (key, belief) pairs in the gaussian map.
    /// Used by snapshot serializer.
    pub fn gaussian_iter(
        &self,
    ) -> impl Iterator<Item = (&GaussianKey, &GaussianBelief)> {
        self.gaussian.iter()
    }

    /// Iterator over all (symbol, belief) pairs in the categorical map.
    pub fn categorical_iter(
        &self,
    ) -> impl Iterator<Item = (&Symbol, &CategoricalBelief<PersistentStateKind>)> {
        self.categorical.iter()
    }
```

Modify `src/pipeline/belief.rs` — add method to GaussianBelief to expose m2:

```rust
    /// Internal Welford M2 (sum of squared deviations). Exposed for
    /// serialization roundtrip — callers should not rely on this for
    /// anything else.
    pub fn m2_internal(&self) -> Decimal {
        self.m2
    }
```

And a constructor that restores from raw parts:

```rust
    /// Restore a belief from previously-serialized internal state.
    /// Used by snapshot deserialization only.
    pub fn from_raw(
        mean: Decimal,
        variance: Decimal,
        m2: Decimal,
        sample_count: u32,
    ) -> Self {
        Self {
            mean,
            variance,
            sample_count,
            m2,
        }
    }
```

- [ ] **Step 4: Test — roundtrip preserves informed beliefs**

Append to tests mod in `src/persistence/belief_snapshot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn serialize_skips_uninformed() {
        let mut field = PressureBeliefField::new(Market::Hk);
        // Never write — so no informed beliefs.
        let snap = serialize_field(&field, Utc.timestamp_opt(0, 0).unwrap());
        assert!(snap.gaussian.is_empty());
        assert!(snap.categorical.is_empty());
    }

    #[test]
    fn serialize_writes_informed_gaussians_and_categoricals() {
        let mut field = PressureBeliefField::new(Market::Us);
        let s = Symbol("NVDA.US".to_string());
        for _ in 0..5 {
            field.record_gaussian_sample(&s, PressureChannel::Volume, dec!(2.0), 1);
        }
        field.record_state_sample(&s, PersistentStateKind::Continuation);

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);

        assert_eq!(snap.market, "us");
        assert_eq!(snap.snapshot_ts, now);
        assert_eq!(snap.tick, 1);
        assert_eq!(snap.gaussian.len(), 1);
        assert_eq!(snap.gaussian[0].symbol, "NVDA.US");
        assert_eq!(snap.gaussian[0].channel, "volume");
        assert_eq!(snap.gaussian[0].sample_count, 5);
        assert_eq!(snap.categorical.len(), 1);
        assert_eq!(snap.categorical[0].symbol, "NVDA.US");
    }
}
```

- [ ] **Step 5: Compile + run**

Run: `cargo check --lib -q && cargo test --lib -q belief_snapshot`
Expected: compiles, 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs src/pipeline/belief.rs \
        src/persistence/belief_snapshot.rs src/persistence/mod.rs
git commit -m "$(cat <<'EOF'
feat(belief_snapshot): types + serialize_field writer

BeliefSnapshot + row structs + market/channel/state string converters.
Exposes m2 + from_raw on GaussianBelief for roundtrip. Uninformed beliefs
are skipped at serialize time.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Snapshot module — deserialize + restore

**Files:**
- Modify: `src/persistence/belief_snapshot.rs`

- [ ] **Step 1: Add deserialize**

Append to `src/persistence/belief_snapshot.rs` (above tests mod):

```rust
fn str_to_market(s: &str) -> Option<Market> {
    match s {
        "hk" => Some(Market::Hk),
        "us" => Some(Market::Us),
        _ => None,
    }
}

fn str_to_channel(s: &str) -> Option<PressureChannel> {
    match s {
        "order_book" => Some(PressureChannel::OrderBook),
        "capital_flow" => Some(PressureChannel::CapitalFlow),
        "institutional" => Some(PressureChannel::Institutional),
        "momentum" => Some(PressureChannel::Momentum),
        "volume" => Some(PressureChannel::Volume),
        "structure" => Some(PressureChannel::Structure),
        _ => None,
    }
}

fn str_to_state(s: &str) -> Option<PersistentStateKind> {
    match s {
        "continuation" => Some(PersistentStateKind::Continuation),
        "turning_point" => Some(PersistentStateKind::TurningPoint),
        "low_information" => Some(PersistentStateKind::LowInformation),
        "conflicted" => Some(PersistentStateKind::Conflicted),
        "latent" => Some(PersistentStateKind::Latent),
        _ => None,
    }
}

fn f64_to_decimal(v: f64) -> Decimal {
    Decimal::try_from(v).unwrap_or(Decimal::ZERO)
}

/// Errors surfaced by `restore_field`. All are non-fatal — caller should
/// fall back to an empty field.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("unknown market: {0}")]
    UnknownMarket(String),
    #[error("unknown channel: {0}")]
    UnknownChannel(String),
    #[error("unknown state: {0}")]
    UnknownState(String),
}

/// Reconstruct a PressureBeliefField from a snapshot. Uninformed beliefs
/// in the snapshot are not expected (serialize skipped them) but are
/// handled gracefully.
pub fn restore_field(snap: &BeliefSnapshot) -> Result<PressureBeliefField, RestoreError> {
    let market = str_to_market(&snap.market)
        .ok_or_else(|| RestoreError::UnknownMarket(snap.market.clone()))?;

    let mut field = PressureBeliefField::new(market);

    for row in &snap.gaussian {
        let channel = str_to_channel(&row.channel)
            .ok_or_else(|| RestoreError::UnknownChannel(row.channel.clone()))?;
        let belief = GaussianBelief::from_raw(
            f64_to_decimal(row.mean),
            f64_to_decimal(row.variance),
            f64_to_decimal(row.m2),
            row.sample_count,
        );
        field.insert_gaussian_raw(Symbol(row.symbol.clone()), channel, belief);
    }

    for row in &snap.categorical {
        let mut dist: HashMap<PersistentStateKind, f64> = HashMap::new();
        for (name, mass) in &row.distribution {
            let state = str_to_state(name)
                .ok_or_else(|| RestoreError::UnknownState(name.clone()))?;
            dist.insert(state, *mass);
        }
        let cat = CategoricalBelief::<PersistentStateKind>::from_distribution(
            dist,
            row.sample_count,
        );
        field.insert_categorical_raw(Symbol(row.symbol.clone()), cat);
    }

    field.set_last_tick(snap.tick);
    field.set_last_snapshot_ts(snap.snapshot_ts);

    Ok(field)
}
```

- [ ] **Step 2: Add field raw-insert helpers**

Modify `src/pipeline/belief_field.rs` — append to `impl PressureBeliefField`:

```rust
    /// Raw insert for restore path. Bypasses update logic; used only by
    /// snapshot deserialization.
    pub fn insert_gaussian_raw(
        &mut self,
        symbol: Symbol,
        channel: PressureChannel,
        belief: GaussianBelief,
    ) {
        self.gaussian.insert((symbol, channel), belief);
    }

    /// Raw insert for restore path.
    pub fn insert_categorical_raw(
        &mut self,
        symbol: Symbol,
        belief: CategoricalBelief<PersistentStateKind>,
    ) {
        self.categorical.insert(symbol, belief);
    }

    /// Set last_tick from snapshot metadata during restore.
    pub fn set_last_tick(&mut self, tick: u64) {
        self.last_tick = tick;
    }
```

- [ ] **Step 3: Add CategoricalBelief::from_distribution**

Modify `src/pipeline/belief.rs` — add method to CategoricalBelief impl:

```rust
    /// Restore a categorical belief from a probability map + sample count.
    /// Used by snapshot deserialization only.
    pub fn from_distribution(
        probabilities: std::collections::HashMap<K, f64>,
        sample_count: u32,
    ) -> Self {
        // Assumes K: Hash + Eq + Copy + and whatever the original CategoricalBelief
        // type constraints are. Adapt signature if CategoricalBelief takes
        // different generics.
        CategoricalBelief::from_raw_probabilities(probabilities, sample_count)
    }
```

**Before coding**, check existing constructors. Run:

```bash
grep -n "impl.*CategoricalBelief\|pub fn new\|pub fn from" src/pipeline/belief.rs | head -10
```

If `from_raw_probabilities` does not exist, add a similarly-named constructor that takes the distribution map and sample_count and initializes internal fields directly. Adapt the code above.

- [ ] **Step 4: Roundtrip test**

Append to tests mod in `src/persistence/belief_snapshot.rs`:

```rust
    #[test]
    fn roundtrip_preserves_gaussian_welford() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        for v in [dec!(1.0), dec!(2.0), dec!(3.0), dec!(4.0), dec!(5.0)] {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, v, 1);
        }

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        let restored = restore_field(&snap).expect("restore ok");

        let orig = field
            .query_gaussian(&s, PressureChannel::OrderBook)
            .unwrap();
        let again = restored
            .query_gaussian(&s, PressureChannel::OrderBook)
            .unwrap();

        assert_eq!(orig.sample_count, again.sample_count);
        assert_eq!(orig.mean, again.mean);
        // Variance may differ in the last decimal due to f64↔Decimal rounding;
        // allow 1e-6 tolerance.
        let d: f64 = (orig.variance - again.variance).abs().try_into().unwrap_or(0.0);
        assert!(d < 1e-6, "variance drift {}", d);
    }

    #[test]
    fn roundtrip_preserves_categorical_distribution() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        for _ in 0..7 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }
        for _ in 0..3 {
            field.record_state_sample(&s, PersistentStateKind::TurningPoint);
        }

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        let restored = restore_field(&snap).expect("restore ok");

        let again = restored.query_state_posterior(&s).unwrap();
        assert_eq!(again.sample_count(), 10);
        assert!((again.probability(&PersistentStateKind::Continuation) - 0.7).abs() < 1e-6);
        assert!((again.probability(&PersistentStateKind::TurningPoint) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn restore_on_bad_market_returns_err() {
        let snap = BeliefSnapshot {
            market: "bad".to_string(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            tick: 0,
            gaussian: vec![],
            categorical: vec![],
        };
        assert!(matches!(restore_field(&snap), Err(RestoreError::UnknownMarket(_))));
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib -q belief_snapshot`
Expected: 5 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/belief_field.rs src/pipeline/belief.rs \
        src/persistence/belief_snapshot.rs
git commit -m "$(cat <<'EOF'
feat(belief_snapshot): restore_field + raw-insert helpers

Reconstruct PressureBeliefField from a BeliefSnapshot. Roundtrip tests
verify Welford-correctness (±1e-6 on variance) and categorical posterior.
restore_field returns RestoreError on unknown market/channel/state;
caller falls back to empty field.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: SurrealDB migration + EdenStore methods

**Files:**
- Modify: `src/persistence/schema.rs`
- Modify: `src/persistence/store.rs`

- [ ] **Step 1: Add migration constant + entry + bump LATEST_SCHEMA_VERSION**

Edit `src/persistence/schema.rs`:

1. Near the other `const MIGRATION_NNN` blocks, append:

```rust
const MIGRATION_035: &str = r#"
-- Belief snapshot table: periodic PressureBeliefField snapshots.
DEFINE TABLE belief_snapshot SCHEMAFULL;
DEFINE FIELD market ON belief_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON belief_snapshot TYPE datetime;
DEFINE FIELD tick ON belief_snapshot TYPE int;
DEFINE FIELD gaussian ON belief_snapshot TYPE array;
DEFINE FIELD categorical ON belief_snapshot TYPE array;
DEFINE INDEX idx_belief_market_ts ON belief_snapshot FIELDS market, snapshot_ts;
"#;
```

2. Find `pub const LATEST_SCHEMA_VERSION: u32 = 34;` (line ~944). Change to:

```rust
pub const LATEST_SCHEMA_VERSION: u32 = 35;
```

3. At the end of the `static MIGRATIONS: &[SchemaMigration]` array (after the v34 entry), add:

```rust
    SchemaMigration {
        version: 35,
        name: "belief_snapshot_table",
        statements: MIGRATION_035,
    },
```

- [ ] **Step 2: Verify existing migration tests still pass**

Run: `cargo test --lib -q schema::tests`
Expected: tests pass, `LATEST_SCHEMA_VERSION` = 35 respected.

- [ ] **Step 3: Add save/load methods on EdenStore**

Read `src/persistence/store.rs` to find existing method patterns (e.g. `save_tick_record`).

Locate the `impl EdenStore` block that contains tick-record-style persistence methods. Append:

```rust
    /// Persist a belief snapshot. Returns Ok(()) on success, Err on any
    /// SurrealDB failure. Caller should log-and-continue on Err (belief
    /// snapshots are not golden data).
    pub async fn save_belief_snapshot(
        &self,
        snapshot: &crate::persistence::belief_snapshot::BeliefSnapshot,
    ) -> Result<(), crate::persistence::PersistenceError> {
        let _: Vec<crate::persistence::belief_snapshot::BeliefSnapshot> = self
            .db()
            .create("belief_snapshot")
            .content(snapshot.clone())
            .await
            .map_err(|e| crate::persistence::PersistenceError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Load the most recent belief snapshot for the given market, or None
    /// if no snapshot exists. Returns Err on SurrealDB failure; caller
    /// should log-and-continue.
    pub async fn load_latest_belief_snapshot(
        &self,
        market: &str,
    ) -> Result<Option<crate::persistence::belief_snapshot::BeliefSnapshot>, crate::persistence::PersistenceError> {
        let mut result = self
            .db()
            .query(
                "SELECT * FROM belief_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC \
                 LIMIT 1",
            )
            .bind(("market", market.to_string()))
            .await
            .map_err(|e| crate::persistence::PersistenceError::Backend(e.to_string()))?;

        let snaps: Vec<crate::persistence::belief_snapshot::BeliefSnapshot> = result
            .take(0)
            .map_err(|e| crate::persistence::PersistenceError::Backend(e.to_string()))?;

        Ok(snaps.into_iter().next())
    }
```

**Adapt to local EdenStore API**: if the existing pattern uses different method names (`db().select()` vs `.query()`, a different `PersistenceError` variant, etc.), match the local style. Grep first:

```bash
grep -n "pub async fn save_\|pub async fn load_\|db().query\|db().select" src/persistence/store.rs | head -20
```

- [ ] **Step 4: Compile check**

Run: `cargo check --lib --features persistence -q`
Expected: compiles clean.

- [ ] **Step 5: Commit**

```bash
git add src/persistence/schema.rs src/persistence/store.rs
git commit -m "$(cat <<'EOF'
feat(persistence): MIGRATION_035 belief_snapshot + EdenStore methods

New SurrealDB table for PressureBeliefField snapshots (append-only,
indexed on (market, snapshot_ts)). EdenStore gains save_belief_snapshot
and load_latest_belief_snapshot — both async, both Result-returning so
callers can log-and-continue on failure.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: HK runtime integration

**Files:**
- Modify: `src/hk/runtime.rs`

- [ ] **Step 1: Locate tick loop structure + startup**

Run:
```bash
grep -n "pub async fn run\|let pressure_field\|pressure_field =\|pressure\.\w*(" src/hk/runtime.rs | head -20
```

Find: (a) the startup function that initializes runtime state, (b) the per-tick location where pressure_field is built, (c) the wake-output code block. Record line numbers for each.

- [ ] **Step 2: Add PressureBeliefField to runtime state**

In `src/hk/runtime.rs`, find the struct holding HK runtime state (e.g. `HkRuntime` / similar). Add field:

```rust
belief_field: PressureBeliefField,
```

and import at top of file:

```rust
use crate::pipeline::belief_field::PressureBeliefField;
use crate::persistence::belief_snapshot::{serialize_field, restore_field};
use crate::ontology::objects::Market;
use chrono::Utc;
```

In the constructor/initializer, wire it:

```rust
let belief_field = match store.as_ref() {
    Some(s) => {
        match s.load_latest_belief_snapshot("hk").await {
            Ok(Some(snap)) => match restore_field(&snap) {
                Ok(field) => {
                    tracing::info!(
                        target: "belief",
                        restored_gaussian = field.gaussian_count(),
                        restored_categorical = field.categorical_count(),
                        ts = %snap.snapshot_ts,
                        "restored belief field from snapshot"
                    );
                    field
                }
                Err(e) => {
                    tracing::warn!(target: "belief", err = %e, "restore failed; starting fresh");
                    PressureBeliefField::new(Market::Hk)
                }
            },
            Ok(None) => {
                tracing::info!(target: "belief", "no prior snapshot; starting with uninformed prior");
                PressureBeliefField::new(Market::Hk)
            }
            Err(e) => {
                tracing::warn!(target: "belief", err = %e, "snapshot load failed; starting fresh");
                PressureBeliefField::new(Market::Hk)
            }
        }
    }
    None => PressureBeliefField::new(Market::Hk),
};
```

Adapt `store.as_ref()` to whatever the local pattern is (may be `self.store`, `self.persistence`, etc).

- [ ] **Step 3: Wire per-tick update**

In the tick-processing function, immediately after `pressure_field` is built (Step 1 located it), add:

```rust
// Update belief field from the freshly-built pressure field.
let gaussian_samples: Vec<(Symbol, PressureChannel, rust_decimal::Decimal)> =
    pressure_field.iter_pressures().map(|(symbol, channel, value)| {
        (symbol.clone(), channel, value)
    }).collect();
self.belief_field.update_from_pressure_samples(gaussian_samples, tick_seq);

// Update categorical from state engine.
for (symbol, state) in self.state_engine.iter_current_states() {
    self.belief_field.record_state_sample(symbol, state);
}
```

**Note**: `pressure_field.iter_pressures()` and `self.state_engine.iter_current_states()` are hypothetical method names; adapt to the actual iteration APIs. If no such iterator exists on `PressureField`, add one:

```rust
// In src/pipeline/pressure.rs, append to impl PressureField:
pub fn iter_pressures(&self) -> impl Iterator<Item = (&Symbol, PressureChannel, Decimal)> + '_ {
    self.nodes.iter().flat_map(|(symbol, node)| {
        PressureChannel::ALL.iter().filter_map(move |channel| {
            let channel_pressure = node.channel(*channel)?;
            let value = channel_pressure.net();
            Some((symbol, *channel, value))
        })
    })
}
```

(Similarly for state engine — add `iter_current_states()` if it doesn't exist, returning `impl Iterator<Item = (&Symbol, PersistentStateKind)>`.)

- [ ] **Step 4: Wire snapshot cadence (60s)**

After the update calls, add:

```rust
let snapshot_due = match self.belief_field.last_snapshot_ts() {
    None => true,
    Some(prev) => (Utc::now() - prev).num_seconds() >= 60,
};
if snapshot_due {
    let now = Utc::now();
    let snap = serialize_field(&self.belief_field, now);
    if let Some(store) = self.store.as_ref() {
        let snap_cloned = snap.clone();
        let store_handle = store.clone();
        // Write async, don't block tick.
        tokio::spawn(async move {
            if let Err(e) = store_handle.save_belief_snapshot(&snap_cloned).await {
                tracing::warn!(target: "belief", err = %e, "snapshot write failed");
            }
        });
    }
    self.belief_field.set_last_snapshot_ts(now);
    tracing::info!(
        target: "belief",
        gaussian = snap.gaussian.len(),
        categorical = snap.categorical.len(),
        "snapshot written"
    );
}
```

Adapt `store.clone()` — EdenStore is likely `Arc<...>` internally so clone is cheap. If store is held differently, adjust.

- [ ] **Step 5: Emit belief wake lines**

Find the wake-output section. Add, before the wake-flush call:

```rust
for notable in self.belief_field.top_notable_beliefs(5) {
    wake_lines.push(format_belief_wake_line(&notable));
}
```

Add a helper at bottom of file or inline:

```rust
fn format_belief_wake_line(n: &crate::pipeline::belief_field::NotableBelief) -> String {
    use crate::pipeline::belief_field::NotableBelief;
    use crate::pipeline::belief::BELIEF_INFORMED_MIN_SAMPLES;
    match n {
        NotableBelief::Gaussian {
            symbol, channel, mean, variance, sample_count, kl_since_last, just_became_informed,
        } => {
            let ch = match channel {
                crate::pipeline::pressure::PressureChannel::OrderBook => "orderbook",
                crate::pipeline::pressure::PressureChannel::CapitalFlow => "capital_flow",
                crate::pipeline::pressure::PressureChannel::Institutional => "institutional",
                crate::pipeline::pressure::PressureChannel::Momentum => "momentum",
                crate::pipeline::pressure::PressureChannel::Volume => "volume",
                crate::pipeline::pressure::PressureChannel::Structure => "structure",
            };
            let status = if *sample_count >= BELIEF_INFORMED_MIN_SAMPLES {
                "informed"
            } else {
                "prior-heavy"
            };
            let kl_part = kl_since_last
                .map(|kl| format!(" (KL vs prev={:.2})", kl))
                .unwrap_or_else(|| {
                    if *just_became_informed {
                        " (just informed)".to_string()
                    } else {
                        String::new()
                    }
                });
            format!(
                "belief: {} {} μ={:.3} σ²={:.3} n={} {}{}",
                symbol.0, ch, mean, variance, sample_count, status, kl_part
            )
        }
        NotableBelief::Categorical { symbol, distribution, sample_count, .. } => {
            let top3: Vec<String> = {
                let mut d = distribution.clone();
                d.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                d.into_iter().take(3).map(|(k, p)| format!("{:?}={:.2}", k, p)).collect()
            };
            format!(
                "belief: {} state_posterior {} (n={})",
                symbol.0,
                top3.join(", "),
                sample_count
            )
        }
    }
}
```

- [ ] **Step 6: Compile check**

Run: `cargo check --lib --features persistence -q`
Expected: compiles clean. Fix any type mismatches between helper/method signatures and actual ones.

- [ ] **Step 7: Commit**

```bash
git add src/hk/runtime.rs src/pipeline/pressure.rs src/pipeline/state_engine.rs
git commit -m "$(cat <<'EOF'
feat(hk): integrate PressureBeliefField into HK tick loop

Startup restores latest snapshot or creates fresh field. Per-tick update
after pressure field build + state classification. Snapshot written every
60s via tokio::spawn (non-blocking). New belief: wake lines, top 5 cap.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: US runtime integration (symmetric)

**Files:**
- Modify: `src/us/runtime.rs`

- [ ] **Step 1: Mirror HK integration**

Apply the same changes from Task 8 to `src/us/runtime.rs`:

1. Add `belief_field: PressureBeliefField` to the US runtime state struct
2. At startup, load/restore using `"us"` instead of `"hk"`:

```rust
match s.load_latest_belief_snapshot("us").await { ... }
```

and `Market::Us` instead of `Market::Hk`.

3. Wire per-tick update using US pressure field + US state engine iterators (US likely has its own equivalent APIs — adapt names as needed).

4. Wire snapshot cadence (60s) — identical logic, `"us"` market tag flows in automatically via `serialize_field` reading `field.market()`.

5. Emit `belief:` wake lines via the same `format_belief_wake_line` helper.

**Before editing**, grep to find US runtime's state struct + tick function:

```bash
grep -n "pub struct UsRuntime\|pub async fn run\|pressure_field" src/us/runtime.rs | head -10
```

- [ ] **Step 2: Consolidate format_belief_wake_line if duplicated**

If `format_belief_wake_line` was inlined into `hk/runtime.rs`, move it to `src/pipeline/belief_field.rs` as `pub fn format_wake_line(n: &NotableBelief) -> String` so both runtimes share it. Update HK import accordingly.

- [ ] **Step 3: Compile check**

Run: `cargo check --lib --features persistence -q`
Expected: compiles clean.

- [ ] **Step 4: Verify no-persistence path still compiles**

Run: `cargo check --lib -q`
Expected: compiles clean. Belief-related code should be `#[cfg(feature = "persistence")]`-gated if the store isn't available without it; otherwise the field should work as a no-op. Use the pattern used by existing persistence-sensitive code.

- [ ] **Step 5: Commit**

```bash
git add src/us/runtime.rs src/pipeline/belief_field.rs src/hk/runtime.rs
git commit -m "$(cat <<'EOF'
feat(us): integrate PressureBeliefField into US tick loop (symmetric)

Mirrors HK integration with Market::Us + "us" snapshot key. Shared
format_wake_line helper lives in belief_field module.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Integration test — restart continuity

**Files:**
- Create: `tests/belief_field_integration.rs`

- [ ] **Step 1: Write the integration test**

Create `tests/belief_field_integration.rs`:

```rust
//! Integration test: belief field survives serialize → restore roundtrip
//! and continues accumulating correctly across the "restart" boundary.
//!
//! Does NOT exercise SurrealDB — that's an integration vs disk concern
//! covered in the live runtime. This test isolates the belief layer.

use chrono::{TimeZone, Utc};
use eden::ontology::objects::{Market, Symbol};
use eden::persistence::belief_snapshot::{restore_field, serialize_field};
use eden::pipeline::belief_field::PressureBeliefField;
use eden::pipeline::pressure::PressureChannel;
use eden::pipeline::state_engine::PersistentStateKind;
use rust_decimal_macros::dec;

#[test]
fn belief_field_survives_snapshot_restore_continues_to_accumulate() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0700.HK".to_string());

    // Session 1: 100 samples.
    for i in 1..=100 {
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), i);
        if i % 10 == 0 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }
    }

    let before_g = field
        .query_gaussian(&s, PressureChannel::OrderBook)
        .cloned();
    let before_c_count = field
        .query_state_posterior(&s)
        .map(|c| c.sample_count())
        .unwrap_or(0);

    // Snapshot + restore (simulates process restart).
    let snap = serialize_field(&field, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    let mut restored = restore_field(&snap).expect("restore ok");

    // Session 2: 100 more samples on the restored field.
    for i in 101..=200 {
        restored.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), i);
        if i % 10 == 0 {
            restored.record_state_sample(&s, PersistentStateKind::Continuation);
        }
    }

    // Verify continuity:
    let after_g = restored
        .query_gaussian(&s, PressureChannel::OrderBook)
        .unwrap();
    assert_eq!(
        after_g.sample_count,
        200,
        "gaussian accumulated across restart: before={:?}, after={}",
        before_g.as_ref().map(|b| b.sample_count),
        after_g.sample_count
    );

    let after_c = restored.query_state_posterior(&s).unwrap();
    assert_eq!(
        after_c.sample_count(),
        before_c_count + 10,
        "categorical accumulated across restart"
    );
}

#[test]
fn hk_and_us_snapshots_are_independent() {
    let mut hk = PressureBeliefField::new(Market::Hk);
    let mut us = PressureBeliefField::new(Market::Us);

    let hk_sym = Symbol("0700.HK".to_string());
    let us_sym = Symbol("NVDA.US".to_string());

    for _ in 0..6 {
        hk.record_gaussian_sample(&hk_sym, PressureChannel::OrderBook, dec!(1.0), 1);
        us.record_gaussian_sample(&us_sym, PressureChannel::Volume, dec!(2.0), 1);
    }

    let hk_snap = serialize_field(&hk, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    let us_snap = serialize_field(&us, Utc.timestamp_opt(1_700_000_000, 0).unwrap());

    assert_eq!(hk_snap.market, "hk");
    assert_eq!(us_snap.market, "us");
    assert_eq!(hk_snap.gaussian.len(), 1);
    assert_eq!(us_snap.gaussian.len(), 1);
    assert_eq!(hk_snap.gaussian[0].symbol, "0700.HK");
    assert_eq!(us_snap.gaussian[0].symbol, "NVDA.US");

    // Cross-restore must fail at market check.
    let mut us_as_hk = us_snap.clone();
    us_as_hk.market = "hk".to_string(); // force
    let restored = restore_field(&us_as_hk).expect("hk restore ok");
    assert_eq!(restored.market(), Market::Hk);
    // But symbol content crosses — that's fine; markets are just tags.
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test belief_field_integration -q`
Expected: 2 tests pass.

If this fails with linker OOM (known issue on this machine per CLAUDE.md), instead run:

```bash
cargo test --lib -q belief_field_integration
```

and accept that linker OOM may require moving the test into the crate tests module if it doesn't complete.

- [ ] **Step 3: Commit**

```bash
git add tests/belief_field_integration.rs
git commit -m "$(cat <<'EOF'
test(belief_field): restart continuity + HK/US independence

Integration test proves a field accumulates across serialize→restore
boundary with correct sample_count. Second test verifies HK/US fields
produce isolated snapshots with correct market tag.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Acceptance + wrap up

- [ ] **Step 1: Library compiles with and without persistence**

Run both:
```bash
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
```

Expected: both compile clean.

- [ ] **Step 2: All new unit tests pass**

Run:
```bash
cargo test --lib -q belief_field
cargo test --lib -q belief_snapshot
cargo test --lib -q schema::tests
```

Expected: all tests pass. Tally: ≥ 8 new unit tests (2 + 3 + 1 + 4 across tasks 1-4) + 2 snapshot serialize + 3 snapshot restore + 2 integration.

- [ ] **Step 3: Integration test passes**

Run: `cargo test --test belief_field_integration -q`
Expected: 2 tests pass.

- [ ] **Step 4: Verify acceptance criteria from spec**

Check each AC from `docs/superpowers/specs/2026-04-19-belief-persistence-design.md § 驗證`:

```bash
echo "AC1 cargo check --features persistence: "; cargo check --lib --features persistence -q && echo PASS
echo "AC2 new unit tests (≥8): "; cargo test --lib -q belief_field belief_snapshot 2>&1 | tail -1
echo "AC3 integration test: "; cargo test --test belief_field_integration -q 2>&1 | tail -1
echo "AC4 AC5 require live run — manual"
echo "AC6 AC7 require Eden runtime — manual"
echo "AC8 cargo check --no-default-features: "; cargo check --lib --no-default-features -q && echo PASS
```

Expected: AC1/AC2/AC3/AC8 automated PASS. AC4-7 (live runtime behavior) must be manually verified by running HK/US Eden binary, observing `belief:` wake lines, stopping/restarting, and confirming `[belief] restored N beliefs` log.

- [ ] **Step 5: Benchmark tick latency impact (advisory, not blocking)**

Create a micro-benchmark using existing bench harness or the simplest approach — time N iterations:

```rust
// In a temporary file tests/belief_field_bench.rs (or just run inline with criterion if available):
#[test]
#[ignore] // advisory only, run with --include-ignored
fn benchmark_update_from_pressure_samples_latency() {
    use std::time::Instant;
    use eden::ontology::objects::{Market, Symbol};
    use eden::pipeline::belief_field::PressureBeliefField;
    use eden::pipeline::pressure::PressureChannel;
    use rust_decimal_macros::dec;

    let mut field = PressureBeliefField::new(Market::Hk);
    let symbols: Vec<Symbol> = (0..1150).map(|i| Symbol(format!("{:04}.HK", i))).collect();

    let samples: Vec<_> = symbols
        .iter()
        .flat_map(|s| {
            [PressureChannel::OrderBook, PressureChannel::Volume, PressureChannel::CapitalFlow]
                .iter()
                .map(move |ch| (s.clone(), *ch, dec!(1.0)))
        })
        .collect();

    // Warm up.
    field.update_from_pressure_samples(samples.clone(), 1);

    let start = Instant::now();
    for i in 2..=100 {
        field.update_from_pressure_samples(samples.clone(), i);
    }
    let elapsed = start.elapsed();
    let per_tick = elapsed / 99;
    println!("per-tick avg: {:?}", per_tick);
    assert!(
        per_tick.as_millis() < 10,
        "per-tick belief update took {:?}, expected < 10ms",
        per_tick
    );
}
```

Run: `cargo test --test belief_field_bench -- --ignored --nocapture`
Expected: per-tick average prints, is < 10ms. If higher, document in commit message — doesn't block merge but worth noting.

- [ ] **Step 6: Commit bench (if kept) + final summary**

If benchmark file was kept:

```bash
git add tests/belief_field_bench.rs
git commit -m "test(belief_field): advisory microbenchmark, 1150×3 samples"
```

- [ ] **Step 7: Print final summary**

Print to stdout:

```
✓ Belief Persistence Implementation Plan complete
  - src/pipeline/belief_field.rs (new, ~400 LOC)
  - src/persistence/belief_snapshot.rs (new, ~250 LOC)
  - src/persistence/schema.rs (MIGRATION_035 added, v34→v35)
  - src/persistence/store.rs (2 methods added)
  - src/hk/runtime.rs (belief integration + wake + snapshot + restore)
  - src/us/runtime.rs (symmetric)
  - tests/belief_field_integration.rs (new, 2 tests)

Acceptance:
  - cargo check --lib --features persistence: PASS
  - cargo check --lib --no-default-features: PASS
  - unit tests (≥11): PASS
  - integration test: PASS
  - benchmark (advisory): per-tick < 10ms

Manual verification remaining:
  - AC4: Tick latency <5% increase vs baseline (run HK session pre/post)
  - AC5: Wake lines appear (tail .run/eden-hk.log for "belief:")
  - AC6: Restart restores (stop, wait 70s, restart, log has [belief] restored)
  - AC7: belief_snapshot table gets rows (surrealdb query)

Next spec: 2026-04-20-decisions-ingestor-design.md (A2)
```

---

## Self-Review

**Spec coverage:**

| Spec requirement | Task |
|------------------|------|
| `PressureBeliefField` struct (new module) | Task 1 |
| Gaussian per (symbol, channel) | Task 2 |
| Categorical per symbol | Task 3 |
| previous_gaussian / previous_categorical diff buffers | Task 2, 3 |
| top_notable_beliefs | Task 4 |
| BeliefSnapshot types | Task 5 |
| serialize_field (skip uninformed) | Task 5 |
| restore_field (graceful degrade via Result) | Task 6 |
| GaussianBelief::from_raw, m2_internal | Task 5 / 6 |
| CategoricalBelief::from_distribution | Task 6 |
| MIGRATION_035 belief_snapshot table | Task 7 |
| EdenStore::save_belief_snapshot | Task 7 |
| EdenStore::load_latest_belief_snapshot | Task 7 |
| HK runtime: state field | Task 8 |
| HK runtime: per-tick update | Task 8 |
| HK runtime: 60s snapshot cadence (async) | Task 8 |
| HK runtime: restore on startup | Task 8 |
| HK runtime: belief: wake lines | Task 8 |
| US runtime: symmetric | Task 9 |
| Integration test: restart continuity | Task 10 |
| Integration test: HK/US independence | Task 10 |
| AC1: cargo check --features persistence | Task 11 |
| AC2: unit tests ≥ 8 | Task 11 |
| AC3: integration test | Task 11 |
| AC4: benchmark <5% increase | Task 11 (advisory bench) |
| AC5: wake lines present | Task 8, 9 (manual verify Task 11) |
| AC6: restart restores | Task 8 (manual verify Task 11) |
| AC7: belief_snapshot rows | Task 7 (manual verify Task 11) |
| AC8: cargo check --no-default-features | Task 11 |

All spec requirements have tasks.

**Placeholder scan:**

Searched for TBD / TODO / "similar to" / missing code blocks. Two minor areas flagged explicit adaptation hooks (not placeholders):

- Task 2 Step 1: "Record the exact method signature for use below" — this is legitimate because PressureField iteration API varies; providing a fallback method definition in Task 8 Step 3
- Task 3 Step 4 and Task 6 Step 3: "If `observe` is not the actual method name" / "If `from_raw_probabilities` does not exist" — similar legitimate adaptation to actual CategoricalBelief API

These are not TBDs — they tell the engineer to grep and substitute the real name, and provide a fallback implementation strategy. Kept as-is.

**Type consistency:**

- `Symbol(pub String)` — used consistently (never `SymbolId`)
- `PressureChannel` — used consistently (never `ChannelKind` which was in earlier spec drafts)
- `PersistentStateKind` — used consistently (never `StateKind`)
- `Market::{Hk, Us}` — used consistently
- `GaussianBelief::from_raw(mean, variance, m2, sample_count)` — consistent signature across Task 5 definition and Task 6 restoration
- `CategoricalBelief::from_distribution(map, sample_count)` — same
- `record_gaussian_sample(&mut self, &Symbol, PressureChannel, Decimal, u64)` — same signature in Task 2 definition, Task 4 tests, Task 10 integration test
- `serialize_field(field, now)` and `restore_field(snap)` — consistent

No type or signature drift detected.
