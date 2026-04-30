//! Loopy Belief Propagation on Eden's master KG.
//!
//! The structural foundation Eden has been missing: given partial
//! observation on some symbols (their Pressure + Intent state), compute
//! the marginal posterior distribution over EVERY symbol's state via
//! Pearl-style sum-product message passing on the typed KG.
//!
//! This is the math behind "局部推全局":
//!   - We observe ~10% of symbols clearly (the active ones with strong
//!     Pressure + Intent signal)
//!   - 90% of symbols are LowInformation per state_engine
//!   - Naive answer: ignore the 90%
//!   - BP answer: propagate the 10%'s beliefs along graph edges,
//!     converge to consistent marginals over the 100%
//!
//! Each symbol has a discrete categorical state ∈ {Bull, Bear, Neutral}.
//! State derived from Pressure + Intent (no rules I invented — pure
//! sign + magnitude of existing Eden signals). Edge potential biases
//! toward neighbor agreement, with strength = master KG StockToStock
//! similarity weight.
//!
//! Pure deterministic iteration. Converges to fixed point on tree
//! topology (gives exact marginals); approximate on loopy graphs
//! (well-known stable property of LBP). No learning, no training.
//!
//! Output: `.run/eden-bp-marginals-{market}.ndjson` — per symbol per
//! snapshot, the converged 3-vector posterior.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::core::market::MarketRegistry;
use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};
use crate::ontology::reasoning::{
    direction_from_setup, ReasoningScope, TacticalDirection, TacticalSetup,
};
use crate::pipeline::lead_lag_index::LeadLagEvent;
use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};

/// 3-state discrete posterior over Bull / Bear / Neutral.
pub const N_STATES: usize = 3;
pub const STATE_BULL: usize = 0;
pub const STATE_BEAR: usize = 1;
pub const STATE_NEUTRAL: usize = 2;

/// Number of LBP iterations. Typical convergence on sparse graphs
/// happens in 5-15 iterations; cap higher for safety.
pub const MAX_ITERATIONS: usize = 20;

/// Convergence threshold — when max |Δbelief| across all symbols falls
/// below this between iterations, stop early.
pub const CONVERGENCE_TOL: f64 = 1e-3;

/// Edge potential parameter — controls neighbor-agreement strength.
/// 0.0 = no influence (BP useless), 1.0 = total agreement (trivial
/// uniform copying). 0.25 keeps the per-edge agreement-vs-disagreement
/// ratio at ~1.38×, low enough that compounding over a 31-degree dense
/// master KG (post task #110 sparse fix) doesn't drive posteriors to
/// 0.99999 saturation, but high enough that single-neighbor pull on a
/// sparse graph still moves an unobserved node ~5% off uniform. The
/// original 0.7 was calibrated for sparser graphs and pushed posteriors
/// to extremes once density grew.
const EDGE_AGREEMENT_STRENGTH: f64 = 0.25;

/// Message damping for non-tree (cyclic) BP. Each iteration blends the
/// freshly-computed message with the previous iteration's value:
/// `m_new = damping * m_computed + (1 - damping) * m_prev`. Standard
/// literature treatment to prevent runaway oscillation/saturation on
/// dense cyclic graphs (Murphy/Weiss/Jordan 1999, Heskes 2003).
/// 0.3 is on the gentler end of the typical 0.2–0.5 range — combined
/// with α=0.15 it keeps dense-graph posteriors calibrated while still
/// letting sparse-graph signals propagate (~40% Bull mass after one
/// observed-Bull neighbor on a 2-node graph, matching prior behaviour
/// without the saturation pathology).
const MESSAGE_DAMPING: f64 = 0.3;

/// Smoothing for normalization: prevents zero beliefs from killing
/// downstream multiplications. Pure numerical hygiene.
const EPSILON: f64 = 1e-9;

/// Below this prior magnitude the symbol is treated as unobserved
/// (uniform prior). Algorithmic floor — not a business threshold.
pub const PRIOR_MAGNITUDE_FLOOR: f64 = 0.05;

/// Algorithmic stability bounds for directional message weights.
const MIN_MESSAGE_WEIGHT: f64 = 0.1;
const MAX_MESSAGE_WEIGHT: f64 = 2.0;
pub const BP_PRUNING_SHADOW_WEIGHT_FLOOR: f64 = 0.2;

/// One symbol's prior + observed evidence.
#[derive(Debug, Clone)]
pub struct NodePrior {
    pub belief: [f64; N_STATES],
    /// True when we have actual observation; false when prior is uniform
    /// (= unobserved). Unobserved nodes participate in message passing
    /// but don't anchor the field.
    pub observed: bool,
}

impl Default for NodePrior {
    fn default() -> Self {
        // Uninformed prior = uniform.
        Self {
            belief: [1.0 / N_STATES as f64; N_STATES],
            observed: false,
        }
    }
}

/// Edge between two symbols carrying StockToStock similarity weight.
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub kind: BpEdgeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BpInputEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    pub edge_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BpEdgeKind {
    StockToStock,
    PeerSimilarity,
    CrossMarket,
    Unknown,
}

impl BpEdgeKind {
    pub fn from_edge_type(edge_type: &str) -> Self {
        match edge_type {
            "StockToStock" => Self::StockToStock,
            "PeerSimilarity" => Self::PeerSimilarity,
            "CrossMarket" => Self::CrossMarket,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BpPruningShadowSummary {
    pub total_edges: usize,
    pub observed_priors: usize,
    pub observed_incident_edges: usize,
    pub low_weight_edges: usize,
    pub shadow_retained_edges: usize,
    pub shadow_pruned_edges: usize,
    pub stock_to_stock_edges: usize,
    pub peer_similarity_edges: usize,
    pub cross_market_edges: usize,
    pub unknown_edges: usize,
    pub weight_floor: f64,
    pub weight_mean: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginalRow {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    pub iterations: usize,
    pub converged: bool,
    pub p_bull: f64,
    pub p_bear: f64,
    pub p_neutral: f64,
    /// Was this node observed (anchored) or fully inferred from
    /// neighbors via BP?
    pub observed: bool,
}

#[derive(Debug, Clone)]
pub struct BpRunResult {
    pub beliefs: HashMap<String, [f64; N_STATES]>,
    pub messages: HashMap<(String, String), [f64; N_STATES]>,
    pub iterations: usize,
    pub converged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BpTraceKind {
    Prior,
    Message,
    Posterior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BpMessageTraceRow {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub kind: BpTraceKind,
    pub iterations: usize,
    pub converged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_weight: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<bool>,
    pub p_bull: f64,
    pub p_bear: f64,
    pub p_neutral: f64,
}

/// Extract symbol prior directly from sub-KG. Single entry point —
/// ALL evidence (Pressure + Intent + Memory + Belief + Causal) flows
/// through `NodeId` value reads. Per v2 plan: there is no
/// `EvidenceContext` shadow path — sub-KG is the only substrate.
///
/// Direction (signed):
///   pcf + 0.5·pm + acc − dist          (existing Pressure + Intent)
/// + outcome_memory + engram_alignment  (Memory NodeKind, signed [-1,1])
///
/// Concentration (unsigned [0,1]):
///   |direction| / 2.0                  (saturate, normalize)
/// × evidence_strength                  (mean of WL + sample_count + forecast_acc)
/// × (1 − belief_entropy)               (high entropy ⇒ pull toward uniform)
///
/// Below `PRIOR_MAGNITUDE_FLOOR` the symbol is treated as unobserved
/// (uniform prior) — algorithmic stability, not a business threshold.
pub fn observe_from_subkg(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG) -> NodePrior {
    fn read(kg: &crate::pipeline::symbol_sub_kg::SymbolSubKG, id: &NodeId) -> f64 {
        kg.nodes
            .get(id)
            .and_then(|n| n.value)
            .map(|v| v.to_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
    // Pressure + Intent (existing)
    let pcf = read(kg, &NodeId::PressureCapitalFlow);
    let pm = read(kg, &NodeId::PressureMomentum);
    let acc = read(kg, &NodeId::IntentAccumulation);
    let dist = read(kg, &NodeId::IntentDistribution);
    // Memory + Belief + Causal (Phase 1 NodeKinds)
    let outcome_memory = read(kg, &NodeId::OutcomeMemory);
    let engram_alignment = read(kg, &NodeId::EngramAlignment);
    let wl_confidence = read(kg, &NodeId::WlAnalogConfidence);
    let belief_entropy = read(kg, &NodeId::BeliefEntropy);
    let sample_count = read(kg, &NodeId::BeliefSampleCount);
    let forecast_acc = read(kg, &NodeId::ForecastAccuracy);
    // V3.2 cross-ontology: parent sector intent (Accumulation - Distribution)
    // becomes a signed direction contribution. Symbols sharing a sector
    // get the same pair so BP propagates a sector-aware prior even when
    // direct symbol evidence is weak.
    let sector_bull = read(kg, &NodeId::SectorIntentBull);
    let sector_bear = read(kg, &NodeId::SectorIntentBear);
    // V4 Surprise NodeKind: self-referential KL surprise signals.
    // Magnitude ∈ [0,1] saturates with z-score; direction ∈ [-1,1] is
    // the sign of the dominant channel mean shift. Both flow as evidence —
    // direction adds (kl_dir × kl_mag) to direction_raw, magnitude adds
    // to evidence boost. No gating, no thresholds.
    let kl_mag = read(kg, &NodeId::KlSurpriseMagnitude);
    let kl_dir = read(kg, &NodeId::KlSurpriseDirection);
    // V5 Option NodeKind (US only — HK uses warrants instead). Three
    // signals fold into the direction term:
    //   - put_call_oi_ratio: bullish when < 1, bearish when > 1.
    //     Mapped via -tanh(ln(ratio)) so ratio=1 contributes 0, ratio→0
    //     saturates +1, ratio→∞ saturates −1.
    //   - put_call_skew: (put_iv − call_iv) / call_iv. Positive = fear
    //     smirk → bearish. Mapped via -tanh(skew * 5).
    //   - atm_call_iv − atm_put_iv: rare bullish term when call IV > put IV.
    //     Small contribution, bounded.
    // ATM IV magnitude (call+put / 2) feeds evidence_boost — the BP
    // posterior should sharpen when option market expresses opinion at
    // all (high IV = market is pricing event), not just when direction
    // agrees.
    let pcr_oi = read(kg, &NodeId::OptionPutCallOiRatio);
    let opt_skew = read(kg, &NodeId::OptionPutCallSkew);
    let atm_call_iv = read(kg, &NodeId::OptionAtmCallIv);
    let atm_put_iv = read(kg, &NodeId::OptionAtmPutIv);
    let pcr_signal = if pcr_oi > 1e-6 {
        -((pcr_oi).ln()).tanh()
    } else {
        0.0
    };
    let skew_signal = -(opt_skew * 5.0).tanh();
    let iv_diff_signal = (atm_call_iv - atm_put_iv).clamp(-0.5, 0.5);
    let option_direction = pcr_signal + skew_signal + iv_diff_signal;
    // ATM IV magnitude → [0, 1] evidence component. Typical equity ATM
    // IV is 0.15-0.50; map (call_iv + put_iv) / 2 / 0.40 with a soft
    // saturation so values above 0.40 contribute strongly but capped
    // at 1.0.
    let iv_avg = if atm_call_iv > 0.0 && atm_put_iv > 0.0 {
        (atm_call_iv + atm_put_iv) / 2.0
    } else {
        0.0
    };
    let iv_evidence = (iv_avg / 0.40).clamp(0.0, 1.0);

    // Direction: combined signed signal. Pressure+Intent (existing) +
    // outcome_memory + engram_alignment (Memory NodeKind, signed) +
    // sector_intent (Sector NodeKind, signed via bull − bear) +
    // KL surprise (Surprise NodeKind, signed via direction × magnitude) +
    // option_direction (Option NodeKind, US-only — see above).
    let direction_raw = pcf + 0.5 * pm + acc - dist
        + outcome_memory
        + engram_alignment
        + (sector_bull - sector_bear)
        + (kl_dir * kl_mag)
        + option_direction;
    let base_magnitude = (direction_raw.abs() / 2.0).min(1.0);

    // Evidence uplift: WL recurrence + belief sample density + forecast
    // accuracy + KL surprise magnitude + option ATM IV (each [0,1]) —
    // additive boost toward saturation, NOT a gate. Pure Pressure+Intent
    // alone still anchors the prior; new NodeKind values just sharpen it.
    let evidence_boost =
        (wl_confidence + sample_count + forecast_acc + kl_mag + iv_evidence) / 5.0;

    // Belief entropy dampens (high entropy = Eden uncertain about its
    // own belief = pull toward uniform). Halving max effect keeps
    // missing-data default (entropy = 0 from builder) inert.
    let entropy_dampener = (1.0 - 0.5 * belief_entropy).clamp(0.0, 1.0);

    let concentration =
        (base_magnitude + evidence_boost * (1.0 - base_magnitude)) * entropy_dampener;

    if concentration < PRIOR_MAGNITUDE_FLOOR {
        // Too weak to call — uniform / unobserved. BP still passes
        // messages through this node, but the prior doesn't anchor.
        return NodePrior::default();
    }

    let dominant_idx = if direction_raw > 0.0 {
        STATE_BULL
    } else {
        STATE_BEAR
    };
    let mut belief = [0.0; N_STATES];
    let dominant_mass = (1.0 + concentration) / (N_STATES as f64 + concentration);
    let rest_mass = (1.0 - dominant_mass) / (N_STATES - 1) as f64;
    for i in 0..N_STATES {
        belief[i] = if i == dominant_idx {
            dominant_mass
        } else {
            rest_mass
        };
    }
    NodePrior {
        belief,
        observed: true,
    }
}

/// Sub-tick prior derivation for the event-driven path. Builds a
/// `NodePrior` from a subset of pressure channels (whichever channels
/// have been wired so far). This intentionally does NOT match
/// `observe_from_subkg` exactly — that one fuses Memory/Belief/Sector/KL
/// signals which only update at tick boundary. The sub-tick prior is
/// pressure-only and acts as a delta on top of the latest tick-bound
/// belief; observe_symbol's residual queue propagates the difference.
///
/// Channels are passed as `Option<f64>` so the aggregator can pass
/// `None` for channels not yet wired event-driven. Missing channels
/// contribute zero to direction_raw, weakening (but not biasing) the
/// prior.
pub fn prior_from_pressure_channels(
    order_book: Option<f64>,
    capital_flow: Option<f64>,
    institutional: Option<f64>,
    momentum: Option<f64>,
    volume: Option<f64>,
    structure: Option<f64>,
) -> NodePrior {
    let ob = order_book.unwrap_or(0.0);
    let cf = capital_flow.unwrap_or(0.0);
    let inst = institutional.unwrap_or(0.0);
    let mo = momentum.unwrap_or(0.0);
    let vol = volume.unwrap_or(0.0);
    let st = structure.unwrap_or(0.0);

    // Direction signal: dimensions agree → reinforce; disagree → cancel.
    // Coefficients echo observe_from_subkg's pcf + 0.5*pm weighting; the
    // remaining channels are added as supportive evidence with smaller
    // weights so the dominant tick-bound prior (CapitalFlow/Momentum-led)
    // continues to drive the magnitude when those are wired.
    //
    // 2026-04-30: vol channel removed from direction_raw — it computes
    // (volume / ema_volume) - 1, which is a SIZE anomaly, NOT a
    // directional signal. Including it as 0.2*vol biased priors toward
    // bear whenever a small odd-lot print happened to be the most
    // recent trade. cf already encodes signed volume direction.
    let _ = vol; // vol is a size anomaly, not directional; retained in signature for back-compat
    let direction_raw = cf + 0.5 * mo + 0.3 * ob + 0.3 * inst + 0.2 * st;
    let base_magnitude = (direction_raw.abs() / 2.0).min(1.0);
    if base_magnitude < PRIOR_MAGNITUDE_FLOOR {
        return NodePrior::default();
    }
    let dominant_idx = if direction_raw > 0.0 {
        STATE_BULL
    } else {
        STATE_BEAR
    };
    let mut belief = [0.0; N_STATES];
    let dominant_mass = (1.0 + base_magnitude) / (N_STATES as f64 + base_magnitude);
    let rest_mass = (1.0 - dominant_mass) / (N_STATES - 1) as f64;
    for i in 0..N_STATES {
        belief[i] = if i == dominant_idx {
            dominant_mass
        } else {
            rest_mass
        };
    }
    NodePrior {
        belief,
        observed: true,
    }
}

/// Edge potential: phi(x_i, x_j) = if x_i == x_j { 1 + α·w } else { 1 - α·w/2 }.
/// Pure numerical bias toward agreement scaled by similarity weight.
pub(crate) fn edge_potential_value(weight: f64, x_i: usize, x_j: usize) -> f64 {
    edge_potential(weight, x_i, x_j)
}

fn edge_potential(weight: f64, x_i: usize, x_j: usize) -> f64 {
    let aw = EDGE_AGREEMENT_STRENGTH * weight;
    if x_i == x_j {
        1.0 + aw
    } else {
        (1.0 - aw / 2.0).max(EPSILON)
    }
}

fn normalize(b: &mut [f64; N_STATES]) {
    let s: f64 = b.iter().sum::<f64>();
    if s < EPSILON {
        // Long products of small messages can underflow every cell to
        // exactly 0.0; sum/EPSILON would leave the array as [0,0,0].
        // Fall back to uniform — posterior is uninformative, not
        // "believes nothing."
        let uniform = 1.0 / N_STATES as f64;
        for v in b.iter_mut() {
            *v = uniform;
        }
    } else {
        for v in b.iter_mut() {
            *v /= s;
        }
    }
}

fn max_diff(a: &[f64; N_STATES], b: &[f64; N_STATES]) -> f64 {
    let mut m = 0.0_f64;
    for i in 0..N_STATES {
        m = m.max((a[i] - b[i]).abs());
    }
    m
}

fn directed_edge_weights(edges: &[GraphEdge]) -> HashMap<(String, String), f64> {
    let mut directed_edges: HashMap<(String, String), f64> = HashMap::new();
    for e in edges {
        directed_edges.insert((e.from.clone(), e.to.clone()), e.weight);
    }
    for e in edges {
        let reverse = (e.to.clone(), e.from.clone());
        if !directed_edges.contains_key(&reverse) {
            directed_edges.insert(reverse, e.weight);
        }
    }
    directed_edges
}

/// Run loopy belief propagation. Returns per-symbol final beliefs +
/// metadata (iterations consumed, whether converged).
pub fn run(
    priors: &HashMap<String, NodePrior>,
    edges: &[GraphEdge],
) -> (HashMap<String, [f64; N_STATES]>, usize, bool) {
    let result = run_with_messages(priors, edges);
    (result.beliefs, result.iterations, result.converged)
}

/// Run loopy BP and retain final directed messages for visual
/// inspection. Same sum-product algorithm as `run`; this only exposes
/// the final message vectors that `run` normally discards.
pub fn run_with_messages(priors: &HashMap<String, NodePrior>, edges: &[GraphEdge]) -> BpRunResult {
    // Build directed adjacency for fast iteration. A single GraphEdge is
    // still treated as symmetric for compatibility; when runtime supplies
    // both directions, their weights are preserved independently.
    let directed_edges = directed_edge_weights(edges);
    let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for ((from, to), weight) in &directed_edges {
        adj.entry(from.clone())
            .or_default()
            .push((to.clone(), *weight));
    }

    // Messages: (from, to) -> 3-vector. Initialize to uniform.
    let mut messages: HashMap<(String, String), [f64; N_STATES]> = HashMap::new();
    for (from, to) in directed_edges.keys() {
        messages.insert(
            (from.clone(), to.clone()),
            [1.0 / N_STATES as f64; N_STATES],
        );
    }

    let nodes: Vec<&String> = priors.keys().collect();
    let mut beliefs: HashMap<String, [f64; N_STATES]> =
        priors.iter().map(|(k, p)| (k.clone(), p.belief)).collect();

    let mut iter = 0;
    let mut converged = false;
    while iter < MAX_ITERATIONS {
        iter += 1;
        // Compute new messages.
        let mut new_messages: HashMap<(String, String), [f64; N_STATES]> = HashMap::new();
        for (from, neighbors) in &adj {
            let from_prior = priors.get(from).cloned().unwrap_or_default().belief;
            for (to, weight) in neighbors {
                // m_{from → to}(x_to) ∝ Σ_x_from [phi(x_from, x_to) ·
                //                                  prior_from(x_from) ·
                //                                  Π_{k ≠ to} m_{k→from}(x_from)]
                let mut prod_in: [f64; N_STATES] = from_prior;
                if let Some(other_neighbors) = adj.get(from) {
                    for (k, _) in other_neighbors {
                        if k == to {
                            continue;
                        }
                        if let Some(m) = messages.get(&(k.clone(), from.clone())) {
                            for i in 0..N_STATES {
                                prod_in[i] *= m[i];
                            }
                        }
                    }
                }
                let mut new_msg = [0.0; N_STATES];
                for x_to in 0..N_STATES {
                    let mut s = 0.0;
                    for x_from in 0..N_STATES {
                        s += edge_potential(*weight, x_from, x_to) * prod_in[x_from];
                    }
                    new_msg[x_to] = s;
                }
                normalize(&mut new_msg);
                // Damping: blend with prior iteration's message to prevent
                // runaway saturation on cyclic graphs. See MESSAGE_DAMPING
                // doc above.
                if let Some(prev_msg) = messages.get(&(from.clone(), to.clone())) {
                    for i in 0..N_STATES {
                        new_msg[i] =
                            MESSAGE_DAMPING * new_msg[i] + (1.0 - MESSAGE_DAMPING) * prev_msg[i];
                    }
                }
                new_messages.insert((from.clone(), to.clone()), new_msg);
            }
        }
        messages = new_messages;

        // Recompute beliefs.
        let mut new_beliefs: HashMap<String, [f64; N_STATES]> = HashMap::new();
        let mut max_change = 0.0_f64;
        for n in &nodes {
            let prior = priors.get(*n).cloned().unwrap_or_default().belief;
            let mut belief = prior;
            if let Some(neighbors) = adj.get(*n) {
                for (k, _) in neighbors {
                    if let Some(m) = messages.get(&(k.clone(), (*n).clone())) {
                        for i in 0..N_STATES {
                            belief[i] *= m[i];
                        }
                    }
                }
            }
            normalize(&mut belief);
            if let Some(prev) = beliefs.get(*n) {
                max_change = max_change.max(max_diff(prev, &belief));
            }
            new_beliefs.insert((*n).clone(), belief);
        }
        beliefs = new_beliefs;

        if max_change < CONVERGENCE_TOL {
            converged = true;
            break;
        }
    }
    BpRunResult {
        beliefs,
        messages,
        iterations: iter,
        converged,
    }
}

/// Build the inputs (priors + edges) from current Eden state.
/// `master_edges` should contain (from_symbol, to_symbol, similarity)
/// triples from the BrainGraph / UsGraph StockToStock edges.
/// Build BP inputs from sub-KG registry + master KG edges.
///
/// Single entry point per v2 plan. All node-level evidence already
/// lives in sub-KG NodeIds (Pressure / Intent / Memory / Belief /
/// Causal — populated upstream by `update_from_substrate_evidence`).
/// `observe_from_subkg` is the single prior reader.
///
/// Lead-lag is the one exception that stays as an explicit parameter:
/// it modifies edge MESSAGE WEIGHT, not node prior, because lead-lag
/// is structurally an edge property (A leads B), not a node property.
/// Modifying edge weight is graph-native; injecting lead-lag into a
/// node prior would conflate edge semantics into node space.
pub fn build_inputs(
    registry: &SubKgRegistry,
    master_edges: &[BpInputEdge],
    lead_lag_events: &[LeadLagEvent],
) -> (HashMap<String, NodePrior>, Vec<GraphEdge>) {
    let mut priors: HashMap<String, NodePrior> = HashMap::new();
    for (sym, kg) in &registry.graphs {
        priors.insert(sym.clone(), observe_from_subkg(kg));
    }
    let edges: Vec<GraphEdge> = master_edges
        .iter()
        .map(|edge| GraphEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            weight: directional_message_weight(&edge.from, &edge.to, edge.weight, lead_lag_events),
            kind: BpEdgeKind::from_edge_type(&edge.edge_type),
        })
        .collect();
    (priors, edges)
}

pub fn build_pruning_shadow_summary(
    priors: &HashMap<String, NodePrior>,
    edges: &[GraphEdge],
) -> BpPruningShadowSummary {
    let observed_priors: std::collections::HashSet<&str> = priors
        .iter()
        .filter(|(_, prior)| prior.observed)
        .map(|(symbol, _)| symbol.as_str())
        .collect();
    let mut observed_incident_edges = 0usize;
    let mut low_weight_edges = 0usize;
    let mut shadow_pruned_edges = 0usize;
    let mut stock_to_stock_edges = 0usize;
    let mut peer_similarity_edges = 0usize;
    let mut cross_market_edges = 0usize;
    let mut unknown_edges = 0usize;
    let mut weight_sum = 0.0;

    for edge in edges {
        match edge.kind {
            BpEdgeKind::StockToStock => stock_to_stock_edges += 1,
            BpEdgeKind::PeerSimilarity => peer_similarity_edges += 1,
            BpEdgeKind::CrossMarket => cross_market_edges += 1,
            BpEdgeKind::Unknown => unknown_edges += 1,
        }
        let abs_weight = edge.weight.abs();
        weight_sum += abs_weight;
        let observed_incident = observed_priors.contains(edge.from.as_str())
            || observed_priors.contains(edge.to.as_str());
        if observed_incident {
            observed_incident_edges += 1;
        }
        if abs_weight < BP_PRUNING_SHADOW_WEIGHT_FLOOR {
            low_weight_edges += 1;
        }
        if !observed_incident && abs_weight < BP_PRUNING_SHADOW_WEIGHT_FLOOR {
            shadow_pruned_edges += 1;
        }
    }

    BpPruningShadowSummary {
        total_edges: edges.len(),
        observed_priors: observed_priors.len(),
        observed_incident_edges,
        low_weight_edges,
        shadow_retained_edges: edges.len().saturating_sub(shadow_pruned_edges),
        shadow_pruned_edges,
        stock_to_stock_edges,
        peer_similarity_edges,
        cross_market_edges,
        unknown_edges,
        weight_floor: BP_PRUNING_SHADOW_WEIGHT_FLOOR,
        weight_mean: if edges.is_empty() {
            0.0
        } else {
            weight_sum / edges.len() as f64
        },
    }
}

/// Directional lead-lag evidence modifies the message weight in the
/// direction of observed lead and dampens the reverse direction.
pub fn directional_message_weight(
    from: &str,
    to: &str,
    base_weight: f64,
    lead_lag_events: &[LeadLagEvent],
) -> f64 {
    let mut weight = base_weight;
    for event in lead_lag_events {
        let corr = event.correlation_at_lag.abs();
        let multiplier = match event.direction.as_str() {
            "from_leads" if event.from_symbol == from && event.to_symbol == to => Some(1.0 + corr),
            "from_leads" if event.from_symbol == to && event.to_symbol == from => {
                Some(1.0 - 0.5 * corr)
            }
            "to_leads" if event.from_symbol == from && event.to_symbol == to => {
                Some(1.0 - 0.5 * corr)
            }
            "to_leads" if event.from_symbol == to && event.to_symbol == from => Some(1.0 + corr),
            _ => None,
        };
        if let Some(multiplier) = multiplier {
            weight *= multiplier;
        }
    }
    weight.clamp(MIN_MESSAGE_WEIGHT, MAX_MESSAGE_WEIGHT)
}

/// Set a setup's confidence directly from the BP posterior for its
/// explicit tactical direction. This is not a modulation stage: BP
/// posterior is the graph-native source of truth for directional setup
/// confidence.
pub fn apply_posterior_confidence(
    setup: &mut TacticalSetup,
    beliefs: &HashMap<String, [f64; N_STATES]>,
) -> bool {
    let symbol = match &setup.scope {
        ReasoningScope::Symbol(s) => s.0.clone(),
        _ => return false,
    };
    let Some(direction) = direction_from_setup(setup) else {
        return false;
    };
    let Some(post) = beliefs.get(&symbol) else {
        return false;
    };
    let p_target = match direction {
        TacticalDirection::Long => post[STATE_BULL],
        TacticalDirection::Short => post[STATE_BEAR],
    };
    if let Ok(confidence) = Decimal::try_from(p_target.clamp(0.0, 1.0)) {
        setup.confidence = confidence;
    } else {
        return false;
    }
    setup.risk_notes.push(format!(
        "bp_posterior_confidence: p_target={:.3} (bull={:.3} bear={:.3} neutral={:.3})",
        p_target, post[STATE_BULL], post[STATE_BEAR], post[STATE_NEUTRAL],
    ));
    true
}

/// Compose rows that expose the complete BP fusion surface for visual
/// inspection: prior rows per symbol, final directed message rows per
/// edge direction, and posterior rows per symbol. This is an artifact
/// only; it is not read back by inference.
pub fn build_message_trace_rows(
    market: &str,
    tick: u64,
    priors: &HashMap<String, NodePrior>,
    edges: &[GraphEdge],
    result: &BpRunResult,
    ts: DateTime<Utc>,
) -> Vec<BpMessageTraceRow> {
    let mut rows = Vec::with_capacity(priors.len() + result.messages.len() + result.beliefs.len());

    let mut prior_symbols: Vec<&String> = priors.keys().collect();
    prior_symbols.sort();
    for symbol in prior_symbols {
        let prior = priors.get(symbol).expect("symbol from key set");
        rows.push(BpMessageTraceRow {
            ts,
            market: market.to_string(),
            tick,
            kind: BpTraceKind::Prior,
            iterations: result.iterations,
            converged: result.converged,
            symbol: Some(symbol.clone()),
            from_symbol: None,
            to_symbol: None,
            edge_weight: None,
            observed: Some(prior.observed),
            p_bull: prior.belief[STATE_BULL],
            p_bear: prior.belief[STATE_BEAR],
            p_neutral: prior.belief[STATE_NEUTRAL],
        });
    }

    let edge_weights = directed_edge_weights(edges);
    let mut message_keys: Vec<&(String, String)> = result.messages.keys().collect();
    message_keys.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    for key in message_keys {
        let message = result.messages.get(key).expect("key from message set");
        rows.push(BpMessageTraceRow {
            ts,
            market: market.to_string(),
            tick,
            kind: BpTraceKind::Message,
            iterations: result.iterations,
            converged: result.converged,
            symbol: None,
            from_symbol: Some(key.0.clone()),
            to_symbol: Some(key.1.clone()),
            edge_weight: edge_weights.get(key).copied(),
            observed: None,
            p_bull: message[STATE_BULL],
            p_bear: message[STATE_BEAR],
            p_neutral: message[STATE_NEUTRAL],
        });
    }

    let mut posterior_symbols: Vec<&String> = result.beliefs.keys().collect();
    posterior_symbols.sort();
    for symbol in posterior_symbols {
        let post = result.beliefs.get(symbol).expect("symbol from belief set");
        rows.push(BpMessageTraceRow {
            ts,
            market: market.to_string(),
            tick,
            kind: BpTraceKind::Posterior,
            iterations: result.iterations,
            converged: result.converged,
            symbol: Some(symbol.clone()),
            from_symbol: None,
            to_symbol: None,
            edge_weight: None,
            observed: priors.get(symbol).map(|p| p.observed),
            p_bull: post[STATE_BULL],
            p_bear: post[STATE_BEAR],
            p_neutral: post[STATE_NEUTRAL],
        });
    }

    rows
}

/// Belief-only variant of `build_message_trace_rows`. Used after the
/// sync substrate deletion (2026-04-29): the event substrate exposes
/// per-symbol beliefs via `PosteriorView` but does not surface
/// message-level history (those live inside the worker pool's
/// inboxes and are not snapshotable cheaply). Trace rows therefore
/// carry the prior + posterior layers only — sufficient for
/// visual_graph_frame and operator inspection workflows.
pub fn build_belief_only_trace_rows(
    market: &str,
    tick: u64,
    priors: &HashMap<String, NodePrior>,
    edges: &[GraphEdge],
    beliefs: &HashMap<String, [f64; N_STATES]>,
    ts: DateTime<Utc>,
) -> Vec<BpMessageTraceRow> {
    let mut rows = Vec::with_capacity(priors.len() + beliefs.len());
    let _ = edges;

    let mut prior_symbols: Vec<&String> = priors.keys().collect();
    prior_symbols.sort();
    for symbol in prior_symbols {
        let prior = priors.get(symbol).expect("symbol from key set");
        rows.push(BpMessageTraceRow {
            ts,
            market: market.to_string(),
            tick,
            kind: BpTraceKind::Prior,
            iterations: 0,
            converged: true,
            symbol: Some(symbol.clone()),
            from_symbol: None,
            to_symbol: None,
            edge_weight: None,
            observed: Some(prior.observed),
            p_bull: prior.belief[STATE_BULL],
            p_bear: prior.belief[STATE_BEAR],
            p_neutral: prior.belief[STATE_NEUTRAL],
        });
    }

    let mut posterior_symbols: Vec<&String> = beliefs.keys().collect();
    posterior_symbols.sort();
    for symbol in posterior_symbols {
        let post = beliefs.get(symbol).expect("symbol from belief set");
        rows.push(BpMessageTraceRow {
            ts,
            market: market.to_string(),
            tick,
            kind: BpTraceKind::Posterior,
            iterations: 0,
            converged: true,
            symbol: Some(symbol.clone()),
            from_symbol: None,
            to_symbol: None,
            edge_weight: None,
            observed: priors.get(symbol).map(|p| p.observed),
            p_bull: post[STATE_BULL],
            p_bear: post[STATE_BEAR],
            p_neutral: post[STATE_NEUTRAL],
        });
    }

    rows
}

pub fn write_message_trace(market: &str, rows: &[BpMessageTraceRow]) -> std::io::Result<usize> {
    write_message_trace_to_store(&RuntimeArtifactStore::default(), market, rows)
}

fn write_message_trace_to_store(
    store: &RuntimeArtifactStore,
    market: &str,
    rows: &[BpMessageTraceRow],
) -> std::io::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let market = MarketRegistry::by_slug(market).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown market for BP message trace: {market}"),
        )
    })?;
    let mut written = 0;
    for r in rows {
        store.append_json_line(RuntimeArtifactKind::BpMessageTrace, market, r)?;
        written += 1;
    }
    Ok(written)
}

/// Compose ndjson rows from BP results.
pub fn build_marginal_rows(
    market: &str,
    priors: &HashMap<String, NodePrior>,
    beliefs: &HashMap<String, [f64; N_STATES]>,
    iterations: usize,
    converged: bool,
    ts: DateTime<Utc>,
) -> Vec<MarginalRow> {
    beliefs
        .iter()
        .map(|(sym, b)| MarginalRow {
            ts,
            market: market.to_string(),
            symbol: sym.clone(),
            iterations,
            converged,
            p_bull: b[STATE_BULL],
            p_bear: b[STATE_BEAR],
            p_neutral: b[STATE_NEUTRAL],
            observed: priors.get(sym).map(|p| p.observed).unwrap_or(false),
        })
        .collect()
}

pub fn write_marginals(market: &str, rows: &[MarginalRow]) -> std::io::Result<usize> {
    write_marginals_to_store(&RuntimeArtifactStore::default(), market, rows)
}

fn write_marginals_to_store(
    store: &RuntimeArtifactStore,
    market: &str,
    rows: &[MarginalRow],
) -> std::io::Result<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let market = MarketRegistry::by_slug(market).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown market for BP marginals: {market}"),
        )
    })?;
    let mut written = 0;
    for r in rows {
        store.append_json_line(RuntimeArtifactKind::BpMarginals, market, r)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_underflow_falls_back_to_uniform() {
        // Regression: long products of small messages can drive every
        // cell to 0.0; the previous impl divided by sum+EPSILON which
        // left the array as [0,0,0]. Now we detect the degenerate case
        // and emit a proper uniform distribution.
        let mut b = [0.0_f64; N_STATES];
        normalize(&mut b);
        let uniform = 1.0 / N_STATES as f64;
        for v in &b {
            assert!((*v - uniform).abs() < 1e-12, "expected uniform, got {b:?}");
        }
        let s: f64 = b.iter().sum();
        assert!((s - 1.0).abs() < 1e-12, "expected sum=1, got {s}");
    }

    #[test]
    fn dense_cyclic_graph_with_mild_prior_does_not_saturate_to_extreme() {
        // Regression: on a 31-degree fully-connected mini-graph (mirroring
        // the master KG's avg degree), priors that are only mildly above
        // uniform (dominant 0.45, rest 0.27 — concentration ≈ 0.18 above
        // uniform) used to saturate to p > 0.99999 after BP because
        // EDGE_AGREEMENT_STRENGTH × graph_density × MAX_ITERATIONS
        // exponentiates concentration on cyclic graphs. The fix lowers
        // α and adds message damping (standard BP literature treatment
        // for non-tree graphs).
        let n = 32;
        let mut priors = HashMap::new();
        for i in 0..n {
            priors.insert(
                format!("N{i}"),
                NodePrior {
                    belief: [0.45, 0.27, 0.28],
                    observed: false,
                },
            );
        }
        let mut edges = Vec::new();
        for i in 0..n {
            for j in (i + 1)..n {
                edges.push(GraphEdge {
                    from: format!("N{i}"),
                    to: format!("N{j}"),
                    weight: 0.5,
                    kind: BpEdgeKind::StockToStock,
                });
            }
        }
        let (beliefs, _, _) = run(&priors, &edges);
        for (sym, b) in &beliefs {
            let max_mass = b.iter().cloned().fold(0.0_f64, f64::max);
            assert!(
                max_mass < 0.95,
                "{sym} posterior {b:?} saturated (max={max_mass}) — \
                 BP should stay calibrated on dense graphs"
            );
        }
    }

    #[test]
    fn normalize_handles_normal_input() {
        let mut b = [1.0_f64, 2.0, 3.0];
        normalize(&mut b);
        let s: f64 = b.iter().sum();
        assert!((s - 1.0).abs() < 1e-12);
        assert!((b[0] - 1.0 / 6.0).abs() < 1e-12);
        assert!((b[1] - 2.0 / 6.0).abs() < 1e-12);
        assert!((b[2] - 3.0 / 6.0).abs() < 1e-12);
    }

    fn observed(state: usize, magnitude: f64) -> NodePrior {
        let mut b = [0.0; N_STATES];
        let dominant = (1.0 + magnitude) / (N_STATES as f64 + magnitude);
        let rest = (1.0 - dominant) / (N_STATES - 1) as f64;
        for i in 0..N_STATES {
            b[i] = if i == state { dominant } else { rest };
        }
        NodePrior {
            belief: b,
            observed: true,
        }
    }

    #[test]
    fn marginal_writer_uses_runtime_artifact_schema_envelope() {
        let root = std::env::temp_dir().join(format!(
            "eden-bp-marginals-{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let store = crate::core::runtime_artifacts::RuntimeArtifactStore::new(&root);
        let rows = vec![MarginalRow {
            ts: Utc::now(),
            market: "us".to_string(),
            symbol: "A.US".to_string(),
            iterations: 3,
            converged: true,
            p_bull: 0.7,
            p_bear: 0.2,
            p_neutral: 0.1,
            observed: true,
        }];

        let written = write_marginals_to_store(&store, "us", &rows).expect("write marginals");
        let latest: crate::core::runtime_artifacts::RuntimeArtifactEnvelope<MarginalRow> = store
            .read_latest_json_line(
                crate::core::runtime_artifacts::RuntimeArtifactKind::BpMarginals,
                crate::core::market::MarketId::Us,
            )
            .expect("read latest marginal")
            .expect("marginal exists");

        assert_eq!(written, 1);
        assert_eq!(
            latest.schema_version,
            crate::core::runtime_artifacts::RUNTIME_ARTIFACT_SCHEMA_VERSION
        );
        assert_eq!(latest.payload.symbol, "A.US");

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn isolated_observed_node_keeps_prior() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.8));
        let (beliefs, _, _) = run(&priors, &[]);
        let b = beliefs.get("A").unwrap();
        assert!(b[STATE_BULL] > b[STATE_BEAR]);
        assert!(b[STATE_BULL] > b[STATE_NEUTRAL]);
    }

    #[test]
    fn isolated_unobserved_stays_uniform() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), NodePrior::default());
        let (beliefs, _, converged) = run(&priors, &[]);
        let b = beliefs.get("A").unwrap();
        assert!((b[STATE_BULL] - 1.0 / 3.0).abs() < 1e-6);
        assert!(converged);
    }

    #[test]
    fn neighbor_belief_pulls_unobserved_toward_observed() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.9));
        priors.insert("B".to_string(), NodePrior::default()); // unobserved
        let edges = vec![GraphEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.9,
            kind: BpEdgeKind::StockToStock,
        }];
        let (beliefs, _, _) = run(&priors, &edges);
        let b = beliefs.get("B").unwrap();
        // B should now be biased toward Bull because A is.
        assert!(
            b[STATE_BULL] > b[STATE_BEAR],
            "B should lean Bull from A's influence: {:?}",
            b
        );
        assert!(b[STATE_BULL] > 1.0 / 3.0 + 0.01);
    }

    #[test]
    fn run_with_messages_exposes_final_directed_messages() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.9));
        priors.insert("B".to_string(), NodePrior::default());
        let edges = vec![GraphEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.9,
            kind: BpEdgeKind::StockToStock,
        }];

        let result = run_with_messages(&priors, &edges);
        let ab = result
            .messages
            .get(&("A".to_string(), "B".to_string()))
            .expect("A->B message");
        let ba = result
            .messages
            .get(&("B".to_string(), "A".to_string()))
            .expect("B->A reverse message");

        assert!((ab.iter().sum::<f64>() - 1.0).abs() < 1e-6);
        assert!((ba.iter().sum::<f64>() - 1.0).abs() < 1e-6);
        assert!(ab[STATE_BULL] > ab[STATE_BEAR]);
    }

    #[test]
    fn typed_bp_inputs_preserve_master_edge_kind() {
        use crate::pipeline::symbol_sub_kg::SubKgRegistry;

        let registry = SubKgRegistry::default();
        let input_edges = vec![BpInputEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.8,
            edge_type: "PeerSimilarity".to_string(),
        }];

        let (_, edges) = build_inputs(&registry, &input_edges, &[]);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].kind, BpEdgeKind::PeerSimilarity);
        assert_eq!(edges[0].from, "A");
        assert_eq!(edges[0].to, "B");
    }

    #[test]
    fn pruning_shadow_keeps_observed_neighborhood_and_counts_low_weight_edges() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.9));
        priors.insert("B".to_string(), NodePrior::default());
        priors.insert("C".to_string(), NodePrior::default());
        priors.insert("D".to_string(), NodePrior::default());
        let edges = vec![
            GraphEdge {
                from: "A".into(),
                to: "B".into(),
                weight: 0.1,
                kind: BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "C".into(),
                to: "D".into(),
                weight: 0.1,
                kind: BpEdgeKind::PeerSimilarity,
            },
            GraphEdge {
                from: "B".into(),
                to: "C".into(),
                weight: 0.8,
                kind: BpEdgeKind::CrossMarket,
            },
        ];

        let summary = build_pruning_shadow_summary(&priors, &edges);

        assert_eq!(summary.total_edges, 3);
        assert_eq!(summary.observed_priors, 1);
        assert_eq!(summary.observed_incident_edges, 1);
        assert_eq!(summary.low_weight_edges, 2);
        assert_eq!(summary.shadow_retained_edges, 2);
        assert_eq!(summary.shadow_pruned_edges, 1);
        assert_eq!(summary.stock_to_stock_edges, 1);
        assert_eq!(summary.peer_similarity_edges, 1);
        assert_eq!(summary.cross_market_edges, 1);
        assert!((summary.weight_mean - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn bp_message_trace_contains_prior_message_and_posterior_rows() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.9));
        priors.insert("B".to_string(), NodePrior::default());
        let edges = vec![GraphEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.9,
            kind: BpEdgeKind::StockToStock,
        }];
        let result = run_with_messages(&priors, &edges);

        let rows = build_message_trace_rows("us", 42, &priors, &edges, &result, Utc::now());

        assert!(rows
            .iter()
            .any(|r| { r.kind == BpTraceKind::Prior && r.symbol.as_deref() == Some("A") }));
        assert!(rows.iter().any(|r| {
            r.kind == BpTraceKind::Message
                && r.from_symbol.as_deref() == Some("A")
                && r.to_symbol.as_deref() == Some("B")
                && r.edge_weight == Some(0.9)
        }));
        assert!(rows
            .iter()
            .any(|r| { r.kind == BpTraceKind::Posterior && r.symbol.as_deref() == Some("B") }));
    }

    #[test]
    fn opposing_observed_yields_neutral_in_between() {
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), observed(STATE_BULL, 0.8));
        priors.insert("B".to_string(), NodePrior::default());
        priors.insert("C".to_string(), observed(STATE_BEAR, 0.8));
        let edges = vec![
            GraphEdge {
                from: "A".into(),
                to: "B".into(),
                weight: 0.9,
                kind: BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "B".into(),
                to: "C".into(),
                weight: 0.9,
                kind: BpEdgeKind::StockToStock,
            },
        ];
        let (beliefs, _, _) = run(&priors, &edges);
        let b = beliefs.get("B").unwrap();
        // B receives Bull pull from A and Bear pull from C — Neutral
        // shouldn't dominate, but Bull and Bear should be near-equal.
        assert!(
            (b[STATE_BULL] - b[STATE_BEAR]).abs() < 0.15,
            "B caught between opposing forces should be ~symmetric: {:?}",
            b
        );
    }

    #[test]
    fn convergence_stable_within_tolerance() {
        // 5-node chain with 1 anchor at each end (one Bull, one Bear).
        let mut priors = HashMap::new();
        priors.insert("S0".to_string(), observed(STATE_BULL, 0.9));
        priors.insert("S1".to_string(), NodePrior::default());
        priors.insert("S2".to_string(), NodePrior::default());
        priors.insert("S3".to_string(), NodePrior::default());
        priors.insert("S4".to_string(), observed(STATE_BEAR, 0.9));
        let edges = vec![
            GraphEdge {
                from: "S0".into(),
                to: "S1".into(),
                weight: 0.5,
                kind: BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "S1".into(),
                to: "S2".into(),
                weight: 0.5,
                kind: BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "S2".into(),
                to: "S3".into(),
                weight: 0.5,
                kind: BpEdgeKind::StockToStock,
            },
            GraphEdge {
                from: "S3".into(),
                to: "S4".into(),
                weight: 0.5,
                kind: BpEdgeKind::StockToStock,
            },
        ];
        let (_beliefs, iters, _converged) = run(&priors, &edges);
        // Tree topology — should converge well within MAX_ITERATIONS.
        assert!(iters <= MAX_ITERATIONS, "should not exceed iteration cap");
    }

    #[test]
    fn observation_extraction_strong_bull_signal() {
        use crate::pipeline::symbol_sub_kg::SymbolSubKG;
        use rust_decimal_macros::dec;
        let mut kg = SymbolSubKG::new_empty("X.US".into(), Utc::now());
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.6), Utc::now());
        kg.set_node_value(NodeId::IntentAccumulation, dec!(0.5), Utc::now());
        let prior = observe_from_subkg(&kg);
        assert!(prior.observed, "strong signal must be observed");
        assert!(prior.belief[STATE_BULL] > prior.belief[STATE_BEAR]);
    }

    #[test]
    fn observation_extraction_weak_signal_unobserved() {
        use crate::pipeline::symbol_sub_kg::SymbolSubKG;
        use rust_decimal_macros::dec;
        let mut kg = SymbolSubKG::new_empty("X.US".into(), Utc::now());
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.01), Utc::now());
        let prior = observe_from_subkg(&kg);
        assert!(!prior.observed, "weak signal stays uniform");
    }

    #[test]
    fn engram_alignment_node_biases_prior_direction() {
        // V2 path: EngramAlignment NodeId is the carrier of historical
        // regime outcome bias. Previously injected via EvidenceContext
        // (now deleted) — now flows through sub-KG.
        use crate::pipeline::symbol_sub_kg::SymbolSubKG;
        use rust_decimal_macros::dec;

        let mut kg = SymbolSubKG::new_empty("X.US".into(), Utc::now());
        // Strong direction signal + bullish engram alignment + sufficient
        // evidence strength so concentration clears the floor.
        kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.5), Utc::now());
        kg.set_node_value(NodeId::EngramAlignment, dec!(0.4), Utc::now());
        kg.set_node_value(NodeId::WlAnalogConfidence, dec!(0.5), Utc::now());
        kg.set_node_value(NodeId::BeliefSampleCount, dec!(0.5), Utc::now());

        let prior = observe_from_subkg(&kg);
        assert!(prior.observed);
        assert!(prior.belief[STATE_BULL] > prior.belief[STATE_BEAR]);
        assert!(prior.belief[STATE_BULL] > prior.belief[STATE_NEUTRAL]);
    }

    #[test]
    fn evidence_strength_concentrates_dominant_state() {
        // V2 path: WlAnalogConfidence + BeliefSampleCount + ForecastAccuracy
        // are the evidence-strength carriers. Higher values → tighter
        // concentration on dominant state.
        use crate::pipeline::symbol_sub_kg::SymbolSubKG;
        use rust_decimal_macros::dec;

        let make = |ev: Decimal| -> [f64; N_STATES] {
            let mut kg = SymbolSubKG::new_empty("X.US".into(), Utc::now());
            kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.6), Utc::now());
            kg.set_node_value(NodeId::WlAnalogConfidence, ev, Utc::now());
            kg.set_node_value(NodeId::BeliefSampleCount, ev, Utc::now());
            observe_from_subkg(&kg).belief
        };
        let weak = make(dec!(0.2));
        let strong = make(dec!(0.9));
        // Stronger evidence → bigger gap between dominant and rest.
        let weak_gap = weak[STATE_BULL] - weak[STATE_BEAR];
        let strong_gap = strong[STATE_BULL] - strong[STATE_BEAR];
        assert!(
            strong_gap > weak_gap,
            "stronger evidence must concentrate more (weak gap {weak_gap} vs strong gap {strong_gap})",
        );
    }

    #[test]
    fn high_belief_entropy_reduces_concentration() {
        // V2 path: BeliefEntropy multiplies (1 - 0.5*entropy). Max
        // entropy halves the concentration but doesn't zero it out
        // (allow Pressure+Intent to still anchor when Eden has no
        // categorical belief data yet).
        use crate::pipeline::symbol_sub_kg::SymbolSubKG;
        use rust_decimal_macros::dec;

        let make = |entropy: Decimal| -> [f64; N_STATES] {
            let mut kg = SymbolSubKG::new_empty("X.US".into(), Utc::now());
            kg.set_node_value(NodeId::PressureCapitalFlow, dec!(0.6), Utc::now());
            kg.set_node_value(NodeId::WlAnalogConfidence, dec!(0.5), Utc::now());
            kg.set_node_value(NodeId::BeliefSampleCount, dec!(0.5), Utc::now());
            kg.set_node_value(NodeId::BeliefEntropy, entropy, Utc::now());
            observe_from_subkg(&kg).belief
        };
        let calm = make(dec!(0.0));
        let noisy = make(dec!(1.0));
        // Both observed (Pressure signal still anchors), but high entropy
        // pulls the dominant mass toward uniform (1/3).
        let calm_gap = calm[STATE_BULL] - calm[STATE_BEAR];
        let noisy_gap = noisy[STATE_BULL] - noisy[STATE_BEAR];
        assert!(
            calm_gap > noisy_gap,
            "low entropy must concentrate more than high entropy (calm gap {calm_gap} vs noisy gap {noisy_gap})",
        );
    }

    #[test]
    fn vol_alone_does_not_drive_direction() {
        // Only vol = -1 (max negative size anomaly), all other channels zero.
        let prior = prior_from_pressure_channels(
            Some(0.0),  // ob
            Some(0.0),  // cf
            Some(0.0),  // institutional
            Some(0.0),  // mo
            Some(-1.0), // vol — should NOT drive direction
            Some(0.0),  // st
        );
        // With vol no longer in direction_raw and all other channels at zero,
        // base_magnitude will be 0 and prior should be unobserved.
        assert!(!prior.observed, "lone vol-only signal must not produce observed prior");
    }

    #[test]
    fn cf_drives_direction() {
        // cf = +1 should produce bullish prior.
        let prior = prior_from_pressure_channels(
            Some(0.0),  // ob
            Some(1.0),  // cf — strong positive
            Some(0.0),  // institutional
            Some(0.0),  // mo
            Some(0.0),  // vol
            Some(0.0),  // st
        );
        assert!(prior.observed, "strong cf should produce observed prior");
        // belief[STATE_BULL] should be highest
        let p_bull = prior.belief[STATE_BULL];
        let p_bear = prior.belief[STATE_BEAR];
        assert!(p_bull > p_bear, "cf=+1 must produce bullish belief; got bull={} bear={}", p_bull, p_bear);
    }

    #[test]
    fn leading_edge_changes_message_strength() {
        let event = LeadLagEvent {
            ts: Utc::now(),
            market: "us".to_string(),
            from_symbol: "A.US".to_string(),
            to_symbol: "B.US".to_string(),
            edge_weight: 0.5,
            dominant_lag: 1,
            correlation_at_lag: 0.6,
            n_samples: 12,
            direction: "from_leads".to_string(),
        };
        let ab = directional_message_weight("A.US", "B.US", 0.5, &[event.clone()]);
        let ba = directional_message_weight("B.US", "A.US", 0.5, &[event]);

        assert!(ab > 0.5, "leader-to-lagger direction should strengthen");
        assert!(ba < 0.5, "lagger-to-leader direction should dampen");
        assert!(ab > ba);
    }
}
