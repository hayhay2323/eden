use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::ontology::microstructure::LevelChange;
use crate::ontology::reasoning::ActionNode;
use crate::ontology::store::ObjectStore;
use crate::ontology::Symbol;
use crate::ontology::{
    action_direction_from_intent_direction, action_direction_sign,
    infer_action_direction_from_texts, ArchetypeProjection, CaseSignature, ExpectationBinding,
    ExpectationViolation, IntentHypothesis,
};
use crate::pipeline::raw_events::{RawEventStore, RawQueryWindow, RawSourceExport};
use crate::pipeline::state_engine::PersistentSymbolState;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum LiveMarket {
    #[serde(alias = "hk", alias = "HK")]
    Hk,
    #[serde(alias = "us", alias = "US")]
    Us,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSnapshot {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub market_phase: String,
    pub market_active: bool,
    pub stock_count: usize,
    pub edge_count: usize,
    pub hypothesis_count: usize,
    pub observation_count: usize,
    /// Count of Eden's **internally-tracked position fingerprints** — NOT the
    /// operator's broker positions. On the US runtime this counts entries in
    /// `UsPositionTracker` (positions Eden believes it entered via its own
    /// simulated `enter`/`exit` flow). Operators watching this to gauge their
    /// real broker state will be misled; use Longport / broker APIs for that.
    pub active_positions: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_position_nodes: Vec<ActionNode>,
    #[serde(deserialize_with = "deserialize_market_regime")]
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub scorecard: LiveScorecard,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tactical_cases: Vec<LiveTacticalCase>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hypothesis_tracks: Vec<LiveHypothesisTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_transitions: Vec<crate::agent::AgentTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_signals: Vec<LiveSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub convergence_scores: Vec<LiveSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pressures: Vec<LivePressure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backward_chains: Vec<LiveBackwardChain>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_leaders: Vec<LiveCausalLeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LiveEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_market_signals: Vec<LiveCrossMarketSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_market_anomalies: Vec<LiveCrossMarketAnomaly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structural_deltas: Vec<LiveStructuralDelta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_senses: Vec<LivePropagationSense>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_microstructure: Vec<LiveRawMicrostructure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sources: Vec<LiveRawSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signal_translation_gaps: Vec<LiveSignalTranslationGap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cluster_states: Vec<LiveClusterState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_states: Vec<PersistentSymbolState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_summary: Option<LiveWorldSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temporal_bars: Vec<LiveTemporalBar>,
    #[serde(default, deserialize_with = "deserialize_lineage")]
    pub lineage: Vec<LiveLineageMetric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub success_patterns: Vec<LiveSuccessPattern>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveRawMicrostructure {
    pub symbol: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveRawSource {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub scope: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_end: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSignalTranslationGap {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub composite: Decimal,
    #[serde(default)]
    pub pre_post_market_anomaly: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub capital_flow_direction: Decimal,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_highlights: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveClusterState {
    pub cluster_key: String,
    pub label: String,
    pub direction: String,
    pub state: String,
    pub confidence: Decimal,
    pub member_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leader_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub laggard_symbols: Vec<String>,
    pub summary: String,
    /// Ticks since this cluster identity first appeared with a non-low-information state.
    #[serde(default)]
    pub age_ticks: u64,
    /// Ticks the cluster has spent in its current `state` value.
    /// Resets whenever `state` flips.
    #[serde(default)]
    pub state_persistence_ticks: u16,
    /// Relative trend versus the previous tick:
    /// `"strengthening" | "weakening" | "stable"`. Empty when no prior snapshot exists.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trend: String,
    /// Human summary of the most recent state flip, written once when the flip happens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveWorldSummary {
    pub regime: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dominant_clusters: Vec<String>,
    pub summary: String,
    /// Ticks since the world first produced any non-low-information regime.
    #[serde(default)]
    pub age_ticks: u64,
    /// Ticks the world has spent in its current `regime` value.
    /// Resets on regime flip.
    #[serde(default)]
    pub state_persistence_ticks: u16,
    /// Relative trend versus the previous tick:
    /// `"strengthening" | "weakening" | "stable"`. Empty when no prior snapshot exists.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trend: String,
    /// Human summary of the most recent regime flip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveRawDisagreement {
    pub alignment: String,
    pub expected_direction: String,
    pub support_count: usize,
    pub contradict_count: usize,
    pub count_support_fraction: Decimal,
    pub support_fraction: Decimal,
    pub support_weight: Decimal,
    pub contradict_weight: Decimal,
    pub adjusted_action: String,
    pub adjusted_confidence: Decimal,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradicting_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_confidence: Option<Decimal>,
}

fn to_live_raw_source(item: RawSourceExport) -> LiveRawSource {
    LiveRawSource {
        source: item.source,
        symbol: item.symbol.map(|symbol| symbol.0),
        scope: item.scope,
        summary: item.summary,
        window_start: item.window_start.and_then(|ts| {
            ts.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
        window_end: item.window_end.and_then(|ts| {
            ts.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
        payload: item.payload,
    }
}

fn select_raw_source_symbols(
    tactical_cases: &[LiveTacticalCase],
    top_signals: &[LiveSignal],
    active_position_nodes: &[ActionNode],
) -> Vec<crate::ontology::Symbol> {
    let mut symbols = Vec::new();
    for item in tactical_cases {
        if !item.symbol.is_empty() {
            let symbol = crate::ontology::Symbol(item.symbol.clone());
            if !symbols.contains(&symbol) {
                symbols.push(symbol);
            }
        }
    }
    for node in active_position_nodes {
        let symbol = node.symbol.clone();
        if !symbols.contains(&symbol) {
            symbols.push(symbol);
        }
    }
    for item in top_signals.iter().take(10) {
        let symbol = crate::ontology::Symbol(item.symbol.clone());
        if !symbols.contains(&symbol) {
            symbols.push(symbol);
        }
        if symbols.len() >= 10 {
            break;
        }
    }
    symbols
}

pub fn build_live_raw_microstructure(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    tactical_cases: &[LiveTacticalCase],
    top_signals: &[LiveSignal],
    active_position_nodes: &[ActionNode],
    window: time::Duration,
) -> Vec<LiveRawMicrostructure> {
    select_raw_source_symbols(tactical_cases, top_signals, active_position_nodes)
        .into_iter()
        .filter_map(|symbol| {
            let explanation = raw_events.explain_microstructure(
                &symbol,
                RawQueryWindow::LastDuration(window),
                store,
            );
            (explanation.summary != "no recent raw observations").then(|| LiveRawMicrostructure {
                symbol: symbol.0,
                summary: explanation.summary,
                trade_summary: explanation.trade_summary,
                depth_summary: explanation.depth_summary,
                broker_summary: explanation.broker_summary,
            })
        })
        .collect()
}

pub fn build_live_raw_sources(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    tactical_cases: &[LiveTacticalCase],
    top_signals: &[LiveSignal],
    active_position_nodes: &[ActionNode],
    window: time::Duration,
) -> Vec<LiveRawSource> {
    let mut raw_sources = raw_events
        .export_market_sources(RawQueryWindow::LastDuration(window))
        .into_iter()
        .map(to_live_raw_source)
        .collect::<Vec<_>>();

    for symbol in select_raw_source_symbols(tactical_cases, top_signals, active_position_nodes) {
        raw_sources.extend(
            raw_events
                .export_longport_sources(&symbol, RawQueryWindow::LastDuration(window), store)
                .into_iter()
                .map(to_live_raw_source),
        );
    }

    raw_sources
}

pub fn build_signal_translation_gaps(
    tactical_cases: &[LiveTacticalCase],
    top_signals: &[LiveSignal],
    raw_sources: &[LiveRawSource],
    limit: usize,
) -> Vec<LiveSignalTranslationGap> {
    let case_symbols = tactical_cases
        .iter()
        .filter(|item| !item.symbol.is_empty())
        .map(|item| item.symbol.as_str())
        .collect::<std::collections::HashSet<_>>();

    let mut raw_by_symbol = HashMap::<&str, Vec<&LiveRawSource>>::new();
    for item in raw_sources {
        if let Some(symbol) = item.symbol.as_deref() {
            raw_by_symbol.entry(symbol).or_default().push(item);
        }
    }

    top_signals
        .iter()
        .filter(|signal| !case_symbols.contains(signal.symbol.as_str()))
        .filter(|signal| {
            signal.composite.abs() >= Decimal::new(35, 2)
                || signal.pre_post_market_anomaly.abs() >= Decimal::new(5, 1)
                || signal.price_momentum.abs() >= Decimal::new(7, 1)
        })
        .take(limit)
        .map(|signal| {
            let raw_highlights = raw_by_symbol
                .get(signal.symbol.as_str())
                .into_iter()
                .flat_map(|items| items.iter().copied())
                .filter(|item| !item.summary.starts_with("no recent "))
                .filter(|item| {
                    matches!(
                        item.source.as_str(),
                        "trade"
                            | "quote"
                            | "calc_index"
                            | "option_surface"
                            | "capital_flow"
                            | "capital_distribution"
                            | "candlestick"
                            | "intraday"
                    )
                })
                .take(4)
                .map(|item| format!("{}: {}", item.source, item.summary))
                .collect::<Vec<_>>();

            let summary = if raw_highlights.is_empty() {
                "strong top signal is not yet represented in tactical cases".to_string()
            } else {
                format!(
                    "strong top signal is not yet represented in tactical cases; {}",
                    raw_highlights.join(" | ")
                )
            };

            LiveSignalTranslationGap {
                symbol: signal.symbol.clone(),
                sector: signal.sector.clone(),
                composite: signal.composite,
                pre_post_market_anomaly: signal.pre_post_market_anomaly,
                price_momentum: signal.price_momentum,
                capital_flow_direction: signal.capital_flow_direction,
                summary,
                raw_highlights,
            }
        })
        .collect()
}

pub fn build_signal_translation_cases(
    gaps: &[LiveSignalTranslationGap],
    limit: usize,
) -> Vec<LiveTacticalCase> {
    gaps.iter()
        .take(limit)
        .map(|gap| {
            let direction = if gap.price_momentum != Decimal::ZERO {
                if gap.price_momentum > Decimal::ZERO {
                    "Long"
                } else {
                    "Short"
                }
            } else if gap.composite >= Decimal::ZERO {
                "Long"
            } else {
                "Short"
            };
            let confidence = (Decimal::new(45, 2) + gap.composite.abs() * Decimal::new(4, 1))
                .min(Decimal::new(85, 2))
                .max(Decimal::new(55, 2));
            LiveTacticalCase {
                setup_id: format!("translation_gap:{}", gap.symbol),
                symbol: gap.symbol.clone(),
                title: format!("{direction} {} (signal translation)", gap.symbol),
                action: "review".into(),
                confidence,
                confidence_gap: Decimal::ZERO,
                heuristic_edge: gap.composite.abs(),
                entry_rationale: gap.summary.clone(),
                causal_narrative: Some(gap.summary.clone()),
                review_reason_code: Some("signal_translation_gap".into()),
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: Some("Signal Translation".into()),
                counter_label: None,
                matched_success_pattern_signature: None,
                lifecycle_phase: None,
                tension_driver: Some("signal_translation".into()),
                driver_class: Some("orphan_signal".into()),
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
                horizon_bucket: Some("fast5m".into()),
                horizon_urgency: Some("immediate".into()),
                horizon_secondary: vec![],
                case_signature: None,
                archetype_projections: vec![],
                expectation_bindings: vec![],
                expectation_violations: vec![],
                inferred_intent: None,
                freshness_state: Some("fresh".into()),
                first_enter_tick: None,
                ticks_since_first_enter: None,
                ticks_since_first_seen: None,
                timing_state: Some("range_unknown".into()),
                timing_position_in_range: None,
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
                raw_disagreement: None,
            }
        })
        .collect()
}

pub fn materialize_signal_translation_cases(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    gaps: &[LiveSignalTranslationGap],
    window: time::Duration,
    limit: usize,
    allow_enter_promotion: bool,
) -> Vec<LiveTacticalCase> {
    let mut cases = build_signal_translation_cases(gaps, limit);
    apply_raw_disagreement_layer(raw_events, store, &mut cases, window);
    cases.retain(|case| {
        case.raw_disagreement
            .as_ref()
            .map(|item| item.alignment.as_str() != "conflicted")
            .unwrap_or(true)
    });

    if allow_enter_promotion {
        for case in &mut cases {
            if translation_case_enter_ready(case) {
                let original_action = case.action.clone();
                let original_confidence = case.confidence;
                case.action = "enter".into();
                case.confidence = case.confidence.max(Decimal::new(68, 2));
                case.review_reason_code = None;
                case.policy_primary = Some("translation_confirmed".into());
                case.policy_reason =
                    Some("orphan signal was confirmed by post-open raw follow-through".into());
                if let Some(item) = case.raw_disagreement.as_mut() {
                    if item.original_action.is_none() {
                        item.original_action = Some(original_action);
                    }
                    if item.original_confidence.is_none() {
                        item.original_confidence = Some(original_confidence);
                    }
                    item.adjusted_action = case.action.clone();
                    item.adjusted_confidence = case.confidence;
                    item.summary = format!(
                        "{}; translation candidate promoted after raw follow-through",
                        item.summary
                    );
                }
            }
        }
    }

    cases
}

fn translation_case_enter_ready(case: &LiveTacticalCase) -> bool {
    let Some(item) = case.raw_disagreement.as_ref() else {
        return false;
    };
    if item.alignment.as_str() != "aligned" || case.action != "review" {
        return false;
    }
    let has_quote = item
        .supporting_sources
        .iter()
        .any(|source| source == "quote");
    let has_flow = item
        .supporting_sources
        .iter()
        .any(|source| matches!(source.as_str(), "trade" | "intraday" | "candlestick"));
    has_quote && has_flow
}

pub fn action_surface_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

pub fn enforce_orphan_action_cap(tactical_cases: &mut [LiveTacticalCase]) {
    for case in tactical_cases.iter_mut() {
        let is_orphan = case.driver_class.as_deref() == Some("orphan_signal")
            || case.tension_driver.as_deref() == Some("orphan_signal");
        if !is_orphan || case.action != "enter" {
            continue;
        }

        let original_action = case.action.clone();
        let original_confidence = case.confidence;
        case.action = "review".into();
        case.confidence = case.confidence.min(Decimal::new(65, 2));
        case.review_reason_code = Some("orphan_signal_cap".into());
        case.policy_primary = Some("orphan_signal_capped".into());
        case.policy_reason = Some(
            "orphan-signal cases are capped at review until topology-backed confirmation arrives"
                .into(),
        );

        if let Some(item) = case.raw_disagreement.as_mut() {
            if item.original_action.is_none() {
                item.original_action = Some(original_action);
            }
            if item.original_confidence.is_none() {
                item.original_confidence = Some(original_confidence);
            }
            item.adjusted_action = case.action.clone();
            item.adjusted_confidence = case.confidence;
            if !item
                .contradicting_sources
                .iter()
                .any(|source| source == "orphan_signal_cap")
            {
                item.contradicting_sources.push("orphan_signal_cap".into());
            }
            item.summary = format!("{}; orphan-signal cases are capped at review", item.summary);
        }
    }
}

pub fn apply_raw_disagreement_layer(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    tactical_cases: &mut [LiveTacticalCase],
    window: time::Duration,
) {
    for case in tactical_cases.iter_mut() {
        let Some(symbol) = (!case.symbol.is_empty()).then(|| Symbol(case.symbol.clone())) else {
            continue;
        };
        let Some(expected_direction) = infer_case_direction(case) else {
            continue;
        };
        let Some(mut disagreement) =
            assess_raw_disagreement(raw_events, store, &symbol, case, expected_direction, window)
        else {
            continue;
        };

        if disagreement.adjusted_action != case.action {
            disagreement.original_action = Some(case.action.clone());
            case.action = disagreement.adjusted_action.clone();
        }
        if disagreement.adjusted_confidence != case.confidence {
            disagreement.original_confidence = Some(case.confidence);
            case.confidence = disagreement.adjusted_confidence;
        }
        if disagreement.support_fraction < Decimal::new(67, 2) {
            case.review_reason_code = Some("insufficient_raw_support".into());
            case.policy_primary = Some("raw_support_capped".into());
            case.policy_reason = Some(format!(
                "weighted raw support below supermajority threshold (weighted_sf={}, count_sf={}, channels={}/{})",
                disagreement.support_fraction.round_dp(3),
                disagreement.count_support_fraction.round_dp(3),
                disagreement.support_count,
                disagreement.support_count + disagreement.contradict_count
            ));
        } else if disagreement.alignment == "conflicted" {
            case.review_reason_code = Some("raw_direction_conflict".into());
            case.policy_primary = Some("raw_direction_conflict".into());
            case.policy_reason = Some("raw sources contradict the case direction".into());
        }
        case.raw_disagreement = Some(disagreement);
    }
}

#[derive(Clone)]
struct DirectionalEvidence {
    source: &'static str,
    direction: i8,
    strength: Decimal,
}

impl DirectionalEvidence {
    fn attention_weight(&self) -> Decimal {
        attention_weight_for_channel(self.source)
    }

    fn weighted_strength(&self) -> Decimal {
        self.strength * self.attention_weight()
    }
}

fn assess_raw_disagreement(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    symbol: &Symbol,
    case: &LiveTacticalCase,
    expected_direction: i8,
    window: time::Duration,
) -> Option<LiveRawDisagreement> {
    let window = RawQueryWindow::LastDuration(window);
    let mut evidence = Vec::new();

    if let Some(item) = trade_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = depth_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = broker_evidence(raw_events, store, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = capital_distribution_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = capital_flow_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = calc_index_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = quote_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = candle_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }
    if let Some(item) = intraday_evidence(raw_events, symbol, window) {
        evidence.push(item);
    }

    if evidence.is_empty() {
        return None;
    }

    let mut supporting_sources = Vec::new();
    let mut contradicting_sources = Vec::new();
    let mut support_strength = Decimal::ZERO;
    let mut contradict_strength = Decimal::ZERO;

    for item in evidence {
        if item.direction == expected_direction {
            supporting_sources.push(item.source.to_string());
            support_strength += item.weighted_strength();
        } else if item.direction == -expected_direction {
            contradicting_sources.push(item.source.to_string());
            contradict_strength += item.weighted_strength();
        }
    }

    let support_count = supporting_sources.len();
    let contradict_count = contradicting_sources.len();
    let total_count = support_count + contradict_count;
    let count_support_fraction = if total_count == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(support_count as i64) / Decimal::from(total_count as i64)
    };
    let total_weight = support_strength + contradict_strength;
    let support_fraction = if total_weight <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        clamp_unit_interval(support_strength / total_weight)
    };

    let alignment = classify_raw_alignment(
        support_count,
        contradict_count,
        support_strength,
        contradict_strength,
    );

    let (adjusted_action, adjusted_confidence) = adjusted_case_surface(
        case.action.as_str(),
        case.confidence,
        alignment,
        support_fraction,
        contradict_strength,
    );
    let expected_direction_label = if expected_direction > 0 {
        "buy"
    } else {
        "sell"
    };
    let summary = if contradicting_sources.is_empty() && !supporting_sources.is_empty() {
        format!(
            "raw sources support the {expected_direction_label} case via {} (weighted_sf={}, count_sf={})",
            supporting_sources.join(", "),
            support_fraction.round_dp(3),
            count_support_fraction.round_dp(3),
        )
    } else if supporting_sources.is_empty() && !contradicting_sources.is_empty() {
        format!(
            "raw sources contradict the {expected_direction_label} case via {} (weighted_sf={}, count_sf={})",
            contradicting_sources.join(", "),
            support_fraction.round_dp(3),
            count_support_fraction.round_dp(3),
        )
    } else {
        format!(
            "raw sources are mixed for the {expected_direction_label} case (support: {}; contradict: {}; weighted_sf={}; count_sf={})",
            supporting_sources.join(", "),
            contradicting_sources.join(", "),
            support_fraction.round_dp(3),
            count_support_fraction.round_dp(3),
        )
    };

    Some(LiveRawDisagreement {
        alignment: alignment.to_string(),
        expected_direction: expected_direction_label.to_string(),
        support_count,
        contradict_count,
        count_support_fraction,
        support_fraction,
        support_weight: support_strength.round_dp(4),
        contradict_weight: contradict_strength.round_dp(4),
        adjusted_action,
        adjusted_confidence,
        summary,
        supporting_sources,
        contradicting_sources,
        original_action: None,
        original_confidence: None,
    })
}

fn classify_raw_alignment(
    support_count: usize,
    contradict_count: usize,
    support_strength: Decimal,
    contradict_strength: Decimal,
) -> &'static str {
    if support_count == 0 && contradict_count == 0 {
        "ambiguous"
    } else if contradict_strength >= support_strength && contradict_count > 0 {
        "conflicted"
    } else {
        "aligned"
    }
}

fn infer_case_direction(case: &LiveTacticalCase) -> Option<i8> {
    case.inferred_intent
        .as_ref()
        .and_then(|intent| action_direction_from_intent_direction(intent.direction))
        .or_else(|| infer_action_direction_from_texts(&[&case.title, &case.entry_rationale]))
        .map(action_direction_sign)
}

fn adjusted_case_surface(
    current_action: &str,
    current_confidence: Decimal,
    alignment: &str,
    support_fraction: Decimal,
    contradict_strength: Decimal,
) -> (String, Decimal) {
    let confidence_floor = Decimal::ZERO;
    let confidence_cap = Decimal::ONE;
    let penalty = match alignment {
        "conflicted" => Decimal::new(12, 2) + contradict_strength.min(Decimal::new(8, 2)),
        "ambiguous" => Decimal::new(5, 2),
        _ => Decimal::ZERO,
    };
    let adjusted_confidence = (current_confidence - penalty)
        .max(confidence_floor)
        .min(confidence_cap);
    let adjusted_action = match (current_action, alignment) {
        ("enter", _) if support_fraction < Decimal::new(67, 2) => "review",
        ("enter", "conflicted") => "review",
        ("review", "conflicted") => "observe",
        _ => current_action,
    };
    (adjusted_action.to_string(), adjusted_confidence)
}

fn attention_weight_for_channel(source: &str) -> Decimal {
    match source {
        "broker" => Decimal::ONE,
        "depth" => Decimal::new(90, 2),
        "trade" => Decimal::new(75, 2),
        "capital_flow" => Decimal::new(60, 2),
        "capital_distribution" => Decimal::new(55, 2),
        "quote" => Decimal::new(35, 2),
        "candle" => Decimal::new(25, 2),
        "intraday" => Decimal::new(20, 2),
        "calc_index" => Decimal::new(15, 2),
        _ => Decimal::new(30, 2),
    }
}

fn trade_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.trade_aggression(symbol, window);
    if report.trade_count == 0 {
        return None;
    }
    let edge = report.buy_volume_ratio - report.sell_volume_ratio;
    if edge == Decimal::ZERO {
        return None;
    }
    Some(DirectionalEvidence {
        source: "trade",
        direction: if edge > Decimal::ZERO { 1 } else { -1 },
        strength: clamp_unit_interval(edge.abs()),
    })
}

fn depth_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.depth_evolution(symbol, window);
    let delta = report.net_delta?;
    let bid_delta = signed_level_volume_delta(&delta.bid_changes);
    let ask_delta = signed_level_volume_delta(&delta.ask_changes);
    let net = bid_delta - ask_delta;
    let total = bid_delta.abs() + ask_delta.abs();
    if net == 0 || total == 0 {
        return None;
    }
    Some(DirectionalEvidence {
        source: "depth",
        direction: if net > 0 { 1 } else { -1 },
        strength: clamp_unit_interval(Decimal::from(net.abs()) / Decimal::from(total)),
    })
}

fn broker_evidence(
    raw_events: &RawEventStore,
    store: &ObjectStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.broker_onset(symbol, window, store);
    if report.events.is_empty() {
        return None;
    }
    let bid_count = report
        .events
        .iter()
        .filter(|event| matches!(event.side, crate::ontology::links::Side::Bid))
        .count() as i64;
    let ask_count = report.events.len() as i64 - bid_count;
    let diff = bid_count - ask_count;
    if diff == 0 {
        return None;
    }
    Some(DirectionalEvidence {
        source: "broker",
        direction: if diff > 0 { 1 } else { -1 },
        strength: clamp_unit_interval(
            Decimal::from(diff.abs()) / Decimal::from(report.events.len() as i64),
        ),
    })
}

fn capital_distribution_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.capital_distribution_shift(symbol, window);
    if report.observation_count == 0 {
        return None;
    }
    let dominant_delta = match report.dominant_bucket.as_deref() {
        Some("large") => report.delta_large_net,
        Some("medium") => report.delta_medium_net,
        Some("small") => report.delta_small_net,
        _ => Decimal::ZERO,
    };
    if dominant_delta == Decimal::ZERO {
        return None;
    }
    let total =
        report.delta_large_net.abs() + report.delta_medium_net.abs() + report.delta_small_net.abs();
    if total == Decimal::ZERO {
        return None;
    }
    Some(DirectionalEvidence {
        source: "capital_distribution",
        direction: if dominant_delta > Decimal::ZERO {
            1
        } else {
            -1
        },
        strength: clamp_unit_interval(dominant_delta.abs() / total),
    })
}

fn capital_flow_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.capital_flow_shift(symbol, window);
    if report.observation_count == 0 {
        return None;
    }
    let signal = report
        .velocity
        .filter(|value| *value != Decimal::ZERO)
        .or(report.delta_inflow.filter(|value| *value != Decimal::ZERO))
        .or(report.latest_inflow.filter(|value| *value != Decimal::ZERO))?;
    Some(DirectionalEvidence {
        source: "capital_flow",
        direction: if signal > Decimal::ZERO { 1 } else { -1 },
        strength: Decimal::new(4, 1),
    })
}

fn calc_index_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.calc_index_state(symbol, window);
    if report.observation_count == 0 {
        return None;
    }
    let signal = report
        .five_minutes_change_rate
        .filter(|value| *value != Decimal::ZERO)
        .or(report.change_rate.filter(|value| *value != Decimal::ZERO))?;
    Some(DirectionalEvidence {
        source: "calc_index",
        direction: if signal > Decimal::ZERO { 1 } else { -1 },
        strength: clamp_unit_interval(signal.abs() * Decimal::new(8, 0)),
    })
}

fn quote_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.quote_state(symbol, window);
    let last = report.last_done?;
    let prev = report.prev_close?;
    if prev == Decimal::ZERO {
        return None;
    }
    let delta = (last - prev) / prev;
    if delta == Decimal::ZERO {
        return None;
    }
    Some(DirectionalEvidence {
        source: "quote",
        direction: if delta > Decimal::ZERO { 1 } else { -1 },
        strength: clamp_unit_interval(delta.abs() * Decimal::new(20, 0)),
    })
}

fn candle_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.candlestick_state(symbol, window);
    let delta = report.net_change?;
    if delta == Decimal::ZERO {
        return None;
    }
    let range = report.range.unwrap_or(delta.abs()).max(Decimal::new(1, 6));
    Some(DirectionalEvidence {
        source: "candlestick",
        direction: if delta > Decimal::ZERO { 1 } else { -1 },
        strength: clamp_unit_interval(delta.abs() / range),
    })
}

fn intraday_evidence(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: RawQueryWindow,
) -> Option<DirectionalEvidence> {
    let report = raw_events.intraday_profile(symbol, window);
    let deviation = report.vwap_deviation?;
    if deviation == Decimal::ZERO {
        return None;
    }
    Some(DirectionalEvidence {
        source: "intraday",
        direction: if deviation > Decimal::ZERO { 1 } else { -1 },
        strength: clamp_unit_interval(deviation.abs() * Decimal::new(20, 0)),
    })
}

fn signed_level_volume_delta(changes: &[LevelChange]) -> i64 {
    changes
        .iter()
        .map(|change| match change {
            LevelChange::Added { volume, .. } => *volume,
            LevelChange::Removed { prev_volume, .. } => -*prev_volume,
            LevelChange::VolumeChanged {
                prev_volume,
                new_volume,
                ..
            } => *new_volume - *prev_volume,
        })
        .sum()
}

fn clamp_unit_interval(value: Decimal) -> Decimal {
    value.max(Decimal::ZERO).min(Decimal::ONE)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveMarketRegime {
    pub bias: String,
    pub confidence: Decimal,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub average_return: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directional_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_market_sentiment: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveStressSnapshot {
    pub composite_stress: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector_synchrony: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub momentum_consensus: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_dispersion: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_anomaly: Option<Decimal>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LiveScorecard {
    pub total_signals: usize,
    pub resolved_signals: usize,
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    /// Resolved signals that were flagged `requires_confirmation=false` at
    /// emission time (the tradable subset). `total_*` above is noise floor.
    #[serde(default)]
    pub actionable_resolved: usize,
    #[serde(default)]
    pub actionable_hits: usize,
    #[serde(default)]
    pub actionable_hit_rate: Decimal,
    #[serde(default)]
    pub actionable_mean_return: Decimal,
    /// `actionable_hit_rate - hit_rate`. Regime-independent selectivity edge;
    /// use this as the headline metric instead of raw AHR.
    #[serde(default)]
    pub actionable_excess_hit_rate: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveTacticalCase {
    pub setup_id: String,
    #[serde(default)]
    pub symbol: String,
    pub title: String,
    pub action: String,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub heuristic_edge: Decimal,
    pub entry_rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causal_narrative: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_family: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reason_subreasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_primary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_horizon_gate_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_success_pattern_signature: Option<String>,
    /// Pressure field reasoning: lifecycle phase (Growing/Peaking/Fading/New).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_phase: Option<String>,
    /// Pressure field reasoning: tension driver (trade_flow/capital_flow/institutional/etc).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tension_driver: Option<String>,
    /// Pressure reasoning: refined driver class after peer/competition checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_class: Option<String>,
    /// Pressure field reasoning: isolated from sector peers?
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_isolated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_active_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_silent_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_confirmation_ratio: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation_score: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub competition_margin: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_confidence: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absence_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub competition_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub competition_winner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub competition_runner_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_velocity: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_acceleration: Option<Decimal>,
    /// Primary horizon bucket (fast5m/mid30m/session/multi_session).
    /// Written from `CaseHorizon.primary` in Wave 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_bucket: Option<String>,
    /// Action urgency (immediate/normal/relaxed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_urgency: Option<String>,
    /// Secondary horizon buckets (context only, no ranking impact).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub horizon_secondary: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archetype_projections: Vec<ArchetypeProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_violations: Vec<ExpectationViolation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inferred_intent: Option<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_state: Option<String>,
    /// Tick on which this setup_id first emitted `action=enter`. Derived from
    /// `recent_transitions` scanning; `None` when no prior enter transition is
    /// found (either a fresh enter on the current tick or transitions buffer
    /// does not cover the history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_enter_tick: Option<u64>,
    /// `current_tick - first_enter_tick`, populated alongside `first_enter_tick`.
    /// Used by the freshness-decay guardrail to downgrade stale enters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_since_first_enter: Option<u64>,
    /// `current_tick - first surfaced tick` for this setup on the live surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticks_since_first_seen: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_position_in_range: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_state_confidence: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_velocity_5t: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_fraction_velocity_5t: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority_rank: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_persistence_ticks: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_stability_rounds: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_disagreement: Option<LiveRawDisagreement>,
}

#[derive(Debug, Clone)]
pub struct SurfacedCaseHistorySample {
    pub tick: u64,
    pub setup_id: String,
    pub symbol: String,
    pub confidence: Decimal,
    pub support_fraction: Option<Decimal>,
}

#[derive(Debug, Clone)]
struct SurfacedCaseMomentumObservation {
    tick: u64,
    confidence: Decimal,
    support_fraction: Option<Decimal>,
}

static SURFACED_CASE_MOMENTUM: OnceLock<
    Mutex<HashMap<String, Vec<SurfacedCaseMomentumObservation>>>,
> = OnceLock::new();

pub fn live_case_freshness_state(risk_notes: &[String]) -> Option<String> {
    if risk_notes.iter().any(|note| note == "carried_forward=true") {
        Some("carried_forward".into())
    } else {
        Some("fresh".into())
    }
}

pub fn consolidated_review_reason_family(code: &str) -> &'static str {
    match code {
        "insufficient_raw_support"
        | "raw_persistence_insufficient"
        | "stale_symbol_confirmation" => "raw_support_gate",
        "freshness_decay_aging" | "freshness_decay_expired" => "freshness_gate",
        "raw_direction_conflict" | "directional_conflict" => "directional_conflict_gate",
        "late_signal_timing" => "timing_gate",
        "signal_translation_gap" => "translation_gap",
        "orphan_signal_cap" => "orphan_signal_gate",
        _ => "other_gate",
    }
}

pub fn consolidated_review_reason_subreasons(
    code: &str,
    timing_state: Option<&str>,
) -> Vec<String> {
    match code {
        "insufficient_raw_support" => vec!["supermajority".into()],
        "raw_persistence_insufficient" => vec!["persistence".into(), "multi_tick".into()],
        "stale_symbol_confirmation" => vec!["multi_tick".into()],
        "freshness_decay_aging" => vec!["aging".into()],
        "freshness_decay_expired" => vec!["expired".into()],
        "raw_direction_conflict" => vec!["raw_sources".into()],
        "directional_conflict" => vec!["roster_conflict".into()],
        "late_signal_timing" => vec![timing_state.unwrap_or("late_signal").to_string()],
        "signal_translation_gap" => vec!["missing_case_translation".into()],
        "orphan_signal_cap" => vec!["orphan_signal".into()],
        other => vec![other.to_string()],
    }
}

pub fn apply_review_reason_consolidation(tactical_cases: &mut [LiveTacticalCase]) {
    for case in tactical_cases.iter_mut() {
        let Some(code) = case.review_reason_code.as_deref() else {
            case.review_reason_family = None;
            case.review_reason_subreasons.clear();
            continue;
        };
        case.review_reason_family = Some(consolidated_review_reason_family(code).to_string());
        case.review_reason_subreasons =
            consolidated_review_reason_subreasons(code, case.timing_state.as_deref());
    }
}

pub fn note_value<'a>(notes: &'a [String], prefix: &str) -> Option<&'a str> {
    notes.iter().find_map(|note| note.strip_prefix(prefix))
}

pub fn note_decimal(notes: &[String], prefix: &str) -> Option<Decimal> {
    note_value(notes, prefix)?.parse::<Decimal>().ok()
}

pub fn note_usize(notes: &[String], prefix: &str) -> Option<usize> {
    note_value(notes, prefix)?.parse::<usize>().ok()
}

pub fn note_bool(notes: &[String], prefix: &str) -> Option<bool> {
    note_value(notes, prefix)?.parse::<bool>().ok()
}

pub fn apply_case_structural_notes(case: &mut LiveTacticalCase, risk_notes: &[String]) {
    if case.lifecycle_phase.is_none() {
        case.lifecycle_phase = note_value(risk_notes, "phase=").map(|value| value.to_string());
    }
    if case.tension_driver.is_none() {
        case.tension_driver = note_value(risk_notes, "driver=").map(|value| value.to_string());
    }
    if case.driver_class.is_none() {
        case.driver_class = note_value(risk_notes, "driver_class=").map(|value| value.to_string());
    }
    if case.is_isolated.is_none() {
        case.is_isolated = note_bool(risk_notes, "is_isolated=").or_else(|| {
            note_decimal(risk_notes, "isolation_score=").map(|value| value >= Decimal::new(8, 1))
        });
    }
    if case.peer_active_count.is_none() {
        case.peer_active_count = note_usize(risk_notes, "peer_active_count=");
    }
    if case.peer_silent_count.is_none() {
        case.peer_silent_count = note_usize(risk_notes, "peer_silent_count=");
    }
    if case.peer_confirmation_ratio.is_none() {
        case.peer_confirmation_ratio = note_decimal(risk_notes, "peer_confirmation_ratio=");
    }
    if case.isolation_score.is_none() {
        case.isolation_score = note_decimal(risk_notes, "isolation_score=");
    }
    if case.competition_margin.is_none() {
        case.competition_margin = note_decimal(risk_notes, "competition_margin=");
    }
    if case.driver_confidence.is_none() {
        case.driver_confidence = note_decimal(risk_notes, "driver_confidence=");
    }
    if case.absence_summary.is_none() {
        case.absence_summary =
            note_value(risk_notes, "absence_summary=").map(|value| value.to_string());
    }
    if case.competition_summary.is_none() {
        case.competition_summary =
            note_value(risk_notes, "competition_summary=").map(|value| value.to_string());
    }
    if case.competition_winner.is_none() {
        case.competition_winner =
            note_value(risk_notes, "competition_winner=").map(|value| value.to_string());
    }
    if case.competition_runner_up.is_none() {
        case.competition_runner_up =
            note_value(risk_notes, "competition_runner_up=").map(|value| value.to_string());
    }
    if case.lifecycle_velocity.is_none() {
        case.lifecycle_velocity = note_decimal(risk_notes, "velocity=");
    }
    if case.lifecycle_acceleration.is_none() {
        case.lifecycle_acceleration = note_decimal(risk_notes, "acceleration=");
    }
}

pub fn enrich_surfaced_case_evidence(
    tactical_cases: &mut [LiveTacticalCase],
    sector_by_symbol: &HashMap<String, String>,
    history_samples: &[SurfacedCaseHistorySample],
) {
    let eligible_indexes = tactical_cases
        .iter()
        .enumerate()
        .filter_map(|(index, case)| (case.confidence >= Decimal::new(7, 1)).then_some(index))
        .collect::<Vec<_>>();

    for &index in &eligible_indexes {
        let structural_present = tactical_cases[index].peer_confirmation_ratio.is_some()
            || tactical_cases[index].lifecycle_velocity.is_some()
            || tactical_cases[index].lifecycle_acceleration.is_some();
        if structural_present {
            continue;
        }

        tactical_cases[index].peer_confirmation_ratio =
            fallback_peer_confirmation_ratio(index, tactical_cases, sector_by_symbol);
        let (velocity, acceleration) =
            fallback_lifecycle_derivatives(&tactical_cases[index], history_samples);
        tactical_cases[index].lifecycle_velocity = velocity;
        tactical_cases[index].lifecycle_acceleration = acceleration;
    }

    apply_priority_ranking(tactical_cases);
}

pub fn enrich_cross_tick_momentum(
    market: LiveMarket,
    tick: u64,
    tactical_cases: &mut [LiveTacticalCase],
) {
    let store = SURFACED_CASE_MOMENTUM.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = store.lock().expect("surfaced case momentum lock");
    let prune_before = tick.saturating_sub(128);
    guard.retain(|_, samples| {
        samples.retain(|sample| sample.tick >= prune_before);
        !samples.is_empty()
    });

    for case in tactical_cases.iter_mut() {
        if case.symbol.is_empty() {
            case.ticks_since_first_seen = None;
            case.confidence_velocity_5t = None;
            case.support_fraction_velocity_5t = None;
            continue;
        }

        let key = surfaced_case_history_key(market, &case.setup_id, &case.symbol);
        let history = guard.entry(key).or_default();
        case.ticks_since_first_seen = Some(
            history
                .first()
                .map(|sample| tick.saturating_sub(sample.tick))
                .unwrap_or(0),
        );

        let lookback_tick = tick.saturating_sub(5);
        let lookback = history
            .iter()
            .rev()
            .find(|sample| sample.tick <= lookback_tick);
        case.confidence_velocity_5t = lookback.map(|sample| case.confidence - sample.confidence);
        case.support_fraction_velocity_5t = lookback.and_then(|sample| {
            let current = case
                .raw_disagreement
                .as_ref()
                .map(|item| item.support_fraction)?;
            let previous = sample.support_fraction?;
            Some(current - previous)
        });

        history.push(SurfacedCaseMomentumObservation {
            tick,
            confidence: case.confidence,
            support_fraction: case
                .raw_disagreement
                .as_ref()
                .map(|item| item.support_fraction),
        });
        if history.len() > 64 {
            let drop_count = history.len().saturating_sub(64);
            history.drain(0..drop_count);
        }
    }
}

fn surfaced_case_history_key(market: LiveMarket, setup_id: &str, symbol: &str) -> String {
    let market_slug = match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    };
    if !setup_id.is_empty() {
        format!("{market_slug}:{}", setup_id.to_ascii_lowercase())
    } else {
        format!("{market_slug}:symbol:{}", symbol.to_ascii_lowercase())
    }
}

fn fallback_peer_confirmation_ratio(
    index: usize,
    tactical_cases: &[LiveTacticalCase],
    sector_by_symbol: &HashMap<String, String>,
) -> Option<Decimal> {
    let case = tactical_cases.get(index)?;
    if case.confidence < Decimal::new(7, 1) {
        return None;
    }
    let direction = infer_case_direction(case)?;
    let cohort = if let Some(sector) = sector_by_symbol.get(&case.symbol) {
        CohortKey::Sector(sector.clone())
    } else if let Some(driver_class) = case.driver_class.as_ref().filter(|value| !value.is_empty())
    {
        CohortKey::DriverClass(driver_class.clone())
    } else if let Some(family_label) = case.family_label.as_ref().filter(|value| !value.is_empty())
    {
        CohortKey::FamilyLabel(family_label.clone())
    } else {
        return None;
    };

    let mut same_direction = 0usize;
    let mut eligible_peers = 0usize;
    for (peer_index, peer) in tactical_cases.iter().enumerate() {
        if peer_index == index || peer.confidence < Decimal::new(7, 1) {
            continue;
        }
        if !matches_cohort(&cohort, peer, sector_by_symbol) {
            continue;
        }
        let Some(peer_direction) = infer_case_direction(peer) else {
            eligible_peers += 1;
            continue;
        };
        if peer_direction == -direction {
            continue;
        }
        eligible_peers += 1;
        if peer_direction == direction {
            same_direction += 1;
        }
    }
    if eligible_peers == 0 {
        None
    } else {
        Some(Decimal::from(same_direction as i64) / Decimal::from(eligible_peers as i64))
    }
}

fn fallback_lifecycle_derivatives(
    case: &LiveTacticalCase,
    history_samples: &[SurfacedCaseHistorySample],
) -> (Option<Decimal>, Option<Decimal>) {
    if case.confidence < Decimal::new(7, 1) {
        return (None, None);
    }
    let mut matched = history_samples
        .iter()
        .filter(|sample| sample.setup_id == case.setup_id)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        matched = history_samples
            .iter()
            .filter(|sample| sample.symbol == case.symbol)
            .collect::<Vec<_>>();
    }
    matched.sort_by(|left, right| left.tick.cmp(&right.tick));

    let Some(previous) = matched.last() else {
        return (None, None);
    };
    let velocity = case.confidence - previous.confidence;
    let acceleration = matched
        .iter()
        .rev()
        .nth(1)
        .map(|previous_previous| velocity - (previous.confidence - previous_previous.confidence))
        .unwrap_or(Decimal::ZERO);
    (Some(velocity), Some(acceleration))
}

pub fn apply_priority_ranking(tactical_cases: &mut [LiveTacticalCase]) {
    let mut scored = tactical_cases
        .iter()
        .enumerate()
        .filter_map(|(index, case)| {
            if case.confidence < Decimal::new(7, 1) {
                return None;
            }
            Some((index, priority_capped(case), priority_score(case)))
        })
        .collect::<Vec<_>>();

    scored.sort_by(
        |(left_index, left_capped, left_score), (right_index, right_capped, right_score)| {
            left_capped
                .cmp(right_capped)
                .then_with(|| right_score.cmp(left_score))
                .then_with(|| {
                    action_surface_priority(tactical_cases[*left_index].action.as_str()).cmp(
                        &action_surface_priority(tactical_cases[*right_index].action.as_str()),
                    )
                })
                .then_with(|| {
                    tactical_cases[*right_index]
                        .heuristic_edge
                        .cmp(&tactical_cases[*left_index].heuristic_edge)
                })
                .then_with(|| {
                    tactical_cases[*right_index]
                        .confidence_gap
                        .cmp(&tactical_cases[*left_index].confidence_gap)
                })
                .then_with(|| {
                    tactical_cases[*right_index]
                        .confidence
                        .cmp(&tactical_cases[*left_index].confidence)
                })
                .then_with(|| {
                    tactical_cases[*left_index]
                        .setup_id
                        .cmp(&tactical_cases[*right_index].setup_id)
                })
        },
    );

    for case in tactical_cases.iter_mut() {
        case.priority_rank = None;
    }
    for (rank, (index, _, _)) in scored.into_iter().enumerate() {
        tactical_cases[index].priority_rank = Some((rank + 1) as u16);
    }
}

pub fn sort_tactical_cases_for_surface(tactical_cases: &mut [LiveTacticalCase]) {
    tactical_cases.sort_by(|a, b| {
        priority_rank_sort_key(a.priority_rank)
            .cmp(&priority_rank_sort_key(b.priority_rank))
            .then_with(|| {
                action_surface_priority(a.action.as_str())
                    .cmp(&action_surface_priority(b.action.as_str()))
            })
            .then_with(|| b.heuristic_edge.cmp(&a.heuristic_edge))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.setup_id.cmp(&b.setup_id))
    });
}

fn priority_rank_sort_key(rank: Option<u16>) -> (u8, u16) {
    match rank {
        Some(value) => (0, value),
        None => (1, u16::MAX),
    }
}

fn priority_capped(case: &LiveTacticalCase) -> bool {
    case.actionability_state.as_deref() == Some("do_not_trade")
}

fn priority_score(case: &LiveTacticalCase) -> Decimal {
    let mut score = case.actionability_score.unwrap_or(Decimal::ZERO);

    if case
        .raw_disagreement
        .as_ref()
        .map(|item| item.support_fraction >= Decimal::new(75, 2))
        .unwrap_or(false)
    {
        score += Decimal::new(15, 2);
    }
    if case
        .peer_confirmation_ratio
        .map(|value| value >= Decimal::new(90, 2))
        .unwrap_or(false)
    {
        score += Decimal::new(15, 2);
    }

    let velocity = case.lifecycle_velocity.unwrap_or(Decimal::ZERO);
    let acceleration = case.lifecycle_acceleration.unwrap_or(Decimal::ZERO);
    if velocity > Decimal::ZERO {
        score += Decimal::new(5, 2);
    }
    if acceleration > velocity && acceleration > Decimal::ZERO && velocity > Decimal::ZERO {
        score += Decimal::new(10, 2);
    }
    if velocity >= acceleration && velocity > Decimal::ZERO && acceleration > Decimal::ZERO {
        score -= Decimal::new(15, 2);
    }

    match infer_case_direction(case) {
        Some(1) => {
            if case
                .timing_position_in_range
                .map(|value| value >= Decimal::new(20, 2) && value <= Decimal::new(70, 2))
                .unwrap_or(false)
            {
                score += Decimal::new(10, 2);
            }
        }
        Some(-1) => {
            if case
                .timing_position_in_range
                .map(|value| value >= Decimal::new(30, 2) && value <= Decimal::new(80, 2))
                .unwrap_or(false)
            {
                score += Decimal::new(10, 2);
            }
        }
        _ => {}
    }

    if case.freshness_state.as_deref() == Some("fresh") {
        score += Decimal::new(5, 2);
    }
    if matches!(
        case.timing_state.as_deref(),
        Some("late_chase") | Some("range_extreme")
    ) {
        score -= Decimal::new(15, 2);
    }
    if matches!(
        case.freshness_state.as_deref(),
        Some("carried_forward") | Some("aging") | Some("stale") | Some("expired")
    ) {
        score -= Decimal::new(20, 2);
    }

    score
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CohortKey {
    Sector(String),
    DriverClass(String),
    FamilyLabel(String),
}

fn matches_cohort(
    cohort: &CohortKey,
    case: &LiveTacticalCase,
    sector_by_symbol: &HashMap<String, String>,
) -> bool {
    match cohort {
        CohortKey::Sector(sector) => sector_by_symbol.get(&case.symbol) == Some(sector),
        CohortKey::DriverClass(driver_class) => case.driver_class.as_ref() == Some(driver_class),
        CohortKey::FamilyLabel(family_label) => case.family_label.as_ref() == Some(family_label),
    }
}

pub fn live_case_timing_state(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    case: &LiveTacticalCase,
    window: time::Duration,
) -> Option<String> {
    let report = raw_events.quote_state(symbol, RawQueryWindow::LastDuration(window));
    let last = report.last_done?;
    let high = report.high?;
    let low = report.low?;
    let range = high - low;
    if range <= Decimal::ZERO {
        return Some("range_unknown".into());
    }

    let position = (last - low) / range;
    let short_case = infer_case_direction(case) == Some(-1);
    let long_case = infer_case_direction(case) == Some(1);

    let timing = if long_case && position >= Decimal::new(70, 2) {
        "late_chase"
    } else if short_case && position <= Decimal::new(30, 2) {
        "late_chase"
    } else if position <= Decimal::new(20, 2) || position >= Decimal::new(80, 2) {
        "range_extreme"
    } else {
        "timely"
    };
    Some(timing.into())
}

pub fn live_case_position_in_range(
    raw_events: &RawEventStore,
    symbol: &Symbol,
    window: time::Duration,
) -> Option<Decimal> {
    let report = raw_events.quote_state(symbol, RawQueryWindow::LastDuration(window));
    let last = report.last_done?;
    let high = report.high?;
    let low = report.low?;
    let range = high - low;
    if range <= Decimal::ZERO {
        return None;
    }
    Some(((last - low) / range).max(Decimal::ZERO).min(Decimal::ONE))
}

pub fn enforce_timing_action_cap(tactical_cases: &mut [LiveTacticalCase]) {
    for case in tactical_cases.iter_mut() {
        let Some(timing_state) = case.timing_state.as_deref() else {
            continue;
        };
        if case.action != "enter" {
            continue;
        }
        if !matches!(timing_state, "late_chase" | "range_extreme") {
            continue;
        }

        let original_action = case.action.clone();
        let original_confidence = case.confidence;
        case.action = "review".into();
        case.confidence = case.confidence.min(Decimal::new(70, 2));
        case.review_reason_code = Some("late_signal_timing".into());
        case.policy_primary = Some("late_signal_capped".into());
        case.policy_reason = Some(match case.timing_position_in_range {
            Some(position) => format!(
                "signal fired near an unfavorable intraday range extreme (position_in_range={}) and was capped to review",
                position.round_dp(3)
            ),
            None => {
                "signal fired near an unfavorable intraday range extreme and was capped to review"
                    .into()
            }
        });

        if let Some(item) = case.raw_disagreement.as_mut() {
            if item.original_action.is_none() {
                item.original_action = Some(original_action);
            }
            if item.original_confidence.is_none() {
                item.original_confidence = Some(original_confidence);
            }
            item.adjusted_action = case.action.clone();
            item.adjusted_confidence = case.confidence;
            if !item
                .contradicting_sources
                .iter()
                .any(|source| source == "timing_guardrail")
            {
                item.contradicting_sources.push("timing_guardrail".into());
            }
            item.summary = format!(
                "{}; timing guardrail capped a late/range-extreme enter",
                item.summary
            );
        }
    }
}

/// Scan `recent_transitions` for the earliest tick where `setup_id` emitted a
/// state containing `"enter"`. Returns None when no prior enter transition is
/// present (either fresh enter this tick, or the transitions buffer does not
/// span the case's history).
pub fn compute_first_enter_tick(
    setup_id: &str,
    transitions: &[crate::agent::AgentTransition],
) -> Option<u64> {
    transitions
        .iter()
        .filter(|t| {
            t.setup_id
                .as_deref()
                .map(|s| s == setup_id)
                .unwrap_or(false)
                && t.to_state.contains("enter")
        })
        .map(|t| t.from_tick)
        .min()
}

/// Freshness-decay cap: once a case has been `action=enter` for too many ticks
/// the entry window closes — price has likely moved, the "catch it on first
/// tick" window has passed, and following Eden's signal here means chasing.
///
/// Thresholds (ticks since first_enter_tick):
/// -  0..=2  → `fresh`,  no action change
/// -  3..=5  → `aging`,  no action change but flag visible
/// -  6..=10 → `stale`,  enter → review with `review_reason_code=freshness_decay_aging`
/// - 11..    → `expired`, enter → review with `review_reason_code=freshness_decay_expired`
///
/// Called after `enforce_timing_action_cap` but before the final sort, so
/// downgraded cases drop to review-tier priority naturally.
pub fn enforce_freshness_decay(
    tactical_cases: &mut [LiveTacticalCase],
    current_tick: u64,
    transitions: &[crate::agent::AgentTransition],
) {
    for case in tactical_cases.iter_mut() {
        let first_enter = compute_first_enter_tick(&case.setup_id, transitions);
        case.first_enter_tick = first_enter;
        let age = first_enter.map(|t| current_tick.saturating_sub(t));
        case.ticks_since_first_enter = age;

        let Some(age_ticks) = age else {
            continue;
        };
        // Only cap action when the case is still claiming enter. Prior
        // adjustments may have already demoted it.
        if case.action != "enter" {
            if age_ticks <= 2 {
                case.freshness_state = Some("fresh".into());
            } else if age_ticks <= 5 {
                case.freshness_state = Some("aging".into());
            } else if age_ticks <= 10 {
                case.freshness_state = Some("stale".into());
            } else {
                case.freshness_state = Some("expired".into());
            }
            continue;
        }

        let (label, reason_code, reason_text) = if age_ticks <= 2 {
            ("fresh", None, None)
        } else if age_ticks <= 5 {
            ("aging", None, None)
        } else if age_ticks <= 10 {
            (
                "stale",
                Some("freshness_decay_aging"),
                Some("entry window aging: signal first promoted to enter 6-10 ticks ago"),
            )
        } else {
            (
                "expired",
                Some("freshness_decay_expired"),
                Some(
                    "entry window closed: signal has been at enter > 10 ticks, move is likely done",
                ),
            )
        };
        case.freshness_state = Some(label.into());

        let Some(code) = reason_code else {
            continue;
        };
        let original_action = case.action.clone();
        let original_confidence = case.confidence;
        case.action = "review".into();
        case.review_reason_code = Some(code.into());
        case.policy_primary = Some("freshness_decay".into());
        case.policy_reason = Some(
            reason_text
                .map(String::from)
                .unwrap_or_else(|| "entry window decayed".into()),
        );
        if !case
            .state_reason_codes
            .iter()
            .any(|c| c == "freshness_decay")
        {
            case.state_reason_codes.push("freshness_decay".into());
        }
        if let Some(item) = case.raw_disagreement.as_mut() {
            if item.original_action.is_none() {
                item.original_action = Some(original_action);
            }
            if item.original_confidence.is_none() {
                item.original_confidence = Some(original_confidence);
            }
            item.adjusted_action = case.action.clone();
            item.adjusted_confidence = case.confidence;
            if !item
                .contradicting_sources
                .iter()
                .any(|source| source == "freshness_guardrail")
            {
                item.contradicting_sources
                    .push("freshness_guardrail".into());
            }
            item.summary = format!(
                "{}; freshness guardrail capped an enter that had been live {} ticks",
                item.summary, age_ticks
            );
        }
    }
}

/// Mark cases as `directional_conflict` when the same symbol has both Long and
/// Short cases in the roster simultaneously. These are rare by construction
/// (different horizons or noise in the reasoning layer) but are operator-
/// hazardous: trading against a conflicted setup means you're directly fighting
/// another Eden conclusion. We keep the cases visible (so operators can still
/// see the conflict) but force them to `review` + `do_not_trade`.
pub fn mark_directional_conflicts(tactical_cases: &mut [LiveTacticalCase]) {
    // Collect (symbol → set of directions) and also (symbol → Vec<index>) in one pass.
    let mut direction_sets: HashMap<String, HashSet<i8>> = HashMap::new();
    for case in tactical_cases.iter() {
        if let Some(dir) = infer_case_direction(case) {
            direction_sets
                .entry(case.symbol.clone())
                .or_default()
                .insert(dir);
        }
    }
    // A symbol is conflicted if both +1 (buy) and -1 (sell) directions appear.
    let conflicted_symbols: HashSet<String> = direction_sets
        .into_iter()
        .filter_map(|(symbol, dirs)| {
            if dirs.contains(&1) && dirs.contains(&-1) {
                Some(symbol)
            } else {
                None
            }
        })
        .collect();
    if conflicted_symbols.is_empty() {
        return;
    }
    for case in tactical_cases.iter_mut() {
        if !conflicted_symbols.contains(&case.symbol) {
            continue;
        }
        let original_action = case.action.clone();
        let original_confidence = case.confidence;
        if case.action == "enter" {
            case.action = "review".into();
        }
        case.actionability_state = Some("do_not_trade".into());
        case.review_reason_code = Some("directional_conflict".into());
        case.policy_primary = Some("directional_conflict".into());
        case.policy_reason = Some(format!(
            "{} has both long and short cases active in the same roster; forced to do_not_trade",
            case.symbol
        ));
        if !case
            .state_reason_codes
            .iter()
            .any(|c| c == "directional_conflict")
        {
            case.state_reason_codes.push("directional_conflict".into());
        }
        if let Some(item) = case.raw_disagreement.as_mut() {
            if item.original_action.is_none() {
                item.original_action = Some(original_action);
            }
            if item.original_confidence.is_none() {
                item.original_confidence = Some(original_confidence);
            }
            item.adjusted_action = case.action.clone();
            item.adjusted_confidence = case.confidence;
            if !item
                .contradicting_sources
                .iter()
                .any(|source| source == "directional_conflict")
            {
                item.contradicting_sources
                    .push("directional_conflict".into());
            }
            item.summary = format!(
                "{}; another Eden case points the opposite direction on the same symbol",
                item.summary
            );
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveHypothesisTrack {
    pub symbol: String,
    pub title: String,
    pub status: String,
    pub age_ticks: u64,
    pub confidence: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSignal {
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub composite: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_price: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_composite: Option<Decimal>,
    #[serde(default)]
    pub capital_flow_direction: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub volume_profile: Decimal,
    #[serde(default)]
    pub pre_post_market_anomaly: Decimal,
    #[serde(default)]
    pub valuation: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_stock_correlation: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector_coherence: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cross_market_propagation: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveStructuralDelta {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub composite_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub capital_flow_delta: Decimal,
    pub flow_persistence: u64,
    pub flow_reversal: bool,
    pub pre_market_trend: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LivePropagationSense {
    pub source_symbol: String,
    pub target_symbol: String,
    pub channel: String,
    pub propagation_strength: Decimal,
    pub target_momentum: Decimal,
    pub lag_gap: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveTemporalBar {
    pub horizon: String,
    pub symbol: String,
    pub bucket_started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<Decimal>,
    pub composite_open: Decimal,
    pub composite_high: Decimal,
    pub composite_low: Decimal,
    pub composite_close: Decimal,
    pub composite_mean: Decimal,
    pub capital_flow_sum: Decimal,
    pub capital_flow_delta: Decimal,
    pub volume_total: i64,
    pub event_count: usize,
    pub signal_persistence: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LivePressure {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default)]
    pub capital_flow_pressure: Decimal,
    pub momentum: Decimal,
    pub pressure_delta: Decimal,
    pub pressure_duration: u64,
    pub accelerating: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveBackwardChain {
    pub symbol: String,
    pub conclusion: String,
    pub primary_driver: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<LiveEvidence>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveEvidence {
    pub source: String,
    pub description: String,
    pub weight: Decimal,
    pub direction: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCausalLeader {
    pub symbol: String,
    pub current_leader: String,
    pub leader_streak: u64,
    pub flips: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveEvent {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub magnitude: Decimal,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCrossMarketSignal {
    pub us_symbol: String,
    pub hk_symbol: String,
    pub propagation_confidence: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_since_hk_close_minutes: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveCrossMarketAnomaly {
    pub us_symbol: String,
    pub hk_symbol: String,
    pub expected_direction: Decimal,
    pub actual_direction: Decimal,
    pub divergence: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveLineageMetric {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon: Option<String>,
    pub template: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LiveSuccessPattern {
    pub family: String,
    pub signature: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dominant_channels: Vec<String>,
    pub samples: usize,
    pub mean_net_return: Decimal,
    pub mean_strength: Decimal,
    pub mean_coherence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_channel_diversity: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// Accepts either a full `LiveMarketRegime` object (HK format)
/// or a plain string like `"neutral"` (US format).
fn deserialize_market_regime<'de, D>(deserializer: D) -> Result<LiveMarketRegime, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        Full(LiveMarketRegime),
        Short(String),
    }
    match Helper::deserialize(deserializer)? {
        Helper::Full(r) => Ok(r),
        Helper::Short(bias) => Ok(LiveMarketRegime {
            bias,
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        }),
    }
}

/// Accepts either a `Vec<LiveLineageMetric>` or `{"by_template": [...]}`.
fn deserialize_lineage<'de, D>(deserializer: D) -> Result<Vec<LiveLineageMetric>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        Vec(Vec<LiveLineageMetric>),
        Map { by_template: Vec<LiveLineageMetric> },
    }
    match Helper::deserialize(deserializer)? {
        Helper::Vec(v) => Ok(v),
        Helper::Map { by_template } => Ok(by_template),
    }
}

pub fn snapshot_path(env_var: &str, default_path: &str) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
}

pub async fn ensure_snapshot_parent(path: &str) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
}

async fn write_snapshot_atomic(path: &str, payload: &str) -> std::io::Result<()> {
    let path = Path::new(path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("snapshot.json");
    let temp_path = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    let mut file = tokio::fs::File::create(&temp_path).await?;
    file.write_all(payload.as_bytes()).await?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    tokio::fs::rename(&temp_path, path).await
}

async fn append_jsonl_line(path: &str, line: &str) -> std::io::Result<()> {
    ensure_snapshot_parent(path).await;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    file.sync_data().await
}

fn snapshot_group_locks() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snapshot_group_latest_ticks() -> &'static Mutex<HashMap<String, u64>> {
    static LATEST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    LATEST.get_or_init(|| Mutex::new(HashMap::new()))
}

fn snapshot_group_lock(group: &str) -> Arc<AsyncMutex<()>> {
    let mut locks = snapshot_group_locks()
        .lock()
        .expect("snapshot group lock poisoned");
    locks
        .entry(group.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub fn spawn_write_json_snapshot<T>(path: String, snapshot: T)
where
    T: Serialize + Send + 'static,
{
    let payload = match serde_json::to_string(&snapshot) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("Warning: failed to serialize snapshot {}: {}", path, error);
            return;
        }
    };
    tokio::spawn(async move {
        if let Err(error) = write_snapshot_atomic(&path, &payload).await {
            eprintln!(
                "Warning: failed to write snapshot {} atomically: {}",
                path, error
            );
        }
    });
}

pub fn spawn_write_json_snapshot_if_latest<T>(group: String, tick: u64, path: String, snapshot: T)
where
    T: Serialize + Send + 'static,
{
    let payload = match serde_json::to_string(&snapshot) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!(
                "Warning: failed to serialize snapshot batch {}:{} for {}: {}",
                group, tick, path, error
            );
            return;
        }
    };
    spawn_write_json_snapshots_batch(group, tick, vec![(path, payload)]);
}

pub fn spawn_write_json_snapshots_batch(
    group: String,
    tick: u64,
    snapshots: Vec<(String, String)>,
) {
    {
        let mut latest = snapshot_group_latest_ticks()
            .lock()
            .expect("snapshot latest tick lock poisoned");
        latest.insert(group.clone(), tick);
    }

    let lock = snapshot_group_lock(&group);
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        let latest_tick = snapshot_group_latest_ticks()
            .lock()
            .expect("snapshot latest tick lock poisoned")
            .get(&group)
            .copied()
            .unwrap_or_default();
        if latest_tick != tick {
            return;
        }

        for (path, payload) in snapshots {
            if let Err(error) = write_snapshot_atomic(&path, &payload).await {
                eprintln!(
                    "Warning: failed to write snapshot batch {}:{} atomically for {}: {}",
                    group, tick, path, error
                );
                return;
            }
        }
    });
}

pub fn json_payload<T>(snapshot: &T) -> Result<String, serde_json::Error>
where
    T: Serialize,
{
    serde_json::to_string(snapshot)
}

pub fn spawn_write_snapshot(path: String, snapshot: LiveSnapshot) {
    spawn_write_json_snapshot(path, snapshot);
}

pub fn spawn_append_jsonl_line(group: String, path: String, line: String) {
    let lock = snapshot_group_lock(&format!("append:{group}"));
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        if let Err(error) = append_jsonl_line(&path, &line).await {
            eprintln!("Warning: failed to append journal line {}: {}", path, error);
        }
    });
}

pub fn spawn_mutate_text_file<F>(group: String, path: String, transform: F)
where
    F: FnOnce(String) -> String + Send + 'static,
{
    let lock = snapshot_group_lock(&format!("append:{group}"));
    tokio::spawn(async move {
        let _guard = lock.lock().await;
        let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let updated = transform(existing);
        if let Err(error) = write_snapshot_atomic(&path, &updated).await {
            eprintln!("Warning: failed to mutate text file {}: {}", path, error);
        }
    });
}

pub fn horizon_bucket_label(bucket: crate::ontology::horizon::HorizonBucket) -> String {
    match bucket {
        crate::ontology::horizon::HorizonBucket::Tick50 => "tick50".to_string(),
        crate::ontology::horizon::HorizonBucket::Fast5m => "fast5m".to_string(),
        crate::ontology::horizon::HorizonBucket::Mid30m => "mid30m".to_string(),
        crate::ontology::horizon::HorizonBucket::Session => "session".to_string(),
        crate::ontology::horizon::HorizonBucket::MultiSession => "multi_session".to_string(),
    }
}

pub fn horizon_urgency_label(urgency: crate::ontology::horizon::Urgency) -> String {
    match urgency {
        crate::ontology::horizon::Urgency::Immediate => "immediate".to_string(),
        crate::ontology::horizon::Urgency::Normal => "normal".to_string(),
        crate::ontology::horizon::Urgency::Relaxed => "relaxed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::store::ObjectStore;
    use longport::quote::{Trade, TradeDirection, TradeSession};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn test_case(symbol: &str, title: &str) -> LiveTacticalCase {
        LiveTacticalCase {
            setup_id: format!("setup:{symbol}"),
            symbol: symbol.into(),
            title: title.into(),
            action: "enter".into(),
            confidence: dec!(0.72),
            confidence_gap: dec!(0.12),
            heuristic_edge: dec!(0.08),
            entry_rationale: "raw disagreement test".into(),
            causal_narrative: None,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            policy_primary: None,
            policy_reason: None,
            multi_horizon_gate_reason: None,
            family_label: None,
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
            inferred_intent: None,
            freshness_state: None,
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: None,
            timing_position_in_range: None,
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
            raw_disagreement: None,
        }
    }

    fn make_trade(price: Decimal, volume: i64, seconds: i64, direction: TradeDirection) -> Trade {
        Trade {
            price,
            volume,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(seconds),
            trade_type: "automatch".into(),
            direction,
            trade_session: TradeSession::Intraday,
        }
    }

    fn test_snapshot() -> LiveSnapshot {
        LiveSnapshot {
            tick: 1,
            timestamp: "2026-03-22T00:00:00Z".into(),
            market: LiveMarket::Us,
            market_phase: "closed".into(),
            market_active: false,
            stock_count: 1,
            edge_count: 2,
            hypothesis_count: 3,
            observation_count: 4,
            active_positions: 0,
            active_position_nodes: vec![],
            market_regime: LiveMarketRegime {
                bias: "neutral".into(),
                confidence: Decimal::ZERO,
                breadth_up: Decimal::ZERO,
                breadth_down: Decimal::ZERO,
                average_return: Decimal::ZERO,
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: Decimal::ZERO,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            scorecard: LiveScorecard {
                total_signals: 0,
                resolved_signals: 0,
                hits: 0,
                misses: 0,
                hit_rate: Decimal::ZERO,
                mean_return: Decimal::ZERO,
                actionable_resolved: 0,
                actionable_hits: 0,
                actionable_hit_rate: Decimal::ZERO,
                actionable_mean_return: Decimal::ZERO,
                actionable_excess_hit_rate: Decimal::ZERO,
            },
            tactical_cases: vec![],
            hypothesis_tracks: vec![],
            recent_transitions: vec![],
            top_signals: vec![],
            convergence_scores: vec![],
            pressures: vec![],
            backward_chains: vec![],
            causal_leaders: vec![],
            events: vec![],
            temporal_bars: vec![],
            lineage: vec![],
            success_patterns: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            structural_deltas: vec![],
            propagation_senses: vec![],
            raw_microstructure: vec![],
            raw_sources: vec![],
            signal_translation_gaps: vec![],
            cluster_states: vec![],
            symbol_states: vec![],
            world_summary: None,
        }
    }

    #[tokio::test]
    async fn writes_snapshot_atomically() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-live-snapshot-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let path = dir.join("snapshot.json");
        let payload = serde_json::to_string(&test_snapshot()).unwrap();
        write_snapshot_atomic(path.to_str().unwrap(), &payload)
            .await
            .unwrap();

        let written = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(written, payload);

        let temp_files = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count();
        assert_eq!(temp_files, 0);

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn batch_writer_keeps_latest_tick_per_group() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-live-batch-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path_a = dir.join("a.json");
        let path_b = dir.join("b.json");
        let group = format!("test-group-{}", dir.display());

        spawn_write_json_snapshots_batch(
            group.clone(),
            1,
            vec![
                (path_a.to_string_lossy().to_string(), "{\"tick\":1}".into()),
                (path_b.to_string_lossy().to_string(), "{\"tick\":1}".into()),
            ],
        );
        spawn_write_json_snapshots_batch(
            group,
            2,
            vec![
                (path_a.to_string_lossy().to_string(), "{\"tick\":2}".into()),
                (path_b.to_string_lossy().to_string(), "{\"tick\":2}".into()),
            ],
        );

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let written_a = tokio::fs::read_to_string(&path_a).await.unwrap();
        let written_b = tokio::fs::read_to_string(&path_b).await.unwrap();
        assert_eq!(written_a, "{\"tick\":2}");
        assert_eq!(written_b, "{\"tick\":2}");

        let _ = tokio::fs::remove_file(&path_a).await;
        let _ = tokio::fs::remove_file(&path_b).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    struct FailingSnapshot;

    impl Serialize for FailingSnapshot {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("intentional serialize failure"))
        }
    }

    #[tokio::test]
    async fn serialization_failure_does_not_write_empty_snapshot_file() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-live-serialize-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();

        let path = dir.join("snapshot.json");
        spawn_write_json_snapshot(path.to_string_lossy().to_string(), FailingSnapshot);

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert!(
            !path.exists(),
            "serialization failure should not leave an empty snapshot file behind"
        );

        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[test]
    fn raw_disagreement_downgrades_conflicted_enter_case() {
        let symbol = Symbol("AAPL.US".into());
        let mut raw_events = RawEventStore::default();
        raw_events.record_trades(
            symbol.clone(),
            &[
                make_trade(dec!(10), 200, 0, TradeDirection::Down),
                make_trade(dec!(10), 120, 10, TradeDirection::Down),
            ],
            OffsetDateTime::UNIX_EPOCH,
            crate::pipeline::raw_events::RawEventSource::Push,
        );
        let store = ObjectStore::from_parts(vec![], vec![], vec![]);
        let mut cases = vec![test_case("AAPL.US", "Long AAPL.US")];

        apply_raw_disagreement_layer(&raw_events, &store, &mut cases, time::Duration::minutes(5));

        assert_eq!(cases[0].action, "review");
        assert!(cases[0].confidence < dec!(0.72));
        assert_eq!(
            cases[0]
                .raw_disagreement
                .as_ref()
                .map(|item| item.alignment.as_str()),
            Some("conflicted")
        );
        assert_eq!(
            cases[0]
                .raw_disagreement
                .as_ref()
                .and_then(|item| item.original_action.as_deref()),
            Some("enter")
        );
    }

    #[test]
    fn raw_disagreement_keeps_aligned_enter_case() {
        let symbol = Symbol("AAPL.US".into());
        let mut raw_events = RawEventStore::default();
        raw_events.record_trades(
            symbol.clone(),
            &[
                make_trade(dec!(10), 200, 0, TradeDirection::Up),
                make_trade(dec!(10), 120, 10, TradeDirection::Up),
            ],
            OffsetDateTime::UNIX_EPOCH,
            crate::pipeline::raw_events::RawEventSource::Push,
        );
        let store = ObjectStore::from_parts(vec![], vec![], vec![]);
        let mut cases = vec![test_case("AAPL.US", "Long AAPL.US")];

        apply_raw_disagreement_layer(&raw_events, &store, &mut cases, time::Duration::minutes(5));

        assert_eq!(cases[0].action, "enter");
        assert_eq!(cases[0].confidence, dec!(0.72));
        assert_eq!(
            cases[0]
                .raw_disagreement
                .as_ref()
                .map(|item| item.alignment.as_str()),
            Some("aligned")
        );
    }

    #[test]
    fn channel_attention_prioritizes_broker_and_depth() {
        assert!(attention_weight_for_channel("broker") > attention_weight_for_channel("quote"));
        assert!(attention_weight_for_channel("depth") > attention_weight_for_channel("candle"));
        assert!(attention_weight_for_channel("trade") > attention_weight_for_channel("calc_index"));
    }

    #[test]
    fn raw_alignment_uses_weighted_strength_not_vote_count() {
        assert_eq!(
            classify_raw_alignment(1, 2, dec!(0.92), dec!(0.40)),
            "aligned"
        );
        assert_eq!(
            classify_raw_alignment(2, 1, dec!(0.35), dec!(0.41)),
            "conflicted"
        );
    }

    #[test]
    fn translation_gaps_surface_strong_signals_without_cases() {
        let top_signals = vec![LiveSignal {
            symbol: "QUBT.US".into(),
            sector: Some("Quantum".into()),
            composite: dec!(0.51),
            mark_price: None,
            dimension_composite: None,
            capital_flow_direction: Decimal::ZERO,
            price_momentum: Decimal::ONE,
            volume_profile: dec!(0.2),
            pre_post_market_anomaly: Decimal::ONE,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        }];
        let raw_sources = vec![
            LiveRawSource {
                source: "quote".into(),
                symbol: Some("QUBT.US".into()),
                scope: "symbol".into(),
                summary: "last quote 8.120 (+7.50% vs prev close)".into(),
                window_start: None,
                window_end: None,
                payload: Value::Null,
            },
            LiveRawSource {
                source: "calc_index".into(),
                symbol: Some("QUBT.US".into()),
                scope: "symbol".into(),
                summary: "volume_ratio=6.17, change_rate=7.42%, 5m_change=-0.56%".into(),
                window_start: None,
                window_end: None,
                payload: Value::Null,
            },
        ];

        let gaps = build_signal_translation_gaps(&[], &top_signals, &raw_sources, 8);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].symbol, "QUBT.US");
        assert!(gaps[0].summary.contains("not yet represented"));
        assert_eq!(gaps[0].raw_highlights.len(), 2);
    }

    #[test]
    fn translation_gaps_can_become_review_cases() {
        let gaps = vec![LiveSignalTranslationGap {
            symbol: "QUBT.US".into(),
            sector: Some("Quantum".into()),
            composite: dec!(0.51),
            pre_post_market_anomaly: Decimal::ONE,
            price_momentum: Decimal::ONE,
            capital_flow_direction: Decimal::ZERO,
            summary: "strong top signal is not yet represented in tactical cases".into(),
            raw_highlights: vec![
                "quote: last quote 8.195 (+12.72% vs prev close)".into(),
                "calc_index: volume_ratio=5.42, change_rate=12.72%".into(),
            ],
        }];

        let cases = build_signal_translation_cases(&gaps, 3);
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].symbol, "QUBT.US");
        assert_eq!(cases[0].action, "review");
        assert!(cases[0].title.contains("signal translation"));
        assert_eq!(
            cases[0].review_reason_code.as_deref(),
            Some("signal_translation_gap")
        );
    }

    #[test]
    fn materialized_translation_case_promotes_on_raw_follow_through() {
        let symbol = Symbol("QUBT.US".into());
        let mut raw_events = RawEventStore::default();
        raw_events.record_quote(
            symbol.clone(),
            longport::quote::SecurityQuote {
                symbol: "QUBT.US".into(),
                last_done: dec!(8.195),
                prev_close: dec!(7.27),
                open: dec!(8.10),
                high: dec!(8.25),
                low: dec!(8.00),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                volume: 1000,
                turnover: dec!(8195),
                trade_status: longport::quote::TradeStatus::Normal,
                pre_market_quote: None,
                post_market_quote: None,
                overnight_quote: None,
            },
            crate::pipeline::raw_events::RawEventSource::Rest,
        );
        raw_events.record_trades(
            symbol.clone(),
            &[make_trade(dec!(8.19), 500, 1, TradeDirection::Up)],
            OffsetDateTime::UNIX_EPOCH,
            crate::pipeline::raw_events::RawEventSource::Push,
        );
        let store = ObjectStore::from_parts(vec![], vec![], vec![]);
        let gaps = vec![LiveSignalTranslationGap {
            symbol: "QUBT.US".into(),
            sector: Some("Quantum".into()),
            composite: dec!(0.51),
            pre_post_market_anomaly: Decimal::ONE,
            price_momentum: Decimal::ONE,
            capital_flow_direction: Decimal::ZERO,
            summary: "strong top signal is not yet represented in tactical cases".into(),
            raw_highlights: vec![],
        }];

        let cases = materialize_signal_translation_cases(
            &raw_events,
            &store,
            &gaps,
            time::Duration::minutes(5),
            3,
            true,
        );
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].action, "enter");
        assert_eq!(
            cases[0].policy_primary.as_deref(),
            Some("translation_confirmed")
        );
    }

    #[test]
    fn orphan_signal_enter_is_capped_to_review() {
        let mut cases = vec![LiveTacticalCase {
            setup_id: "setup:orphan".into(),
            symbol: "MSTR.US".into(),
            title: "Long MSTR.US".into(),
            action: "enter".into(),
            confidence: dec!(0.82),
            confidence_gap: Decimal::ZERO,
            heuristic_edge: dec!(0.12),
            entry_rationale: "orphan".into(),
            causal_narrative: None,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            policy_primary: None,
            policy_reason: None,
            multi_horizon_gate_reason: None,
            family_label: None,
            counter_label: None,
            matched_success_pattern_signature: None,
            lifecycle_phase: None,
            tension_driver: Some("orphan_signal".into()),
            driver_class: Some("orphan_signal".into()),
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
            inferred_intent: None,
            freshness_state: None,
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: None,
            timing_position_in_range: None,
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
            raw_disagreement: None,
        }];

        enforce_orphan_action_cap(&mut cases);

        assert_eq!(cases[0].action, "review");
        assert_eq!(
            cases[0].review_reason_code.as_deref(),
            Some("orphan_signal_cap")
        );
        assert_eq!(
            cases[0].policy_primary.as_deref(),
            Some("orphan_signal_capped")
        );
        assert_eq!(cases[0].confidence, dec!(0.65));
    }

    #[test]
    fn non_orphan_enter_is_not_capped() {
        let mut cases = vec![LiveTacticalCase {
            setup_id: "setup:vortex".into(),
            symbol: "MARA.US".into(),
            title: "Long MARA.US".into(),
            action: "enter".into(),
            confidence: dec!(0.82),
            confidence_gap: Decimal::ZERO,
            heuristic_edge: dec!(0.12),
            entry_rationale: "vortex".into(),
            causal_narrative: None,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            policy_primary: None,
            policy_reason: None,
            multi_horizon_gate_reason: None,
            family_label: None,
            counter_label: None,
            matched_success_pattern_signature: None,
            lifecycle_phase: None,
            tension_driver: Some("sector_wave".into()),
            driver_class: Some("cluster_confirmed".into()),
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
            inferred_intent: None,
            freshness_state: None,
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: None,
            timing_position_in_range: None,
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
            raw_disagreement: None,
        }];

        enforce_orphan_action_cap(&mut cases);

        assert_eq!(cases[0].action, "enter");
        assert_eq!(cases[0].confidence, dec!(0.82));
        assert!(cases[0].review_reason_code.is_none());
    }

    #[test]
    fn structural_notes_populate_peer_and_lifecycle_fields() {
        let mut case = test_case("SNAP.US", "Long SNAP.US");
        let notes = vec![
            "phase=Growing".to_string(),
            "driver=trade_flow".to_string(),
            "driver_class=sector_wave".to_string(),
            "peer_confirmation_ratio=0.9722".to_string(),
            "peer_active_count=4".to_string(),
            "peer_silent_count=1".to_string(),
            "competition_margin=0.3300".to_string(),
            "driver_confidence=0.8000".to_string(),
            "absence_summary=SNAP.US active while sector peers stay silent".to_string(),
            "competition_summary=best explanation is SectorWide over CompanySpecific".to_string(),
            "competition_winner=SectorWide".to_string(),
            "competition_runner_up=CompanySpecific".to_string(),
            "velocity=0.1800".to_string(),
            "acceleration=0.2600".to_string(),
        ];

        apply_case_structural_notes(&mut case, &notes);

        assert_eq!(case.lifecycle_phase.as_deref(), Some("Growing"));
        assert_eq!(case.driver_class.as_deref(), Some("sector_wave"));
        assert_eq!(case.peer_confirmation_ratio, Some(dec!(0.9722)));
        assert_eq!(case.driver_confidence, Some(dec!(0.8)));
        assert_eq!(
            case.absence_summary.as_deref(),
            Some("SNAP.US active while sector peers stay silent")
        );
        assert_eq!(
            case.competition_summary.as_deref(),
            Some("best explanation is SectorWide over CompanySpecific")
        );
        assert_eq!(case.competition_winner.as_deref(), Some("SectorWide"));
        assert_eq!(
            case.competition_runner_up.as_deref(),
            Some("CompanySpecific")
        );
        assert_eq!(case.lifecycle_velocity, Some(dec!(0.18)));
        assert_eq!(case.lifecycle_acceleration, Some(dec!(0.26)));
    }

    #[test]
    fn high_confidence_case_gets_fallback_peer_and_lifecycle_values() {
        let mut cases = vec![
            test_case("SNAP.US", "Long SNAP.US"),
            test_case("META.US", "Long META.US"),
        ];
        cases[0].driver_class = Some("sector_wave".into());
        cases[0].family_label = Some("Momentum".into());
        cases[0].confidence = dec!(0.81);
        cases[1].driver_class = Some("sector_wave".into());
        cases[1].family_label = Some("Momentum".into());
        cases[1].confidence = dec!(0.76);

        let sectors = HashMap::from([
            ("SNAP.US".to_string(), "Internet".to_string()),
            ("META.US".to_string(), "Internet".to_string()),
        ]);
        let history = vec![
            SurfacedCaseHistorySample {
                tick: 10,
                setup_id: cases[0].setup_id.clone(),
                symbol: cases[0].symbol.clone(),
                confidence: dec!(0.70),
                support_fraction: None,
            },
            SurfacedCaseHistorySample {
                tick: 11,
                setup_id: cases[0].setup_id.clone(),
                symbol: cases[0].symbol.clone(),
                confidence: dec!(0.74),
                support_fraction: None,
            },
        ];

        enrich_surfaced_case_evidence(&mut cases, &sectors, &history);

        assert_eq!(cases[0].peer_confirmation_ratio, Some(dec!(1)));
        assert_eq!(cases[0].lifecycle_velocity, Some(dec!(0.07)));
        assert_eq!(cases[0].lifecycle_acceleration, Some(dec!(0.03)));
    }

    #[test]
    fn priority_rank_prefers_accelerating_non_chase_case() {
        let mut snap = test_case("SNAP.US", "Long SNAP.US");
        snap.confidence = dec!(0.84);
        snap.actionability_score = Some(dec!(0.52));
        snap.actionability_state = Some("observe_only".into());
        snap.freshness_state = Some("fresh".into());
        snap.timing_state = Some("timely".into());
        snap.timing_position_in_range = Some(dec!(0.31));
        snap.peer_confirmation_ratio = Some(dec!(0.97));
        snap.lifecycle_velocity = Some(dec!(0.18));
        snap.lifecycle_acceleration = Some(dec!(0.26));
        snap.raw_disagreement = Some(LiveRawDisagreement {
            alignment: "aligned".into(),
            expected_direction: "buy".into(),
            support_count: 3,
            contradict_count: 1,
            count_support_fraction: dec!(0.75),
            support_fraction: dec!(0.75),
            support_weight: dec!(0.75),
            contradict_weight: dec!(0.25),
            adjusted_action: "enter".into(),
            adjusted_confidence: dec!(0.84),
            summary: "aligned".into(),
            supporting_sources: vec![],
            contradicting_sources: vec![],
            original_action: None,
            original_confidence: None,
        });

        let mut ionq = test_case("IONQ.US", "Long IONQ.US");
        ionq.confidence = dec!(0.84);
        ionq.actionability_score = Some(dec!(0.52));
        ionq.actionability_state = Some("observe_only".into());
        ionq.freshness_state = Some("fresh".into());
        ionq.timing_state = Some("late_chase".into());
        ionq.timing_position_in_range = Some(dec!(0.84));
        ionq.peer_confirmation_ratio = Some(dec!(0.97));
        ionq.lifecycle_velocity = Some(dec!(0.30));
        ionq.lifecycle_acceleration = Some(dec!(0.12));
        ionq.raw_disagreement = snap.raw_disagreement.clone();

        let mut cases = vec![ionq, snap];
        apply_priority_ranking(&mut cases);
        sort_tactical_cases_for_surface(&mut cases);

        assert_eq!(cases[0].symbol, "SNAP.US");
        assert_eq!(cases[0].priority_rank, Some(1));
        assert_eq!(cases[1].symbol, "IONQ.US");
        assert_eq!(cases[1].priority_rank, Some(2));
    }

    #[test]
    fn cross_tick_momentum_tracks_first_seen_and_confidence_velocity() {
        // Unique symbol so test ordering against other cross_tick_momentum
        // tests doesn't pollute the global SURFACED_CASE_MOMENTUM store.
        let mut cases = vec![test_case("XTRACK1.US", "Long XTRACK1.US")];
        cases[0].raw_disagreement = Some(LiveRawDisagreement {
            alignment: "aligned".into(),
            expected_direction: "buy".into(),
            support_count: 3,
            contradict_count: 1,
            count_support_fraction: dec!(0.75),
            support_fraction: dec!(0.75),
            support_weight: dec!(0.75),
            contradict_weight: dec!(0.25),
            adjusted_action: "enter".into(),
            adjusted_confidence: dec!(0.72),
            summary: "aligned".into(),
            supporting_sources: vec![],
            contradicting_sources: vec![],
            original_action: None,
            original_confidence: None,
        });

        enrich_cross_tick_momentum(LiveMarket::Us, 100, &mut cases);
        assert_eq!(cases[0].ticks_since_first_seen, Some(0));
        assert_eq!(cases[0].confidence_velocity_5t, None);
        assert_eq!(cases[0].support_fraction_velocity_5t, None);

        cases[0].confidence = dec!(0.90);
        if let Some(raw) = cases[0].raw_disagreement.as_mut() {
            raw.support_fraction = dec!(1.0);
        }
        enrich_cross_tick_momentum(LiveMarket::Us, 106, &mut cases);
        assert_eq!(cases[0].ticks_since_first_seen, Some(6));
        assert_eq!(cases[0].confidence_velocity_5t, Some(dec!(0.18)));
        assert_eq!(cases[0].support_fraction_velocity_5t, Some(dec!(0.25)));
    }

    #[test]
    fn cross_tick_momentum_is_market_scoped() {
        let mut us_case = vec![test_case("SNAP.US", "Long SNAP.US")];
        let mut hk_case = vec![test_case("700.HK", "Long 700.HK")];

        enrich_cross_tick_momentum(LiveMarket::Us, 200, &mut us_case);
        enrich_cross_tick_momentum(LiveMarket::Hk, 200, &mut hk_case);

        assert_eq!(us_case[0].ticks_since_first_seen, Some(0));
        assert_eq!(hk_case[0].ticks_since_first_seen, Some(0));
    }

    #[test]
    fn raw_alignment_requires_majority_support() {
        assert_eq!(
            classify_raw_alignment(3, 3, dec!(0.5), dec!(0.5)),
            "conflicted"
        );
        assert_eq!(
            classify_raw_alignment(2, 4, dec!(0.3), dec!(0.7)),
            "conflicted"
        );
        assert_eq!(
            classify_raw_alignment(5, 1, dec!(0.8), dec!(0.1)),
            "aligned"
        );
    }

    #[test]
    fn enter_requires_supermajority_raw_support() {
        let (action, _) = adjusted_case_surface(
            "enter",
            dec!(0.9),
            "aligned",
            Decimal::new(50, 2),
            Decimal::ZERO,
        );
        assert_eq!(action, "review");
        let (action, _) = adjusted_case_surface(
            "enter",
            dec!(0.9),
            "aligned",
            Decimal::new(67, 2),
            Decimal::ZERO,
        );
        assert_eq!(action, "enter");
    }

    #[tokio::test]
    async fn append_jsonl_is_serialized_per_group() {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "eden-jsonl-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal.jsonl");

        spawn_append_jsonl_line(
            "test-journal".into(),
            path.to_string_lossy().to_string(),
            "{\"tick\":1}\n".into(),
        );
        spawn_append_jsonl_line(
            "test-journal".into(),
            path.to_string_lossy().to_string(),
            "{\"tick\":2}\n".into(),
        );

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let written = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(written.contains("{\"tick\":1}\n"));
        assert!(written.contains("{\"tick\":2}\n"));

        let _ = tokio::fs::remove_file(&path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[test]
    fn horizon_bucket_label_is_snake_case() {
        use crate::ontology::horizon::HorizonBucket;
        assert_eq!(horizon_bucket_label(HorizonBucket::Fast5m), "fast5m");
        assert_eq!(horizon_bucket_label(HorizonBucket::Mid30m), "mid30m");
        assert_eq!(horizon_bucket_label(HorizonBucket::Session), "session");
        assert_eq!(
            horizon_bucket_label(HorizonBucket::MultiSession),
            "multi_session"
        );
    }

    #[test]
    fn horizon_urgency_label_is_snake_case() {
        use crate::ontology::horizon::Urgency;
        assert_eq!(horizon_urgency_label(Urgency::Immediate), "immediate");
        assert_eq!(horizon_urgency_label(Urgency::Normal), "normal");
        assert_eq!(horizon_urgency_label(Urgency::Relaxed), "relaxed");
    }
}

/// Read the most recent NDJSON records from the tail of a file.
///
/// Reads at most `buffer_bytes` from the end of the file, drops any
/// partial first or last line, parses remaining lines as `T` (skipping
/// JSON parse errors), and returns the most recent `max_records`
/// successfully parsed records (preserving file order, oldest first).
///
/// Returns empty `Vec` if file does not exist. This is intentional:
/// fresh runtime has no perception streams yet — caller should treat
/// missing files as "no perception data" rather than as errors.
pub fn tail_records<T>(path: &std::path::Path, buffer_bytes: u64, max_records: usize) -> Vec<T>
where
    T: serde::de::DeserializeOwned,
{
    use std::io::{Read, Seek, SeekFrom};

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let file_len = metadata.len();
    let read_len = buffer_bytes.min(file_len);
    let seek_from = file_len.saturating_sub(read_len);

    if file.seek(SeekFrom::Start(seek_from)).is_err() {
        return Vec::new();
    }

    let mut buf = Vec::with_capacity(read_len as usize);
    if file.take(read_len).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }

    let text = match std::str::from_utf8(&buf) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // If we did not start at offset 0, the first line is potentially partial.
    let start = if seek_from > 0 { 1 } else { 0 };

    // Always drop the final element from the split: when the file ends with
    // '\n' it's an empty trailing string; when it doesn't, it's a record
    // currently being written by the producer (we'd rather drop one complete
    // record on a non-newline-terminated tail than ingest a half-written one).
    // NDJSON producers in this codebase always terminate records with '\n',
    // so the dropped record is normally the empty trailing string.
    let end = lines.len().saturating_sub(1);

    if start >= end {
        return Vec::new();
    }

    let parsed: Vec<T> = lines[start..end]
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<T>(trimmed).ok()
        })
        .collect();

    if parsed.len() <= max_records {
        parsed
    } else {
        parsed.into_iter().rev().take(max_records).rev().collect()
    }
}

/// Internal NDJSON record shape for `eden-emergence-{market}.ndjson`.
/// Mirrors `pipeline::sub_kg_emergence::EmergenceClusterEvent` minus
/// internal fields. We use a private deserialise-only struct rather
/// than depending on the producer struct so the reader is decoupled
/// from upstream schema changes.
#[derive(Debug, serde::Deserialize)]
struct RawEmergenceRecord {
    cluster_key: String,
    cluster_total_members: u32,
    sync_member_count: u32,
    #[serde(default)]
    sync_members: Vec<String>,
    #[serde(default)]
    mean_activation_per_kind: std::collections::HashMap<String, f64>,
    strongest_member: String,
    strongest_member_mean_activation: f64,
}

const PERCEPTION_TAIL_BYTES: u64 = 256 * 1024;
const EMERGENCE_MAX_RECORDS: usize = 30;

/// Read recent emergence cluster records from the NDJSON stream and
/// surface those passing the filter as `EmergentCluster`s.
pub fn read_emergent_clusters(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::EmergentCluster> {
    let raw: Vec<RawEmergenceRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, EMERGENCE_MAX_RECORDS);
    raw.into_iter()
        .filter_map(|rec| {
            let total = rec.cluster_total_members.max(1);
            let sync_pct = rec.sync_member_count as f64 / total as f64;
            if sync_pct < cfg.min_cluster_sync_pct {
                return None;
            }
            Some(crate::agent::EmergentCluster {
                sector: rec.cluster_key,
                total_members: rec.cluster_total_members,
                sync_member_count: rec.sync_member_count,
                sync_ratio: format!("{}/{}", rec.sync_member_count, rec.cluster_total_members),
                sync_pct,
                strongest_member: rec.strongest_member,
                strongest_activation: rec.strongest_member_mean_activation,
                mean_activation_intent: rec.mean_activation_per_kind.get("Intent").copied().unwrap_or(0.0),
                mean_activation_pressure: rec.mean_activation_per_kind.get("Pressure").copied().unwrap_or(0.0),
                members: rec.sync_members,
            })
        })
        .collect()
}

#[derive(Debug, serde::Deserialize)]
struct RawContrastRecord {
    symbol: String,
    #[serde(default)]
    sector_id: Option<String>,
    center_activation: f64,
    sector_mean_activation: f64,
    vs_sector_contrast: f64,
    node_kind: String,
}

const CONTRAST_MAX_RECORDS: usize = 100;

pub fn read_sector_leaders(
    path: &std::path::Path,
    cfg: &crate::agent::PerceptionFilterConfig,
) -> Vec<crate::agent::SymbolContrast> {
    let raw: Vec<RawContrastRecord> = tail_records(path, PERCEPTION_TAIL_BYTES, CONTRAST_MAX_RECORDS);
    let mut filtered: Vec<crate::agent::SymbolContrast> = raw
        .into_iter()
        .filter(|rec| rec.vs_sector_contrast >= cfg.min_leader_contrast)
        .map(|rec| crate::agent::SymbolContrast {
            symbol: rec.symbol,
            sector: rec.sector_id,
            center_activation: rec.center_activation,
            sector_mean: rec.sector_mean_activation,
            vs_sector_contrast: rec.vs_sector_contrast,
            node_kind: rec.node_kind,
            persistence_ticks: None,
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.vs_sector_contrast
            .partial_cmp(&a.vs_sector_contrast)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    filtered.truncate(cfg.max_leaders);
    filtered
}

#[cfg(test)]
mod perception_reader_tests {
    use super::*;
    use serde::Deserialize;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[derive(Debug, Deserialize, PartialEq)]
    struct ToyRecord {
        id: u32,
        value: String,
    }

    fn make_ndjson(records: &[(u32, &str)]) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp file");
        for (id, value) in records {
            writeln!(f, r#"{{"id":{id},"value":"{value}"}}"#).expect("write");
        }
        f
    }

    #[test]
    fn tail_records_returns_empty_when_file_missing() {
        let path = std::path::PathBuf::from("/nonexistent/path/zzz.ndjson");
        let out: Vec<ToyRecord> = tail_records(&path, 1024, 10);
        assert!(out.is_empty());
    }

    #[test]
    fn tail_records_reads_all_when_file_smaller_than_buffer() {
        let f = make_ndjson(&[(1, "a"), (2, "b"), (3, "c")]);
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 10);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].id, 1);
        assert_eq!(out[2].value, "c");
    }

    #[test]
    fn tail_records_caps_at_max_records() {
        let f = make_ndjson(&[(1, "a"), (2, "b"), (3, "c"), (4, "d"), (5, "e")]);
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 2);
        // Most recent 2: ids 4 and 5
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, 4);
        assert_eq!(out[1].id, 5);
    }

    #[test]
    fn tail_records_skips_partial_first_line_in_buffer() {
        // Build a file where the buffer window cuts mid-line.
        let mut f = NamedTempFile::new().expect("temp file");
        // First record is long; next two are short. With small buffer we'll
        // read mid-way through the first record and must drop it.
        let long_value = "x".repeat(500);
        writeln!(f, r#"{{"id":1,"value":"{}"}}"#, long_value).expect("write");
        writeln!(f, r#"{{"id":2,"value":"b"}}"#).expect("write");
        writeln!(f, r#"{{"id":3,"value":"c"}}"#).expect("write");
        let out: Vec<ToyRecord> = tail_records(f.path(), 64, 10);
        // Must NOT include id=1 (partial). Must include id=2 and id=3.
        assert!(out.iter().all(|r| r.id != 1));
        assert!(out.iter().any(|r| r.id == 2));
        assert!(out.iter().any(|r| r.id == 3));
    }

    #[test]
    fn tail_records_skips_unparseable_lines() {
        let mut f = NamedTempFile::new().expect("temp file");
        writeln!(f, r#"{{"id":1,"value":"a"}}"#).expect("write");
        writeln!(f, "not valid json").expect("write");
        writeln!(f, r#"{{"id":3,"value":"c"}}"#).expect("write");
        let out: Vec<ToyRecord> = tail_records(f.path(), 4096, 10);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, 1);
        assert_eq!(out[1].id, 3);
    }

    #[test]
    fn read_emergent_clusters_filters_by_sync_pct() {
        let mut f = NamedTempFile::new().expect("temp file");
        // 9/9 sync (100%) — included
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"semiconductor","cluster_total_members":9,"sync_member_count":9,"sync_members":["981.HK"],"lit_node_kinds":["Pressure","Intent"],"mean_activation_per_kind":{{"Intent":0.5,"Pressure":0.7}},"strongest_member":"6809.HK","strongest_member_mean_activation":0.79}}"#).expect("write");
        // 3/10 sync (30%) — filtered out (below 70%)
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"toys","cluster_total_members":10,"sync_member_count":3,"sync_members":["x"],"lit_node_kinds":["Intent"],"mean_activation_per_kind":{{"Intent":0.3}},"strongest_member":"x","strongest_member_mean_activation":0.3}}"#).expect("write");
        // 8/10 sync (80%) — included
        writeln!(f, r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","cluster_key":"tech","cluster_total_members":10,"sync_member_count":8,"sync_members":["a","b"],"lit_node_kinds":["Pressure"],"mean_activation_per_kind":{{"Pressure":0.6}},"strongest_member":"a","strongest_member_mean_activation":0.6}}"#).expect("write");

        let cfg = crate::agent::PerceptionFilterConfig::default();
        let out = read_emergent_clusters(f.path(), &cfg);
        assert_eq!(out.len(), 2, "expected only sync >= 70%");
        assert!(out.iter().any(|c| c.sector == "semiconductor"));
        assert!(out.iter().any(|c| c.sector == "tech"));
        assert!(!out.iter().any(|c| c.sector == "toys"));
    }

    #[test]
    fn read_sector_leaders_filters_and_caps() {
        let mut f = NamedTempFile::new().expect("temp file");
        for (sym, contrast) in &[("a", 7.5), ("b", 4.0), ("c", 2.0), ("d", 6.0), ("e", 1.0)] {
            writeln!(
                f,
                r#"{{"ts":"2026-04-30T09:00:00Z","market":"hk","symbol":"{}.HK","node_kind":"Role","center_activation":10.0,"surround_mean":1.0,"surround_count":20,"contrast":9.0,"sector_id":"semiconductor","sector_mean_activation":3.0,"vs_sector_contrast":{}}}"#,
                sym, contrast
            ).expect("write");
        }
        let mut cfg = crate::agent::PerceptionFilterConfig::default();
        cfg.max_leaders = 3;
        let out = read_sector_leaders(f.path(), &cfg);
        // Only a (7.5), d (6.0), b (4.0) pass min_leader_contrast (3.0); c & e filtered
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].symbol, "a.HK");
        assert!((out[0].vs_sector_contrast - 7.5).abs() < 1e-9);
        assert_eq!(out[1].symbol, "d.HK");
        assert_eq!(out[2].symbol, "b.HK");
    }
}
