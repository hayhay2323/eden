//! Cross-sector contrast — second hop of structural visual model.
//!
//! `structural_contrast.rs` answers: "is this symbol a standout vs its
//! peers / vs its sector?"
//! `cross_sector_contrast` answers: "is this SECTOR a standout vs all
//! other sectors?"
//!
//! Same biological vision primitive (center − surround), one zoom level
//! up. Reuses the SectorSubKgRegistry that `sector_sub_kg::build_from_registry`
//! already produces — pure read of existing per-tick state.
//!
//! Method:
//!   for each NodeKind in SECTOR_AGG_KINDS:
//!     for each whitelisted sector that has an aggregate:
//!       center = sector.agg[kind].mean
//!     surround_mean = mean across all OTHER sectors' centers
//!     contrast = center − surround_mean
//!   Apply the same per-kind 99th percentile noise floor as
//!   structural_contrast — only sectors whose |contrast| exceeds the
//!   tick-local noise floor survive.
//!
//! Output: `.run/eden-sector-contrast-{market}.ndjson` — answers
//! "which sector is THE standout this snapshot tick".

use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::pipeline::sector_sub_kg::{
    sector_in_contrast_whitelist, SectorSubKgRegistry, SECTOR_AGG_KINDS,
};

/// Minimum number of sectors required to compute meaningful contrast.
/// Below this, even a clear standout doesn't have enough peer-sectors
/// to be distinguished from sample noise.
pub const MIN_SECTORS: usize = 5;

/// At sector level we only have at most ~17 (HK) or ~25 (US) data
/// points per NodeKind, so the percentile-based noise floor used at
/// symbol level (`structural_contrast::NOISE_FLOOR_PERCENTILE = 0.99`)
/// degenerates: the 99th percentile of 17 values IS the max, so the
/// standout can never survive `> floor`. Instead we emit ALL qualifying
/// (sector, kind) pairs and let the operator (or downstream code) sort
/// by `|contrast|`. ndjson volume is bounded — at most 17 × 7 = 119 rows
/// per snapshot.

#[derive(Debug, Clone, Serialize)]
pub struct SectorContrastEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub sector_id: String,
    pub node_kind: String,
    pub center_activation: f64,
    pub surround_mean: f64,
    pub surround_count: usize,
    /// center − surround_mean. Positive = sector exceeds peer-sectors,
    /// negative = sector below.
    pub contrast: f64,
}

/// Compute per-(sector, NodeKind) cross-sector contrast.
/// Only whitelisted sectors are included (excludes US overlay sectors
/// like china_adr / etf / crypto whose composition is heterogeneous).
pub fn detect_sector_contrasts(
    market: &str,
    sectors: &SectorSubKgRegistry,
    ts: DateTime<Utc>,
) -> Vec<SectorContrastEvent> {
    let mut all_events = Vec::new();

    for kind in SECTOR_AGG_KINDS {
        // Collect (sector_id, mean) across whitelisted sectors with
        // aggregates that have ≥1 lit member (no point comparing dark
        // sectors).
        let centers: Vec<(&String, f64)> = sectors
            .sectors
            .iter()
            .filter(|(sid, _)| sector_in_contrast_whitelist(sid))
            .filter_map(|(sid, sec)| {
                if sec.n_with_subkg == 0 {
                    return None;
                }
                let agg = sec.agg.get(&kind)?;
                if agg.n_lit == 0 {
                    return None;
                }
                Some((sid, agg.mean))
            })
            .collect();

        if centers.len() < MIN_SECTORS {
            continue;
        }

        let total: f64 = centers.iter().map(|(_, v)| *v).sum();
        let n = centers.len() as f64;

        for (sid, center) in &centers {
            // Surround mean excludes self (peer-sector mean).
            let surround_mean = (total - *center) / (n - 1.0);
            let contrast = *center - surround_mean;
            all_events.push(SectorContrastEvent {
                ts,
                market: market.to_string(),
                sector_id: (*sid).clone(),
                node_kind: format!("{:?}", kind),
                center_activation: *center,
                surround_mean,
                surround_count: centers.len() - 1,
                contrast,
            });
        }
    }

    all_events
}

/// Mutate the perception graph's sector-contrast sub-graph from this
/// tick's events. Unlike sector_kinematics, contrast is fully
/// recomputed each tick (stateless detector), so a sector that
/// dropped out of the universe leaves its old reading lingering —
/// Y / L4 readers must compare `last_tick` against current tick to
/// judge freshness.
pub fn apply_to_perception_graph(
    events: &[SectorContrastEvent],
    graph: &mut crate::perception::PerceptionGraph,
    tick: u64,
) {
    for ev in events {
        graph.sector_contrast.upsert(
            ev.sector_id.clone(),
            ev.node_kind.clone(),
            crate::perception::SectorContrastSnapshot {
                center_activation: ev.center_activation,
                surround_mean: ev.surround_mean,
                contrast: ev.contrast,
                surround_count: ev.surround_count,
                last_tick: tick,
            },
        );
    }
}

pub fn write_events(market: &str, events: &[SectorContrastEvent]) -> std::io::Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-sector-contrast-{}.ndjson", market);
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
    use crate::pipeline::symbol_sub_kg::NodeKind;
    use std::collections::HashMap;

    fn mk_sector(id: &str, kind: NodeKind, mean: f64, n_lit: usize) -> SectorSubKG {
        let mut agg = HashMap::new();
        agg.insert(
            kind,
            KindAggregate {
                mean,
                variance: 0.0,
                n_lit,
                n_total_members: 10,
                outlier_count: 0,
                max_member_activation: mean,
                top_member: Some("X.US".to_string()),
            },
        );
        SectorSubKG {
            sector_id: id.to_string(),
            sector_name: Some(id.to_string()),
            ts: Utc::now(),
            n_total_members: 10,
            n_with_subkg: n_lit.max(1),
            coverage_ratio: 1.0,
            agg,
            supplementary: HashMap::new(),
        }
    }

    fn build_registry(rows: &[(&str, NodeKind, f64)]) -> SectorSubKgRegistry {
        let mut reg = SectorSubKgRegistry::default();
        for (id, kind, mean) in rows {
            reg.sectors
                .insert((*id).to_string(), mk_sector(id, *kind, *mean, 5));
        }
        reg
    }

    #[test]
    fn standout_sector_survives_floor() {
        let kind = NodeKind::Pressure;
        // 16 sectors with mean 0.10 each + tech standing out at 1.0.
        let mut rows: Vec<(&str, NodeKind, f64)> = vec![
            ("finance", kind, 0.1),
            ("energy", kind, 0.1),
            ("telecom", kind, 0.1),
            ("property", kind, 0.1),
            ("consumer", kind, 0.1),
            ("healthcare", kind, 0.1),
            ("utilities", kind, 0.1),
            ("insurance", kind, 0.1),
            ("auto", kind, 0.1),
            ("materials", kind, 0.1),
            ("industrial", kind, 0.1),
            ("conglomerate", kind, 0.1),
            ("media", kind, 0.1),
            ("logistics", kind, 0.1),
            ("education", kind, 0.1),
            ("semiconductor", kind, 0.1),
            ("tech", kind, 1.0),
        ];
        rows.sort_by(|a, b| a.0.cmp(b.0));
        let reg = build_registry(&rows);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());
        let tech_ev = evs
            .iter()
            .find(|e| e.sector_id == "tech")
            .expect("tech should survive");
        assert!(tech_ev.contrast > 0.5, "tech contrast should be large");
    }

    #[test]
    fn uniform_sectors_emit_zero_contrast() {
        // Without noise floor, uniform sectors still emit events but
        // each contrast = 0 (every sector at the average). Operator
        // sorts by |contrast| and sees nothing actionable.
        let kind = NodeKind::Pressure;
        let rows: Vec<(&str, NodeKind, f64)> = (0..17)
            .map(|i| match i {
                0 => ("tech", kind, 0.5),
                1 => ("finance", kind, 0.5),
                2 => ("energy", kind, 0.5),
                3 => ("telecom", kind, 0.5),
                4 => ("property", kind, 0.5),
                5 => ("consumer", kind, 0.5),
                6 => ("healthcare", kind, 0.5),
                7 => ("utilities", kind, 0.5),
                8 => ("insurance", kind, 0.5),
                9 => ("auto", kind, 0.5),
                10 => ("materials", kind, 0.5),
                11 => ("industrial", kind, 0.5),
                12 => ("conglomerate", kind, 0.5),
                13 => ("media", kind, 0.5),
                14 => ("logistics", kind, 0.5),
                15 => ("education", kind, 0.5),
                16 => ("semiconductor", kind, 0.5),
                _ => unreachable!(),
            })
            .collect();
        let reg = build_registry(&rows);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());
        assert_eq!(evs.len(), 17, "all 17 whitelisted sectors emit");
        for ev in &evs {
            assert!(ev.contrast.abs() < 1e-9, "uniform: contrast must be 0");
        }
    }

    #[test]
    fn overlay_sectors_excluded_from_centers() {
        let kind = NodeKind::Pressure;
        let rows: Vec<(&str, NodeKind, f64)> = vec![
            ("etf", kind, 99.0),       // overlay — should be excluded
            ("crypto", kind, 88.0),    // overlay — excluded
            ("china_adr", kind, 77.0), // overlay — excluded
            ("tech", kind, 1.0),
            ("finance", kind, 0.1),
            ("energy", kind, 0.1),
            ("telecom", kind, 0.1),
            ("property", kind, 0.1),
        ];
        let reg = build_registry(&rows);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());
        // None of the overlay sectors should appear.
        for ev in &evs {
            assert!(
                !["etf", "crypto", "china_adr"].contains(&ev.sector_id.as_str()),
                "overlay sector {} leaked into output",
                ev.sector_id
            );
        }
    }

    #[test]
    fn apply_to_perception_graph_writes_one_snapshot_per_event() {
        let kind = NodeKind::Pressure;
        let mut rows: Vec<(&str, NodeKind, f64)> = vec![
            ("finance", kind, 0.1),
            ("energy", kind, 0.1),
            ("telecom", kind, 0.1),
            ("property", kind, 0.1),
            ("consumer", kind, 0.1),
            ("healthcare", kind, 0.1),
            ("utilities", kind, 0.1),
            ("insurance", kind, 0.1),
            ("auto", kind, 0.1),
            ("materials", kind, 0.1),
            ("industrial", kind, 0.1),
            ("conglomerate", kind, 0.1),
            ("media", kind, 0.1),
            ("logistics", kind, 0.1),
            ("education", kind, 0.1),
            ("semiconductor", kind, 0.1),
            ("tech", kind, 1.0),
        ];
        rows.sort_by(|a, b| a.0.cmp(b.0));
        let reg = build_registry(&rows);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());

        let mut graph = crate::perception::PerceptionGraph::new();
        apply_to_perception_graph(&evs, &mut graph, 42);

        assert_eq!(graph.sector_contrast.len(), evs.len());
        let tech = graph
            .sector_contrast
            .get("tech", "Pressure")
            .expect("tech reading should be in graph");
        assert!(tech.contrast > 0.5, "tech contrast should be large");
        assert_eq!(tech.last_tick, 42);
    }

    #[test]
    fn apply_to_perception_graph_empty_events_is_noop() {
        let mut graph = crate::perception::PerceptionGraph::new();
        apply_to_perception_graph(&[], &mut graph, 99);
        assert!(graph.sector_contrast.is_empty());
    }

    #[test]
    fn apply_to_perception_graph_overwrites_prior_reading() {
        let mut graph = crate::perception::PerceptionGraph::new();
        let kind = NodeKind::Pressure;
        // First tick: tech standout at 1.0.
        let mut rows: Vec<(&str, NodeKind, f64)> = vec![
            ("finance", kind, 0.1),
            ("energy", kind, 0.1),
            ("telecom", kind, 0.1),
            ("property", kind, 0.1),
            ("consumer", kind, 0.1),
            ("healthcare", kind, 0.1),
            ("utilities", kind, 0.1),
            ("insurance", kind, 0.1),
            ("auto", kind, 0.1),
            ("materials", kind, 0.1),
            ("industrial", kind, 0.1),
            ("conglomerate", kind, 0.1),
            ("media", kind, 0.1),
            ("logistics", kind, 0.1),
            ("education", kind, 0.1),
            ("semiconductor", kind, 0.1),
            ("tech", kind, 1.0),
        ];
        rows.sort_by(|a, b| a.0.cmp(b.0));
        let reg = build_registry(&rows);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());
        apply_to_perception_graph(&evs, &mut graph, 1);
        let tick1_contrast = graph
            .sector_contrast
            .get("tech", "Pressure")
            .unwrap()
            .contrast;

        // Second tick: tech back to baseline 0.1.
        let mut rows2 = rows.clone();
        if let Some(idx) = rows2.iter().position(|r| r.0 == "tech") {
            rows2[idx].2 = 0.1;
        }
        let reg2 = build_registry(&rows2);
        let evs2 = detect_sector_contrasts("test", &reg2, Utc::now());
        apply_to_perception_graph(&evs2, &mut graph, 2);
        let tick2_snap = graph.sector_contrast.get("tech", "Pressure").unwrap();
        assert!(
            tick2_snap.contrast.abs() < 1e-9,
            "tech should now be at the mean, contrast≈0; got {}",
            tick2_snap.contrast
        );
        assert_eq!(tick2_snap.last_tick, 2);
        // Sanity: the two ticks differ.
        assert!((tick1_contrast - tick2_snap.contrast).abs() > 0.5);
    }

    #[test]
    fn too_few_sectors_no_event() {
        let kind = NodeKind::Pressure;
        // Only 3 sectors — below MIN_SECTORS_FOR_FLOOR (5).
        let reg = build_registry(&[
            ("tech", kind, 1.0),
            ("finance", kind, 0.5),
            ("energy", kind, 0.1),
        ]);
        let evs = detect_sector_contrasts("test", &reg, Utc::now());
        assert!(evs.is_empty());
    }
}
