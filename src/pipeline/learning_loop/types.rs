use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReasoningLearningFeedback {
    pub paired_examples: usize,
    pub reinforced_examples: usize,
    pub corrected_examples: usize,
    pub mechanism_adjustments: Vec<LearningAdjustment>,
    pub mechanism_factor_adjustments: Vec<MechanismFactorAdjustment>,
    pub predicate_adjustments: Vec<LearningAdjustment>,
    pub conditioned_adjustments: Vec<ConditionedLearningAdjustment>,
    pub outcome_context: OutcomeLearningContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningAdjustment {
    pub label: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionedLearningAdjustment {
    pub mechanism: String,
    pub scope: String,
    pub conditioned_on: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismFactorAdjustment {
    pub mechanism: String,
    pub factor_key: String,
    pub factor_label: String,
    pub delta: Decimal,
    pub samples: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeLearningContext {
    pub source: String,
    pub reward_multiplier: Decimal,
    pub penalty_multiplier: Decimal,
    pub promoted_follow_through: Decimal,
    pub promoted_retention: Decimal,
    pub promoted_mean_net_return: Decimal,
    pub falsified_invalidation: Decimal,
    pub falsified_follow_through: Decimal,
    pub us_hit_rate: Decimal,
    pub us_mean_return: Decimal,
}

impl Default for OutcomeLearningContext {
    fn default() -> Self {
        Self {
            source: "none".into(),
            reward_multiplier: Decimal::ZERO,
            penalty_multiplier: Decimal::ZERO,
            promoted_follow_through: Decimal::ZERO,
            promoted_retention: Decimal::ZERO,
            promoted_mean_net_return: Decimal::ZERO,
            falsified_invalidation: Decimal::ZERO,
            falsified_follow_through: Decimal::ZERO,
            us_hit_rate: Decimal::ZERO,
            us_mean_return: Decimal::ZERO,
        }
    }
}

impl ReasoningLearningFeedback {
    pub fn mechanism_delta(&self, label: &str) -> Decimal {
        self.mechanism_adjustments
            .iter()
            .find(|item| item.label == label)
            .map(|item| item.delta)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn predicate_delta(&self, label: &str) -> Decimal {
        self.predicate_adjustments
            .iter()
            .find(|item| item.label == label)
            .map(|item| item.delta)
            .unwrap_or(Decimal::ZERO)
    }

    pub fn conditioned_delta(
        &self,
        mechanism: &str,
        active_states: &[String],
        active_predicates: &[String],
    ) -> Decimal {
        let total = self
            .conditioned_adjustments
            .iter()
            .filter(|item| item.mechanism == mechanism)
            .filter(|item| match item.scope.as_str() {
                "state" => active_states
                    .iter()
                    .any(|state| state == &item.conditioned_on),
                "predicate" => active_predicates
                    .iter()
                    .any(|predicate| predicate == &item.conditioned_on),
                _ => false,
            })
            .fold(Decimal::ZERO, |acc, item| acc + item.delta);
        super::feedback::clamp_delta(total)
    }

    pub fn mechanism_factor_lookup(&self) -> HashMap<(String, String), Decimal> {
        self.mechanism_factor_adjustments
            .iter()
            .map(|item| {
                (
                    (item.mechanism.clone(), item.factor_key.clone()),
                    item.delta,
                )
            })
            .collect()
    }
}
