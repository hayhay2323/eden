//! Sector sub-KG — first composition layer in Eden's structural visual model.
//!
//! Eden's per-symbol sub-KG (92 nodes) is 1x magnification of structure.
//! `regime_fingerprint` is 100x. The 5x layer in between — "what does this
//! sector look like right now?" — was missing. This module fills it.
//!
//! Method (pure ontology + KG aggregation, no learning, no thresholds I
//! invented):
//!
//!   for each (sector_id, members) in ontology:
//!     for each kind in SECTOR_AGG_KINDS:
//!       member_activation_for_kind = sum |value| over kg.nodes_of_kind(kind)
//!         (matches structural_contrast::detect_contrasts line 88-99 — same
//!         formula so cross-level comparison is meaningful)
//!       sector_kind.mean = mean across members
//!       sector_kind.variance = unbiased sample variance (n-1 denom)
//!       sector_kind.n_lit = members with non-zero activation on this kind
//!       sector_kind.outlier_count = members above 99th percentile (sector-scope)
//!       sector_kind.top_member = argmax member
//!
//! Output: `.run/eden-sector-subkg-{market}.ndjson` — one row per sector
//! per snapshot tick. Sectors with zero member sub-KGs are skipped.
//!
//! Downstream: `structural_contrast` reads `SectorSubKgRegistry` to compute
//! `vs_sector_contrast` alongside the existing `vs_neighbor_contrast`.
//! Pure forward composition (Symbol → Sector). Backward propagation
//! (Sector → Symbol prior adjustment) is a future PR.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::ontology::objects::{SectorId, Symbol};
use crate::pipeline::symbol_sub_kg::{NodeKind, SubKgRegistry};

/// NodeKinds aggregated and FED INTO the contrast hop. These are the same
/// kinds tracked by `structural_contrast::detect_contrasts` so cross-level
/// comparison (`symbol vs own-sector mean` for the same kind) is on
/// comparable scales.
pub const SECTOR_AGG_KINDS: [NodeKind; 7] = [
    NodeKind::Pressure,
    NodeKind::Intent,
    NodeKind::Microstructure,
    NodeKind::Event,
    NodeKind::CapitalFlow,
    NodeKind::Role,
    NodeKind::BookQuality,
];

/// NodeKinds aggregated for ndjson INSPECTION but excluded from the
/// contrast hop. Reasons documented per kind:
///   Holder — homogeneous within a sector (slow-moving, similar regulatory
///     exposure). Variance near-zero, sector mean = no per-tick info.
///   Macro — sector-shared by construction (every member of `tech` shares
///     the same SectorIndexLevel). Sector mean = the value itself.
///   Warrant — concentrated in a few liquid underlyings; mean dominated
///     by 1-2 members.
pub const SECTOR_SUPPLEMENTARY_KINDS: [NodeKind; 3] =
    [NodeKind::Holder, NodeKind::Macro, NodeKind::Warrant];

/// Per-tick sector-scoped percentile threshold for "outlier" inside the
/// sector. Same value as `structural_contrast::NOISE_FLOOR_PERCENTILE`.
pub const SECTOR_OUTLIER_PERCENTILE: f64 = 0.99;

/// Minimum sector size at which we trust the percentile-based outlier
/// computation. Below this, outlier_count = 0.
pub const SECTOR_OUTLIER_MIN_N: usize = 10;

/// 17 base GICS-style sectors that have semantically homogeneous member
/// composition. Used as a whitelist for `vs_sector_contrast` population —
/// US overlay sectors (china_adr, ev_auto, real_estate, etc.) emit
/// sector-subkg ndjson rows but their sector mean is not used in contrast
/// because it would mix structurally dissimilar symbols.
pub const SECTOR_CONTRAST_WHITELIST: [&str; 17] = [
    "tech",
    "semiconductor",
    "finance",
    "energy",
    "telecom",
    "property",
    "consumer",
    "healthcare",
    "utilities",
    "insurance",
    "auto",
    "materials",
    "industrial",
    "conglomerate",
    "media",
    "logistics",
    "education",
];

#[derive(Debug, Clone)]
pub struct KindAggregate {
    pub mean: f64,
    pub variance: f64,
    pub n_lit: usize,
    pub n_total_members: usize,
    pub outlier_count: usize,
    pub max_member_activation: f64,
    pub top_member: Option<String>,
}

impl Serialize for KindAggregate {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut s = ser.serialize_struct("KindAggregate", 7)?;
        s.serialize_field("mean", &self.mean)?;
        s.serialize_field("variance", &self.variance)?;
        s.serialize_field("n_lit", &self.n_lit)?;
        s.serialize_field("n_total_members", &self.n_total_members)?;
        s.serialize_field("outlier_count", &self.outlier_count)?;
        s.serialize_field("max_member_activation", &self.max_member_activation)?;
        s.serialize_field("top_member", &self.top_member)?;
        s.end()
    }
}

#[derive(Debug, Clone)]
pub struct SectorSubKG {
    pub sector_id: String,
    pub sector_name: Option<String>,
    pub ts: DateTime<Utc>,
    pub n_total_members: usize,
    pub n_with_subkg: usize,
    pub coverage_ratio: f64,
    /// NodeKind → aggregate; only contains kinds in SECTOR_AGG_KINDS.
    pub agg: HashMap<NodeKind, KindAggregate>,
    /// Operator-inspection-only aggregates for SECTOR_SUPPLEMENTARY_KINDS.
    pub supplementary: HashMap<NodeKind, KindAggregate>,
}

impl Serialize for SectorSubKG {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        // Use BTreeMap with String keys for stable, JSON-friendly output.
        let agg_map: BTreeMap<String, &KindAggregate> = self
            .agg
            .iter()
            .map(|(k, v)| (format!("{:?}", k), v))
            .collect();
        let supp_map: BTreeMap<String, &KindAggregate> = self
            .supplementary
            .iter()
            .map(|(k, v)| (format!("{:?}", k), v))
            .collect();
        let mut s = ser.serialize_struct("SectorSubKG", 8)?;
        s.serialize_field("sector_id", &self.sector_id)?;
        s.serialize_field("sector_name", &self.sector_name)?;
        s.serialize_field("ts", &self.ts)?;
        s.serialize_field("n_total_members", &self.n_total_members)?;
        s.serialize_field("n_with_subkg", &self.n_with_subkg)?;
        s.serialize_field("coverage_ratio", &self.coverage_ratio)?;
        s.serialize_field("agg", &agg_map)?;
        s.serialize_field("supplementary", &supp_map)?;
        s.end()
    }
}

#[derive(Debug, Default)]
pub struct SectorSubKgRegistry {
    /// sector_id (string) → SectorSubKG
    pub sectors: HashMap<String, SectorSubKG>,
}

impl SectorSubKgRegistry {
    pub fn get(&self, sector_id: &str) -> Option<&SectorSubKG> {
        self.sectors.get(sector_id)
    }
}

/// Compute member activation for one (kg, kind) — sum of |value| across
/// nodes of that kind. Matches `structural_contrast::detect_contrasts`
/// line 88-99 (intentional — comparable across the contrast hop).
fn member_activation_for_kind(
    kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG,
    kind: NodeKind,
) -> f64 {
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

/// Aggregate a list of (member_name, activation) into a KindAggregate.
fn aggregate_kind(per_member: &[(String, f64)], n_total_members: usize) -> KindAggregate {
    let n = per_member.len();
    if n == 0 {
        return KindAggregate {
            mean: 0.0,
            variance: 0.0,
            n_lit: 0,
            n_total_members,
            outlier_count: 0,
            max_member_activation: 0.0,
            top_member: None,
        };
    }
    let sum: f64 = per_member.iter().map(|(_, v)| v).sum();
    let mean = sum / n as f64;
    let variance = if n > 1 {
        let ss: f64 = per_member.iter().map(|(_, v)| (v - mean).powi(2)).sum();
        ss / (n - 1) as f64
    } else {
        0.0
    };
    let n_lit = per_member.iter().filter(|(_, v)| *v > 0.0).count();
    // Argmax — top_member.
    let (top_name, max_v) =
        per_member
            .iter()
            .fold((None, 0.0_f64), |(best_name, best_v), (name, v)| {
                if best_name.is_none() || *v > best_v {
                    (Some(name.clone()), *v)
                } else {
                    (best_name, best_v)
                }
            });
    // Outlier count via per-sector 99th percentile. Skip when sample too small.
    let outlier_count = if n >= SECTOR_OUTLIER_MIN_N {
        let mut sorted: Vec<f64> = per_member.iter().map(|(_, v)| *v).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = (SECTOR_OUTLIER_PERCENTILE * sorted.len() as f64) as usize;
        let floor = sorted
            .get(idx.min(sorted.len() - 1))
            .copied()
            .unwrap_or(0.0);
        per_member.iter().filter(|(_, v)| *v > floor).count()
    } else {
        0
    };
    KindAggregate {
        mean,
        variance,
        n_lit,
        n_total_members,
        outlier_count,
        max_member_activation: max_v,
        top_member: top_name,
    }
}

/// Build all SectorSubKGs for one tick from current symbol-level
/// SubKgRegistry + ontology sector membership.
pub fn build_from_registry(
    registry: &SubKgRegistry,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    sector_names: &HashMap<SectorId, String>,
    ts: DateTime<Utc>,
) -> SectorSubKgRegistry {
    let mut out = SectorSubKgRegistry::default();
    for (sector_id, members) in sector_members {
        let n_total = members.len();
        let mut agg = HashMap::new();
        let mut supp = HashMap::new();
        // Collect per-member sub-KGs that exist.
        let member_kgs: Vec<(String, &crate::pipeline::symbol_sub_kg::SymbolSubKG)> = members
            .iter()
            .filter_map(|sym| registry.get(&sym.0).map(|kg| (sym.0.clone(), kg)))
            .collect();
        let n_with = member_kgs.len();
        let coverage = if n_total > 0 {
            n_with as f64 / n_total as f64
        } else {
            0.0
        };
        // Aggregate kinds that feed contrast.
        for kind in SECTOR_AGG_KINDS {
            let per_member: Vec<(String, f64)> = member_kgs
                .iter()
                .map(|(name, kg)| (name.clone(), member_activation_for_kind(kg, kind)))
                .collect();
            agg.insert(kind, aggregate_kind(&per_member, n_total));
        }
        // Supplementary kinds (operator inspection only).
        for kind in SECTOR_SUPPLEMENTARY_KINDS {
            let per_member: Vec<(String, f64)> = member_kgs
                .iter()
                .map(|(name, kg)| (name.clone(), member_activation_for_kind(kg, kind)))
                .collect();
            supp.insert(kind, aggregate_kind(&per_member, n_total));
        }
        out.sectors.insert(
            sector_id.0.clone(),
            SectorSubKG {
                sector_id: sector_id.0.clone(),
                sector_name: sector_names.get(sector_id).cloned(),
                ts,
                n_total_members: n_total,
                n_with_subkg: n_with,
                coverage_ratio: coverage,
                agg,
                supplementary: supp,
            },
        );
    }
    out
}

/// Append snapshot to ndjson — one line per sector with at least one
/// member sub-KG. Sectors with n_with_subkg == 0 skipped (no signal).
pub fn snapshot_to_ndjson(reg: &SectorSubKgRegistry, market: &str) -> std::io::Result<usize> {
    let lines = serialize_active_to_lines(reg, market)?;
    append_sector_subkg_lines_to_ndjson(market, &lines)
}

/// Serialize active sector sub-KGs to one JSON line each.
/// 2026-04-29: split out so the runtime consumer can serialize
/// synchronously and hand the resulting `Vec<String>` to a background
/// NDJSON writer. Pairs with [`append_sector_subkg_lines_to_ndjson`].
pub fn serialize_active_to_lines(
    reg: &SectorSubKgRegistry,
    market: &str,
) -> std::io::Result<Vec<String>> {
    // Wrap each row with market for easy grep.
    #[derive(Serialize)]
    struct Row<'a> {
        market: &'a str,
        #[serde(flatten)]
        sector: &'a SectorSubKG,
    }
    let mut lines = Vec::with_capacity(reg.sectors.len());
    for sector in reg.sectors.values() {
        if sector.n_with_subkg == 0 {
            continue;
        }
        let row = Row { market, sector };
        let line = serde_json::to_string(&row)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        lines.push(line);
    }
    Ok(lines)
}

/// Pure IO step. Background NDJSON writer task calls this; runtime
/// consumer never blocks on it.
pub fn append_sector_subkg_lines_to_ndjson(
    market: &str,
    lines: &[String],
) -> std::io::Result<usize> {
    if lines.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-sector-subkg-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0usize;
    for line in lines {
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

/// Whitelist check for whether a sector should feed `vs_sector_contrast`
/// in `structural_contrast`. Heterogeneous overlay sectors (china_adr,
/// ev_auto, etc.) return false — their sector mean mixes structurally
/// dissimilar symbols.
pub fn sector_in_contrast_whitelist(sector_id: &str) -> bool {
    SECTOR_CONTRAST_WHITELIST.iter().any(|s| *s == sector_id)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use rust_decimal_macros::dec;

    fn lit(reg: &mut SubKgRegistry, sym: &str, node: NodeId, v: rust_decimal::Decimal) {
        reg.upsert(sym, Utc::now())
            .set_node_value(node, v, Utc::now());
    }

    fn make_sector(
        name: &str,
        members: Vec<&str>,
    ) -> (HashMap<SectorId, Vec<Symbol>>, HashMap<SectorId, String>) {
        let mut sm = HashMap::new();
        let mut sn = HashMap::new();
        let sid = SectorId(name.into());
        sm.insert(
            sid.clone(),
            members.iter().map(|s| Symbol(s.to_string())).collect(),
        );
        sn.insert(sid, name.to_string());
        (sm, sn)
    }

    #[test]
    fn aggregate_three_members_with_subkg_computes_mean_and_top() {
        let mut reg = SubKgRegistry::new();
        // Three tech members, all have PressureOrderBook activation.
        lit(&mut reg, "A.US", NodeId::PressureOrderBook, dec!(0.30));
        lit(&mut reg, "B.US", NodeId::PressureOrderBook, dec!(0.50));
        lit(&mut reg, "C.US", NodeId::PressureOrderBook, dec!(0.70));
        let (sm, sn) = make_sector("tech", vec!["A.US", "B.US", "C.US"]);
        let out = build_from_registry(&reg, &sm, &sn, Utc::now());
        let tech = out.get("tech").expect("tech sector");
        assert_eq!(tech.n_total_members, 3);
        assert_eq!(tech.n_with_subkg, 3);
        assert!((tech.coverage_ratio - 1.0).abs() < 1e-9);
        let pressure = tech.agg.get(&NodeKind::Pressure).expect("pressure agg");
        assert!(
            (pressure.mean - 0.5).abs() < 1e-9,
            "mean = {}",
            pressure.mean
        );
        assert_eq!(pressure.n_lit, 3);
        assert_eq!(pressure.top_member.as_deref(), Some("C.US"));
        assert!((pressure.max_member_activation - 0.7).abs() < 1e-9);
    }

    #[test]
    fn empty_sector_skipped_in_ndjson() {
        let reg = SubKgRegistry::new();
        let (sm, sn) = make_sector("tech", vec!["X.US", "Y.US"]); // members not in registry
        let out = build_from_registry(&reg, &sm, &sn, Utc::now());
        let tech = out.get("tech").expect("tech sector entry exists");
        assert_eq!(tech.n_with_subkg, 0);
        // snapshot_to_ndjson should write 0 rows
        let market = "test_empty";
        let path = format!(".run/eden-sector-subkg-{}.ndjson", market);
        let _ = std::fs::remove_file(&path);
        let written = snapshot_to_ndjson(&out, market).expect("write ok");
        assert_eq!(written, 0, "empty sector should produce 0 rows");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mixed_coverage_uses_n_with_subkg_as_denom() {
        let mut reg = SubKgRegistry::new();
        // Only A.US and B.US have data; C.US, D.US declared in sector but no kg.
        lit(&mut reg, "A.US", NodeId::PressureOrderBook, dec!(0.40));
        lit(&mut reg, "B.US", NodeId::PressureOrderBook, dec!(0.60));
        let (sm, sn) = make_sector("tech", vec!["A.US", "B.US", "C.US", "D.US"]);
        let out = build_from_registry(&reg, &sm, &sn, Utc::now());
        let tech = out.get("tech").unwrap();
        assert_eq!(tech.n_total_members, 4);
        assert_eq!(tech.n_with_subkg, 2);
        assert!((tech.coverage_ratio - 0.5).abs() < 1e-9);
        // mean = (0.4 + 0.6) / 2 = 0.5
        let p = tech.agg.get(&NodeKind::Pressure).unwrap();
        assert!((p.mean - 0.5).abs() < 1e-9);
    }

    #[test]
    fn whitelist_respects_overlay_exclusion() {
        assert!(sector_in_contrast_whitelist("tech"));
        assert!(sector_in_contrast_whitelist("semiconductor"));
        assert!(!sector_in_contrast_whitelist("china_adr"));
        assert!(!sector_in_contrast_whitelist("etf"));
        assert!(!sector_in_contrast_whitelist("crypto"));
        assert!(!sector_in_contrast_whitelist("other"));
    }
}
