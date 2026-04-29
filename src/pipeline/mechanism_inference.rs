use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{
    AtomicPredicate, AtomicPredicateKind, CaseReasoningProfile, CompositeState, CompositeStateKind,
    HumanReviewContext, MechanismCandidate, MechanismCandidateKind, MechanismCounterfactual,
    MechanismFactor, MechanismFactorSource,
};
use crate::pipeline::state_composition::{compose_law_profile, compose_states};

#[derive(Clone, Copy)]
enum FactorSelector {
    State(CompositeStateKind),
    Clarity,
}

#[derive(Clone, Copy)]
struct RecipeFactor {
    key: &'static str,
    label: &'static str,
    source: MechanismFactorSource,
    selector: FactorSelector,
    weight: Decimal,
}

#[derive(Clone)]
struct MechanismRecipe {
    kind: MechanismCandidateKind,
    factors: Vec<RecipeFactor>,
}

pub fn build_reasoning_profile(
    predicates: &[crate::ontology::AtomicPredicate],
    invalidation_rules: &[String],
    human_review: Option<HumanReviewContext>,
) -> CaseReasoningProfile {
    let laws = compose_law_profile(predicates);
    let states = compose_states(predicates);
    let (primary_mechanism, competing_mechanisms) =
        infer_mechanisms_with_factor_adjustments(&states, invalidation_rules, &HashMap::new());

    let automated_invalidations = check_mechanism_invalidations(predicates, &states)
        .into_iter()
        .map(|(kind, reason)| crate::ontology::MechanismInvalidation {
            mechanism: kind.label().to_string(),
            reason,
        })
        .collect();

    CaseReasoningProfile {
        laws,
        predicates: predicates.to_vec(),
        composite_states: states,
        human_review,
        primary_mechanism,
        competing_mechanisms,
        automated_invalidations,
    }
}

pub fn build_reasoning_profile_with_adjustments(
    predicates: &[crate::ontology::AtomicPredicate],
    invalidation_rules: &[String],
    human_review: Option<HumanReviewContext>,
    factor_adjustments: &HashMap<(String, String), Decimal>,
) -> CaseReasoningProfile {
    let laws = compose_law_profile(predicates);
    let states = compose_states(predicates);
    let (primary_mechanism, competing_mechanisms) =
        infer_mechanisms_with_factor_adjustments(&states, invalidation_rules, factor_adjustments);

    let automated_invalidations = check_mechanism_invalidations(predicates, &states)
        .into_iter()
        .map(|(kind, reason)| crate::ontology::MechanismInvalidation {
            mechanism: kind.label().to_string(),
            reason,
        })
        .collect();

    CaseReasoningProfile {
        laws,
        predicates: predicates.to_vec(),
        composite_states: states,
        human_review,
        primary_mechanism,
        competing_mechanisms,
        automated_invalidations,
    }
}

pub fn infer_mechanisms(
    states: &[CompositeState],
    invalidation_rules: &[String],
) -> (Option<MechanismCandidate>, Vec<MechanismCandidate>) {
    infer_mechanisms_with_factor_adjustments(states, invalidation_rules, &HashMap::new())
}

pub fn infer_mechanisms_with_factor_adjustments(
    states: &[CompositeState],
    invalidation_rules: &[String],
    factor_adjustments: &HashMap<(String, String), Decimal>,
) -> (Option<MechanismCandidate>, Vec<MechanismCandidate>) {
    let scores = state_scores(states);
    let clarity = Decimal::ONE
        - scores
            .get(&CompositeStateKind::MechanisticAmbiguity)
            .copied()
            .unwrap_or(Decimal::ZERO);

    let mut candidates = recipes()
        .into_iter()
        .map(|recipe| {
            score_recipe(
                &recipe,
                &scores,
                clarity,
                invalidation_rules,
                factor_adjustments,
            )
        })
        .collect::<Vec<_>>();
    let viability_floor = explanatory_score_floor(&candidates);

    let best_scores = candidates.iter().map(|item| item.score).collect::<Vec<_>>();
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let best_other_score = best_scores
            .iter()
            .enumerate()
            .filter(|(other_index, _)| *other_index != index)
            .map(|(_, score)| *score)
            .max()
            .unwrap_or(Decimal::ZERO);
        candidate.counterfactuals = build_counterfactuals(
            candidate.score,
            &candidate.factors,
            best_other_score,
            viability_floor,
        );
    }

    retain_explanatory_mechanisms(&mut candidates);

    if candidates.is_empty() {
        return (None, Vec::new());
    }

    let primary = candidates.first().cloned();
    let competing = candidates.into_iter().skip(1).take(3).collect::<Vec<_>>();
    (primary, competing)
}

fn recipes() -> Vec<MechanismRecipe> {
    vec![
        MechanismRecipe {
            kind: MechanismCandidateKind::MechanicalExecutionSignature,
            factors: vec![
                state_factor(
                    CompositeStateKind::DirectionalReinforcement,
                    "state:directional_reinforcement",
                    dec!(0.45),
                ),
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.35),
                ),
                clarity_factor(dec!(0.20)),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::FragilityBuildUp,
            factors: vec![
                state_factor(
                    CompositeStateKind::StructuralFragility,
                    "state:structural_fragility",
                    dec!(0.60),
                ),
                state_factor(
                    CompositeStateKind::MechanisticAmbiguity,
                    "state:mechanistic_ambiguity",
                    dec!(0.25),
                ),
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.15),
                ),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::ContagionOnset,
            factors: vec![
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.50),
                ),
                state_factor(
                    CompositeStateKind::StructuralFragility,
                    "state:structural_fragility",
                    dec!(0.40),
                ),
                state_factor(
                    CompositeStateKind::DirectionalReinforcement,
                    "state:directional_reinforcement",
                    dec!(0.10),
                ),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::NarrativeFailure,
            factors: vec![
                state_factor(
                    CompositeStateKind::MechanisticAmbiguity,
                    "state:mechanistic_ambiguity",
                    dec!(0.50),
                ),
                state_factor(
                    CompositeStateKind::StructuralFragility,
                    "state:structural_fragility",
                    dec!(0.30),
                ),
                state_factor(
                    CompositeStateKind::ReflexiveCorrection,
                    "state:reflexive_correction",
                    dec!(0.20),
                ),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::LiquidityTrap,
            factors: vec![
                state_factor(
                    CompositeStateKind::LiquidityConstraint,
                    "state:liquidity_constraint",
                    dec!(0.55),
                ),
                state_factor(
                    CompositeStateKind::StructuralFragility,
                    "state:structural_fragility",
                    dec!(0.20),
                ),
                state_factor(
                    CompositeStateKind::DirectionalReinforcement,
                    "state:directional_reinforcement",
                    dec!(0.15),
                ),
                clarity_factor(dec!(0.10)),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::EventDrivenDislocation,
            factors: vec![
                state_factor(
                    CompositeStateKind::EventCatalyst,
                    "state:event_catalyst",
                    dec!(0.60),
                ),
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.15),
                ),
                state_factor(
                    CompositeStateKind::MechanisticAmbiguity,
                    "state:mechanistic_ambiguity",
                    dec!(0.15),
                ),
                clarity_factor(dec!(0.10)),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::MeanReversionSnapback,
            factors: vec![
                state_factor(
                    CompositeStateKind::ReversionPressure,
                    "state:reversion_pressure",
                    dec!(0.55),
                ),
                state_factor(
                    CompositeStateKind::MechanisticAmbiguity,
                    "state:mechanistic_ambiguity",
                    dec!(0.20),
                ),
                state_factor(
                    CompositeStateKind::StructuralFragility,
                    "state:structural_fragility",
                    dec!(0.15),
                ),
                clarity_factor(dec!(0.10)),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::ArbitrageConvergence,
            factors: vec![
                state_factor(
                    CompositeStateKind::CrossMarketDislocation,
                    "state:cross_market_dislocation",
                    dec!(0.60),
                ),
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.20),
                ),
                state_factor(
                    CompositeStateKind::DirectionalReinforcement,
                    "state:directional_reinforcement",
                    dec!(0.10),
                ),
                clarity_factor(dec!(0.10)),
            ],
        },
        MechanismRecipe {
            kind: MechanismCandidateKind::CapitalRotation,
            factors: vec![
                state_factor(
                    CompositeStateKind::SubstitutionFlow,
                    "state:substitution_flow",
                    dec!(0.60),
                ),
                state_factor(
                    CompositeStateKind::DirectionalReinforcement,
                    "state:directional_reinforcement",
                    dec!(0.15),
                ),
                state_factor(
                    CompositeStateKind::CrossScopeContagion,
                    "state:cross_scope_contagion",
                    dec!(0.15),
                ),
                clarity_factor(dec!(0.10)),
            ],
        },
    ]
}

fn state_factor(kind: CompositeStateKind, key: &'static str, weight: Decimal) -> RecipeFactor {
    RecipeFactor {
        key,
        label: kind.label(),
        source: MechanismFactorSource::State,
        selector: FactorSelector::State(kind),
        weight,
    }
}

fn clarity_factor(weight: Decimal) -> RecipeFactor {
    RecipeFactor {
        key: "derived:clarity",
        label: "Clarity",
        source: MechanismFactorSource::Derived,
        selector: FactorSelector::Clarity,
        weight,
    }
}

fn score_recipe(
    recipe: &MechanismRecipe,
    scores: &HashMap<CompositeStateKind, Decimal>,
    clarity: Decimal,
    invalidation_rules: &[String],
    factor_adjustments: &HashMap<(String, String), Decimal>,
) -> MechanismCandidate {
    let lookup_key = recipe.kind.label().to_string();
    let mut raw_factors = recipe
        .factors
        .iter()
        .map(|spec| {
            let activation = match spec.selector {
                FactorSelector::State(kind) => scores.get(&kind).copied().unwrap_or(Decimal::ZERO),
                FactorSelector::Clarity => clarity,
            };
            let learned_delta = factor_adjustments
                .get(&(lookup_key.clone(), spec.key.to_string()))
                .copied()
                .unwrap_or(Decimal::ZERO);
            (
                *spec,
                activation,
                learned_delta,
                clamp_non_negative(spec.weight + learned_delta),
            )
        })
        .collect::<Vec<_>>();

    let total_raw_weight = raw_factors
        .iter()
        .fold(Decimal::ZERO, |acc, (_, _, _, weight)| acc + *weight);
    let fallback_total = recipe
        .factors
        .iter()
        .fold(Decimal::ZERO, |acc, factor| acc + factor.weight);

    let mut factors = raw_factors
        .drain(..)
        .map(|(spec, activation, learned_delta, raw_weight)| {
            let effective_weight = if total_raw_weight > Decimal::ZERO {
                raw_weight / total_raw_weight
            } else if fallback_total > Decimal::ZERO {
                spec.weight / fallback_total
            } else {
                Decimal::ZERO
            };
            MechanismFactor {
                key: spec.key.to_string(),
                label: spec.label.to_string(),
                source: spec.source,
                activation,
                base_weight: spec.weight,
                learned_weight_delta: learned_delta,
                effective_weight,
                contribution: clamp_unit_interval(activation * effective_weight),
            }
        })
        .collect::<Vec<_>>();

    factors.sort_by(|left, right| {
        right
            .contribution
            .cmp(&left.contribution)
            .then_with(|| left.label.cmp(&right.label))
    });

    let score = clamp_unit_interval(
        factors
            .iter()
            .fold(Decimal::ZERO, |acc, factor| acc + factor.contribution),
    );
    let supporting_states = factors
        .iter()
        .filter_map(|factor| {
            if factor.source != MechanismFactorSource::State || factor.activation < dec!(0.15) {
                return None;
            }
            factor_state_kind(&factor.key)
        })
        .collect::<Vec<_>>();

    let mut invalidation = recipe
        .kind
        .invalidation()
        .iter()
        .map(|item| item.to_string())
        .collect::<Vec<_>>();
    invalidation.extend(invalidation_rules.iter().take(2).cloned());

    MechanismCandidate {
        kind: recipe.kind,
        label: recipe.kind.label().to_string(),
        score,
        summary: recipe.kind.summary().to_string(),
        supporting_states,
        invalidation,
        human_checks: recipe
            .kind
            .human_checks()
            .iter()
            .map(|item| item.to_string())
            .collect(),
        factors,
        counterfactuals: Vec::new(),
    }
}

fn build_counterfactuals(
    score: Decimal,
    factors: &[MechanismFactor],
    best_other_score: Decimal,
    viability_floor: Decimal,
) -> Vec<MechanismCounterfactual> {
    factors
        .iter()
        .filter(|factor| factor.contribution > Decimal::ZERO)
        .take(3)
        .map(|factor| {
            let adjusted_score = clamp_unit_interval(score - factor.contribution);
            MechanismCounterfactual {
                factor_key: factor.key.clone(),
                factor_label: factor.label.clone(),
                scenario: format!("remove {}", factor.label),
                adjusted_score,
                score_delta: adjusted_score - score,
                remains_viable: adjusted_score >= viability_floor,
                remains_primary: adjusted_score >= best_other_score,
            }
        })
        .collect()
}

pub fn retain_explanatory_mechanisms(candidates: &mut Vec<MechanismCandidate>) {
    candidates.sort_by(|left, right| right.score.cmp(&left.score));
    let floor = explanatory_score_floor(candidates);
    candidates.retain(|candidate| candidate.score >= floor);
}

fn check_mechanism_invalidations(
    predicates: &[AtomicPredicate],
    states: &[CompositeState],
) -> Vec<(MechanismCandidateKind, String)> {
    let pred = |kind: AtomicPredicateKind| -> Decimal {
        predicates
            .iter()
            .find(|p| p.kind == kind)
            .map(|p| p.score)
            .unwrap_or(Decimal::ZERO)
    };
    let state = |kind: CompositeStateKind| -> Decimal {
        states
            .iter()
            .find(|s| s.kind == kind)
            .map(|s| s.score)
            .unwrap_or(Decimal::ZERO)
    };

    let mut invalidated = Vec::new();

    if pred(AtomicPredicateKind::SourceConcentrated) < dec!(0.20)
        && pred(AtomicPredicateKind::PressurePersists) < dec!(0.20)
    {
        invalidated.push((
            MechanismCandidateKind::MechanicalExecutionSignature,
            "主導 source 已分散且壓力未持續".into(),
        ));
    }

    if pred(AtomicPredicateKind::StressAccelerating) < dec!(0.15)
        && pred(AtomicPredicateKind::StructuralDegradation) < dec!(0.20)
    {
        invalidated.push((
            MechanismCandidateKind::FragilityBuildUp,
            "stress 已回落且結構退化緩解".into(),
        ));
    }

    if pred(AtomicPredicateKind::CrossScopePropagation) < dec!(0.20)
        && state(CompositeStateKind::CrossScopeContagion) < dec!(0.25)
    {
        invalidated.push((
            MechanismCandidateKind::ContagionOnset,
            "跨範圍傳播已消退".into(),
        ));
    }

    if pred(AtomicPredicateKind::PriceReasoningDivergence) < dec!(0.20) {
        invalidated.push((
            MechanismCandidateKind::NarrativeFailure,
            "價格與推理的分歧已收窄".into(),
        ));
    }

    if pred(AtomicPredicateKind::LiquidityImbalance) < dec!(0.20)
        && state(CompositeStateKind::LiquidityConstraint) < dec!(0.25)
    {
        invalidated.push((
            MechanismCandidateKind::LiquidityTrap,
            "流動性失衡已正常化".into(),
        ));
    }

    if pred(AtomicPredicateKind::EventCatalystActive) < dec!(0.20) {
        invalidated.push((
            MechanismCandidateKind::EventDrivenDislocation,
            "事件催化已消退".into(),
        ));
    }

    if pred(AtomicPredicateKind::MeanReversionPressure) < dec!(0.20)
        && state(CompositeStateKind::ReversionPressure) < dec!(0.25)
    {
        invalidated.push((
            MechanismCandidateKind::MeanReversionSnapback,
            "均值回歸壓力已消退".into(),
        ));
    }

    if pred(AtomicPredicateKind::CrossMarketLinkActive) < dec!(0.20)
        && pred(AtomicPredicateKind::CrossMarketDislocation) < dec!(0.20)
    {
        invalidated.push((
            MechanismCandidateKind::ArbitrageConvergence,
            "跨市場連結已失活".into(),
        ));
    }

    if pred(AtomicPredicateKind::SectorRotationPressure) < dec!(0.20)
        && state(CompositeStateKind::SubstitutionFlow) < dec!(0.25)
    {
        invalidated.push((
            MechanismCandidateKind::CapitalRotation,
            "板塊輪動壓力已消退".into(),
        ));
    }

    invalidated
}

fn explanatory_score_floor(candidates: &[MechanismCandidate]) -> Decimal {
    // Use the current case's score distribution rather than a fixed magic cutoff.
    let mut positive_scores = candidates
        .iter()
        .map(|candidate| candidate.score)
        .filter(|score| *score > Decimal::ZERO)
        .collect::<Vec<_>>();
    if positive_scores.is_empty() {
        return Decimal::ZERO;
    }

    positive_scores.sort();
    let mean = positive_scores
        .iter()
        .fold(Decimal::ZERO, |acc, score| acc + *score)
        / Decimal::from(positive_scores.len() as i64);
    let median = if positive_scores.len() % 2 == 1 {
        positive_scores[positive_scores.len() / 2]
    } else {
        let upper = positive_scores[positive_scores.len() / 2];
        let lower = positive_scores[(positive_scores.len() / 2) - 1];
        (upper + lower) / Decimal::from(2)
    };

    mean.max(median)
}

fn state_scores(states: &[CompositeState]) -> HashMap<CompositeStateKind, Decimal> {
    states
        .iter()
        .map(|state| (state.kind, state.score))
        .collect::<HashMap<_, _>>()
}

fn factor_state_kind(key: &str) -> Option<CompositeStateKind> {
    match key {
        "state:directional_reinforcement" => Some(CompositeStateKind::DirectionalReinforcement),
        "state:cross_scope_contagion" => Some(CompositeStateKind::CrossScopeContagion),
        "state:structural_fragility" => Some(CompositeStateKind::StructuralFragility),
        "state:mechanistic_ambiguity" => Some(CompositeStateKind::MechanisticAmbiguity),
        "state:reflexive_correction" => Some(CompositeStateKind::ReflexiveCorrection),
        "state:event_catalyst" => Some(CompositeStateKind::EventCatalyst),
        "state:liquidity_constraint" => Some(CompositeStateKind::LiquidityConstraint),
        "state:reversion_pressure" => Some(CompositeStateKind::ReversionPressure),
        "state:cross_market_dislocation" => Some(CompositeStateKind::CrossMarketDislocation),
        "state:substitution_flow" => Some(CompositeStateKind::SubstitutionFlow),
        _ => None,
    }
}

fn clamp_non_negative(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        Decimal::ZERO
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn mechanisms_are_ranked_from_states() {
        let states = vec![
            CompositeState {
                kind: CompositeStateKind::StructuralFragility,
                label: "Structural Fragility".into(),
                score: dec!(0.82),
                summary: String::new(),
                predicates: vec![],
            },
            CompositeState {
                kind: CompositeStateKind::CrossScopeContagion,
                label: "Cross-scope Contagion".into(),
                score: dec!(0.77),
                summary: String::new(),
                predicates: vec![],
            },
            CompositeState {
                kind: CompositeStateKind::CrossMarketDislocation,
                label: "Cross-market Dislocation".into(),
                score: dec!(0.71),
                summary: String::new(),
                predicates: vec![],
            },
        ];

        let (primary, competing) = infer_mechanisms(&states, &[]);
        assert!(primary.is_some());
        assert!(!competing.is_empty());
    }

    #[test]
    fn counterfactuals_and_factors_are_emitted() {
        let states = vec![
            CompositeState {
                kind: CompositeStateKind::EventCatalyst,
                label: "Event Catalyst".into(),
                score: dec!(0.90),
                summary: String::new(),
                predicates: vec![],
            },
            CompositeState {
                kind: CompositeStateKind::MechanisticAmbiguity,
                label: "Mechanistic Ambiguity".into(),
                score: dec!(0.20),
                summary: String::new(),
                predicates: vec![],
            },
        ];

        let (primary, _) = infer_mechanisms(&states, &[]);
        let primary = primary.expect("primary mechanism");
        assert!(!primary.factors.is_empty());
        assert!(!primary.counterfactuals.is_empty());
    }
}
