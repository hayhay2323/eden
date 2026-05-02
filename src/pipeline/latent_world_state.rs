//! Shift A: latent world state — linear Gaussian state-space model.
//!
//! Until now Eden had no single object representing "what the world
//! looks like right now." Each stage read its own field. TacticalSetup
//! was the output; there was no unified current-state representation
//! to reason or plan over. That blocks causal rollout (Shift B) and
//! counterfactual planning (Shift C) because they need a single
//! anchor for "world state at time t."
//!
//! This module introduces `LatentWorldState` — a low-dim Gaussian
//! latent z_t evolved by a linear state-space model:
//!
//!   z_{t+1} = F z_t + w,   w ~ N(0, Q)        (transition)
//!   y_t    = H z_t + v,    v ~ N(0, R)        (observation)
//!
//! Kalman filter predict/update each tick. Dimensions chosen to be
//! operator-interpretable:
//!
//!   0: market stress            (composite of pressure field vortex tension)
//!   1: breadth                  (positive minus negative persistent state count)
//!   2: synchrony                (fraction of symbols moving together)
//!   3: institutional flow       (aggregate institutional channel pressure)
//!   4: retail flow              (aggregate non-institutional channel pressure)
//!
//! 5-dim is enough for a first prototype — operator can read each
//! dimension, and all five are already emergent quantities Eden
//! computes elsewhere (we're just wrapping them in a coherent SSM).
//!
//! Non-goals for v1:
//!   - Non-linear dynamics (VAE / neural SSM — Tier 2)
//!   - High-dim latents
//!   - Observation-dependent transition (that's a regime-switching
//!     SSM — worth doing later)

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::horizon::{HorizonBucket, Urgency};
use crate::ontology::objects::Market;
use crate::ontology::{
    ExpectationBinding, ExpectationKind, ExpectationViolation, ExpectationViolationKind,
    IntentDirection, IntentHypothesis, IntentKind, IntentOpportunityBias, IntentOpportunityWindow,
    IntentState, IntentStrength, ReasoningScope,
};
use crate::perception::{PerceptionGraph, WorldIntentSnapshot};
use crate::pipeline::belief::CategoricalBelief;

pub const LATENT_DIM: usize = 5;
pub const LATENT_NAMES: [&str; LATENT_DIM] =
    ["stress", "breadth", "synchrony", "inst_flow", "retail_flow"];
const STRESS: usize = 0;
const BREADTH: usize = 1;
const SYNCHRONY: usize = 2;
const INST_FLOW: usize = 3;
const RETAIL_FLOW: usize = 4;

const WORLD_INTENT_COUNT: usize = 6;
const WORLD_INTENT_VARIANTS: [IntentKind; WORLD_INTENT_COUNT] = [
    IntentKind::Accumulation,
    IntentKind::Distribution,
    IntentKind::ForcedUnwind,
    IntentKind::EventRepricing,
    IntentKind::Absorption,
    IntentKind::Unknown,
];
const NUMERIC_EPSILON: f64 = 1.0e-9;

/// One tick's observation vector — emitted by the aggregator below
/// before being fed to the Kalman update step.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorldObservation {
    pub values: [f64; LATENT_DIM],
    /// If false, treat observation as missing for that dim (infinite
    /// variance in R). First tick typically has missing breadth /
    /// synchrony before Eden has enough symbol state samples.
    pub mask: [bool; LATENT_DIM],
}

impl WorldObservation {
    pub fn all_missing() -> Self {
        Self {
            values: [0.0; LATENT_DIM],
            mask: [false; LATENT_DIM],
        }
    }
}

/// 5×5 matrix stored row-major for serde simplicity.
pub type Mat5 = [[f64; LATENT_DIM]; LATENT_DIM];
pub type Vec5 = [f64; LATENT_DIM];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentWorldState {
    pub market: Market,
    pub last_tick: u64,
    /// z_t — latent mean.
    pub mean: Vec5,
    /// P_t — latent covariance.
    pub covariance: Mat5,
    /// F — transition matrix. Default is `mean_reversion * I`.
    pub transition: Mat5,
    /// Q — process noise covariance (diagonal default).
    pub process_noise: Mat5,
    /// H — observation matrix. Default is identity (each latent dim
    /// is directly observed).
    pub observation: Mat5,
    /// R — observation noise covariance (diagonal).
    pub observation_noise: Mat5,
    /// Total tick count — used to distinguish "never updated" from
    /// "just updated with missing observation."
    pub update_count: u32,
}

impl LatentWorldState {
    /// Sensible defaults: mild mean-reversion (F = 0.95 I), small
    /// process noise (Q = 0.02 I), identity observation (H = I),
    /// moderate observation noise (R = 0.10 I). Initial covariance
    /// is large (P0 = 1.0 I) so the first observation dominates.
    pub fn new(market: Market) -> Self {
        let mut transition = identity5();
        for i in 0..LATENT_DIM {
            transition[i][i] = 0.95;
        }
        let process_noise = scaled_identity5(0.02);
        let observation = identity5();
        let observation_noise = scaled_identity5(0.10);
        Self {
            market,
            last_tick: 0,
            mean: [0.0; LATENT_DIM],
            covariance: scaled_identity5(1.0),
            transition,
            process_noise,
            observation,
            observation_noise,
            update_count: 0,
        }
    }

    /// Kalman predict + update for one observation. `tick` is only
    /// used for the persistence tag.
    pub fn step(&mut self, tick: u64, obs: WorldObservation) {
        self.predict();
        self.update(obs);
        self.last_tick = tick;
        self.update_count = self.update_count.saturating_add(1);
    }

    fn predict(&mut self) {
        // mean' = F mean
        let new_mean = mat_vec(&self.transition, &self.mean);
        // cov' = F cov F^T + Q
        let f_cov = mat_mul(&self.transition, &self.covariance);
        let f_cov_ft = mat_mul_t(&f_cov, &self.transition);
        let new_cov = mat_add(&f_cov_ft, &self.process_noise);
        self.mean = new_mean;
        self.covariance = new_cov;
    }

    fn update(&mut self, obs: WorldObservation) {
        // Missing-value handling: rows of H corresponding to
        // unobserved dims are zeroed and their R entries bumped to
        // a huge value so the Kalman gain there ≈ 0.
        let mut h = self.observation;
        let mut r = self.observation_noise;
        for i in 0..LATENT_DIM {
            if !obs.mask[i] {
                for j in 0..LATENT_DIM {
                    h[i][j] = 0.0;
                }
                r[i][i] = 1.0e6;
            }
        }
        // innovation y - H mean
        let predicted_obs = mat_vec(&h, &self.mean);
        let mut innovation = [0.0_f64; LATENT_DIM];
        for i in 0..LATENT_DIM {
            innovation[i] = if obs.mask[i] {
                obs.values[i] - predicted_obs[i]
            } else {
                0.0
            };
        }
        // S = H cov H^T + R
        let h_cov = mat_mul(&h, &self.covariance);
        let h_cov_ht = mat_mul_t(&h_cov, &h);
        let s = mat_add(&h_cov_ht, &r);
        // K = cov H^T S^{-1}
        let s_inv = match invert_5x5(&s) {
            Some(inv) => inv,
            None => return, // degenerate — skip update rather than NaN out
        };
        let cov_ht = mat_mul_t(&self.covariance, &h);
        let k = mat_mul(&cov_ht, &s_inv);
        // mean += K * innovation
        let k_innov = mat_vec(&k, &innovation);
        for i in 0..LATENT_DIM {
            self.mean[i] += k_innov[i];
        }
        // cov = (I - K H) cov
        let k_h = mat_mul(&k, &h);
        let i_kh = mat_sub(&identity5(), &k_h);
        self.covariance = mat_mul(&i_kh, &self.covariance);
        // Symmetrize to keep numeric stability.
        for i in 0..LATENT_DIM {
            for j in i + 1..LATENT_DIM {
                let avg = 0.5 * (self.covariance[i][j] + self.covariance[j][i]);
                self.covariance[i][j] = avg;
                self.covariance[j][i] = avg;
            }
        }
    }

    pub fn dim_value(&self, idx: usize) -> Option<f64> {
        self.mean.get(idx).copied()
    }

    pub fn dim_variance(&self, idx: usize) -> Option<f64> {
        self.covariance
            .get(idx)
            .and_then(|row| row.get(idx))
            .copied()
    }

    /// Summary for wake emission: top dimensions by absolute value,
    /// paired with their standard deviation. Grep-friendly key=value.
    pub fn summary_line(&self) -> String {
        let mut parts = Vec::with_capacity(LATENT_DIM);
        for i in 0..LATENT_DIM {
            let stdev = self.dim_variance(i).unwrap_or(0.0).max(0.0).sqrt();
            parts.push(format!(
                "{}={:+.2}±{:.2}",
                LATENT_NAMES[i], self.mean[i], stdev
            ));
        }
        format!(
            "world_state: tick={} updates={} {}",
            self.last_tick,
            self.update_count,
            parts.join(" "),
        )
    }

    pub fn dominant_world_intent(&self) -> IntentHypothesis {
        infer_world_intent(self)
    }
}

// ---------------------------------------------------------------------------
// Intent posterior — project the latent world state into the ontology's
// existing IntentHypothesis schema.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WorldIntentBelief {
    market: Market,
    previous: CategoricalBelief<IntentKind>,
    posterior: CategoricalBelief<IntentKind>,
    previous_intent: Option<IntentHypothesis>,
}

impl WorldIntentBelief {
    pub fn new(market: Market) -> Self {
        let posterior = world_intent_prior();
        Self {
            market,
            previous: posterior.clone(),
            posterior,
            previous_intent: None,
        }
    }

    pub fn observe_state(&mut self, state: &LatentWorldState) -> IntentHypothesis {
        if self.market != state.market {
            *self = Self::new(state.market);
        }
        self.previous = self.posterior.clone();
        let likelihoods = world_intent_likelihoods(state);
        let likelihoods_decimal: Vec<Decimal> =
            likelihoods.iter().map(|v| decimal_positive(*v)).collect();
        self.posterior.update_likelihoods(&likelihoods_decimal);
        let mut intent =
            build_world_intent_hypothesis(state, &self.posterior, Some(&self.previous));
        if let Some(previous_intent) = self.previous_intent.as_ref() {
            intent.expectation_violations =
                world_intent_expectation_violations(previous_intent, &intent, state);
        }
        self.previous_intent = Some(intent.clone());
        intent
    }

    pub fn posterior(&self) -> &CategoricalBelief<IntentKind> {
        &self.posterior
    }
}

pub fn infer_world_intent(state: &LatentWorldState) -> IntentHypothesis {
    let mut belief = WorldIntentBelief::new(state.market);
    belief.observe_state(state)
}

fn build_world_intent_hypothesis(
    state: &LatentWorldState,
    posterior: &CategoricalBelief<IntentKind>,
    previous: Option<&CategoricalBelief<IntentKind>>,
) -> IntentHypothesis {
    let summary = summarize_world_intent_posterior(posterior);
    let kind = summary.kind;
    let direction = infer_intent_direction(kind, state);
    let certainty = latent_certainty(state);
    let persistence = evidence_maturity(state.update_count);
    let conflict = intent_conflict_score(state);
    let confidence = if kind == IntentKind::Unknown || state.update_count == 0 {
        0.0
    } else {
        clamp01(summary.edge * certainty * persistence * (1.0 - conflict))
    };
    let urgency = world_intent_urgency(state);
    let strength = build_intent_strength(
        active_abs_dim(state, INST_FLOW).max(active_abs_dim(state, RETAIL_FLOW)),
        active_abs_dim(state, STRESS),
        persistence,
        active_abs_dim(state, SYNCHRONY),
        conflict,
    );
    let state_label = classify_intent_state(kind, &summary, conflict);
    let surprise = previous
        .and_then(|prior| posterior.kl_divergence(prior))
        .unwrap_or(0.0);
    let rationale = format_world_intent_rationale(state, certainty, &summary, posterior, surprise);
    let expectation_bindings = world_intent_expectations(kind, direction, state, summary.edge);
    let falsifiers = world_intent_falsifiers(kind);

    IntentHypothesis {
        intent_id: format!("world_intent:{}:{}", state.market, state.last_tick),
        kind,
        scope: ReasoningScope::market(),
        direction,
        state: state_label,
        confidence: decimal01(confidence),
        urgency: decimal01(urgency),
        persistence: decimal01(persistence),
        conflict_score: decimal01(conflict),
        strength,
        propagation_targets: vec![],
        supporting_archetypes: vec![],
        supporting_case_signature: None,
        expectation_bindings,
        expectation_violations: vec![],
        exit_signals: vec![],
        opportunities: vec![IntentOpportunityWindow::new(
            opportunity_bucket(urgency, persistence),
            opportunity_urgency(urgency),
            opportunity_bias(kind, state_label, confidence, conflict),
            decimal01(confidence),
            decimal01(1.0 - conflict),
            rationale.clone(),
        )],
        falsifiers,
        rationale,
    }
}

pub fn format_world_intent_line(intent: &IntentHypothesis) -> String {
    format!(
        "world_intent: id={} kind={} direction={} state={} confidence={} urgency={} persistence={} conflict={} strength={}",
        intent.intent_id,
        intent_kind_label(intent.kind),
        intent_direction_label(intent.direction),
        intent_state_label(intent.state),
        intent.confidence,
        intent.urgency,
        intent.persistence,
        intent.conflict_score,
        intent.strength.composite,
    )
}

pub fn format_world_reflection_line(intent: &IntentHypothesis) -> Option<String> {
    let expectation = intent.expectation_bindings.first()?;
    let falsifier = intent
        .falsifiers
        .first()
        .map(String::as_str)
        .unwrap_or("none");
    let violation = intent
        .expectation_violations
        .first()
        .map(|item| item.description.as_str())
        .unwrap_or("none");
    Some(format!(
        "world_reflection: id={} belief={} expectation={} falsifier={} violation={} confidence={} conflict={}",
        intent.intent_id,
        intent_kind_label(intent.kind),
        expectation.rationale,
        falsifier,
        violation,
        intent.confidence,
        intent.conflict_score,
    ))
}

pub fn apply_world_intent_to_perception_graph(
    state: &LatentWorldState,
    intent: &IntentHypothesis,
    graph: &mut PerceptionGraph,
) {
    graph.world_intent.upsert(
        state.market,
        WorldIntentSnapshot {
            intent_id: intent.intent_id.clone(),
            kind: intent.kind,
            direction: intent.direction,
            state: intent.state,
            confidence: intent.confidence,
            urgency: intent.urgency,
            persistence: intent.persistence,
            conflict_score: intent.conflict_score,
            strength: intent.strength.composite,
            rationale: intent.rationale.clone(),
            top_expectation: intent
                .expectation_bindings
                .first()
                .map(|item| item.rationale.clone()),
            top_falsifier: intent.falsifiers.first().cloned(),
            expectation_count: intent.expectation_bindings.len(),
            top_violation: intent
                .expectation_violations
                .first()
                .map(|item| item.description.clone()),
            violation_count: intent.expectation_violations.len(),
            last_tick: state.last_tick,
        },
    );
}

// ---------------------------------------------------------------------------
// Observation aggregator — turn pressure field + belief field stats
// into the 5-dim observation the SSM expects.
// ---------------------------------------------------------------------------

/// Compute a WorldObservation from per-tick aggregates. Dimensions
/// whose inputs are absent come back as masked.
pub fn aggregate_observation(inputs: &ObservationInputs) -> WorldObservation {
    let mut values = [0.0_f64; LATENT_DIM];
    let mut mask = [false; LATENT_DIM];

    if let Some(v) = inputs.market_stress {
        values[0] = v;
        mask[0] = true;
    }
    if let Some(v) = inputs.breadth {
        values[1] = v;
        mask[1] = true;
    }
    if let Some(v) = inputs.synchrony {
        values[2] = v;
        mask[2] = true;
    }
    if let Some(v) = inputs.institutional_flow {
        values[3] = v;
        mask[3] = true;
    }
    if let Some(v) = inputs.retail_flow {
        values[4] = v;
        mask[4] = true;
    }
    WorldObservation { values, mask }
}

/// Raw inputs from the runtime; each is Optional because early ticks
/// won't have e.g. informed intent belief.
#[derive(Debug, Clone, Copy, Default)]
pub struct ObservationInputs {
    pub market_stress: Option<f64>,
    pub breadth: Option<f64>,
    pub synchrony: Option<f64>,
    pub institutional_flow: Option<f64>,
    pub retail_flow: Option<f64>,
}

pub fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

pub fn signed_breadth_signal(breadth_up: Decimal, breadth_down: Decimal) -> f64 {
    clamp_signed_unit(decimal_to_f64(breadth_up - breadth_down))
}

pub fn mean_decimal_signal(values: impl IntoIterator<Item = Decimal>) -> Option<f64> {
    let mut count = 0_i64;
    let mut total = Decimal::ZERO;
    for value in values {
        total += value;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some(clamp_signed_unit(decimal_to_f64(
        total / Decimal::from(count),
    )))
}

#[derive(Debug, Clone, Copy)]
struct IntentPosteriorSummary {
    kind: IntentKind,
    best_prob: f64,
    runner_up_prob: f64,
    margin: f64,
    entropy: f64,
    edge: f64,
}

fn world_intent_prior() -> CategoricalBelief<IntentKind> {
    CategoricalBelief::uniform(WORLD_INTENT_VARIANTS.to_vec())
}

fn summarize_world_intent_posterior(
    posterior: &CategoricalBelief<IntentKind>,
) -> IntentPosteriorSummary {
    let mut best = (IntentKind::Unknown, 0.0_f64);
    let mut runner_up = 0.0_f64;
    for (variant, prob) in posterior.variants.iter().zip(posterior.probs.iter()) {
        let prob = decimal_to_f64(*prob);
        if prob > best.1 {
            runner_up = best.1;
            best = (*variant, prob);
        } else if prob > runner_up {
            runner_up = prob;
        }
    }
    let prior = 1.0 / WORLD_INTENT_COUNT as f64;
    let entropy = posterior.entropy().unwrap_or(0.0);
    IntentPosteriorSummary {
        kind: best.0,
        best_prob: best.1,
        runner_up_prob: runner_up,
        margin: clamp01(best.1 - runner_up),
        entropy,
        edge: clamp01((best.1 - prior) / (1.0 - prior)),
    }
}

fn world_intent_likelihoods(state: &LatentWorldState) -> [f64; WORLD_INTENT_COUNT] {
    let stress_pos = evidence_positive(state, STRESS);
    let stress_active = evidence_active_abs(state, STRESS);
    let breadth_pos = evidence_positive(state, BREADTH);
    let breadth_neg = evidence_negative(state, BREADTH);
    let breadth_compressed = evidence_neutral(state, BREADTH).max(breadth_neg);
    let synchrony_active = evidence_active_abs(state, SYNCHRONY);
    let inst_pos = evidence_positive(state, INST_FLOW);
    let inst_neg = evidence_negative(state, INST_FLOW);
    let inst_neutral = evidence_neutral(state, INST_FLOW);
    let retail_neg = evidence_negative(state, RETAIL_FLOW);
    let retail_neutral = evidence_neutral(state, RETAIL_FLOW);
    let unknown = joint_likelihood(&[
        evidence_neutral(state, STRESS),
        evidence_neutral(state, BREADTH),
        evidence_neutral(state, SYNCHRONY),
        evidence_neutral(state, INST_FLOW),
        evidence_neutral(state, RETAIL_FLOW),
    ]);

    [
        joint_likelihood(&[
            inst_pos,
            breadth_pos,
            evidence_neutral(state, STRESS),
            retail_neutral.max(evidence_positive(state, RETAIL_FLOW)),
        ]),
        joint_likelihood(&[
            inst_neg,
            breadth_neg,
            stress_pos.max(evidence_neutral(state, STRESS)),
        ]),
        joint_likelihood(&[
            stress_pos,
            synchrony_active,
            breadth_neg,
            inst_neg.max(retail_neg),
        ]),
        joint_likelihood(&[
            stress_active,
            synchrony_active,
            inst_neutral,
            retail_neutral,
        ]),
        joint_likelihood(&[
            stress_pos,
            inst_pos,
            breadth_compressed,
            evidence_neutral(state, SYNCHRONY),
        ]),
        unknown,
    ]
}

fn infer_intent_direction(kind: IntentKind, state: &LatentWorldState) -> IntentDirection {
    match kind {
        IntentKind::Accumulation => IntentDirection::Buy,
        IntentKind::Distribution | IntentKind::ForcedUnwind => IntentDirection::Sell,
        IntentKind::EventRepricing => repricing_direction(state),
        IntentKind::Absorption => IntentDirection::Neutral,
        _ => IntentDirection::Neutral,
    }
}

fn repricing_direction(state: &LatentWorldState) -> IntentDirection {
    let buy = geometric_mean(&[
        evidence_positive(state, BREADTH),
        evidence_positive(state, INST_FLOW),
        evidence_positive(state, RETAIL_FLOW),
    ]);
    let sell = geometric_mean(&[
        evidence_negative(state, BREADTH),
        evidence_negative(state, INST_FLOW),
        evidence_negative(state, RETAIL_FLOW),
    ]);
    let mixed = geometric_mean(&[
        evidence_neutral(state, BREADTH),
        evidence_neutral(state, INST_FLOW),
        evidence_neutral(state, RETAIL_FLOW),
    ]);
    if mixed >= buy.max(sell) {
        IntentDirection::Mixed
    } else if buy > sell {
        IntentDirection::Buy
    } else {
        IntentDirection::Sell
    }
}

fn classify_intent_state(
    kind: IntentKind,
    summary: &IntentPosteriorSummary,
    conflict: f64,
) -> IntentState {
    if kind == IntentKind::Unknown || summary.edge <= 0.0 {
        IntentState::Unknown
    } else if conflict > summary.best_prob {
        IntentState::AtRisk
    } else if summary.best_prob >= (1.0 / WORLD_INTENT_COUNT as f64) * 2.0
        && summary.margin >= 1.0 / WORLD_INTENT_COUNT as f64
    {
        IntentState::Active
    } else {
        IntentState::Forming
    }
}

fn build_intent_strength(
    flow_strength: f64,
    impact_strength: f64,
    persistence_strength: f64,
    propagation_strength: f64,
    resistance_strength: f64,
) -> IntentStrength {
    let composite = clamp01(
        ((flow_strength + impact_strength + persistence_strength + propagation_strength)
            / LATENT_DIM.saturating_sub(1) as f64)
            * (1.0 - resistance_strength),
    );
    IntentStrength {
        flow_strength: decimal01(flow_strength),
        impact_strength: decimal01(impact_strength),
        persistence_strength: decimal01(persistence_strength),
        propagation_strength: decimal01(propagation_strength),
        resistance_strength: decimal01(resistance_strength),
        composite: decimal01(composite),
    }
}

fn intent_conflict_score(state: &LatentWorldState) -> f64 {
    let flow_breadth = opposite_sign_evidence(state, INST_FLOW, BREADTH);
    let inst_retail = opposite_sign_evidence(state, INST_FLOW, RETAIL_FLOW);
    let uncertainty = 1.0 - latent_certainty(state);
    clamp01(flow_breadth.max(inst_retail).max(uncertainty))
}

fn opposite_sign_evidence(state: &LatentWorldState, a: usize, b: usize) -> f64 {
    let a_value = state.mean[a];
    let b_value = state.mean[b];
    if a_value.signum() == b_value.signum() {
        return 0.0;
    }
    geometric_mean(&[evidence_active_abs(state, a), evidence_active_abs(state, b)])
}

fn evidence_maturity(update_count: u32) -> f64 {
    let updates = update_count as f64;
    if updates <= 0.0 {
        0.0
    } else {
        updates / (updates + LATENT_DIM as f64)
    }
}

fn world_intent_urgency(state: &LatentWorldState) -> f64 {
    root_mean_square(&[
        evidence_active_abs(state, STRESS),
        evidence_active_abs(state, SYNCHRONY),
        active_abs_dim(state, INST_FLOW).max(active_abs_dim(state, RETAIL_FLOW)),
    ])
}

fn evidence_positive(state: &LatentWorldState, idx: usize) -> f64 {
    sigmoid(normalized_dim(state, idx))
}

fn evidence_negative(state: &LatentWorldState, idx: usize) -> f64 {
    sigmoid(-normalized_dim(state, idx))
}

fn evidence_neutral(state: &LatentWorldState, idx: usize) -> f64 {
    let z = normalized_dim(state, idx);
    (-0.5 * z * z).exp().clamp(NUMERIC_EPSILON, 1.0)
}

fn evidence_active_abs(state: &LatentWorldState, idx: usize) -> f64 {
    1.0 - evidence_neutral(state, idx)
}

fn active_abs_dim(state: &LatentWorldState, idx: usize) -> f64 {
    evidence_active_abs(state, idx)
}

fn normalized_dim(state: &LatentWorldState, idx: usize) -> f64 {
    let variance = state.covariance[idx][idx].max(0.0) + state.observation_noise[idx][idx].max(0.0);
    state.mean[idx] / variance.sqrt().max(NUMERIC_EPSILON)
}

fn sigmoid(value: f64) -> f64 {
    if !value.is_finite() {
        0.5
    } else {
        1.0 / (1.0 + (-value).exp())
    }
}

fn geometric_mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let log_sum = values
        .iter()
        .map(|value| value.clamp(NUMERIC_EPSILON, 1.0).ln())
        .sum::<f64>();
    (log_sum / values.len() as f64).exp()
}

fn joint_likelihood(values: &[f64]) -> f64 {
    values
        .iter()
        .fold(1.0, |acc, value| acc * value.clamp(NUMERIC_EPSILON, 1.0))
}

fn root_mean_square(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean_square = values
        .iter()
        .map(|value| clamp01(*value).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    mean_square.sqrt()
}

fn latent_certainty(state: &LatentWorldState) -> f64 {
    let avg_stdev = (0..LATENT_DIM)
        .map(|idx| state.covariance[idx][idx].max(0.0).sqrt())
        .sum::<f64>()
        / LATENT_DIM as f64;
    clamp01(1.0 / (1.0 + avg_stdev))
}

fn opportunity_bucket(urgency: f64, persistence: f64) -> HorizonBucket {
    if urgency >= 0.70 {
        HorizonBucket::Fast5m
    } else if persistence >= 0.70 {
        HorizonBucket::MultiSession
    } else if urgency >= 0.45 {
        HorizonBucket::Mid30m
    } else {
        HorizonBucket::Session
    }
}

fn opportunity_urgency(urgency: f64) -> Urgency {
    if urgency >= 0.70 {
        Urgency::Immediate
    } else if urgency <= 0.25 {
        Urgency::Relaxed
    } else {
        Urgency::Normal
    }
}

fn opportunity_bias(
    kind: IntentKind,
    state: IntentState,
    confidence: f64,
    conflict: f64,
) -> IntentOpportunityBias {
    if kind == IntentKind::Unknown {
        IntentOpportunityBias::Watch
    } else if state == IntentState::AtRisk || conflict >= 0.65 {
        IntentOpportunityBias::Exit
    } else if state == IntentState::Active && confidence >= 0.65 {
        IntentOpportunityBias::Hold
    } else {
        IntentOpportunityBias::Watch
    }
}

fn world_intent_expectations(
    kind: IntentKind,
    direction: IntentDirection,
    state: &LatentWorldState,
    strength: f64,
) -> Vec<ExpectationBinding> {
    match kind {
        IntentKind::Accumulation => vec![
            world_expectation(
                state,
                "accumulation_flow",
                ExpectationKind::Confirmation,
                "fast5m",
                strength,
                "institutional flow should remain constructive while breadth broadens",
            ),
            world_expectation(
                state,
                "accumulation_stress",
                ExpectationKind::Observation,
                "next_tick",
                strength,
                "stress should not rise against positive flow",
            ),
        ],
        IntentKind::Distribution => vec![
            world_expectation(
                state,
                "distribution_flow",
                ExpectationKind::Confirmation,
                "fast5m",
                strength,
                "institutional flow should remain negative while breadth narrows",
            ),
            world_expectation(
                state,
                "distribution_propagation",
                ExpectationKind::Propagation,
                "session",
                strength,
                "sell pressure should spread beyond the first affected cluster",
            ),
        ],
        IntentKind::ForcedUnwind => vec![
            world_expectation(
                state,
                "forced_unwind_sync",
                ExpectationKind::CoMovement,
                "next_tick",
                strength,
                "stress and synchrony should stay coupled across symbols",
            ),
            world_expectation(
                state,
                "forced_unwind_breadth",
                ExpectationKind::Propagation,
                "fast5m",
                strength,
                "breadth should remain under pressure unless flow stabilizes",
            ),
        ],
        IntentKind::EventRepricing => vec![
            world_expectation(
                state,
                "event_repricing_sync",
                ExpectationKind::CoMovement,
                "next_tick",
                strength,
                "synchrony should stay elevated until the repricing resolves",
            ),
            world_expectation(
                state,
                "event_repricing_direction",
                ExpectationKind::Confirmation,
                "fast5m",
                strength,
                &format!(
                    "flow or breadth should resolve toward {} instead of staying neutral",
                    intent_direction_label(direction),
                ),
            ),
        ],
        IntentKind::Absorption => vec![
            world_expectation(
                state,
                "absorption_flow",
                ExpectationKind::Confirmation,
                "fast5m",
                strength,
                "constructive flow should persist while downside breadth fails to expand",
            ),
            world_expectation(
                state,
                "absorption_sync",
                ExpectationKind::Observation,
                "next_tick",
                strength,
                "synchrony should stay contained rather than becoming forced unwind",
            ),
        ],
        IntentKind::Unknown => vec![world_expectation(
            state,
            "unknown_disambiguate",
            ExpectationKind::Observation,
            "next_tick",
            strength,
            "posterior should sharpen only after a latent dimension moves beyond its uncertainty",
        )],
        _ => vec![world_expectation(
            state,
            "generic_follow_through",
            ExpectationKind::Confirmation,
            "fast5m",
            strength,
            "the dominant intent should produce confirming observation before case escalation",
        )],
    }
}

fn world_expectation(
    state: &LatentWorldState,
    suffix: &str,
    kind: ExpectationKind,
    horizon: &str,
    strength: f64,
    rationale: &str,
) -> ExpectationBinding {
    ExpectationBinding {
        expectation_id: format!(
            "world_intent_expectation:{}:{}:{}",
            state.market, state.last_tick, suffix
        ),
        kind,
        scope: ReasoningScope::market(),
        target_scope: None,
        horizon: horizon.into(),
        strength: decimal01(strength),
        rationale: rationale.into(),
    }
}

fn world_intent_expectation_violations(
    previous: &IntentHypothesis,
    current: &IntentHypothesis,
    state: &LatentWorldState,
) -> Vec<ExpectationViolation> {
    if previous.kind == IntentKind::Unknown {
        return Vec::new();
    }

    let mut violations = Vec::new();
    match previous.kind {
        IntentKind::Accumulation => {
            push_world_violation(
                &mut violations,
                previous,
                "accumulation_flow",
                ExpectationViolationKind::FailedConfirmation,
                "accumulation flow expectation failed",
                "institutional flow or breadth turned against accumulation",
                evidence_negative(state, INST_FLOW).max(evidence_negative(state, BREADTH)),
                evidence_positive(state, INST_FLOW).max(evidence_positive(state, BREADTH)),
            );
            push_world_violation(
                &mut violations,
                previous,
                "accumulation_stress",
                ExpectationViolationKind::ModalConflict,
                "stress rose against accumulation",
                "stress rises while accumulation should be constructive",
                evidence_positive(state, STRESS),
                evidence_neutral(state, STRESS),
            );
        }
        IntentKind::Distribution => {
            push_world_violation(
                &mut violations,
                previous,
                "distribution_flow",
                ExpectationViolationKind::FailedConfirmation,
                "distribution flow expectation failed",
                "institutional flow or breadth recovered against distribution",
                evidence_positive(state, INST_FLOW).max(evidence_positive(state, BREADTH)),
                evidence_negative(state, INST_FLOW).max(evidence_negative(state, BREADTH)),
            );
            push_world_violation(
                &mut violations,
                previous,
                "distribution_propagation",
                ExpectationViolationKind::MissingPropagation,
                "distribution propagation failed",
                "sell pressure remained isolated instead of propagating",
                evidence_neutral(state, SYNCHRONY).max(evidence_positive(state, BREADTH)),
                evidence_active_abs(state, SYNCHRONY).max(evidence_negative(state, BREADTH)),
            );
        }
        IntentKind::ForcedUnwind => {
            push_world_violation(
                &mut violations,
                previous,
                "forced_unwind_sync",
                ExpectationViolationKind::FailedConfirmation,
                "forced unwind synchrony expectation failed",
                "stress and synchrony decoupled",
                evidence_neutral(state, STRESS).max(evidence_neutral(state, SYNCHRONY)),
                geometric_mean(&[
                    evidence_active_abs(state, STRESS),
                    evidence_active_abs(state, SYNCHRONY),
                ]),
            );
            push_world_violation(
                &mut violations,
                previous,
                "forced_unwind_breadth",
                ExpectationViolationKind::UnexpectedPropagation,
                "breadth recovered against forced unwind",
                "breadth recovered without delayed downside propagation",
                evidence_positive(state, BREADTH),
                evidence_negative(state, BREADTH),
            );
        }
        IntentKind::EventRepricing => {
            if !matches!(
                current.kind,
                IntentKind::Accumulation | IntentKind::Distribution | IntentKind::ForcedUnwind
            ) {
                push_world_violation(
                    &mut violations,
                    previous,
                    "event_repricing_sync",
                    ExpectationViolationKind::TimingMismatch,
                    "event repricing synchrony faded before resolution",
                    "synchrony decoupled without directional follow-through",
                    evidence_neutral(state, SYNCHRONY).max(evidence_neutral(state, STRESS)),
                    geometric_mean(&[
                        evidence_active_abs(state, STRESS),
                        evidence_active_abs(state, SYNCHRONY),
                    ]),
                );
            }
            if current.kind == IntentKind::Unknown {
                push_world_violation(
                    &mut violations,
                    previous,
                    "event_repricing_direction",
                    ExpectationViolationKind::FailedConfirmation,
                    "event repricing produced no directional follow-through",
                    "flow and breadth stayed neutral after repricing",
                    geometric_mean(&[
                        evidence_neutral(state, BREADTH),
                        evidence_neutral(state, INST_FLOW),
                    ]),
                    active_abs_dim(state, BREADTH).max(active_abs_dim(state, INST_FLOW)),
                );
            }
        }
        IntentKind::Absorption => {
            push_world_violation(
                &mut violations,
                previous,
                "absorption_sync",
                ExpectationViolationKind::UnexpectedPropagation,
                "absorption became broad synchrony",
                "synchrony expanded instead of staying contained",
                evidence_active_abs(state, SYNCHRONY),
                evidence_neutral(state, SYNCHRONY),
            );
            push_world_violation(
                &mut violations,
                previous,
                "absorption_flow",
                ExpectationViolationKind::FailedConfirmation,
                "absorption flow disappeared",
                "institutional flow disappeared while breadth stayed weak",
                evidence_negative(state, INST_FLOW).max(evidence_negative(state, BREADTH)),
                evidence_positive(state, INST_FLOW),
            );
        }
        _ => {}
    }

    if previous.kind != current.kind
        && current.kind != IntentKind::Unknown
        && !(previous.kind == IntentKind::EventRepricing
            && matches!(
                current.kind,
                IntentKind::Accumulation | IntentKind::Distribution
            ))
    {
        let current_confidence = decimal_to_f64(current.confidence);
        if current_confidence > 0.0 {
            violations.push(ExpectationViolation {
                kind: ExpectationViolationKind::ModalConflict,
                expectation_id: previous
                    .expectation_bindings
                    .first()
                    .map(|binding| binding.expectation_id.clone()),
                description: format!(
                    "posterior shifted from {} to {}",
                    intent_kind_label(previous.kind),
                    intent_kind_label(current.kind),
                ),
                magnitude: decimal01(current_confidence),
                falsifier: previous.falsifiers.first().cloned(),
            });
        }
    }

    violations.sort_by(|a, b| {
        b.magnitude
            .partial_cmp(&a.magnitude)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    violations.truncate(2);
    violations
}

fn push_world_violation(
    violations: &mut Vec<ExpectationViolation>,
    previous: &IntentHypothesis,
    expectation_suffix: &str,
    kind: ExpectationViolationKind,
    description: &str,
    falsifier: &str,
    counter_evidence: f64,
    support_evidence: f64,
) {
    let magnitude = clamp01(counter_evidence - support_evidence);
    if magnitude <= 0.0 {
        return;
    }
    violations.push(ExpectationViolation {
        kind,
        expectation_id: previous_expectation_id(previous, expectation_suffix),
        description: description.into(),
        magnitude: decimal01(magnitude),
        falsifier: Some(falsifier.into()),
    });
}

fn previous_expectation_id(
    previous: &IntentHypothesis,
    expectation_suffix: &str,
) -> Option<String> {
    previous
        .expectation_bindings
        .iter()
        .find(|binding| binding.expectation_id.ends_with(expectation_suffix))
        .or_else(|| previous.expectation_bindings.first())
        .map(|binding| binding.expectation_id.clone())
}

fn world_intent_falsifiers(kind: IntentKind) -> Vec<String> {
    match kind {
        IntentKind::Accumulation => vec![
            "institutional flow turns non-positive".into(),
            "breadth turns negative while stress rises".into(),
            "synchrony fails to propagate beyond the first cluster".into(),
        ],
        IntentKind::Distribution => vec![
            "institutional flow recovers positive".into(),
            "breadth broadens while stress mean-reverts".into(),
            "sell pressure remains isolated instead of propagating".into(),
        ],
        IntentKind::ForcedUnwind => vec![
            "stress and synchrony both mean-revert".into(),
            "institutional flow turns positive".into(),
            "breadth recovers without delayed downside propagation".into(),
        ],
        IntentKind::EventRepricing => vec![
            "stress mean-reverts faster than synchrony propagates".into(),
            "synchrony decouples across symbols".into(),
            "flow and breadth show no follow-through".into(),
        ],
        IntentKind::Absorption => vec![
            "stress fades without persistent flow pressure".into(),
            "breadth expands instead of staying compressed".into(),
            "institutional flow disappears".into(),
        ],
        _ => vec!["posterior stays diffuse after covariance falls".into()],
    }
}

fn format_world_intent_rationale(
    state: &LatentWorldState,
    certainty: f64,
    summary: &IntentPosteriorSummary,
    posterior: &CategoricalBelief<IntentKind>,
    surprise: f64,
) -> String {
    let mut parts = Vec::with_capacity(LATENT_DIM + 1);
    for (idx, name) in LATENT_NAMES.iter().enumerate() {
        let stdev = state.covariance[idx][idx].max(0.0).sqrt();
        parts.push(format!("{}={:+.2}±{:.2}", name, state.mean[idx], stdev));
    }
    parts.push(format!("certainty={:.2}", certainty));
    parts.push(format!("intent_edge={:.2}", summary.edge));
    parts.push(format!("intent_margin={:.2}", summary.margin));
    parts.push(format!("intent_entropy={:.2}", summary.entropy));
    parts.push(format!("intent_surprise={:.2}", surprise));
    parts.push(format!("runner_up={:.2}", summary.runner_up_prob));
    parts.push(format_world_intent_posterior(posterior));
    format!("latent posterior {}", parts.join(" "))
}

fn format_world_intent_posterior(posterior: &CategoricalBelief<IntentKind>) -> String {
    let mut parts = Vec::with_capacity(posterior.variants.len());
    for (variant, prob) in posterior.variants.iter().zip(posterior.probs.iter()) {
        parts.push(format!(
            "{}={:.2}",
            intent_kind_label(*variant),
            decimal_to_f64(*prob)
        ));
    }
    format!("intent_posterior[{}]", parts.join(","))
}

fn intent_kind_label(kind: IntentKind) -> &'static str {
    match kind {
        IntentKind::Accumulation => "accumulation",
        IntentKind::Distribution => "distribution",
        IntentKind::ForcedUnwind => "forced_unwind",
        IntentKind::PassiveRebalance => "passive_rebalance",
        IntentKind::EventRepricing => "event_repricing",
        IntentKind::FailedPropagation => "failed_propagation",
        IntentKind::CrossMarketLead => "cross_market_lead",
        IntentKind::Absorption => "absorption",
        IntentKind::Unknown => "unknown",
    }
}

fn intent_direction_label(direction: IntentDirection) -> &'static str {
    match direction {
        IntentDirection::Buy => "buy",
        IntentDirection::Sell => "sell",
        IntentDirection::Mixed => "mixed",
        IntentDirection::Neutral => "neutral",
    }
}

fn intent_state_label(state: IntentState) -> &'static str {
    match state {
        IntentState::Forming => "forming",
        IntentState::Active => "active",
        IntentState::AtRisk => "at_risk",
        IntentState::Exhausted => "exhausted",
        IntentState::Invalidated => "invalidated",
        IntentState::Fulfilled => "fulfilled",
        IntentState::Unknown => "unknown",
    }
}

fn clamp01(value: f64) -> f64 {
    if !value.is_finite() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

fn decimal01(value: f64) -> Decimal {
    Decimal::from_f64_retain(clamp01(value))
        .unwrap_or(Decimal::ZERO)
        .round_dp(4)
}

fn decimal_positive(value: f64) -> Decimal {
    Decimal::from_f64_retain(value.max(NUMERIC_EPSILON))
        .unwrap_or(Decimal::ONE)
        .round_dp(8)
}

fn clamp_signed_unit(value: f64) -> f64 {
    if !value.is_finite() {
        0.0
    } else {
        value.clamp(-1.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Matrix helpers — small fixed-size 5x5 inlined arithmetic. No nalgebra
// dep to keep the build light; we only need this specific size.
// ---------------------------------------------------------------------------

fn identity5() -> Mat5 {
    let mut m = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        m[i][i] = 1.0;
    }
    m
}

fn scaled_identity5(s: f64) -> Mat5 {
    let mut m = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        m[i][i] = s;
    }
    m
}

fn mat_mul(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            let mut s = 0.0;
            for k in 0..LATENT_DIM {
                s += a[i][k] * b[k][j];
            }
            out[i][j] = s;
        }
    }
    out
}

/// Multiply a by b^T.
fn mat_mul_t(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            let mut s = 0.0;
            for k in 0..LATENT_DIM {
                s += a[i][k] * b[j][k];
            }
            out[i][j] = s;
        }
    }
    out
}

fn mat_add(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            out[i][j] = a[i][j] + b[i][j];
        }
    }
    out
}

fn mat_sub(a: &Mat5, b: &Mat5) -> Mat5 {
    let mut out = [[0.0_f64; LATENT_DIM]; LATENT_DIM];
    for i in 0..LATENT_DIM {
        for j in 0..LATENT_DIM {
            out[i][j] = a[i][j] - b[i][j];
        }
    }
    out
}

fn mat_vec(a: &Mat5, v: &Vec5) -> Vec5 {
    let mut out = [0.0_f64; LATENT_DIM];
    for i in 0..LATENT_DIM {
        let mut s = 0.0;
        for k in 0..LATENT_DIM {
            s += a[i][k] * v[k];
        }
        out[i] = s;
    }
    out
}

/// 5×5 matrix inverse via Gauss-Jordan with partial pivoting.
/// Returns None if singular (any pivot < 1e-12).
fn invert_5x5(m: &Mat5) -> Option<Mat5> {
    const N: usize = LATENT_DIM;
    let mut aug = [[0.0_f64; 2 * N]; N];
    for i in 0..N {
        for j in 0..N {
            aug[i][j] = m[i][j];
        }
        aug[i][N + i] = 1.0;
    }
    for col in 0..N {
        // partial pivoting
        let mut pivot = col;
        for row in col + 1..N {
            if aug[row][col].abs() > aug[pivot][col].abs() {
                pivot = row;
            }
        }
        if aug[pivot][col].abs() < 1e-12 {
            return None;
        }
        if pivot != col {
            aug.swap(col, pivot);
        }
        // normalize pivot row
        let pv = aug[col][col];
        for j in 0..2 * N {
            aug[col][j] /= pv;
        }
        // eliminate other rows
        for row in 0..N {
            if row == col {
                continue;
            }
            let factor = aug[row][col];
            for j in 0..2 * N {
                aug[row][j] -= factor * aug[col][j];
            }
        }
    }
    let mut inv = [[0.0_f64; N]; N];
    for i in 0..N {
        for j in 0..N {
            inv[i][j] = aug[i][N + j];
        }
    }
    Some(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::PerceptionGraph;

    #[test]
    fn identity_inverse_roundtrip() {
        let i5 = identity5();
        let inv = invert_5x5(&i5).unwrap();
        for r in 0..LATENT_DIM {
            for c in 0..LATENT_DIM {
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!((inv[r][c] - expected).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn diagonal_inverse_correct() {
        let mut m = [[0.0_f64; 5]; 5];
        for i in 0..5 {
            m[i][i] = (i as f64) + 2.0;
        }
        let inv = invert_5x5(&m).unwrap();
        for i in 0..5 {
            let expected = 1.0 / ((i as f64) + 2.0);
            assert!((inv[i][i] - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn singular_matrix_returns_none() {
        let mut m = [[0.0_f64; 5]; 5];
        m[0][0] = 1.0;
        m[1][1] = 1.0;
        // row 2, 3, 4 all zero → singular
        assert!(invert_5x5(&m).is_none());
    }

    #[test]
    fn first_observation_dominates_prior() {
        let mut state = LatentWorldState::new(Market::Hk);
        let obs = WorldObservation {
            values: [0.8, 0.2, 0.5, 0.1, -0.1],
            mask: [true; 5],
        };
        state.step(1, obs);
        // With R=0.1 I and P0=1.0 I, Kalman gain pulls mean ~80-90%
        // toward observation on first step. Assert each dim moved past
        // half of the observation value.
        for i in 0..5 {
            assert!(
                state.mean[i].abs() > obs.values[i].abs() * 0.4,
                "dim {} mean={} obs={}",
                i,
                state.mean[i],
                obs.values[i]
            );
            assert!(
                state.mean[i].signum() == obs.values[i].signum() || obs.values[i].abs() < 1e-9,
                "dim {} sign mismatch",
                i
            );
        }
    }

    #[test]
    fn missing_observation_does_not_shift_mean_on_that_dim() {
        let mut state = LatentWorldState::new(Market::Hk);
        // Step 1: all observed, mean moves.
        state.step(
            1,
            WorldObservation {
                values: [0.5; 5],
                mask: [true; 5],
            },
        );
        let after1 = state.mean;

        // Step 2: only dim 0 observed, others masked.
        state.step(
            2,
            WorldObservation {
                values: [0.9, 0.0, 0.0, 0.0, 0.0],
                mask: [true, false, false, false, false],
            },
        );
        // Dim 0 should continue toward 0.9. Dims 1-4 should mean-
        // revert (0.95 * previous) since only transition runs.
        assert!(state.mean[0] > after1[0]);
        for i in 1..5 {
            // Without evidence, mean drifts toward 0 via 0.95 factor.
            assert!(
                state.mean[i] < after1[i],
                "dim {}: should mean-revert, got {} vs {}",
                i,
                state.mean[i],
                after1[i]
            );
        }
    }

    #[test]
    fn variance_shrinks_under_consistent_observations() {
        let mut state = LatentWorldState::new(Market::Hk);
        let obs = WorldObservation {
            values: [0.5; 5],
            mask: [true; 5],
        };
        let v0 = state.covariance[0][0];
        for t in 1..=20 {
            state.step(t, obs);
        }
        let v20 = state.covariance[0][0];
        assert!(v20 < v0 * 0.5, "variance should shrink: {} → {}", v0, v20);
    }

    #[test]
    fn aggregate_observation_masks_missing() {
        let inputs = ObservationInputs {
            market_stress: Some(0.4),
            breadth: None,
            synchrony: Some(0.7),
            institutional_flow: None,
            retail_flow: Some(-0.2),
        };
        let obs = aggregate_observation(&inputs);
        assert_eq!(obs.mask, [true, false, true, false, true]);
        assert!((obs.values[0] - 0.4).abs() < 1e-9);
        assert!((obs.values[2] - 0.7).abs() < 1e-9);
        assert!((obs.values[4] - (-0.2)).abs() < 1e-9);
    }

    #[test]
    fn decimal_signal_helpers_clamp_and_average_existing_metrics() {
        assert!((signed_breadth_signal(Decimal::ONE, Decimal::ZERO) - 1.0).abs() < 1e-9);
        assert!((signed_breadth_signal(Decimal::ZERO, Decimal::ONE) + 1.0).abs() < 1e-9);
        assert!(mean_decimal_signal(std::iter::empty()).is_none());

        let mean = mean_decimal_signal([Decimal::ONE, Decimal::new(-5, 1), Decimal::ZERO])
            .expect("non-empty iterator");
        assert!((mean - (1.0 - 0.5) / 3.0).abs() < 1e-9);
    }

    #[test]
    fn summary_line_is_greppable() {
        let mut state = LatentWorldState::new(Market::Hk);
        state.step(
            5,
            WorldObservation {
                values: [0.42, 0.13, 0.85, 0.23, -0.08],
                mask: [true; 5],
            },
        );
        let line = state.summary_line();
        assert!(line.starts_with("world_state:"));
        assert!(line.contains("tick=5"));
        assert!(line.contains("updates=1"));
        assert!(line.contains("stress="));
        assert!(line.contains("breadth="));
        assert!(line.contains("synchrony="));
        assert!(line.contains("inst_flow="));
        assert!(line.contains("retail_flow="));
        // Each dim has ±stdev notation
        assert!(line.contains("±"));
    }

    #[test]
    fn mean_reversion_pulls_toward_zero_with_no_observations() {
        let mut state = LatentWorldState::new(Market::Hk);
        state.step(
            1,
            WorldObservation {
                values: [1.0; 5],
                mask: [true; 5],
            },
        );
        let after_obs = state.mean;
        // 20 ticks with no observations — mean should decay toward 0.
        for t in 2..=21 {
            state.step(t, WorldObservation::all_missing());
        }
        for i in 0..5 {
            assert!(state.mean[i].abs() < after_obs[i].abs() * 0.5);
        }
    }

    #[test]
    fn world_intent_infers_accumulation_from_positive_institutional_flow() {
        let state = steady_state_from_observation([0.10, 0.35, 0.25, 0.80, 0.10], 8);
        let intent = infer_world_intent(&state);

        assert_eq!(intent.kind, IntentKind::Accumulation);
        assert_eq!(intent.direction, IntentDirection::Buy);
        assert!(intent.confidence > Decimal::ZERO);
        assert!(!intent.expectation_bindings.is_empty());
        assert!(!intent.opportunities.is_empty());
        assert!(intent.rationale.contains("latent posterior"));
    }

    #[test]
    fn world_intent_infers_distribution_from_negative_institutional_flow() {
        let state = steady_state_from_observation([0.20, -0.35, 0.25, -0.80, -0.15], 8);
        let intent = state.dominant_world_intent();

        assert_eq!(intent.kind, IntentKind::Distribution);
        assert_eq!(intent.direction, IntentDirection::Sell);
        assert!(intent
            .falsifiers
            .iter()
            .any(|item| item.contains("institutional flow recovers")));
    }

    #[test]
    fn world_intent_belief_accumulates_repeated_soft_evidence() {
        let mut state = LatentWorldState::new(Market::Hk);
        let mut belief = WorldIntentBelief::new(Market::Hk);
        let mut intent = None;
        for tick in 1..=6 {
            state.step(
                tick,
                WorldObservation {
                    values: [0.10, 0.35, 0.25, 0.80, 0.10],
                    mask: [true; LATENT_DIM],
                },
            );
            intent = Some(belief.observe_state(&state));
        }
        let intent = intent.expect("intent emitted");

        let accum_idx = belief
            .posterior()
            .variants
            .iter()
            .position(|kind| *kind == IntentKind::Accumulation)
            .unwrap();
        let unknown_idx = belief
            .posterior()
            .variants
            .iter()
            .position(|kind| *kind == IntentKind::Unknown)
            .unwrap();

        assert_eq!(intent.kind, IntentKind::Accumulation);
        assert!(belief.posterior().probs[accum_idx] > belief.posterior().probs[unknown_idx]);
        assert_eq!(belief.posterior().sample_count, 6);
        assert!(intent.rationale.contains("intent_posterior["));
    }

    #[test]
    fn world_intent_reflection_surfaces_expectation_and_falsifier() {
        let state = steady_state_from_observation([0.80, 0.05, 0.75, 0.05, 0.00], 20);
        let intent = infer_world_intent(&state);
        let line = format_world_reflection_line(&intent).expect("reflection line");

        assert!(line.contains("world_reflection:"));
        assert!(line.contains("expectation="));
        assert!(line.contains("falsifier="));
        assert!(line.contains("violation=none"));
        assert!(intent
            .expectation_bindings
            .iter()
            .any(|item| item.kind == ExpectationKind::CoMovement));
    }

    #[test]
    fn world_intent_belief_flags_previous_expectation_violation() {
        let mut state = LatentWorldState::new(Market::Hk);
        let mut belief = WorldIntentBelief::new(Market::Hk);
        for tick in 1..=5 {
            state.step(
                tick,
                WorldObservation {
                    values: [0.10, 0.35, 0.25, 0.80, 0.10],
                    mask: [true; LATENT_DIM],
                },
            );
            let _ = belief.observe_state(&state);
        }

        let mut intent = None;
        for tick in 6..=12 {
            state.step(
                tick,
                WorldObservation {
                    values: [0.20, -0.35, 0.25, -0.80, -0.15],
                    mask: [true; LATENT_DIM],
                },
            );
            let next_intent = belief.observe_state(&state);
            if !next_intent.expectation_violations.is_empty() {
                intent = Some(next_intent);
                break;
            }
        }
        let intent = intent.expect("expect previous world intent to be falsified");
        let line = format_world_reflection_line(&intent).expect("reflection line");

        assert!(!intent.expectation_violations.is_empty());
        assert!(intent
            .expectation_violations
            .iter()
            .any(|violation| matches!(
                violation.kind,
                ExpectationViolationKind::FailedConfirmation
                    | ExpectationViolationKind::ModalConflict
            )));
        assert!(intent
            .expectation_violations
            .iter()
            .any(|violation| violation.magnitude > Decimal::ZERO));
        assert!(line.contains("violation="));
        assert!(!line.contains("violation=none"));
    }

    #[test]
    fn world_intent_infers_event_repricing_from_stress_and_synchrony() {
        let state = steady_state_from_observation([0.80, 0.05, 0.75, 0.05, 0.00], 20);
        let intent = infer_world_intent(&state);

        assert_eq!(intent.kind, IntentKind::EventRepricing);
        assert_eq!(intent.direction, IntentDirection::Mixed);
        assert_eq!(intent.state, IntentState::Active);
        assert!(format_world_intent_line(&intent).contains("kind=event_repricing"));
    }

    #[test]
    fn world_intent_writes_typed_perception_graph_snapshot() {
        let state = steady_state_from_observation([0.80, 0.05, 0.75, 0.05, 0.00], 20);
        let intent = infer_world_intent(&state);
        let mut graph = PerceptionGraph::new();

        apply_world_intent_to_perception_graph(&state, &intent, &mut graph);

        let view = graph.world(Market::Hk);
        let snap = view.world_intent.expect("world intent snapshot");
        assert_eq!(snap.kind, intent.kind);
        assert_eq!(snap.direction, intent.direction);
        assert_eq!(snap.confidence, intent.confidence);
        assert_eq!(
            snap.top_expectation.as_deref(),
            intent
                .expectation_bindings
                .first()
                .map(|item| item.rationale.as_str())
        );
        assert_eq!(
            snap.top_falsifier.as_deref(),
            intent.falsifiers.first().map(String::as_str)
        );
        assert_eq!(snap.expectation_count, intent.expectation_bindings.len());
        assert_eq!(snap.top_violation, None);
        assert_eq!(snap.violation_count, 0);
        assert_eq!(snap.last_tick, state.last_tick);
    }

    #[test]
    fn world_intent_keeps_flat_state_unknown() {
        let state = LatentWorldState::new(Market::Us);
        let intent = infer_world_intent(&state);

        assert_eq!(intent.kind, IntentKind::Unknown);
        assert_eq!(intent.direction, IntentDirection::Neutral);
        assert_eq!(intent.state, IntentState::Unknown);
        assert_eq!(intent.confidence, Decimal::ZERO);
        assert_eq!(intent.opportunities[0].bias, IntentOpportunityBias::Watch);
    }

    fn steady_state_from_observation(values: [f64; LATENT_DIM], ticks: u64) -> LatentWorldState {
        let mut state = LatentWorldState::new(Market::Hk);
        for tick in 1..=ticks {
            state.step(
                tick,
                WorldObservation {
                    values,
                    mask: [true; LATENT_DIM],
                },
            );
        }
        state
    }
}
