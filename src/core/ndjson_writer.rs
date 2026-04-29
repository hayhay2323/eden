//! Background NDJSON writer.
//!
//! Phase A of the event-driven BP migration (2026-04-29). Each artifact
//! kind owns one [`NdjsonWriter<T>`] that holds a sender to a dedicated
//! `tokio::spawn` task. The task pulls batches off a bounded `mpsc` and
//! invokes a caller-provided closure to perform the actual file append.
//!
//! The runtime tick body used to call the per-module sync writer
//! (`write_marginals`, `write_frame`, `snapshot_to_ndjson`, …)
//! synchronously inside the consumer task; that blocked the consumer for
//! tens of milliseconds per write and was a major contributor to the
//! `push_channel_full` HK overflow under live regular-session load.
//! With the writer running on its own task, the consumer just
//! `try_send_batch`s and moves on.
//!
//! **Why a closure rather than a generic `Serialize` impl**: each
//! existing writer in `pipeline/` has its own envelope shape (some use
//! [`RuntimeArtifactStore`], some write raw `serde_json` lines, some
//! iterate a registry to derive multiple lines). Rather than re-flatten
//! all of those, the writer task just calls back into the per-module
//! sync function. Phase A is purely a scheduling change, not a
//! file-format change.
//!
//! **Drop policy**: on `try_send_batch` failure (Full), the *new* batch
//! is dropped and a counter increments. Operator artifacts are
//! eventually-fresh observability data — losing a single batch under
//! sustained pressure is acceptable; losing all WS push events because
//! the consumer was busy is not. Dropping new (vs dropping old) is
//! simpler and equivalent in steady-state log loss.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc::{self, error::TrySendError};

/// Default bounded-channel capacity. Small (4) by design — the writer
/// task is fast (single file append per row) and a deeper buffer just
/// hides backpressure from operators. Tune up only if telemetry shows
/// sustained drops on a healthy market session.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 4;

/// One-shot generic NDJSON writer for a single artifact kind.
///
/// `T` is whatever payload the per-tick caller has on hand
/// (`Vec<MarginalRow>`, `VisualGraphFrame`, `Vec<String>`, …). The
/// writer task invokes the closure provided to [`Self::spawn`] for each
/// received batch.
pub struct NdjsonWriter<T> {
    tx: mpsc::Sender<T>,
    drops: Arc<AtomicUsize>,
    sends: Arc<AtomicUsize>,
}

impl<T: Send + 'static> NdjsonWriter<T> {
    /// Spawn the writer task with the default channel capacity.
    ///
    /// `name` is used only for error logs (e.g. `"hk:bp_marginals"`).
    /// `write_fn` is invoked once per received batch and may perform
    /// arbitrary blocking IO — it runs on the writer task, not on the
    /// runtime consumer.
    pub fn spawn<F>(name: impl Into<String>, write_fn: F) -> Self
    where
        F: FnMut(T) -> std::io::Result<usize> + Send + 'static,
    {
        Self::spawn_with_capacity(name, write_fn, DEFAULT_CHANNEL_CAPACITY)
    }

    pub fn spawn_with_capacity<F>(
        name: impl Into<String>,
        mut write_fn: F,
        capacity: usize,
    ) -> Self
    where
        F: FnMut(T) -> std::io::Result<usize> + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel::<T>(capacity.max(1));
        let drops = Arc::new(AtomicUsize::new(0));
        let sends = Arc::new(AtomicUsize::new(0));
        let name = name.into();
        tokio::spawn(async move {
            while let Some(batch) = rx.recv().await {
                if let Err(err) = write_fn(batch) {
                    eprintln!("[ndjson_writer:{name}] append failed: {err}");
                }
            }
        });
        Self { tx, drops, sends }
    }

    /// Try to enqueue a batch. Non-blocking. On full, the batch is
    /// dropped and the drop counter increments.
    pub fn try_send_batch(&self, batch: T) -> Result<(), TrySendError<T>> {
        match self.tx.try_send(batch) {
            Ok(()) => {
                self.sends.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(err @ TrySendError::Closed(_)) => Err(err),
            Err(err @ TrySendError::Full(_)) => {
                self.drops.fetch_add(1, Ordering::Relaxed);
                Err(err)
            }
        }
    }

    /// Drop counter — total batches rejected for `Full`. Operator-facing
    /// telemetry can surface this to detect sustained backpressure.
    pub fn drops(&self) -> usize {
        self.drops.load(Ordering::Relaxed)
    }

    /// Send counter — total batches successfully enqueued.
    pub fn sends(&self) -> usize {
        self.sends.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn try_send_batch_drops_on_full_and_preserves_count() {
        // Capacity 4 with no consumer reading: first 4 batches succeed,
        // the rest get dropped. Drop counter must reflect the rejected
        // count exactly. This protects the operator-visible drop metric.
        let capacity = 4;
        let (tx, _rx) = mpsc::channel::<Vec<u64>>(capacity);
        let drops = Arc::new(AtomicUsize::new(0));
        let sends = Arc::new(AtomicUsize::new(0));
        let writer: NdjsonWriter<Vec<u64>> = NdjsonWriter {
            tx,
            drops: drops.clone(),
            sends: sends.clone(),
        };
        let mut rejected = 0usize;
        for n in 0..1000u64 {
            if writer.try_send_batch(vec![n]).is_err() {
                rejected += 1;
            }
        }
        assert_eq!(writer.sends(), capacity);
        assert_eq!(writer.drops(), 1000 - capacity);
        assert_eq!(rejected, 1000 - capacity);
    }

    #[tokio::test]
    async fn drained_consumer_keeps_drops_at_zero() {
        // With a draining consumer, drops should stay at 0 across many
        // batches. Sanity check the happy path.
        let drained = Arc::new(AtomicUsize::new(0));
        let drained_clone = drained.clone();
        let writer: NdjsonWriter<Vec<u64>> = NdjsonWriter::spawn("test", move |batch: Vec<u64>| {
            drained_clone.fetch_add(batch.len(), Ordering::Relaxed);
            Ok(batch.len())
        });
        for n in 0..200u64 {
            writer
                .try_send_batch(vec![n])
                .expect("draining consumer should keep capacity available");
            // Yield so the consumer task has a chance to drain. Without
            // this, the spawn'd consumer never runs and the bounded
            // channel fills — that's a test artefact, not a real
            // backpressure scenario (production callers don't loop in a
            // tight sender block).
            tokio::task::yield_now().await;
        }
        // Final drain window for the last few in-flight batches.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(writer.drops(), 0);
        assert_eq!(writer.sends(), 200);
        assert_eq!(drained.load(Ordering::Relaxed), 200);
    }

    #[tokio::test]
    async fn closure_io_error_does_not_kill_writer_task() {
        // If the write closure returns Err, the task must keep draining
        // subsequent batches (operator should still see the drop counter
        // increment on subsequent Full, not have the channel deadlock).
        let attempted = Arc::new(AtomicUsize::new(0));
        let attempted_clone = attempted.clone();
        let writer: NdjsonWriter<Vec<u64>> =
            NdjsonWriter::spawn("test_err", move |batch: Vec<u64>| {
                attempted_clone.fetch_add(batch.len(), Ordering::Relaxed);
                Err(std::io::Error::new(std::io::ErrorKind::Other, "synthetic"))
            });
        for n in 0..10u64 {
            writer
                .try_send_batch(vec![n])
                .expect("draining consumer should keep capacity available");
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(attempted.load(Ordering::Relaxed), 10);
    }
}
