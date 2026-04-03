use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::reasoning::TacticalSetup;
use crate::ontology::world::Vortex;
use crate::temporal::buffer::TickHistory;
use crate::temporal::record::TickRecord;

use super::CaseRealizedOutcome;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexOutcomeFingerprint {
    pub setup_id: String,
    pub family: String,
    pub symbol: Option<String>,
    pub entry_tick: u64,
    pub resolved_tick: u64,
    pub center_scope: String,
    pub center_kind: String,
    pub role: String,
    pub channel_signature: String,
    pub dominant_channels: Vec<String>,
    pub channel_diversity: usize,
    pub path_count: usize,
    pub strength: Decimal,
    pub coherence: Decimal,
    pub net_return: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VortexSuccessPattern {
    pub center_kind: String,
    pub role: String,
    pub channel_signature: String,
    pub dominant_channels: Vec<String>,
    pub top_family: String,
    pub samples: usize,
    pub mean_net_return: Decimal,
    pub mean_strength: Decimal,
    pub mean_coherence: Decimal,
    pub mean_channel_diversity: Decimal,
}

pub fn compute_vortex_successful_fingerprints(
    history: &TickHistory,
    limit: usize,
) -> Vec<VortexOutcomeFingerprint> {
    let window = history.latest_n(limit);
    if window.is_empty() {
        return Vec::new();
    }

    let by_tick = window
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect::<HashMap<_, _>>();

    let mut fingerprints = super::compute_case_realized_outcomes_adaptive(history, limit)
        .into_iter()
        .filter(is_successful_outcome)
        .filter_map(|outcome| {
            let entry_record = by_tick.get(&outcome.entry_tick).copied()?;
            let setup = entry_record
                .tactical_setups
                .iter()
                .find(|setup| setup.setup_id == outcome.setup_id)?;
            let (vortex, role) = match_setup_vortex(setup, entry_record)?;
            let dominant_channels = dominant_channels(vortex);
            Some(VortexOutcomeFingerprint {
                setup_id: outcome.setup_id,
                family: outcome.family,
                symbol: outcome.symbol,
                entry_tick: outcome.entry_tick,
                resolved_tick: outcome.resolved_tick,
                center_scope: vortex.center_scope.label(),
                center_kind: vortex.center_scope.kind_slug().into(),
                role: role.into(),
                channel_signature: channel_signature(&dominant_channels),
                dominant_channels,
                channel_diversity: vortex.channel_diversity,
                path_count: vortex.flow_paths.len(),
                strength: vortex.strength,
                coherence: vortex.coherence,
                net_return: outcome.net_return,
            })
        })
        .collect::<Vec<_>>();

    fingerprints.sort_by(|left, right| {
        right
            .resolved_tick
            .cmp(&left.resolved_tick)
            .then_with(|| right.net_return.cmp(&left.net_return))
            .then_with(|| left.setup_id.cmp(&right.setup_id))
    });
    fingerprints
}

pub fn compute_vortex_success_patterns(
    history: &TickHistory,
    limit: usize,
) -> Vec<VortexSuccessPattern> {
    #[derive(Default)]
    struct PatternAccumulator {
        samples: usize,
        sum_net_return: Decimal,
        sum_strength: Decimal,
        sum_coherence: Decimal,
        sum_channel_diversity: Decimal,
        family_counts: HashMap<String, usize>,
        dominant_channels: Vec<String>,
    }

    let mut acc = HashMap::<(String, String, String), PatternAccumulator>::new();

    for fingerprint in compute_vortex_successful_fingerprints(history, limit) {
        let key = (
            fingerprint.center_kind.clone(),
            fingerprint.role.clone(),
            fingerprint.channel_signature.clone(),
        );
        let entry = acc.entry(key).or_default();
        entry.samples += 1;
        entry.sum_net_return += fingerprint.net_return;
        entry.sum_strength += fingerprint.strength;
        entry.sum_coherence += fingerprint.coherence;
        entry.sum_channel_diversity += Decimal::from(fingerprint.channel_diversity as i64);
        entry.dominant_channels = fingerprint.dominant_channels.clone();
        *entry
            .family_counts
            .entry(fingerprint.family.clone())
            .or_insert(0) += 1;
    }

    let mut patterns = acc
        .into_iter()
        .map(
            |((center_kind, role, channel_signature), accumulator)| VortexSuccessPattern {
                center_kind,
                role,
                channel_signature,
                dominant_channels: accumulator.dominant_channels,
                top_family: accumulator
                    .family_counts
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(family, _)| family)
                    .unwrap_or_else(|| "Unknown".into()),
                samples: accumulator.samples,
                mean_net_return: mean(accumulator.sum_net_return, accumulator.samples),
                mean_strength: mean(accumulator.sum_strength, accumulator.samples),
                mean_coherence: mean(accumulator.sum_coherence, accumulator.samples),
                mean_channel_diversity: mean(
                    accumulator.sum_channel_diversity,
                    accumulator.samples,
                ),
            },
        )
        .collect::<Vec<_>>();

    patterns.sort_by(|left, right| {
        right
            .mean_net_return
            .cmp(&left.mean_net_return)
            .then_with(|| right.samples.cmp(&left.samples))
            .then_with(|| right.mean_strength.cmp(&left.mean_strength))
            .then_with(|| left.channel_signature.cmp(&right.channel_signature))
    });
    patterns
}

pub fn vortex_matches_success_pattern(vortex: &Vortex, pattern: &VortexSuccessPattern) -> bool {
    if vortex.center_scope.kind_slug() != pattern.center_kind {
        return false;
    }

    let current_channels = dominant_channels(vortex);
    if current_channels.is_empty() || pattern.dominant_channels.is_empty() {
        return false;
    }

    let overlap = current_channels
        .iter()
        .filter(|channel| pattern.dominant_channels.contains(channel))
        .count();
    let required_overlap = current_channels
        .len()
        .min(pattern.dominant_channels.len())
        .min(2)
        .max(1);

    overlap >= required_overlap
}

fn is_successful_outcome(outcome: &CaseRealizedOutcome) -> bool {
    outcome.net_return > Decimal::ZERO && outcome.followed_through && outcome.structure_retained
}

fn match_setup_vortex<'a>(
    setup: &TacticalSetup,
    entry_record: &'a TickRecord,
) -> Option<(&'a Vortex, &'static str)> {
    let mut best: Option<(&Vortex, &'static str, usize)> = None;

    for vortex in &entry_record.world_state.vortices {
        let candidate = if setup.scope == vortex.center_scope {
            Some((vortex, "center", 2usize))
        } else if vortex
            .flow_paths
            .iter()
            .any(|path| path.source_scope == setup.scope)
        {
            Some((vortex, "edge", 1usize))
        } else {
            None
        };

        if let Some((vortex, role, rank)) = candidate {
            let replace = best
                .as_ref()
                .map(|(current, _, current_rank)| {
                    rank > *current_rank
                        || (rank == *current_rank
                            && (vortex.strength > current.strength
                                || (vortex.strength == current.strength
                                    && vortex.coherence > current.coherence)))
                })
                .unwrap_or(true);
            if replace {
                best = Some((vortex, role, rank));
            }
        }
    }

    best.map(|(vortex, role, _)| (vortex, role))
}

fn dominant_channels(vortex: &Vortex) -> Vec<String> {
    let mut totals = HashMap::<String, Decimal>::new();
    for path in &vortex.flow_paths {
        totals
            .entry(path.channel.clone())
            .and_modify(|value| *value += path.weight.abs())
            .or_insert(path.weight.abs());
    }

    let mut ranked = totals.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked
        .into_iter()
        .take(3)
        .map(|(channel, _)| channel)
        .collect()
}

fn channel_signature(channels: &[String]) -> String {
    channels.join("|")
}

fn mean(total: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        total / Decimal::from(count as i64)
    }
}

// ---------------------------------------------------------------------------
// Candidate Mechanism promotion
// ---------------------------------------------------------------------------

use crate::persistence::candidate_mechanism::CandidateMechanismRecord;

/// Minimum samples a success pattern needs before it can become a candidate mechanism.
const PROMOTION_MIN_SAMPLES: usize = 3;

/// Evaluate current success patterns against existing candidate mechanisms.
/// Returns new or updated mechanism records ready for persistence.
pub fn evaluate_candidate_mechanisms(
    patterns: &[VortexSuccessPattern],
    existing: &[CandidateMechanismRecord],
    market: &str,
    current_tick: u64,
    now_rfc3339: &str,
) -> Vec<CandidateMechanismRecord> {
    let mut results = Vec::new();
    let existing_by_id: std::collections::HashMap<&str, &CandidateMechanismRecord> = existing
        .iter()
        .map(|mech| (mech.mechanism_id.as_str(), mech))
        .collect();

    for pattern in patterns {
        let mech_id = CandidateMechanismRecord::mechanism_key(
            market,
            &pattern.center_kind,
            &pattern.role,
            &pattern.channel_signature,
        );

        let promotion_min_return = Decimal::new(1, 2); // 0.01
        if pattern.samples < PROMOTION_MIN_SAMPLES || pattern.mean_net_return < promotion_min_return
        {
            // Not ready for promotion; if already exists, update last_seen but don't create
            if let Some(existing_mech) = existing_by_id.get(mech_id.as_str()) {
                let mut updated = (*existing_mech).clone();
                updated.last_seen_tick = current_tick;
                updated.samples = pattern.samples as u64;
                updated.mean_net_return = pattern.mean_net_return;
                updated.mean_strength = pattern.mean_strength;
                updated.mean_coherence = pattern.mean_coherence;
                updated.mean_channel_diversity = pattern.mean_channel_diversity;
                updated.updated_at = now_rfc3339.to_string();
                results.push(updated);
            }
            continue;
        }

        if let Some(existing_mech) = existing_by_id.get(mech_id.as_str()) {
            // Update existing mechanism with fresh statistics
            let mut updated = (*existing_mech).clone();
            updated.last_seen_tick = current_tick;
            updated.samples = pattern.samples as u64;
            updated.mean_net_return = pattern.mean_net_return;
            updated.mean_strength = pattern.mean_strength;
            updated.mean_coherence = pattern.mean_coherence;
            updated.mean_channel_diversity = pattern.mean_channel_diversity;
            updated.dominant_channels = pattern.dominant_channels.clone();
            updated.top_family = pattern.top_family.clone();
            updated.updated_at = now_rfc3339.to_string();

            // Lifecycle transitions
            if updated.should_promote_to_live() {
                updated.mode = "live".into();
            } else if updated.should_promote_to_assist() {
                updated.mode = "assist".into();
            }
            if updated.should_decay(current_tick) {
                if let Some(demoted) = updated.demoted_mode() {
                    updated.mode = demoted.into();
                    updated.consecutive_misses = 0;
                }
                // If already shadow and should_decay, we still keep it but it stays shadow.
                // The runtime can choose to prune mechanisms in shadow mode that keep decaying.
            }
            results.push(updated);
        } else {
            // Create new candidate mechanism in shadow mode
            results.push(CandidateMechanismRecord {
                mechanism_id: mech_id,
                market: market.to_string(),
                center_kind: pattern.center_kind.clone(),
                role: pattern.role.clone(),
                channel_signature: pattern.channel_signature.clone(),
                dominant_channels: pattern.dominant_channels.clone(),
                top_family: pattern.top_family.clone(),
                samples: pattern.samples as u64,
                mean_net_return: pattern.mean_net_return,
                mean_strength: pattern.mean_strength,
                mean_coherence: pattern.mean_coherence,
                mean_channel_diversity: pattern.mean_channel_diversity,
                mode: "shadow".into(),
                promoted_at_tick: current_tick,
                last_seen_tick: current_tick,
                last_hit_tick: None,
                consecutive_misses: 0,
                post_promotion_hits: 0,
                post_promotion_misses: 0,
                post_promotion_net_return: Decimal::ZERO,
                created_at: now_rfc3339.to_string(),
                updated_at: now_rfc3339.to_string(),
            });
        }
    }

    // Carry forward existing mechanisms not seen in current patterns
    for existing_mech in existing {
        let already_handled = results
            .iter()
            .any(|mech| mech.mechanism_id == existing_mech.mechanism_id);
        if !already_handled {
            let mut updated = existing_mech.clone();
            updated.consecutive_misses += 1;
            updated.updated_at = now_rfc3339.to_string();
            if updated.should_decay(current_tick) {
                if let Some(demoted) = updated.demoted_mode() {
                    updated.mode = demoted.into();
                    updated.consecutive_misses = 0;
                }
            }
            results.push(updated);
        }
    }

    results
}

/// Record a hit or miss for a candidate mechanism after an outcome resolves.
pub fn score_candidate_mechanism(
    mech: &mut CandidateMechanismRecord,
    hit: bool,
    net_return: Decimal,
    tick: u64,
    now_rfc3339: &str,
) {
    if hit {
        mech.post_promotion_hits += 1;
        mech.last_hit_tick = Some(tick);
        mech.consecutive_misses = 0;
    } else {
        mech.post_promotion_misses += 1;
        mech.consecutive_misses += 1;
    }
    mech.post_promotion_net_return += net_return;
    mech.last_seen_tick = tick;
    mech.updated_at = now_rfc3339.to_string();
}

/// Filter mechanisms that are in "live" mode and can act as hypothesis templates.
pub fn live_candidate_mechanisms(
    mechanisms: &[CandidateMechanismRecord],
) -> Vec<&CandidateMechanismRecord> {
    mechanisms
        .iter()
        .filter(|mech| mech.mode == "live")
        .collect()
}

/// Filter mechanisms that are in "assist" or "live" mode (can influence attention/confidence).
pub fn active_candidate_mechanisms(
    mechanisms: &[CandidateMechanismRecord],
) -> Vec<&CandidateMechanismRecord> {
    mechanisms
        .iter()
        .filter(|mech| mech.mode == "assist" || mech.mode == "live")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::reasoning::{
        DecisionLineage, Hypothesis, PropagationPath, ReasoningScope, TacticalSetup,
    };
    use crate::ontology::world::{
        BackwardReasoningSnapshot, EntityState, FlowPath, FlowPolarity, WorldLayer,
        WorldStateSnapshot,
    };
    use crate::temporal::buffer::TickHistory;
    use crate::temporal::record::{SymbolSignals, TickRecord};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn prov(tag: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(tag)
            .with_inputs([tag.to_string()])
    }

    fn signal(mark_price: Decimal, composite: Decimal) -> SymbolSignals {
        SymbolSignals {
            mark_price: Some(mark_price),
            composite,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: None,
            convergence_score: Some(dec!(0.5)),
            composite_degradation: None,
            institution_retention: None,
            edge_stability: None,
            temporal_weight: None,
            microstructure_confirmation: None,
            component_spread: None,
            institutional_edge_age: None,
        }
    }

    fn make_tick(tick_number: u64, price: Decimal, include_setup: bool) -> TickRecord {
        let symbol = crate::ontology::objects::Symbol("700.HK".into());
        let peer = crate::ontology::objects::Symbol("9988.HK".into());
        let mut signals = HashMap::new();
        signals.insert(symbol.clone(), signal(price, dec!(0.6)));
        signals.insert(peer.clone(), signal(dec!(100), dec!(0.3)));
        let setup = TacticalSetup {
            setup_id: "setup:700.HK".into(),
            hypothesis_id: "hyp:700.HK:convergence_hypothesis".into(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:700.HK"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(symbol.clone()),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.72),
            confidence_gap: dec!(0.20),
            heuristic_edge: dec!(0.12),
            convergence_score: Some(dec!(0.55)),
            convergence_detail: None,
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "vortex".into(),
            causal_narrative: None,
            risk_notes: vec!["family=Convergence Hypothesis".into()],
            review_reason_code: None,
            policy_verdict: None,
        };
        let hypothesis = Hypothesis {
            hypothesis_id: setup.hypothesis_id.clone(),
            family_key: "convergence_hypothesis".into(),
            family_label: "Convergence Hypothesis".into(),
            provenance: prov("hyp:700.HK"),
            scope: setup.scope.clone(),
            statement: "700.HK shows an emergent convergence vortex".into(),
            confidence: dec!(0.72),
            local_support_weight: dec!(0.5),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.3),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        };

        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick_number as i64),
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: if include_setup {
                vec![hypothesis]
            } else {
                vec![]
            },
            propagation_paths: vec![PropagationPath {
                path_id: "path:setup".into(),
                summary: "propagation".into(),
                confidence: dec!(0.4),
                steps: vec![],
            }],
            tactical_setups: if include_setup { vec![setup] } else { vec![] },
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![EntityState {
                    entity_id: "world:market".into(),
                    scope: ReasoningScope::market(),
                    layer: WorldLayer::Forest,
                    provenance: prov("world:market"),
                    label: "Market canopy".into(),
                    regime: "risk_on".into(),
                    confidence: dec!(0.7),
                    local_support: dec!(0.6),
                    propagated_support: dec!(0.3),
                    drivers: vec![],
                }],
                vortices: if include_setup {
                    vec![crate::ontology::world::Vortex {
                        vortex_id: "vortex:700.HK".into(),
                        center_entity_id: "world:setup:700.HK".into(),
                        center_scope: ReasoningScope::Symbol(symbol),
                        layer: WorldLayer::Leaf,
                        flow_paths: vec![
                            FlowPath {
                                source_entity_id: "world:setup:9988.HK".into(),
                                source_scope: ReasoningScope::Symbol(peer),
                                channel: "broker_flow".into(),
                                weight: dec!(0.4),
                                polarity: FlowPolarity::Confirming,
                            },
                            FlowPath {
                                source_entity_id: "world:market".into(),
                                source_scope: ReasoningScope::market(),
                                channel: "catalyst".into(),
                                weight: dec!(0.3),
                                polarity: FlowPolarity::Confirming,
                            },
                            FlowPath {
                                source_entity_id: "world:rotation".into(),
                                source_scope: ReasoningScope::Custom("rotation".into()),
                                channel: "propagation".into(),
                                weight: dec!(0.2),
                                polarity: FlowPolarity::Confirming,
                            },
                        ],
                        strength: dec!(0.48),
                        channel_diversity: 3,
                        coherence: dec!(0.75),
                        narrative: None,
                    }]
                } else {
                    vec![]
                },
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        }
    }

    #[test]
    fn computes_vortex_success_pattern_from_profitable_outcome() {
        let mut history = TickHistory::new(32);
        history.push(make_tick(1, dec!(100), true));
        for tick in 2..=12 {
            history.push(make_tick(
                tick,
                dec!(100) + Decimal::from(tick as i64),
                false,
            ));
        }

        let fingerprints = compute_vortex_successful_fingerprints(&history, 32);
        assert_eq!(fingerprints.len(), 1);
        assert_eq!(fingerprints[0].role, "center");
        assert_eq!(
            fingerprints[0].channel_signature,
            "broker_flow|catalyst|propagation"
        );

        let patterns = compute_vortex_success_patterns(&history, 32);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].top_family, "Convergence Hypothesis");
        assert_eq!(patterns[0].samples, 1);
        assert!(patterns[0].mean_net_return > Decimal::ZERO);
    }

    #[test]
    fn matches_success_pattern_by_center_kind_and_channel_overlap() {
        let pattern = VortexSuccessPattern {
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "broker_flow|catalyst|propagation".into(),
            dominant_channels: vec![
                "broker_flow".into(),
                "catalyst".into(),
                "propagation".into(),
            ],
            top_family: "Convergence Hypothesis".into(),
            samples: 2,
            mean_net_return: dec!(0.04),
            mean_strength: dec!(0.45),
            mean_coherence: dec!(0.70),
            mean_channel_diversity: dec!(3),
        };
        let vortex = crate::ontology::world::Vortex {
            vortex_id: "vortex:test".into(),
            center_entity_id: "world:setup:700.HK".into(),
            center_scope: ReasoningScope::Symbol(crate::ontology::objects::Symbol("700.HK".into())),
            layer: WorldLayer::Leaf,
            flow_paths: vec![
                FlowPath {
                    source_entity_id: "a".into(),
                    source_scope: ReasoningScope::market(),
                    channel: "broker_flow".into(),
                    weight: dec!(0.3),
                    polarity: FlowPolarity::Confirming,
                },
                FlowPath {
                    source_entity_id: "b".into(),
                    source_scope: ReasoningScope::market(),
                    channel: "propagation".into(),
                    weight: dec!(0.2),
                    polarity: FlowPolarity::Confirming,
                },
            ],
            strength: dec!(0.28),
            channel_diversity: 2,
            coherence: dec!(0.55),
            narrative: None,
        };

        assert!(vortex_matches_success_pattern(&vortex, &pattern));
    }

    #[test]
    fn evaluate_candidate_mechanisms_creates_new_shadow_mechanism() {
        let pattern = VortexSuccessPattern {
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "broker_flow|catalyst|propagation".into(),
            dominant_channels: vec![
                "broker_flow".into(),
                "catalyst".into(),
                "propagation".into(),
            ],
            top_family: "Convergence Hypothesis".into(),
            samples: 5,
            mean_net_return: dec!(0.03),
            mean_strength: dec!(0.5),
            mean_coherence: dec!(0.7),
            mean_channel_diversity: dec!(3.0),
        };

        let results =
            evaluate_candidate_mechanisms(&[pattern], &[], "hk", 100, "2026-04-01T00:00:00Z");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mode, "shadow");
        assert_eq!(results[0].market, "hk");
        assert_eq!(results[0].samples, 5);
        assert_eq!(results[0].promoted_at_tick, 100);
    }

    #[test]
    fn evaluate_candidate_mechanisms_skips_low_sample_pattern() {
        let pattern = VortexSuccessPattern {
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "broker_flow".into(),
            dominant_channels: vec!["broker_flow".into()],
            top_family: "Directed Flow".into(),
            samples: 1, // Below threshold
            mean_net_return: dec!(0.05),
            ..VortexSuccessPattern::default()
        };

        let results =
            evaluate_candidate_mechanisms(&[pattern], &[], "hk", 100, "2026-04-01T00:00:00Z");
        assert!(results.is_empty());
    }

    #[test]
    fn candidate_mechanism_lifecycle_shadow_to_assist() {
        let mut mech = CandidateMechanismRecord {
            mechanism_id: "mech:hk:symbol:center:test".into(),
            market: "hk".into(),
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "test".into(),
            dominant_channels: vec!["test".into()],
            top_family: "Test".into(),
            samples: 10,
            mean_net_return: dec!(0.05),
            mean_strength: dec!(0.5),
            mean_coherence: dec!(0.7),
            mean_channel_diversity: dec!(2.0),
            mode: "shadow".into(),
            promoted_at_tick: 1,
            last_seen_tick: 50,
            last_hit_tick: Some(48),
            consecutive_misses: 0,
            post_promotion_hits: 4,
            post_promotion_misses: 1,
            post_promotion_net_return: dec!(0.15),
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        };

        // 5 evaluations, 80% hit rate, positive return → should promote
        assert!(mech.should_promote_to_assist());
        mech.mode = "assist".into();

        // Not enough for live yet (need 12)
        assert!(!mech.should_promote_to_live());

        // Add more evaluations
        mech.post_promotion_hits = 8;
        mech.post_promotion_misses = 4;
        assert!(mech.should_promote_to_live());
    }

    #[test]
    fn candidate_mechanism_decay_on_consecutive_misses() {
        let mech = CandidateMechanismRecord {
            mechanism_id: "mech:hk:symbol:center:decay".into(),
            market: "hk".into(),
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "decay".into(),
            dominant_channels: vec![],
            top_family: "Test".into(),
            samples: 10,
            mean_net_return: dec!(0.02),
            mean_strength: dec!(0.3),
            mean_coherence: dec!(0.4),
            mean_channel_diversity: dec!(1.0),
            mode: "live".into(),
            promoted_at_tick: 1,
            last_seen_tick: 50,
            last_hit_tick: Some(30),
            consecutive_misses: 8,
            post_promotion_hits: 3,
            post_promotion_misses: 8,
            post_promotion_net_return: dec!(-0.02),
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        };

        assert!(mech.should_decay(60));
        assert_eq!(mech.demoted_mode(), Some("assist"));
    }

    #[test]
    fn evaluate_carries_forward_unseen_mechanisms() {
        let existing = CandidateMechanismRecord {
            mechanism_id: "mech:hk:symbol:center:old".into(),
            market: "hk".into(),
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "old".into(),
            dominant_channels: vec!["old".into()],
            top_family: "Old".into(),
            samples: 5,
            mean_net_return: dec!(0.03),
            mean_strength: dec!(0.4),
            mean_coherence: dec!(0.5),
            mean_channel_diversity: dec!(2.0),
            mode: "shadow".into(),
            promoted_at_tick: 10,
            last_seen_tick: 80,
            last_hit_tick: None,
            consecutive_misses: 2,
            post_promotion_hits: 0,
            post_promotion_misses: 0,
            post_promotion_net_return: Decimal::ZERO,
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        };

        // No matching patterns → existing mechanism carried forward with bumped misses
        let results =
            evaluate_candidate_mechanisms(&[], &[existing], "hk", 100, "2026-04-01T01:00:00Z");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].consecutive_misses, 3);
    }
}
