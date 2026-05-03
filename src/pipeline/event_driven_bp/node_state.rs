//! Per-node state container for the event-driven substrate.
//!
//! Hot fields (current belief, prior, observed flag) live in a
//! `crossbeam_utils::atomic::AtomicCell` for lock-free reads on every
//! BP iteration. Cold fields (incoming-message inbox, neighbour list)
//! sit behind a [`parking_lot::Mutex`] because they grow per
//! observation and are touched less often.

use parking_lot::Mutex;
use smallvec::SmallVec;

use crate::pipeline::loopy_bp::N_STATES;

/// Lightweight, copy-able snapshot of belief state. Held in an
/// `AtomicCell` so concurrent readers (BP workers) never block on a
/// node's belief or prior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeStateLite {
    /// Current marginal belief over [Bull, Bear, Neutral]. Maintained
    /// at unit sum.
    pub belief: [f64; N_STATES],
    /// Prior from substrate evidence (sub-KG NodeId activations + KL
    /// surprise + lead-lag). Refreshed on every `observe_tick`.
    pub prior: [f64; N_STATES],
    /// Whether the prior is informed enough to act as evidence (vs
    /// uniform unobserved). Mirror of `loopy_bp::NodePrior::observed`.
    pub observed: bool,
    /// Last residual computed when this node's belief was updated.
    /// Drives the residual-BP scheduler — high values prioritise
    /// re-propagation, low values let the worker pool skip.
    pub last_residual: f64,
}

impl Default for NodeStateLite {
    fn default() -> Self {
        let uniform = 1.0 / N_STATES as f64;
        Self {
            belief: [uniform; N_STATES],
            prior: [uniform; N_STATES],
            observed: false,
            last_residual: 0.0,
        }
    }
}

/// Cold per-node fields. Inbox is the set of latest messages received
/// from each neighbour; neighbour list (with edge weights) is the
/// graph topology snapshot for this node.
#[derive(Debug, Default)]
pub struct NodeAux {
    /// `(neighbour_symbol, latest_message)` pairs. SmallVec inline
    /// capacity 8 covers the common case (avg degree ≈ 31 on master
    /// KG, but per-iteration touches are smaller).
    pub inbox: SmallVec<[(String, [f64; N_STATES]); 8]>,
    /// `(neighbour_symbol, edge_weight)`. Built once when the node is
    /// inserted; mutates only on topology refresh.
    pub neighbours: Vec<(String, f64)>,
}

impl NodeAux {
    /// Drop all pending messages in the inbox while keeping the
    /// neighbour topology intact. Used at tick boundaries to clear
    /// stale messages from prior priors — the event substrate's
    /// per-tick fixpoint semantics depend on each tick re-deriving
    /// messages from the *current* prior, not blending with damped
    /// remnants of the previous tick.
    pub fn clear_inbox(&mut self) {
        self.inbox.clear();
    }
}

/// Composite per-node state — fast path (`lite`) is lock-free,
/// slow path (`aux`) is mutex-guarded.
pub struct NodeState {
    pub lite: crossbeam_utils::atomic::AtomicCell<NodeStateLite>,
    pub aux: Mutex<NodeAux>,
}

impl Default for NodeState {
    fn default() -> Self {
        Self {
            lite: crossbeam_utils::atomic::AtomicCell::new(NodeStateLite::default()),
            aux: Mutex::new(NodeAux::default()),
        }
    }
}

impl NodeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot_lite(&self) -> NodeStateLite {
        self.lite.load()
    }

    pub fn store_lite(&self, value: NodeStateLite) {
        self.lite.store(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lite_lock_free_status_is_known() {
        // NodeStateLite is currently 56-64 bytes (belief + prior +
        // observed + last_residual + padding) — too large for the
        // ARM64 / x86_64 lock-free atomic-load bound (≤16 bytes).
        // crossbeam_utils::AtomicCell falls back to an internal
        // SeqLock on this platform. That's still wait-free for readers
        // when there's no contending writer, which matches our
        // low-contention pattern (one publisher per node per BP step).
        // We assert the *status* not the *value* — flagging if a
        // future refactor accidentally claims lock-free when it isn't,
        // or vice versa, both worth knowing about.
        let actual = crossbeam_utils::atomic::AtomicCell::<NodeStateLite>::is_lock_free();
        // On 64-bit Apple Silicon and modern x86_64, the 56-byte
        // payload exceeds the 16-byte lock-free bound, so we expect
        // false. If this ever flips to true (e.g., 64-byte CMPXCHG
        // landed), it's worth re-evaluating the SeqLock fallback path.
        assert!(
            !actual,
            "NodeStateLite unexpectedly became lock-free; revisit substrate hot path assumptions"
        );
    }

    #[test]
    fn default_belief_is_uniform() {
        let s = NodeStateLite::default();
        let uniform = 1.0 / N_STATES as f64;
        for v in s.belief.iter().chain(s.prior.iter()) {
            assert!((v - uniform).abs() < 1e-12);
        }
        assert!(!s.observed);
    }

    #[test]
    fn clear_inbox_drops_messages_keeps_neighbours() {
        let mut aux = NodeAux::default();
        aux.inbox.push(("A".to_string(), [0.5, 0.3, 0.2]));
        aux.inbox.push(("B".to_string(), [0.2, 0.6, 0.2]));
        aux.neighbours.push(("A".to_string(), 0.7));
        aux.neighbours.push(("B".to_string(), 0.4));
        aux.clear_inbox();
        assert!(aux.inbox.is_empty());
        assert_eq!(
            aux.neighbours.len(),
            2,
            "neighbour topology must survive inbox reset"
        );
    }
}
