# Pressure Event-Driven Cutover (Option C) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all 6 pressure channels (OrderBook, CapitalFlow, Institutional, Momentum, Volume, Structure) update from market pushes instead of from the 5 s tick boundary, so BP posteriors and downstream tactical reads can shift sub-tick.

**Architecture:** Push handler stays cheap — it demultiplexes incoming `longport::PushEvent` into per-channel `PressureEvent`s on a bounded mpsc with drop-oldest semantics. Per-channel async workers maintain incremental state, recompute their channel's value on event arrival, write into sub-KG, and call `BeliefSubstrate::observe_symbol` to feed BP. Tick boundary still fires the existing tactical pipeline, but it now reads a posterior that has been continuously refreshed between ticks. Edge propagation (graph edges across symbols) stays tick-bound for now — only local channel pressure goes event-driven.

**Tech Stack:** Rust, tokio, dashmap, arc-swap. Existing modules: `src/pipeline/event_driven_bp/`, `src/pipeline/pressure.rs`, `src/pipeline/dimensions.rs`, `src/us/runtime.rs`, `src/hk/runtime.rs`.

---

## File Structure

**New files:**
- `src/pipeline/pressure_events/mod.rs` — module root, re-exports
- `src/pipeline/pressure_events/event.rs` — `PressureEvent` enum + push demultiplexer
- `src/pipeline/pressure_events/bus.rs` — bounded mpsc with drop-oldest helper
- `src/pipeline/pressure_events/channel_state.rs` — per-channel per-symbol incremental state structs
- `src/pipeline/pressure_events/worker.rs` — per-channel async worker spawn fn
- `src/pipeline/pressure_events/aggregator.rs` — per-symbol composite recompute, sub-KG write, observe_symbol call

**Modified files:**
- `src/pipeline/event_driven_bp/substrate.rs` — add `observe_symbol` to trait
- `src/pipeline/event_driven_bp/event_substrate.rs` — extract per-symbol logic from `observe_tick` into reusable inner fn, implement `observe_symbol`
- `src/us/runtime.rs` — boot pressure event bus, wire push handler
- `src/hk/runtime.rs` — same as US
- `src/pipeline/dimensions.rs` — split each `compute_*_pressure` fn into per-symbol incremental variant
- `src/pipeline/symbol_sub_kg.rs` — expose per-symbol pressure setter (variant of `update_from_pressure` taking single symbol)

---

## Phase A — `observe_symbol` substrate API

Foundation: BP can observe a single symbol's prior change without rebuilding the whole graph.

### Task A1: Add `observe_symbol` to `BeliefSubstrate` trait

**Files:**
- Modify: `src/pipeline/event_driven_bp/substrate.rs`

- [ ] **Step 1: Read the trait file**

`Read src/pipeline/event_driven_bp/substrate.rs`

- [ ] **Step 2: Add the method to the trait**

After `observe_tick` (line 59), add:

```rust
    /// Observe a single symbol's prior change. Cheaper than `observe_tick`
    /// when only one symbol's state has moved (e.g. event-driven path:
    /// orderbook depth update touches just that symbol's OrderBook
    /// channel). Implementations seed residual queue updates from this
    /// symbol to every neighbour using the cached neighbour list — caller
    /// MUST have called `observe_tick` at least once with the full edge
    /// set so neighbours are populated.
    ///
    /// `neighbours` lets the caller pass the latest edges-for-this-symbol
    /// snapshot if available; pass an empty slice to use whatever was
    /// cached during the last `observe_tick`.
    fn observe_symbol(
        &self,
        symbol: &str,
        prior: NodePrior,
        neighbours: &[GraphEdge],
    );
```

- [ ] **Step 3: Compile (expect error: missing impl on EventDrivenSubstrate)**

```bash
cd ~/eden-src && CARGO_TARGET_DIR=/tmp/eden-target cargo check --lib --features persistence 2>&1 | tail -10
```

Expected: error `not all trait items implemented, missing: observe_symbol`.

---

### Task A2: Implement `observe_symbol` on `EventDrivenSubstrate`

**Files:**
- Modify: `src/pipeline/event_driven_bp/event_substrate.rs`

- [ ] **Step 1: Read current `observe_tick` body**

`Read src/pipeline/event_driven_bp/event_substrate.rs:293-375`

- [ ] **Step 2: Extract per-symbol observation into a private helper**

Inside `impl EventDrivenSubstrate { ... }` block (after `dropped_message_count`, before `Default impl`):

```rust
    /// Per-symbol prior update. Shared between `observe_tick` and
    /// `observe_symbol`. Returns whether the prior actually changed
    /// (so observe_tick can skip seeding for unchanged nodes).
    fn observe_symbol_inner(
        &self,
        symbol: &str,
        prior: NodePrior,
        adj_neighbours: Option<Vec<(String, f64)>>,
    ) -> bool {
        let entry = self
            .nodes
            .entry(symbol.to_string())
            .or_insert_with(|| Arc::new(NodeState::new()));
        let state = Arc::clone(entry.value());
        drop(entry);

        let mut lite = state.snapshot_lite();
        let prior_changed = lite.prior != prior.belief || lite.observed != prior.observed;
        lite.prior = prior.belief;
        lite.observed = prior.observed;
        if prior_changed {
            lite.belief = prior.belief;
            if !lite.observed && lite.belief.iter().all(|v| v.abs() < 1e-9) {
                let uniform = 1.0 / N_STATES as f64;
                lite.belief = [uniform; N_STATES];
            }
        }
        state.store_lite(lite);

        let neighbours = match adj_neighbours {
            Some(ns) => ns,
            None => state.aux.lock().neighbours.clone(),
        };

        {
            let mut aux = state.aux.lock();
            // Caller-provided neighbours override cached topology.
            if !neighbours.is_empty() {
                aux.neighbours = neighbours.clone();
            }
            if prior_changed {
                aux.clear_inbox();
            }
        }

        if prior_changed {
            let belief = state.snapshot_lite().belief;
            for (k, weight) in &neighbours {
                let msg = compute_outgoing_message(&belief, *weight);
                self.queue.push(EdgeUpdate {
                    from: symbol.to_string(),
                    to: k.clone(),
                    message: msg,
                    residual: 1.0,
                });
            }
        }

        prior_changed
    }
```

- [ ] **Step 3: Refactor `observe_tick` to call the helper**

Replace the current body of `observe_tick` (lines 294-375) with:

```rust
    fn observe_tick(
        &self,
        priors: &HashMap<String, NodePrior>,
        edges: &[GraphEdge],
        _tick: u64,
    ) {
        // Build adjacency once.
        let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for edge in edges {
            adj.entry(edge.from.clone())
                .or_default()
                .push((edge.to.clone(), edge.weight));
            adj.entry(edge.to.clone())
                .or_default()
                .push((edge.from.clone(), edge.weight));
        }

        for (sym, prior) in priors {
            let neighbours = adj.get(sym).cloned().unwrap_or_default();
            self.observe_symbol_inner(sym, *prior, Some(neighbours));
        }
    }
```

- [ ] **Step 4: Implement `observe_symbol` trait method**

Add to the `impl BeliefSubstrate for EventDrivenSubstrate` block (after `observe_tick`):

```rust
    fn observe_symbol(
        &self,
        symbol: &str,
        prior: NodePrior,
        neighbours: &[GraphEdge],
    ) {
        let adj: Vec<(String, f64)> = neighbours
            .iter()
            .filter_map(|e| {
                if e.from == symbol {
                    Some((e.to.clone(), e.weight))
                } else if e.to == symbol {
                    Some((e.from.clone(), e.weight))
                } else {
                    None
                }
            })
            .collect();
        let pass_neighbours = if adj.is_empty() { None } else { Some(adj) };
        self.observe_symbol_inner(symbol, prior, pass_neighbours);
    }
```

- [ ] **Step 5: Add a unit test**

Append to the `tests` module of `event_substrate.rs`:

```rust
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn observe_symbol_propagates_to_cached_neighbours() {
        let substrate = EventDrivenSubstrate::new(EventConfig {
            workers: 2,
            residual_threshold: 1e-6,
            publish_interval_ms: 5,
            message_damping: 0.3,
        });
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), NodePrior::default());
        priors.insert("B".to_string(), NodePrior::default());
        let edges = vec![GraphEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.5,
            kind: BpEdgeKind::StockToStock,
        }];
        // Tick once to populate neighbour cache.
        substrate.observe_tick(&priors, &edges, 1);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        let view_before = substrate.posterior_snapshot();
        let gen_before = view_before.generation;

        // Observe A only — B should still receive a propagated message.
        let mut a_prior = NodePrior::default();
        a_prior.belief = [0.7, 0.2, 0.1];
        a_prior.observed = true;
        substrate.observe_symbol("A", a_prior, &[]);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        substrate.drain_pending();

        let view_after = substrate.posterior_snapshot();
        assert!(
            view_after.generation > gen_before,
            "generation must advance after observe_symbol"
        );
        let belief_b = view_after.beliefs.get("B").expect("B present");
        assert!(belief_b[0] > 1.0 / 3.0 + 1e-6, "B's bull should rise after A propagates bullish prior");
    }
```

- [ ] **Step 6: Compile + run**

```bash
cd ~/eden-src && CARGO_TARGET_DIR=/tmp/eden-target cargo test --lib --features persistence event_driven_bp 2>&1 | tail -15
```

Expected: 13 tests pass (12 existing + new one).

- [ ] **Step 7: Commit**

```bash
cd ~/eden-src && git add src/pipeline/event_driven_bp/
git commit -m "Add observe_symbol to BeliefSubstrate (Phase A)

Single-symbol prior update path. EventDrivenSubstrate refactored to
share a private observe_symbol_inner helper between observe_tick and
the new trait method. Test covers cross-symbol propagation through
cached neighbour topology after observe_tick has primed it.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Phase B — Push event bus

Foundation: push events flow into a bounded async channel without blocking the longport push consumer.

### Task B1: Create the `pressure_events` module skeleton

**Files:**
- Create: `src/pipeline/pressure_events/mod.rs`
- Create: `src/pipeline/pressure_events/event.rs`
- Create: `src/pipeline/pressure_events/bus.rs`
- Modify: `src/pipeline/mod.rs` (add module declaration)

- [ ] **Step 1: Create module skeleton**

Write `src/pipeline/pressure_events/mod.rs`:

```rust
//! Push-event bus for sub-tick pressure recomputation.
//!
//! The longport push consumer demultiplexes incoming events into
//! [`PressureEvent`]s and publishes them to a bounded mpsc with
//! drop-oldest semantics. Per-channel worker tasks drain the bus,
//! update incremental state, and notify the per-symbol aggregator
//! which writes into sub-KG and calls `BeliefSubstrate::observe_symbol`.

pub mod bus;
pub mod event;

pub use bus::{spawn_bus, EventBusHandle};
pub use event::{PressureEvent, demux_push_event};
```

- [ ] **Step 2: Define `PressureEvent`**

Write `src/pipeline/pressure_events/event.rs`:

```rust
//! Per-channel push events. `PressureEvent` carries just the data each
//! channel cares about, so workers don't have to filter against an
//! envelope type.

use chrono::{DateTime, Utc};
use longport::quote::PushEvent;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub enum PressureEvent {
    /// Top-of-book + 10-level depth update. Drives OrderBook + Structure
    /// channels.
    Depth {
        symbol: String,
        bids: Vec<(Decimal, Decimal)>,  // (price, volume)
        asks: Vec<(Decimal, Decimal)>,
        ts: DateTime<Utc>,
    },
    /// Last trade. Drives CapitalFlow + Momentum + Volume channels.
    Trade {
        symbol: String,
        price: Decimal,
        volume: Decimal,
        side: TradeSide,
        ts: DateTime<Utc>,
    },
    /// Broker queue update. Drives Institutional channel.
    Broker {
        symbol: String,
        broker_id: String,
        side: TradeSide,
        position: u32,
        ts: DateTime<Utc>,
    },
    /// Quote (last/prev_close/day_high/day_low/volume/turnover). Drives
    /// CapitalFlow (turnover delta) + Volume (volume delta).
    Quote {
        symbol: String,
        last: Decimal,
        volume: Decimal,
        turnover: Decimal,
        ts: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
    Unknown,
}

/// Demultiplex a single longport `PushEvent` into zero or more
/// `PressureEvent`s. Returns a small Vec to keep the call site flat.
pub fn demux_push_event(_evt: &PushEvent) -> Vec<PressureEvent> {
    // STUB: each variant of PushEvent maps to one PressureEvent variant.
    // Filled in during Task B2 once we read the longport variant shapes.
    Vec::new()
}
```

- [ ] **Step 3: Bus implementation**

Write `src/pipeline/pressure_events/bus.rs`:

```rust
//! Bounded mpsc with drop-oldest semantics. When the bus is full, the
//! producer evicts the oldest event in the queue rather than blocking.
//! This keeps the longport push consumer non-blocking even under burst
//! load — pressure freshness is recoverable (next push restores it),
//! push consumer back-pressure is not.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;
use std::collections::VecDeque;

use super::event::PressureEvent;

const DEFAULT_CAPACITY: usize = 50_000;

#[derive(Debug)]
pub struct EventBusHandle {
    inner: Arc<BusInner>,
}

#[derive(Debug)]
struct BusInner {
    queue: Mutex<VecDeque<PressureEvent>>,
    notify: Notify,
    capacity: usize,
    dropped: std::sync::atomic::AtomicU64,
}

impl EventBusHandle {
    /// Non-blocking publish. Drops the oldest event if full.
    pub fn publish(&self, evt: PressureEvent) {
        let mut q = self.inner.queue.lock();
        if q.len() >= self.inner.capacity {
            q.pop_front();
            self.inner
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        q.push_back(evt);
        drop(q);
        self.inner.notify.notify_one();
    }

    /// Async pop. Awaits until an event is available.
    pub async fn pop(&self) -> Option<PressureEvent> {
        loop {
            {
                let mut q = self.inner.queue.lock();
                if let Some(evt) = q.pop_front() {
                    return Some(evt);
                }
            }
            self.inner.notify.notified().await;
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.inner
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn pending_count(&self) -> usize {
        self.inner.queue.lock().len()
    }
}

pub fn spawn_bus() -> EventBusHandle {
    let inner = Arc::new(BusInner {
        queue: Mutex::new(VecDeque::with_capacity(DEFAULT_CAPACITY)),
        notify: Notify::new(),
        capacity: DEFAULT_CAPACITY,
        dropped: std::sync::atomic::AtomicU64::new(0),
    });
    EventBusHandle { inner }
}
```

- [ ] **Step 4: Add module to pipeline/mod.rs**

`Read src/pipeline/mod.rs`, find the `pub mod` lines, add:

```rust
pub mod pressure_events;
```

in the alphabetically-sorted spot.

- [ ] **Step 5: Test the bus drop-oldest behaviour**

Append to `bus.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    #[tokio::test]
    async fn drop_oldest_when_full() {
        let inner = Arc::new(BusInner {
            queue: Mutex::new(VecDeque::with_capacity(2)),
            notify: Notify::new(),
            capacity: 2,
            dropped: std::sync::atomic::AtomicU64::new(0),
        });
        let bus = EventBusHandle { inner };
        for i in 0..5 {
            bus.publish(PressureEvent::Quote {
                symbol: format!("S{i}"),
                last: Decimal::ONE,
                volume: Decimal::ONE,
                turnover: Decimal::ONE,
                ts: Utc::now(),
            });
        }
        assert_eq!(bus.pending_count(), 2);
        assert_eq!(bus.dropped_count(), 3);

        // First survivor is index 3 (0,1,2 dropped).
        let first = bus.pop().await.unwrap();
        if let PressureEvent::Quote { symbol, .. } = first {
            assert_eq!(symbol, "S3");
        } else {
            panic!("wrong variant");
        }
    }
}
```

- [ ] **Step 6: Compile + test**

```bash
cd ~/eden-src && CARGO_TARGET_DIR=/tmp/eden-target cargo test --lib --features persistence pressure_events 2>&1 | tail -10
```

Expected: 1 test passes.

- [ ] **Step 7: Commit**

```bash
cd ~/eden-src && git add src/pipeline/pressure_events/ src/pipeline/mod.rs
git commit -m "Pressure event bus skeleton (Phase B)

Bounded mpsc with drop-oldest. Producer never blocks; consumer awaits.
PressureEvent demux is stubbed — wired to longport in next task."
```

---

### Task B2: Wire the demux + spawn bus from US runtime

**Files:**
- Modify: `src/pipeline/pressure_events/event.rs` (fill in `demux_push_event`)
- Modify: `src/us/runtime.rs` (boot bus, hook push handler)

- [ ] **Step 1: Read longport PushEvent variants**

```bash
cd ~/eden-src && grep -rn "PushEvent::" src/us/runtime.rs src/hk/runtime.rs | head -10
```

Use the variants seen there to fill in `demux_push_event` — typically `PushQuote`, `PushDepth`, `PushTrades`, `PushBrokers`. For each, extract symbol + payload into the matching `PressureEvent` variant.

- [ ] **Step 2: Implement `demux_push_event`**

Replace the stub in `event.rs` with the real match. Pattern:

```rust
pub fn demux_push_event(evt: &PushEvent) -> Vec<PressureEvent> {
    use longport::quote::PushEventDetail;
    let symbol = evt.symbol.clone();
    let ts = evt.sequence_time().unwrap_or_else(Utc::now); // placeholder — replace with actual ts field
    match &evt.detail {
        PushEventDetail::Quote(q) => vec![PressureEvent::Quote {
            symbol,
            last: q.last_done,
            volume: q.volume.into(),
            turnover: q.turnover,
            ts,
        }],
        PushEventDetail::Depth(d) => vec![PressureEvent::Depth {
            symbol,
            bids: d.bids.iter().map(|l| (l.price, l.volume.into())).collect(),
            asks: d.asks.iter().map(|l| (l.price, l.volume.into())).collect(),
            ts,
        }],
        PushEventDetail::Trade(trades) => trades
            .iter()
            .map(|t| PressureEvent::Trade {
                symbol: symbol.clone(),
                price: t.price,
                volume: t.volume.into(),
                side: TradeSide::Unknown, // refine when actual field known
                ts,
            })
            .collect(),
        PushEventDetail::Brokers(b) => {
            let mut out = Vec::new();
            for seat in &b.bid_brokers {
                out.push(PressureEvent::Broker {
                    symbol: symbol.clone(),
                    broker_id: seat.broker_id.clone(),
                    side: TradeSide::Buy,
                    position: seat.position as u32,
                    ts,
                });
            }
            for seat in &b.ask_brokers {
                out.push(PressureEvent::Broker {
                    symbol: symbol.clone(),
                    broker_id: seat.broker_id.clone(),
                    side: TradeSide::Sell,
                    position: seat.position as u32,
                    ts,
                });
            }
            out
        }
    }
}
```

NOTE: Field names (`evt.detail`, `evt.symbol`, `q.last_done`, `b.bid_brokers`, etc.) likely differ from the actual longport API. The task is to look at how the existing code reads these fields and copy that pattern. Use `grep -A 10 "PushEvent" src/us/runtime.rs` to see real shapes.

- [ ] **Step 3: Boot the bus + spawn an idle drainer in US runtime**

In `src/us/runtime.rs`, find the section where `belief_substrate` is initialised (around line 489). Below it, add:

```rust
    let pressure_event_bus = std::sync::Arc::new(
        crate::pipeline::pressure_events::spawn_bus()
    );
    // Phase B drainer: just consumes events and increments a counter.
    // Real per-channel workers are wired in Phase C.
    let drainer_bus = std::sync::Arc::clone(&pressure_event_bus);
    let drainer_handle = tokio::spawn(async move {
        let mut counter = 0u64;
        loop {
            if drainer_bus.pop().await.is_none() {
                break;
            }
            counter = counter.wrapping_add(1);
            if counter % 10_000 == 0 {
                eprintln!("[us pressure-bus] drained {} events", counter);
            }
        }
    });
    let _ = drainer_handle;
```

- [ ] **Step 4: Hook the push handler**

Find where push events are received (likely inside `begin_tick` or in an inner loop that drains `push_rx`). Look for:

```bash
cd ~/eden-src && grep -n "push_rx\|push_event\|PushEvent" src/core/runtime/context.rs src/core/runtime/begin_tick.rs 2>&1 | head
```

Identify the loop that takes from `push_rx` (it produces `PushEvent`s into the runtime). Insert a `pressure_event_bus.publish(...)` call for each demuxed event right where the push is received. This needs the bus handle to be passed into `begin_tick` or accessible via the runtime context — likely needs adding a field to the runtime context.

If threading the handle through is too invasive, alternative: store the bus on `RuntimeContext` (modify `src/core/runtime/context.rs` to hold `Option<Arc<EventBusHandle>>`), and have begin_tick check it.

- [ ] **Step 5: Compile**

```bash
cd ~/eden-src && CARGO_TARGET_DIR=/tmp/eden-target cargo check --lib --bin eden --features persistence 2>&1 | tail -15
```

Iterate field-name fixes until clean.

- [ ] **Step 6: Live smoke test**

Stop existing US, start fresh, watch for the `drained N events` log lines and verify dropped count stays low (< 1000 in first 60 s).

```bash
pkill -f "/tmp/eden-target/debug/eden"
cd ~/eden-src && cargo build --bin eden --features persistence 2>&1 | tail -3
nohup /tmp/eden-target/debug/eden us > .run/eden-us-bus.log 2>&1 &
sleep 30
grep "drained\|push_channel_full\|bus dropped" ~/eden-src/.run/eden-us-bus.log | tail -10
```

Expected: drained counter increasing every few seconds, no `push_channel_full`.

- [ ] **Step 7: Commit**

```bash
cd ~/eden-src && git add src/
git commit -m "Wire pressure event bus into US push handler (Phase B)

Drainer is a no-op counter for now; per-channel workers come in Phase C."
```

- [ ] **Step 8: Repeat the wire-up for HK runtime** (same pattern as US Step 3 + 4).

---

## Phase C — One channel at a time

Each channel: incremental state struct → worker drains its events → recompute its channel value → notify aggregator → aggregator writes sub-KG + calls observe_symbol.

We deliberately do channels in order of expected impact (most pushes first, simplest computation second).

### Task C1: OrderBook channel (driven by `Depth` events)

**Files:**
- Create: `src/pipeline/pressure_events/channel_state.rs`
- Create: `src/pipeline/pressure_events/worker.rs`
- Create: `src/pipeline/pressure_events/aggregator.rs`
- Modify: `src/pipeline/dimensions.rs` (extract per-symbol variant of `compute_order_book_pressure`)
- Modify: `src/pipeline/symbol_sub_kg.rs` (per-symbol pressure setter)

- [ ] **Step 1: Read existing `compute_order_book_pressure`**

```bash
cd ~/eden-src && sed -n '133,165p' src/pipeline/dimensions.rs
```

The fn already iterates per symbol. Extract a per-symbol variant:

- [ ] **Step 2: Add per-symbol fn to `dimensions.rs`**

After `compute_order_book_pressure`, add:

```rust
/// Per-symbol variant of `compute_order_book_pressure` for event-driven
/// callers. Takes the same shape of input as the snapshot's per-symbol
/// slice but for one symbol only.
pub fn compute_order_book_pressure_for(
    bids: &[(Decimal, Decimal)],
    asks: &[(Decimal, Decimal)],
) -> Decimal {
    // Copy of the inner aggregation from compute_order_book_pressure
    // applied to a single symbol's bid/ask vectors.
    //
    // STUB: copy the actual formula by reading lines 133-148 of this file
    // and translating from LinkSnapshot iteration to direct bid/ask args.
    Decimal::ZERO
}
```

- [ ] **Step 3: Fill in the formula**

Copy the body of the existing `compute_order_book_pressure` (the inner loop) and adapt to take `bids`/`asks` slices directly. Do not delete the existing fn — keep it for tick-bound parity.

- [ ] **Step 4: Define the channel state struct**

Write `src/pipeline/pressure_events/channel_state.rs`:

```rust
//! Per-symbol per-channel incremental state. Each channel keeps the
//! latest input slices it needs to recompute its single-symbol value
//! without rebuilding the full LinkSnapshot.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Default)]
pub struct OrderBookState {
    pub bids: Vec<(Decimal, Decimal)>,
    pub asks: Vec<(Decimal, Decimal)>,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_value: Decimal,
}

#[derive(Debug, Default)]
pub struct ChannelStates {
    pub orderbook: RwLock<HashMap<String, OrderBookState>>,
    // Other channels added in Tasks C2..C6.
}

pub type SharedChannelStates = Arc<ChannelStates>;
```

- [ ] **Step 5: Define the per-channel worker**

Write `src/pipeline/pressure_events/worker.rs`:

```rust
//! Per-channel worker tasks. Each worker drains the global event bus,
//! filters events relevant to its channel, updates that channel's
//! state, and notifies the aggregator that the symbol's pressure
//! profile has changed.

use std::sync::Arc;

use crate::pipeline::dimensions;

use super::aggregator::AggregatorHandle;
use super::bus::EventBusHandle;
use super::channel_state::SharedChannelStates;
use super::event::PressureEvent;

pub fn spawn_orderbook_worker(
    bus: Arc<EventBusHandle>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let evt = match bus.pop().await {
                Some(e) => e,
                None => break,
            };
            if let PressureEvent::Depth { symbol, bids, asks, ts } = evt {
                let new_value = dimensions::compute_order_book_pressure_for(&bids, &asks);
                {
                    let mut map = states.orderbook.write();
                    let s = map.entry(symbol.clone()).or_default();
                    let changed = (s.last_value - new_value).abs()
                        > rust_decimal_macros::dec!(0.001);
                    s.bids = bids;
                    s.asks = asks;
                    s.last_updated = Some(ts);
                    s.last_value = new_value;
                    if !changed {
                        continue;
                    }
                }
                aggregator.notify_symbol_changed(symbol);
            }
        }
    });
}
```

NOTE: there's a problem — every worker pops from the same bus, so each event is consumed by exactly one worker. We need either (a) one drainer that fans out to per-channel queues, or (b) each event delivered to all workers. Option (a) is cleaner.

Refactor `bus.rs` to support multi-consumer fan-out — simplest: drainer task pops once, dispatches to per-channel sub-buses. Add this to the worker module:

```rust
pub fn spawn_dispatch(
    bus: Arc<EventBusHandle>,
    depth_tx: tokio::sync::mpsc::Sender<PressureEvent>,
    trade_tx: tokio::sync::mpsc::Sender<PressureEvent>,
    broker_tx: tokio::sync::mpsc::Sender<PressureEvent>,
    quote_tx: tokio::sync::mpsc::Sender<PressureEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let evt = match bus.pop().await {
                Some(e) => e,
                None => break,
            };
            let target = match &evt {
                PressureEvent::Depth { .. } => &depth_tx,
                PressureEvent::Trade { .. } => &trade_tx,
                PressureEvent::Broker { .. } => &broker_tx,
                PressureEvent::Quote { .. } => &quote_tx,
            };
            let _ = target.try_send(evt);
        }
    });
}
```

And rewrite `spawn_orderbook_worker` to take `tokio::sync::mpsc::Receiver<PressureEvent>` instead of `Arc<EventBusHandle>`.

- [ ] **Step 6: Define the aggregator**

Write `src/pipeline/pressure_events/aggregator.rs`:

```rust
//! When a channel's state changes for a symbol, the aggregator
//! recomputes that symbol's composite NodePressure, writes the
//! corresponding NodeIds into sub-KG, derives a fresh `NodePrior` via
//! `observe_from_subkg`, and calls `BeliefSubstrate::observe_symbol`.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::pipeline::event_driven_bp::BeliefSubstrate;
use crate::pipeline::loopy_bp;
use crate::pipeline::symbol_sub_kg::SubKgRegistry;

use super::channel_state::SharedChannelStates;

#[derive(Clone)]
pub struct AggregatorHandle {
    tx: mpsc::Sender<String>,
}

impl AggregatorHandle {
    pub fn notify_symbol_changed(&self, symbol: String) {
        let _ = self.tx.try_send(symbol);
    }
}

pub fn spawn_aggregator(
    states: SharedChannelStates,
    registry: Arc<Mutex<SubKgRegistry>>,
    substrate: Arc<dyn BeliefSubstrate>,
) -> AggregatorHandle {
    let (tx, mut rx) = mpsc::channel::<String>(50_000);
    tokio::spawn(async move {
        while let Some(symbol) = rx.recv().await {
            let prior = {
                let reg = registry.lock();
                if let Some(kg) = reg.graphs.get(&symbol) {
                    loopy_bp::observe_from_subkg(kg)
                } else {
                    continue;
                }
            };
            // STUB: also update sub-KG with the latest channel values
            // before deriving the prior. For OrderBook only:
            let ob_value = states
                .orderbook
                .read()
                .get(&symbol)
                .map(|s| s.last_value)
                .unwrap_or_default();
            {
                let mut reg = registry.lock();
                if let Some(kg) = reg.graphs.get_mut(&symbol) {
                    use crate::pipeline::symbol_sub_kg::NodeId;
                    let now = chrono::Utc::now();
                    kg.set_node_value(NodeId::PressureOrderBook, ob_value, now);
                }
            }
            // Re-derive prior after sub-KG update.
            let prior_after = {
                let reg = registry.lock();
                reg.graphs
                    .get(&symbol)
                    .map(loopy_bp::observe_from_subkg)
                    .unwrap_or(prior)
            };
            substrate.observe_symbol(&symbol, prior_after, &[]);
        }
    });
    AggregatorHandle { tx }
}
```

NOTE: `Arc<Mutex<SubKgRegistry>>` is a sharing model that doesn't exist today — the registry lives owned by the runtime tick loop. This is the integration point we have to design carefully:

Option (a): wrap registry in `Arc<RwLock<SubKgRegistry>>` and share with the aggregator. Tick loop also takes the lock when it does its tick-bound updates. Lock contention is the worry.

Option (b): aggregator does NOT touch sub-KG. Instead, it computes the channel-derived `NodePrior` directly from the channel state struct (skipping sub-KG round-trip), and calls observe_symbol with that prior. Sub-KG keeps getting tick-bound writes. This breaks the invariant that "sub-KG is the source of truth for priors", so downstream reads (visual frame, marginals) won't see the sub-tick value — but BP will. This is acceptable as a first cut.

Pick option (b) for the first cut. Re-evaluate after Phase D when we have multiple channels.

REWRITE the aggregator: take `SharedChannelStates` and a fn that combines the latest channel values into a `NodePrior` directly. We'll need a small inline copy of the formula from `observe_from_subkg` — or better, a helper that takes raw channel values and returns a `NodePrior`. Add such a helper to `loopy_bp.rs`:

```rust
pub fn prior_from_channel_values(
    order_book: f64,
    capital_flow: f64,
    institutional: f64,
    momentum: f64,
    volume: f64,
    structure: f64,
) -> NodePrior {
    // Subset of the observe_from_subkg formula: just pressure channels,
    // no Memory/Belief/Causal/Sector/KL terms (those stay tick-bound).
    let direction_raw = capital_flow + 0.5 * momentum + order_book - structure;
    let base_magnitude = (direction_raw.abs() / 2.0).min(1.0);
    if base_magnitude < PRIOR_MAGNITUDE_FLOOR {
        return NodePrior::default();
    }
    let dominant_idx = if direction_raw > 0.0 { STATE_BULL } else { STATE_BEAR };
    let mut belief = [0.0; N_STATES];
    let dominant_mass = (1.0 + base_magnitude) / (N_STATES as f64 + base_magnitude);
    let rest_mass = (1.0 - dominant_mass) / (N_STATES - 1) as f64;
    for i in 0..N_STATES {
        belief[i] = if i == dominant_idx { dominant_mass } else { rest_mass };
    }
    NodePrior { belief, observed: true }
}
```

Aggregator computes its own prior from `ChannelStates`, calls observe_symbol. Sub-KG stays tick-bound — that's an acceptable concession for the first cut. The invariant we want at this stage is: **between ticks, BP posterior reflects channel-derived sub-tick priors; at tick boundary, full observe_from_subkg path runs and may overwrite.**

- [ ] **Step 7: Wire it all together in US runtime**

In `src/us/runtime.rs`, replace the stub drainer with:

```rust
    let pressure_event_bus = Arc::new(crate::pipeline::pressure_events::spawn_bus());
    let channel_states = Arc::new(crate::pipeline::pressure_events::channel_state::ChannelStates::default());
    let aggregator = crate::pipeline::pressure_events::aggregator::spawn_aggregator(
        Arc::clone(&channel_states),
        // TODO: registry sharing — for option (b) we don't need it here.
        Arc::clone(&belief_substrate),
    );
    let (depth_tx, depth_rx) = tokio::sync::mpsc::channel(20_000);
    let (trade_tx, _trade_rx) = tokio::sync::mpsc::channel(20_000);
    let (broker_tx, _broker_rx) = tokio::sync::mpsc::channel(20_000);
    let (quote_tx, _quote_rx) = tokio::sync::mpsc::channel(20_000);
    let _dispatch = crate::pipeline::pressure_events::worker::spawn_dispatch(
        Arc::clone(&pressure_event_bus),
        depth_tx,
        trade_tx,
        broker_tx,
        quote_tx,
    );
    let _ob_worker = crate::pipeline::pressure_events::worker::spawn_orderbook_worker(
        depth_rx,
        Arc::clone(&channel_states),
        aggregator.clone(),
    );
```

- [ ] **Step 8: Compile + live test**

Build, run US, observe BP posterior generation increments between ticks for active depth-update symbols.

```bash
cd ~/eden-src && cargo build --bin eden --features persistence 2>&1 | tail -3
pkill -f "/tmp/eden-target/debug/eden"
sleep 2
cd ~/eden-src && nohup /tmp/eden-target/debug/eden us > .run/eden-us-orderbook-event.log 2>&1 &
sleep 60
grep "tick=\|push_channel_full\|generation" ~/eden-src/.run/eden-us-orderbook-event.log | tail -10
```

Verify: drops still 0; BP posterior shows continuous evolution (you'll need a probe — add a debug log in aggregator that prints generation after observe_symbol).

- [ ] **Step 9: Commit**

```bash
cd ~/eden-src && git add src/
git commit -m "OrderBook channel event-driven (Phase C1)

Depth pushes drive per-symbol OrderBook channel state. Aggregator
derives sub-tick NodePrior (channels only — no Memory/Belief/Sector
terms) and calls BeliefSubstrate::observe_symbol. Tick-bound sub-KG
write still runs and remains source of truth at tick boundary."
```

---

### Tasks C2-C6: Remaining channels (template)

Each follows the same pattern as C1. Per-channel summary:

| # | Channel | Driver event | State to keep | Compute fn to copy | Drives |
|---|---------|--------------|---------------|--------------------|--------|
| C2 | Structure | Depth | Same as OrderBook (already cached) | `compute_depth_structure_imbalance` (line 148) | Depth → use the cached state from C1's Depth events; just add a Structure channel value computed alongside OrderBook in the same worker (no new event subscription needed). |
| C3 | CapitalFlow | Trade + Quote | Rolling turnover window | `compute_capital_flow_direction` (line 163) | Trade/Quote events. Window is ~30 s — keep a deque of (ts, turnover_delta), prune older. |
| C4 | Volume (`capital_size_divergence`) | Trade | Rolling per-trade size distribution | `compute_capital_size_divergence` (line 194) | Trade events. Maintain per-symbol histogram of trade sizes over a window. |
| C5 | Momentum (`activity_momentum`) | Trade | Rolling price + volume rate | `compute_activity_momentum` (line 340) | Trade events. EMA of price change × volume. |
| C6 | Institutional | Broker | Per-broker seat positions | `compute_institutional_direction` (line 213) | Broker events. Map broker_id → side+position; recompute on change. |

For each, the steps are:
1. Add per-symbol incremental compute fn in `dimensions.rs` (mirror existing batch fn).
2. Add channel state struct in `channel_state.rs`.
3. Add worker in `worker.rs` (drain the channel's mpsc, update state, notify aggregator).
4. Extend `aggregator.rs` to read the new channel value when constructing the prior.
5. Wire the new sub-channel rx in US (and HK) runtime.
6. Compile, live smoke test.
7. Commit.

Mark each channel as a separate task; ship one commit per channel.

---

## Phase D — Cutover + cleanup

Once all 6 channels are event-driven, the tick-bound `sk::update_from_pressure(...)` call at `src/us/runtime.rs:2701` is largely redundant — the aggregator has already written the latest values for each symbol. But sub-KG also receives Memory/Belief/Sector/KL writes in the same tick block, so we can't simply delete the call.

### Task D1: Decide and implement the tick-bound role

- [ ] **Step 1: Audit what `update_from_pressure` writes that the aggregator doesn't**

Goal: identify the gap. Likely: the aggregator writes only PressureOrderBook/CapitalFlow/etc. node values; `update_from_pressure` ALSO writes the composite/convergence/conflict on `PressureOrderBook` aux. If aggregator covers everything, delete the call. Otherwise, keep it but make it idempotent vs aggregator writes (last-writer-wins is fine since timestamps move forward).

- [ ] **Step 2: Either delete or keep**

If keep: no code change beyond removing the redundant per-symbol PressureSnapshot building loop (lines 2660-2701 of `src/us/runtime.rs`). If delete: remove the whole pressure-snapshot block from the tick path.

- [ ] **Step 3: Commit**

---

### Task D2: Live verify on US

- [ ] **Step 1: Restart US on the cutover build**
- [ ] **Step 2: Monitor 10 minutes — drops should be 0, BP generation should advance hundreds of times per tick**
- [ ] **Step 3: Spot-check tactical setup confidence** — pick 5 active symbols, verify confidence shifts between two ticks (`grep "us_bp_conf" .run/...log`).

---

## Phase E — Memory + decision log

### Task E1: Update memory

- [ ] **Step 1: Add a project memory entry**

Write `~/.claude/projects/-Users-hayhay2323-Desktop-eden/memory/project_pressure_event_driven_2026_04_29.md` summarising:
- Why (qualitative change in reaction time, propagation order, pressure shape, multi-rate)
- Architecture (push → bus → dispatch → per-channel worker → aggregator → observe_symbol)
- Gotchas (sub-KG vs aggregator-derived prior split, tick-bound Memory/Belief writes still authoritative for those NodeIds)
- Live verification outcome.

- [ ] **Step 2: Index it in `MEMORY.md`**

---

## Self-Review

**Spec coverage:**
- ✅ Phase A: substrate `observe_symbol` API
- ✅ Phase B: push event bus + demux
- ✅ Phase C1-C6: 6 channels event-driven
- ✅ Phase D: cutover + cleanup
- ✅ Phase E: memory + decision log

**Placeholder scan:** Several `STUB:` markers in code blocks — all explicitly call out what to fill in (formula copy from existing fn, longport field names). Acceptable because the formulae live in named existing fns and the engineer only needs to copy them; the stub structure makes the integration shape explicit. The longport field-name resolution requires reading existing code — explicitly directed.

**Type consistency:** `BeliefSubstrate::observe_symbol` signature matches between Phase A2 trait declaration and aggregator call in Phase C1.

**Scope:** Phase A is fully detailed and ready to execute today. Phases B-E have architecture detail but not microscopic TDD steps — each will be re-planned in its own dedicated planning pass when reached, since each phase produces independent commits and the user has explicitly chosen to walk through them iteratively.

---

## Execution

Run Phase A inline in the current session. Phases B-E will be planned in dedicated passes when their predecessor ships.
