//! Causal schema extraction from candidate mechanisms + tick history.
//!
//! A causal schema goes beyond "these channels co-occur in successful cases" to
//! capture:
//! - **Channel ordering**: which channel appears first (temporal precedence)
//! - **Preconditions**: regime, session, coherence, contest state at entry
//! - **Invalidation rules**: structural conditions that predict failure
//! - **Transferability**: evidence of cross-symbol/session/regime success

use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;

use crate::ontology::world::{FlowPolarity, Vortex};
use crate::persistence::candidate_mechanism::CandidateMechanismRecord;
use crate::persistence::causal_schema::{
    CausalSchemaRecord, RegimeAffinity, SchemaInvalidationRule, SessionAffinity,
};
use crate::temporal::buffer::TickHistory;
use crate::temporal::record::TickRecord;

use super::{CaseRealizedOutcome, VortexOutcomeFingerprint};

/// Extract a causal schema from a candidate mechanism using tick history.
///
/// Returns None if there isn't enough data to extract meaningful causal structure.
pub fn extract_causal_schema(
    mechanism: &CandidateMechanismRecord,
    fingerprints: &[VortexOutcomeFingerprint],
    all_outcomes: &[CaseRealizedOutcome],
    history: &TickHistory,
    current_tick: u64,
    now_rfc3339: &str,
) -> Option<CausalSchemaRecord> {
    // Filter fingerprints belonging to this mechanism
    let mechanism_fingerprints: Vec<&VortexOutcomeFingerprint> = fingerprints
        .iter()
        .filter(|fp| {
            fp.center_kind == mechanism.center_kind
                && fp.role == mechanism.role
                && fp.channel_signature == mechanism.channel_signature
        })
        .collect();

    if mechanism_fingerprints.len() < 2 {
        return None;
    }

    let by_tick: HashMap<u64, &TickRecord> = history
        .latest_n(history.len())
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect();

    // --- 1. Extract channel causal ordering ---
    let channel_chain = extract_channel_ordering(&mechanism_fingerprints, &by_tick);

    // --- 2. Extract preconditions from successful outcomes ---
    let successful_outcome_ids: HashSet<&str> = mechanism_fingerprints
        .iter()
        .map(|fp| fp.setup_id.as_str())
        .collect();
    let successful_outcomes: Vec<&CaseRealizedOutcome> = all_outcomes
        .iter()
        .filter(|outcome| successful_outcome_ids.contains(outcome.setup_id.as_str()))
        .collect();

    // Failed outcomes for the same channel pattern (for invalidation extraction)
    let all_pattern_outcomes: Vec<&CaseRealizedOutcome> = all_outcomes
        .iter()
        .filter(|outcome| {
            // Match by family if available
            mechanism
                .dominant_channels
                .iter()
                .any(|ch| outcome.family.to_lowercase().contains(&ch.to_lowercase()))
                || outcome.family == mechanism.top_family
        })
        .collect();
    let failed_outcomes: Vec<&CaseRealizedOutcome> = all_pattern_outcomes
        .iter()
        .copied()
        .filter(|outcome| outcome.net_return <= Decimal::ZERO || !outcome.followed_through)
        .collect();

    let regime_affinity = extract_regime_affinity(&successful_outcomes, &failed_outcomes);
    let session_affinity = extract_session_affinity(&successful_outcomes, &failed_outcomes);
    let (min_coherence, min_strength) =
        extract_structural_thresholds(&mechanism_fingerprints);
    let min_convergence_score = successful_outcomes
        .iter()
        .map(|o| o.convergence_score)
        .min()
        .unwrap_or(Decimal::ZERO);
    let preferred_contest_states =
        extract_contest_state_preference(&mechanism_fingerprints, &by_tick);

    // --- 3. Extract invalidation rules ---
    let invalidation_rules =
        extract_invalidation_rules(&failed_outcomes, &mechanism_fingerprints, &by_tick);

    // --- 4. Compute transferability ---
    let observed_symbols: Vec<String> = mechanism_fingerprints
        .iter()
        .filter_map(|fp| fp.symbol.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let observed_sectors = extract_sectors_from_fingerprints(&mechanism_fingerprints, &by_tick);
    let cross_symbol_validated = observed_symbols.len() >= 2;
    let cross_session_validated = {
        let sessions: HashSet<&str> = successful_outcomes
            .iter()
            .map(|o| o.session.as_str())
            .collect();
        sessions.len() >= 2
    };
    let cross_regime_validated = {
        let regimes: HashSet<&str> = successful_outcomes
            .iter()
            .map(|o| o.market_regime.as_str())
            .collect();
        regimes.len() >= 2
    };

    // --- 5. Build causal narrative ---
    let causal_narrative = build_causal_narrative(&channel_chain, &mechanism);

    let schema_id = CausalSchemaRecord::schema_key(&mechanism.mechanism_id);

    Some(CausalSchemaRecord {
        schema_id,
        mechanism_id: mechanism.mechanism_id.clone(),
        market: mechanism.market.clone(),
        channel_chain,
        causal_narrative,
        regime_affinity,
        session_affinity,
        min_coherence,
        min_strength,
        min_convergence_score,
        preferred_contest_states,
        invalidation_rules,
        observed_symbols,
        observed_sectors,
        applicable_center_kinds: vec![mechanism.center_kind.clone()],
        cross_symbol_validated,
        cross_session_validated,
        cross_regime_validated,
        total_applications: mechanism.post_promotion_hits + mechanism.post_promotion_misses,
        successful_applications: mechanism.post_promotion_hits,
        failed_applications: mechanism.post_promotion_misses,
        mean_return_when_applied: mechanism.mean_net_return,
        mean_return_when_preconditions_met: Decimal::ZERO, // populated after runtime evaluation
        mean_return_when_preconditions_violated: Decimal::ZERO,
        status: "candidate".into(),
        promoted_at_tick: current_tick,
        last_applied_tick: current_tick,
        created_at: now_rfc3339.to_string(),
        updated_at: now_rfc3339.to_string(),
    })
}

/// Extract the temporal ordering of channels from successful vortex instances.
///
/// For each fingerprint, look at the entry tick's vortex flow paths and rank
/// channels by weight (higher weight = earlier causal role). The ordering
/// that appears most consistently across fingerprints becomes the chain.
fn extract_channel_ordering(
    fingerprints: &[&VortexOutcomeFingerprint],
    by_tick: &HashMap<u64, &TickRecord>,
) -> Vec<String> {
    let mut ordering_votes: HashMap<Vec<String>, usize> = HashMap::new();

    for fp in fingerprints {
        let Some(record) = by_tick.get(&fp.entry_tick) else {
            continue;
        };

        // Find the matching vortex
        let vortex = record
            .world_state
            .vortices
            .iter()
            .find(|v| v.center_scope.label() == fp.center_scope);

        if let Some(vortex) = vortex {
            let ordering = channel_ordering_from_vortex(vortex);
            *ordering_votes.entry(ordering).or_insert(0) += 1;
        }
    }

    // Pick the most common ordering
    ordering_votes
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(ordering, _)| ordering)
        .unwrap_or_default()
}

/// Order channels in a vortex by weight (descending = causal precedence).
/// Confirming channels come before contradicting ones.
fn channel_ordering_from_vortex(vortex: &Vortex) -> Vec<String> {
    let mut channels: Vec<(&String, Decimal, bool)> = vortex
        .flow_paths
        .iter()
        .map(|path| {
            let is_confirming = matches!(path.polarity, FlowPolarity::Confirming);
            (&path.channel, path.weight.abs(), is_confirming)
        })
        .collect();

    // Sort: confirming first, then by weight descending
    channels.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.0.cmp(b.0))
    });

    // Deduplicate channel names
    let mut seen = HashSet::new();
    channels
        .into_iter()
        .filter_map(|(ch, _, _)| {
            if seen.insert(ch.clone()) {
                Some(ch.clone())
            } else {
                None
            }
        })
        .collect()
}

/// Extract regime affinity from outcomes.
fn extract_regime_affinity(
    successes: &[&CaseRealizedOutcome],
    failures: &[&CaseRealizedOutcome],
) -> Vec<RegimeAffinity> {
    let mut regimes: HashMap<String, (u64, u64, Decimal)> = HashMap::new();

    for outcome in successes {
        let entry = regimes
            .entry(outcome.market_regime.clone())
            .or_insert((0, 0, Decimal::ZERO));
        entry.0 += 1;
        entry.2 += outcome.net_return;
    }
    for outcome in failures {
        let entry = regimes
            .entry(outcome.market_regime.clone())
            .or_insert((0, 0, Decimal::ZERO));
        entry.1 += 1;
        entry.2 += outcome.net_return;
    }

    regimes
        .into_iter()
        .map(|(regime, (hits, misses, total_return))| {
            let count = hits + misses;
            RegimeAffinity {
                regime,
                hit_count: hits,
                miss_count: misses,
                mean_return: if count > 0 {
                    total_return / Decimal::from(count)
                } else {
                    Decimal::ZERO
                },
            }
        })
        .collect()
}

/// Extract session affinity from outcomes.
fn extract_session_affinity(
    successes: &[&CaseRealizedOutcome],
    failures: &[&CaseRealizedOutcome],
) -> Vec<SessionAffinity> {
    let mut sessions: HashMap<String, (u64, u64, Decimal)> = HashMap::new();

    for outcome in successes {
        let entry = sessions
            .entry(outcome.session.clone())
            .or_insert((0, 0, Decimal::ZERO));
        entry.0 += 1;
        entry.2 += outcome.net_return;
    }
    for outcome in failures {
        let entry = sessions
            .entry(outcome.session.clone())
            .or_insert((0, 0, Decimal::ZERO));
        entry.1 += 1;
        entry.2 += outcome.net_return;
    }

    sessions
        .into_iter()
        .map(|(session, (hits, misses, total_return))| {
            let count = hits + misses;
            SessionAffinity {
                session,
                hit_count: hits,
                miss_count: misses,
                mean_return: if count > 0 {
                    total_return / Decimal::from(count)
                } else {
                    Decimal::ZERO
                },
            }
        })
        .collect()
}

/// Extract minimum coherence and strength thresholds from fingerprints.
fn extract_structural_thresholds(
    fingerprints: &[&VortexOutcomeFingerprint],
) -> (Decimal, Decimal) {
    let min_coherence = fingerprints
        .iter()
        .map(|fp| fp.coherence)
        .min()
        .unwrap_or(Decimal::ZERO);
    let min_strength = fingerprints
        .iter()
        .map(|fp| fp.strength)
        .min()
        .unwrap_or(Decimal::ZERO);
    (min_coherence, min_strength)
}

/// Extract preferred contest states from backward reasoning at entry tick.
fn extract_contest_state_preference(
    fingerprints: &[&VortexOutcomeFingerprint],
    by_tick: &HashMap<u64, &TickRecord>,
) -> Vec<String> {
    let mut state_counts: HashMap<String, usize> = HashMap::new();

    for fp in fingerprints {
        let Some(record) = by_tick.get(&fp.entry_tick) else {
            continue;
        };

        // Find matching backward investigation
        for investigation in &record.backward_reasoning.investigations {
            if investigation.leaf_scope.label() == fp.center_scope {
                let state = format!("{:?}", investigation.contest_state);
                *state_counts.entry(state).or_insert(0) += 1;
            }
        }
    }

    // Return states that appear in >30% of fingerprints
    let threshold = (fingerprints.len() as f64 * 0.3).ceil() as usize;
    let mut preferred: Vec<String> = state_counts
        .into_iter()
        .filter(|(_, count)| *count >= threshold.max(1))
        .map(|(state, _)| state)
        .collect();
    preferred.sort();
    preferred
}

/// Extract invalidation rules from failed outcomes and causal flip events.
fn extract_invalidation_rules(
    failed_outcomes: &[&CaseRealizedOutcome],
    _fingerprints: &[&VortexOutcomeFingerprint],
    by_tick: &HashMap<u64, &TickRecord>,
) -> Vec<SchemaInvalidationRule> {
    let mut rules = Vec::new();
    let failure_count = failed_outcomes.len().max(1) as u64;

    // Count how many failures had causal contest flips
    let flip_count = failed_outcomes
        .iter()
        .filter(|outcome| {
            // Check if there was a causal flip between entry and resolution
            (outcome.entry_tick..=outcome.resolved_tick).any(|tick| {
                by_tick.get(&tick).map_or(false, |record| {
                    record
                        .backward_reasoning
                        .investigations
                        .iter()
                        .any(|inv| {
                            matches!(
                                inv.contest_state,
                                crate::ontology::world::CausalContestState::Flipped
                                    | crate::ontology::world::CausalContestState::Contested
                            )
                        })
                })
            })
        })
        .count() as u64;

    if flip_count > 0 {
        rules.push(SchemaInvalidationRule {
            kind: "contest_flip".into(),
            description: "causal leadership flipped or became contested during the trade".into(),
            failure_correlation: Decimal::from(flip_count * 100) / Decimal::from(failure_count),
        });
    }

    // Count failures where structure wasn't retained
    let structure_loss_count = failed_outcomes
        .iter()
        .filter(|o| !o.structure_retained)
        .count() as u64;

    if structure_loss_count > 0 {
        rules.push(SchemaInvalidationRule {
            kind: "coherence_drop".into(),
            description: "vortex structure degraded — coherence dropped below entry threshold"
                .into(),
            failure_correlation: Decimal::from(structure_loss_count * 100)
                / Decimal::from(failure_count),
        });
    }

    // Count failures where hypothesis was invalidated
    let invalidation_count = failed_outcomes
        .iter()
        .filter(|o| o.invalidated)
        .count() as u64;

    if invalidation_count > 0 {
        rules.push(SchemaInvalidationRule {
            kind: "contradicting_dominance".into(),
            description: "contradicting evidence exceeded supporting evidence".into(),
            failure_correlation: Decimal::from(invalidation_count * 100)
                / Decimal::from(failure_count),
        });
    }

    // Channel absence: if any failure had convergence_score notably lower
    let low_convergence_failures = failed_outcomes
        .iter()
        .filter(|o| o.convergence_score < Decimal::new(3, 1)) // < 0.3
        .count() as u64;

    if low_convergence_failures > 0 {
        rules.push(SchemaInvalidationRule {
            kind: "channel_absence".into(),
            description: "weak convergence suggests missing channel contributions".into(),
            failure_correlation: Decimal::from(low_convergence_failures * 100)
                / Decimal::from(failure_count),
        });
    }

    rules
}

/// Extract sectors from fingerprints by looking at entity states.
fn extract_sectors_from_fingerprints(
    fingerprints: &[&VortexOutcomeFingerprint],
    by_tick: &HashMap<u64, &TickRecord>,
) -> Vec<String> {
    let mut sectors = HashSet::new();

    for fp in fingerprints {
        let Some(record) = by_tick.get(&fp.entry_tick) else {
            continue;
        };

        for entity in &record.world_state.entities {
            if entity.scope.label() == fp.center_scope
                && entity.layer == crate::ontology::world::WorldLayer::Leaf
            {
                // Try to find a branch-level entity linked to this leaf
                for branch_entity in &record.world_state.entities {
                    if matches!(
                        branch_entity.layer,
                        crate::ontology::world::WorldLayer::Branch
                    ) {
                        sectors.insert(branch_entity.scope.label());
                    }
                }
            }
        }
    }

    sectors.into_iter().collect()
}

/// Build a human-readable causal narrative from the channel chain.
fn build_causal_narrative(chain: &[String], mechanism: &CandidateMechanismRecord) -> String {
    if chain.is_empty() {
        return format!(
            "{} pattern with {} center (no channel ordering resolved)",
            mechanism.top_family, mechanism.center_kind
        );
    }

    let chain_description = if chain.len() == 1 {
        format!("{} drives the move", chain[0])
    } else {
        let steps: Vec<String> = chain
            .windows(2)
            .map(|pair| format!("{} precedes {}", pair[0], pair[1]))
            .collect();
        steps.join(", then ")
    };

    format!(
        "{} {} pattern: {} (mean return {:.2}% over {} samples)",
        mechanism.center_kind,
        mechanism.top_family,
        chain_description,
        mechanism.mean_net_return * Decimal::from(100),
        mechanism.samples,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::reasoning::ReasoningScope;
    use crate::ontology::world::{
        BackwardReasoningSnapshot, EntityState, FlowPath, FlowPolarity, WorldLayer,
        WorldStateSnapshot,
    };
    use crate::temporal::record::{SymbolSignals, TickRecord};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn prov(tag: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(tag)
            .with_inputs([tag.to_string()])
    }

    fn signal(price: Decimal) -> SymbolSignals {
        SymbolSignals {
            mark_price: Some(price),
            composite: dec!(0.6),
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

    fn make_mechanism() -> CandidateMechanismRecord {
        CandidateMechanismRecord {
            mechanism_id: "mech:hk:symbol:center:broker_flow|catalyst|propagation".into(),
            market: "hk".into(),
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
            mean_strength: dec!(0.48),
            mean_coherence: dec!(0.75),
            mean_channel_diversity: dec!(3.0),
            mode: "assist".into(),
            promoted_at_tick: 1,
            last_seen_tick: 20,
            last_hit_tick: Some(18),
            consecutive_misses: 0,
            post_promotion_hits: 4,
            post_promotion_misses: 1,
            post_promotion_net_return: dec!(0.12),
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        }
    }

    fn make_fingerprints() -> Vec<VortexOutcomeFingerprint> {
        vec![
            VortexOutcomeFingerprint {
                setup_id: "setup:700.HK:1".into(),
                family: "Convergence Hypothesis".into(),
                symbol: Some("700.HK".into()),
                entry_tick: 1,
                resolved_tick: 5,
                center_scope: "700.HK".into(),
                center_kind: "symbol".into(),
                role: "center".into(),
                channel_signature: "broker_flow|catalyst|propagation".into(),
                dominant_channels: vec![
                    "broker_flow".into(),
                    "catalyst".into(),
                    "propagation".into(),
                ],
                channel_diversity: 3,
                path_count: 3,
                strength: dec!(0.48),
                coherence: dec!(0.75),
                net_return: dec!(0.03),
            },
            VortexOutcomeFingerprint {
                setup_id: "setup:9988.HK:1".into(),
                family: "Convergence Hypothesis".into(),
                symbol: Some("9988.HK".into()),
                entry_tick: 3,
                resolved_tick: 8,
                center_scope: "9988.HK".into(),
                center_kind: "symbol".into(),
                role: "center".into(),
                channel_signature: "broker_flow|catalyst|propagation".into(),
                dominant_channels: vec![
                    "broker_flow".into(),
                    "catalyst".into(),
                    "propagation".into(),
                ],
                channel_diversity: 3,
                path_count: 3,
                strength: dec!(0.52),
                coherence: dec!(0.80),
                net_return: dec!(0.04),
            },
        ]
    }

    fn make_outcomes() -> Vec<CaseRealizedOutcome> {
        vec![
            CaseRealizedOutcome {
                setup_id: "setup:700.HK:1".into(),
                workflow_id: None,
                symbol: Some("700.HK".into()),
                entry_tick: 1,
                entry_timestamp: OffsetDateTime::UNIX_EPOCH,
                resolved_tick: 5,
                resolved_at: OffsetDateTime::UNIX_EPOCH,
                family: "Convergence Hypothesis".into(),
                session: "opening".into(),
                market_regime: "risk_on".into(),
                direction: 1,
                return_pct: dec!(0.05),
                net_return: dec!(0.03),
                max_favorable_excursion: dec!(0.06),
                max_adverse_excursion: dec!(-0.01),
                followed_through: true,
                invalidated: false,
                structure_retained: true,
                convergence_score: dec!(0.55),
            },
            CaseRealizedOutcome {
                setup_id: "setup:9988.HK:1".into(),
                workflow_id: None,
                symbol: Some("9988.HK".into()),
                entry_tick: 3,
                entry_timestamp: OffsetDateTime::UNIX_EPOCH,
                resolved_tick: 8,
                resolved_at: OffsetDateTime::UNIX_EPOCH,
                family: "Convergence Hypothesis".into(),
                session: "midday".into(),
                market_regime: "risk_on".into(),
                direction: 1,
                return_pct: dec!(0.06),
                net_return: dec!(0.04),
                max_favorable_excursion: dec!(0.07),
                max_adverse_excursion: dec!(-0.005),
                followed_through: true,
                invalidated: false,
                structure_retained: true,
                convergence_score: dec!(0.60),
            },
        ]
    }

    fn make_tick(tick_number: u64, price: Decimal) -> TickRecord {
        let symbol = crate::ontology::objects::Symbol("700.HK".into());
        let peer = crate::ontology::objects::Symbol("9988.HK".into());
        let mut signals = HashMap::new();
        signals.insert(symbol.clone(), signal(price));
        signals.insert(peer.clone(), signal(dec!(100)));

        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick_number as i64),
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![EntityState {
                    entity_id: format!("world:{}", tick_number),
                    scope: ReasoningScope::Symbol(symbol.clone()),
                    layer: WorldLayer::Leaf,
                    provenance: prov("entity"),
                    label: "700.HK".into(),
                    regime: "risk_on".into(),
                    confidence: dec!(0.7),
                    local_support: dec!(0.6),
                    propagated_support: dec!(0.3),
                    drivers: vec![],
                }],
                vortices: vec![crate::ontology::world::Vortex {
                    vortex_id: format!("vortex:{}", tick_number),
                    center_entity_id: format!("world:{}", tick_number),
                    center_scope: ReasoningScope::Symbol(symbol.clone()),
                    layer: WorldLayer::Leaf,
                    flow_paths: vec![
                        FlowPath {
                            source_entity_id: "world:peer".into(),
                            source_scope: ReasoningScope::Symbol(peer.clone()),
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
                }],
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
    fn extracts_causal_schema_from_mechanism() {
        let mechanism = make_mechanism();
        let fingerprints = make_fingerprints();
        let outcomes = make_outcomes();

        let mut history = TickHistory::new(32);
        for tick in 1..=10 {
            history.push(make_tick(tick, dec!(100) + Decimal::from(tick as i64)));
        }

        let schema = extract_causal_schema(
            &mechanism,
            &fingerprints,
            &outcomes,
            &history,
            20,
            "2026-04-01T00:00:00Z",
        );

        assert!(schema.is_some());
        let schema = schema.unwrap();

        assert_eq!(schema.market, "hk");
        assert_eq!(schema.status, "candidate");
        assert!(schema.causal_narrative.contains("broker_flow"));
        assert!(schema.causal_narrative.contains("catalyst"));

        // Channel chain should have ordering
        assert!(!schema.channel_chain.is_empty());
        assert_eq!(schema.channel_chain[0], "broker_flow"); // highest weight

        // Two different symbols → cross-symbol validated
        assert!(schema.cross_symbol_validated);

        // Two different sessions → cross-session validated
        assert!(schema.cross_session_validated);

        // Min thresholds extracted from fingerprints
        assert!(schema.min_coherence > Decimal::ZERO);
        assert!(schema.min_strength > Decimal::ZERO);
    }

    #[test]
    fn returns_none_for_insufficient_fingerprints() {
        let mechanism = make_mechanism();
        let fingerprints = vec![make_fingerprints().remove(0)]; // Only one
        let outcomes = make_outcomes();

        let mut history = TickHistory::new(32);
        history.push(make_tick(1, dec!(100)));

        let schema = extract_causal_schema(
            &mechanism,
            &fingerprints,
            &outcomes,
            &history,
            20,
            "2026-04-01T00:00:00Z",
        );

        assert!(schema.is_none());
    }

    #[test]
    fn preconditions_check_works() {
        let mechanism = make_mechanism();
        let fingerprints = make_fingerprints();
        let outcomes = make_outcomes();

        let mut history = TickHistory::new(32);
        for tick in 1..=10 {
            history.push(make_tick(tick, dec!(100) + Decimal::from(tick as i64)));
        }

        let schema = extract_causal_schema(
            &mechanism,
            &fingerprints,
            &outcomes,
            &history,
            20,
            "2026-04-01T00:00:00Z",
        )
        .unwrap();

        // Should pass with good context
        assert!(schema.preconditions_met(
            "risk_on",
            "opening",
            dec!(0.75),
            dec!(0.48),
            dec!(0.55),
            "Stable",
        ));

        // Should fail with low coherence
        assert!(!schema.preconditions_met(
            "risk_on",
            "opening",
            dec!(0.1), // below min_coherence
            dec!(0.48),
            dec!(0.55),
            "Stable",
        ));
    }
}
