use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{InstitutionId, Symbol};
use crate::pipeline::learning_loop::{
    ConditionedLearningAdjustment, ReasoningLearningFeedback,
};

#[derive(Debug, Clone, Default)]
pub struct AccumulatedKnowledge {
    pub institutional_memory: HashMap<(InstitutionId, Symbol), InstitutionSymbolProfile>,
    pub mechanism_priors: HashMap<String, MechanismPrior>,
    pub calibrated_weights: CalibratedWeights,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstitutionSymbolProfile {
    pub observation_count: u32,
    pub directional_hit_count: u32,
    pub avg_presence_ticks: Decimal,
    pub last_seen_tick: u64,
    pub directional_bias: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MechanismPrior {
    pub hit_rate: Decimal,
    pub sample_count: u32,
    pub regime_hit_rates: HashMap<String, Decimal>,
    pub mean_net_return: Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct CalibratedWeights {
    pub factor_adjustments: HashMap<(String, String), Decimal>,
    pub predicate_adjustments: HashMap<String, Decimal>,
    pub conditioned_adjustments: Vec<ConditionedLearningAdjustment>,
}

impl AccumulatedKnowledge {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn apply_calibration(&mut self, feedback: &ReasoningLearningFeedback) {
        self.calibrated_weights.factor_adjustments = feedback.mechanism_factor_lookup();
        self.calibrated_weights.predicate_adjustments = feedback
            .predicate_adjustments
            .iter()
            .map(|adj| (adj.label.clone(), adj.delta))
            .collect();
        self.calibrated_weights.conditioned_adjustments =
            feedback.conditioned_adjustments.clone();
    }

    pub fn institution_history_bonus(
        &self,
        institution_id: &InstitutionId,
        symbol: &Symbol,
    ) -> Decimal {
        self.institutional_memory
            .get(&(*institution_id, symbol.clone()))
            .map(|profile| {
                if profile.observation_count >= 5 {
                    let hit_rate = Decimal::from(profile.directional_hit_count)
                        / Decimal::from(profile.observation_count);
                    (hit_rate - Decimal::new(5, 1)) * Decimal::new(2, 1)
                } else {
                    Decimal::ZERO
                }
            })
            .unwrap_or(Decimal::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::pipeline::learning_loop::{
        LearningAdjustment, MechanismFactorAdjustment, OutcomeLearningContext,
    };

    #[test]
    fn empty_knowledge_has_no_data() {
        let k = AccumulatedKnowledge::empty();
        assert!(k.institutional_memory.is_empty());
        assert!(k.mechanism_priors.is_empty());
        assert!(k.calibrated_weights.factor_adjustments.is_empty());
    }

    #[test]
    fn institution_profile_hit_rate() {
        let profile = InstitutionSymbolProfile {
            observation_count: 10,
            directional_hit_count: 7,
            avg_presence_ticks: dec!(3.5),
            last_seen_tick: 100,
            directional_bias: dec!(0.6),
        };
        let hit_rate = Decimal::from(profile.directional_hit_count)
            / Decimal::from(profile.observation_count);
        assert_eq!(hit_rate, dec!(0.7));
    }

    #[test]
    fn calibrated_weights_default_is_empty() {
        let w = CalibratedWeights::default();
        assert!(w.factor_adjustments.is_empty());
        assert!(w.predicate_adjustments.is_empty());
        assert!(w.conditioned_adjustments.is_empty());
    }

    #[test]
    fn history_bonus_positive_for_high_hit_rate() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(100);
        let sym = Symbol("700.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 10,
                directional_hit_count: 8,
                avg_presence_ticks: dec!(5.0),
                last_seen_tick: 50,
                directional_bias: dec!(0.7),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        // hit_rate = 0.8, bonus = (0.8 - 0.5) * 0.2 = 0.06
        assert_eq!(bonus, dec!(0.06));
    }

    #[test]
    fn history_bonus_negative_for_low_hit_rate() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(200);
        let sym = Symbol("9988.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 10,
                directional_hit_count: 2,
                avg_presence_ticks: dec!(3.0),
                last_seen_tick: 40,
                directional_bias: dec!(-0.3),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        // hit_rate = 0.2, bonus = (0.2 - 0.5) * 0.2 = -0.06
        assert_eq!(bonus, dec!(-0.06));
    }

    #[test]
    fn history_bonus_zero_when_insufficient_samples() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(300);
        let sym = Symbol("5.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 3,
                directional_hit_count: 3,
                avg_presence_ticks: dec!(2.0),
                last_seen_tick: 10,
                directional_bias: dec!(1.0),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        assert_eq!(bonus, Decimal::ZERO);
    }

    #[test]
    fn history_bonus_zero_when_not_found() {
        let k = AccumulatedKnowledge::empty();
        let bonus = k.institution_history_bonus(
            &InstitutionId(999),
            &Symbol("FAKE.HK".into()),
        );
        assert_eq!(bonus, Decimal::ZERO);
    }

    #[test]
    fn apply_calibration_populates_weights() {
        let mut k = AccumulatedKnowledge::empty();
        let feedback = ReasoningLearningFeedback {
            paired_examples: 5,
            reinforced_examples: 3,
            corrected_examples: 2,
            mechanism_adjustments: vec![LearningAdjustment {
                label: "Narrative Failure".into(),
                delta: dec!(-0.04),
                samples: 2,
            }],
            mechanism_factor_adjustments: vec![MechanismFactorAdjustment {
                mechanism: "Narrative Failure".into(),
                factor_key: "state:reflexive_correction".into(),
                factor_label: "Reflexive Correction".into(),
                delta: dec!(-0.02),
                samples: 2,
            }],
            predicate_adjustments: vec![LearningAdjustment {
                label: "Counterevidence Present".into(),
                delta: dec!(0.03),
                samples: 3,
            }],
            conditioned_adjustments: vec![ConditionedLearningAdjustment {
                mechanism: "Narrative Failure".into(),
                scope: "state".into(),
                conditioned_on: "Reflexive Correction".into(),
                delta: dec!(-0.01),
                samples: 1,
            }],
            outcome_context: OutcomeLearningContext::default(),
        };

        k.apply_calibration(&feedback);

        assert_eq!(k.calibrated_weights.factor_adjustments.len(), 1);
        assert_eq!(
            k.calibrated_weights.factor_adjustments[&(
                "Narrative Failure".into(),
                "state:reflexive_correction".into()
            )],
            dec!(-0.02)
        );
        assert_eq!(k.calibrated_weights.predicate_adjustments.len(), 1);
        assert_eq!(
            k.calibrated_weights.predicate_adjustments["Counterevidence Present"],
            dec!(0.03)
        );
        assert_eq!(k.calibrated_weights.conditioned_adjustments.len(), 1);
    }
}
