//! Push-event bus for sub-tick pressure recomputation.
//!
//! The longport push consumer demultiplexes incoming events into
//! [`PressureEvent`]s and publishes them to a bounded mpsc with
//! drop-oldest semantics. Per-channel worker tasks (added in Phase C)
//! drain the bus, update incremental state, and notify the per-symbol
//! aggregator which writes into sub-KG and calls
//! `BeliefSubstrate::observe_symbol`.
//!
//! Phase B (this module): bus + demux only. No workers yet — a single
//! drainer in the runtime counts events for liveness check.

pub mod aggregator;
pub mod bus;
pub mod channel_state;
pub mod event;
pub mod worker;

pub use aggregator::{spawn_aggregator, AggregatorHandle};
pub use bus::{spawn_bus, EventBusHandle};
pub use channel_state::{ChannelStates, SharedChannelStates};
pub use event::{demux_push_event, PressureEvent, TradeSide};
pub use worker::{spawn_worker_pool, WorkerPoolHandles};
