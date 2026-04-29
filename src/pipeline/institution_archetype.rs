//! Institution-level belief — aggregate broker archetypes up to KG
//! Institution entity.
//!
//! Each `Institution` in ObjectStore has 1..N brokers (trading desks).
//! `BrokerArchetypeBeliefField` gives per-broker posteriors; this
//! module rolls those up to a per-institution archetype distribution.
//!
//! Why aggregation, not a separate-source belief: broker behavior IS
//! the institution's behavior — a Goldman desk accumulating on 0700.HK
//! is Goldman accumulating 0700.HK. The institution-level posterior is
//! a sample-weighted average of its brokers' posteriors.
//!
//! Why at all: decision layer can ask "what's Barclays Asia doing
//! today?" as a single query, without iterating brokers. Also, KG
//! knowledge_link records use institution as a node kind ("institution
//! X holds symbol Y") — eventually the institution's belief posterior
//! can weight knowledge-link confidence.
//!
//! This module is **stateless** — aggregation is recomputed on demand
//! rather than maintained as a separate field, because broker beliefs
//! update every tick and keeping a second mirror synchronized would
//! be wasted work. Call `compute_institution_belief` when you need it.

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;

use crate::ontology::objects::{Institution, InstitutionId};
use crate::pipeline::broker_archetype::{
    BrokerArchetype, BrokerArchetypeBeliefField, BROKER_ARCHETYPE_VARIANTS,
};

/// Per-broker informed threshold before it contributes to the roll-up.
const MIN_BROKER_SAMPLES_FOR_INSTITUTION: u32 = 20;

/// Minimum informed brokers contributing to an institution's verdict
/// before `top_confident` surfaces it.
const MIN_INSTITUTION_INFORMED_BROKERS: usize = 2;

/// Institution-level belief summary. Built on demand from broker
/// archetype posteriors. Not a CategoricalBelief directly because it
/// carries extra metadata (broker counts, informed counts) beyond the
/// posterior itself.
#[derive(Debug, Clone)]
pub struct InstitutionArchetypeVerdict {
    pub institution_id: InstitutionId,
    /// Institution's aggregate posterior over the 5 BrokerArchetype
    /// variants, in canonical BROKER_ARCHETYPE_VARIANTS order. Sums
    /// to 1 within floating-point tolerance.
    pub posterior: [f64; 5],
    /// Archetype with highest posterior probability.
    pub dominant: BrokerArchetype,
    /// Probability of the dominant archetype.
    pub dominant_probability: f64,
    pub total_brokers: usize,
    pub informed_brokers: usize,
    /// Sum of sample_counts across contributing brokers.
    pub effective_samples: u32,
}

/// Compute one institution's aggregate archetype verdict. Returns None
/// when the institution has no informed brokers.
pub fn compute_institution_belief(
    institution: &Institution,
    broker_field: &BrokerArchetypeBeliefField,
) -> Option<InstitutionArchetypeVerdict> {
    let total_brokers = institution.broker_ids.len();
    let mut informed_brokers = 0usize;
    let mut effective_samples: u64 = 0;
    let mut weighted_mass = [0.0_f64; 5];

    for broker_id in &institution.broker_ids {
        let Some(belief) = broker_field.query(*broker_id) else {
            continue;
        };
        if belief.sample_count < MIN_BROKER_SAMPLES_FOR_INSTITUTION {
            continue;
        }
        informed_brokers += 1;
        let weight = belief.sample_count as f64;
        effective_samples += belief.sample_count as u64;
        // For each archetype, add weight × its probability.
        for (i, archetype) in BROKER_ARCHETYPE_VARIANTS.iter().enumerate() {
            let p = belief
                .variants
                .iter()
                .position(|v| v == archetype)
                .and_then(|idx| belief.probs.get(idx))
                .and_then(|p| p.to_f64())
                .unwrap_or(0.0);
            weighted_mass[i] += weight * p;
        }
    }

    if informed_brokers == 0 {
        return None;
    }

    // Normalise to posterior.
    let total_weight: f64 = weighted_mass.iter().sum();
    let mut posterior = [0.0_f64; 5];
    if total_weight > 0.0 {
        for (i, m) in weighted_mass.iter().enumerate() {
            posterior[i] = m / total_weight;
        }
    }

    // Pick dominant.
    let (dominant_idx, dominant_probability) =
        posterior
            .iter()
            .enumerate()
            .fold(
                (0usize, 0.0_f64),
                |(bi, bp), (i, p)| {
                    if *p > bp {
                        (i, *p)
                    } else {
                        (bi, bp)
                    }
                },
            );
    let dominant = BROKER_ARCHETYPE_VARIANTS[dominant_idx];

    Some(InstitutionArchetypeVerdict {
        institution_id: institution.id,
        posterior,
        dominant,
        dominant_probability,
        total_brokers,
        informed_brokers,
        effective_samples: effective_samples.min(u32::MAX as u64) as u32,
    })
}

/// Compute verdicts for all institutions in a map, return top-K by
/// dominant-probability × log(effective_samples). Enforces
/// min-informed-brokers and min-dominance guards.
pub fn top_confident_institutions(
    institutions: &HashMap<InstitutionId, Institution>,
    broker_field: &BrokerArchetypeBeliefField,
    k: usize,
    min_dominance: f64,
) -> Vec<InstitutionArchetypeVerdict> {
    let mut out: Vec<InstitutionArchetypeVerdict> = institutions
        .values()
        .filter_map(|inst| compute_institution_belief(inst, broker_field))
        .filter(|v| {
            v.informed_brokers >= MIN_INSTITUTION_INFORMED_BROKERS
                && v.dominant_probability >= min_dominance
        })
        .collect();
    out.sort_by(|a, b| {
        let score_b = b.dominant_probability * (b.effective_samples as f64 + 1.0).ln();
        let score_a = a.dominant_probability * (a.effective_samples as f64 + 1.0).ln();
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(k);
    out
}

fn archetype_name(a: BrokerArchetype) -> &'static str {
    match a {
        BrokerArchetype::Accumulative => "accumulative",
        BrokerArchetype::Distributive => "distributive",
        BrokerArchetype::Arbitrage => "arbitrage",
        BrokerArchetype::Algo => "algo",
        BrokerArchetype::Unknown => "unknown",
    }
}

pub fn format_institution_archetype_line(v: &InstitutionArchetypeVerdict, name: &str) -> String {
    format!(
        "institution_archetype: {} {} {} {:.2} via {}/{} brokers (n_eff={})",
        v.institution_id.0,
        name,
        archetype_name(v.dominant),
        v.dominant_probability,
        v.informed_brokers,
        v.total_brokers,
        v.effective_samples,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::ontology::objects::{BrokerId, InstitutionClass, Market, Symbol};
    use crate::pipeline::raw_expectation::RawBrokerPresence;

    fn mk_institution(id: i32, broker_ids: Vec<i32>) -> Institution {
        Institution {
            id: InstitutionId(id),
            name_en: format!("Inst-{id}"),
            name_cn: format!("Inst-{id}"),
            name_hk: format!("Inst-{id}"),
            broker_ids: broker_ids.into_iter().map(BrokerId).collect::<HashSet<_>>(),
            class: InstitutionClass::InvestmentBank,
        }
    }

    fn seed_field_with_brokers(
        broker_archetype_pairs: &[(i32, BrokerArchetype)],
    ) -> BrokerArchetypeBeliefField {
        let mut field = BrokerArchetypeBeliefField::new(Market::Hk);
        let mut presence = RawBrokerPresence::default();
        // 30 ticks so each broker crosses MIN_BROKER_SAMPLES_FOR_INSTITUTION=20.
        for _ in 0..30 {
            for (broker_id, archetype) in broker_archetype_pairs {
                let per_sym = presence
                    .per_symbol
                    .entry(Symbol("T.HK".into()))
                    .or_default();
                match archetype {
                    BrokerArchetype::Accumulative => {
                        per_sym.bid.entry(*broker_id).or_default().push(true);
                        per_sym.ask.entry(*broker_id).or_default().push(false);
                    }
                    BrokerArchetype::Distributive => {
                        per_sym.bid.entry(*broker_id).or_default().push(false);
                        per_sym.ask.entry(*broker_id).or_default().push(true);
                    }
                    _ => {}
                }
            }
            field.observe_tick(&presence);
        }
        field
    }

    #[test]
    fn institution_with_no_informed_brokers_returns_none() {
        let inst = mk_institution(1, vec![111, 112]);
        let field = BrokerArchetypeBeliefField::new(Market::Hk);
        assert!(compute_institution_belief(&inst, &field).is_none());
    }

    #[test]
    fn institution_aggregates_across_same_archetype_brokers() {
        let field = seed_field_with_brokers(&[
            (101, BrokerArchetype::Accumulative),
            (102, BrokerArchetype::Accumulative),
            (103, BrokerArchetype::Accumulative),
        ]);
        let inst = mk_institution(1, vec![101, 102, 103]);
        let v = compute_institution_belief(&inst, &field).unwrap();
        assert_eq!(v.dominant, BrokerArchetype::Accumulative);
        assert_eq!(v.informed_brokers, 3);
        assert_eq!(v.total_brokers, 3);
        assert!(v.dominant_probability > 0.35);
        // Posterior sums to ~1.
        let sum: f64 = v.posterior.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn institution_with_mixed_brokers_reflects_blend() {
        let field = seed_field_with_brokers(&[
            (201, BrokerArchetype::Accumulative),
            (202, BrokerArchetype::Distributive),
            (203, BrokerArchetype::Accumulative),
        ]);
        let inst = mk_institution(2, vec![201, 202, 203]);
        let v = compute_institution_belief(&inst, &field).unwrap();
        // 2/3 accumulative, 1/3 distributive → accumulative dominant
        // but not overwhelmingly so.
        assert_eq!(v.dominant, BrokerArchetype::Accumulative);
        assert_eq!(v.informed_brokers, 3);
        let accum_idx = BROKER_ARCHETYPE_VARIANTS
            .iter()
            .position(|a| *a == BrokerArchetype::Accumulative)
            .unwrap();
        let dist_idx = BROKER_ARCHETYPE_VARIANTS
            .iter()
            .position(|a| *a == BrokerArchetype::Distributive)
            .unwrap();
        assert!(v.posterior[accum_idx] > v.posterior[dist_idx]);
        assert!(
            v.posterior[dist_idx] > 0.1,
            "distributive should have non-trivial mass, got {}",
            v.posterior[dist_idx]
        );
    }

    #[test]
    fn top_confident_respects_min_informed_brokers() {
        let field = seed_field_with_brokers(&[
            (301, BrokerArchetype::Accumulative), // only this one is informed
        ]);
        let inst_small = mk_institution(1, vec![301]); // only 1 broker
        let inst_empty = mk_institution(2, vec![]); // 0 brokers
        let mut insts = HashMap::new();
        insts.insert(inst_small.id, inst_small);
        insts.insert(inst_empty.id, inst_empty);

        let top = top_confident_institutions(&insts, &field, 10, 0.3);
        // inst_small has only 1 informed broker — below MIN_INSTITUTION_INFORMED_BROKERS=2.
        assert!(top.is_empty());
    }

    #[test]
    fn top_confident_ranks_by_effective_samples() {
        let field = seed_field_with_brokers(&[
            // inst 1: 2 brokers, 30 samples each = 60 eff samples
            (401, BrokerArchetype::Accumulative),
            (402, BrokerArchetype::Accumulative),
            // inst 2: 4 brokers same archetype = 120 eff samples
            (411, BrokerArchetype::Accumulative),
            (412, BrokerArchetype::Accumulative),
            (413, BrokerArchetype::Accumulative),
            (414, BrokerArchetype::Accumulative),
        ]);
        let inst1 = mk_institution(1, vec![401, 402]);
        let inst2 = mk_institution(2, vec![411, 412, 413, 414]);
        let mut insts = HashMap::new();
        insts.insert(inst1.id, inst1);
        insts.insert(inst2.id, inst2);

        let top = top_confident_institutions(&insts, &field, 5, 0.3);
        assert_eq!(top.len(), 2);
        // Inst 2 should rank first (more effective samples, same dominance).
        assert_eq!(top[0].institution_id, InstitutionId(2));
        assert_eq!(top[1].institution_id, InstitutionId(1));
    }

    #[test]
    fn format_line_is_greppable() {
        let v = InstitutionArchetypeVerdict {
            institution_id: InstitutionId(2040),
            posterior: [0.68, 0.15, 0.08, 0.05, 0.04],
            dominant: BrokerArchetype::Accumulative,
            dominant_probability: 0.68,
            total_brokers: 12,
            informed_brokers: 8,
            effective_samples: 240,
        };
        let line = format_institution_archetype_line(&v, "Barclays Asia");
        assert_eq!(
            line,
            "institution_archetype: 2040 Barclays Asia accumulative 0.68 via 8/12 brokers (n_eff=240)"
        );
    }
}
