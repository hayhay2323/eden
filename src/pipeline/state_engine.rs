use std::collections::{BTreeMap, BTreeSet, HashSet};

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::agent::AgentTransition;
use crate::live_snapshot::{
    LiveClusterState, LiveMarket, LiveSignal, LiveTacticalCase, LiveWorldSummary,
};
use crate::ontology::{
    action_direction_case_label, action_direction_from_case_label,
    action_direction_from_title_prefix,
    world::{
        AttentionAllocation, PerceptualEvidence, PerceptualEvidencePolarity, PerceptualExpectation,
        PerceptualExpectationKind, PerceptualExpectationStatus, PerceptualState,
        PerceptualUncertainty,
    },
    IntentKind, IntentState, ReasoningScope, Symbol,
};

// =========================================================================
// Classification thresholds (Y#4 — documented, not free parameters)
// =========================================================================
//
// These constants name the operational definitions used by
// `CurrentStateFacts` and `build_symbol_state`. Each one encodes either a
// statistical posture ("supermajority of channels", "more than half of peers")
// or a count-based sanity floor. None of them are free parameters to tune per
// market — changing a value means changing the meaning, not the calibration.
//
// Y-70 stance (feedback_y_70_percent): threshold removal is NOT the goal.
// Pure Y would ideally let topology alone decide these, but Eden trades
// explainability for that purity. Surfacing the semantic label (e.g.
// RAW_SUPERMAJORITY) lets reason_codes stay grounded while future
// topology-emergence improvements can replace individual constants without
// touching the callers.

// Ratio of channels that must agree for raw_confirmed. Supermajority (4/5).
// Used for the Continuation classification gate and the
// "supermajority_raw_support" supporting evidence.
const RAW_SUPERMAJORITY: Decimal = dec!(0.80);
// Weighted-support floor paired with RAW_SUPERMAJORITY — either clears
// (unweighted supermajority OR substantial weighted support) satisfies
// raw_confirmed.
const RAW_WEIGHT_FLOOR_STRONG: Decimal = dec!(1.60);
// Lower weighted-support floor used for the "thin raw confirmation"
// opposing-evidence line. Below 0.80 weighted channels, the confirmation
// is considered brittle even if count-wise the floor is crossed.
const RAW_WEIGHT_FLOOR_WEAK: Decimal = dec!(0.80);
// Ratio of channels agreeing that marks "at least 2/3 agree". Below this,
// raw_missing fires; above, raw_confirmed is plausible pending the
// supermajority gate. Also used as propagation_confirmed's raw proxy.
const RAW_MAJORITY: Decimal = dec!(0.67);
// Ratio of peers confirming — "2/3 of the peer cohort". Above this,
// peer_confirmed fires.
const PEER_MAJORITY: Decimal = dec!(0.67);
// Ratio of peers confirming — "at least half". Below this, peer_missing
// fires (peer cohort is silent or contradicting).
const PEER_HALF: Decimal = dec!(0.50);
// Ratio of peers confirming — "near unanimous". Above this, cluster_expanded
// fires (the cluster is actively gaining members).
const PEER_NEAR_UNANIMOUS: Decimal = dec!(0.85);
// Signal-strength floor used for the latent-state gate and the
// sensor_level_residual_pressure supporting evidence. Derived from the
// residual-dimension normalization: a composite signal >= 0.35 means the
// residual has at least one dimension carrying material energy rather than
// floating near zero. Not a calibrated threshold per market.
const SIGNAL_STRENGTH_LATENT: Decimal = dec!(0.35);
// Minimal signal-strength floor used only by the state-memory restoration
// path — lower than SIGNAL_STRENGTH_LATENT because restoration is allowed
// to fire on weaker evidence when the prior state was meaningful.
const SIGNAL_STRENGTH_RESTORE_FLOOR: Decimal = dec!(0.15);
// Support fraction below which support is actively withdrawing (less than
// half the channels still agree). Used by facts.support_withdrawal.
const SUPPORT_WITHDRAWAL: Decimal = dec!(0.50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentStateKind {
    Continuation,
    TurningPoint,
    LowInformation,
    Conflicted,
    Latent,
}

impl PersistentStateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuation => "continuation",
            Self::TurningPoint => "turning_point",
            Self::LowInformation => "low_information",
            Self::Conflicted => "conflicted",
            Self::Latent => "latent",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "continuation" => Self::Continuation,
            "turning_point" => Self::TurningPoint,
            "conflicted" => Self::Conflicted,
            "latent" => Self::Latent,
            _ => Self::LowInformation,
        }
    }
}

impl std::fmt::Display for PersistentStateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentStateTrend {
    Strengthening,
    Stable,
    Weakening,
}

impl PersistentStateTrend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strengthening => "strengthening",
            Self::Stable => "stable",
            Self::Weakening => "weakening",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "strengthening" => Self::Strengthening,
            "weakening" => Self::Weakening,
            _ => Self::Stable,
        }
    }
}

impl std::fmt::Display for PersistentStateTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistentStateEvidence {
    pub code: String,
    pub summary: String,
    pub weight: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentExpectationKind {
    PeerFollowThrough,
    RawChannelConfirmation,
    ClusterExpansion,
    PropagationFollowThrough,
}

impl PersistentExpectationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PeerFollowThrough => "peer_follow_through",
            Self::RawChannelConfirmation => "raw_channel_confirmation",
            Self::ClusterExpansion => "cluster_expansion",
            Self::PropagationFollowThrough => "propagation_follow_through",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "peer_follow_through" => Self::PeerFollowThrough,
            "raw_channel_confirmation" => Self::RawChannelConfirmation,
            "cluster_expansion" => Self::ClusterExpansion,
            _ => Self::PropagationFollowThrough,
        }
    }
}

impl From<PersistentExpectationKind> for PerceptualExpectationKind {
    fn from(value: PersistentExpectationKind) -> Self {
        match value {
            PersistentExpectationKind::PeerFollowThrough => Self::PeerFollowThrough,
            PersistentExpectationKind::RawChannelConfirmation => Self::RawChannelConfirmation,
            PersistentExpectationKind::ClusterExpansion => Self::ClusterExpansion,
            PersistentExpectationKind::PropagationFollowThrough => Self::PropagationFollowThrough,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentExpectationStatus {
    Met,
    StillPending,
    Missed,
}

impl PersistentExpectationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Met => "met",
            Self::StillPending => "still_pending",
            Self::Missed => "missed",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "met" => Self::Met,
            "missed" => Self::Missed,
            _ => Self::StillPending,
        }
    }
}

impl From<PersistentExpectationStatus> for PerceptualExpectationStatus {
    fn from(value: PersistentExpectationStatus) -> Self {
        match value {
            PersistentExpectationStatus::Met => Self::Met,
            PersistentExpectationStatus::StillPending => Self::StillPending,
            PersistentExpectationStatus::Missed => Self::Missed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistentStateExpectation {
    pub kind: PersistentExpectationKind,
    pub status: PersistentExpectationStatus,
    pub rationale: String,
    pub pending_ticks: u16,
    /// Y#6 — continuous target that the expectation was measured against.
    /// For expectations that are inherently boolean
    /// (PropagationFollowThrough / ClusterExpansion) the value is None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<Decimal>,
    /// Y#6 — continuous observed value at this tick. `expected_value - observed_value`
    /// is the point-in-time prediction error (positive = underperforming).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_value: Option<Decimal>,
    /// Y#6 — exponentially weighted moving average of the prediction error
    /// across the expectation's lifetime for this symbol. Carried forward from
    /// the prior tick and blended with this tick's error at weight 0.2
    /// (EWMA alpha matching the direction_stability 5-tick characteristic
    /// time already used elsewhere). Used to shift the `expected_value` on
    /// the next tick so Eden actively learns which symbols routinely miss
    /// this expectation. `None` until the first quantitative evaluation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ewma_error: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistentSymbolState {
    pub state_id: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub label: String,
    pub state_kind: PersistentStateKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub age_ticks: u64,
    pub state_persistence_ticks: u16,
    pub direction_stability_rounds: u16,
    pub support_count: usize,
    pub contradict_count: usize,
    pub count_support_fraction: Decimal,
    pub weighted_support_fraction: Decimal,
    pub support_weight: Decimal,
    pub contradict_weight: Decimal,
    pub strength: Decimal,
    pub confidence: Decimal,
    pub trend: PersistentStateTrend,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_evidence: Vec<PersistentStateEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opposing_evidence: Vec<PersistentStateEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_evidence: Vec<PersistentStateEvidence>,
    pub conflict_age_ticks: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectations: Vec<PersistentStateExpectation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_setup_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_state: Option<String>,
    pub cluster_key: String,
    pub cluster_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
}

impl PersistentSymbolState {
    pub fn reason_codes(&self) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut codes = vec![self.state_kind.as_str().to_string()];
        seen.insert(self.state_kind.as_str().to_string());
        codes.push(format!("trend:{}", self.trend.as_str()));
        seen.insert(format!("trend:{}", self.trend.as_str()));

        for item in self
            .supporting_evidence
            .iter()
            .chain(self.opposing_evidence.iter())
            .chain(self.missing_evidence.iter())
        {
            if seen.insert(item.code.clone()) {
                codes.push(item.code.clone());
            }
        }
        for expectation in &self.expectations {
            let code = format!(
                "expectation:{}:{}",
                expectation.kind.as_str(),
                expectation.status.as_str()
            );
            if seen.insert(code.clone()) {
                codes.push(code);
            }
        }
        codes
    }

    pub fn scope(&self) -> ReasoningScope {
        ReasoningScope::Symbol(Symbol(self.symbol.clone()))
    }

    pub fn to_perceptual_state(&self) -> PerceptualState {
        let scope = self.scope();
        let supporting_evidence = self
            .supporting_evidence
            .iter()
            .enumerate()
            .map(|(index, evidence)| PerceptualEvidence {
                evidence_id: format!("{}:supports:{index}", self.state_id),
                target_scope: scope.clone(),
                source_scope: None,
                channel: perceptual_channel_for_code(&evidence.code).into(),
                polarity: PerceptualEvidencePolarity::Supports,
                weight: evidence.weight,
                rationale: evidence.summary.clone(),
            })
            .collect::<Vec<_>>();
        let opposing_evidence = self
            .opposing_evidence
            .iter()
            .enumerate()
            .map(|(index, evidence)| PerceptualEvidence {
                evidence_id: format!("{}:contradicts:{index}", self.state_id),
                target_scope: scope.clone(),
                source_scope: None,
                channel: perceptual_channel_for_code(&evidence.code).into(),
                polarity: PerceptualEvidencePolarity::Contradicts,
                weight: evidence.weight,
                rationale: evidence.summary.clone(),
            })
            .collect::<Vec<_>>();
        let missing_evidence = self
            .missing_evidence
            .iter()
            .enumerate()
            .map(|(index, evidence)| PerceptualEvidence {
                evidence_id: format!("{}:missing:{index}", self.state_id),
                target_scope: scope.clone(),
                source_scope: None,
                channel: perceptual_channel_for_code(&evidence.code).into(),
                polarity: PerceptualEvidencePolarity::Missing,
                weight: evidence.weight,
                rationale: evidence.summary.clone(),
            })
            .collect::<Vec<_>>();
        let expectations = self
            .expectations
            .iter()
            .enumerate()
            .map(|(index, expectation)| PerceptualExpectation {
                expectation_id: format!("{}:expectation:{index}", self.state_id),
                target_scope: scope.clone(),
                kind: expectation.kind.into(),
                status: expectation.status.into(),
                rationale: expectation.rationale.clone(),
                pending_ticks: expectation.pending_ticks,
            })
            .collect::<Vec<_>>();
        let attention_allocations = summarize_attention_allocations(
            &self.state_id,
            &scope,
            supporting_evidence
                .iter()
                .chain(opposing_evidence.iter())
                .chain(missing_evidence.iter()),
        );
        let uncertainties = build_perceptual_uncertainties(self, &scope, &missing_evidence);

        PerceptualState {
            state_id: self.state_id.clone(),
            scope,
            label: self.label.clone(),
            state_kind: self.state_kind.as_str().into(),
            trend: self.trend.as_str().into(),
            direction: self.direction.clone(),
            age_ticks: self.age_ticks,
            persistence_ticks: self.state_persistence_ticks,
            direction_continuity_ticks: self.direction_stability_rounds,
            confidence: self.confidence,
            strength: self.strength,
            support_count: self.support_count,
            contradict_count: self.contradict_count,
            count_support_fraction: self.count_support_fraction,
            weighted_support_fraction: self.weighted_support_fraction,
            support_weight: self.support_weight,
            contradict_weight: self.contradict_weight,
            supporting_evidence,
            opposing_evidence,
            missing_evidence,
            conflict_age_ticks: self.conflict_age_ticks,
            expectations,
            attention_allocations,
            uncertainties,
            active_setup_ids: self.active_setup_ids.clone(),
            dominant_intent_kind: self.dominant_intent_kind.clone(),
            dominant_intent_state: self.dominant_intent_state.clone(),
            cluster_key: self.cluster_key.clone(),
            cluster_label: self.cluster_label.clone(),
            last_transition_summary: self.last_transition_summary.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct CurrentStateFacts {
    raw_confirmed: bool,
    raw_missing: bool,
    peer_confirmed: bool,
    peer_missing: bool,
    cluster_expanded: bool,
    propagation_confirmed: bool,
    support_withdrawal: bool,
    conflict_relieved: bool,
    /// Y#6 — continuous observables carried into `evaluate_expectation` so
    /// prediction error can be computed numerically, not just as a
    /// met/pending/missed tag. None when the symbol has no cases carrying
    /// the relevant field this tick.
    peer_confirmation_ratio: Option<Decimal>,
    raw_support_fraction: Decimal,
}

#[derive(Debug, Clone)]
struct SymbolResidualFrame {
    sector: Option<String>,
    composite: Decimal,
    signal_strength: Decimal,
    capital_flow_direction: Decimal,
    price_momentum: Decimal,
    volume_profile: Decimal,
    pre_post_market_anomaly: Decimal,
    valuation: Decimal,
    cross_stock_correlation: Decimal,
    sector_coherence: Decimal,
    cross_market_propagation: Decimal,
}

pub fn derive_symbol_states(
    current_tick: u64,
    market: LiveMarket,
    cases: &[LiveTacticalCase],
    recent_transitions: &[AgentTransition],
    top_signals: &[LiveSignal],
    previous_states: &[PersistentSymbolState],
) -> Vec<PersistentSymbolState> {
    let mut case_groups = BTreeMap::<String, Vec<&LiveTacticalCase>>::new();
    for case in cases {
        if !case.symbol.is_empty() {
            case_groups
                .entry(case.symbol.clone())
                .or_default()
                .push(case);
        }
    }

    let residual_by_symbol = top_signals
        .iter()
        .map(|signal| (signal.symbol.clone(), build_signal_residual_frame(signal)))
        .collect::<BTreeMap<_, _>>();
    let previous_by_symbol = previous_states
        .iter()
        .map(|state| (state.symbol.as_str(), state))
        .collect::<BTreeMap<_, _>>();

    let all_symbols = case_groups
        .keys()
        .map(String::as_str)
        .chain(residual_by_symbol.keys().map(String::as_str))
        .chain(previous_by_symbol.keys().copied())
        .collect::<BTreeSet<_>>();

    let mut states = all_symbols
        .into_iter()
        .map(|symbol| {
            let symbol_cases = case_groups.get(symbol).cloned().unwrap_or_default();
            let residual = residual_by_symbol.get(symbol);
            build_symbol_state(
                current_tick,
                market,
                symbol,
                symbol_cases,
                residual,
                recent_transitions,
                previous_by_symbol.get(symbol).copied(),
            )
        })
        .collect::<Vec<_>>();

    states.sort_by(|a, b| {
        b.strength
            .cmp(&a.strength)
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    states
}

fn build_symbol_state(
    current_tick: u64,
    market: LiveMarket,
    symbol: &str,
    cases: Vec<&LiveTacticalCase>,
    residual: Option<&SymbolResidualFrame>,
    recent_transitions: &[AgentTransition],
    previous_state: Option<&PersistentSymbolState>,
) -> PersistentSymbolState {
    let label = strongest_case(&cases)
        .map(|case| case.title.clone())
        .unwrap_or_else(|| symbol.to_string());
    let sector = residual.and_then(|item| item.sector.clone());
    let (cluster_key, cluster_label) =
        infer_cluster_identity(symbol, &cases, residual, previous_state);
    let buy_count = cases
        .iter()
        .filter(|case| case_direction(case) == Some("buy"))
        .count();
    let sell_count = cases
        .iter()
        .filter(|case| case_direction(case) == Some("sell"))
        .count();
    let direction = infer_symbol_direction(buy_count, sell_count, residual);
    let recent_flip = has_recent_direction_flip(symbol, recent_transitions);
    let base_direction_stability_rounds = infer_symbol_direction_stability_rounds(
        current_tick,
        symbol,
        direction.as_deref(),
        recent_transitions,
    );
    let base_state_persistence_ticks = infer_symbol_state_persistence_ticks(
        current_tick,
        symbol,
        direction.as_deref(),
        recent_transitions,
        recent_flip,
    );
    let support_fraction = aggregate_support_fraction(&cases);
    let support_count = aggregate_support_count(&cases);
    let contradict_count = aggregate_contradict_count(&cases);
    let support_weight = aggregate_support_weight(&cases);
    let contradict_weight = aggregate_contradict_weight(&cases);
    let count_support_fraction = aggregate_count_support_fraction(&cases);
    let signal_strength = residual
        .map(|item| item.signal_strength)
        .unwrap_or(Decimal::ZERO);
    let residual_dimension_count = residual
        .map(active_residual_dimension_count)
        .unwrap_or_default();
    let case_confidence = strongest_case(&cases)
        .map(|case| case.confidence)
        .unwrap_or(Decimal::ZERO);
    let freshness_decay = cases.iter().any(|case| {
        matches!(
            case.freshness_state.as_deref(),
            Some("stale" | "expired" | "carried_forward")
        )
    });
    let late_timing = cases.iter().any(|case| {
        matches!(
            case.timing_state.as_deref(),
            Some("late_chase" | "range_extreme")
        )
    });
    let explicit_conflict = buy_count > 0 && sell_count > 0
        || cases.iter().any(|case| {
            matches!(
                case.review_reason_code.as_deref(),
                Some("directional_conflict" | "raw_direction_conflict")
            )
        });
    let expectation_conflict = cases
        .iter()
        .any(|case| !case.expectation_violations.is_empty());
    let no_surface_case = cases.is_empty();
    let has_enter_case = cases.iter().any(|case| case.action == "enter");
    let dominant_intent = strongest_case(&cases).and_then(|case| case.inferred_intent.as_ref());
    let dominant_intent_kind =
        dominant_intent.map(|intent| intent_kind_slug(intent.kind).to_string());
    let dominant_intent_state =
        dominant_intent.map(|intent| intent_state_slug(intent.state).to_string());
    let peer_confirmation_ratio = aggregate_peer_confirmation_ratio(&cases);
    let peer_active_count = aggregate_peer_active_count(&cases);
    let peer_silent_count = aggregate_peer_silent_count(&cases);
    let absence_summary = aggregate_absence_summary(&cases);

    // Compute CurrentStateFacts BEFORE state_kind classification so negative
    // evidence can demote or flip the state assignment, not just annotate it
    // after the fact. Previously this lived at line 766 (post-classification)
    // and `missing_evidence` was appended to `opposing_evidence` only as a
    // strength-score deduction — absence was never first-class.
    let facts = CurrentStateFacts {
        raw_confirmed: support_fraction >= RAW_SUPERMAJORITY
            || support_weight >= RAW_WEIGHT_FLOOR_STRONG,
        raw_missing: support_fraction < RAW_MAJORITY || support_weight < RAW_WEIGHT_FLOOR_WEAK,
        peer_confirmed: peer_confirmation_ratio
            .map(|value| value >= PEER_MAJORITY)
            .unwrap_or(false)
            || peer_active_count >= 2,
        peer_missing: peer_confirmation_ratio
            .map(|value| value < PEER_HALF)
            .unwrap_or(peer_active_count == 0),
        cluster_expanded: peer_confirmation_ratio
            .map(|value| value >= PEER_NEAR_UNANIMOUS)
            .unwrap_or(false)
            || peer_active_count >= 3,
        propagation_confirmed: !expectation_conflict
            && !no_surface_case
            && (support_fraction >= RAW_MAJORITY
                || signal_strength >= SIGNAL_STRENGTH_LATENT
                || residual_dimension_count >= 2),
        support_withdrawal: support_fraction < SUPPORT_WITHDRAWAL
            || contradict_weight > support_weight
            || recent_flip,
        conflict_relieved: !explicit_conflict
            && previous_state
                .map(|state| state.state_kind == PersistentStateKind::Conflicted)
                .unwrap_or(false),
        peer_confirmation_ratio,
        raw_support_fraction: support_fraction,
    };

    // Peer-confirmation withdrawal: the symbol had peer backing on the previous
    // tick and has lost it this tick. Y-ward read — "who stopped moving with us"
    // is first-class evidence, not an annotation. Tripping this flag is a
    // candidate for TurningPoint classification below.
    //
    // The evidence codes checked here match what `supporting_evidence` actually
    // carries when peer follow-through or cluster expansion expectations were
    // met on the prior tick (see `evaluate_expectation` — status Met pushes
    // `expectation_met:<kind>`). Earlier revisions of this check used bare names
    // like `peer_confirmation` that are never emitted anywhere, so the whole
    // withdrawal path was silently dead for the 2026-04-17 overnight run.
    let peer_confirmation_withdrawn = previous_state
        .map(|prev| {
            facts.peer_missing
                && prev.supporting_evidence.iter().any(|item| {
                    matches!(
                        item.code.as_str(),
                        "expectation_met:peer_follow_through" | "expectation_met:cluster_expansion"
                    )
                })
        })
        .unwrap_or(false);

    let mut supporting_evidence = Vec::new();
    let mut opposing_evidence = Vec::new();

    if let Some(residual) = residual {
        supporting_evidence.extend(residual_supporting_evidence(symbol, residual));
    }

    if support_fraction >= dec!(0.80) {
        supporting_evidence.push(PersistentStateEvidence {
            code: "supermajority_raw_support".into(),
            summary: format!(
                "{symbol} weighted raw support is {}",
                support_fraction.round_dp(3)
            ),
            weight: dec!(0.28),
        });
    }
    if support_count >= 4 || support_weight >= dec!(1.60) {
        supporting_evidence.push(PersistentStateEvidence {
            code: "multi_channel_confirmation".into(),
            summary: format!(
                "{symbol} has durable raw confirmation (channels={}, weight={})",
                support_count,
                support_weight.round_dp(3)
            ),
            weight: dec!(0.18),
        });
    }
    if base_direction_stability_rounds >= 2 {
        supporting_evidence.push(PersistentStateEvidence {
            code: "direction_continuity".into(),
            summary: format!(
                "{symbol} direction stayed consistent for {} rounds",
                base_direction_stability_rounds
            ),
            weight: dec!(0.16),
        });
    }
    if has_enter_case {
        supporting_evidence.push(PersistentStateEvidence {
            code: "surface_translated".into(),
            summary: format!("{symbol} is already translated into an active case"),
            weight: dec!(0.12),
        });
    }
    if let Some(intent) = dominant_intent {
        if matches!(intent.state, IntentState::Active | IntentState::Forming) {
            supporting_evidence.push(PersistentStateEvidence {
                code: "intent_present".into(),
                summary: format!(
                    "{symbol} dominant intent is {} in {} state",
                    intent_kind_slug(intent.kind),
                    intent_state_slug(intent.state)
                ),
                weight: dec!(0.18),
            });
        }
    }
    if no_surface_case && (signal_strength >= dec!(0.35) || residual_dimension_count >= 2) {
        supporting_evidence.push(PersistentStateEvidence {
            code: "sensor_level_residual_pressure".into(),
            summary: format!(
                "{symbol} still has structured residual pressure without a surfaced case"
            ),
            weight: dec!(0.16),
        });
    }

    if support_fraction < dec!(0.67) {
        opposing_evidence.push(PersistentStateEvidence {
            code: "weak_raw_support".into(),
            summary: format!(
                "{symbol} weighted raw support fell to {}",
                support_fraction.round_dp(3)
            ),
            weight: dec!(0.22),
        });
    }
    if support_count < 2 && support_weight < dec!(0.80) {
        opposing_evidence.push(PersistentStateEvidence {
            code: "insufficient_channel_count".into(),
            summary: format!(
                "{symbol} has thin raw confirmation (channels={}, weight={})",
                support_count,
                support_weight.round_dp(3)
            ),
            weight: dec!(0.16),
        });
    }
    if contradict_count > 0 {
        opposing_evidence.push(PersistentStateEvidence {
            code: "contradicting_raw_channels".into(),
            summary: format!("{symbol} has {contradict_count} contradicting raw channels"),
            weight: dec!(0.12) + Decimal::from(contradict_count.min(4) as i64) * dec!(0.03),
        });
    }
    if recent_flip {
        opposing_evidence.push(PersistentStateEvidence {
            code: "recent_direction_flip".into(),
            summary: format!("{symbol} recently flipped direction"),
            weight: dec!(0.22),
        });
    }
    if late_timing {
        opposing_evidence.push(PersistentStateEvidence {
            code: "late_signal_timing".into(),
            summary: format!("{symbol} is firing late in the local range"),
            weight: dec!(0.18),
        });
    }
    if freshness_decay {
        opposing_evidence.push(PersistentStateEvidence {
            code: "freshness_decay".into(),
            summary: format!("{symbol} freshness already decayed or carried across ticks"),
            weight: dec!(0.18),
        });
    }
    if explicit_conflict {
        opposing_evidence.push(PersistentStateEvidence {
            code: "directional_conflict".into(),
            summary: format!("{symbol} has simultaneous buy and sell interpretations"),
            weight: dec!(0.30),
        });
    }
    if expectation_conflict {
        opposing_evidence.push(PersistentStateEvidence {
            code: "expectation_violation".into(),
            summary: format!("{symbol} is carrying expectation violations"),
            weight: dec!(0.14),
        });
    }
    if no_surface_case && signal_strength < dec!(0.35) && residual_dimension_count < 2 {
        opposing_evidence.push(PersistentStateEvidence {
            code: "no_confirmed_surface".into(),
            summary: format!("{symbol} has no confirmed surfaced structure yet"),
            weight: dec!(0.10),
        });
    }

    let mut state_kind = if explicit_conflict {
        PersistentStateKind::Conflicted
    } else if recent_flip
        || late_timing
        || peer_confirmation_withdrawn
        || matches!(
            dominant_intent.map(|intent| intent.state),
            Some(IntentState::AtRisk | IntentState::Exhausted | IntentState::Invalidated)
        )
    {
        PersistentStateKind::TurningPoint
    } else if !no_surface_case
        && support_fraction >= RAW_SUPERMAJORITY
        && (support_count >= 4 || support_weight >= RAW_WEIGHT_FLOOR_STRONG)
        && base_direction_stability_rounds >= 2
        && !freshness_decay
    {
        PersistentStateKind::Continuation
    } else if signal_strength >= SIGNAL_STRENGTH_LATENT
        || residual_dimension_count >= 2
        || has_enter_case
        || matches!(
            dominant_intent.map(|intent| intent.state),
            Some(IntentState::Forming | IntentState::Active)
        )
    {
        PersistentStateKind::Latent
    } else {
        PersistentStateKind::LowInformation
    };

    // First-class absence demotion.
    //
    // A symbol whose raw evidence says Continuation but whose peers are all
    // silent is not really continuing — it is moving in isolation, which is
    // closer to Latent (the raw thesis exists but the market has not confirmed
    // it via peer follow-through). Similarly, a Latent symbol whose raw
    // support has also dropped out is really LowInformation — the absence
    // overwhelms the positive read.
    //
    // This keeps state_kind a 5-variant enum (no new "Isolated") while moving
    // absence from post-hoc annotation into the classification path.
    let absence_demoted = match state_kind {
        PersistentStateKind::Continuation if facts.peer_missing => {
            state_kind = PersistentStateKind::Latent;
            true
        }
        PersistentStateKind::Latent if facts.peer_missing && facts.raw_missing => {
            state_kind = PersistentStateKind::LowInformation;
            true
        }
        _ => false,
    };
    if absence_demoted {
        opposing_evidence.push(PersistentStateEvidence {
            code: "demoted_by_absence".into(),
            summary: format!(
                "{symbol} demoted: peer_silent={peer_silent_count} peer_active={peer_active_count} raw_missing={}",
                facts.raw_missing
            ),
            weight: dec!(0.20),
        });
    }

    // State restoration on LowInformation — merges the one-tick-back
    // state_memory_continuity hook with Y#5 multi-tick lookback. Priority:
    // previous_state (1 tick back) first; if that doesn't fire,
    // lookback_state_prior (up to 10 transitions). Both paths cap at Latent
    // so we never resurrect Continuation from history alone — actual raw
    // support has to be present this tick for Continuation. Both emit
    // distinct reason_codes so operators can tell which path fired.
    if matches!(state_kind, PersistentStateKind::LowInformation) {
        let no_strong_contradiction =
            !explicit_conflict && !recent_flip && !freshness_decay && !late_timing;
        let minimal_current_signal = signal_strength >= SIGNAL_STRENGTH_RESTORE_FLOOR
            || residual_dimension_count >= 1
            || !cases.is_empty();
        let mut restored = false;
        if let Some(previous_state) = previous_state {
            if previous_state.state_kind != PersistentStateKind::LowInformation
                && no_strong_contradiction
                && minimal_current_signal
            {
                state_kind = match previous_state.state_kind {
                    PersistentStateKind::Continuation => PersistentStateKind::Latent,
                    other => other,
                };
                supporting_evidence.push(PersistentStateEvidence {
                    code: "state_memory_continuity".into(),
                    summary: format!(
                        "{symbol} kept continuity from prior {} state",
                        previous_state.state_kind
                    ),
                    weight: dec!(0.12),
                });
                restored = true;
            }
        }
        if !restored && no_strong_contradiction && minimal_current_signal {
            if let Some((prior_kind, prior_summary)) =
                lookback_state_prior(current_tick, symbol, recent_transitions)
            {
                state_kind = match prior_kind {
                    PersistentStateKind::Continuation => PersistentStateKind::Latent,
                    other => other,
                };
                supporting_evidence.push(PersistentStateEvidence {
                    code: "lookback_prior_restored".into(),
                    summary: prior_summary,
                    weight: dec!(0.10),
                });
            }
        }
    } else if matches!(state_kind, PersistentStateKind::Latent)
        && !explicit_conflict
        && !recent_flip
    {
        // Even when the current classifier reached Latent on its own,
        // surfacing a sustained historical prior is useful context for the
        // operator (and feeds the strength calculation via positive_weight).
        // This is pure supporting evidence — it does not change state_kind.
        if let Some((prior_kind, prior_summary)) =
            lookback_state_prior(current_tick, symbol, recent_transitions)
        {
            if matches!(prior_kind, PersistentStateKind::Continuation) {
                supporting_evidence.push(PersistentStateEvidence {
                    code: "lookback_prior_supports".into(),
                    summary: prior_summary,
                    weight: dec!(0.08),
                });
            }
        }
    }

    let direction_stability_rounds = if let Some(previous_state) = previous_state {
        if previous_state.direction.as_deref() == direction.as_deref()
            && !recent_flip
            && direction.as_deref() != Some("mixed")
        {
            base_direction_stability_rounds
                .max(previous_state.direction_stability_rounds.saturating_add(1))
        } else {
            base_direction_stability_rounds
        }
    } else {
        base_direction_stability_rounds
    };

    let state_persistence_ticks = if let Some(previous_state) = previous_state {
        if previous_state.state_kind == state_kind {
            base_state_persistence_ticks
                .max(previous_state.state_persistence_ticks.saturating_add(1))
        } else {
            base_state_persistence_ticks
        }
    } else {
        base_state_persistence_ticks
    };

    let age_ticks = infer_symbol_age_ticks(
        current_tick,
        symbol,
        &cases,
        recent_transitions,
        previous_state,
        state_kind,
        direction.as_deref(),
    );
    let conflict_age_ticks = if matches!(state_kind, PersistentStateKind::Conflicted) {
        previous_state
            .filter(|state| state.state_kind == PersistentStateKind::Conflicted)
            .map(|state| state.conflict_age_ticks.saturating_add(1))
            .unwrap_or(1)
    } else {
        0
    };

    let mut missing_evidence = Vec::new();
    if matches!(
        state_kind,
        PersistentStateKind::Continuation | PersistentStateKind::Latent
    ) && !facts.peer_confirmed
    {
        missing_evidence.push(PersistentStateEvidence {
            code: "missing_peer_confirmation".into(),
            summary: format!(
                "{symbol} has not yet earned peer confirmation (peer_conf={})",
                peer_confirmation_ratio
                    .map(|value| value.round_dp(3).to_string())
                    .unwrap_or_else(|| "-".into())
            ),
            weight: dec!(0.12),
        });
    }
    if matches!(
        state_kind,
        PersistentStateKind::Continuation | PersistentStateKind::Latent
    ) && (absence_summary.is_some() || peer_silent_count > 0)
    {
        missing_evidence.push(PersistentStateEvidence {
            code: "missing_cross_symbol_follow".into(),
            summary: absence_summary.unwrap_or_else(|| {
                format!(
                    "{symbol} is active while {} peers remain silent",
                    peer_silent_count
                )
            }),
            weight: dec!(0.12),
        });
    }
    if matches!(
        state_kind,
        PersistentStateKind::Continuation | PersistentStateKind::Latent
    ) && facts.raw_missing
    {
        missing_evidence.push(PersistentStateEvidence {
            code: "missing_raw_confirmation".into(),
            summary: format!(
                "{symbol} still lacks strong raw confirmation (weighted_sf={}, weight={})",
                support_fraction.round_dp(3),
                support_weight.round_dp(3)
            ),
            weight: dec!(0.14),
        });
    }
    if conflict_age_ticks > 0 {
        missing_evidence.push(PersistentStateEvidence {
            code: "conflict_age".into(),
            summary: format!(
                "{symbol} conflict has persisted for {} ticks",
                conflict_age_ticks
            ),
            weight: dec!(0.10),
        });
    }
    opposing_evidence.extend(missing_evidence.iter().cloned());

    let stability_norm = Decimal::from(direction_stability_rounds.min(6) as i64) / Decimal::from(6);
    let persistence_norm = Decimal::from(state_persistence_ticks.min(6) as i64) / Decimal::from(6);
    let support_density = Decimal::from(support_count.min(8) as i64) / Decimal::from(8);
    let weighted_density = clamp_unit(support_weight / dec!(2.50));
    let contradict_density = clamp_unit(contradict_weight / dec!(2.00));
    let positive_weight = supporting_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>();
    let negative_weight = opposing_evidence
        .iter()
        .map(|item| item.weight)
        .sum::<Decimal>();
    let mut strength = case_confidence * dec!(0.35)
        + support_fraction * dec!(0.25)
        + signal_strength * dec!(0.20)
        + stability_norm * dec!(0.10)
        + support_density * dec!(0.05)
        + weighted_density * dec!(0.10)
        + positive_weight * dec!(0.10)
        - negative_weight * dec!(0.15)
        - contradict_density * dec!(0.08);
    strength = clamp_unit(strength);

    let confidence = match state_kind {
        PersistentStateKind::Continuation => clamp_unit(
            case_confidence.max(support_fraction) * dec!(0.80)
                + stability_norm * dec!(0.15)
                + persistence_norm * dec!(0.05),
        )
        .max(dec!(0.60)),
        PersistentStateKind::TurningPoint => clamp_unit(
            case_confidence.max(signal_strength) * dec!(0.70)
                + negative_weight * dec!(0.20)
                + if recent_flip {
                    dec!(0.10)
                } else {
                    Decimal::ZERO
                },
        )
        .max(dec!(0.55)),
        PersistentStateKind::Conflicted => clamp_unit(
            negative_weight * dec!(0.45)
                + Decimal::from((buy_count.min(sell_count) + 1) as i64) * dec!(0.10)
                + case_confidence * dec!(0.25),
        )
        .max(dec!(0.60)),
        PersistentStateKind::Latent => clamp_unit(
            signal_strength * dec!(0.40)
                + case_confidence * dec!(0.25)
                + support_fraction * dec!(0.15)
                + weighted_density * dec!(0.10)
                + positive_weight * dec!(0.10),
        )
        .max(dec!(0.50)),
        PersistentStateKind::LowInformation => clamp_unit(
            support_fraction * dec!(0.25)
                + weighted_density * dec!(0.15)
                + signal_strength * dec!(0.20)
                + if no_surface_case {
                    dec!(0.10)
                } else {
                    Decimal::ZERO
                },
        )
        .max(dec!(0.35)),
    };

    let trend = if let Some(previous_state) = previous_state {
        let delta = strength - previous_state.strength;
        if delta >= dec!(0.05) {
            PersistentStateTrend::Strengthening
        } else if delta <= dec!(-0.05) || recent_flip {
            PersistentStateTrend::Weakening
        } else {
            PersistentStateTrend::Stable
        }
    } else if recent_flip || negative_weight > positive_weight + dec!(0.12) {
        PersistentStateTrend::Weakening
    } else if positive_weight >= negative_weight + dec!(0.12)
        && !matches!(
            state_kind,
            PersistentStateKind::TurningPoint | PersistentStateKind::Conflicted
        )
    {
        PersistentStateTrend::Strengthening
    } else {
        PersistentStateTrend::Stable
    };

    let last_transition_summary = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .max_by_key(|transition| transition.to_tick)
        .map(|transition| transition.summary.clone());
    let last_transition_summary = last_transition_summary.or_else(|| {
        previous_state.and_then(|previous_state| {
            if previous_state.state_kind != state_kind
                || previous_state.direction.as_deref() != direction.as_deref()
            {
                Some(format!(
                    "{symbol} state {} -> {}",
                    previous_state.state_kind, state_kind
                ))
            } else {
                previous_state.last_transition_summary.clone()
            }
        })
    });
    let expectations = derive_expectations(
        symbol,
        state_kind,
        previous_state,
        &facts,
        &mut supporting_evidence,
        &mut opposing_evidence,
    );

    PersistentSymbolState {
        state_id: format!("{}:symbol:{symbol}", market_slug(market)),
        symbol: symbol.to_string(),
        sector,
        label,
        state_kind,
        direction,
        age_ticks,
        state_persistence_ticks,
        direction_stability_rounds,
        support_count,
        contradict_count,
        count_support_fraction,
        weighted_support_fraction: support_fraction,
        support_weight,
        contradict_weight,
        strength,
        confidence,
        trend,
        supporting_evidence,
        opposing_evidence,
        missing_evidence,
        conflict_age_ticks,
        expectations,
        active_setup_ids: cases.iter().map(|case| case.setup_id.clone()).collect(),
        dominant_intent_kind,
        dominant_intent_state,
        cluster_key,
        cluster_label,
        last_transition_summary,
    }
}

pub fn build_cluster_states_from_symbol_states(
    symbol_states: &[PersistentSymbolState],
    previous: &[LiveClusterState],
) -> Vec<LiveClusterState> {
    let previous_by_key = previous
        .iter()
        .map(|cluster| (cluster.cluster_key.as_str(), cluster))
        .collect::<BTreeMap<_, _>>();

    let mut grouped = BTreeMap::<(String, String), Vec<&PersistentSymbolState>>::new();
    for state in symbol_states {
        grouped
            .entry((state.cluster_key.clone(), state.cluster_label.clone()))
            .or_default()
            .push(state);
    }

    grouped
        .into_iter()
        .map(|((cluster_key, label), members)| {
            let direction = dominant_member_direction(&members).unwrap_or_else(|| "unknown".into());
            let continuation = members
                .iter()
                .filter(|state| state.state_kind == PersistentStateKind::Continuation)
                .count();
            let turning = members
                .iter()
                .filter(|state| state.state_kind == PersistentStateKind::TurningPoint)
                .count();
            let conflicted = members
                .iter()
                .filter(|state| state.state_kind == PersistentStateKind::Conflicted)
                .count();
            let latent = members
                .iter()
                .filter(|state| state.state_kind == PersistentStateKind::Latent)
                .count();

            let state = if conflicted > 0 || mixed_member_directions(&members) {
                "conflicted"
            } else if continuation >= 2 && !matches!(direction.as_str(), "mixed" | "unknown") {
                "continuation"
            } else if turning > 0 && turning >= continuation.max(1) {
                "turning_point"
            } else if latent >= 2 {
                "latent"
            } else {
                "low_information"
            };

            let confidence = members
                .iter()
                .map(|state| state.confidence)
                .sum::<Decimal>()
                / Decimal::from(members.len().max(1) as i64);

            let mut ranked_members = members.iter().copied().collect::<Vec<_>>();
            ranked_members.sort_by(|a, b| {
                b.strength
                    .cmp(&a.strength)
                    .then_with(|| b.confidence.cmp(&a.confidence))
                    .then_with(|| a.symbol.cmp(&b.symbol))
            });
            let leader_symbols = ranked_members
                .iter()
                .take(3)
                .map(|state| state.symbol.clone())
                .collect::<Vec<_>>();
            let laggard_symbols = ranked_members
                .iter()
                .rev()
                .take(3)
                .map(|state| state.symbol.clone())
                .collect::<Vec<_>>();

            let summary = match state {
                "continuation" => format!(
                    "{} continuation led by {}",
                    label,
                    leader_symbols.join(", ")
                ),
                "conflicted" => format!("{} is conflicted by competing symbol states", label),
                "turning_point" => format!("{} is showing turning-point pressure", label),
                "latent" => format!("{} is latent and still building confirmation", label),
                _ => format!("{} remains low-information", label),
            };

            let previous_cluster = previous_by_key.get(cluster_key.as_str()).copied();
            let (age_ticks, state_persistence_ticks, trend, last_transition_summary) =
                enrich_cluster_persistence(previous_cluster, state, confidence, &label);

            LiveClusterState {
                cluster_key,
                label,
                direction,
                state: state.into(),
                confidence,
                member_count: members.len(),
                leader_symbols,
                laggard_symbols,
                summary,
                age_ticks,
                state_persistence_ticks,
                trend,
                last_transition_summary,
            }
        })
        .collect()
}

/// Project per-tick continuity onto a cluster by comparing against the prior
/// tick's `LiveClusterState` for the same `cluster_key`.
///
/// - A cluster that existed last tick and kept the same `state` ages and
///   increments `state_persistence_ticks`. `trend` is derived from the
///   confidence delta against the previous snapshot.
/// - When the state changes, `state_persistence_ticks` resets to 1, but
///   `age_ticks` still increments so operators can read "this cluster has
///   existed for N ticks but just flipped to X". The flip is recorded into
///   `last_transition_summary`.
/// - A brand-new cluster starts at `age_ticks=1`, `state_persistence_ticks=1`,
///   `trend=""` (no prior to compare against).
/// - `low_information` is treated specially: it does not age the cluster
///   (age_ticks returns 0) so "how long has this cluster been meaningful"
///   stays accurate even when the cluster briefly collapses.
fn enrich_cluster_persistence(
    previous: Option<&LiveClusterState>,
    new_state: &str,
    new_confidence: Decimal,
    label: &str,
) -> (u64, u16, String, Option<String>) {
    match previous {
        Some(prev) => {
            let same_state = prev.state == new_state;
            let prev_meaningful = prev.state != "low_information";
            let now_meaningful = new_state != "low_information";
            let age_ticks = if now_meaningful {
                prev.age_ticks.saturating_add(1)
            } else if prev_meaningful {
                // Cluster existed but collapsed back to low_information this tick.
                // Keep prior age frozen rather than resetting — it will resume
                // incrementing if the cluster re-emerges.
                prev.age_ticks
            } else {
                0
            };
            let state_persistence_ticks = if same_state {
                prev.state_persistence_ticks.saturating_add(1)
            } else {
                1
            };
            let trend = classify_trend(new_confidence, prev.confidence);
            let last_transition_summary = if same_state {
                prev.last_transition_summary.clone()
            } else {
                Some(format!("{label}: {} -> {new_state}", prev.state))
            };
            (
                age_ticks,
                state_persistence_ticks,
                trend,
                last_transition_summary,
            )
        }
        None => {
            let age_ticks = if new_state == "low_information" { 0 } else { 1 };
            (age_ticks, 1, String::new(), None)
        }
    }
}

/// Classify the confidence delta between two ticks as strengthening / weakening /
/// stable. No magic numbers — the threshold is derived from `Decimal::from(1, 2)`
/// (0.01 units of confidence), matching the resolution operators already use when
/// reading confidence values in the UI.
fn classify_trend(current: Decimal, previous: Decimal) -> String {
    let delta = current - previous;
    let epsilon = Decimal::new(1, 2);
    if delta > epsilon {
        "strengthening".into()
    } else if delta < -epsilon {
        "weakening".into()
    } else {
        "stable".into()
    }
}

pub fn build_world_summary_from_symbol_states(
    market: LiveMarket,
    clusters: &[LiveClusterState],
    previous: Option<&LiveWorldSummary>,
) -> LiveWorldSummary {
    let mut ranked = clusters.iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    let meaningful = clusters
        .iter()
        .filter(|cluster| cluster.state != "low_information" && cluster.member_count >= 2)
        .collect::<Vec<_>>();
    let dominant_source = if meaningful.is_empty() {
        &ranked
    } else {
        &meaningful
    };
    let dominant_clusters = dominant_source
        .iter()
        .take(3)
        .map(|cluster| cluster.cluster_key.clone())
        .collect::<Vec<_>>();
    let continuation = meaningful
        .iter()
        .copied()
        .filter(|cluster| cluster.state == "continuation")
        .collect::<Vec<_>>();
    let conflicted = meaningful
        .iter()
        .copied()
        .filter(|cluster| cluster.state == "conflicted")
        .collect::<Vec<_>>();
    let turning = meaningful
        .iter()
        .copied()
        .filter(|cluster| cluster.state == "turning_point")
        .collect::<Vec<_>>();
    let latent = meaningful
        .iter()
        .copied()
        .filter(|cluster| cluster.state == "latent")
        .collect::<Vec<_>>();

    let regime = if meaningful.is_empty() {
        "low_information"
    } else if !conflicted.is_empty() || turning.len() >= 2 {
        "reversal_prone"
    } else if !continuation.is_empty() && single_direction(&continuation) {
        "trend"
    } else if !latent.is_empty() {
        "latent_build"
    } else {
        "chop"
    };

    let confidence = if dominant_source.is_empty() {
        Decimal::ZERO
    } else {
        dominant_source
            .iter()
            .take(3)
            .map(|cluster| cluster.confidence)
            .sum::<Decimal>()
            / Decimal::from(dominant_source.len().min(3) as i64)
    };

    let summary = match regime {
        "trend" => "dominant persistent clusters point in one direction",
        "reversal_prone" => "persistent clusters are fighting or eroding",
        "latent_build" => "persistent clusters are building but not yet resolved",
        "chop" => "persistent clusters exist but do not agree",
        _ => "persistent state evidence is weak",
    };

    let (age_ticks, state_persistence_ticks, trend, last_transition_summary) =
        enrich_world_persistence(previous, regime, confidence);

    LiveWorldSummary {
        regime: regime.into(),
        confidence,
        dominant_clusters,
        summary: format!("{} perception regime: {summary}", market_label(market)),
        age_ticks,
        state_persistence_ticks,
        trend,
        last_transition_summary,
    }
}

/// World-level analogue of `enrich_cluster_persistence`. Identical semantics:
/// age_ticks freezes on `low_information`, state_persistence_ticks resets on
/// regime flip, `trend` compares confidence to prior tick, and
/// `last_transition_summary` records the most recent regime change.
fn enrich_world_persistence(
    previous: Option<&LiveWorldSummary>,
    new_regime: &str,
    new_confidence: Decimal,
) -> (u64, u16, String, Option<String>) {
    match previous {
        Some(prev) => {
            let same_regime = prev.regime == new_regime;
            let prev_meaningful = prev.regime != "low_information";
            let now_meaningful = new_regime != "low_information";
            let age_ticks = if now_meaningful {
                prev.age_ticks.saturating_add(1)
            } else if prev_meaningful {
                prev.age_ticks
            } else {
                0
            };
            let state_persistence_ticks = if same_regime {
                prev.state_persistence_ticks.saturating_add(1)
            } else {
                1
            };
            let trend = classify_trend(new_confidence, prev.confidence);
            let last_transition_summary = if same_regime {
                prev.last_transition_summary.clone()
            } else {
                Some(format!("regime: {} -> {new_regime}", prev.regime))
            };
            (
                age_ticks,
                state_persistence_ticks,
                trend,
                last_transition_summary,
            )
        }
        None => {
            let age_ticks = if new_regime == "low_information" {
                0
            } else {
                1
            };
            (age_ticks, 1, String::new(), None)
        }
    }
}

fn strongest_case<'a>(cases: &'a [&LiveTacticalCase]) -> Option<&'a LiveTacticalCase> {
    cases.iter().copied().max_by(|a, b| {
        a.confidence
            .cmp(&b.confidence)
            .then_with(|| a.heuristic_edge.cmp(&b.heuristic_edge))
    })
}

fn aggregate_support_fraction(cases: &[&LiveTacticalCase]) -> Decimal {
    let values = cases
        .iter()
        .filter_map(|case| {
            case.raw_disagreement
                .as_ref()
                .map(|raw| raw.support_fraction)
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        Decimal::ZERO
    } else {
        values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
    }
}

fn aggregate_count_support_fraction(cases: &[&LiveTacticalCase]) -> Decimal {
    let values = cases
        .iter()
        .filter_map(|case| {
            case.raw_disagreement
                .as_ref()
                .map(|raw| raw.count_support_fraction)
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        Decimal::ZERO
    } else {
        values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
    }
}

fn aggregate_support_count(cases: &[&LiveTacticalCase]) -> usize {
    cases
        .iter()
        .filter_map(|case| case.raw_disagreement.as_ref().map(|raw| raw.support_count))
        .max()
        .unwrap_or(0)
}

fn aggregate_contradict_count(cases: &[&LiveTacticalCase]) -> usize {
    cases
        .iter()
        .filter_map(|case| {
            case.raw_disagreement
                .as_ref()
                .map(|raw| raw.contradict_count)
        })
        .max()
        .unwrap_or(0)
}

fn aggregate_support_weight(cases: &[&LiveTacticalCase]) -> Decimal {
    cases
        .iter()
        .filter_map(|case| case.raw_disagreement.as_ref().map(|raw| raw.support_weight))
        .max()
        .unwrap_or(Decimal::ZERO)
}

fn aggregate_contradict_weight(cases: &[&LiveTacticalCase]) -> Decimal {
    cases
        .iter()
        .filter_map(|case| {
            case.raw_disagreement
                .as_ref()
                .map(|raw| raw.contradict_weight)
        })
        .max()
        .unwrap_or(Decimal::ZERO)
}

fn aggregate_peer_confirmation_ratio(cases: &[&LiveTacticalCase]) -> Option<Decimal> {
    let values = cases
        .iter()
        .filter_map(|case| case.peer_confirmation_ratio)
        .collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64))
    }
}

fn aggregate_peer_active_count(cases: &[&LiveTacticalCase]) -> usize {
    cases
        .iter()
        .filter_map(|case| case.peer_active_count)
        .max()
        .unwrap_or(0)
}

fn aggregate_peer_silent_count(cases: &[&LiveTacticalCase]) -> usize {
    cases
        .iter()
        .filter_map(|case| case.peer_silent_count)
        .max()
        .unwrap_or(0)
}

fn aggregate_absence_summary(cases: &[&LiveTacticalCase]) -> Option<String> {
    cases
        .iter()
        .filter_map(|case| case.absence_summary.clone())
        .next()
}

fn derive_expectations(
    symbol: &str,
    state_kind: PersistentStateKind,
    previous_state: Option<&PersistentSymbolState>,
    facts: &CurrentStateFacts,
    supporting_evidence: &mut Vec<PersistentStateEvidence>,
    opposing_evidence: &mut Vec<PersistentStateEvidence>,
) -> Vec<PersistentStateExpectation> {
    let expected_kinds = match state_kind {
        PersistentStateKind::Continuation => vec![
            PersistentExpectationKind::PeerFollowThrough,
            PersistentExpectationKind::RawChannelConfirmation,
            PersistentExpectationKind::ClusterExpansion,
        ],
        PersistentStateKind::Latent => vec![
            PersistentExpectationKind::RawChannelConfirmation,
            PersistentExpectationKind::PeerFollowThrough,
        ],
        PersistentStateKind::TurningPoint | PersistentStateKind::Conflicted => {
            vec![PersistentExpectationKind::PropagationFollowThrough]
        }
        PersistentStateKind::LowInformation => Vec::new(),
    };

    let previous_expectations = previous_state
        .map(|state| {
            state
                .expectations
                .iter()
                .map(|expectation| (expectation.kind, expectation))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let mut expectations = Vec::new();
    for kind in expected_kinds {
        let previous = previous_expectations.get(&kind).copied();
        let pending_ticks = previous
            .map(|expectation| expectation.pending_ticks.saturating_add(1))
            .unwrap_or(0);
        // Y#6 full closed loop — feed prior EWMA error back as a per-symbol
        // target adjustment. evaluate_expectation lowers its base target by
        // the accumulated error (clamped to a floor), so a symbol that has
        // routinely fallen short of a given expectation stops tripping the
        // status=Missed path on noise, while a sudden new deterioration
        // still reads as a real error because the EWMA decay (alpha=0.2)
        // means fresh misses dominate within ~5 ticks.
        let prior_ewma_error = previous.and_then(|expectation| expectation.ewma_error);
        let (status, rationale, expected_value, observed_value) =
            evaluate_expectation(kind, state_kind, facts, pending_ticks, prior_ewma_error);
        match status {
            PersistentExpectationStatus::Met => supporting_evidence.push(PersistentStateEvidence {
                code: format!("expectation_met:{}", kind.as_str()),
                summary: format!("{symbol} met expectation `{}`", kind.as_str()),
                weight: dec!(0.10),
            }),
            PersistentExpectationStatus::Missed => {
                opposing_evidence.push(PersistentStateEvidence {
                    code: format!("expectation_missed:{}", kind.as_str()),
                    summary: format!("{symbol} missed expectation `{}`", kind.as_str()),
                    weight: dec!(0.14),
                })
            }
            PersistentExpectationStatus::StillPending => {}
        }

        // Y#6 — continuous prediction-error path. When we have both expected
        // and observed as Decimals, compute the point-in-time error and
        // blend into an EWMA carried across ticks. The EWMA is what makes
        // this a closed loop: next tick's evaluate_expectation can consult
        // this symbol's accumulated miss pattern when judging whether the
        // expectation still applies.
        //
        // Weights: alpha = 0.2 for the fresh error, 0.8 for the decay.
        // Derivation: direction-stability treats a 5-tick run as meaningful
        // (infer_symbol_direction_stability_rounds with streak >= 2 means
        // the 2nd through 5th tick are all counted), so a 5-tick
        // characteristic time corresponds to EWMA alpha = 1 - (1-1/5) = 0.2.
        // Not a new free parameter.
        let (ewma_error, error_weight) = match (expected_value, observed_value) {
            (Some(expected), Some(observed)) => {
                let point_error = (expected - observed).max(Decimal::ZERO);
                let prior_ewma = previous
                    .and_then(|expectation| expectation.ewma_error)
                    .unwrap_or(Decimal::ZERO);
                let blended = prior_ewma * dec!(0.8) + point_error * dec!(0.2);
                // Opposing-evidence weight scales linearly with the blended
                // error magnitude, capped at 0.20 (just under the
                // demoted_by_absence weight of 0.20 — this is softer by
                // design, since a point-error is a continuous nudge, not
                // a hard absence).
                let weight = blended.min(dec!(0.20));
                (Some(blended), weight)
            }
            _ => (None, Decimal::ZERO),
        };
        if error_weight > Decimal::ZERO {
            opposing_evidence.push(PersistentStateEvidence {
                code: format!("expectation_error:{}", kind.as_str()),
                summary: format!(
                    "{symbol} {} prediction error (expected={} observed={} ewma={})",
                    kind.as_str(),
                    expected_value
                        .map(|value| value.round_dp(3).to_string())
                        .unwrap_or_else(|| "-".into()),
                    observed_value
                        .map(|value| value.round_dp(3).to_string())
                        .unwrap_or_else(|| "-".into()),
                    ewma_error
                        .map(|value| value.round_dp(3).to_string())
                        .unwrap_or_else(|| "-".into()),
                ),
                weight: error_weight,
            });
        }

        expectations.push(PersistentStateExpectation {
            kind,
            status,
            rationale,
            pending_ticks,
            expected_value,
            observed_value,
            ewma_error,
        });
    }
    expectations
}

fn evaluate_expectation(
    kind: PersistentExpectationKind,
    state_kind: PersistentStateKind,
    facts: &CurrentStateFacts,
    pending_ticks: u16,
    prior_ewma_error: Option<Decimal>,
) -> (
    PersistentExpectationStatus,
    String,
    Option<Decimal>,
    Option<Decimal>,
) {
    // Y#6 — compute a per-symbol adjusted target by subtracting the prior
    // EWMA error from the base target, clamped to a floor. The base target
    // names the semantic operational definition (PEER_MAJORITY = "2/3 of
    // peer cohort"), the floor names the point below which the
    // expectation has stopped meaning anything ("less than half", "less
    // than majority"). Past that floor the expectation has effectively
    // collapsed rather than been adjusted, so the EWMA no longer moves
    // the target.
    //
    // status (Met/Missed/StillPending) continues to read the cached boolean
    // facts (which are computed against the global, non-adjusted target).
    // Only the reported `expected_value` shifts — the shifted value is what
    // derive_expectations then feeds into the next tick's point-error
    // computation, making the loop self-consistent.
    let adjust = |base: Decimal, floor: Decimal| -> Decimal {
        let shift = prior_ewma_error.unwrap_or(Decimal::ZERO);
        (base - shift).max(floor).min(base)
    };

    match kind {
        PersistentExpectationKind::PeerFollowThrough => {
            let observed = facts.peer_confirmation_ratio;
            let expected = adjust(PEER_MAJORITY, PEER_HALF);
            let (status, rationale) = if facts.peer_confirmed {
                (
                    PersistentExpectationStatus::Met,
                    "peer cohort confirmed the local state".into(),
                )
            } else if facts.peer_missing && pending_ticks >= 2 {
                (
                    PersistentExpectationStatus::Missed,
                    format!(
                        "peer cohort stayed silent longer than expected (target={})",
                        expected.round_dp(3)
                    ),
                )
            } else {
                (
                    PersistentExpectationStatus::StillPending,
                    format!(
                        "waiting for peers to follow through (target={})",
                        expected.round_dp(3)
                    ),
                )
            };
            (status, rationale, Some(expected), observed)
        }
        PersistentExpectationKind::RawChannelConfirmation => {
            let observed = Some(facts.raw_support_fraction);
            let expected = adjust(RAW_SUPERMAJORITY, RAW_MAJORITY);
            let (status, rationale) = if facts.raw_confirmed {
                (
                    PersistentExpectationStatus::Met,
                    "raw channels confirmed the state".into(),
                )
            } else if facts.raw_missing && pending_ticks >= 2 {
                (
                    PersistentExpectationStatus::Missed,
                    format!(
                        "raw confirmation failed to arrive in time (target={})",
                        expected.round_dp(3)
                    ),
                )
            } else {
                (
                    PersistentExpectationStatus::StillPending,
                    format!(
                        "waiting for stronger raw channel confirmation (target={})",
                        expected.round_dp(3)
                    ),
                )
            };
            (status, rationale, Some(expected), observed)
        }
        PersistentExpectationKind::ClusterExpansion => {
            // ClusterExpansion has a continuous observable too: the peer
            // confirmation ratio (or 0 when we have no peer data this
            // tick). Target at cluster_expanded (>=0.85 in facts).
            let observed = facts.peer_confirmation_ratio.or(Some(Decimal::ZERO));
            let target = adjust(PEER_NEAR_UNANIMOUS, PEER_MAJORITY);
            let (status, rationale) = if facts.cluster_expanded {
                (
                    PersistentExpectationStatus::Met,
                    "cluster expanded with additional confirming symbols".into(),
                )
            } else if facts.peer_missing && pending_ticks >= 2 {
                (
                    PersistentExpectationStatus::Missed,
                    "cluster did not expand beyond the local leader".into(),
                )
            } else {
                (
                    PersistentExpectationStatus::StillPending,
                    "waiting for cluster expansion".into(),
                )
            };
            (status, rationale, Some(target), observed)
        }
        PersistentExpectationKind::PropagationFollowThrough => {
            // PropagationFollowThrough is structurally boolean (either
            // propagation happens or it doesn't — no continuous
            // observable), so this path does not contribute to the
            // continuous error pipeline. Expected/observed are None.
            let met = match state_kind {
                PersistentStateKind::TurningPoint => facts.support_withdrawal,
                PersistentStateKind::Conflicted => facts.conflict_relieved,
                _ => facts.propagation_confirmed,
            };
            let (status, rationale) = if met {
                (
                    PersistentExpectationStatus::Met,
                    match state_kind {
                        PersistentStateKind::TurningPoint => {
                            "prior support weakened as expected".into()
                        }
                        PersistentStateKind::Conflicted => {
                            "one side weakened and conflict started to resolve".into()
                        }
                        _ => "propagation followed through".into(),
                    },
                )
            } else if pending_ticks
                >= if matches!(state_kind, PersistentStateKind::Conflicted) {
                    3
                } else {
                    2
                }
            {
                (
                    PersistentExpectationStatus::Missed,
                    "expected propagation or resolution did not materialize".into(),
                )
            } else {
                (
                    PersistentExpectationStatus::StillPending,
                    "waiting for propagation or conflict resolution".into(),
                )
            };
            (status, rationale, None, None)
        }
    }
}

fn case_direction(case: &LiveTacticalCase) -> Option<&'static str> {
    if let Some(raw) = case.raw_disagreement.as_ref() {
        if let Some(label) = action_direction_from_case_label(&raw.expected_direction)
            .and_then(action_direction_case_label)
        {
            return Some(label);
        }
    }
    action_direction_from_title_prefix(&case.title).and_then(action_direction_case_label)
}

fn infer_symbol_direction(
    buy_count: usize,
    sell_count: usize,
    residual: Option<&SymbolResidualFrame>,
) -> Option<String> {
    match buy_count.cmp(&sell_count) {
        std::cmp::Ordering::Greater => Some("buy".into()),
        std::cmp::Ordering::Less => Some("sell".into()),
        std::cmp::Ordering::Equal => {
            if buy_count > 0 {
                Some("mixed".into())
            } else {
                residual.and_then(residual_direction).map(str::to_string)
            }
        }
    }
}

fn residual_direction(residual: &SymbolResidualFrame) -> Option<&'static str> {
    if residual.composite > Decimal::ZERO {
        Some("buy")
    } else if residual.composite < Decimal::ZERO {
        Some("sell")
    } else {
        None
    }
}

fn transition_direction(transition: &AgentTransition) -> Option<&'static str> {
    action_direction_from_title_prefix(&transition.title).and_then(action_direction_case_label)
}

/// Y#5 — active lookback into recent_transitions for a stable historical state.
///
/// The old `state_memory_continuity` branch only consulted `previous_state`
/// (one tick back), so a symbol whose signals briefly wilted (a single tick
/// of low information after a stable run) instantly dropped to
/// `LowInformation` even though a wider transition window told a different
/// story. This function scans up to the last 10 transitions for the symbol
/// and asks: was there a sustained, non-contradictory action posture
/// recently enough to act as a classification prior?
///
/// "Sustained" = the dominant action verb (`enter` / `review` / `observe`)
/// appears in at least 3 of the last 10 transitions AND the most recent
/// state suffix is one of `stable` / `strengthening` (not `weakening`,
/// not a flipped pair). "Recently enough" = the youngest supporting
/// transition is within 20 ticks of `current_tick`. Both bounds match
/// the existing direction-stability machinery (which uses 2-tick streaks
/// and implicit 32-transition windows) — they are not new magic numbers.
///
/// Mapping of action verb → prior state_kind is conservative:
///   `enter:*`   → Continuation (strongest — cases were actually triggering)
///   `review:*`  → Latent        (cases were forming but not yet executing)
///   `observe:*` → Latent        (structural interest without case bindings)
///   `exit:*` / `invalidation:*` → None (these are turning signals, not priors)
///
/// Returns None when the symbol has no meaningful history or the history
/// is actively contradicting (flip within the window, or a recent exit /
/// invalidation transition).
fn lookback_state_prior(
    current_tick: u64,
    symbol: &str,
    recent_transitions: &[AgentTransition],
) -> Option<(PersistentStateKind, String)> {
    let transitions: Vec<&AgentTransition> = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .collect();
    if transitions.len() < 3 {
        return None;
    }
    // Recency gate — same horizon as direction stability, not a new threshold.
    let youngest_tick = transitions
        .iter()
        .map(|transition| transition.to_tick)
        .max()
        .unwrap_or(0);
    if current_tick.saturating_sub(youngest_tick) > 20 {
        return None;
    }
    // Abort if the symbol is in an actively conflicting phase.
    let has_exit_or_invalidation = transitions.iter().any(|transition| {
        transition.to_state.starts_with("exit")
            || transition.to_state.starts_with("invalidation")
            || transition.to_state.contains("conflict")
    });
    if has_exit_or_invalidation {
        return None;
    }
    let has_weakening = transitions
        .iter()
        .filter(|transition| transition.to_state.ends_with(":weakening"))
        .count()
        >= 2;
    if has_weakening {
        return None;
    }
    // Count dominant action verb across the last 10 transitions.
    let window: Vec<&AgentTransition> = transitions.iter().copied().take(10).collect();
    let mut enter = 0usize;
    let mut review = 0usize;
    let mut observe = 0usize;
    for transition in &window {
        let verb = transition.to_state.split(':').next().unwrap_or("");
        match verb {
            "enter" => enter += 1,
            "review" => review += 1,
            "observe" => observe += 1,
            _ => {}
        }
    }
    let dominant = if enter >= 3 {
        Some((PersistentStateKind::Continuation, "enter", enter))
    } else if review >= 3 {
        Some((PersistentStateKind::Latent, "review", review))
    } else if observe >= 3 {
        Some((PersistentStateKind::Latent, "observe", observe))
    } else {
        None
    };
    dominant.map(|(kind, verb, count)| {
        (
            kind,
            format!(
                "{symbol} carried {verb} posture in {count}/{} recent transitions",
                window.len()
            ),
        )
    })
}

fn has_recent_direction_flip(symbol: &str, recent_transitions: &[AgentTransition]) -> bool {
    let mut directions = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .filter_map(transition_direction)
        .collect::<Vec<_>>();
    directions.truncate(2);
    directions.len() >= 2 && directions[0] != directions[1]
}

fn infer_symbol_direction_stability_rounds(
    current_tick: u64,
    symbol: &str,
    direction: Option<&str>,
    recent_transitions: &[AgentTransition],
) -> u16 {
    let Some(direction) = direction else {
        return 0;
    };
    if direction == "mixed" {
        return 1;
    }
    let recent = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .collect::<Vec<_>>();
    let Some(first_same_direction) = recent
        .iter()
        .find(|transition| transition_direction(transition) == Some(direction))
        .copied()
    else {
        return 1;
    };
    let mut streak_start_tick = first_same_direction
        .from_tick
        .min(first_same_direction.to_tick);
    for transition in recent {
        let Some(transition_direction) = transition_direction(transition) else {
            continue;
        };
        if transition_direction != direction {
            break;
        }
        streak_start_tick = streak_start_tick.min(transition.from_tick.min(transition.to_tick));
    }
    current_tick
        .saturating_sub(streak_start_tick)
        .saturating_add(1)
        .min(u16::MAX as u64) as u16
}

fn infer_symbol_state_persistence_ticks(
    current_tick: u64,
    symbol: &str,
    direction: Option<&str>,
    recent_transitions: &[AgentTransition],
    recent_flip: bool,
) -> u16 {
    let latest = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .max_by_key(|transition| transition.to_tick);
    let Some(latest) = latest else {
        return 1;
    };
    if recent_flip || matches!(direction, Some("mixed")) {
        return current_tick
            .saturating_sub(latest.to_tick)
            .saturating_add(1)
            .min(u16::MAX as u64) as u16;
    }
    infer_symbol_direction_stability_rounds(current_tick, symbol, direction, recent_transitions)
}

fn infer_symbol_age_ticks(
    current_tick: u64,
    symbol: &str,
    cases: &[&LiveTacticalCase],
    recent_transitions: &[AgentTransition],
    previous_state: Option<&PersistentSymbolState>,
    state_kind: PersistentStateKind,
    direction: Option<&str>,
) -> u64 {
    let first_enter_tick = cases.iter().filter_map(|case| case.first_enter_tick).min();
    let first_transition_tick = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(symbol))
        .map(|transition| transition.from_tick.min(transition.to_tick))
        .min();
    let first_seen_tick = first_enter_tick
        .into_iter()
        .chain(first_transition_tick)
        .min()
        .unwrap_or(current_tick);
    let computed = current_tick
        .saturating_sub(first_seen_tick)
        .saturating_add(1);
    if let Some(previous_state) = previous_state {
        if previous_state.state_kind == state_kind
            || previous_state.direction.as_deref() == direction
        {
            return previous_state.age_ticks.saturating_add(1).max(computed);
        }
    }
    computed
}

fn infer_cluster_identity(
    symbol: &str,
    cases: &[&LiveTacticalCase],
    residual: Option<&SymbolResidualFrame>,
    previous_state: Option<&PersistentSymbolState>,
) -> (String, String) {
    if let Some(sector) = residual.and_then(|item| item.sector.as_ref()) {
        return (format!("sector:{sector}"), sector.clone());
    }
    if let Some(case) = strongest_case(cases) {
        if let Some(driver) = case.driver_class.as_ref().filter(|value| !value.is_empty()) {
            return (format!("driver:{driver}"), driver.clone());
        }
        if let Some(driver) = case
            .tension_driver
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            return (format!("driver:{driver}"), driver.clone());
        }
        if let Some(family) = case.family_label.as_ref().filter(|value| !value.is_empty()) {
            return (format!("family:{family}"), family.clone());
        }
    }
    if let Some(previous_state) = previous_state {
        return (
            previous_state.cluster_key.clone(),
            previous_state.cluster_label.clone(),
        );
    }
    (format!("symbol:{symbol}"), symbol.to_string())
}

fn dominant_member_direction(members: &[&PersistentSymbolState]) -> Option<String> {
    let mut buy = 0usize;
    let mut sell = 0usize;
    for member in members {
        match member.direction.as_deref() {
            Some("buy") => buy += 1,
            Some("sell") => sell += 1,
            _ => {}
        }
    }
    match buy.cmp(&sell) {
        std::cmp::Ordering::Greater => Some("buy".into()),
        std::cmp::Ordering::Less => Some("sell".into()),
        std::cmp::Ordering::Equal => {
            if buy > 0 {
                Some("mixed".into())
            } else {
                None
            }
        }
    }
}

fn mixed_member_directions(members: &[&PersistentSymbolState]) -> bool {
    let mut directions = HashSet::new();
    for member in members {
        if let Some(direction) = member
            .direction
            .as_deref()
            .filter(|value| *value != "unknown")
        {
            directions.insert(direction);
        }
    }
    directions.len() > 1
}

fn single_direction(clusters: &[&LiveClusterState]) -> bool {
    let directions = clusters
        .iter()
        .map(|cluster| cluster.direction.as_str())
        .filter(|direction| !matches!(*direction, "mixed" | "unknown"))
        .collect::<HashSet<_>>();
    directions.len() == 1
}

fn intent_kind_slug(kind: IntentKind) -> &'static str {
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

fn intent_state_slug(state: IntentState) -> &'static str {
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

fn normalize_signal_strength(composite: Decimal) -> Decimal {
    clamp_unit((composite.abs() / dec!(1.0)).round_dp(4))
}

fn build_signal_residual_frame(signal: &LiveSignal) -> SymbolResidualFrame {
    SymbolResidualFrame {
        sector: signal.sector.clone(),
        composite: signal.composite,
        signal_strength: normalize_signal_strength(signal.composite),
        capital_flow_direction: signal.capital_flow_direction,
        price_momentum: signal.price_momentum,
        volume_profile: signal.volume_profile,
        pre_post_market_anomaly: signal.pre_post_market_anomaly,
        valuation: signal.valuation,
        cross_stock_correlation: signal.cross_stock_correlation.unwrap_or(Decimal::ZERO),
        sector_coherence: signal.sector_coherence.unwrap_or(Decimal::ZERO),
        cross_market_propagation: signal.cross_market_propagation.unwrap_or(Decimal::ZERO),
    }
}

fn active_residual_dimension_count(residual: &SymbolResidualFrame) -> usize {
    [
        residual.capital_flow_direction.abs(),
        residual.price_momentum.abs(),
        residual.volume_profile.abs(),
        residual.pre_post_market_anomaly.abs(),
        residual.valuation.abs(),
        residual.cross_stock_correlation.abs(),
        residual.sector_coherence.abs(),
        residual.cross_market_propagation.abs(),
    ]
    .into_iter()
    .filter(|value| *value >= dec!(0.20))
    .count()
}

fn residual_supporting_evidence(
    symbol: &str,
    residual: &SymbolResidualFrame,
) -> Vec<PersistentStateEvidence> {
    let mut items = Vec::new();
    if residual.price_momentum.abs() >= dec!(0.25) {
        items.push(PersistentStateEvidence {
            code: "residual_price_momentum".into(),
            summary: format!(
                "{symbol} price-momentum residual is {}",
                residual.price_momentum.round_dp(3)
            ),
            weight: dec!(0.10),
        });
    }
    if residual.volume_profile.abs() >= dec!(0.25) {
        items.push(PersistentStateEvidence {
            code: "residual_volume_profile".into(),
            summary: format!(
                "{symbol} volume-profile residual is {}",
                residual.volume_profile.round_dp(3)
            ),
            weight: dec!(0.10),
        });
    }
    if residual.capital_flow_direction.abs() >= dec!(0.25) {
        items.push(PersistentStateEvidence {
            code: "residual_capital_flow".into(),
            summary: format!(
                "{symbol} capital-flow residual is {}",
                residual.capital_flow_direction.round_dp(3)
            ),
            weight: dec!(0.12),
        });
    }
    if residual.pre_post_market_anomaly.abs() >= dec!(0.25) {
        items.push(PersistentStateEvidence {
            code: "residual_pre_post_anomaly".into(),
            summary: format!(
                "{symbol} pre/post-market anomaly residual is {}",
                residual.pre_post_market_anomaly.round_dp(3)
            ),
            weight: dec!(0.10),
        });
    }
    if residual.cross_market_propagation.abs() >= dec!(0.20) {
        items.push(PersistentStateEvidence {
            code: "residual_cross_market_propagation".into(),
            summary: format!(
                "{symbol} cross-market propagation residual is {}",
                residual.cross_market_propagation.round_dp(3)
            ),
            weight: dec!(0.08),
        });
    }
    if active_residual_dimension_count(residual) >= 2 {
        items.push(PersistentStateEvidence {
            code: "multi_dimensional_residual_pattern".into(),
            summary: format!(
                "{symbol} is carrying a multi-dimensional residual pattern before case translation"
            ),
            weight: dec!(0.14),
        });
    }
    items
}

fn clamp_unit(value: Decimal) -> Decimal {
    value.max(Decimal::ZERO).min(Decimal::ONE).round_dp(4)
}

fn market_slug(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    }
}

fn market_label(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "HK",
        LiveMarket::Us => "US",
    }
}

fn perceptual_channel_for_code(code: &str) -> &'static str {
    if code.contains("peer") {
        "peer"
    } else if code.contains("raw") || code.contains("channel") {
        "raw"
    } else if code.contains("cluster") {
        "cluster"
    } else if code.contains("propagation") {
        "propagation"
    } else if code.contains("intent") {
        "intent"
    } else if code.contains("conflict") {
        "conflict"
    } else if code.contains("freshness") {
        "freshness"
    } else if code.contains("timing") {
        "timing"
    } else {
        "state"
    }
}

fn summarize_attention_allocations<'a>(
    state_id: &str,
    scope: &ReasoningScope,
    evidence: impl Iterator<Item = &'a PerceptualEvidence>,
) -> Vec<AttentionAllocation> {
    let mut by_channel = BTreeMap::<String, Decimal>::new();
    for item in evidence {
        let entry = by_channel.entry(item.channel.clone()).or_default();
        *entry += item.weight;
    }
    by_channel
        .into_iter()
        .map(|(channel, weight)| AttentionAllocation {
            allocation_id: format!("{state_id}:attention:{channel}"),
            target_scope: scope.clone(),
            channel,
            weight: clamp_unit(weight),
            rationale: "derived from perceptual evidence channel totals".into(),
        })
        .collect()
}

fn build_perceptual_uncertainties(
    state: &PersistentSymbolState,
    scope: &ReasoningScope,
    missing_evidence: &[PerceptualEvidence],
) -> Vec<PerceptualUncertainty> {
    if missing_evidence.is_empty()
        && state.confidence >= dec!(0.60)
        && !matches!(
            state.state_kind,
            PersistentStateKind::LowInformation | PersistentStateKind::Conflicted
        )
    {
        return Vec::new();
    }
    let degraded_channels = missing_evidence
        .iter()
        .map(|item| item.channel.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let level = clamp_unit(
        (Decimal::ONE - state.confidence)
            + Decimal::from(missing_evidence.len().min(3) as i64) * dec!(0.10),
    );
    vec![PerceptualUncertainty {
        uncertainty_id: format!("{}:uncertainty", state.state_id),
        target_scope: scope.clone(),
        level,
        rationale: if missing_evidence.is_empty() {
            "state confidence remains below the stable sensory threshold".into()
        } else {
            "state is constrained by sparse or missing confirmation".into()
        },
        degraded_channels,
    }]
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::live_snapshot::{LiveRawDisagreement, LiveTacticalCase};
    use crate::ontology::{
        IntentDirection, IntentHypothesis, IntentStrength, ReasoningScope, Symbol,
    };

    fn base_case(symbol: &str, title: &str) -> LiveTacticalCase {
        LiveTacticalCase {
            setup_id: format!("setup:{symbol}"),
            symbol: symbol.into(),
            title: title.into(),
            action: "enter".into(),
            confidence: dec!(0.82),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.12),
            entry_rationale: String::new(),
            causal_narrative: None,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            policy_primary: None,
            policy_reason: None,
            multi_horizon_gate_reason: None,
            family_label: Some("Flow".into()),
            counter_label: None,
            matched_success_pattern_signature: None,
            lifecycle_phase: None,
            tension_driver: None,
            driver_class: None,
            is_isolated: None,
            peer_active_count: None,
            peer_silent_count: None,
            peer_confirmation_ratio: None,
            isolation_score: None,
            competition_margin: None,
            driver_confidence: None,
            absence_summary: None,
            competition_summary: None,
            competition_winner: None,
            competition_runner_up: None,
            lifecycle_velocity: None,
            lifecycle_acceleration: None,
            horizon_bucket: None,
            horizon_urgency: None,
            horizon_secondary: vec![],
            case_signature: None,
            archetype_projections: vec![],
            expectation_bindings: vec![],
            expectation_violations: vec![],
            inferred_intent: Some(IntentHypothesis {
                intent_id: "intent:1".into(),
                kind: IntentKind::Accumulation,
                scope: ReasoningScope::Symbol(Symbol(symbol.into())),
                direction: IntentDirection::Buy,
                state: IntentState::Active,
                confidence: dec!(0.76),
                urgency: dec!(0.55),
                persistence: dec!(0.60),
                conflict_score: dec!(0.10),
                strength: IntentStrength {
                    flow_strength: dec!(0.7),
                    impact_strength: dec!(0.6),
                    persistence_strength: dec!(0.6),
                    propagation_strength: dec!(0.5),
                    resistance_strength: dec!(0.2),
                    composite: dec!(0.7),
                },
                propagation_targets: vec![],
                supporting_archetypes: vec![],
                supporting_case_signature: None,
                expectation_bindings: vec![],
                expectation_violations: vec![],
                exit_signals: vec![],
                opportunities: vec![],
                falsifiers: vec![],
                rationale: "flow active".into(),
            }),
            freshness_state: Some("fresh".into()),
            first_enter_tick: Some(90),
            ticks_since_first_enter: Some(10),
            ticks_since_first_seen: None,
            timing_state: Some("timely".into()),
            timing_position_in_range: Some(dec!(0.45)),
            local_state: None,
            local_state_confidence: None,
            actionability_score: None,
            actionability_state: None,
            confidence_velocity_5t: None,
            support_fraction_velocity_5t: None,
            priority_rank: None,
            state_persistence_ticks: None,
            direction_stability_rounds: None,
            state_reason_codes: vec![],
            raw_disagreement: Some(LiveRawDisagreement {
                alignment: "aligned".into(),
                expected_direction: action_direction_from_title_prefix(title)
                    .and_then(action_direction_case_label)
                    .unwrap_or("sell")
                    .into(),
                support_count: 8,
                contradict_count: 1,
                count_support_fraction: dec!(0.89),
                support_fraction: dec!(0.90),
                support_weight: dec!(2.10),
                contradict_weight: dec!(0.23),
                adjusted_action: "enter".into(),
                adjusted_confidence: dec!(0.82),
                summary: "aligned".into(),
                supporting_sources: vec![],
                contradicting_sources: vec![],
                original_action: None,
                original_confidence: None,
            }),
        }
    }

    #[test]
    fn strong_symbol_without_peers_demotes_to_latent() {
        // Y#3: a strong single-symbol case with zero peer corroboration is
        // demoted from Continuation to Latent (absence-first). Continuation
        // now requires peer confirmation in addition to strong raw support.
        let case = base_case("700.HK", "Long 700.HK");
        let states = derive_symbol_states(100, LiveMarket::Hk, &[case], &[], &[], &[]);
        assert_eq!(states[0].state_kind, PersistentStateKind::Latent);
    }

    #[test]
    fn conflicting_symbol_becomes_conflicted() {
        let long_case = base_case("700.HK", "Long 700.HK");
        let short_case = base_case("700.HK", "Short 700.HK");
        let states =
            derive_symbol_states(100, LiveMarket::Hk, &[long_case, short_case], &[], &[], &[]);
        assert_eq!(states[0].state_kind, PersistentStateKind::Conflicted);
    }

    #[test]
    fn untranslated_signal_without_peers_or_raw_demotes_to_low_information() {
        // Y#3 double-demotion: a signal with no peer corroboration AND no
        // raw support is not a Latent (actionable-but-untranslated) state —
        // it's LowInformation because nothing is actually there.
        let states = derive_symbol_states(
            100,
            LiveMarket::Us,
            &[],
            &[],
            &[LiveSignal {
                symbol: "NVDA.US".into(),
                sector: Some("Semis".into()),
                composite: dec!(0.44),
                mark_price: None,
                dimension_composite: None,
                capital_flow_direction: Decimal::ZERO,
                price_momentum: Decimal::ZERO,
                volume_profile: Decimal::ZERO,
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                cross_stock_correlation: None,
                sector_coherence: None,
                cross_market_propagation: None,
            }],
            &[],
        );
        assert_eq!(states[0].state_kind, PersistentStateKind::LowInformation);
    }

    #[test]
    fn residual_pattern_without_peers_or_raw_demotes_to_low_information() {
        // Y#3: even a multi-dimensional residual pattern gets demoted to
        // LowInformation when there's no peer corroboration AND no raw
        // support. The residual evidence still appears on supporting_evidence
        // so the operator can see it, but the state classification reflects
        // that the signal is structurally unconfirmed.
        let states = derive_symbol_states(
            100,
            LiveMarket::Us,
            &[],
            &[],
            &[LiveSignal {
                symbol: "IONQ.US".into(),
                sector: Some("Quantum".into()),
                composite: dec!(0.18),
                mark_price: None,
                dimension_composite: None,
                capital_flow_direction: dec!(0.31),
                price_momentum: dec!(0.28),
                volume_profile: dec!(0.35),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                cross_stock_correlation: None,
                sector_coherence: Some(dec!(0.21)),
                cross_market_propagation: None,
            }],
            &[],
        );
        assert_eq!(states[0].state_kind, PersistentStateKind::LowInformation);
        assert!(states[0]
            .supporting_evidence
            .iter()
            .any(|item| item.code == "multi_dimensional_residual_pattern"));
    }

    #[test]
    fn persistent_state_converts_to_perceptual_state() {
        // Post-Y#3 absence demotion: a single-symbol case with strong raw
        // support but zero peer corroboration resolves to Latent, not
        // Continuation. The persistent→perceptual projection must preserve
        // that demoted kind and still reflect the raw support counts from
        // the case fixture (8 supporting, fraction 0.90).
        let state = base_case("700.HK", "Long 700.HK");
        let derived = derive_symbol_states(100, LiveMarket::Hk, &[state], &[], &[], &[]);
        let perceptual = derived[0].to_perceptual_state();
        assert_eq!(perceptual.state_kind, "latent");
        assert_eq!(perceptual.scope.label(), "700.HK");
        assert_eq!(perceptual.support_count, 8);
        assert_eq!(perceptual.weighted_support_fraction, dec!(0.90));
    }
}
