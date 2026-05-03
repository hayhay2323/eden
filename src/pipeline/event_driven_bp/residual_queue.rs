//! Residual-priority queue for asynchronous BP scheduling.
//!
//! Each [`EdgeUpdate`] carries a residual magnitude. Workers pop the
//! highest-residual updates first — the canonical Residual BP
//! scheduler (Elidan/McGraw/Koller 2006, Sutton & McCallum 2007). A
//! shared `tokio::sync::Notify` wakes idle workers when the queue
//! transitions from empty to non-empty.
//!
//! Producers (the substrate's `observe_tick`, plus workers themselves
//! when they propagate updates to neighbours) call [`Self::push`].
//! Consumers (workers) call [`Self::pop`] which awaits when empty.
//!
//! ## Backpressure
//!
//! The heap is **bounded** at `max_size` (default = 200_000 updates,
//! ≈ 5 MB at 24 bytes per `EdgeUpdate`). On overflow, the *lowest-
//! residual* pending update is evicted to make room for the new one.
//! Rationale: residual ≡ how much this message would change a node's
//! belief. The smallest-residual updates are the least informative,
//! so dropping them sheds load while preserving the convergence-
//! relevant updates. Drops are counted in `dropped_count` and exposed
//! for telemetry; a steady increase signals workers can't keep up.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use ordered_float::OrderedFloat;
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::pipeline::loopy_bp::N_STATES;

/// Default upper bound on pending updates in the residual queue.
/// Picked so the queue costs at most ~5 MB of heap (24 bytes × 200 k
/// EdgeUpdates) — large enough that healthy HK PM session never hits
/// it (peak observed: ~50 k after a tick burst), small enough that a
/// runaway producer can't OOM the runtime.
pub const DEFAULT_MAX_QUEUE_SIZE: usize = 200_000;

/// One pending update from `from` to `to`: the message vector and the
/// residual that justified scheduling. Residual = `|m_new - m_prev|_∞`
/// per Sutton & McCallum's residual definition.
#[derive(Debug, Clone)]
pub struct EdgeUpdate {
    pub from: String,
    pub to: String,
    pub message: [f64; N_STATES],
    pub residual: f64,
}

impl PartialEq for EdgeUpdate {
    fn eq(&self, other: &Self) -> bool {
        self.residual == other.residual
    }
}

impl Eq for EdgeUpdate {}

// `EdgeUpdate` is no longer ordered directly — the queue stores
// `(OrderedFloat<f64>, seq)` keys in a `BTreeSet` for ordering. The
// `PartialEq`/`Eq` impls remain in case downstream code wants to
// compare residuals quickly.

/// Shared priority queue + notify. Many producers, many consumers.
///
/// Internal layout: a `BTreeSet` keyed by `(residual, seq)` provides
/// O(log N) min and max access; a parallel `HashMap` from `seq` to
/// `EdgeUpdate` carries the payload. The `seq` tiebreak makes
/// duplicate residuals well-ordered without losing any update. Push
/// and pop on a saturated queue are both O(log N), versus the prior
/// `BinaryHeap` impl whose overflow path was O(N) (linear scan +
/// drain + heapify) and serialized all producers under one mutex —
/// see livelock observed at tick=6732 on 2026-05-01.
pub struct ResidualQueue {
    inner: Mutex<ResidualQueueInner>,
    notify: Notify,
    max_size: usize,
    /// Cumulative count of updates dropped due to overflow. Production
    /// telemetry — a non-zero value means workers fell behind and
    /// some low-residual messages were skipped.
    dropped_count: AtomicU64,
}

struct ResidualQueueInner {
    /// Ordered set of `(residual, seq)` keys. `pop_first` / `pop_last`
    /// are O(log N).
    by_priority: BTreeSet<(OrderedFloat<f64>, u64)>,
    /// Payloads keyed by sequence number. The set's tiebreak `u64`
    /// is also the key here, so insertion is single-allocation.
    by_seq: HashMap<u64, EdgeUpdate>,
    /// Monotonic sequence counter — provides FIFO ordering for
    /// updates with identical residuals and avoids `BTreeSet` key
    /// collisions on duplicate priorities. Wraps after 2^64 pushes
    /// (≈ 5 ⨉ 10^11 years at 1 push/ns).
    next_seq: u64,
}

impl ResidualQueueInner {
    fn new() -> Self {
        Self {
            by_priority: BTreeSet::new(),
            by_seq: HashMap::new(),
            next_seq: 0,
        }
    }

    fn len(&self) -> usize {
        self.by_priority.len()
    }

    fn is_empty(&self) -> bool {
        self.by_priority.is_empty()
    }

    /// Insert `update`; assumes the caller has already enforced
    /// `max_size` (the function never evicts).
    fn push_unbounded(&mut self, update: EdgeUpdate) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.by_priority
            .insert((OrderedFloat(update.residual), seq));
        self.by_seq.insert(seq, update);
    }

    /// Remove and return the highest-residual update.
    fn pop_max(&mut self) -> Option<EdgeUpdate> {
        let (_, seq) = self.by_priority.pop_last()?;
        self.by_seq.remove(&seq)
    }

    /// Remove and return the lowest-residual update.
    fn pop_min(&mut self) -> Option<EdgeUpdate> {
        let (_, seq) = self.by_priority.pop_first()?;
        self.by_seq.remove(&seq)
    }

    /// Inspect the lowest residual without removing it.
    fn min_residual(&self) -> Option<f64> {
        self.by_priority.iter().next().map(|(r, _)| r.0)
    }
}

impl Default for ResidualQueue {
    fn default() -> Self {
        Self::with_max_size(DEFAULT_MAX_QUEUE_SIZE)
    }
}

impl ResidualQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            inner: Mutex::new(ResidualQueueInner::new()),
            notify: Notify::new(),
            max_size: max_size.max(1),
            dropped_count: AtomicU64::new(0),
        }
    }

    /// Enqueue an update. Wakes one waiting worker.
    ///
    /// If the queue is at `max_size`, evicts the *lowest-residual*
    /// pending update (least informative under residual BP) to make
    /// room. The new update is dropped instead if its own residual is
    /// already lower than every pending one — i.e. we never replace a
    /// high-priority message with a lower-priority one. Either way,
    /// `dropped_count` is incremented so operators can observe load
    /// shedding.
    pub fn push(&self, update: EdgeUpdate) {
        let mut inner = self.inner.lock();
        if inner.len() < self.max_size {
            inner.push_unbounded(update);
            drop(inner);
            self.notify.notify_one();
            return;
        }
        // Saturated. O(log N) min lookup via BTreeSet's first key.
        let min_residual = inner
            .min_residual()
            .expect("len() == max_size >= 1 implies at least one entry");
        if update.residual <= min_residual {
            // Incoming update is no more informative than the smallest
            // pending one — drop the incoming one instead of evicting
            // a still-useful entry.
            self.dropped_count.fetch_add(1, AtomicOrdering::Relaxed);
            return;
        }
        // Evict the lowest-residual entry (O(log N)) and insert the
        // new one (O(log N)). Total overflow cost: O(log N), versus
        // the prior O(N) drain + heapify.
        inner.pop_min();
        inner.push_unbounded(update);
        self.dropped_count.fetch_add(1, AtomicOrdering::Relaxed);
        drop(inner);
        self.notify.notify_one();
    }

    /// Pop the highest-residual pending update. If the queue is empty,
    /// awaits until a producer enqueues something or shutdown wakes us
    /// via `notify_waiters`.
    pub async fn pop(&self) -> EdgeUpdate {
        loop {
            if let Some(u) = self.inner.lock().pop_max() {
                return u;
            }
            self.notify.notified().await;
        }
    }

    /// Non-blocking variant. Returns `None` immediately if the queue
    /// is empty. Used by `drain_pending` and shutdown.
    pub fn try_pop(&self) -> Option<EdgeUpdate> {
        self.inner.lock().pop_max()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Snapshot of cumulative drops. Telemetry surface: a non-zero
    /// value means the queue overflowed and low-residual updates were
    /// shed. Polled by parity / health endpoints.
    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(AtomicOrdering::Relaxed)
    }

    /// Wake all waiters — used during graceful shutdown so workers
    /// observe the shutdown signal even when the queue is empty.
    pub fn wake_all(&self) {
        self.notify.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(from: &str, to: &str, residual: f64) -> EdgeUpdate {
        EdgeUpdate {
            from: from.to_string(),
            to: to.to_string(),
            message: [1.0 / 3.0; N_STATES],
            residual,
        }
    }

    #[test]
    fn equal_residuals_pop_in_lifo_order() {
        // Pin the tiebreak contract for pop on equal residuals.
        //
        // The `BTreeSet<(OrderedFloat, seq)>` ordering is
        // lexicographic ascending on both keys, so `pop_last` returns
        // (largest residual, largest seq) — i.e. LIFO within a tied
        // residual band. `pop_min` (eviction) is symmetric: smallest
        // residual + smallest seq → FIFO eviction of the oldest tied
        // pending update.
        //
        // True FIFO on the pop side would require a different shape
        // (e.g. `BTreeMap<OrderedFloat, VecDeque<seq>>`); the current
        // single-set design cannot give "ascending residual + descending
        // seq" simultaneously. Document the actual behaviour so a future
        // refactor that breaks it has to do so deliberately.
        let q = ResidualQueue::new();
        q.push(update("first", "x", 0.5));
        q.push(update("second", "x", 0.5));
        q.push(update("third", "x", 0.5));
        assert_eq!(q.try_pop().unwrap().from, "third");
        assert_eq!(q.try_pop().unwrap().from, "second");
        assert_eq!(q.try_pop().unwrap().from, "first");
    }

    #[test]
    fn pops_highest_residual_first() {
        // Residual scheduler invariant: messages with the largest
        // delta from the previous iteration are processed first. This
        // is the convergence-speed lever vs naive synchronous BP.
        let q = ResidualQueue::new();
        q.push(update("A", "B", 0.10));
        q.push(update("C", "D", 0.50));
        q.push(update("E", "F", 0.30));
        let first = q.try_pop().unwrap();
        assert_eq!(first.residual, 0.50);
        let second = q.try_pop().unwrap();
        assert_eq!(second.residual, 0.30);
        let third = q.try_pop().unwrap();
        assert_eq!(third.residual, 0.10);
        assert!(q.try_pop().is_none());
    }

    #[tokio::test]
    async fn pop_awaits_until_producer_pushes() {
        let q = std::sync::Arc::new(ResidualQueue::new());
        let q_clone = q.clone();
        let pop_handle = tokio::spawn(async move { q_clone.pop().await });
        // Yield so the consumer reaches the await point.
        tokio::task::yield_now().await;
        q.push(update("A", "B", 1.0));
        let popped = pop_handle.await.unwrap();
        assert_eq!(popped.residual, 1.0);
    }

    #[test]
    fn overflow_evicts_lowest_residual_keeps_high() {
        // Backpressure invariant: when the queue is full, the *least
        // informative* pending update (smallest residual) is shed
        // first, never the high-residual ones that drive convergence.
        let q = ResidualQueue::with_max_size(3);
        q.push(update("A", "B", 0.10));
        q.push(update("C", "D", 0.50));
        q.push(update("E", "F", 0.30));
        // Queue full. Pushing residual 0.40 should evict the 0.10 one.
        q.push(update("G", "H", 0.40));
        assert_eq!(q.len(), 3);
        assert_eq!(q.dropped_count(), 1);
        let mut residuals: Vec<f64> = (0..3).map(|_| q.try_pop().unwrap().residual).collect();
        residuals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(residuals, vec![0.30, 0.40, 0.50]);
    }

    #[test]
    fn overflow_drops_incoming_when_residual_lower_than_min() {
        // Incoming update less informative than every pending one →
        // drop the incoming one, leave the heap unchanged. Avoids
        // pointless heap churn and keeps the highest-residual mix.
        let q = ResidualQueue::with_max_size(2);
        q.push(update("A", "B", 0.50));
        q.push(update("C", "D", 0.30));
        // 0.20 < min(0.50, 0.30) = 0.30 → drop incoming.
        q.push(update("E", "F", 0.20));
        assert_eq!(q.len(), 2);
        assert_eq!(q.dropped_count(), 1);
        let first = q.try_pop().unwrap();
        let second = q.try_pop().unwrap();
        assert_eq!(first.residual, 0.50);
        assert_eq!(second.residual, 0.30);
    }

    #[test]
    fn overflow_rebuild_path_keeps_len_at_max_size() {
        // Pins the structural contract: when incoming residual strictly
        // exceeds the current min, push goes through the eviction-rebuild
        // branch and the queue length stays at max_size. Any future fix
        // that changes the overflow strategy must update this test.
        let q = ResidualQueue::with_max_size(3);
        q.push(update("A", "B", 0.10));
        q.push(update("C", "D", 0.50));
        q.push(update("E", "F", 0.30));
        let len_before = q.len();
        let drops_before = q.dropped_count();

        q.push(update("G", "H", 0.20)); // 0.20 > min=0.10 → rebuild
        q.push(update("I", "J", 0.40)); // 0.40 > min=0.20 → rebuild

        assert_eq!(q.len(), len_before, "rebuild must keep len bounded");
        assert_eq!(
            q.dropped_count(),
            drops_before + 2,
            "every overflow push above min must register one eviction"
        );
    }

    #[test]
    fn overflow_push_must_not_be_linear_in_max_size() {
        // Regression canary for the residual_queue cold-start livelock.
        //
        // When the heap is saturated and incoming residuals strictly
        // exceed the current min on every push, the current
        // implementation pays O(N) under `Mutex<BinaryHeap>` per push:
        //   (1) O(N) linear scan to locate min (line 124)
        //   (2) O(N) `heap.drain().collect()` into Vec (line 139)
        //   (3) O(N) `BinaryHeap::from(keep)` Floyd heapify (line 142)
        // The mutex is held the entire time, so producers serialize and
        // consumers calling `pop` are starved.
        //
        // Empirically: PID 8729 froze at tick=6732 for 5+ minutes at
        // 99.7% CPU under exactly this workload (cold-start BP residual
        // bursts whose magnitudes exceed the persisted-state seed mins).
        //
        // This test fails on the O(N)-per-push implementation and passes
        // on any O(log N)-per-push eviction strategy (e.g., min-max
        // double-ended priority queue, BTreeMap keyed by residual).
        const MAX_SIZE: usize = 2_048;
        const OVERFLOW_PUSHES: usize = 20_000;
        const BUDGET: std::time::Duration = std::time::Duration::from_secs(1);

        let q = ResidualQueue::with_max_size(MAX_SIZE);
        // Saturate at residual=0.0 so every subsequent push is strictly
        // above the current min — guarantees rebuild branch on every
        // overflow push.
        for _ in 0..MAX_SIZE {
            q.push(update("seed", "seed", 0.0));
        }
        assert_eq!(q.len(), MAX_SIZE, "saturation phase must fill heap");
        let drops_after_seed = q.dropped_count();

        let started = std::time::Instant::now();
        for i in 0..OVERFLOW_PUSHES {
            // Strictly increasing residuals → always > current min →
            // always trigger eviction-rebuild branch (never early-drop).
            let residual = 1.0 + (i as f64) * 1e-9;
            q.push(update("hot", "hot", residual));
        }
        let elapsed = started.elapsed();

        assert_eq!(q.len(), MAX_SIZE, "overflow path must keep len bounded");
        assert_eq!(
            q.dropped_count() - drops_after_seed,
            OVERFLOW_PUSHES as u64,
            "every overflow push at residual > min must register an eviction \
             (proves rebuild branch was hit, not early-drop)"
        );

        // Threshold derivation: O(log N) per push at MAX_SIZE=2048 is
        // ~11 comparisons; 20_000 pushes ≈ 220 K cmps, comfortably
        // under 100 ms even in debug mode. The current O(N) impl runs
        // ≈ 20_000 * 2_048 = 41 M element-ops + heap allocations,
        // observed > 1 s in debug. A 1-second ceiling is far above any
        // reasonable O(log N) fix and well below the broken baseline.
        assert!(
            elapsed < BUDGET,
            "{OVERFLOW_PUSHES} overflow pushes at max_size={MAX_SIZE} took {elapsed:?} \
             (budget {BUDGET:?}); indicates O(N)-per-push regression in \
             ResidualQueue::push — see livelock observed at tick=6732 on 2026-05-01"
        );
    }

    #[test]
    fn unbounded_via_default_max_size_doesnt_drop() {
        // Default max_size = DEFAULT_MAX_QUEUE_SIZE. Tiny synthetic
        // workload must not trigger any drops.
        let q = ResidualQueue::new();
        for i in 0..100 {
            q.push(update("X", "Y", 0.01 * (i as f64)));
        }
        assert_eq!(q.len(), 100);
        assert_eq!(q.dropped_count(), 0);
    }
}
