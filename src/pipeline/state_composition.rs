use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{
    AtomicPredicate, AtomicPredicateKind, CompositeState, CompositeStateKind, GoverningLawKind,
    LawActivation,
};

pub fn compose_law_profile(predicates: &[AtomicPredicate]) -> Vec<LawActivation> {
    let laws = [
        GoverningLawKind::Persistence,
        GoverningLawKind::Propagation,
        GoverningLawKind::AbsorptionRelease,
        GoverningLawKind::CouplingDecoupling,
        GoverningLawKind::Competition,
        GoverningLawKind::ThresholdTransition,
        GoverningLawKind::Invariance,
        GoverningLawKind::ReflexiveCalibration,
    ];

    let mut activations = laws
        .iter()
        .filter_map(|law| {
            let relevant = predicates
                .iter()
                .filter(|predicate| predicate.law == *law)
                .map(|predicate| predicate.score)
                .collect::<Vec<_>>();
            if relevant.is_empty() {
                return None;
            }

            Some(LawActivation {
                kind: *law,
                label: law.label().to_string(),
                score: mean(&relevant),
                summary: law.summary().to_string(),
            })
        })
        .collect::<Vec<_>>();

    activations.sort_by(|left, right| right.score.cmp(&left.score));
    activations
}

pub fn compose_states(predicates: &[AtomicPredicate]) -> Vec<CompositeState> {
    let scores = predicate_scores(predicates);
    let mut states = vec![
        composite_state(
            CompositeStateKind::DirectionalReinforcement,
            &scores,
            &[
                (AtomicPredicateKind::SignalRecurs, dec!(0.35)),
                (AtomicPredicateKind::ConfidenceBuilds, dec!(0.35)),
                (AtomicPredicateKind::PressurePersists, dec!(0.30)),
            ],
        ),
        composite_state(
            CompositeStateKind::CrossScopeContagion,
            &scores,
            &[
                (AtomicPredicateKind::CrossScopePropagation, dec!(0.40)),
                (AtomicPredicateKind::CrossMarketLinkActive, dec!(0.35)),
                (AtomicPredicateKind::SourceConcentrated, dec!(0.25)),
            ],
        ),
        composite_state(
            CompositeStateKind::StructuralFragility,
            &scores,
            &[
                (AtomicPredicateKind::StructuralDegradation, dec!(0.45)),
                (AtomicPredicateKind::StressAccelerating, dec!(0.30)),
                (AtomicPredicateKind::PriceReasoningDivergence, dec!(0.25)),
            ],
        ),
        composite_state(
            CompositeStateKind::MechanisticAmbiguity,
            &scores,
            &[
                (AtomicPredicateKind::LeaderFlipDetected, dec!(0.55)),
                (AtomicPredicateKind::CounterevidencePresent, dec!(0.45)),
            ],
        ),
        composite_state(
            CompositeStateKind::ReflexiveCorrection,
            &scores,
            &[
                (AtomicPredicateKind::HumanRejected, dec!(0.60)),
                (AtomicPredicateKind::CounterevidencePresent, dec!(0.40)),
            ],
        ),
        composite_state(
            CompositeStateKind::EventCatalyst,
            &scores,
            &[
                (AtomicPredicateKind::EventCatalystActive, dec!(0.55)),
                (AtomicPredicateKind::PriceReasoningDivergence, dec!(0.25)),
                (AtomicPredicateKind::CrossScopePropagation, dec!(0.20)),
            ],
        ),
        composite_state(
            CompositeStateKind::LiquidityConstraint,
            &scores,
            &[
                (AtomicPredicateKind::LiquidityImbalance, dec!(0.50)),
                (AtomicPredicateKind::PressurePersists, dec!(0.30)),
                (AtomicPredicateKind::SourceConcentrated, dec!(0.20)),
            ],
        ),
        composite_state(
            CompositeStateKind::ReversionPressure,
            &scores,
            &[
                (AtomicPredicateKind::MeanReversionPressure, dec!(0.55)),
                (AtomicPredicateKind::PriceReasoningDivergence, dec!(0.25)),
                (AtomicPredicateKind::CounterevidencePresent, dec!(0.20)),
            ],
        ),
        composite_state(
            CompositeStateKind::CrossMarketDislocation,
            &scores,
            &[
                (AtomicPredicateKind::CrossMarketDislocation, dec!(0.55)),
                (AtomicPredicateKind::CrossMarketLinkActive, dec!(0.30)),
                (AtomicPredicateKind::PriceReasoningDivergence, dec!(0.15)),
            ],
        ),
        composite_state(
            CompositeStateKind::SubstitutionFlow,
            &scores,
            &[
                (AtomicPredicateKind::SectorRotationPressure, dec!(0.55)),
                (AtomicPredicateKind::PressurePersists, dec!(0.25)),
                (AtomicPredicateKind::CrossScopePropagation, dec!(0.20)),
            ],
        ),
    ];

    states.retain(|state| state.score >= dec!(0.25));
    states.sort_by(|left, right| right.score.cmp(&left.score));
    states
}

fn composite_state(
    kind: CompositeStateKind,
    scores: &HashMap<AtomicPredicateKind, Decimal>,
    weights: &[(AtomicPredicateKind, Decimal)],
) -> CompositeState {
    let score = clamp_unit_interval(weights.iter().fold(
        Decimal::ZERO,
        |acc, (predicate, weight)| {
            acc + scores.get(predicate).copied().unwrap_or(Decimal::ZERO) * *weight
        },
    ));
    let predicates = weights
        .iter()
        .filter_map(|(predicate, _)| {
            let score = scores.get(predicate).copied().unwrap_or(Decimal::ZERO);
            if score >= dec!(0.15) {
                Some(*predicate)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    CompositeState {
        kind,
        label: kind.label().to_string(),
        score,
        summary: kind.summary().to_string(),
        predicates,
    }
}

fn predicate_scores(predicates: &[AtomicPredicate]) -> HashMap<AtomicPredicateKind, Decimal> {
    predicates
        .iter()
        .map(|predicate| (predicate.kind, predicate.score))
        .collect()
}

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    clamp_unit_interval(
        values.iter().fold(Decimal::ZERO, |acc, value| acc + *value)
            / Decimal::from(values.len() as i64),
    )
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::ontology::AtomicPredicate;

    #[test]
    fn states_are_built_from_predicates() {
        let predicates = vec![
            AtomicPredicate {
                kind: AtomicPredicateKind::SignalRecurs,
                label: "Signal Recurs".into(),
                law: GoverningLawKind::Persistence,
                score: dec!(0.8),
                summary: String::new(),
                evidence: vec![],
            },
            AtomicPredicate {
                kind: AtomicPredicateKind::ConfidenceBuilds,
                label: "Confidence Builds".into(),
                law: GoverningLawKind::Persistence,
                score: dec!(0.7),
                summary: String::new(),
                evidence: vec![],
            },
            AtomicPredicate {
                kind: AtomicPredicateKind::PressurePersists,
                label: "Pressure Persists".into(),
                law: GoverningLawKind::Persistence,
                score: dec!(0.6),
                summary: String::new(),
                evidence: vec![],
            },
        ];

        let states = compose_states(&predicates);
        assert!(states
            .iter()
            .any(|state| state.kind == CompositeStateKind::DirectionalReinforcement));
    }
}
