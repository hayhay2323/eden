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
}

impl SectorKinematicsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    fn observe(&mut self, sector_id: &str, kind: NodeKind, value: f64) {
        let key = (sector_id.to_string(), kind);
        let entry = self.history.entry(key).or_default();
        entry.push_back(value);
        while entry.len() > HISTORY_LEN {
            entry.pop_front();
        }
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
}

/// Update tracker with current snapshot's sector means and emit any
/// turning-point events. Pure — no internal state mutation between
/// observation and detection.
pub fn update_and_detect(
    market: &str,
    sectors: &SectorSubKgRegistry,
    tracker: &mut SectorKinematicsTracker,
    ts: DateTime<Utc>,
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
            tracker.observe(sid, kind, agg.mean);

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
        for v in [0.1, 0.3, 0.5, 0.6, 0.55] {
            let reg = mk_registry_at_mean("tech", kind, v);
            all.extend(update_and_detect("test", &reg, &mut tracker, Utc::now()));
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
        for v in [0.6, 0.4, 0.2, 0.1, 0.15] {
            let reg = mk_registry_at_mean("finance", kind, v);
            all.extend(update_and_detect("test", &reg, &mut tracker, Utc::now()));
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
        for v in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7] {
            let reg = mk_registry_at_mean("energy", kind, v);
            all_evs.extend(update_and_detect("test", &reg, &mut tracker, Utc::now()));
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
    fn overlay_sector_skipped() {
        let mut tracker = SectorKinematicsTracker::new();
        let kind = NodeKind::Pressure;
        for v in [0.1, 0.3, 0.5, 0.4, 0.3] {
            let reg = mk_registry_at_mean("china_adr", kind, v);
            let evs = update_and_detect("test", &reg, &mut tracker, Utc::now());
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
