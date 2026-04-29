//! [`BeliefSubstrate`] trait — the substitution seam between HK/US
//! runtimes and the BP engine. Phase B introduces it as a pure refactor;
//! Phase C plugs in an event-driven async impl behind the same surface.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::ontology::TacticalSetup;
use crate::pipeline::loopy_bp::{GraphEdge, NodePrior};

/// Snapshot of the BP posterior at one moment in substrate time.
///
/// `generation` is monotonic per substrate instance and increments on
/// every observation that changes the posterior. `last_updated`
/// timestamps the last observation. Both let downstream consumers
/// detect staleness without comparing the (potentially large) belief
/// map directly.
#[derive(Debug, Clone)]
pub struct PosteriorView {
    pub beliefs: HashMap<String, [f64; 3]>,
    pub iterations: usize,
    pub converged: bool,
    pub generation: u64,
    pub last_updated: DateTime<Utc>,
}

impl PosteriorView {
    /// Construct an empty initial view (generation 0). Used by substrate
    /// constructors before the first observation lands.
    pub fn empty() -> Self {
        Self {
            beliefs: HashMap::new(),
            iterations: 0,
            converged: false,
            generation: 0,
            last_updated: Utc::now(),
        }
    }
}

/// The substitution seam between runtime tick loop and the BP engine.
///
/// Both impls (sync, event) own internal posterior state; callers
/// observe and read via this trait. The trait is designed so the runtime
/// is identical between HK and US (modulo market-specific args), and so
/// future substrates (e.g. residual-BP, dataflow) plug in without
/// touching the runtime files.
///
/// **Concurrency contract**: `&self` (not `&mut self`) so substrates
/// can be shared via `Arc` and observed from multiple threads. Impls
/// use interior mutability.
pub trait BeliefSubstrate: Send + Sync {
    /// Push the latest tick observation. Sync substrate runs BP
    /// synchronously and stashes the resulting posterior; event
    /// substrate (Phase C) seeds residual-queue updates and returns
    /// immediately.
    fn observe_tick(&self, priors: &HashMap<String, NodePrior>, edges: &[GraphEdge], tick: u64);

    /// Read the current posterior. For sync substrate this is the
    /// just-computed state. For event substrate this is the latest
    /// converged-or-still-converging snapshot at query time. Cheap
    /// (Arc clone, no copy).
    fn posterior_snapshot(&self) -> Arc<PosteriorView>;

    /// Apply confidence to setups using the current posterior. Returns
    /// (applied, skipped). Bit-identical to the in-tick loop today.
    fn apply_posterior_confidence(&self, setups: &mut [TacticalSetup]) -> (usize, usize);

    /// Reconcile setup direction against current posterior. Same shape
    /// as `sub_kg_emergence::reconcile_direction_with_bp`. Returns
    /// the count of setups touched.
    fn reconcile_direction(&self, setups: &mut [TacticalSetup]) -> usize;

    /// Test-only hook. Sync impl: no-op (tick is synchronous so
    /// observe-then-assert always sees the latest state). Event impl
    /// (Phase C): blocks until the residual queue is empty and all
    /// workers are idle. Architectural invariant tests will assert
    /// this is called between `observe_tick` and `posterior_snapshot`
    /// in production code that wants a strict ordering.
    fn drain_pending(&self);
}
