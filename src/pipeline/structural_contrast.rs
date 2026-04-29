//! Spatial contrast on the graph — biological vision primitive applied
//! to sub-KG activations along master KG edges.
//!
//! Retinal center-surround receptive field:
//!   response(cell) = activation(cell) − mean(activation of neighbors)
//!
//! For Eden:
//!   contrast(symbol, kind) = activation(symbol, kind)
//!                            − mean(activation(neighbors, kind))
//!
//! where `neighbors` are this symbol's master-KG-connected sub-KGs
//! (peers / sector co-members).
//!
//! Properties:
//!   - Uniform market-wide rise → every cell activated → neighbors
//!     cancel center → contrast ≈ 0 → nothing fires (market beta invisible)
//!   - Local standout → center activated but neighbors quiet → contrast
//!     large → fires (standout alpha pops out)
//!   - Sector-wide event → cluster cells all lit, but contrast against
//!     OTHER clusters still surfaces "this cluster is the standout"
//!
//! No history. No baseline. No magic threshold. Pure current-tick
//! spatial derivative on graph topology.
//!
//! Output: `.run/eden-contrast-{market}.ndjson` — top-K |contrast|
//! events per tick per NodeKind. K is operator attention budget, not
//! a signal threshold.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::pipeline::sector_sub_kg::{sector_in_contrast_whitelist, SectorSubKgRegistry};
use crate::pipeline::symbol_sub_kg::{NodeKind, SubKgRegistry};

/// Noise floor percentile — any contrast at or below this percentile of
/// the current tick's distribution is considered market-wide noise
/// (HFT / background activity affecting many symbols simultaneously).
/// Only contrasts ABOVE this percentile survive as structural signals.
/// 0.99 = keep top 1% of contrasts per NodeKind. Pure statistical primitive,
/// NOT a learned threshold.
pub const NOISE_FLOOR_PERCENTILE: f64 = 0.99;

#[derive(Debug, Clone, Serialize)]
pub struct ContrastEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    pub node_kind: String,
    pub center_activation: f64,
    pub surround_mean: f64,
    pub surround_count: usize,
    /// center − surround_mean. Positive = symbol exceeds neighbors,
    /// negative = symbol below neighbors.
    pub contrast: f64,
    /// Sector containing this symbol (None if symbol unmapped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector_id: Option<String>,
    /// Mean of activation across the symbol's own sector members for
    /// this NodeKind. Lets the second contrast hop (vs sector mean)
    /// stand alongside the original (vs neighbor mean).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector_mean_activation: Option<f64>,
    /// center − sector_mean_activation. Positive = symbol exceeds its
    /// sector average, negative = below. Populated only when:
    ///   (a) sector is in `SECTOR_CONTRAST_WHITELIST` (excludes US
    ///       overlay sectors like china_adr / etf / crypto whose
    ///       composition is structurally heterogeneous)
    ///   (b) sector aggregate has n_with_subkg >= 2
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vs_sector_contrast: Option<f64>,
}

/// Master KG neighbor map: for each symbol, a list of its connected
/// master-KG neighbors. Caller provides (built from BrainGraph /
/// UsGraph StockToStock edges).
pub type NeighborMap = HashMap<String, Vec<String>>;

/// Compute center-surround contrast per (symbol, NodeKind).
/// Aggregates per-symbol per-kind activation = sum of |value| for nodes of that kind.
///
/// `sector_subkgs` + `symbol_to_sector` are optional second-hop input —
/// when both supplied, ContrastEvent additionally carries
/// `vs_sector_contrast` (center − own sector mean) for whitelisted sectors.
pub fn detect_contrasts(
    market: &str,
    registry: &SubKgRegistry,
    neighbors: &NeighborMap,
    sector_subkgs: Option<&SectorSubKgRegistry>,
    symbol_to_sector: &HashMap<String, String>,
    ts: DateTime<Utc>,
) -> Vec<ContrastEvent> {
    // Kinds we track (the 0..1-ish scale ones, where contrast is meaningful).
    let tracked = [
        NodeKind::Pressure,
        NodeKind::Intent,
        NodeKind::Microstructure,
        NodeKind::Event,
        NodeKind::CapitalFlow,
        NodeKind::Role,
        NodeKind::BookQuality,
    ];

    // Pre-compute per-(symbol, kind) activation sum.
    let mut activation: HashMap<(&String, NodeKind), f64> = HashMap::new();
    for (sym, kg) in &registry.graphs {
        for kind in tracked {
            let sum: f64 = kg
                .nodes
                .iter()
                .filter(|(_, a)| a.kind == kind)
                .map(|(_, a)| {
                    a.value
                        .map(|v| v.abs().to_f64().unwrap_or(0.0))
                        .unwrap_or(0.0)
                })
                .sum();
            activation.insert((sym, kind), sum);
        }
    }

    // Per NodeKind: compute contrasts for all symbols, then apply
    // noise-floor subtraction via percentile of |contrast| distribution.
    // Only contrasts ABOVE the current-tick noise floor survive.
    // No ranking. No top-K. No operator attention budget. Either the
    // signal exceeds ambient structural noise or it doesn't.
    let mut all_events: Vec<ContrastEvent> = Vec::new();
    for kind in tracked {
        let mut per_kind: Vec<ContrastEvent> = Vec::new();
        for (sym, _kg) in &registry.graphs {
            let center = *activation.get(&(sym, kind)).unwrap_or(&0.0);
            let neigh_list = match neighbors.get(sym) {
                Some(l) => l,
                None => continue,
            };
            let mut s = 0.0;
            let mut c = 0;
            for n in neigh_list {
                if let Some(v) = activation.get(&(&(n.clone()), kind)) {
                    s += v;
                    c += 1;
                }
            }
            if c == 0 {
                continue;
            }
            let surround_mean = s / c as f64;
            let contrast = center - surround_mean;
            // Sector hop — populate only when sector_subkgs supplied AND
            // sector is whitelisted AND aggregate has ≥2 members with sub-KG.
            let sector_id = symbol_to_sector.get(sym.as_str()).cloned();
            let (sector_mean_activation, vs_sector_contrast) =
                match (sector_subkgs, sector_id.as_deref()) {
                    (Some(reg), Some(sid)) if sector_in_contrast_whitelist(sid) => {
                        if let Some(sec) = reg.get(sid) {
                            if sec.n_with_subkg >= 2 {
                                if let Some(agg) = sec.agg.get(&kind) {
                                    (Some(agg.mean), Some(center - agg.mean))
                                } else {
                                    (None, None)
                                }
                            } else {
                                (None, None)
                            }
                        } else {
                            (None, None)
                        }
                    }
                    _ => (None, None),
                };
            per_kind.push(ContrastEvent {
                ts,
                market: market.to_string(),
                symbol: sym.clone(),
                node_kind: format!("{:?}", kind),
                center_activation: center,
                surround_mean,
                surround_count: c,
                contrast,
                sector_id,
                sector_mean_activation,
                vs_sector_contrast,
            });
        }
        // Noise floor: compute percentile of |contrast| distribution for
        // THIS kind THIS tick. Any |contrast| ≤ floor is ambient noise.
        if per_kind.len() < 10 {
            // Too few to define a noise floor; skip (no signal survives).
            continue;
        }
        let mut mags: Vec<f64> = per_kind.iter().map(|e| e.contrast.abs()).collect();
        mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let floor_idx = (NOISE_FLOOR_PERCENTILE * mags.len() as f64) as usize;
        let floor = mags
            .get(floor_idx.min(mags.len() - 1))
            .copied()
            .unwrap_or(0.0);
        // Keep events whose |contrast| strictly exceeds the floor.
        for ev in per_kind.into_iter().filter(|e| e.contrast.abs() > floor) {
            all_events.push(ev);
        }
    }
    all_events
}

pub fn write_events(market: &str, events: &[ContrastEvent]) -> std::io::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let path = format!(".run/eden-contrast-{}.ndjson", market);
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
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    fn make_universe(
        reg: &mut SubKgRegistry,
        nb: &mut HashMap<String, Vec<String>>,
        n: usize,
        activations: &[(usize, rust_decimal::Decimal)],
    ) {
        let syms: Vec<String> = (0..n).map(|i| format!("S{}.HK", i)).collect();
        for sym in &syms {
            reg.upsert(sym, Utc::now());
        }
        for (idx, v) in activations {
            reg.upsert(&syms[*idx], Utc::now()).set_node_value(
                NodeId::PressureOrderBook,
                *v,
                Utc::now(),
            );
        }
        // Each symbol's neighbors = all other symbols
        for i in 0..n {
            let neigh: Vec<String> = (0..n)
                .filter(|j| *j != i)
                .map(|j| syms[j].clone())
                .collect();
            nb.insert(syms[i].clone(), neigh);
        }
    }

    #[test]
    fn uniform_activation_produces_no_events_after_noise_floor() {
        let mut reg = SubKgRegistry::new();
        let mut nb = HashMap::new();
        let acts: Vec<(usize, rust_decimal::Decimal)> = (0..200).map(|i| (i, dec!(0.5))).collect();
        make_universe(&mut reg, &mut nb, 200, &acts);
        let evs = detect_contrasts("hk", &reg, &nb, None, &HashMap::new(), Utc::now());
        assert!(
            evs.is_empty(),
            "uniform field should be entirely noise floor, got {}",
            evs.len()
        );
    }

    #[test]
    fn local_standout_exceeds_noise_floor() {
        let mut reg = SubKgRegistry::new();
        let mut nb = HashMap::new();
        let mut acts: Vec<(usize, rust_decimal::Decimal)> =
            (0..200).map(|i| (i, dec!(0.1))).collect();
        acts[0] = (0, dec!(5.0)); // extreme standout
        make_universe(&mut reg, &mut nb, 200, &acts);
        let evs = detect_contrasts("hk", &reg, &nb, None, &HashMap::new(), Utc::now());
        let a_ev = evs
            .iter()
            .find(|e| e.symbol == "S0.HK")
            .expect("S0 should survive noise floor");
        assert!(a_ev.contrast > 1.0);
    }

    #[test]
    fn vs_sector_contrast_populated_when_registry_passed() {
        use crate::ontology::objects::{SectorId, Symbol};
        use crate::pipeline::sector_sub_kg::build_from_registry;

        let mut reg = SubKgRegistry::new();
        let mut nb = HashMap::new();
        // Build a 200-symbol universe so noise floor doesn't kill the standout
        let mut acts: Vec<(usize, rust_decimal::Decimal)> =
            (0..200).map(|i| (i, dec!(0.10))).collect();
        acts[0] = (0, dec!(5.0));
        make_universe(&mut reg, &mut nb, 200, &acts);

        // Sector "tech" contains S0..S9 (S0 is the standout).
        let mut sm: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        let mut sn: HashMap<SectorId, String> = HashMap::new();
        let sid = SectorId("tech".into());
        sm.insert(
            sid.clone(),
            (0..10).map(|i| Symbol(format!("S{}.HK", i))).collect(),
        );
        sn.insert(sid, "tech".to_string());
        let sector_reg = build_from_registry(&reg, &sm, &sn, Utc::now());

        let mut s2s: HashMap<String, String> = HashMap::new();
        for i in 0..10 {
            s2s.insert(format!("S{}.HK", i), "tech".to_string());
        }

        let evs = detect_contrasts("hk", &reg, &nb, Some(&sector_reg), &s2s, Utc::now());
        let s0 = evs
            .iter()
            .find(|e| e.symbol == "S0.HK")
            .expect("S0 should survive noise floor");
        assert_eq!(s0.sector_id.as_deref(), Some("tech"));
        assert!(
            s0.sector_mean_activation.is_some(),
            "sector mean must be populated"
        );
        assert!(
            s0.vs_sector_contrast.is_some(),
            "vs_sector_contrast must be populated"
        );
        assert!(
            s0.vs_sector_contrast.unwrap() > 0.0,
            "S0 standout should be above own sector mean"
        );
    }

    #[test]
    fn noise_floor_survives_are_bounded() {
        let mut reg = SubKgRegistry::new();
        let mut nb = HashMap::new();
        let acts: Vec<(usize, rust_decimal::Decimal)> = (0..200)
            .map(|i| (i, rust_decimal::Decimal::new(10 + (i as i64) * 1, 2)))
            .collect();
        make_universe(&mut reg, &mut nb, 200, &acts);
        let evs = detect_contrasts("hk", &reg, &nb, None, &HashMap::new(), Utc::now());
        // With 200 symbols and 99th percentile floor, at most ~4 events survive
        assert!(evs.len() <= 5, "got {} events", evs.len());
    }
}
