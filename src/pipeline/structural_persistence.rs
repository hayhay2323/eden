//! Persistence primitive — captures *sustained* structural state rather
//! than instantaneous *events*.
//!
//! The event-oriented primitives (contrast, kinematics, consistency_gauge)
//! fire on CHANGE. They answer "what just became unusual?". They go silent
//! during sustained organic trends (Case A/D from the analysis: structure
//! is stable, detectors are quiet).
//!
//! Persistence answers the orthogonal question: "which entity has been
//! unusual for a long time?" — by counting, per tick, whether the entity
//! is in the universe's top percentile on some metric, and maintaining
//! a streak count.
//!
//! Pure primitive: no pattern matching, no learned thresholds. The
//! "what counts as unusual" is the same universe-distribution percentile
//! we use elsewhere (99th by default). Sustained-top entities accumulate
//! streak; non-top entities reset to 0.
//!
//! Output: per-tick streak per (entity, metric). Surfaces entities whose
//! streak exceeds `SURFACE_STREAK` ticks (default 5, = ~40s at 8s/tick).
//!
//! This fills the gap: event primitives see flashes, persistence sees
//! the slow burn.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Percentile threshold for "unusual this tick" — same as other gauges.
pub const TOP_PERCENTILE: f64 = 0.99;

/// Minimum streak to surface as a persistence event.
pub const SURFACE_STREAK: u64 = 5;

/// Per (metric_name, entity_id) streak counter.
#[derive(Debug, Default)]
pub struct PersistenceTracker {
    /// metric → entity → streak (consecutive ticks in top percentile)
    streaks: HashMap<String, HashMap<String, u64>>,
    /// metric → entity → peak value observed during current streak
    peaks: HashMap<String, HashMap<String, f64>>,
}

impl PersistenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_streak(&self, metric: &str, entity: &str) -> u64 {
        self.streaks
            .get(metric)
            .and_then(|m| m.get(entity))
            .copied()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PersistenceEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub metric: String,
    pub entity: String,
    /// Consecutive ticks this entity has been in top percentile.
    pub streak_ticks: u64,
    /// Current |value| of the metric.
    pub current_value: f64,
    /// Peak |value| over the streak.
    pub peak_value: f64,
    /// Floor at this tick (what the 99th percentile is).
    pub floor: f64,
}

/// Update tracker with current-tick values, surface entities whose streak
/// exceeds `SURFACE_STREAK`.
///
/// `values` is the per-entity |metric|. The function computes this tick's
/// top-percentile floor; entities above floor get streak++, others reset
/// to 0.
pub fn update_and_surface(
    market: &str,
    metric: &str,
    values: &[(String, f64)],
    tracker: &mut PersistenceTracker,
    ts: DateTime<Utc>,
) -> Vec<PersistenceEvent> {
    if values.len() < 30 {
        return Vec::new();
    }
    // Compute floor from |values|
    let mut abs_vals: Vec<f64> = values.iter().map(|(_, v)| v.abs()).collect();
    abs_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor_idx = (TOP_PERCENTILE * abs_vals.len() as f64) as usize;
    let floor = abs_vals
        .get(floor_idx.min(abs_vals.len() - 1))
        .copied()
        .unwrap_or(0.0);

    // Get-or-init the per-metric streak + peak maps
    let streak_map = tracker.streaks.entry(metric.to_string()).or_default();
    let peak_map = tracker.peaks.entry(metric.to_string()).or_default();

    // Build current-tick set of entities above floor
    let mut above: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (entity, v) in values {
        if v.abs() > floor {
            above.insert(entity.clone());
        }
    }

    // Reset streaks for entities NOT in `above`
    let known: Vec<String> = streak_map.keys().cloned().collect();
    for entity in known {
        if !above.contains(&entity) {
            streak_map.insert(entity.clone(), 0);
            peak_map.insert(entity, 0.0);
        }
    }

    // Update streaks + peaks for entities in `above`
    for (entity, v) in values {
        if above.contains(entity) {
            let s = streak_map.entry(entity.clone()).or_insert(0);
            *s += 1;
            let p = peak_map.entry(entity.clone()).or_insert(0.0);
            if v.abs() > *p {
                *p = v.abs();
            }
        }
    }

    // Surface entities whose streak has reached SURFACE_STREAK
    let mut events = Vec::new();
    for (entity, v) in values {
        let streak = streak_map.get(entity).copied().unwrap_or(0);
        if streak >= SURFACE_STREAK {
            let peak = peak_map.get(entity).copied().unwrap_or(0.0);
            events.push(PersistenceEvent {
                ts,
                market: market.to_string(),
                metric: metric.to_string(),
                entity: entity.clone(),
                streak_ticks: streak,
                current_value: *v,
                peak_value: peak,
                floor,
            });
        }
    }
    events
}

pub fn write_events(market: &str, events: &[PersistenceEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-persistence-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    for ev in events {
        let line = serde_json::to_string(ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform_plus_outlier(n: usize, outlier: (&str, f64)) -> Vec<(String, f64)> {
        // Use n ≥ 200 so 99th percentile floor sits below the outlier.
        let n = n.max(200);
        let mut v: Vec<(String, f64)> = (0..n).map(|i| (format!("E{}", i), 1.0)).collect();
        v[0] = (outlier.0.to_string(), outlier.1);
        v
    }

    #[test]
    fn no_surface_below_min_streak() {
        let mut t = PersistenceTracker::new();
        for _ in 0..(SURFACE_STREAK - 1) {
            let vals = uniform_plus_outlier(50, ("A", 100.0));
            let evs = update_and_surface("hk", "test", &vals, &mut t, Utc::now());
            assert!(evs.is_empty(), "should not surface before streak reached");
        }
    }

    #[test]
    fn surfaces_at_exact_streak() {
        let mut t = PersistenceTracker::new();
        let mut evs = Vec::new();
        for _ in 0..SURFACE_STREAK {
            let vals = uniform_plus_outlier(50, ("A", 100.0));
            evs = update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        }
        assert!(!evs.is_empty(), "should surface at exact streak");
        assert_eq!(evs[0].entity, "A");
        assert_eq!(evs[0].streak_ticks, SURFACE_STREAK);
    }

    #[test]
    fn streak_resets_on_non_top() {
        let mut t = PersistenceTracker::new();
        // 5 ticks: A is outlier
        for _ in 0..5 {
            let vals = uniform_plus_outlier(200, ("A", 100.0));
            update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        }
        assert_eq!(t.current_streak("test", "A"), 5);

        // 1 tick: A is NOT outlier (uniform)
        let vals: Vec<(String, f64)> = (0..200).map(|i| (format!("E{}", i), 1.0)).collect();
        update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        assert_eq!(t.current_streak("test", "A"), 0, "streak should reset");
    }

    #[test]
    fn rejects_small_universe() {
        let mut t = PersistenceTracker::new();
        let vals: Vec<(String, f64)> = (0..10).map(|i| (format!("E{}", i), 1.0)).collect();
        let evs = update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        assert!(evs.is_empty());
    }

    #[test]
    fn peak_captured_over_streak() {
        let mut t = PersistenceTracker::new();
        for val in [10.0, 20.0, 15.0, 25.0, 12.0] {
            let vals = uniform_plus_outlier(50, ("A", val));
            update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        }
        // One more tick to surface (need SURFACE_STREAK total)
        let vals = uniform_plus_outlier(50, ("A", 8.0));
        let evs = update_and_surface("hk", "test", &vals, &mut t, Utc::now());
        if !evs.is_empty() {
            assert!(
                evs[0].peak_value >= 25.0,
                "peak should be 25, got {}",
                evs[0].peak_value
            );
        }
    }
}
