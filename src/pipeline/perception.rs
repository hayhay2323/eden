use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::agent::AgentTransition;
use crate::live_snapshot::{
    LiveClusterState, LiveMarket, LiveSignal, LiveTacticalCase, LiveWorldSummary,
};
use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::objects::{CustomScopeId, SectorId, Symbol};
use crate::ontology::reasoning::ReasoningScope;
use crate::ontology::world::{
    EntityState, FlowPath, FlowPolarity, Vortex, WorldLayer, WorldStateSnapshot,
};
use crate::ontology::{
    action_direction_case_label, action_direction_from_case_label,
    action_direction_from_title_prefix,
};
use crate::pipeline::state_engine::{
    build_cluster_states_from_symbol_states, build_world_summary_from_symbol_states,
    derive_symbol_states, PersistentStateKind, PersistentSymbolState,
};

pub struct PerceptionArtifacts {
    pub cluster_states: Vec<LiveClusterState>,
    pub symbol_states: Vec<PersistentSymbolState>,
    pub world_summary: Option<LiveWorldSummary>,
}

pub fn apply_perception_layer(
    tick: u64,
    market: LiveMarket,
    _timestamp: &str,
    cases: &mut [LiveTacticalCase],
    recent_transitions: &[AgentTransition],
    top_signals: &[LiveSignal],
    previous_states: &[PersistentSymbolState],
    previous_cluster_states: &[LiveClusterState],
    previous_world_summary: Option<&LiveWorldSummary>,
) -> PerceptionArtifacts {
    let symbol_states = derive_symbol_states(
        tick,
        market,
        cases,
        recent_transitions,
        top_signals,
        previous_states,
    );
    let symbol_state_by_symbol = symbol_states
        .iter()
        .map(|state| (state.symbol.as_str(), state))
        .collect::<HashMap<_, _>>();

    for case in cases.iter_mut() {
        let recent_flip = has_recent_direction_flip(case, recent_transitions);
        let direction_stability_rounds =
            infer_direction_stability_rounds(tick, case, recent_transitions);
        let state = symbol_state_by_symbol
            .get(case.symbol.as_str())
            .copied()
            .or_else(|| {
                symbol_states
                    .iter()
                    .find(|state| state.symbol == case.symbol)
            });
        let actionability_score = state
            .map(|state| compute_actionability_score(case, state, recent_flip))
            .unwrap_or(Decimal::ZERO);
        case.local_state = state.map(|state| state.state_kind.as_str().to_string());
        case.local_state_confidence = state.map(|state| state.confidence);
        case.actionability_score = Some(actionability_score);
        case.actionability_state = Some(
            state
                .map(|state| actionability_state_from_score(state.state_kind, actionability_score))
                .unwrap_or("do_not_trade")
                .into(),
        );
        case.state_persistence_ticks = state.map(|state| state.state_persistence_ticks);
        case.direction_stability_rounds = Some(direction_stability_rounds);
        case.state_reason_codes = state
            .map(PersistentSymbolState::reason_codes)
            .unwrap_or_else(|| vec!["low_information".into()]);
        apply_raw_persistence_gate(
            case,
            state
                .map(|state| state.state_persistence_ticks)
                .unwrap_or(1),
            recent_flip,
        );
    }

    let cluster_states =
        build_cluster_states_from_symbol_states(&symbol_states, previous_cluster_states);
    let world_summary = Some(build_world_summary_from_symbol_states(
        market,
        &cluster_states,
        previous_world_summary,
    ));
    PerceptionArtifacts {
        cluster_states,
        symbol_states,
        world_summary,
    }
}

pub fn build_world_state_snapshot(
    market: LiveMarket,
    timestamp: &str,
    symbol_states: &[PersistentSymbolState],
    clusters: &[LiveClusterState],
    world_summary: Option<&LiveWorldSummary>,
) -> WorldStateSnapshot {
    let observed_at =
        OffsetDateTime::parse(timestamp, &Rfc3339).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let provenance = ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at)
        .with_note("persistent_state_engine_v1");
    let perceptual_states = symbol_states
        .iter()
        .map(PersistentSymbolState::to_perceptual_state)
        .collect::<Vec<_>>();

    let mut entities = Vec::new();
    for state in symbol_states.iter().cloned() {
        let mut drivers = state
            .supporting_evidence
            .iter()
            .map(|item| format!("for:{}", item.code))
            .collect::<Vec<_>>();
        drivers.extend(
            state
                .opposing_evidence
                .iter()
                .map(|item| format!("against:{}", item.code)),
        );
        drivers.push(format!("trend={}", state.trend));
        if let Some(summary) = state.last_transition_summary.clone() {
            drivers.push(summary);
        }

        let scope = ReasoningScope::Symbol(Symbol(state.symbol.clone()));
        let local_support = match state.state_kind {
            PersistentStateKind::LowInformation => Decimal::ZERO,
            _ => state.strength,
        };
        entities.push(EntityState {
            entity_id: format!("symbol:{}", state.symbol),
            scope,
            layer: WorldLayer::Leaf,
            provenance: provenance.clone(),
            label: state.label,
            regime: state.state_kind.as_str().to_string(),
            confidence: state.confidence,
            local_support,
            propagated_support: state.strength,
            drivers,
        });
    }

    for cluster in clusters {
        let scope = if let Some(sector) = cluster.cluster_key.strip_prefix("sector:") {
            ReasoningScope::Sector(SectorId(sector.to_string()))
        } else {
            ReasoningScope::Custom(CustomScopeId(cluster.cluster_key.clone()))
        };
        entities.push(EntityState {
            entity_id: format!("cluster:{}", cluster.cluster_key),
            scope,
            layer: WorldLayer::Branch,
            provenance: provenance.clone(),
            label: cluster.label.clone(),
            regime: cluster.state.clone(),
            confidence: cluster.confidence,
            local_support: cluster.confidence,
            propagated_support: cluster.confidence,
            drivers: vec![format!("direction={}", cluster.direction)],
        });
    }

    let market_regime = world_summary
        .map(|summary| summary.regime.clone())
        .unwrap_or_else(|| "low_information".into());
    let market_confidence = world_summary
        .map(|summary| summary.confidence)
        .unwrap_or(Decimal::ZERO);
    entities.push(EntityState {
        entity_id: format!("market:{:?}", market).to_ascii_lowercase(),
        scope: ReasoningScope::market(),
        layer: WorldLayer::Forest,
        provenance: provenance.clone(),
        label: market_label(market).into(),
        regime: market_regime,
        confidence: market_confidence,
        local_support: market_confidence,
        propagated_support: market_confidence,
        drivers: world_summary
            .map(|summary| summary.dominant_clusters.clone())
            .unwrap_or_default(),
    });

    let vortices = clusters
        .iter()
        .filter(|cluster| cluster.state == "continuation" && cluster.member_count >= 2)
        .map(|cluster| Vortex {
            vortex_id: format!("vortex:{}", cluster.cluster_key),
            center_entity_id: format!("cluster:{}", cluster.cluster_key),
            center_scope: if let Some(sector) = cluster.cluster_key.strip_prefix("sector:") {
                ReasoningScope::Sector(SectorId(sector.to_string()))
            } else {
                ReasoningScope::Custom(CustomScopeId(cluster.cluster_key.clone()))
            },
            layer: WorldLayer::Branch,
            flow_paths: cluster
                .leader_symbols
                .iter()
                .map(|symbol| FlowPath {
                    source_entity_id: format!("symbol:{symbol}"),
                    source_scope: ReasoningScope::Symbol(Symbol(symbol.clone())),
                    channel: "continuation".into(),
                    weight: cluster.confidence,
                    polarity: FlowPolarity::Confirming,
                })
                .collect(),
            strength: cluster.confidence,
            channel_diversity: 1,
            coherence: cluster.confidence,
            narrative: Some(cluster.summary.clone()),
        })
        .collect();

    WorldStateSnapshot {
        timestamp: observed_at,
        entities,
        world_intents: vec![],
        perceptual_states,
        vortices,
    }
}

fn compute_actionability_score(
    case: &LiveTacticalCase,
    state: &PersistentSymbolState,
    recent_flip: bool,
) -> Decimal {
    let base = match state.state_kind {
        PersistentStateKind::Continuation => dec!(0.72),
        PersistentStateKind::Latent => dec!(0.46),
        PersistentStateKind::TurningPoint => dec!(0.34),
        PersistentStateKind::Conflicted => dec!(0.18),
        PersistentStateKind::LowInformation => dec!(0.12),
    };
    let support_fraction = raw_support_fraction(case);
    let mut score = base
        + state.strength * dec!(0.18)
        + state.confidence * dec!(0.14)
        + support_fraction * dec!(0.16);

    match state.trend {
        crate::pipeline::state_engine::PersistentStateTrend::Strengthening => {
            score += dec!(0.08);
        }
        crate::pipeline::state_engine::PersistentStateTrend::Weakening => {
            score -= dec!(0.12);
        }
        crate::pipeline::state_engine::PersistentStateTrend::Stable => {}
    }

    match case.timing_state.as_deref() {
        Some("late_chase") => score -= dec!(0.20),
        Some("range_extreme") => score -= dec!(0.12),
        Some("timely") => score += dec!(0.03),
        _ => {}
    }
    match case.freshness_state.as_deref() {
        Some("carried_forward") | Some("stale") | Some("expired") => score -= dec!(0.18),
        Some("fresh") => score += dec!(0.02),
        Some("aging") => score -= dec!(0.03),
        _ => {}
    }
    if recent_flip {
        score -= dec!(0.15);
    }
    if matches!(
        case.review_reason_code.as_deref(),
        Some("signal_translation_gap" | "stale_symbol_confirmation")
    ) {
        score -= dec!(0.10);
    }

    score = match state.state_kind {
        PersistentStateKind::LowInformation | PersistentStateKind::Conflicted => {
            score.min(dec!(0.34))
        }
        PersistentStateKind::TurningPoint => score.max(dec!(0.20)).min(dec!(0.55)),
        PersistentStateKind::Latent => score.max(dec!(0.30)).min(dec!(0.68)),
        PersistentStateKind::Continuation => score.max(dec!(0.60)).min(dec!(0.99)),
    };

    score.max(Decimal::ZERO).min(Decimal::ONE).round_dp(4)
}

fn actionability_state_from_score(state_kind: PersistentStateKind, score: Decimal) -> &'static str {
    match state_kind {
        PersistentStateKind::LowInformation | PersistentStateKind::Conflicted => "do_not_trade",
        PersistentStateKind::TurningPoint => {
            if score >= dec!(0.45) {
                "observe_only"
            } else {
                "do_not_trade"
            }
        }
        PersistentStateKind::Latent => {
            if score >= dec!(0.55) {
                "observe_only"
            } else {
                "do_not_trade"
            }
        }
        PersistentStateKind::Continuation => {
            if score >= dec!(0.65) {
                "actionable"
            } else if score >= dec!(0.40) {
                "observe_only"
            } else {
                "do_not_trade"
            }
        }
    }
}

fn raw_support_fraction(case: &LiveTacticalCase) -> Decimal {
    case.raw_disagreement
        .as_ref()
        .map(|raw| raw.support_fraction)
        .unwrap_or(Decimal::ZERO)
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

fn transition_direction(transition: &AgentTransition) -> Option<&'static str> {
    action_direction_from_title_prefix(&transition.title).and_then(action_direction_case_label)
}

fn infer_direction_stability_rounds(
    current_tick: u64,
    case: &LiveTacticalCase,
    recent_transitions: &[AgentTransition],
) -> u16 {
    let Some(direction) = case_direction(case) else {
        return 0;
    };
    let mut recent = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(&case.symbol))
        .collect::<Vec<_>>();
    recent.sort_by(|a, b| b.to_tick.cmp(&a.to_tick));
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

/// Explicit raw-persistence guardrail that replaces the opaque
/// `stale_symbol_confirmation` label for fresh enters. Checks three signals:
///
/// 1. `raw_disagreement.support_fraction >= 0.85` — current tick must have
///    super-majority raw agreement, not the 67% floor used by `enforce_*_cap`.
/// 2. `state_persistence_ticks >= 2` — the case has been in its current state
///    for at least two ticks (proxy for "raw was also supportive previously",
///    since we do not snapshot raw_disagreement history).
/// 3. No recent direction flip — the case has not reversed direction inside
///    the recent transitions window.
///
/// When any check fails and the case is still claiming `action="enter"`, it is
/// downgraded to `review` with `review_reason_code="raw_persistence_insufficient"`
/// and a human-readable policy_reason naming the failing clause. This gives
/// operators an explicit answer to "why is this enter not enterable?" instead
/// of the opaque stale_symbol_confirmation black box.
fn apply_raw_persistence_gate(
    case: &mut LiveTacticalCase,
    state_persistence_ticks: u16,
    recent_flip: bool,
) {
    if case.action != "enter" {
        return;
    }
    let support_fraction = case
        .raw_disagreement
        .as_ref()
        .map(|raw| raw.support_fraction)
        .unwrap_or_default();
    let support_ok = support_fraction >= rust_decimal_macros::dec!(0.85);
    let persistence_ok = state_persistence_ticks >= 2;
    let direction_ok = !recent_flip;
    if support_ok && persistence_ok && direction_ok {
        return;
    }
    let reason = if !support_ok {
        format!(
            "raw support_fraction {} below 0.85 persistence floor",
            support_fraction.round_dp(3)
        )
    } else if !persistence_ok {
        format!(
            "state persisted only {} tick(s), need >= 2 consecutive ticks",
            state_persistence_ticks
        )
    } else {
        "recent direction flip detected — wait for stability".into()
    };
    let original_action = case.action.clone();
    let original_confidence = case.confidence;
    case.action = "review".into();
    case.review_reason_code = Some("raw_persistence_insufficient".into());
    case.policy_primary = Some("raw_persistence_gate".into());
    case.policy_reason = Some(reason);
    if !case
        .state_reason_codes
        .iter()
        .any(|c| c == "raw_persistence_insufficient")
    {
        case.state_reason_codes
            .push("raw_persistence_insufficient".into());
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
    }
}

fn has_recent_direction_flip(
    case: &LiveTacticalCase,
    recent_transitions: &[AgentTransition],
) -> bool {
    let mut recent = recent_transitions
        .iter()
        .filter(|transition| transition.symbol.eq_ignore_ascii_case(&case.symbol))
        .filter_map(|transition| transition_direction(transition))
        .collect::<Vec<_>>();
    recent.truncate(4);
    recent.truncate(2);
    recent.len() >= 2 && recent[0] != recent[1]
}

fn market_label(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "HK",
        LiveMarket::Us => "US",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentTransition;
    use crate::live_snapshot::{LiveRawDisagreement, LiveTacticalCase};
    use rust_decimal_macros::dec;

    fn case(symbol: &str, title: &str) -> LiveTacticalCase {
        LiveTacticalCase {
            setup_id: format!("setup:{symbol}"),
            symbol: symbol.into(),
            title: title.into(),
            action: "enter".into(),
            confidence: dec!(0.9),
            confidence_gap: dec!(0.1),
            heuristic_edge: dec!(0.1),
            entry_rationale: "test".into(),
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
            freshness_state: Some("fresh".into()),
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: Some("timely".into()),
            timing_position_in_range: Some(dec!(0.5)),
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
                contradict_count: 0,
                count_support_fraction: dec!(1.0),
                support_fraction: dec!(1.0),
                support_weight: dec!(2.6),
                contradict_weight: dec!(0.0),
                adjusted_action: "enter".into(),
                adjusted_confidence: dec!(0.9),
                summary: "test".into(),
                supporting_sources: vec![],
                contradicting_sources: vec![],
                original_action: None,
                original_confidence: None,
            }),
        }
    }

    fn transition(
        symbol: &str,
        title: &str,
        from_tick: u64,
        to_tick: u64,
        to_state: &str,
    ) -> AgentTransition {
        AgentTransition {
            from_tick,
            to_tick,
            symbol: symbol.into(),
            sector: None,
            setup_id: Some(format!("setup:{symbol}")),
            title: title.into(),
            from_state: Some("review:stable".into()),
            to_state: to_state.into(),
            confidence: dec!(0.8),
            summary: "test".into(),
            transition_reason: None,
        }
    }

    #[test]
    fn high_quality_case_becomes_continuation() {
        let mut cases = vec![case("6869.HK", "Short 6869.HK")];
        let transitions = vec![
            transition("6869.HK", "Short 6869.HK", 95, 96, "review:strengthening"),
            transition("6869.HK", "Short 6869.HK", 97, 98, "enter:strengthening"),
            transition("6869.HK", "Short 6869.HK", 98, 99, "enter:stable"),
        ];
        let signals = vec![LiveSignal {
            symbol: "6869.HK".into(),
            sector: Some("Technology".into()),
            composite: dec!(-0.8),
            mark_price: None,
            dimension_composite: None,
            capital_flow_direction: dec!(-0.8),
            price_momentum: dec!(-0.6),
            volume_profile: dec!(0.4),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        }];
        let artifacts = apply_perception_layer(
            101,
            LiveMarket::Hk,
            "2026-04-15T02:00:00Z",
            &mut cases,
            &transitions,
            &signals,
            &[],
            &[],
            None,
        );
        // Y#3 absence demotion: a high-quality case with no peer
        // corroboration is demoted from Continuation to Latent. The
        // cluster-level classification still reflects low aggregate
        // information because the isolated single-symbol signal can't
        // sustain a cluster-level continuation read on its own.
        assert_eq!(cases[0].local_state.as_deref(), Some("latent"));
        // Latent without peer corroboration downgrades actionability from
        // "actionable" to "observe_only" — the operator can see it but
        // should not size up on an isolated signal.
        assert_eq!(
            cases[0].actionability_state.as_deref(),
            Some("observe_only")
        );
        assert_eq!(artifacts.cluster_states[0].state, "low_information");
    }

    #[test]
    fn late_signal_becomes_turning_point() {
        let mut cases = vec![case("3750.HK", "Short 3750.HK")];
        cases[0].timing_state = Some("late_chase".into());
        let artifacts = apply_perception_layer(
            100,
            LiveMarket::Hk,
            "2026-04-15T02:00:00Z",
            &mut cases,
            &[],
            &[],
            &[],
            &[],
            None,
        );
        assert_eq!(cases[0].local_state.as_deref(), Some("turning_point"));
        assert_eq!(
            cases[0].actionability_state.as_deref(),
            Some("observe_only")
        );
        assert!(artifacts.world_summary.is_some());
    }

    #[test]
    fn small_sample_case_without_peers_stays_at_latent() {
        // Post-Y#3: a small-sample single-symbol case (5 supports,
        // fraction 1.0) has raw signal present but no peer corroboration,
        // so it's Latent rather than LowInformation. LowInformation now
        // requires BOTH raw absence and peer absence — partial evidence
        // survives as Latent so the operator still sees the read.
        let mut cases = vec![case("116.HK", "Short 116.HK")];
        if let Some(raw) = cases[0].raw_disagreement.as_mut() {
            raw.support_count = 5;
            raw.support_fraction = dec!(1.0);
        }
        apply_perception_layer(
            100,
            LiveMarket::Hk,
            "2026-04-15T02:00:00Z",
            &mut cases,
            &[],
            &[],
            &[],
            &[],
            None,
        );
        assert_eq!(cases[0].local_state.as_deref(), Some("latent"));
    }

    #[test]
    fn stability_defaults_to_one_without_history() {
        let current = case("2488.HK", "Long 2488.HK");
        assert_eq!(infer_direction_stability_rounds(100, &current, &[]), 1);
    }

    #[test]
    fn consecutive_same_direction_transitions_extend_stability_streak() {
        let current = case("6869.HK", "Short 6869.HK");
        let transitions = vec![
            transition("6869.HK", "Short 6869.HK", 95, 96, "review:strengthening"),
            transition("6869.HK", "Short 6869.HK", 97, 98, "enter:strengthening"),
            transition("6869.HK", "Short 6869.HK", 99, 100, "enter:stable"),
        ];
        assert_eq!(
            infer_direction_stability_rounds(101, &current, &transitions),
            7
        );
        assert!(!has_recent_direction_flip(&current, &transitions));
    }
}
