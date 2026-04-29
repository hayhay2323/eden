# Temporal Ring Buffer + SurrealDB Persistence

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Eden temporal memory — store recent tick history in a ring buffer so signals have delta/acceleration/duration, and persist tick data to SurrealDB so entity behavior can be analyzed across sessions.

**Architecture:** A `TickHistory` ring buffer (VecDeque, capacity ~300 ticks = ~10 min at 2s debounce) stores per-tick `TickRecord` structs containing convergence scores, dimension vectors, depth profiles, and trade activity. A `TemporalAnalysis` module computes deltas, acceleration, and duration from the history. SurrealDB stores tick records and institution entity state for cross-session analysis. The pipeline in main.rs feeds the buffer after each tick and passes temporal analysis to display.

**Tech Stack:** Rust, VecDeque (ring buffer), SurrealDB (`surrealdb` crate with RocksDB backend), `serde` for serialization

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/temporal/mod.rs` | Create | Module declaration |
| `src/temporal/buffer.rs` | Create | `TickHistory` ring buffer — stores `TickRecord`, provides windowed queries |
| `src/temporal/analysis.rs` | Create | `TemporalAnalysis` — computes signal delta, acceleration, duration from buffer |
| `src/temporal/record.rs` | Create | `TickRecord` struct — snapshot of one tick's key signals |
| `src/persistence/mod.rs` | Create | Module declaration |
| `src/persistence/store.rs` | Create | `EdenStore` — SurrealDB connection, write/query tick records |
| `src/persistence/schema.rs` | Create | Table definitions and migrations |
| `src/lib.rs` | Modify | Add `pub mod temporal; pub mod persistence;` |
| `src/main.rs` | Modify | Feed tick history, display temporal signals, init SurrealDB |
| `src/ontology/objects.rs` | Modify | Add `Serialize, Deserialize` derives to `Symbol` |
| `Cargo.toml` | Modify | Add `surrealdb`, add `serde` feature to `time` |

---

## Task 1: TickRecord — The Tick Snapshot Struct

**Files:**
- Create: `src/temporal/mod.rs`
- Create: `src/temporal/record.rs`
- Modify: `src/lib.rs`
- Modify: `src/ontology/objects.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Prerequisites — Serde derives and time feature**

Add `Serialize, Deserialize` to `Symbol` in `src/ontology/objects.rs`:
```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub String);
```

Update `time` in `Cargo.toml` to enable serde:
```toml
time = { version = "0.3", features = ["serde"] }
```

- [ ] **Step 2: Create module structure**

`src/temporal/mod.rs` (only declare record for now; buffer and analysis added in their tasks):
```rust
pub mod record;
```

Add to `src/lib.rs`:
```rust
pub mod temporal;
```

- [ ] **Step 3: Write TickRecord struct**

`src/temporal/record.rs`:
```rust
use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::links::{DepthProfile, TradeActivity};
use crate::ontology::objects::Symbol;

/// Compact snapshot of one pipeline tick's key signals.
/// Stored in ring buffer and persisted to SurrealDB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRecord {
    pub tick_number: u64,
    pub timestamp: OffsetDateTime,
    pub signals: HashMap<Symbol, SymbolSignals>,
}

/// Per-symbol signals captured at one tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSignals {
    // Convergence
    pub composite: Decimal,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,

    // Dimensions (5D vector)
    pub order_book_pressure: Decimal,
    pub capital_flow_direction: Decimal,
    pub capital_size_divergence: Decimal,
    pub institutional_direction: Decimal,
    pub depth_structure_imbalance: Decimal,

    // Depth structure
    pub bid_top3_ratio: Decimal,
    pub ask_top3_ratio: Decimal,
    pub bid_best_ratio: Decimal,
    pub ask_best_ratio: Decimal,
    pub spread: Option<Decimal>,

    // Trade activity (this tick only)
    pub trade_count: usize,
    pub trade_volume: i64,
    pub buy_volume: i64,
    pub sell_volume: i64,
    pub vwap: Option<Decimal>,

    // Degradation (if tracked)
    pub composite_degradation: Option<Decimal>,
    pub institution_retention: Option<Decimal>,
}

impl TickRecord {
    /// Build a TickRecord from the current pipeline output.
    pub fn capture(
        tick_number: u64,
        timestamp: OffsetDateTime,
        convergence: &HashMap<Symbol, crate::graph::decision::ConvergenceScore>,
        dimensions: &HashMap<Symbol, crate::pipeline::dimensions::SymbolDimensions>,
        order_books: &[crate::ontology::links::OrderBookObservation],
        trade_activities: &[TradeActivity],
        degradations: &HashMap<Symbol, crate::graph::decision::StructuralDegradation>,
    ) -> Self {
        let mut signals = HashMap::new();

        // Build lookup maps
        let ob_map: HashMap<&Symbol, &crate::ontology::links::OrderBookObservation> =
            order_books.iter().map(|ob| (&ob.symbol, ob)).collect();
        let ta_map: HashMap<&Symbol, &TradeActivity> =
            trade_activities.iter().map(|ta| (&ta.symbol, ta)).collect();

        for (symbol, conv) in convergence {
            let dims = dimensions.get(symbol);
            let ob = ob_map.get(symbol);
            let ta = ta_map.get(symbol);
            let deg = degradations.get(symbol);

            signals.insert(
                symbol.clone(),
                SymbolSignals {
                    composite: conv.composite,
                    institutional_alignment: conv.institutional_alignment,
                    sector_coherence: conv.sector_coherence,
                    cross_stock_correlation: conv.cross_stock_correlation,

                    order_book_pressure: dims.map(|d| d.order_book_pressure).unwrap_or(Decimal::ZERO),
                    capital_flow_direction: dims.map(|d| d.capital_flow_direction).unwrap_or(Decimal::ZERO),
                    capital_size_divergence: dims.map(|d| d.capital_size_divergence).unwrap_or(Decimal::ZERO),
                    institutional_direction: dims.map(|d| d.institutional_direction).unwrap_or(Decimal::ZERO),
                    depth_structure_imbalance: dims.map(|d| d.depth_structure_imbalance).unwrap_or(Decimal::ZERO),

                    bid_top3_ratio: ob.map(|o| o.bid_profile.top3_volume_ratio).unwrap_or(Decimal::ZERO),
                    ask_top3_ratio: ob.map(|o| o.ask_profile.top3_volume_ratio).unwrap_or(Decimal::ZERO),
                    bid_best_ratio: ob.map(|o| o.bid_profile.best_level_ratio).unwrap_or(Decimal::ZERO),
                    ask_best_ratio: ob.map(|o| o.ask_profile.best_level_ratio).unwrap_or(Decimal::ZERO),
                    spread: ob.and_then(|o| o.spread),

                    trade_count: ta.map(|t| t.trade_count).unwrap_or(0),
                    trade_volume: ta.map(|t| t.total_volume).unwrap_or(0),
                    buy_volume: ta.map(|t| t.buy_volume).unwrap_or(0),
                    sell_volume: ta.map(|t| t.sell_volume).unwrap_or(0),
                    vwap: ta.and_then(|t| t.last_price),

                    composite_degradation: deg.map(|d| d.composite_degradation),
                    institution_retention: deg.map(|d| d.institution_retention),
                },
            );
        }

        TickRecord {
            tick_number,
            timestamp,
            signals,
        }
    }
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --tests`
Expected: PASS (no tests yet, just struct compilation)

- [ ] **Step 4: Commit**

```
git add src/temporal/ src/lib.rs
git commit -m "feat(temporal): add TickRecord struct for tick snapshot capture"
```

---

## Task 2: TickHistory Ring Buffer

**Files:**
- Create: `src/temporal/buffer.rs`
- Modify: `src/temporal/mod.rs` — add `pub mod buffer;`

- [ ] **Step 0: Add module declaration**

Add to `src/temporal/mod.rs`:
```rust
pub mod buffer;
```

- [ ] **Step 1: Write failing tests for ring buffer**

Add to `src/temporal/buffer.rs`:
```rust
use std::collections::VecDeque;

use crate::ontology::objects::Symbol;

use super::record::{TickRecord, SymbolSignals};

/// Ring buffer of recent tick records.
/// Capacity is fixed at creation; oldest ticks are evicted when full.
pub struct TickHistory {
    records: VecDeque<TickRecord>,
    capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn make_signal(composite: Decimal) -> SymbolSignals {
        SymbolSignals {
            composite,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: None,
            composite_degradation: None,
            institution_retention: None,
        }
    }

    fn make_tick(tick_number: u64, sym: &str, composite: Decimal) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), make_signal(composite));
        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
        }
    }

    #[test]
    fn push_and_len() {
        let mut h = TickHistory::new(10);
        assert_eq!(h.len(), 0);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut h = TickHistory::new(3);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        h.push(make_tick(2, "700.HK", dec!(0.2)));
        h.push(make_tick(3, "700.HK", dec!(0.3)));
        h.push(make_tick(4, "700.HK", dec!(0.4)));
        assert_eq!(h.len(), 3);
        assert_eq!(h.oldest().unwrap().tick_number, 2);
        assert_eq!(h.latest().unwrap().tick_number, 4);
    }

    #[test]
    fn latest_n() {
        let mut h = TickHistory::new(10);
        for i in 1..=5 {
            h.push(make_tick(i, "700.HK", Decimal::from(i)));
        }
        let last3 = h.latest_n(3);
        assert_eq!(last3.len(), 3);
        assert_eq!(last3[0].tick_number, 3);
        assert_eq!(last3[2].tick_number, 5);
    }

    #[test]
    fn signal_series() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        h.push(make_tick(2, "700.HK", dec!(0.3)));
        h.push(make_tick(3, "700.HK", dec!(0.5)));

        let series = h.signal_series(&Symbol("700.HK".into()), |s| s.composite);
        assert_eq!(series, vec![dec!(0.1), dec!(0.3), dec!(0.5)]);
    }

    #[test]
    fn signal_series_missing_symbol() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", dec!(0.1)));

        let series = h.signal_series(&Symbol("9988.HK".into()), |s| s.composite);
        assert!(series.is_empty());
    }

    #[test]
    fn empty_buffer() {
        let h = TickHistory::new(10);
        assert!(h.latest().is_none());
        assert!(h.oldest().is_none());
        assert!(h.latest_n(5).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test temporal::buffer --lib`
Expected: FAIL (methods not implemented)

- [ ] **Step 3: Implement TickHistory**

```rust
impl TickHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, record: TickRecord) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn latest(&self) -> Option<&TickRecord> {
        self.records.back()
    }

    pub fn oldest(&self) -> Option<&TickRecord> {
        self.records.front()
    }

    /// Return the last N records in chronological order.
    pub fn latest_n(&self, n: usize) -> Vec<&TickRecord> {
        let skip = self.records.len().saturating_sub(n);
        self.records.iter().skip(skip).collect()
    }

    /// Extract a time series of a specific field for a symbol.
    /// Returns values in chronological order, skipping ticks where the symbol is absent.
    pub fn signal_series<F>(&self, symbol: &Symbol, extractor: F) -> Vec<Decimal>
    where
        F: Fn(&SymbolSignals) -> Decimal,
    {
        self.records
            .iter()
            .filter_map(|r| r.signals.get(symbol).map(|s| extractor(s)))
            .collect()
    }
}
```

Add required imports at top of file:
```rust
use rust_decimal::Decimal;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test temporal::buffer --lib`
Expected: 6 passed, 0 failed

- [ ] **Step 5: Commit**

```
git add src/temporal/buffer.rs
git commit -m "feat(temporal): TickHistory ring buffer with windowed queries"
```

---

## Task 3: TemporalAnalysis — Delta, Acceleration, Duration

**Files:**
- Create: `src/temporal/analysis.rs`
- Modify: `src/temporal/mod.rs` — add `pub mod analysis;`

- [ ] **Step 0: Add module declaration**

Add to `src/temporal/mod.rs`:
```rust
pub mod analysis;
```

- [ ] **Step 1: Write failing tests**

`src/temporal/analysis.rs`:
```rust
use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

use super::buffer::TickHistory;
use super::record::SymbolSignals;

/// Temporal analysis for a single symbol: how its signals are changing.
#[derive(Debug, Clone)]
pub struct SignalDynamics {
    pub symbol: Symbol,
    /// Change from previous tick (latest - previous).
    pub composite_delta: Decimal,
    /// Change of delta (acceleration). Positive = strengthening.
    pub composite_acceleration: Decimal,
    /// Number of consecutive ticks with same composite sign.
    pub composite_duration: u64,
    /// Institutional alignment delta.
    pub inst_alignment_delta: Decimal,
    /// Bid wall top3 ratio delta (wall growing or shrinking).
    pub bid_wall_delta: Decimal,
    /// Ask wall top3 ratio delta.
    pub ask_wall_delta: Decimal,
    /// Buy volume ratio over recent window.
    pub buy_ratio_trend: Decimal,
}

/// Compute temporal dynamics for all symbols in the history.
pub fn compute_dynamics(history: &TickHistory) -> HashMap<Symbol, SignalDynamics> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::record::TickRecord;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn make_signal_full(
        composite: Decimal,
        inst: Decimal,
        bid_top3: Decimal,
        ask_top3: Decimal,
        buy_vol: i64,
        sell_vol: i64,
    ) -> SymbolSignals {
        SymbolSignals {
            composite,
            institutional_alignment: inst,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: bid_top3,
            ask_top3_ratio: ask_top3,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: buy_vol + sell_vol,
            buy_volume: buy_vol,
            sell_volume: sell_vol,
            vwap: None,
            composite_degradation: None,
            institution_retention: None,
        }
    }

    fn make_tick(tick: u64, sym: &str, sig: SymbolSignals) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), sig);
        TickRecord {
            tick_number: tick,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
        }
    }

    #[test]
    fn delta_from_two_ticks() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0.05), dec!(0.1), dec!(0.3), dec!(0.4), 100, 50)));
        h.push(make_tick(2, "700.HK", make_signal_full(dec!(0.08), dec!(0.15), dec!(0.35), dec!(0.38), 200, 80)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_delta, dec!(0.03));
        assert_eq!(d.inst_alignment_delta, dec!(0.05));
        assert_eq!(d.bid_wall_delta, dec!(0.05));
        assert_eq!(d.ask_wall_delta, dec!(-0.02));
    }

    #[test]
    fn acceleration_from_three_ticks() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0.01), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(2, "700.HK", make_signal_full(dec!(0.03), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(3, "700.HK", make_signal_full(dec!(0.06), dec!(0), dec!(0), dec!(0), 0, 0)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        // delta at tick 3: 0.06 - 0.03 = 0.03
        assert_eq!(d.composite_delta, dec!(0.03));
        // delta at tick 2: 0.03 - 0.01 = 0.02
        // acceleration: 0.03 - 0.02 = 0.01
        assert_eq!(d.composite_acceleration, dec!(0.01));
    }

    #[test]
    fn duration_same_sign() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0.01), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(2, "700.HK", make_signal_full(dec!(0.03), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(3, "700.HK", make_signal_full(dec!(0.05), dec!(0), dec!(0), dec!(0), 0, 0)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_duration, 3); // 3 consecutive positive ticks
    }

    #[test]
    fn duration_resets_on_sign_change() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0.05), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(2, "700.HK", make_signal_full(dec!(-0.02), dec!(0), dec!(0), dec!(0), 0, 0)));
        h.push(make_tick(3, "700.HK", make_signal_full(dec!(-0.04), dec!(0), dec!(0), dec!(0), 0, 0)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_duration, 2); // only the 2 negative ticks
    }

    #[test]
    fn buy_ratio_trend() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 100, 100)));
        h.push(make_tick(2, "700.HK", make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 150, 50)));
        h.push(make_tick(3, "700.HK", make_signal_full(dec!(0), dec!(0), dec!(0), dec!(0), 200, 50)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        // total buy = 450, total vol = 650, ratio = 450/650 ≈ 0.692...
        assert!(d.buy_ratio_trend > dec!(0.69));
        assert!(d.buy_ratio_trend < dec!(0.70));
    }

    #[test]
    fn single_tick_zeroed_deltas() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", make_signal_full(dec!(0.05), dec!(0.1), dec!(0.3), dec!(0.4), 100, 50)));

        let dynamics = compute_dynamics(&h);
        let d = &dynamics[&Symbol("700.HK".into())];
        assert_eq!(d.composite_delta, Decimal::ZERO);
        assert_eq!(d.composite_acceleration, Decimal::ZERO);
        assert_eq!(d.composite_duration, 1);
    }

    #[test]
    fn empty_history() {
        let h = TickHistory::new(10);
        let dynamics = compute_dynamics(&h);
        assert!(dynamics.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test temporal::analysis --lib`
Expected: FAIL (todo!() panics)

- [ ] **Step 3: Implement compute_dynamics**

Replace `todo!()` with:
```rust
pub fn compute_dynamics(history: &TickHistory) -> HashMap<Symbol, SignalDynamics> {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return HashMap::new();
    }

    // Collect all symbols that appear in the latest tick
    let latest = match records.last() {
        Some(r) => r,
        None => return HashMap::new(),
    };

    let mut result = HashMap::new();

    for symbol in latest.signals.keys() {
        let series: Vec<&SymbolSignals> = records
            .iter()
            .filter_map(|r| r.signals.get(symbol))
            .collect();

        if series.is_empty() {
            continue;
        }

        let current = series.last().unwrap();
        let prev = if series.len() >= 2 { Some(series[series.len() - 2]) } else { None };
        let prev_prev = if series.len() >= 3 { Some(series[series.len() - 3]) } else { None };

        // Delta
        let composite_delta = prev
            .map(|p| current.composite - p.composite)
            .unwrap_or(Decimal::ZERO);
        let inst_alignment_delta = prev
            .map(|p| current.institutional_alignment - p.institutional_alignment)
            .unwrap_or(Decimal::ZERO);
        let bid_wall_delta = prev
            .map(|p| current.bid_top3_ratio - p.bid_top3_ratio)
            .unwrap_or(Decimal::ZERO);
        let ask_wall_delta = prev
            .map(|p| current.ask_top3_ratio - p.ask_top3_ratio)
            .unwrap_or(Decimal::ZERO);

        // Acceleration
        let prev_delta = match (prev, prev_prev) {
            (Some(p), Some(pp)) => p.composite - pp.composite,
            _ => Decimal::ZERO,
        };
        let composite_acceleration = if prev.is_some() && prev_prev.is_some() {
            composite_delta - prev_delta
        } else {
            Decimal::ZERO
        };

        // Duration: consecutive ticks with same sign as current
        let current_sign = current.composite.signum();
        let mut composite_duration: u64 = 0;
        for s in series.iter().rev() {
            if s.composite.signum() == current_sign {
                composite_duration += 1;
            } else {
                break;
            }
        }

        // Buy ratio trend over entire window
        let total_buy: i64 = series.iter().map(|s| s.buy_volume).sum();
        let total_vol: i64 = series.iter().map(|s| s.trade_volume).sum();
        let buy_ratio_trend = if total_vol > 0 {
            Decimal::from(total_buy) / Decimal::from(total_vol)
        } else {
            Decimal::ZERO
        };

        result.insert(
            symbol.clone(),
            SignalDynamics {
                symbol: symbol.clone(),
                composite_delta,
                composite_acceleration,
                composite_duration,
                inst_alignment_delta,
                bid_wall_delta,
                ask_wall_delta,
                buy_ratio_trend,
            },
        );
    }

    result
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test temporal::analysis --lib`
Expected: 7 passed, 0 failed

- [ ] **Step 5: Commit**

```
git add src/temporal/analysis.rs
git commit -m "feat(temporal): compute signal delta, acceleration, duration from tick history"
```

---

## Task 4: Wire TickHistory Into Main Loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add TickHistory initialization after tracker**

After `let mut tracker = PositionTracker::new();` add:
```rust
use eden::temporal::buffer::TickHistory;
use eden::temporal::record::TickRecord;
use eden::temporal::analysis::compute_dynamics;

let mut history = TickHistory::new(300); // ~10 min at 2s debounce
```

- [ ] **Step 2: After pipeline computation, capture and push tick record**

After `let newly_entered = tracker.auto_enter(...)` add:
```rust
        // ── Capture tick record into history ──
        let tick_record = TickRecord::capture(
            tick,
            now,
            &decision.convergence_scores,
            &dim_snapshot.dimensions,
            &links.order_books,
            &links.trade_activities,
            &decision.degradations,
        );
        history.push(tick_record);

        // ── Compute temporal dynamics ──
        let dynamics = compute_dynamics(&history);
```

- [ ] **Step 3: Add temporal dynamics to display output**

After the Convergence Scores display section, add:
```rust
        // ── Display: Temporal Dynamics ──
        if history.len() >= 2 {
            let mut dyn_syms: Vec<_> = dynamics.iter().collect();
            dyn_syms.sort_by(|a, b| b.1.composite_delta.abs().cmp(&a.1.composite_delta.abs()));
            println!("\n── Signal Dynamics (biggest movers) ──");
            for (sym, d) in dyn_syms.iter().take(10) {
                let accel = if d.composite_acceleration > Decimal::ZERO { "accelerating" }
                    else if d.composite_acceleration < Decimal::ZERO { "decelerating" }
                    else { "steady" };
                println!(
                    "  {:>8}  delta={:>+7}%  {}  duration={} ticks  inst_delta={:>+7}%  bid_wall={:>+6}%  ask_wall={:>+6}%  buy_ratio={:>5}%",
                    sym,
                    (d.composite_delta * pct).round_dp(1),
                    accel,
                    d.composite_duration,
                    (d.inst_alignment_delta * pct).round_dp(1),
                    (d.bid_wall_delta * pct).round_dp(1),
                    (d.ask_wall_delta * pct).round_dp(1),
                    (d.buy_ratio_trend * pct).round_dp(0),
                );
            }
        }
```

- [ ] **Step 4: Update summary line**

Change the summary println to include history length:
```rust
        println!(
            "\n  Tracked: {} | New: {} | History: {}/{} ticks | Data: {} depths, {} brokers, {} quotes",
            tracker.active_count(),
            newly_entered.len(),
            history.len(),
            300,
            live.depths.len(),
            live.brokers.len(),
            live.quotes.len(),
        );
```

- [ ] **Step 5: Verify compilation and run**

Run: `cargo check`
Expected: PASS

Run: `cargo run` (briefly, verify tick history output appears from tick 2)

- [ ] **Step 6: Commit**

```
git add src/main.rs
git commit -m "feat: integrate temporal ring buffer into main event loop"
```

---

## Task 5: SurrealDB Persistence — Schema + Store

**Files:**
- Modify: `Cargo.toml`
- Create: `src/persistence/mod.rs`
- Create: `src/persistence/schema.rs`
- Create: `src/persistence/store.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add SurrealDB dependency**

Add to `Cargo.toml` `[dependencies]`:
```toml
surrealdb = { version = "2", features = ["kv-rocksdb"] }
```
(Note: `rust_decimal_macros` is already a dev-dependency — no need to add to production deps.)

Add to `src/lib.rs`:
```rust
pub mod persistence;
```

- [ ] **Step 2: Create module structure**

`src/persistence/mod.rs`:
```rust
pub mod schema;
pub mod store;
```

- [ ] **Step 3: Define schema**

`src/persistence/schema.rs`:
```rust
/// SurrealDB table and index definitions for Eden.
/// Called once at startup to ensure schema exists.
pub const SCHEMA: &str = r#"
-- Tick records: one per pipeline cycle
DEFINE TABLE tick_record SCHEMAFULL;
DEFINE FIELD tick_number ON tick_record TYPE int;
DEFINE FIELD timestamp ON tick_record TYPE datetime;
DEFINE FIELD signals ON tick_record TYPE object;
DEFINE INDEX idx_tick_number ON tick_record FIELDS tick_number UNIQUE;
DEFINE INDEX idx_timestamp ON tick_record FIELDS timestamp;

-- Institution state: tracks institution behavior over time
DEFINE TABLE institution_state SCHEMAFULL;
DEFINE FIELD institution_id ON institution_state TYPE int;
DEFINE FIELD timestamp ON institution_state TYPE datetime;
DEFINE FIELD symbols ON institution_state TYPE array;
DEFINE FIELD ask_symbols ON institution_state TYPE array;
DEFINE FIELD bid_symbols ON institution_state TYPE array;
DEFINE FIELD seat_count ON institution_state TYPE int;
DEFINE INDEX idx_inst_time ON institution_state FIELDS institution_id, timestamp;

-- Daily summary: aggregated per symbol per day
DEFINE TABLE daily_summary SCHEMAFULL;
DEFINE FIELD symbol ON daily_summary TYPE string;
DEFINE FIELD date ON daily_summary TYPE string;
DEFINE FIELD tick_count ON daily_summary TYPE int;
DEFINE FIELD avg_composite ON daily_summary TYPE string;
DEFINE FIELD max_composite ON daily_summary TYPE string;
DEFINE FIELD min_composite ON daily_summary TYPE string;
DEFINE FIELD avg_inst_alignment ON daily_summary TYPE string;
DEFINE INDEX idx_sym_date ON daily_summary FIELDS symbol, date UNIQUE;
"#;
```

- [ ] **Step 4: Implement EdenStore**

`src/persistence/store.rs`:
```rust
use std::collections::HashMap;

use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

use crate::ontology::links::CrossStockPresence;
use crate::ontology::objects::Symbol;
use crate::temporal::record::TickRecord;

use super::schema;

pub struct EdenStore {
    db: Surreal<Db>,
}

impl EdenStore {
    /// Open or create the SurrealDB database at the given path.
    pub async fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("eden").use_db("market").await?;

        // Apply schema
        db.query(schema::SCHEMA).await?;

        Ok(Self { db })
    }

    /// Persist a tick record.
    pub async fn write_tick(&self, record: &TickRecord) -> Result<(), Box<dyn std::error::Error>> {
        let id = format!("tick_{}", record.tick_number);
        let _: Option<serde_json::Value> = self
            .db
            .create(("tick_record", &id))
            .content(record)
            .await?;
        Ok(())
    }

    /// Persist institution cross-stock presences for tracking over time.
    pub async fn write_institution_states(
        &self,
        presences: &[CrossStockPresence],
        timestamp: time::OffsetDateTime,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for p in presences {
            let id = format!("inst_{}_{}", p.institution_id.0, timestamp.unix_timestamp());
            let record = serde_json::json!({
                "institution_id": p.institution_id.0,
                "timestamp": timestamp.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
                "symbols": p.symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "ask_symbols": p.ask_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "bid_symbols": p.bid_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "seat_count": p.symbols.len(),
            });
            let _: Option<serde_json::Value> = self
                .db
                .create(("institution_state", &id))
                .content(record)
                .await?;
        }
        Ok(())
    }

    /// Query recent tick records for a symbol.
    pub async fn recent_ticks(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<TickRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM tick_record WHERE signals.`{sym}`.composite != NONE ORDER BY tick_number DESC LIMIT {limit}",
            sym = symbol.0,
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<TickRecord> = result.take(0)?;
        Ok(records)
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check`

Note: RocksDB compilation will take ~18 minutes on this machine. This is expected.

- [ ] **Step 6: Commit**

```
git add src/persistence/ src/lib.rs Cargo.toml
git commit -m "feat(persistence): SurrealDB store with tick_record and institution_state tables"
```

---

## Task 6: Wire SurrealDB Into Main Loop

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add SurrealDB initialization at startup**

After ObjectStore initialization, add:
```rust
use eden::persistence::store::EdenStore;

    // ── Initialize SurrealDB ──
    let eden_db_path = std::env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".into());
    let eden_store = match EdenStore::open(&eden_db_path).await {
        Ok(store) => {
            println!("SurrealDB opened at {}", eden_db_path);
            Some(store)
        }
        Err(e) => {
            eprintln!("Warning: SurrealDB failed to open: {}. Running without persistence.", e);
            None
        }
    };
```

- [ ] **Step 2: After tick record capture, persist to SurrealDB**

After `history.push(tick_record);` add:
```rust
        // ── Persist to SurrealDB (non-blocking, log errors) ──
        if let Some(ref store) = eden_store {
            if let Some(latest) = history.latest() {
                if let Err(e) = store.write_tick(latest).await {
                    eprintln!("Warning: failed to write tick: {}", e);
                }
            }
            // Write institution states every 30 ticks (~1 min)
            if tick % 30 == 0 {
                if let Err(e) = store.write_institution_states(
                    &links.cross_stock_presences,
                    now,
                ).await {
                    eprintln!("Warning: failed to write institution states: {}", e);
                }
            }
        }
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Test with `cargo run`**

Verify:
- "SurrealDB opened at data/eden.db" in output
- No write errors
- `data/eden.db` directory created

- [ ] **Step 5: Commit**

```
git add src/main.rs
git commit -m "feat: integrate SurrealDB persistence into main event loop"
```

---

## Task 7: Run All Tests

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All existing 136 tests + new temporal tests pass

- [ ] **Step 2: Run with `cargo run`**

Verify:
- Temporal dynamics display from tick 2 onwards
- Signal delta/acceleration/duration shown
- SurrealDB writes without errors
- Ring buffer shows "History: N/300 ticks"

- [ ] **Step 3: Final commit**

```
git add -A
git commit -m "feat: temporal ring buffer + SurrealDB persistence complete"
```
