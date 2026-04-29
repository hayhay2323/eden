//! Event-driven Belief Propagation substrate.
//!
//! Single substrate as of 2026-04-29: [`EventDrivenSubstrate`] is the
//! production BP path. The sync + shadow variants were deleted once
//! the event substrate's per-tick fixpoint semantics were restored
//! (`observe_tick` clears node inboxes when a prior changes, so each
//! tick re-derives messages from the fresh prior).
//!
//! Architecture: `Arc<DashMap>` shared graph state, async worker pool
//! drains a bounded residual queue, 75 ms publisher refreshes an
//! `ArcSwap<PosteriorView>` for wait-free reads.

pub mod event_substrate;
pub mod node_state;
pub mod residual_queue;
pub mod substrate;
pub mod worker_pool;

pub use event_substrate::{EventConfig, EventDrivenSubstrate};
pub use substrate::{BeliefSubstrate, PosteriorView};
