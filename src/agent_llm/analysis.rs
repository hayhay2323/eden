use super::*;

#[path = "analysis/driver.rs"]
mod driver;
#[path = "analysis/fallback.rs"]
mod fallback;

pub use driver::run_analysis;
pub use fallback::{
    deterministic_analysis, run_or_fallback_analysis, spawn_analysis_if_enabled,
};
