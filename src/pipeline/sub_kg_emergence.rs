//! Sub-KG emergence — graph-native setup synthesis.
//!
//! V4 first-principles fix: setup is no longer a derived view of the
//! reasoning_layer's vortex selection. It's a *property of the symbol's
//! own sub-KG state vector* — when multiple NodeIds across NodeKinds
//! synchronously transition from "uninformed / neutral / non-surprising"
//! to "informed / directional / surprising", that's an emergence event,
//! and that event *is* a setup.
//!
//! The reasoning_layer continues to emit vortex-derived setups (existing
//! pressure::bridge path). This module emits a *parallel* stream of
//! emergence-derived setups so symbols that the reasoning_layer never
//! selects (e.g., AFRM, with no pressure vortex but visible KL surprise +
//! capital_flow inflow + post-market positive) still surface to the
//! action_promotion dispatcher.
//!
//! Design constraints (V3 first-principles):
//!   - emergence_score is a *self-referential* graph quantity per symbol;
//!     no global threshold, no hand-coded magic number
//!   - emit only when current score exceeds the symbol's *own* EWMA
//!     baseline by ≥ 1σ (Gaussian first principle, mirrors KL surprise)
//!   - direction inferred from KlSurpriseDirection NodeId (already on the
//!     sub-KG via Phase 1) — no new direction inference logic

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    default_case_horizon, DecisionLineage, ReasoningScope, TacticalAction, TacticalDirection,
    TacticalSetup,
};
use crate::pipeline::graph_attention::AttentionBudget;
use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry, SymbolSubKG};

/// EWMA decay for per-symbol emergence baseline. Matches `kl_surprise`
/// — same memory-length design (~69 ticks half-life), keeps the two
/// trackers comparable.
pub const EMERGENCE_EWMA_DECAY: f64 = 0.99;

/// Minimum baseline samples before z is meaningful. Matches
/// `KL_SURPRISE_BOOTSTRAP_MIN`; both are statistical floors, not
/// behavioural thresholds.
pub const EMERGENCE_BOOTSTRAP_MIN: u32 = 5;

/// Z-score floor for emit decision (Gaussian first principle: 1σ).
/// Pure statistical reference, not a hand-tuned business knob.
pub const EMERGENCE_EMIT_Z: f64 = 1.0;

/// Per-symbol running EWMA baseline of emergence_score.
#[derive(Debug, Clone, Default)]
struct EmergenceBaseline {
    mean: f64,
    mean_sq: f64,
    sample_count: u32,
}

impl EmergenceBaseline {
    fn observe(&mut self, score: f64) {
        if !score.is_finite() {
            return;
        }
        if self.sample_count == 0 {
            self.mean = score;
            self.mean_sq = score * score;
        } else {
            let alpha = 1.0 - EMERGENCE_EWMA_DECAY;
            self.mean = (1.0 - alpha) * self.mean + alpha * score;
            self.mean_sq = (1.0 - alpha) * self.mean_sq + alpha * score * score;
        }
        self.sample_count = self.sample_count.saturating_add(1);
    }

    fn variance(&self) -> f64 {
        (self.mean_sq - self.mean * self.mean).max(0.0)
    }

    fn std_dev(&self) -> f64 {
        self.variance().sqrt()
    }

    fn z(&self, score: f64) -> Option<f64> {
        if !score.is_finite() {
            return None;
        }
        if self.sample_count < EMERGENCE_BOOTSTRAP_MIN {
            return None;
        }
        let std = self.std_dev();
        if std < 1.0e-9 {
            return None;
        }
        Some((score - self.mean) / std)
    }
}

/// One emergence event detected on a symbol's sub-KG.
#[derive(Debug, Clone)]
pub struct SubKgEmergence {
    pub symbol: String,
    pub tick: u64,
    pub ts: DateTime<Utc>,
    /// Self-referential z-score against the symbol's own historical
    /// emergence baseline.
    pub z: f64,
    /// Raw emergence score in [0, 1] (mean of contributing components).
    pub score: f64,
    /// Direction signal derived from KlSurpriseDirection NodeId.
    /// `+1` = bullish surprise, `-1` = bearish, `0` = neutral.
    pub direction: i8,
    /// Components that contributed (kl_mag, 1-entropy, sample_count,
    /// wl_conf, intent_concentration). Stored for risk_note + debug.
    pub components: [f64; 5],
}

/// Tracks per-symbol baselines for emergence detection.
#[derive(Debug, Default)]
pub struct SubKgEmergenceTracker {
    baselines: HashMap<String, EmergenceBaseline>,
    /// V5.1: graph-attention budget — high-centrality symbols processed
    /// every tick, low-centrality symbols throttled. Eliminates the
    /// O(N=639) per-tick walk that triggered Phase 1.5 throughput
    /// overflow.
    attention: AttentionBudget,
}

impl SubKgEmergenceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn baseline_count(&self) -> usize {
        self.baselines.len()
    }

    /// Walk every sub-KG in `registry`, compute emergence_score, observe
    /// it into the per-symbol baseline, and emit a SubKgEmergence for
    /// every symbol whose current score clears `EMERGENCE_EMIT_Z` against
    /// its own baseline.
    ///
    /// V5.1: `centrality` (per-symbol [0, 1] graph-derived weight,
    /// typically `centrality_from_hubs(&prev_tick_hubs)`) drives the
    /// attention budget — high-centrality symbols processed every tick,
    /// low-centrality every up to `MAX_SKIP_TICKS`. Pass an empty
    /// HashMap to retain old "process every symbol every tick" behaviour
    /// (the budget then defaults all symbols to centrality 0; the
    /// "never processed" fallback in `should_process` ensures every
    /// symbol still runs at least once per `MAX_SKIP_TICKS` ticks).
    ///
    /// V5.2: emergence score is multiplied by *liquidity confidence* —
    /// the cross-symbol percentile rank of the symbol's current
    /// turnover. Illiquid symbols (low turnover) end up with a low
    /// confidence factor, so even a high z-score on a quiet sym
    /// contributes ~0 to the surfaced emergence list. No hand-coded
    /// vol filter; the rank is purely cross-sectional.
    pub fn detect_emergences(
        &mut self,
        registry: &SubKgRegistry,
        centrality: &HashMap<String, f64>,
        frontier: Option<&std::collections::HashSet<String>>,
    ) -> Vec<SubKgEmergence> {
        // V5.2: compute cross-symbol turnover distribution once per
        // tick so liquidity confidence is purely cross-sectional rank.
        let liquidity_confidence = compute_liquidity_confidence(registry);

        let mut events = Vec::new();
        for (symbol, kg) in &registry.graphs {
            // V7.2: when the runtime supplies a frontier (the set of
            // symbols whose pressure-channel propagation passed the
            // self-referential noise floor), skip everything outside
            // it. None preserves prior "walk every symbol" behaviour
            // for tests and back-compat callers.
            if let Some(frontier_symbols) = frontier {
                if !frontier_symbols.contains(symbol) {
                    continue;
                }
            }
            let c = centrality.get(symbol).copied().unwrap_or(0.0);
            if !self.attention.should_process(symbol, kg.tick, c) {
                continue;
            }
            let (raw_score, components, direction) = compute_emergence_score(kg);
            if !raw_score.is_finite() {
                self.attention.mark_processed(symbol, kg.tick);
                continue;
            }
            // V5.2: weight by liquidity confidence (cross-symbol percentile).
            let liq = liquidity_confidence.get(symbol).copied().unwrap_or(0.0);
            let score = raw_score * liq;
            // Observe AFTER reading z so we measure the new sample's
            // surprise against the prior baseline.
            let z_opt = self.baselines.get(symbol).and_then(|b| b.z(score));
            self.baselines
                .entry(symbol.clone())
                .or_default()
                .observe(score);
            self.attention.mark_processed(symbol, kg.tick);
            let Some(z) = z_opt else { continue };
            if z >= EMERGENCE_EMIT_Z {
                events.push(SubKgEmergence {
                    symbol: symbol.clone(),
                    tick: kg.tick,
                    ts: kg.ts,
                    z,
                    score,
                    direction,
                    components,
                });
            }
        }
        events
    }
}

/// V5.2: compute per-symbol liquidity confidence as the cross-symbol
/// percentile rank of `Turnover` NodeId. Returns [0, 1] per symbol.
/// Symbols with the highest turnover get 1.0, lowest get 0.0. Self-
/// referential, no hand-coded threshold.
///
/// Edge cases (no rank possible) default to confidence 1.0 so the
/// caller doesn't accidentally zero out emergence on small registries
/// or test fixtures:
/// - 0 symbols → empty map
/// - 1 symbol → confidence 1.0 (no peer to compare against)
/// - All symbols identical turnover → confidence 1.0 (no spread)
pub fn compute_liquidity_confidence(registry: &SubKgRegistry) -> HashMap<String, f64> {
    use rust_decimal::prelude::ToPrimitive;
    let turnovers: Vec<(String, f64)> = registry
        .graphs
        .iter()
        .map(|(sym, kg)| {
            let t = kg
                .nodes
                .get(&NodeId::Turnover)
                .and_then(|n| n.value)
                .and_then(|v| v.to_f64())
                .unwrap_or(0.0)
                .max(0.0);
            (sym.clone(), t)
        })
        .collect();
    if turnovers.is_empty() {
        return HashMap::new();
    }
    // Single symbol or no turnover spread → no meaningful rank; default
    // to "fully liquid" so confidence weighting is a no-op.
    let mut sorted: Vec<f64> = turnovers.iter().map(|(_, t)| *t).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let no_spread = n < 2 || sorted.first() == sorted.last();
    if no_spread {
        return turnovers.into_iter().map(|(s, _)| (s, 1.0)).collect();
    }
    let denom = (n - 1) as f64;
    turnovers
        .into_iter()
        .map(|(sym, t)| {
            let rank = sorted.partition_point(|x| *x < t) as f64;
            (sym, (rank / denom).clamp(0.0, 1.0))
        })
        .collect()
}

/// Pure function: compute the symbol's current emergence score from its
/// sub-KG NodeId state vector. Mean of four [0, 1] components — empty
/// kg ⇒ score 0, fully saturated kg ⇒ score 1:
///
///   1. KlSurpriseMagnitude                 — V4 self-referential surprise
///   2. (1 − BeliefEntropy) × BeliefSampleCount — data-weighted state
///      concentration (no samples ⇒ no concentration claim)
///   3. WlAnalogConfidence                  — historical analog support
///   4. intent_concentration                — (max(intent) − 0.2) / 0.8
///
/// All four are pure sub-KG node reads; no rules, no thresholds. The
/// returned score is in [0, 1]; the direction is the sign of
/// KlSurpriseDirection (-1 / 0 / +1).
pub fn compute_emergence_score(kg: &SymbolSubKG) -> (f64, [f64; 5], i8) {
    fn read(kg: &SymbolSubKG, id: &NodeId) -> f64 {
        kg.nodes
            .get(id)
            .and_then(|n| n.value)
            .map(|v| v.to_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
    let kl_mag = read(kg, &NodeId::KlSurpriseMagnitude).clamp(0.0, 1.0);
    let entropy = read(kg, &NodeId::BeliefEntropy).clamp(0.0, 1.0);
    let entropy_inv = (1.0 - entropy).clamp(0.0, 1.0);
    let sample_count = read(kg, &NodeId::BeliefSampleCount).clamp(0.0, 1.0);
    // Data-weighted concentration: zero samples means we have no basis
    // to claim the categorical posterior is concentrated, so the
    // contribution must vanish.
    let concentration = (entropy_inv * sample_count).clamp(0.0, 1.0);
    let wl_conf = read(kg, &NodeId::WlAnalogConfidence).clamp(0.0, 1.0);

    let intent_max = [
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
        NodeId::IntentRotation,
        NodeId::IntentVolatility,
    ]
    .iter()
    .map(|id| read(kg, id))
    .fold(0.0_f64, f64::max);
    // intent_max is the max of 4 mass values that sum to ≤ 1; uniform
    // distribution across 5 variants (incl. unknown) ≈ 0.2 each. Anything
    // above 0.2 marks growing concentration; above 0.8 = full point mass.
    // Linear normalization to [0, 1].
    let intent_concentration = ((intent_max - 0.2) / 0.8).clamp(0.0, 1.0);

    // Components stored as a 5-element array for backward-compat /
    // observability — slot 1 holds entropy_inv (raw, unweighted) and
    // slot 2 holds sample_count (raw) so the operator can debug each
    // input. The score itself uses the data-weighted product.
    let components = [
        kl_mag,
        entropy_inv,
        sample_count,
        wl_conf,
        intent_concentration,
    ];
    let score_components = [kl_mag, concentration, wl_conf, intent_concentration];
    let score = score_components.iter().sum::<f64>() / score_components.len() as f64;

    let direction_raw = read(kg, &NodeId::KlSurpriseDirection);
    let direction = if direction_raw > 0.5 {
        1_i8
    } else if direction_raw < -0.5 {
        -1_i8
    } else {
        0_i8
    };

    (score, components, direction)
}

/// V5.3: small directional separation epsilon. If `|p_bull - p_bear| < EPSILON`
/// the BP posterior is effectively undecided and we should not let an
/// emerge:* setup fire as either direction. Statistical first principle —
/// 1e-3 is just numerical noise floor, not a behavioural threshold.
pub const BP_DIRECTION_EPSILON: f64 = 1e-3;

/// V5.3: reconcile setup.direction with BP posterior for every emerge:*
/// setup. Run AFTER `apply_posterior_confidence`. If the BP posterior
/// agrees with the emergence-derived direction (or is undecided), the
/// setup keeps its current direction; if it disagrees the direction
/// flips. If the posterior is undecided the setup's action is forced to
/// Observe so action_promotion can't promote a directionally-unsure
/// setup. Returns count of setups touched.
pub fn reconcile_direction_with_bp(
    setups: &mut [TacticalSetup],
    beliefs: &std::collections::HashMap<String, [f64; 3]>,
) -> usize {
    use crate::pipeline::loopy_bp::{STATE_BEAR, STATE_BULL};
    let mut touched = 0;
    for setup in setups.iter_mut() {
        if !setup.setup_id.starts_with("emerge:") {
            continue;
        }
        let symbol = match &setup.scope {
            ReasoningScope::Symbol(s) => s.0.clone(),
            _ => continue,
        };
        let Some(post) = beliefs.get(&symbol) else {
            continue;
        };
        let p_bull = post[STATE_BULL];
        let p_bear = post[STATE_BEAR];
        if (p_bull - p_bear).abs() < BP_DIRECTION_EPSILON {
            // Posterior undecided — don't trust either direction.
            setup.action = TacticalAction::Observe;
            setup.risk_notes.push(format!(
                "bp_undecided: p_bull={p_bull:.3} p_bear={p_bear:.3}"
            ));
            touched += 1;
            continue;
        }
        let new_direction = if p_bull > p_bear {
            TacticalDirection::Long
        } else {
            TacticalDirection::Short
        };
        if setup.direction != Some(new_direction) {
            setup.risk_notes.push(format!(
                "bp_direction_override: {:?} → {:?} (p_bull={:.3} p_bear={:.3})",
                setup.direction, new_direction, p_bull, p_bear
            ));
            setup.direction = Some(new_direction);
            touched += 1;
        }
    }
    touched
}

/// Synthesize a TacticalSetup from an emergence event. Setup id is
/// `emerge:SYM:tick` so the action_promotion dispatcher and the
/// operator can immediately tell vortex-derived from sub-KG-derived
/// setups. Direction comes from KlSurpriseDirection (defaulting to Long
/// when neutral); confidence is the raw emergence score (BP posterior
/// will overwrite it later via apply_posterior_confidence).
pub fn synthesize_setup_from_emergence(e: &SubKgEmergence) -> TacticalSetup {
    let direction = match e.direction {
        d if d < 0 => Some(TacticalDirection::Short),
        _ => Some(TacticalDirection::Long), // 0 / +1 → Long default
    };
    let confidence = Decimal::try_from(e.score.clamp(0.0, 1.0)).unwrap_or(Decimal::ZERO);
    let provenance =
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
    let title = format!(
        "{} {} (sub-KG emergence)",
        match direction {
            Some(TacticalDirection::Short) => "Short",
            _ => "Long",
        },
        e.symbol
    );
    let mut risk_notes = Vec::with_capacity(2);
    risk_notes.push(format!(
        "sub_kg_emergence: z={:.2} score={:.3} dir={}",
        e.z, e.score, e.direction
    ));
    risk_notes.push(format!(
        "emergence_components: kl_mag={:.2} ent_inv={:.2} samples={:.2} wl={:.2} intent={:.2}",
        e.components[0], e.components[1], e.components[2], e.components[3], e.components[4]
    ));
    TacticalSetup {
        setup_id: format!("emerge:{}:{}", e.symbol, e.tick),
        hypothesis_id: format!("hyp:emergence:{}", e.symbol),
        runner_up_hypothesis_id: None,
        provenance,
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(Symbol(e.symbol.clone())),
        title,
        action: TacticalAction::Observe, // dispatcher upgrades
        direction,
        horizon: default_case_horizon(),
        confidence,
        confidence_gap: Decimal::ZERO,
        heuristic_edge: Decimal::ZERO,
        convergence_score: None,
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: format!("sub-KG emergence z={:.2}", e.z),
        causal_narrative: Some(format!(
            "Sub-KG state vector synchronously transitioned (z={:.2}σ above own baseline). \
             KL surprise={:.2}, intent concentration={:.2}, belief informed={:.2}.",
            e.z, e.components[0], e.components[4], e.components[2]
        )),
        risk_notes,
        review_reason_code: None,
        policy_verdict: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn empty_kg(symbol: &str) -> SymbolSubKG {
        SymbolSubKG::new_empty(symbol.into(), Utc::now())
    }

    #[test]
    fn emergence_score_zero_for_empty_subkg() {
        let kg = empty_kg("X.US");
        let (score, components, dir) = compute_emergence_score(&kg);
        // Score must be 0 — empty kg has no signal.
        assert_eq!(score, 0.0);
        assert_eq!(dir, 0);
        // Components vector keeps raw entropy_inv = 1 (entropy default 0)
        // and raw sample_count = 0 for observability; the data-weighted
        // product (entropy_inv × sample_count) used in the score is 0.
        assert_eq!(components[0], 0.0); // kl_mag
        assert_eq!(components[2], 0.0); // sample_count
        assert_eq!(components[3], 0.0); // wl_conf
        assert_eq!(components[4], 0.0); // intent_concentration
    }

    #[test]
    fn emergence_score_max_with_full_signals() {
        let mut kg = empty_kg("X.US");
        let ts = Utc::now();
        // Saturate every component
        kg.set_node_value(NodeId::KlSurpriseMagnitude, dec!(1.0), ts);
        kg.set_node_value(NodeId::BeliefEntropy, dec!(0.0), ts); // entropy_inv = 1
        kg.set_node_value(NodeId::BeliefSampleCount, dec!(1.0), ts);
        kg.set_node_value(NodeId::WlAnalogConfidence, dec!(1.0), ts);
        kg.set_node_value(NodeId::IntentAccumulation, dec!(1.0), ts);
        kg.set_node_value(NodeId::KlSurpriseDirection, dec!(1.0), ts);
        let (score, _, dir) = compute_emergence_score(&kg);
        assert!(
            (score - 1.0).abs() < 1e-6,
            "expected score ≈ 1, got {}",
            score
        );
        assert_eq!(dir, 1);
    }

    #[test]
    fn emergence_direction_signs_from_kl_dir() {
        let mut kg = empty_kg("X.US");
        let ts = Utc::now();
        kg.set_node_value(NodeId::KlSurpriseDirection, dec!(-1.0), ts);
        let (_, _, dir) = compute_emergence_score(&kg);
        assert_eq!(dir, -1);
        kg.set_node_value(NodeId::KlSurpriseDirection, dec!(0.0), ts);
        let (_, _, dir) = compute_emergence_score(&kg);
        assert_eq!(dir, 0);
    }

    #[test]
    fn tracker_bootstrap_returns_no_events() {
        let mut tracker = SubKgEmergenceTracker::new();
        let mut registry = SubKgRegistry::default();
        registry.upsert("X.US", Utc::now());
        // First call observes but cannot z-test (no baseline yet).
        let events = tracker.detect_emergences(&registry, &HashMap::new(), None);
        assert!(events.is_empty());
    }

    #[test]
    fn tracker_emits_only_on_self_referential_z_spike() {
        let mut tracker = SubKgEmergenceTracker::new();
        let mut registry = SubKgRegistry::default();
        let ts = Utc::now();
        // V5.1: high centrality so attention budget processes every tick.
        let centrality: HashMap<String, f64> = std::iter::once(("X.US".to_string(), 1.0)).collect();
        // Build a long quiet baseline at score ≈ 0.05 (mostly zeros).
        for tick in 0..50 {
            let kg = registry.upsert("X.US", ts);
            kg.set_tick(tick, ts);
            // Set tiny WL confidence to avoid all-zero score (which would
            // make std=0 and disable z-test).
            let v = if tick % 2 == 0 {
                dec!(0.04)
            } else {
                dec!(0.06)
            };
            kg.set_node_value(NodeId::WlAnalogConfidence, v, ts);
            let _ = tracker.detect_emergences(&registry, &centrality, None);
        }
        // Now slam every component to saturation — score jumps to ≈ 1.0
        let kg = registry.upsert("X.US", ts);
        kg.set_tick(51, ts);
        kg.set_node_value(NodeId::KlSurpriseMagnitude, dec!(1.0), ts);
        kg.set_node_value(NodeId::BeliefEntropy, dec!(0.0), ts);
        kg.set_node_value(NodeId::BeliefSampleCount, dec!(1.0), ts);
        kg.set_node_value(NodeId::WlAnalogConfidence, dec!(1.0), ts);
        kg.set_node_value(NodeId::IntentAccumulation, dec!(1.0), ts);
        kg.set_node_value(NodeId::KlSurpriseDirection, dec!(1.0), ts);
        let events = tracker.detect_emergences(&registry, &centrality, None);
        assert!(
            !events.is_empty(),
            "shock should trigger an emergence event"
        );
        let e = events
            .iter()
            .find(|e| e.symbol == "X.US")
            .expect("X.US present");
        assert!(e.z >= EMERGENCE_EMIT_Z, "z={} < emit floor", e.z);
        assert!(e.direction > 0);
    }

    #[test]
    fn synthesize_setup_carries_emergence_metadata() {
        let e = SubKgEmergence {
            symbol: "AFRM.US".to_string(),
            tick: 42,
            ts: Utc::now(),
            z: 1.99,
            score: 0.78,
            direction: 1,
            components: [0.76, 0.62, 0.81, 0.30, 0.55],
        };
        let setup = synthesize_setup_from_emergence(&e);
        assert_eq!(setup.setup_id, "emerge:AFRM.US:42");
        assert_eq!(setup.hypothesis_id, "hyp:emergence:AFRM.US");
        assert!(matches!(setup.scope, ReasoningScope::Symbol(_)));
        assert!(matches!(setup.direction, Some(TacticalDirection::Long)));
        assert_eq!(setup.action, TacticalAction::Observe);
        assert!(setup.causal_narrative.is_some());
        assert!(setup
            .risk_notes
            .iter()
            .any(|n| n.starts_with("sub_kg_emergence:")));
    }

    #[test]
    fn liquidity_confidence_ranks_cross_symbol_turnover() {
        let mut registry = SubKgRegistry::default();
        let ts = Utc::now();
        // Three symbols with very different turnover.
        let kg = registry.upsert("LOW.HK", ts);
        kg.set_node_value(NodeId::Turnover, dec!(100), ts);
        let kg = registry.upsert("MID.HK", ts);
        kg.set_node_value(NodeId::Turnover, dec!(1_000_000), ts);
        let kg = registry.upsert("HIGH.HK", ts);
        kg.set_node_value(NodeId::Turnover, dec!(1_000_000_000), ts);

        let lc = compute_liquidity_confidence(&registry);
        assert!(
            (lc["LOW.HK"] - 0.0).abs() < 1e-9,
            "lowest turnover should have rank 0, got {}",
            lc["LOW.HK"]
        );
        assert!(
            (lc["HIGH.HK"] - 1.0).abs() < 1e-9,
            "highest turnover should have rank 1, got {}",
            lc["HIGH.HK"]
        );
        assert!(lc["MID.HK"] > 0.0 && lc["MID.HK"] < 1.0);
    }

    #[test]
    fn liquidity_confidence_attenuates_emergence_for_illiquid() {
        let mut tracker = SubKgEmergenceTracker::new();
        let mut registry = SubKgRegistry::default();
        let ts = Utc::now();
        let centrality: HashMap<String, f64> = [
            ("ILLIQUID.HK".to_string(), 1.0),
            ("LIQUID.HK".to_string(), 1.0),
        ]
        .into_iter()
        .collect();
        // Build baselines with alternating WL value so variance > 0
        // (otherwise std = 0 and surprise_z always returns None).
        for tick in 0..30 {
            let wl = if tick % 2 == 0 {
                dec!(0.04)
            } else {
                dec!(0.06)
            };
            for sym in ["ILLIQUID.HK", "LIQUID.HK"] {
                let kg = registry.upsert(sym, ts);
                kg.set_tick(tick, ts);
                kg.set_node_value(NodeId::WlAnalogConfidence, wl, ts);
            }
            let kg = registry.upsert("ILLIQUID.HK", ts);
            kg.set_node_value(NodeId::Turnover, dec!(100), ts);
            let kg = registry.upsert("LIQUID.HK", ts);
            kg.set_node_value(NodeId::Turnover, dec!(1_000_000_000), ts);
            let _ = tracker.detect_emergences(&registry, &centrality, None);
        }
        // Shock both symbols identically.
        for sym in ["ILLIQUID.HK", "LIQUID.HK"] {
            let kg = registry.upsert(sym, ts);
            kg.set_tick(31, ts);
            kg.set_node_value(NodeId::KlSurpriseMagnitude, dec!(1.0), ts);
            kg.set_node_value(NodeId::BeliefEntropy, dec!(0.0), ts);
            kg.set_node_value(NodeId::BeliefSampleCount, dec!(1.0), ts);
            kg.set_node_value(NodeId::IntentAccumulation, dec!(1.0), ts);
            kg.set_node_value(NodeId::KlSurpriseDirection, dec!(1.0), ts);
        }
        let events = tracker.detect_emergences(&registry, &centrality, None);
        // Liquid symbol's z should clear the emit floor; illiquid's
        // weighted score is multiplied by 0 → no z spike.
        let illiquid = events.iter().find(|e| e.symbol == "ILLIQUID.HK");
        let liquid = events.iter().find(|e| e.symbol == "LIQUID.HK");
        assert!(
            liquid.is_some(),
            "liquid symbol should emit emergence on shock"
        );
        assert!(
            illiquid.is_none(),
            "illiquid symbol should NOT emit (liquidity_confidence ≈ 0)"
        );
    }

    #[test]
    fn reconcile_direction_flips_when_bp_disagrees() {
        // Build an emerge:* setup with KL direction Long, but BP says
        // p_bear > p_bull → direction should flip to Short.
        let e = SubKgEmergence {
            symbol: "X.HK".to_string(),
            tick: 1,
            ts: Utc::now(),
            z: 1.5,
            score: 0.6,
            direction: 1,
            components: [0.5; 5],
        };
        let setup = synthesize_setup_from_emergence(&e);
        assert_eq!(setup.direction, Some(TacticalDirection::Long));

        let mut setups = vec![setup.clone()];
        let mut beliefs: HashMap<String, [f64; 3]> = HashMap::new();
        beliefs.insert("X.HK".to_string(), [0.2, 0.7, 0.1]); // bear-dominant
        let touched = reconcile_direction_with_bp(&mut setups, &beliefs);
        assert_eq!(touched, 1);
        assert_eq!(setups[0].direction, Some(TacticalDirection::Short));
    }

    #[test]
    fn reconcile_direction_observes_when_bp_undecided() {
        let e = SubKgEmergence {
            symbol: "X.HK".to_string(),
            tick: 1,
            ts: Utc::now(),
            z: 1.5,
            score: 0.6,
            direction: 1,
            components: [0.5; 5],
        };
        let setup = synthesize_setup_from_emergence(&e);
        let mut setups = vec![setup];
        let mut beliefs: HashMap<String, [f64; 3]> = HashMap::new();
        // p_bull == p_bear within epsilon → undecided
        beliefs.insert("X.HK".to_string(), [0.4, 0.4, 0.2]);
        let touched = reconcile_direction_with_bp(&mut setups, &beliefs);
        assert_eq!(touched, 1);
        assert_eq!(setups[0].action, TacticalAction::Observe);
    }

    #[test]
    fn reconcile_direction_skips_non_emerge_setups() {
        // Vortex-derived setup (no emerge: prefix) — should NOT be touched.
        let setup = TacticalSetup {
            setup_id: "vortex:X.HK:1".to_string(),
            hypothesis_id: "hyp:vortex:X.HK".to_string(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("X.HK".into())),
            title: "vortex".into(),
            action: TacticalAction::Observe,
            direction: Some(TacticalDirection::Long),
            horizon: default_case_horizon(),
            confidence: rust_decimal::Decimal::ZERO,
            confidence_gap: rust_decimal::Decimal::ZERO,
            heuristic_edge: rust_decimal::Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: Vec::new(),
            review_reason_code: None,
            policy_verdict: None,
        };
        let original_dir = setup.direction;
        let mut setups = vec![setup.clone()];
        let mut beliefs: HashMap<String, [f64; 3]> = HashMap::new();
        beliefs.insert("X.HK".to_string(), [0.1, 0.8, 0.1]); // bear, would flip if applied
        let touched = reconcile_direction_with_bp(&mut setups, &beliefs);
        assert_eq!(touched, 0);
        assert_eq!(setups[0].direction, original_dir);
    }

    #[test]
    fn synthesize_setup_picks_short_for_negative_direction() {
        let e = SubKgEmergence {
            symbol: "ARM.US".to_string(),
            tick: 7,
            ts: Utc::now(),
            z: 1.5,
            score: 0.6,
            direction: -1,
            components: [0.5, 0.5, 0.7, 0.4, 0.6],
        };
        let setup = synthesize_setup_from_emergence(&e);
        assert!(matches!(setup.direction, Some(TacticalDirection::Short)));
    }
}
