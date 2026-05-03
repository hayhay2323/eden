//! Sector kinematics — temporal derivatives at sector level.
//!
//! `structural_kinematics` does this per symbol — first/second time
//! derivatives + zero-crossing turning points on per-symbol activations
//! and force balance.
//!
//! `sector_kinematics` does the same one zoom level up — per-sector
//! mean activation. Lets Eden answer "the SECTOR is forming a
//! top/bottom" not just "this individual symbol is forming a top".
//!
//! Sector signals are smoother than symbol signals (averaged across many
//! members), so a sector turning point is a stronger structural event
//! than any single member turning. When utility sector mean
//! IntentAccumulation flips from rising to falling, it's a regime hint.
//!
//! Method (mirror of `structural_kinematics`):
//!   - Maintain per-(sector_id, NodeKind) ring buffer of mean activation
//!     across last HISTORY_LEN snapshot ticks
//!   - velocity = (latest − oldest) / span
//!   - acceleration = midpoint second difference
//!   - TopForming = level positive AND velocity dropping past zero
//!   - BottomForming = level positive (magnitude) but flipping back up
//!
//! No history-based learning. Pure kinematics on the aggregated signal.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::pipeline::sector_sub_kg::{
    sector_in_contrast_whitelist, SectorSubKgRegistry, SECTOR_AGG_KINDS,
};
use crate::pipeline::symbol_sub_kg::NodeKind;

/// Ring buffer depth — same as per-symbol kinematics for analog
/// behavior. 5 snapshot ticks ≈ 25 raw ticks ≈ 200s at 8s/tick.
pub const HISTORY_LEN: usize = 5;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum SectorTurningPointKind {
    /// Mean activation is positive but velocity recently turned negative.
    TopForming,
    /// Mean activation is positive but velocity recently turned positive
    /// after being negative.
    BottomForming,
    /// Mean is rising and velocity is positive (acceleration phase).
    Accelerating,
    /// Mean is falling and velocity is negative (downward acceleration).
    Decaying,
}

#[derive(Debug, Clone, Serialize)]
pub struct SectorKinematicsEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub sector_id: String,
    pub node_kind: String,
    pub kind: SectorTurningPointKind,
    pub level_now: f64,
    pub velocity: f64,
    pub acceleration: f64,
}

#[derive(Debug, Default)]
pub struct SectorKinematicsTracker {
    /// (sector_id, NodeKind) → rolling sector_mean history
    history: HashMap<(String, NodeKind), VecDeque<f64>>,
    /// (sector_id, NodeKind) → last tick this entry was observed at.
    /// Used by `apply_to_perception_graph` so unobserved-this-tick
    /// entries don't fraudulently bump their `last_tick` and confuse
    /// Y / L4 staleness checks.
    last_observed: HashMap<(String, NodeKind), u64>,
}

impl SectorKinematicsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    fn observe(&mut self, sector_id: &str, kind: NodeKind, value: f64, tick: u64) {
        let key = (sector_id.to_string(), kind);
        let entry = self.history.entry(key.clone()).or_default();
        entry.push_back(value);
        while entry.len() > HISTORY_LEN {
            entry.pop_front();
        }
        self.last_observed.insert(key, tick);
    }

    fn velocity(history: &VecDeque<f64>) -> Option<f64> {
        if history.len() < 2 {
            return None;
        }
        let latest = *history.back()?;
        let oldest = *history.front()?;
        let span = (history.len() - 1) as f64;
        Some((latest - oldest) / span)
    }

    fn acceleration(history: &VecDeque<f64>) -> Option<f64> {
        if history.len() < 3 {
            return None;
        }
        let latest = *history.back()?;
        let mid = history[history.len() / 2];
        let oldest = *history.front()?;
        let half_span = (history.len() - 1) as f64 / 2.0;
        if half_span < 1.0 {
            return None;
        }
        let v_recent = (latest - mid) / half_span;
        let v_prev = (mid - oldest) / half_span;
        Some(v_recent - v_prev)
    }

    /// Publish current per-(sector, kind) kinematic state into the
    /// perception graph. One snapshot per tracked (sector, kind), even
    /// when this tick fired no turning-point event — Y / L4 readers
    /// need the continuous state, not just the events. The
    /// classification field is set only when the matching `events`
    /// entry exists for this (sector, kind), so a "TopForming"
    /// classification persists in the graph until the next event for
    /// that sector slot overwrites it.
    pub fn apply_to_perception_graph(
        &self,
        events: &[SectorKinematicsEvent],
        graph: &mut crate::perception::PerceptionGraph,
        tick: u64,
    ) {
        let _ = tick; // current-tick parameter retained for symmetry; the
                      // per-entry stamp comes from `last_observed`.
        for ((sector_id, kind), history) in self.history.iter() {
            if history.is_empty() {
                continue;
            }
            let level_now = *history.back().unwrap_or(&0.0);
            let velocity = Self::velocity(history).unwrap_or(0.0);
            let acceleration = Self::acceleration(history).unwrap_or(0.0);
            let kind_str = format!("{:?}", kind);
            // Sticky classification: this tick's matching event wins;
            // otherwise inherit the last reading the graph already
            // holds. Y / L4 expect the latest classification, not
            // "what happened this exact tick".
            let classification = events
                .iter()
                .find(|e| e.sector_id == *sector_id && e.node_kind == kind_str)
                .map(|e| format!("{:?}", e.kind))
                .or_else(|| {
                    graph
                        .sector_kinematics
                        .get(sector_id, &kind_str)
                        .map(|s| s.classification)
                });
            // Per-entry last_tick: when this (sector, kind) was last
            // observed by `observe()`, NOT the apply-call tick. A
            // sector that dropped out of the universe keeps its prior
            // last_tick so Y staleness checks aren't lied to.
            let last_tick = self
                .last_observed
                .get(&(sector_id.clone(), *kind))
                .copied()
                .unwrap_or(0);
            graph.sector_kinematics.upsert(
                sector_id.clone(),
                kind_str,
                crate::perception::SectorKinematicsSnapshot {
                    level_now,
                    velocity,
                    acceleration,
                    classification: classification.unwrap_or_else(|| "unclassified".to_string()),
                    last_tick,
                },
            );
        }
    }
}

/// Update tracker with current snapshot's sector means and emit any
/// turning-point events. Pure — no internal state mutation between
/// observation and detection.
pub fn update_and_detect(
    market: &str,
    sectors: &SectorSubKgRegistry,
    tracker: &mut SectorKinematicsTracker,
    ts: DateTime<Utc>,
    tick: u64,
) -> Vec<SectorKinematicsEvent> {
    let mut events = Vec::new();
    for (sid, sector) in &sectors.sectors {
        if !sector_in_contrast_whitelist(sid) {
            continue;
        }
        if sector.n_with_subkg == 0 {
            continue;
        }
        for kind in SECTOR_AGG_KINDS {
            let Some(agg) = sector.agg.get(&kind) else {
                continue;
            };
            tracker.observe(sid, kind, agg.mean, tick);

            let key = (sid.clone(), kind);
            let history = match tracker.history.get(&key) {
                Some(h) if h.len() >= 3 => h,
                _ => continue,
            };

            let level_now = *history.back().unwrap();
            let velocity = SectorKinematicsTracker::velocity(history).unwrap_or(0.0);
            let acceleration = SectorKinematicsTracker::acceleration(history).unwrap_or(0.0);

            // Classify:
            //   - velocity flipped sign recently (zero-crossing) → TopForming or BottomForming
            //   - velocity positive + acceleration positive → Accelerating
            //   - velocity negative + acceleration negative → Decaying
            // We need previous-tick velocity to detect sign flip — derive
            // from history (latest-1 vs latest-2 = recent_v; latest-2 vs latest-3 = prev_v).
            let n = history.len();
            let recent_v = history[n - 1] - history[n - 2];
            let prev_v = if n >= 3 {
                history[n - 2] - history[n - 3]
            } else {
                recent_v
            };

            let kind_evt = if recent_v < 0.0 && prev_v >= 0.0 && level_now > 0.0 {
                Some(SectorTurningPointKind::TopForming)
            } else if recent_v > 0.0 && prev_v <= 0.0 {
                Some(SectorTurningPointKind::BottomForming)
            } else if velocity > 0.0 && acceleration > 0.0 {
                Some(SectorTurningPointKind::Accelerating)
            } else if velocity < 0.0 && acceleration < 0.0 {
                Some(SectorTurningPointKind::Decaying)
            } else {
                None
            };

            if let Some(kind_evt) = kind_evt {
                events.push(SectorKinematicsEvent {
                    ts,
                    market: market.to_string(),
                    sector_id: sid.clone(),
                    node_kind: format!("{:?}", kind),
                    kind: kind_evt,
                    level_now,
                    velocity,
                    acceleration,
                });
            }
        }
    }
    events
}

pub fn write_events(market: &str, events: &[SectorKinematicsEvent]) -> std::io::Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-sector-kinematics-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for ev in events {
        let line = serde_json::to_string(ev)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::sector_sub_kg::{KindAggregate, SectorSubKG, SectorSubKgRegistry};

    fn mk_registry_at_mean(sid: &str, kind: NodeKind, mean: f64) -> SectorSubKgRegistry {
        let mut reg = SectorSubKgRegistry::default();
        let mut agg = HashMap::new();
        agg.insert(
            kind,
            KindAggregate {
                mean,
                variance: 0.0,
                n_lit: 5,
                n_total_members: 10,
                outlier_count: 0,
                max_member_activation: mean,
                top_member: Some("X".to_string()),
            },
        );
        reg.sectors.insert(
            sid.to_string(),
            SectorSubKG {
                sector_id: sid.to_string(),
                sector_name: Some(sid.to_string()),
                ts: Utc::now(),
                n_total_members: 10,
                n_with_subkg: 8,
                coverage_ratio: 0.8,
                agg,
                supplementary: HashMap::new(),
            },
        );
        reg
    }

    #[test]
    fn rising_then_flat_emits_top_forming() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        // Sequence: 0.1, 0.3, 0.5, 0.6, 0.55 — at the moment 0.55 lands,
        // prev_v = +0.10 (rising) and recent_v = -0.05 (just turned).
        // That tick should emit TopForming. Collect events from every
        // observation so we can find it.
        let mut all = Vec::new();
        for (i, v) in [0.1, 0.3, 0.5, 0.6, 0.55].iter().enumerate() {
            let reg = mk_registry_at_mean("tech", kind, *v);
            all.extend(update_and_detect(
                "test",
                &reg,
                &mut tracker,
                Utc::now(),
                i as u64,
            ));
        }
        let top = all.iter().find(|e| {
            e.sector_id == "tech" && matches!(e.kind, SectorTurningPointKind::TopForming)
        });
        assert!(
            top.is_some(),
            "should have TopForming after peak, got {:?}",
            all.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn falling_then_rising_emits_bottom_forming() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Intent;
        let mut all = Vec::new();
        for (i, v) in [0.6, 0.4, 0.2, 0.1, 0.15].iter().enumerate() {
            let reg = mk_registry_at_mean("finance", kind, *v);
            all.extend(update_and_detect(
                "test",
                &reg,
                &mut tracker,
                Utc::now(),
                i as u64,
            ));
        }
        let bot = all.iter().find(|e| {
            e.sector_id == "finance" && matches!(e.kind, SectorTurningPointKind::BottomForming)
        });
        assert!(
            bot.is_some(),
            "should have BottomForming after trough, got {:?}",
            all.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sustained_rising_no_top_forming() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        let mut all_evs = Vec::new();
        for (i, v) in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7].iter().enumerate() {
            let reg = mk_registry_at_mean("energy", kind, *v);
            all_evs.extend(update_and_detect(
                "test",
                &reg,
                &mut tracker,
                Utc::now(),
                i as u64,
            ));
        }
        for ev in &all_evs {
            assert!(
                !matches!(ev.kind, SectorTurningPointKind::TopForming),
                "sustained rising should never emit TopForming, got {:?}",
                ev
            );
        }
    }

    #[test]
    fn apply_to_perception_graph_writes_snapshot_for_each_observed_kind() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        // Three observations on tick 5; apply on tick 9. snap.last_tick
        // should reflect the OBSERVED tick (5), not the apply-call tick.
        for v in [0.1, 0.2, 0.3] {
            let reg = mk_registry_at_mean("tech", kind, v);
            let _ = update_and_detect("test", &reg, &mut tracker, Utc::now(), 5);
        }
        let mut graph = crate::perception::PerceptionGraph::new();
        tracker.apply_to_perception_graph(&[], &mut graph, 9);

        let snap = graph
            .sector_kinematics
            .get("tech", "Pressure")
            .expect("graph should hold snapshot for tracked sector");
        assert_eq!(
            snap.last_tick, 5,
            "last_tick must come from the last observe(), not the apply call"
        );
        // 3 samples, [0.1, 0.2, 0.3] → level_now=0.3, velocity=(0.3-0.1)/2=0.1
        assert!(
            (snap.level_now - 0.3).abs() < 1e-9,
            "got {}",
            snap.level_now
        );
        assert!((snap.velocity - 0.1).abs() < 1e-9, "got {}", snap.velocity);
        assert!(
            snap.classification.is_none(),
            "no events → no classification"
        );
    }

    #[test]
    fn apply_to_perception_graph_carries_classification_when_event_fires() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        // Mirror production: apply after each tick using only that
        // tick's events. Final tick (0.55 after rising 0.6) should
        // trigger TopForming, and the graph snapshot for that tick
        // should carry the classification.
        let mut graph = crate::perception::PerceptionGraph::new();
        let mut last_events = Vec::new();
        let mut last_tick = 0u64;
        for (i, v) in [0.1, 0.3, 0.5, 0.6, 0.55].iter().enumerate() {
            let reg = mk_registry_at_mean("tech", kind, *v);
            last_tick = i as u64;
            last_events = update_and_detect("test", &reg, &mut tracker, Utc::now(), last_tick);
            tracker.apply_to_perception_graph(&last_events, &mut graph, last_tick);
        }
        // After the final tick (0.55 after 0.6), TopForming should fire.
        let has_top_forming = last_events
            .iter()
            .any(|e| matches!(e.kind, SectorTurningPointKind::TopForming));
        assert!(
            has_top_forming,
            "expected TopForming on final tick, got {:?}",
            last_events.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
        let snap = graph.sector_kinematics.get("tech", "Pressure").unwrap();
        assert_eq!(snap.classification.as_str(), "TopForming");
        assert_eq!(snap.last_tick, last_tick);
    }

    #[test]
    fn apply_to_perception_graph_no_op_on_empty_tracker() {
        let tracker = SectorKinematicsTracker::new();
        let mut graph = crate::perception::PerceptionGraph::new();
        tracker.apply_to_perception_graph(&[], &mut graph, 1);
        assert!(graph.sector_kinematics.is_empty());
    }

    #[test]
    fn apply_to_perception_graph_keeps_prior_last_tick_when_sector_unobserved() {
        // Pin Y staleness contract: a (sector, kind) that was observed
        // on tick 5 but NOT on tick 10 must still report last_tick=5
        // in the graph after the tick-10 apply call. Otherwise Y can't
        // distinguish "fresh" from "stale".
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        let mut graph = crate::perception::PerceptionGraph::new();

        // Tick 5: tech only.
        let reg_tech = mk_registry_at_mean("tech", kind, 0.5);
        let evs = update_and_detect("test", &reg_tech, &mut tracker, Utc::now(), 5);
        tracker.apply_to_perception_graph(&evs, &mut graph, 5);
        assert_eq!(
            graph
                .sector_kinematics
                .get("tech", "Pressure")
                .unwrap()
                .last_tick,
            5
        );

        // Tick 10: finance only — tech is NOT in this registry.
        let reg_finance = mk_registry_at_mean("finance", kind, -0.2);
        let evs = update_and_detect("test", &reg_finance, &mut tracker, Utc::now(), 10);
        tracker.apply_to_perception_graph(&evs, &mut graph, 10);

        let tech_snap = graph.sector_kinematics.get("tech", "Pressure").unwrap();
        let finance_snap = graph.sector_kinematics.get("finance", "Pressure").unwrap();
        assert_eq!(
            tech_snap.last_tick, 5,
            "tech wasn't observed on tick 10, last_tick must remain 5"
        );
        assert_eq!(
            finance_snap.last_tick, 10,
            "finance was observed on tick 10"
        );
    }

    #[test]
    fn apply_to_perception_graph_classification_persists_across_event_free_ticks() {
        // Pin the doc-claimed sticky semantics: once a TopForming
        // classification is in the graph, a subsequent tick with no
        // events for that (sector, kind) must NOT clear the
        // classification. Y / L4 readers expect the latest
        // classification, not "what happened this exact tick".
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        let mut graph = crate::perception::PerceptionGraph::new();

        // First five ticks trigger TopForming on the last one.
        let mut last_events = Vec::new();
        for (i, v) in [0.1, 0.3, 0.5, 0.6, 0.55].iter().enumerate() {
            let reg = mk_registry_at_mean("tech", kind, *v);
            last_events = update_and_detect("test", &reg, &mut tracker, Utc::now(), i as u64);
            tracker.apply_to_perception_graph(&last_events, &mut graph, i as u64);
        }
        assert_eq!(
            graph
                .sector_kinematics
                .get("tech", "Pressure")
                .unwrap()
                .classification
                .as_str(),
                "TopForming",
            "TopForming should be present after the trigger tick"
        );

        // Now a tick with no event (continuing the descent gently —
        // velocity stays negative, no new zero-crossing).
        let reg = mk_registry_at_mean("tech", kind, 0.50);
        let next_events = update_and_detect("test", &reg, &mut tracker, Utc::now(), 6);
        tracker.apply_to_perception_graph(&next_events, &mut graph, 6);

        let snap = graph.sector_kinematics.get("tech", "Pressure").unwrap();
        assert_eq!(
            snap.classification.as_str(),
            "TopForming",
            "classification must persist across event-free ticks; \
             this tick's events were {:?}",
            next_events.iter().map(|e| &e.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn overlay_sector_skipped() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        for (i, v) in [0.1, 0.3, 0.5, 0.4, 0.3].iter().enumerate() {
            let reg = mk_registry_at_mean("china_adr", kind, *v);
            let evs = update_and_detect("test", &reg, &mut tracker, Utc::now(), i as u64);
            for ev in &evs {
                assert!(
                    ev.sector_id != "china_adr",
                    "overlay sector should not emit, got {}",
                    ev.sector_id
                );
            }
        }
    }
}
