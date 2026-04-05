use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{PropagationPath, ReasoningScope};

/// Accumulated energy flux per symbol from diffusion propagation paths.
/// Positive = bullish energy arriving, negative = bearish.
#[derive(Debug, Clone, Default)]
pub struct NodeEnergyMap {
    flux: HashMap<Symbol, Decimal>,
}

impl NodeEnergyMap {
    /// Build from propagation paths. For each path, the last step's target
    /// receives energy = path.confidence * polarity (inferred from step direction).
    pub fn from_propagation_paths(paths: &[PropagationPath]) -> Self {
        let mut flux: HashMap<Symbol, Decimal> = HashMap::new();
        for path in paths {
            let Some(last_step) = path.steps.last() else {
                continue;
            };
            let Some(symbol) = scope_symbol(&last_step.to) else {
                continue;
            };
            let polarity = path
                .steps
                .first()
                .map(|step| step.confidence.signum())
                .unwrap_or(Decimal::ONE);
            let energy = path.confidence * polarity;
            *flux.entry(symbol).or_insert(Decimal::ZERO) += energy;
        }
        Self { flux }
    }

    /// Get energy flux for a symbol. Returns 0 if no energy.
    pub fn energy_for(&self, symbol: &Symbol) -> Decimal {
        self.flux.get(symbol).copied().unwrap_or(Decimal::ZERO)
    }

    /// Number of symbols with nonzero energy.
    pub fn len(&self) -> usize {
        self.flux.len()
    }

    pub fn is_empty(&self) -> bool {
        self.flux.is_empty()
    }
}

fn scope_symbol(scope: &ReasoningScope) -> Option<Symbol> {
    match scope {
        ReasoningScope::Symbol(s) => Some(s.clone()),
        _ => None,
    }
}

/// Apply energy flux to an existing set of convergence scores.
/// This is a second-pass enrichment: after DecisionSnapshot computes baseline
/// convergence, diffusion paths produce energy, and this function blends
/// that energy into the composite.
pub fn apply_energy_to_convergence(
    convergence_scores: &mut HashMap<Symbol, crate::graph::convergence::ConvergenceScore>,
    energy_map: &NodeEnergyMap,
) {
    for (symbol, score) in convergence_scores.iter_mut() {
        let energy = energy_map.energy_for(symbol);
        if energy == Decimal::ZERO {
            continue;
        }
        let clamped = energy.clamp(-Decimal::ONE, Decimal::ONE);
        let current = score.composite;
        let blended = (current * Decimal::from(3) + clamped) / Decimal::from(4);
        score.composite = blended;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::reasoning::PropagationStep;
    use rust_decimal_macros::dec;

    fn make_path(from: &str, to: &str, confidence: Decimal) -> PropagationPath {
        PropagationPath {
            path_id: format!("path:{}→{}", from, to),
            summary: format!("{} → {}", from, to),
            confidence: confidence.abs(),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(Symbol(from.into())),
                to: ReasoningScope::Symbol(Symbol(to.into())),
                mechanism: "diffusion".into(),
                confidence,
                references: vec![],
            }],
        }
    }

    #[test]
    fn energy_map_accumulates_from_paths() {
        let paths = vec![
            make_path("700.HK", "388.HK", dec!(0.3)),
            make_path("1810.HK", "388.HK", dec!(0.2)),
        ];
        let map = NodeEnergyMap::from_propagation_paths(&paths);
        assert_eq!(map.energy_for(&Symbol("388.HK".into())), dec!(0.5));
    }

    #[test]
    fn energy_map_returns_zero_for_unknown_symbol() {
        let map = NodeEnergyMap::default();
        assert_eq!(
            map.energy_for(&Symbol("UNKNOWN".into())),
            Decimal::ZERO
        );
    }

    #[test]
    fn apply_energy_blends_into_composite() {
        use crate::graph::convergence::ConvergenceScore;

        let mut scores = HashMap::new();
        scores.insert(
            Symbol("700.HK".into()),
            ConvergenceScore {
                symbol: Symbol("700.HK".into()),
                institutional_alignment: dec!(0.4),
                sector_coherence: Some(dec!(0.3)),
                cross_stock_correlation: dec!(0.2),
                composite: dec!(0.3),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        );

        let mut flux = HashMap::new();
        flux.insert(Symbol("700.HK".into()), dec!(0.8));
        let energy_map = NodeEnergyMap { flux };

        let baseline = scores[&Symbol("700.HK".into())].composite;
        apply_energy_to_convergence(&mut scores, &energy_map);
        let adjusted = scores[&Symbol("700.HK".into())].composite;

        assert!(adjusted > baseline, "energy should increase composite");
        assert_eq!(adjusted, dec!(0.425));
    }
}
