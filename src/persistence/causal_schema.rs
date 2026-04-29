use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A causal schema — a candidate mechanism elevated to a transferable causal
/// structure with preconditions, invalidation rules, and regime affinity.
///
/// While a CandidateMechanism says "channels A|B|C historically work",
/// a CausalSchema says "A precedes B which enables C, this works in regime X
/// when coherence > T, and breaks when contest_state flips or contradicting
/// weight exceeds supporting weight."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalSchemaRecord {
    pub schema_id: String,
    pub mechanism_id: String,
    pub market: String,

    // --- channel causal ordering ---
    /// Ordered sequence of channels by causal precedence (first → last).
    /// Extracted from temporal ordering of flow path weights across successful cases.
    pub channel_chain: Vec<String>,
    /// Human-readable causal narrative synthesized from the chain.
    pub causal_narrative: String,

    // --- preconditions (extracted from successful outcomes) ---
    /// Regimes where this schema has succeeded.
    pub regime_affinity: Vec<RegimeAffinity>,
    /// Sessions where this schema has succeeded.
    pub session_affinity: Vec<SessionAffinity>,
    /// Minimum coherence threshold observed across successes.
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub min_coherence: Decimal,
    /// Minimum strength threshold observed across successes.
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub min_strength: Decimal,
    /// Minimum convergence score at entry across successes.
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub min_convergence_score: Decimal,
    /// Contest state at entry for successful cases (e.g., mostly "Stable" or "New").
    pub preferred_contest_states: Vec<String>,

    // --- invalidation conditions (extracted from failed outcomes + causal flips) ---
    /// Structural conditions that historically break this schema.
    pub invalidation_rules: Vec<SchemaInvalidationRule>,

    // --- transferability ---
    /// Distinct symbols where this schema has been observed succeeding.
    pub observed_symbols: Vec<String>,
    /// Distinct sectors where this schema has been observed succeeding.
    pub observed_sectors: Vec<String>,
    /// Center kinds this schema has applied to (symbol, sector, market).
    pub applicable_center_kinds: Vec<String>,
    /// Whether this schema has transferred successfully to symbols outside
    /// its original observation set.
    pub cross_symbol_validated: bool,
    /// Whether this schema has transferred across sessions.
    pub cross_session_validated: bool,
    /// Whether this schema has transferred across regimes.
    pub cross_regime_validated: bool,

    // --- performance ---
    pub total_applications: u64,
    pub successful_applications: u64,
    pub failed_applications: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_return_when_applied: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_return_when_preconditions_met: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_return_when_preconditions_violated: Decimal,

    // --- lifecycle ---
    /// "candidate" | "validated" | "active" | "degraded"
    pub status: String,
    pub promoted_at_tick: u64,
    pub last_applied_tick: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// How well a schema performs in a particular market regime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeAffinity {
    pub regime: String,
    pub hit_count: u64,
    pub miss_count: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_return: Decimal,
}

/// How well a schema performs in a particular session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAffinity {
    pub session: String,
    pub hit_count: u64,
    pub miss_count: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_return: Decimal,
}

/// A structural invalidation rule: when this condition is true, the schema
/// is expected to fail. More precise than confidence drop alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaInvalidationRule {
    /// "contest_flip" | "coherence_drop" | "contradicting_dominance" | "regime_mismatch" | "channel_absence"
    pub kind: String,
    /// Human-readable description.
    pub description: String,
    /// How often this condition was present in failed cases.
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub failure_correlation: Decimal,
}

impl CausalSchemaRecord {
    pub fn schema_key(mechanism_id: &str) -> String {
        format!("schema:{}", mechanism_id)
    }

    pub fn hit_rate(&self) -> Decimal {
        let total = self.successful_applications + self.failed_applications;
        if total == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(self.successful_applications) / Decimal::from(total)
        }
    }

    /// Check whether preconditions are met for a given context.
    pub fn preconditions_met(
        &self,
        regime: &str,
        session: &str,
        coherence: Decimal,
        strength: Decimal,
        convergence_score: Decimal,
        contest_state: &str,
    ) -> bool {
        // Regime check: at least one affinity with positive mean return
        let regime_ok = self.regime_affinity.is_empty()
            || self
                .regime_affinity
                .iter()
                .any(|aff| aff.regime == regime && aff.mean_return > Decimal::ZERO);

        // Session check
        let session_ok = self.session_affinity.is_empty()
            || self
                .session_affinity
                .iter()
                .any(|aff| aff.session == session && aff.mean_return > Decimal::ZERO);

        // Structural thresholds
        let coherence_ok = coherence >= self.min_coherence;
        let strength_ok = strength >= self.min_strength;
        let convergence_ok = convergence_score >= self.min_convergence_score;

        // Contest state
        let contest_ok = self.preferred_contest_states.is_empty()
            || self
                .preferred_contest_states
                .iter()
                .any(|state| state == contest_state);

        regime_ok && session_ok && coherence_ok && strength_ok && convergence_ok && contest_ok
    }

    /// Check whether any invalidation rule is triggered.
    pub fn invalidation_triggered(
        &self,
        contest_state: &str,
        coherence: Decimal,
        contradicting_weight: Decimal,
        supporting_weight: Decimal,
        active_channels: &[String],
    ) -> Option<&SchemaInvalidationRule> {
        for rule in &self.invalidation_rules {
            let triggered = match rule.kind.as_str() {
                "contest_flip" => contest_state == "Flipped" || contest_state == "Contested",
                "coherence_drop" => coherence < self.min_coherence,
                "contradicting_dominance" => {
                    contradicting_weight > Decimal::ZERO && contradicting_weight > supporting_weight
                }
                "channel_absence" => {
                    // At least one chain channel must be missing
                    self.channel_chain
                        .iter()
                        .any(|ch| !active_channels.contains(ch))
                }
                "regime_mismatch" => {
                    // Check is done in preconditions_met; this is for runtime
                    false
                }
                _ => false,
            };
            if triggered && rule.failure_correlation >= Decimal::new(30, 2) {
                return Some(rule);
            }
        }
        None
    }

    /// Should this schema be promoted from candidate to validated?
    pub fn should_validate(&self) -> bool {
        if self.status != "candidate" {
            return false;
        }
        let total = self.successful_applications + self.failed_applications;
        total >= 5 && self.hit_rate() >= Decimal::new(35, 2)
    }

    /// Should this schema be promoted from validated to active?
    pub fn should_activate(&self) -> bool {
        if self.status != "validated" {
            return false;
        }
        // Need cross-validation evidence
        let cross_validated = self.cross_symbol_validated
            || self.cross_session_validated
            || self.cross_regime_validated;
        let total = self.successful_applications + self.failed_applications;
        total >= 10 && self.hit_rate() >= Decimal::new(30, 2) && cross_validated
    }

    /// Should this schema be degraded?
    pub fn should_degrade(&self, current_tick: u64) -> bool {
        if self.status == "degraded" {
            return false;
        }
        let stale = current_tick.saturating_sub(self.last_applied_tick) > 300;
        let total = self.successful_applications + self.failed_applications;
        let failing = total >= 8 && self.hit_rate() < Decimal::new(20, 2);
        stale || failing
    }
}
