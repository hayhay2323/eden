//! Per-stage wall-clock accounting inside the tick loop.
//!
//! Stages are anonymous sections of work between `mark()` calls. The
//! caller constructs one timer per tick, calls `mark("stage_name")`
//! at each boundary, then asks for `top_n` to surface the stages
//! that ate the most wall time. Stage names are required to be
//! `&'static str` so the timer never allocates inside the hot loop.

#![allow(dead_code)]

use std::time::{Duration, Instant};

pub struct TickStageTimer {
    accum: Vec<(&'static str, Duration)>,
    last_mark: Instant,
}

impl TickStageTimer {
    /// Begin timing. The duration measured by the *first* `mark` call
    /// is the work between construction and that mark.
    pub fn new() -> Self {
        Self {
            accum: Vec::with_capacity(32),
            last_mark: Instant::now(),
        }
    }

    /// Close the current section, attributing its elapsed time to
    /// `stage`. Starts a new section with the same `Instant::now()`
    /// so the next `mark` call's duration begins from this point.
    pub fn mark(&mut self, stage: &'static str) {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_mark);
        self.accum.push((stage, elapsed));
        self.last_mark = now;
    }

    /// Sorted descending — top contributors first. Limited to `n`.
    pub fn top_n(&self, n: usize) -> Vec<(&'static str, Duration)> {
        let mut sorted: Vec<(&'static str, Duration)> = self.accum.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        sorted.truncate(n);
        sorted
    }

    /// Sum of every recorded section. Useful for sanity checks
    /// (should be ≤ tick wall time).
    pub fn total(&self) -> Duration {
        self.accum.iter().map(|(_, d)| *d).sum()
    }
}

impl Default for TickStageTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn empty_timer_yields_no_top_entries() {
        let timer = TickStageTimer::new();
        assert!(timer.top_n(5).is_empty());
        assert_eq!(timer.total(), Duration::ZERO);
    }

    #[test]
    fn mark_records_section_durations() {
        let mut timer = TickStageTimer::new();
        sleep(Duration::from_millis(10));
        timer.mark("alpha");
        sleep(Duration::from_millis(2));
        timer.mark("beta");
        let entries: std::collections::HashMap<&'static str, Duration> =
            timer.top_n(10).into_iter().collect();
        let alpha = entries
            .get("alpha")
            .copied()
            .expect("alpha must be recorded");
        let beta = entries
            .get("beta")
            .copied()
            .expect("beta must be recorded");
        // Wall-clock can drift but alpha > beta should be safe given
        // a 5x ratio in the sleep durations.
        assert!(
            alpha > beta,
            "alpha {alpha:?} should outweigh beta {beta:?} (5x sleep ratio)"
        );
        assert!(
            alpha >= Duration::from_millis(8),
            "alpha {alpha:?} should be ~10 ms; allow scheduler slop"
        );
    }

    #[test]
    fn top_n_orders_by_duration_desc() {
        let mut timer = TickStageTimer::new();
        sleep(Duration::from_millis(1));
        timer.mark("short");
        sleep(Duration::from_millis(5));
        timer.mark("long");
        sleep(Duration::from_millis(2));
        timer.mark("mid");
        let top = timer.top_n(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "long", "longest first");
        assert_eq!(top[1].0, "mid", "second-longest second");
    }

    #[test]
    fn total_sums_all_sections() {
        let mut timer = TickStageTimer::new();
        sleep(Duration::from_millis(3));
        timer.mark("a");
        sleep(Duration::from_millis(4));
        timer.mark("b");
        let total = timer.total();
        // Loose lower bound — a + b combined sleep is ≥7ms.
        assert!(
            total >= Duration::from_millis(6),
            "total {total:?} should be ≥6ms (3+4 minus scheduler granularity)"
        );
    }
}
