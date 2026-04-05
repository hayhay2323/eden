//! Self-evolution closed loop with governance.
//!
//! This module implements the governed lifecycle for candidate mechanisms and
//! causal schemas: shadow scoring, surface quality monitoring, rollback, and
//! survivor selection.
//!
//! The key contract: emergent structures can grow, but they must not degrade
//! the system's overall decision quality. Every promotion is provisional;
//! every schema earns its place or gets cleaned up.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::persistence::candidate_mechanism::CandidateMechanismRecord;
use crate::persistence::causal_schema::CausalSchemaRecord;

// ---------------------------------------------------------------------------
// Audit trail
// ---------------------------------------------------------------------------

/// A single lifecycle event recorded for provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionEvent {
    pub tick: u64,
    pub timestamp: String,
    pub entity_id: String,
    /// "mechanism" | "schema"
    pub entity_kind: String,
    /// "promote" | "demote" | "rollback" | "prune" | "score"
    pub action: String,
    pub from_state: String,
    pub to_state: String,
    pub reason: String,
}

/// Result of running one evolution cycle.
#[derive(Debug, Clone, Default)]
pub struct EvolutionCycleResult {
    pub events: Vec<EvolutionEvent>,
    pub mechanisms_promoted: usize,
    pub mechanisms_demoted: usize,
    pub mechanisms_pruned: usize,
    pub schemas_promoted: usize,
    pub schemas_demoted: usize,
    pub schemas_pruned: usize,
    pub rollbacks_triggered: usize,
}

// ---------------------------------------------------------------------------
// Shadow scoring
// ---------------------------------------------------------------------------

/// Shadow score for a schema that hasn't entered live mode yet.
/// Records "what would have happened if this schema participated".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShadowScore {
    pub schema_id: String,
    pub would_have_matched: u64,
    pub would_have_hit: u64,
    pub would_have_missed: u64,
    pub counterfactual_return: Decimal,
}

impl ShadowScore {
    pub fn counterfactual_hit_rate(&self) -> Decimal {
        let total = self.would_have_hit + self.would_have_missed;
        if total == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(self.would_have_hit) / Decimal::from(total)
        }
    }
}

/// Score a shadow/assist schema against current tick outcomes.
/// Returns true if the schema would have matched this tick's context.
pub fn shadow_score_schema(
    schema: &CausalSchemaRecord,
    score: &mut ShadowScore,
    regime: &str,
    session: &str,
    coherence: Decimal,
    strength: Decimal,
    convergence_score: Decimal,
    contest_state: &str,
    outcome_return: Option<Decimal>,
) -> bool {
    let preconditions_met = schema.preconditions_met(
        regime,
        session,
        coherence,
        strength,
        convergence_score,
        contest_state,
    );

    if !preconditions_met {
        return false;
    }

    score.would_have_matched += 1;

    if let Some(net_return) = outcome_return {
        score.counterfactual_return += net_return;
        if net_return > Decimal::ZERO {
            score.would_have_hit += 1;
        } else {
            score.would_have_missed += 1;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Surface quality monitor
// ---------------------------------------------------------------------------

/// Snapshot of overall system decision quality at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceQualitySnapshot {
    pub tick: u64,
    pub total_setups: u64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub overall_hit_rate: Decimal,
    pub overall_mean_return: Decimal,
    pub live_mechanism_count: usize,
    pub live_schema_count: usize,
}

impl SurfaceQualitySnapshot {
    pub fn from_outcomes(
        tick: u64,
        hit_count: u64,
        miss_count: u64,
        total_return: Decimal,
        live_mechanism_count: usize,
        live_schema_count: usize,
    ) -> Self {
        let total = hit_count + miss_count;
        Self {
            tick,
            total_setups: total,
            total_hits: hit_count,
            total_misses: miss_count,
            overall_hit_rate: if total > 0 {
                Decimal::from(hit_count) / Decimal::from(total)
            } else {
                Decimal::ZERO
            },
            overall_mean_return: if total > 0 {
                total_return / Decimal::from(total)
            } else {
                Decimal::ZERO
            },
            live_mechanism_count,
            live_schema_count,
        }
    }
}

/// Detect if surface quality has degraded since live schemas were introduced.
/// Compares recent quality against a baseline (before live schemas existed).
pub fn detect_quality_degradation(
    baseline: &SurfaceQualitySnapshot,
    current: &SurfaceQualitySnapshot,
) -> Option<String> {
    // Need enough data in both
    if baseline.total_setups < 5 || current.total_setups < 5 {
        return None;
    }

    let hit_rate_drop = baseline.overall_hit_rate - current.overall_hit_rate;
    let return_drop = baseline.overall_mean_return - current.overall_mean_return;

    // Degradation thresholds:
    // Hit rate dropped by >10 percentage points
    if hit_rate_drop > Decimal::new(10, 2) {
        return Some(format!(
            "hit rate degraded from {:.1}% to {:.1}% ({:.1}pp drop)",
            baseline.overall_hit_rate * Decimal::from(100),
            current.overall_hit_rate * Decimal::from(100),
            hit_rate_drop * Decimal::from(100),
        ));
    }

    // Mean return dropped significantly
    if return_drop > Decimal::new(2, 2) && current.overall_mean_return < Decimal::ZERO {
        return Some(format!(
            "mean return degraded from {:.3} to {:.3}",
            baseline.overall_mean_return, current.overall_mean_return,
        ));
    }

    None
}

// ---------------------------------------------------------------------------
// Evolution cycle: the main governance loop
// ---------------------------------------------------------------------------

/// Run one evolution cycle across all mechanisms and schemas.
///
/// This is the core governance function. Call it periodically (e.g., every 50 ticks).
/// It handles:
/// 1. Mechanism lifecycle transitions (promote, demote, prune)
/// 2. Schema lifecycle transitions (validate, activate, degrade, prune)
/// 3. Rollback: demote live schemas if surface quality degraded
/// 4. Survivor selection: prune stale/failing entities
pub fn run_evolution_cycle(
    mechanisms: &mut Vec<CandidateMechanismRecord>,
    schemas: &mut Vec<CausalSchemaRecord>,
    shadow_scores: &HashMap<String, ShadowScore>,
    baseline_quality: Option<&SurfaceQualitySnapshot>,
    current_quality: Option<&SurfaceQualitySnapshot>,
    current_tick: u64,
    now_rfc3339: &str,
) -> EvolutionCycleResult {
    let mut result = EvolutionCycleResult::default();

    // --- 1. Mechanism lifecycle ---
    let mut mechanisms_to_remove = Vec::new();

    for mech in mechanisms.iter_mut() {
        let old_mode = mech.mode.clone();

        // Promote shadow → assist
        if mech.should_promote_to_assist() {
            mech.mode = "assist".into();
            mech.updated_at = now_rfc3339.to_string();
            result.mechanisms_promoted += 1;
            result.events.push(EvolutionEvent {
                tick: current_tick,
                timestamp: now_rfc3339.to_string(),
                entity_id: mech.mechanism_id.clone(),
                entity_kind: "mechanism".into(),
                action: "promote".into(),
                from_state: old_mode.clone(),
                to_state: "assist".into(),
                reason: format!(
                    "hit_rate={:.1}%, net_return={:.4}",
                    mech.hit_rate() * Decimal::from(100),
                    mech.post_promotion_net_return
                ),
            });
        }

        // Promote assist → live
        if mech.should_promote_to_live() {
            mech.mode = "live".into();
            mech.updated_at = now_rfc3339.to_string();
            result.mechanisms_promoted += 1;
            result.events.push(EvolutionEvent {
                tick: current_tick,
                timestamp: now_rfc3339.to_string(),
                entity_id: mech.mechanism_id.clone(),
                entity_kind: "mechanism".into(),
                action: "promote".into(),
                from_state: old_mode.clone(),
                to_state: "live".into(),
                reason: format!(
                    "hit_rate={:.1}%, samples={}",
                    mech.hit_rate() * Decimal::from(100),
                    mech.post_promotion_hits + mech.post_promotion_misses,
                ),
            });
        }

        // Decay: demote or mark for pruning
        if mech.should_decay(current_tick) {
            if let Some(demoted) = mech.demoted_mode() {
                let from = mech.mode.clone();
                mech.mode = demoted.into();
                mech.consecutive_misses = 0;
                mech.updated_at = now_rfc3339.to_string();
                result.mechanisms_demoted += 1;
                result.events.push(EvolutionEvent {
                    tick: current_tick,
                    timestamp: now_rfc3339.to_string(),
                    entity_id: mech.mechanism_id.clone(),
                    entity_kind: "mechanism".into(),
                    action: "demote".into(),
                    from_state: from,
                    to_state: demoted.into(),
                    reason: format!(
                        "consecutive_misses={}, ticks_since_seen={}",
                        mech.consecutive_misses,
                        current_tick.saturating_sub(mech.last_seen_tick),
                    ),
                });
            } else {
                // Already shadow and decaying → mark for pruning
                mechanisms_to_remove.push(mech.mechanism_id.clone());
            }
        }
    }

    // Prune dead mechanisms
    for id in &mechanisms_to_remove {
        if let Some(pos) = mechanisms.iter().position(|m| &m.mechanism_id == id) {
            let removed = mechanisms.remove(pos);
            result.mechanisms_pruned += 1;
            result.events.push(EvolutionEvent {
                tick: current_tick,
                timestamp: now_rfc3339.to_string(),
                entity_id: removed.mechanism_id,
                entity_kind: "mechanism".into(),
                action: "prune".into(),
                from_state: removed.mode,
                to_state: "removed".into(),
                reason: "shadow mechanism decayed beyond recovery".into(),
            });
        }
    }

    // --- 2. Schema lifecycle ---
    let mut schemas_to_remove = Vec::new();

    for schema in schemas.iter_mut() {
        let old_status = schema.status.clone();

        // Promote candidate → validated
        if schema.should_validate() {
            // Check shadow score if available
            let shadow_ok = shadow_scores
                .get(&schema.schema_id)
                .map(|score| {
                    score.counterfactual_hit_rate() >= Decimal::new(30, 2)
                        || score.would_have_matched < 3
                })
                .unwrap_or(true); // No shadow data → allow

            if shadow_ok {
                schema.status = "validated".into();
                schema.updated_at = now_rfc3339.to_string();
                result.schemas_promoted += 1;
                result.events.push(EvolutionEvent {
                    tick: current_tick,
                    timestamp: now_rfc3339.to_string(),
                    entity_id: schema.schema_id.clone(),
                    entity_kind: "schema".into(),
                    action: "promote".into(),
                    from_state: old_status,
                    to_state: "validated".into(),
                    reason: format!(
                        "hit_rate={:.1}%, applications={}",
                        schema.hit_rate() * Decimal::from(100),
                        schema.total_applications,
                    ),
                });
            }
            continue;
        }

        // Promote validated → active
        if schema.should_activate() {
            schema.status = "active".into();
            schema.updated_at = now_rfc3339.to_string();
            result.schemas_promoted += 1;
            result.events.push(EvolutionEvent {
                tick: current_tick,
                timestamp: now_rfc3339.to_string(),
                entity_id: schema.schema_id.clone(),
                entity_kind: "schema".into(),
                action: "promote".into(),
                from_state: old_status,
                to_state: "active".into(),
                reason: format!(
                    "cross_symbol={}, cross_session={}, cross_regime={}",
                    schema.cross_symbol_validated,
                    schema.cross_session_validated,
                    schema.cross_regime_validated,
                ),
            });
            continue;
        }

        // Degrade
        if schema.should_degrade(current_tick) {
            if schema.status == "active" || schema.status == "validated" {
                let from = schema.status.clone();
                schema.status = "degraded".into();
                schema.updated_at = now_rfc3339.to_string();
                result.schemas_demoted += 1;
                result.events.push(EvolutionEvent {
                    tick: current_tick,
                    timestamp: now_rfc3339.to_string(),
                    entity_id: schema.schema_id.clone(),
                    entity_kind: "schema".into(),
                    action: "demote".into(),
                    from_state: from,
                    to_state: "degraded".into(),
                    reason: format!(
                        "stale={}, hit_rate={:.1}%",
                        current_tick.saturating_sub(schema.last_applied_tick) > 300,
                        schema.hit_rate() * Decimal::from(100),
                    ),
                });
            } else if schema.status == "candidate" || schema.status == "degraded" {
                // Candidate/degraded that keeps decaying → prune
                let ticks_since = current_tick.saturating_sub(schema.last_applied_tick);
                if ticks_since > 500 {
                    schemas_to_remove.push(schema.schema_id.clone());
                }
            }
        }
    }

    // --- 3. Rollback: if surface quality degraded, demote live schemas ---
    if let (Some(baseline), Some(current)) = (baseline_quality, current_quality) {
        if let Some(reason) = detect_quality_degradation(baseline, current) {
            // Find live schemas that were recently promoted and demote them
            for schema in schemas.iter_mut() {
                if schema.status == "active" {
                    let ticks_since_promotion =
                        current_tick.saturating_sub(schema.promoted_at_tick);
                    // Only rollback recently promoted schemas (within 100 ticks)
                    if ticks_since_promotion < 100 {
                        schema.status = "validated".into();
                        schema.updated_at = now_rfc3339.to_string();
                        result.rollbacks_triggered += 1;
                        result.events.push(EvolutionEvent {
                            tick: current_tick,
                            timestamp: now_rfc3339.to_string(),
                            entity_id: schema.schema_id.clone(),
                            entity_kind: "schema".into(),
                            action: "rollback".into(),
                            from_state: "active".into(),
                            to_state: "validated".into(),
                            reason: format!("surface quality rollback: {}", reason),
                        });
                    }
                }
            }

            // Also demote recently-promoted live mechanisms
            for mech in mechanisms.iter_mut() {
                if mech.mode == "live" {
                    let ticks_since_seen = current_tick.saturating_sub(mech.promoted_at_tick);
                    if ticks_since_seen < 100 {
                        mech.mode = "assist".into();
                        mech.updated_at = now_rfc3339.to_string();
                        result.rollbacks_triggered += 1;
                        result.events.push(EvolutionEvent {
                            tick: current_tick,
                            timestamp: now_rfc3339.to_string(),
                            entity_id: mech.mechanism_id.clone(),
                            entity_kind: "mechanism".into(),
                            action: "rollback".into(),
                            from_state: "live".into(),
                            to_state: "assist".into(),
                            reason: format!("surface quality rollback: {}", reason),
                        });
                    }
                }
            }
        }
    }

    // --- 4. Prune dead schemas ---
    for id in &schemas_to_remove {
        if let Some(pos) = schemas.iter().position(|s| &s.schema_id == id) {
            let removed = schemas.remove(pos);
            result.schemas_pruned += 1;
            result.events.push(EvolutionEvent {
                tick: current_tick,
                timestamp: now_rfc3339.to_string(),
                entity_id: removed.schema_id,
                entity_kind: "schema".into(),
                action: "prune".into(),
                from_state: removed.status,
                to_state: "removed".into(),
                reason: "stale schema exceeded maximum dormancy".into(),
            });
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::causal_schema::{RegimeAffinity, SchemaInvalidationRule, SessionAffinity};
    use rust_decimal_macros::dec;

    fn make_mechanism(id: &str, mode: &str, hits: u64, misses: u64) -> CandidateMechanismRecord {
        CandidateMechanismRecord {
            mechanism_id: id.into(),
            market: "hk".into(),
            center_kind: "symbol".into(),
            role: "center".into(),
            channel_signature: "test".into(),
            dominant_channels: vec!["test".into()],
            top_family: "Test".into(),
            samples: 10,
            mean_net_return: dec!(0.03),
            mean_strength: dec!(0.5),
            mean_coherence: dec!(0.7),
            mean_channel_diversity: dec!(2.0),
            mode: mode.into(),
            promoted_at_tick: 1,
            last_seen_tick: 50,
            last_hit_tick: Some(48),
            consecutive_misses: 0,
            post_promotion_hits: hits,
            post_promotion_misses: misses,
            post_promotion_net_return: Decimal::from(hits) * dec!(0.03)
                - Decimal::from(misses) * dec!(0.01),
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        }
    }

    fn make_schema(id: &str, status: &str, hits: u64, misses: u64) -> CausalSchemaRecord {
        CausalSchemaRecord {
            schema_id: id.into(),
            mechanism_id: "mech:test".into(),
            market: "hk".into(),
            channel_chain: vec!["broker_flow".into(), "catalyst".into()],
            causal_narrative: "test narrative".into(),
            regime_affinity: vec![RegimeAffinity {
                regime: "risk_on".into(),
                hit_count: hits,
                miss_count: misses,
                mean_return: dec!(0.02),
            }],
            session_affinity: vec![SessionAffinity {
                session: "opening".into(),
                hit_count: hits,
                miss_count: misses,
                mean_return: dec!(0.02),
            }],
            min_coherence: dec!(0.5),
            min_strength: dec!(0.3),
            min_convergence_score: dec!(0.4),
            preferred_contest_states: vec!["Stable".into()],
            invalidation_rules: vec![SchemaInvalidationRule {
                kind: "contest_flip".into(),
                description: "causal flip".into(),
                failure_correlation: dec!(60),
            }],
            observed_symbols: vec!["700.HK".into(), "9988.HK".into()],
            observed_sectors: vec!["tech".into()],
            applicable_center_kinds: vec!["symbol".into()],
            cross_symbol_validated: true,
            cross_session_validated: false,
            cross_regime_validated: false,
            total_applications: hits + misses,
            successful_applications: hits,
            failed_applications: misses,
            mean_return_when_applied: dec!(0.02),
            mean_return_when_preconditions_met: dec!(0.03),
            mean_return_when_preconditions_violated: dec!(-0.01),
            status: status.into(),
            promoted_at_tick: 1,
            last_applied_tick: 50,
            created_at: "2026-04-01T00:00:00Z".into(),
            updated_at: "2026-04-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn evolution_cycle_promotes_shadow_mechanism() {
        let mut mechanisms = vec![make_mechanism("mech:a", "shadow", 4, 1)];
        let mut schemas = vec![];
        let scores = HashMap::new();

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &scores,
            None,
            None,
            100,
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.mechanisms_promoted, 1);
        assert_eq!(mechanisms[0].mode, "assist");
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].action, "promote");
    }

    #[test]
    fn evolution_cycle_promotes_candidate_schema() {
        let mut mechanisms = vec![];
        let mut schemas = vec![make_schema("schema:a", "candidate", 4, 2)];
        let scores = HashMap::new();

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &scores,
            None,
            None,
            100,
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.schemas_promoted, 1);
        assert_eq!(schemas[0].status, "validated");
    }

    #[test]
    fn evolution_cycle_degrades_stale_schema() {
        let mut mechanisms = vec![];
        let mut schemas = vec![make_schema("schema:stale", "active", 3, 1)];
        schemas[0].last_applied_tick = 10; // Very old

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &HashMap::new(),
            None,
            None,
            500, // 490 ticks since last applied
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.schemas_demoted, 1);
        assert_eq!(schemas[0].status, "degraded");
    }

    #[test]
    fn evolution_cycle_prunes_dead_mechanism() {
        let mut mechanisms = vec![make_mechanism("mech:dead", "shadow", 0, 0)];
        mechanisms[0].last_seen_tick = 10;
        mechanisms[0].consecutive_misses = 0;
        // Make it stale: 290 ticks since seen
        let mut schemas = vec![];

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &HashMap::new(),
            None,
            None,
            300, // 290 ticks since seen → stale
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.mechanisms_pruned, 1);
        assert!(mechanisms.is_empty());
    }

    #[test]
    fn rollback_on_quality_degradation() {
        let mut mechanisms = vec![make_mechanism("mech:recent", "live", 3, 1)];
        mechanisms[0].promoted_at_tick = 60; // Recently promoted
        let mut schemas = vec![make_schema("schema:recent", "active", 5, 2)];
        schemas[0].promoted_at_tick = 60; // Recently promoted

        let baseline = SurfaceQualitySnapshot {
            tick: 50,
            total_setups: 20,
            total_hits: 12,
            total_misses: 8,
            overall_hit_rate: dec!(0.60),
            overall_mean_return: dec!(0.02),
            live_mechanism_count: 0,
            live_schema_count: 0,
        };

        let current = SurfaceQualitySnapshot {
            tick: 100,
            total_setups: 20,
            total_hits: 8,
            total_misses: 12,
            overall_hit_rate: dec!(0.40), // Dropped 20pp → triggers rollback
            overall_mean_return: dec!(0.005),
            live_mechanism_count: 1,
            live_schema_count: 1,
        };

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &HashMap::new(),
            Some(&baseline),
            Some(&current),
            100,
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.rollbacks_triggered, 2);
        assert_eq!(mechanisms[0].mode, "assist"); // Rolled back from live
        assert_eq!(schemas[0].status, "validated"); // Rolled back from active
    }

    #[test]
    fn shadow_scoring_tracks_counterfactual() {
        let schema = make_schema("schema:shadow", "candidate", 3, 1);
        let mut score = ShadowScore {
            schema_id: "schema:shadow".into(),
            ..Default::default()
        };

        // Preconditions met, positive outcome
        let matched = shadow_score_schema(
            &schema,
            &mut score,
            "risk_on",
            "opening",
            dec!(0.7),
            dec!(0.5),
            dec!(0.5),
            "Stable",
            Some(dec!(0.03)),
        );
        assert!(matched);
        assert_eq!(score.would_have_matched, 1);
        assert_eq!(score.would_have_hit, 1);

        // Preconditions met, negative outcome
        shadow_score_schema(
            &schema,
            &mut score,
            "risk_on",
            "opening",
            dec!(0.7),
            dec!(0.5),
            dec!(0.5),
            "Stable",
            Some(dec!(-0.02)),
        );
        assert_eq!(score.would_have_matched, 2);
        assert_eq!(score.would_have_hit, 1);
        assert_eq!(score.would_have_missed, 1);
        assert_eq!(score.counterfactual_return, dec!(0.01));

        // Preconditions NOT met (low coherence)
        let not_matched = shadow_score_schema(
            &schema,
            &mut score,
            "risk_on",
            "opening",
            dec!(0.1), // below min_coherence
            dec!(0.5),
            dec!(0.5),
            "Stable",
            Some(dec!(0.05)),
        );
        assert!(!not_matched);
        assert_eq!(score.would_have_matched, 2); // unchanged
    }

    #[test]
    fn no_rollback_when_quality_stable() {
        let mut mechanisms = vec![make_mechanism("mech:ok", "live", 5, 2)];
        mechanisms[0].promoted_at_tick = 60;
        let mut schemas = vec![];

        let baseline = SurfaceQualitySnapshot {
            tick: 50,
            total_setups: 20,
            total_hits: 12,
            total_misses: 8,
            overall_hit_rate: dec!(0.60),
            overall_mean_return: dec!(0.02),
            live_mechanism_count: 0,
            live_schema_count: 0,
        };

        let current = SurfaceQualitySnapshot {
            tick: 100,
            total_setups: 20,
            total_hits: 11,
            total_misses: 9,
            overall_hit_rate: dec!(0.55), // Only 5pp drop → no rollback
            overall_mean_return: dec!(0.015),
            live_mechanism_count: 1,
            live_schema_count: 0,
        };

        let result = run_evolution_cycle(
            &mut mechanisms,
            &mut schemas,
            &HashMap::new(),
            Some(&baseline),
            Some(&current),
            100,
            "2026-04-01T00:00:00Z",
        );

        assert_eq!(result.rollbacks_triggered, 0);
        assert_eq!(mechanisms[0].mode, "live"); // Unchanged
    }
}
