use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::convergence::ConvergenceScore;
use crate::graph::decision::MarketRegimeFilter;
use crate::ontology::objects::{SectorId, Symbol};
use crate::ontology::world::WorldStateSnapshot;
use crate::pipeline::dimensions::SymbolDimensions;
use crate::temporal::lineage::{FamilyContextLineageOutcome, MultiHorizonGate};

use super::ReviewerDoctrinePressure;

// ── Absence Memory ──

#[derive(Debug, Clone)]
pub struct AbsenceEntry {
    pub consecutive_count: u32,
    pub last_seen: OffsetDateTime,
}

/// Tracks which (sector, family) pairs have shown propagation absence.
/// Runtime updates it each tick; ReasoningContext takes a snapshot reference.
#[derive(Debug, Clone, Default)]
pub struct AbsenceMemory {
    entries: HashMap<(String, String), AbsenceEntry>,
}

impl AbsenceMemory {
    /// Record that a (sector, family) pair showed propagation absence this tick.
    pub fn record_absence(
        &mut self,
        sector: &SectorId,
        family: &str,
        _tick: u64,
        now: OffsetDateTime,
    ) {
        let key = (sector.0.clone(), family.to_ascii_lowercase());
        let entry = self.entries.entry(key).or_insert(AbsenceEntry {
            consecutive_count: 0,
            last_seen: now,
        });
        entry.consecutive_count += 1;
        entry.last_seen = now;
    }

    /// Clear absence tracking for a sector that DID propagate this tick.
    pub fn record_propagation(&mut self, sector: &SectorId) {
        self.entries
            .retain(|(sector_key, _), _| *sector_key != sector.0);
    }

    /// Should we suppress hypothesis generation for this (sector, family)?
    pub fn should_suppress(&self, sector: &SectorId, family: &str) -> bool {
        let key = (sector.0.clone(), family.to_ascii_lowercase());
        self.entries
            .get(&key)
            .map(|entry| entry.consecutive_count >= 3)
            .unwrap_or(false)
    }

    /// Remove entries older than 30 minutes.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::minutes(30);
        self.entries.retain(|_, entry| entry.last_seen >= cutoff);
    }
}

// ── Family Boost Ledger ──

/// Positive feedback mirror of FamilyAlphaGate.
/// Families with proven good track records get a confidence boost factor.
#[derive(Debug, Clone, Default)]
pub struct FamilyBoostLedger {
    boosts: HashMap<String, Decimal>,
}

impl FamilyBoostLedger {
    pub fn from_lineage_priors(
        priors: &[FamilyContextLineageOutcome],
        session: &str,
        regime: &str,
    ) -> Self {
        let mut boosts = HashMap::new();
        let families: std::collections::HashSet<String> =
            priors.iter().map(|p| p.family.clone()).collect();
        for family in families {
            let prior = super::family_gate::best_family_prior(priors, &family, session, regime);
            if let Some(prior) = prior {
                let boost = compute_family_boost(prior);
                if boost != Decimal::ONE {
                    boosts.insert(family.to_ascii_lowercase(), boost);
                }
            }
        }
        Self { boosts }
    }

    /// Returns boost factor: 1.0 = neutral, >1.0 = boosted. Never < 1.0.
    pub fn boost_for_family(&self, family: &str) -> Decimal {
        self.boosts
            .get(&family.to_ascii_lowercase())
            .copied()
            .unwrap_or(Decimal::ONE)
    }
}

fn compute_family_boost(prior: &FamilyContextLineageOutcome) -> Decimal {
    if prior.follow_through_rate < Decimal::new(55, 2) || prior.mean_net_return <= Decimal::ZERO {
        return Decimal::ONE;
    }
    let raw = Decimal::ONE
        + (prior.follow_through_rate - Decimal::new(50, 2)) * Decimal::new(5, 1);
    raw.min(Decimal::new(125, 2))
}

// ── Convergence Detail ──

/// Structured subset of ConvergenceScore for TacticalSetup.
/// Preserves the multi-dimensional convergence information that would
/// otherwise be compressed to a single scalar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvergenceDetail {
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub component_spread: Option<Decimal>,
    pub edge_stability: Option<Decimal>,
}

impl ConvergenceDetail {
    pub fn from_convergence_score(score: &ConvergenceScore) -> Self {
        Self {
            institutional_alignment: score.institutional_alignment,
            sector_coherence: score.sector_coherence,
            cross_stock_correlation: score.cross_stock_correlation,
            component_spread: score.component_spread,
            edge_stability: score.edge_stability,
        }
    }
}

// ── Reasoning Context ──

/// Immutable per-tick snapshot of all contextual intelligence
/// available to the reasoning pipeline.
/// Assembled by runtime, consumed by synthesis/policy — never mutated.
pub struct ReasoningContext<'a> {
    pub lineage_priors: &'a [FamilyContextLineageOutcome],
    pub multi_horizon_gate: Option<&'a MultiHorizonGate>,
    pub symbol_dimensions: Option<&'a HashMap<Symbol, SymbolDimensions>>,
    pub reviewer_doctrine: Option<&'a ReviewerDoctrinePressure>,
    pub convergence_components: &'a HashMap<Symbol, ConvergenceScore>,
    pub market_regime: &'a MarketRegimeFilter,
    pub world_state: Option<&'a WorldStateSnapshot>,
    pub absence_memory: &'a AbsenceMemory,
    pub family_boost: &'a FamilyBoostLedger,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    // ── AbsenceMemory tests ──

    #[test]
    fn absence_memory_suppresses_after_3_consecutive() {
        let mut mem = AbsenceMemory::default();
        let sector = SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        mem.record_absence(&sector, "Propagation Chain", 1, now);
        mem.record_absence(&sector, "Propagation Chain", 2, now);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
        mem.record_absence(&sector, "Propagation Chain", 3, now);
        assert!(mem.should_suppress(&sector, "Propagation Chain"));
    }

    #[test]
    fn absence_memory_clears_on_propagation() {
        let mut mem = AbsenceMemory::default();
        let sector = SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        mem.record_absence(&sector, "Propagation Chain", 1, now);
        mem.record_absence(&sector, "Propagation Chain", 2, now);
        mem.record_absence(&sector, "Propagation Chain", 3, now);
        assert!(mem.should_suppress(&sector, "Propagation Chain"));
        mem.record_propagation(&sector);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
    }

    #[test]
    fn absence_memory_decays_after_30_min() {
        let mut mem = AbsenceMemory::default();
        let sector = SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        let old = now - time::Duration::minutes(31);
        mem.record_absence(&sector, "Propagation Chain", 1, old);
        mem.record_absence(&sector, "Propagation Chain", 2, old);
        mem.record_absence(&sector, "Propagation Chain", 3, old);
        assert!(mem.should_suppress(&sector, "Propagation Chain"));
        mem.decay(now);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
    }

    // ── FamilyBoostLedger tests ──

    #[test]
    fn family_boost_neutral_below_55_pct() {
        let priors = vec![make_prior("Directed Flow", dec!(0.50), dec!(0.01))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(ledger.boost_for_family("Directed Flow"), Decimal::ONE);
    }

    #[test]
    fn family_boost_caps_at_1_25() {
        let priors = vec![make_prior("Directed Flow", dec!(0.90), dec!(0.05))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(
            ledger.boost_for_family("Directed Flow"),
            Decimal::new(125, 2)
        );
    }

    #[test]
    fn family_boost_requires_positive_net_return() {
        let priors = vec![make_prior("Directed Flow", dec!(0.60), dec!(-0.01))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(ledger.boost_for_family("Directed Flow"), Decimal::ONE);
    }

    fn make_prior(
        family: &str,
        follow_through_rate: Decimal,
        mean_net_return: Decimal,
    ) -> FamilyContextLineageOutcome {
        FamilyContextLineageOutcome {
            family: family.into(),
            session: "midday".into(),
            market_regime: "neutral".into(),
            total: 50,
            resolved: 30,
            hits: 15,
            hit_rate: dec!(0.50),
            mean_return: Decimal::ZERO,
            mean_net_return,
            mean_mfe: Decimal::ZERO,
            mean_mae: Decimal::ZERO,
            follow_through_rate,
            invalidation_rate: Decimal::ZERO,
            structure_retention_rate: Decimal::ZERO,
            mean_convergence_score: Decimal::ZERO,
            mean_external_delta: Decimal::ZERO,
            external_follow_through_rate: Decimal::ZERO,
            follow_expectancy: Decimal::ZERO,
            fade_expectancy: Decimal::ZERO,
            wait_expectancy: Decimal::ZERO,
        }
    }
}
