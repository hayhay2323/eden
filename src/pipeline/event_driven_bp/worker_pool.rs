//! Bounded async worker pool draining the residual queue.
//!
//! 4–8 tokio tasks loop pop → recompute neighbour beliefs → push
//! outgoing updates back into the queue. This is the canonical
//! Residual BP scheduler shape — see Elidan/McGraw/Koller 2006 §4.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::watch;

use crate::pipeline::loopy_bp::N_STATES;

use super::node_state::NodeState;
use super::residual_queue::{EdgeUpdate, ResidualQueue};

/// Handle to the spawned worker pool. Drop to keep workers running;
/// call [`Self::shutdown`] to stop them. Workers track idleness so
/// `drain_pending` can spin-wait for quiescence.
pub struct WorkerPoolHandle {
    shutdown_tx: watch::Sender<bool>,
    /// Number of workers currently waiting on the queue (idle). When
    /// `idle == workers && queue.is_empty()` the pool is quiescent.
    pub idle: Arc<AtomicUsize>,
    pub workers: usize,
}

impl WorkerPoolHandle {
    pub fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
    }
}

/// Spawn `workers` tasks that drain the residual queue. Each worker
/// loops: await `pop` → call `process_fn` → push outgoing updates.
///
/// `process_fn` is the substrate-defined message-update function. It
/// reads the destination node's `lite` snapshot, multiplies in the
/// new message, normalises, computes outgoing updates to neighbours,
/// and returns them. It does NOT push to the queue itself — the
/// pool drives the dispatch.
pub fn spawn_worker_pool<F>(
    nodes: Arc<DashMap<String, Arc<NodeState>>>,
    queue: Arc<ResidualQueue>,
    workers: usize,
    process_fn: F,
) -> WorkerPoolHandle
where
    F: Fn(
            &Arc<DashMap<String, Arc<NodeState>>>,
            &EdgeUpdate,
        ) -> Vec<EdgeUpdate>
        + Send
        + Sync
        + 'static,
{
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let idle = Arc::new(AtomicUsize::new(workers));
    let process_fn = Arc::new(process_fn);
    for _ in 0..workers {
        let nodes = Arc::clone(&nodes);
        let queue = Arc::clone(&queue);
        let idle = Arc::clone(&idle);
        let process_fn = Arc::clone(&process_fn);
        let mut shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                let update = tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            break;
                        }
                        continue;
                    }
                    u = queue.pop() => u,
                };
                idle.fetch_sub(1, Ordering::Relaxed);
                let outgoing = (process_fn)(&nodes, &update);
                for u in outgoing {
                    queue.push(u);
                }
                idle.fetch_add(1, Ordering::Relaxed);
            }
        });
    }
    WorkerPoolHandle {
        shutdown_tx,
        idle,
        workers,
    }
}

/// Multiply a message vector into a 3-cell distribution and re-normalise.
/// Common helper used by substrate's `process_fn`.
pub fn multiply_and_normalise(
    state: &mut [f64; N_STATES],
    msg: &[f64; N_STATES],
) {
    for i in 0..N_STATES {
        state[i] *= msg[i];
    }
    let sum: f64 = state.iter().sum();
    if sum < 1e-9 {
        let uniform = 1.0 / N_STATES as f64;
        for v in state.iter_mut() {
            *v = uniform;
        }
    } else {
        for v in state.iter_mut() {
            *v /= sum;
        }
    }
}

/// Compute residual = max |a - b| across N_STATES.
pub fn residual(a: &[f64; N_STATES], b: &[f64; N_STATES]) -> f64 {
    let mut m = 0.0_f64;
    for i in 0..N_STATES {
        let d = (a[i] - b[i]).abs();
        if d > m {
            m = d;
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_uniform_keeps_state_unchanged() {
        // Multiplying by a uniform message must not bias the
        // distribution. Sanity check the 0-residual baseline.
        let mut s = [0.4, 0.3, 0.3];
        multiply_and_normalise(&mut s, &[1.0 / 3.0; 3]);
        assert!((s[0] - 0.4).abs() < 1e-12);
        assert!((s[1] - 0.3).abs() < 1e-12);
        assert!((s[2] - 0.3).abs() < 1e-12);
    }

    #[test]
    fn multiply_underflow_falls_back_to_uniform() {
        // Same underflow guard as `loopy_bp::normalize`. If a long
        // chain of multiplications collapses every cell to 0, return
        // honest "I don't know" rather than `[0,0,0]`.
        let mut s = [0.0_f64; 3];
        multiply_and_normalise(&mut s, &[1.0; 3]);
        let uniform = 1.0 / 3.0;
        for v in &s {
            assert!((v - uniform).abs() < 1e-12);
        }
    }

    #[test]
    fn residual_is_max_abs_diff() {
        let a = [0.5, 0.3, 0.2];
        let b = [0.4, 0.4, 0.2];
        let r = residual(&a, &b);
        assert!((r - 0.1).abs() < 1e-12);
    }
}
