//! Event-driven Belief Propagation substrate.
//!
//! Phase B of the migration (2026-04-29): introduce a [`BeliefSubstrate`]
//! trait that the HK + US runtimes route through, and a
//! [`SyncTickSubstrate`] impl that wraps the existing tick-batched BP
//! call (`loopy_bp::run_with_messages` + `apply_posterior_confidence` +
//! `reconcile_direction_with_bp`) bit-identically. Phase B ships
//! pure-refactor — no behaviour change.
//!
//! Phase C will add `EventDrivenSubstrate` implementing the same trait
//! with an asynchronous Residual BP scheduler over `Arc<DashMap>` shared
//! graph state. Both substrates ship side-by-side under
//! `EDEN_SUBSTRATE=sync|event|shadow` env-var selection.

pub mod substrate;
pub mod sync_substrate;

pub use substrate::{BeliefSubstrate, PosteriorView};
pub use sync_substrate::SyncTickSubstrate;
