use super::*;

#[path = "lens/types.rs"]
mod types;
#[path = "lens/engine.rs"]
mod engine;
#[path = "lens/iceberg.rs"]
mod iceberg;
#[path = "lens/structural.rs"]
mod structural;
#[path = "lens/causal.rs"]
mod causal;
#[path = "lens/lineage.rs"]
mod lineage;

pub(crate) use engine::default_lens_engine;
pub(crate) use types::{LensBundle, LensContext, LensObservation, LensPriority, SignalLens};

#[cfg(test)]
#[path = "lens/tests.rs"]
mod tests;
