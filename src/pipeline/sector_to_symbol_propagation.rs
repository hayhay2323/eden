//! Sector → Symbol backward propagation — closes the bidirectional
//! loop in Eden's structural visual model.
//!
//! Forward (already wired):
//!   Symbol sub-KG → Sector sub-KG → cross_sector_contrast
//!
//! Backward (this module):
//!   Sector sub-KG → quiet members' lag exposure
//!
//! Premise: when a sector is "hot" (high coverage + high mean activation
//! across most members AND positive cross-sector contrast), members that
//! are still quiet on the same NodeKind are STRUCTURALLY LAGGING the
//! sector signal. They're either about to follow (early entry) or
//! genuinely orthogonal (worth noting either way).
//!
//! This module DOES NOT mutate sub-KG nodes. Sub-KG is the current-tick
//! observation; mutating it would pollute the structural record. Instead
//! we emit `MemberLagEvent` rows: the operator (or a downstream learning
//! layer) decides what to do with "AAPL is dark while tech sector is
//! 0.42 lit on Pressure".
//!
//! Method (pure composition, no thresholds I invented):
//!   for each whitelisted sector with n_with_subkg >= 2:
//!     for each kind in SECTOR_AGG_KINDS:
//!       if sector.agg[kind].n_lit / sector.n_with_subkg < SYNC_THRESHOLD:
//!         skip (not enough internal consensus to call sector "hot")
//!       sector_mean = sector.agg[kind].mean
//!       for each member with sub-KG in this sector:
//!         member_activation = Σ|v| over member's nodes_of_kind(kind)
//!         lag = sector_mean - member_activation
//!         if lag > 0:
//!           emit MemberLagEvent
//!     (cap top-5 lagging members per (sector, kind) to bound output)
//!
//! Output: `.run/eden-sector-to-symbol-{market}.ndjson`.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::ontology::objects::{SectorId, Symbol};
use crate::pipeline::sector_sub_kg::{
    sector_in_contrast_whitelist, SectorSubKgRegistry, SECTOR_AGG_KINDS,
};
use crate::pipeline::symbol_sub_kg::{NodeKind, SubKgRegistry};

/// Minimum lit-fraction (`n_lit / n_with_subkg`) for a sector to be
/// considered "hot" enough to backward-propagate. 0.50 = at least half
/// of members showing this kind. Below this, the sector mean is being
/// dragged by a few outliers and emitting lag events is misleading.
pub const SYNC_THRESHOLD: f64 = 0.50;

/// At most this many lagging members emitted per (sector, kind). Keeps
/// total ndjson volume bounded: 17 base sectors × 7 kinds × 5 = 595 rows
/// max per snapshot tick.
pub const TOP_LAGGING_PER_KIND: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct MemberLagEvent {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub sector_id: String,
    pub node_kind: String,
    pub symbol: String,
    pub member_activation: f64,
    pub sector_mean: f64,
    /// sector_mean − member_activation. Positive = member is below
    /// sector signal. We only emit positive lag.
    pub lag: f64,
    /// `n_lit / n_with_subkg` for the sector this tick — informs how
    /// confident the "sector is hot" judgment is.
    pub sector_lit_fraction: f64,
}

/// Same Σ|v| activation formula as `structural_contrast::detect_contrasts`
/// and `sector_sub_kg::member_activation_for_kind` — kept consistent so
/// member_activation here is comparable to sector_mean derived upstream.
fn member_activation(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG, kind: NodeKind) -> f64 {
    kg.nodes
        .iter()
        .filter(|(_, a)| a.kind == kind)
        .map(|(_, a)| {
            a.value
                .map(|v| v.abs().to_f64().unwrap_or(0.0))
                .unwrap_or(0.0)
        })
        .sum()
}

/// Compute member lag events: members within hot sectors that are still
/// dark on the dominant NodeKind.
pub fn detect_member_lag(
    market: &str,
    registry: &SubKgRegistry,
    sectors: &SectorSubKgRegistry,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    ts: DateTime<Utc>,
) -> Vec<MemberLagEvent> {
    let mut events = Vec::new();

    for (sector_id_obj, members) in sector_members {
        let sid = &sector_id_obj.0;
        if !sector_in_contrast_whitelist(sid) {
            continue;
        }
        let Some(sector) = sectors.sectors.get(sid) else {
            continue;
        };
        if sector.n_with_subkg < 2 {
            continue;
        }
        for kind in SECTOR_AGG_KINDS {
            let Some(agg) = sector.agg.get(&kind) else {
                continue;
            };
            let lit_fraction = if sector.n_with_subkg > 0 {
                agg.n_lit as f64 / sector.n_with_subkg as f64
            } else {
                0.0
            };
            if lit_fraction < SYNC_THRESHOLD {
                continue;
            }
            // Compute lag for each member that has a sub-KG.
            let mut per_kind: Vec<MemberLagEvent> = members
                .iter()
                .filter_map(|sym| {
                    let kg = registry.get(&sym.0)?;
                    let act = member_activation(kg, kind);
                    let lag = agg.mean - act;
                    if lag <= 0.0 {
                        return None;
                    }
                    Some(MemberLagEvent {
                        ts,
                        market: market.to_string(),
                        sector_id: sid.clone(),
                        node_kind: format!("{:?}", kind),
                        symbol: sym.0.clone(),
                        member_activation: act,
                        sector_mean: agg.mean,
                        lag,
                        sector_lit_fraction: lit_fraction,
                    })
                })
                .collect();
            // Keep top-K lagging members.
            per_kind.sort_by(|a, b| {
                b.lag
                    .partial_cmp(&a.lag)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            per_kind.truncate(TOP_LAGGING_PER_KIND);
            events.extend(per_kind);
        }
    }
    events
}

pub fn write_events(market: &str, events: &[MemberLagEvent]) -> std::io::Result<usize> {
    if events.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-sector-to-symbol-{}.ndjson", market);
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
    use crate::pipeline::sector_sub_kg::build_from_registry;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    fn lit(reg: &mut SubKgRegistry, sym: &str, node: NodeId, v: rust_decimal::Decimal) {
        reg.upsert(sym, Utc::now())
            .set_node_value(node, v, Utc::now());
    }

    #[test]
    fn hot_sector_quiet_member_emits_lag() {
        let mut reg = SubKgRegistry::new();
        // Tech sector: 4 members; A/B/C lit at 0.7, D dark.
        lit(&mut reg, "A.US", NodeId::PressureOrderBook, dec!(0.7));
        lit(&mut reg, "B.US", NodeId::PressureOrderBook, dec!(0.7));
        lit(&mut reg, "C.US", NodeId::PressureOrderBook, dec!(0.7));
        // D has a sub-KG but is dark on Pressure.
        reg.upsert("D.US", Utc::now());

        let mut sm: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        let sid = SectorId("tech".into());
        sm.insert(
            sid.clone(),
            vec![
                Symbol("A.US".into()),
                Symbol("B.US".into()),
                Symbol("C.US".into()),
                Symbol("D.US".into()),
            ],
        );
        let mut sn: HashMap<SectorId, String> = HashMap::new();
        sn.insert(sid, "tech".into());
        let sectors = build_from_registry(&reg, &sm, &sn, Utc::now());

        let evs = detect_member_lag("us", &reg, &sectors, &sm, Utc::now());
        let d_lag = evs
            .iter()
            .find(|e| e.symbol == "D.US" && e.node_kind == "Pressure")
            .expect("D should be flagged as lagging tech sector");
        assert!(d_lag.lag > 0.4, "D lag should be ~0.525 (mean 0.525 - 0)");
        assert!(d_lag.sector_lit_fraction >= 0.7);
    }

    #[test]
    fn cold_sector_no_emission() {
        let mut reg = SubKgRegistry::new();
        // 4 members, only 1 lit — lit_fraction = 0.25 < SYNC_THRESHOLD.
        lit(&mut reg, "A.US", NodeId::PressureOrderBook, dec!(0.7));
        reg.upsert("B.US", Utc::now());
        reg.upsert("C.US", Utc::now());
        reg.upsert("D.US", Utc::now());

        let mut sm: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        let sid = SectorId("tech".into());
        sm.insert(
            sid.clone(),
            vec![
                Symbol("A.US".into()),
                Symbol("B.US".into()),
                Symbol("C.US".into()),
                Symbol("D.US".into()),
            ],
        );
        let mut sn: HashMap<SectorId, String> = HashMap::new();
        sn.insert(sid, "tech".into());
        let sectors = build_from_registry(&reg, &sm, &sn, Utc::now());

        let evs = detect_member_lag("us", &reg, &sectors, &sm, Utc::now());
        assert!(
            evs.is_empty(),
            "cold sector should not emit lag events, got {}",
            evs.len()
        );
    }

    #[test]
    fn overlay_sector_skipped() {
        let mut reg = SubKgRegistry::new();
        lit(&mut reg, "BABA.US", NodeId::PressureOrderBook, dec!(0.7));
        lit(&mut reg, "BIDU.US", NodeId::PressureOrderBook, dec!(0.7));
        reg.upsert("JD.US", Utc::now());

        let mut sm: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        let sid = SectorId("china_adr".into()); // not whitelisted
        sm.insert(
            sid.clone(),
            vec![
                Symbol("BABA.US".into()),
                Symbol("BIDU.US".into()),
                Symbol("JD.US".into()),
            ],
        );
        let mut sn: HashMap<SectorId, String> = HashMap::new();
        sn.insert(sid, "china_adr".into());
        let sectors = build_from_registry(&reg, &sm, &sn, Utc::now());

        let evs = detect_member_lag("us", &reg, &sectors, &sm, Utc::now());
        assert!(
            evs.is_empty(),
            "overlay sector should be skipped, got {}",
            evs.len()
        );
    }

    #[test]
    fn lagging_members_capped_at_top_k() {
        let mut reg = SubKgRegistry::new();
        // 10 hot + 8 dark = 18 total, lit fraction = 0.55 > SYNC_THRESHOLD.
        // 8 dark members all eligible for lag emission, capped at top 5.
        for i in 0..10 {
            lit(
                &mut reg,
                &format!("HOT{}.US", i),
                NodeId::PressureOrderBook,
                dec!(1.0),
            );
        }
        for i in 0..8 {
            reg.upsert(&format!("DARK{}.US", i), Utc::now());
        }

        let mut sm: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
        let sid = SectorId("tech".into());
        let members: Vec<Symbol> = (0..10)
            .map(|i| Symbol(format!("HOT{}.US", i)))
            .chain((0..8).map(|i| Symbol(format!("DARK{}.US", i))))
            .collect();
        sm.insert(sid.clone(), members);
        let mut sn: HashMap<SectorId, String> = HashMap::new();
        sn.insert(sid, "tech".into());
        let sectors = build_from_registry(&reg, &sm, &sn, Utc::now());

        let evs = detect_member_lag("us", &reg, &sectors, &sm, Utc::now());
        let pressure_evs: Vec<_> = evs.iter().filter(|e| e.node_kind == "Pressure").collect();
        assert_eq!(
            pressure_evs.len(),
            TOP_LAGGING_PER_KIND,
            "pressure kind should cap at {}",
            TOP_LAGGING_PER_KIND
        );
    }
}
