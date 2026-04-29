//! Offline dreaming — reads two belief_snapshot rows for a market and
//! computes a delta report: what changed in Eden's attention + posterior
//! between morning and evening?
//!
//! See binary entry at `src/bin/dream.rs`.

pub mod report;

pub use report::{
    compute_diff, render_markdown, AttentionChange, DreamReport, FieldGrowth, PosteriorShift,
    SnapshotSummary,
};
