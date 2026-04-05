use super::*;

#[path = "lens/causal.rs"]
mod causal;
#[path = "lens/engine.rs"]
mod engine;
#[path = "lens/iceberg.rs"]
mod iceberg;
#[path = "lens/lineage.rs"]
mod lineage;
#[path = "lens/structural.rs"]
mod structural;
#[path = "lens/types.rs"]
mod types;

pub(crate) use engine::default_lens_engine;
pub(crate) use types::{LensBundle, LensContext, LensObservation, LensPriority, SignalLens};

#[cfg(test)]
#[path = "lens/tests.rs"]
mod tests;
