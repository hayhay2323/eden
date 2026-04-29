//! Regime fingerprint — continuous embedding replacement for the
//! discrete `RegimeType` enum (US) / `LiveWorldSummary.regime` string
//! (HK). Both markets emit a 5-dimensional universal feature vector
//! (`stress`, `synchrony`, `bull_bias`, `activity`, `turn_pressure`)
//! plus market-specific extension fields, plus a deterministic
//! `bucket_key` for use as a key in `ConditionedLearningAdjustment`.
//!
//! Why: the existing classifiers are hand-tuned thresholds (e.g.
//! `bull_bear_ratio >= 2.5 && pu_trend < -0.03`). They put 24h of
//! continuous market behaviour into 5–6 bins and discard everything
//! about distance / trajectory / similarity to past sessions. The
//! fingerprint preserves the underlying signal so downstream code can
//! (a) feed a regime context into `conditioned_delta`,
//! (b) measure cosine drift between adjacent ticks, and
//! (c) eventually find the most-similar past regime episode.
//!
//! Phase 1 (this file): deterministic feature builders + fixed-quintile
//! bucket key. No clustering, no learned basis. Phase 2 will replace
//! `bucket_key` with k-means / online clustering.

use rust_decimal::prelude::ToPrimitive;
#[cfg(test)]
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 5-dimensional universal feature vector — comparable across markets.
/// The market-specific extension fields are stored separately and not
/// used for cosine similarity.
pub const FINGERPRINT_DIMS: usize = 5;

/// Per-dimension quantile cuts. v1 are hand-picked from observed live
/// distributions; Phase 2 will derive them from rolling Welford stats.
const STRESS_CUTS: [f64; 4] = [0.05, 0.10, 0.20, 0.35];
const SYNC_CUTS: [f64; 4] = [0.30, 0.50, 0.65, 0.80];
const BIAS_CUTS: [f64; 4] = [0.30, 0.45, 0.55, 0.70];
const ACT_CUTS: [f64; 3] = [0.10, 0.35, 0.65];
const TURN_CUTS: [f64; 3] = [0.20, 0.50, 0.80];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimeFingerprint {
    pub market: String,
    pub tick: u64,
    /// ISO-8601 UTC timestamp.
    pub snapshot_ts: String,

    // ===== Universal 5-dim feature vector =====
    pub stress: f64,
    pub synchrony: f64,
    /// `bull_count / (bull_count + bear_count)` clipped to [0, 1].
    /// 0.5 = neutral, >0.5 = bullish, <0.5 = bearish.
    pub bull_bias: f64,
    /// `min(active_count, 30) / 30` — proxy for "is Eden actually
    /// finding things to do".
    pub activity: f64,
    /// `turning_point_count / max(meaningful_count, 1)` — proxy for
    /// regime instability / reversal pressure.
    pub turn_pressure: f64,

    // ===== Market-specific extension fields (Optional) =====
    /// US planner utility — not available in HK.
    pub planner_utility: Option<f64>,
    /// HK regime continuity — `state_persistence_ticks / 20`,
    /// clipped to [0, 1]. Not available in US.
    pub regime_continuity: Option<f64>,
    /// HK dominant `StructuralDriverClass` label, e.g. `"sector_wave"`.
    /// Not available in US.
    pub dominant_driver: Option<String>,

    // ===== Backward compatibility =====
    /// Discrete legacy label (`RegimeType::label()` for US, the HK
    /// `LiveWorldSummary.regime` string for HK). Stored alongside the
    /// vector so downstream consumers can keep using the old label
    /// during transition.
    pub legacy_label: String,
    pub legacy_confidence: f64,

    // ===== Derived key =====
    /// Deterministic bucket identifier, e.g.
    /// `"stress=2|sync=3|bias=4|act=2|turn=1"`. Same across markets so
    /// that learning examples coalesce by shared regime shape.
    pub bucket_key: String,
}

impl RegimeFingerprint {
    /// Returns the 5 universal features as `[stress, synchrony,
    /// bull_bias, activity, turn_pressure]`. Used for cosine similarity
    /// between fingerprints.
    pub fn universal_vec(&self) -> [f64; FINGERPRINT_DIMS] {
        [
            self.stress,
            self.synchrony,
            self.bull_bias,
            self.activity,
            self.turn_pressure,
        ]
    }
}

/// Build a `RegimeFingerprint` from US runtime inputs (existing
/// `RegimeInputs` from `regime_classifier`). Pairs cleanly with the
/// existing `classify()` call site so the fingerprint piggybacks on
/// data already in scope.
pub fn build_us_fingerprint(
    market: impl Into<String>,
    tick: u64,
    snapshot_ts: impl Into<String>,
    inputs: crate::pipeline::regime_classifier::RegimeInputs,
    legacy_label: impl Into<String>,
) -> RegimeFingerprint {
    let bull_bias = bull_bias_from_ratio(inputs.bull_bear_ratio);
    let activity = activity_from_count(inputs.active_count);
    // US has no per-cluster turning_point breakdown; use the magnitude
    // of the bull/bear trend as a turn-pressure proxy. Big swings in
    // either direction count as "the regime is turning".
    let turn_pressure = inputs.bull_bear_trend_24_cycle.abs().clamp(0.0, 1.0);

    let stress = inputs.stress.clamp(0.0, 1.0);
    let synchrony = inputs.synchrony.clamp(0.0, 1.0);
    let bucket_key = bucket_key(stress, synchrony, bull_bias, activity, turn_pressure);

    RegimeFingerprint {
        market: market.into(),
        tick,
        snapshot_ts: snapshot_ts.into(),
        stress,
        synchrony,
        bull_bias,
        activity,
        turn_pressure,
        planner_utility: Some(inputs.planner_utility.clamp(0.0, 1.0)),
        regime_continuity: None,
        dominant_driver: None,
        legacy_label: legacy_label.into(),
        legacy_confidence: 1.0, // US classifier is deterministic
        bucket_key,
    }
}

/// Build a `RegimeFingerprint` from HK runtime inputs. HK does not
/// produce `RegimeInputs` (it computes its regime from cluster
/// topology in `state_engine::build_world_summary`), so we accept the
/// already-derived stress / synchrony plus the cluster vector + world
/// summary and derive the universal features here.
pub fn build_hk_fingerprint(
    market: impl Into<String>,
    tick: u64,
    snapshot_ts: impl Into<String>,
    stress: f64,
    synchrony: f64,
    clusters: &[crate::live_snapshot::LiveClusterState],
    world: &crate::live_snapshot::LiveWorldSummary,
    dominant_driver: Option<String>,
) -> RegimeFingerprint {
    let stress = stress.clamp(0.0, 1.0);
    let synchrony = synchrony.clamp(0.0, 1.0);
    let bull_bias = bull_bias_from_clusters(clusters);
    let activity = hk_activity_from_clusters(clusters);
    let turn_pressure = hk_turn_pressure_from_clusters(clusters);

    let bucket_key = bucket_key(stress, synchrony, bull_bias, activity, turn_pressure);
    let regime_continuity = (world.state_persistence_ticks as f64 / 20.0).clamp(0.0, 1.0);
    let legacy_confidence = world.confidence.to_f64().unwrap_or(0.0).clamp(0.0, 1.0);

    RegimeFingerprint {
        market: market.into(),
        tick,
        snapshot_ts: snapshot_ts.into(),
        stress,
        synchrony,
        bull_bias,
        activity,
        turn_pressure,
        planner_utility: None,
        regime_continuity: Some(regime_continuity),
        dominant_driver,
        legacy_label: world.regime.clone(),
        legacy_confidence,
        bucket_key,
    }
}

/// Build a per-symbol regime fingerprint. Inputs are intentionally
/// decoupled from the broader market snapshot so the caller computes
/// them from whatever single-symbol state is most available:
/// - `stress`   usually the symbol's own residual / pressure magnitude
///              normalized to [0, 1].
/// - `synchrony` alignment across this symbol's horizons (e.g. fraction
///              of active horizons sharing direction).
/// - `bull_bias` long-share of recent setups / direction dominance.
/// - `activity` recent setup count or attention share, normalized.
/// - `turn_pressure` per-symbol RegimeChanged / conflict events over
///              a short rolling window, normalized to [0, 1].
///
/// Enables the `9636`-style case where the overall market regime is
/// locked in `reversal_prone` but a specific symbol is cleanly trending —
/// operators / entry gates can then condition on the *symbol* bucket
/// instead of being blocked by the market bucket alone.
#[allow(clippy::too_many_arguments)]
pub fn build_symbol_fingerprint(
    market: impl Into<String>,
    tick: u64,
    snapshot_ts: impl Into<String>,
    symbol: impl Into<String>,
    stress: f64,
    synchrony: f64,
    bull_bias: f64,
    activity: f64,
    turn_pressure: f64,
    symbol_state_persistence_ticks: Option<u64>,
    dominant_driver: Option<String>,
    legacy_label: impl Into<String>,
) -> RegimeFingerprint {
    let stress = stress.clamp(0.0, 1.0);
    let synchrony = synchrony.clamp(0.0, 1.0);
    let bull_bias = bull_bias.clamp(0.0, 1.0);
    let activity = activity.clamp(0.0, 1.0);
    let turn_pressure = turn_pressure.clamp(0.0, 1.0);
    let bucket_key = bucket_key(stress, synchrony, bull_bias, activity, turn_pressure);
    let regime_continuity =
        symbol_state_persistence_ticks.map(|ticks| (ticks as f64 / 20.0).clamp(0.0, 1.0));
    // Symbol fingerprints are tagged in the `market` field with a
    // `symbol:` prefix so downstream consumers can segregate per-symbol
    // from market-level buckets without schema changes.
    let symbol_label = symbol.into();
    let market_tag = format!("{}:sym:{}", market.into(), symbol_label);

    RegimeFingerprint {
        market: market_tag,
        tick,
        snapshot_ts: snapshot_ts.into(),
        stress,
        synchrony,
        bull_bias,
        activity,
        turn_pressure,
        planner_utility: None,
        regime_continuity,
        dominant_driver,
        legacy_label: legacy_label.into(),
        legacy_confidence: 1.0,
        bucket_key,
    }
}

/// Cosine similarity between the universal feature vectors of two
/// fingerprints. Returns 1.0 for identical regimes, 0.0 for orthogonal.
/// Both vectors must contain at least one non-zero element; otherwise
/// returns 0.0.
pub fn cosine(a: &RegimeFingerprint, b: &RegimeFingerprint) -> f64 {
    let av = a.universal_vec();
    let bv = b.universal_vec();
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..FINGERPRINT_DIMS {
        dot += av[i] * bv[i];
        na += av[i] * av[i];
        nb += bv[i] * bv[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Deterministic bucket key, used as `conditioned_on` value in
/// `ConditionedLearningAdjustment`. Phase 2 will replace this with a
/// k-means cluster id.
pub fn bucket_key(
    stress: f64,
    synchrony: f64,
    bull_bias: f64,
    activity: f64,
    turn_pressure: f64,
) -> String {
    format!(
        "stress={}|sync={}|bias={}|act={}|turn={}",
        bin(stress, &STRESS_CUTS),
        bin(synchrony, &SYNC_CUTS),
        bin(bull_bias, &BIAS_CUTS),
        bin(activity, &ACT_CUTS),
        bin(turn_pressure, &TURN_CUTS),
    )
}

fn bin(value: f64, cuts: &[f64]) -> usize {
    for (idx, cut) in cuts.iter().enumerate() {
        if value < *cut {
            return idx;
        }
    }
    cuts.len()
}

fn bull_bias_from_ratio(ratio: f64) -> f64 {
    // ratio == bulls / bears. Convert to bull share via
    // r / (r + 1). Clamp ratio to [0, 100] to avoid extreme values
    // when bear count is zero.
    let r = ratio.clamp(0.0, 100.0);
    if r == 0.0 {
        0.0
    } else {
        (r / (r + 1.0)).clamp(0.0, 1.0)
    }
}

fn activity_from_count(active_count: usize) -> f64 {
    (active_count.min(30) as f64) / 30.0
}

fn bull_bias_from_clusters(clusters: &[crate::live_snapshot::LiveClusterState]) -> f64 {
    let mut bullish = 0usize;
    let mut bearish = 0usize;
    let mut total = 0usize;
    for cluster in clusters {
        if cluster.state == "low_information" {
            continue;
        }
        match cluster.direction.as_str() {
            "long" => {
                bullish += 1;
                total += 1;
            }
            "short" => {
                bearish += 1;
                total += 1;
            }
            _ => {
                total += 1;
            }
        }
    }
    if total == 0 {
        return 0.5;
    }
    let total = (bullish + bearish) as f64;
    if total == 0.0 {
        return 0.5;
    }
    bullish as f64 / total
}

fn hk_activity_from_clusters(clusters: &[crate::live_snapshot::LiveClusterState]) -> f64 {
    let meaningful = clusters
        .iter()
        .filter(|cluster| cluster.state != "low_information")
        .count();
    (meaningful.min(6) as f64) / 6.0
}

fn hk_turn_pressure_from_clusters(clusters: &[crate::live_snapshot::LiveClusterState]) -> f64 {
    let mut meaningful = 0usize;
    let mut turning = 0usize;
    for cluster in clusters {
        if cluster.state == "low_information" {
            continue;
        }
        meaningful += 1;
        if cluster.state == "turning_point" || cluster.state == "conflicted" {
            turning += 1;
        }
    }
    if meaningful == 0 {
        return 0.0;
    }
    turning as f64 / meaningful as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::regime_classifier::RegimeInputs;
    use rust_decimal_macros::dec;

    fn cluster(
        state: &str,
        direction: &str,
        label: &str,
    ) -> crate::live_snapshot::LiveClusterState {
        crate::live_snapshot::LiveClusterState {
            cluster_key: label.into(),
            label: label.into(),
            direction: direction.into(),
            state: state.into(),
            confidence: dec!(0.5),
            member_count: 1,
            leader_symbols: vec![],
            laggard_symbols: vec![],
            summary: String::new(),
            age_ticks: 0,
            state_persistence_ticks: 0,
            trend: String::new(),
            last_transition_summary: None,
        }
    }

    fn world(
        regime: &str,
        conf: Decimal,
        persistence: u16,
    ) -> crate::live_snapshot::LiveWorldSummary {
        crate::live_snapshot::LiveWorldSummary {
            regime: regime.into(),
            confidence: conf,
            dominant_clusters: vec![],
            summary: String::new(),
            age_ticks: 0,
            state_persistence_ticks: persistence,
            trend: String::new(),
            last_transition_summary: None,
        }
    }

    #[test]
    fn bucket_key_is_stable() {
        let key = bucket_key(0.13, 0.71, 0.57, 0.45, 0.30);
        assert_eq!(key, "stress=2|sync=3|bias=3|act=2|turn=1");
    }

    #[test]
    fn bin_handles_extremes() {
        assert_eq!(bin(0.0, &STRESS_CUTS), 0);
        assert_eq!(bin(1.0, &STRESS_CUTS), STRESS_CUTS.len());
        assert_eq!(bin(0.05 - f64::EPSILON, &STRESS_CUTS), 0);
        assert_eq!(bin(0.05, &STRESS_CUTS), 1);
    }

    #[test]
    fn us_fingerprint_round_trip() {
        let inputs = RegimeInputs {
            stress: 0.13,
            synchrony: 0.71,
            planner_utility: 0.57,
            bull_bear_ratio: 1.31,
            active_count: 9,
            planner_utility_trend_24_cycle: 0.10,
            bull_bear_trend_24_cycle: 0.30,
        };
        let fp = build_us_fingerprint("us", 100, "2026-04-23T03:30:00Z", inputs, "orderly_trend");
        assert_eq!(fp.market, "us");
        assert_eq!(fp.legacy_label, "orderly_trend");
        assert!((fp.stress - 0.13).abs() < 1e-9);
        assert!((fp.synchrony - 0.71).abs() < 1e-9);
        // bull_bias = 1.31 / 2.31 ≈ 0.567
        assert!((fp.bull_bias - 0.567).abs() < 0.01);
        // activity = 9/30 = 0.3
        assert!((fp.activity - 0.3).abs() < 1e-9);
        // turn_pressure = |0.30| = 0.30
        assert!((fp.turn_pressure - 0.30).abs() < 1e-9);
        assert_eq!(fp.planner_utility, Some(0.57));
        assert_eq!(fp.regime_continuity, None);
        assert_eq!(fp.dominant_driver, None);
        // bucket_key recomputed
        assert_eq!(fp.bucket_key, bucket_key(0.13, 0.71, 0.567, 0.3, 0.30));
    }

    #[test]
    fn hk_fingerprint_with_clusters_and_world() {
        let clusters = vec![
            cluster("turning_point", "long", "Auto"),
            cluster("continuation", "long", "Tech"),
            cluster("turning_point", "short", "Banks"),
            cluster("low_information", "mixed", "Materials"), // ignored
        ];
        let w = world("reversal_prone", dec!(0.35), 8);
        let fp = build_hk_fingerprint(
            "hk",
            42,
            "2026-04-23T03:50:00Z",
            0.18,
            0.62,
            &clusters,
            &w,
            Some("sector_wave".into()),
        );
        assert_eq!(fp.market, "hk");
        assert_eq!(fp.legacy_label, "reversal_prone");
        assert!((fp.legacy_confidence - 0.35).abs() < 1e-9);
        // bull_bias: 2 long, 1 short → 2/3 ≈ 0.667
        assert!((fp.bull_bias - 0.667).abs() < 0.01);
        // activity: 3 meaningful / 6 = 0.5
        assert!((fp.activity - 0.5).abs() < 1e-9);
        // turn_pressure: 2 turning_point / 3 meaningful ≈ 0.667
        assert!((fp.turn_pressure - 0.667).abs() < 0.01);
        // regime_continuity: 8 / 20 = 0.4
        assert_eq!(fp.regime_continuity, Some(0.4));
        assert_eq!(fp.dominant_driver, Some("sector_wave".into()));
        assert_eq!(fp.planner_utility, None);
    }

    #[test]
    fn hk_empty_clusters_safe_defaults() {
        let w = world("low_information", dec!(0.0), 0);
        let fp = build_hk_fingerprint("hk", 1, "ts", 0.0, 0.0, &[], &w, None);
        assert_eq!(fp.bull_bias, 0.5);
        assert_eq!(fp.activity, 0.0);
        assert_eq!(fp.turn_pressure, 0.0);
        assert_eq!(fp.regime_continuity, Some(0.0));
    }

    #[test]
    fn hk_only_low_information_treated_as_empty() {
        let clusters = vec![
            cluster("low_information", "mixed", "A"),
            cluster("low_information", "long", "B"),
        ];
        let w = world("low_information", dec!(0.0), 0);
        let fp = build_hk_fingerprint("hk", 1, "ts", 0.0, 0.0, &clusters, &w, None);
        assert_eq!(fp.bull_bias, 0.5);
        assert_eq!(fp.activity, 0.0);
        assert_eq!(fp.turn_pressure, 0.0);
    }

    #[test]
    fn unknown_direction_does_not_panic_and_does_not_count() {
        let clusters = vec![
            cluster("turning_point", "long", "A"),
            cluster("continuation", "weird-unknown", "B"),
        ];
        let w = world("trend", dec!(0.5), 5);
        let fp = build_hk_fingerprint("hk", 1, "ts", 0.1, 0.5, &clusters, &w, None);
        // 1 long, 0 short → bull_bias = 1.0
        assert_eq!(fp.bull_bias, 1.0);
        // 2 meaningful / 6 = 0.333
        assert!((fp.activity - 0.333).abs() < 0.01);
    }

    #[test]
    fn cosine_identity_and_orthogonal() {
        let inputs = RegimeInputs {
            stress: 0.13,
            synchrony: 0.71,
            planner_utility: 0.57,
            bull_bear_ratio: 1.31,
            active_count: 9,
            planner_utility_trend_24_cycle: 0.10,
            bull_bear_trend_24_cycle: 0.30,
        };
        let a = build_us_fingerprint("us", 1, "ts1", inputs, "orderly_trend");
        let b = a.clone();
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-9);

        // Zero vector → cosine returns 0.0
        let mut z = a.clone();
        z.stress = 0.0;
        z.synchrony = 0.0;
        z.bull_bias = 0.0;
        z.activity = 0.0;
        z.turn_pressure = 0.0;
        assert_eq!(cosine(&z, &b), 0.0);
    }

    #[test]
    fn bull_bias_from_ratio_extremes() {
        assert!((bull_bias_from_ratio(0.0) - 0.0).abs() < 1e-9);
        assert!((bull_bias_from_ratio(1.0) - 0.5).abs() < 1e-9);
        // Very large ratios still clamp into [0,1]
        let high = bull_bias_from_ratio(50.0);
        assert!(high >= 0.5 && high <= 1.0);
    }

    // --- build_symbol_fingerprint tests ---

    #[test]
    fn symbol_fingerprint_tags_market_field() {
        let fp = build_symbol_fingerprint(
            "hk",
            100,
            "2026-04-23T04:00:00Z",
            "9636.HK",
            0.08,
            0.82,
            0.72,
            0.40,
            0.05,
            Some(12),
            Some("sector_wave".into()),
            "trend",
        );
        assert_eq!(fp.market, "hk:sym:9636.HK");
        assert_eq!(fp.legacy_label, "trend");
        assert_eq!(fp.regime_continuity, Some(0.6));
        assert_eq!(fp.dominant_driver, Some("sector_wave".into()));
        assert_eq!(fp.planner_utility, None);
    }

    #[test]
    fn symbol_fingerprint_clamps_input_values() {
        let fp = build_symbol_fingerprint(
            "us", 1, "ts", "NVDA",
            // Deliberately out-of-bound inputs on both ends to verify
            // the primitive doesn't trust callers to normalize.
            1.5, -0.2, 2.0, 5.0, -1.0, None, None, "trend",
        );
        assert!((fp.stress - 1.0).abs() < 1e-9);
        assert!((fp.synchrony - 0.0).abs() < 1e-9);
        assert!((fp.bull_bias - 1.0).abs() < 1e-9);
        assert!((fp.activity - 1.0).abs() < 1e-9);
        assert!((fp.turn_pressure - 0.0).abs() < 1e-9);
    }

    #[test]
    fn symbol_bucket_key_matches_market_bucket_for_same_features() {
        let tick = 42u64;
        let ts = "2026-04-23T04:00:00Z";
        let stress = 0.13;
        let synchrony = 0.71;
        let bull_bias = 0.57;
        let activity = 0.45;
        let turn = 0.30;
        let symbol_fp = build_symbol_fingerprint(
            "hk",
            tick,
            ts,
            "TEST.HK",
            stress,
            synchrony,
            bull_bias,
            activity,
            turn,
            Some(0),
            None,
            "trend",
        );
        // Identical 5-dim features → identical bucket_key regardless of
        // market-vs-symbol tag. This is the *intended* property so that
        // learning examples from the same structural regime can coalesce
        // across market-level and symbol-level scopes.
        assert_eq!(
            symbol_fp.bucket_key,
            bucket_key(stress, synchrony, bull_bias, activity, turn)
        );
    }

    #[test]
    fn symbol_fingerprint_regime_continuity_none_when_unknown() {
        let fp = build_symbol_fingerprint(
            "us", 1, "ts", "AAPL", 0.0, 0.0, 0.5, 0.0, 0.0, None, None, "mixed",
        );
        assert_eq!(fp.regime_continuity, None);
    }
}
