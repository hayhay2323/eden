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

// ── Energy Momentum (cross-tick resonance) ──

/// Cross-tick energy accumulator. Consecutive ticks of same-direction energy
/// resonate (amplitude grows), while single-tick noise decays quickly.
/// Runtime holds this across ticks, similar to AbsenceMemory.
#[derive(Debug, Clone, Default)]
pub struct EnergyMomentum {
    state: HashMap<Symbol, Decimal>,
}

impl EnergyMomentum {
    /// Blend current tick's energy into momentum state.
    /// `momentum = momentum * decay + new_energy * (1 - decay)`
    ///
    /// With decay=0.7:
    /// - 3 ticks same direction → ~2.3x amplification
    /// - Single tick noise → 70% after 1 tick, 49% after 2
    pub fn update(&mut self, energy_map: &NodeEnergyMap, decay: Decimal) {
        let blend = Decimal::ONE - decay;
        // Decay existing momentum
        for value in self.state.values_mut() {
            *value *= decay;
        }
        // Blend in new energy
        for (symbol, &new_energy) in &energy_map.flux {
            let entry = self.state.entry(symbol.clone()).or_insert(Decimal::ZERO);
            *entry += new_energy * blend;
        }
        // Remove negligible entries
        self.state
            .retain(|_, value| value.abs() >= Decimal::new(1, 3));
    }

    /// Get accumulated momentum for a symbol.
    pub fn momentum_for(&self, symbol: &Symbol) -> Decimal {
        self.state.get(symbol).copied().unwrap_or(Decimal::ZERO)
    }

    /// Convert momentum state to a NodeEnergyMap for use with apply_energy_to_convergence.
    pub fn as_energy_map(&self) -> NodeEnergyMap {
        NodeEnergyMap {
            flux: self.state.clone(),
        }
    }

    pub fn len(&self) -> usize {
        self.state.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }
}

/// Apply energy flux to an existing set of convergence scores.
/// This is a second-pass enrichment: after DecisionSnapshot computes baseline
/// convergence, diffusion paths produce energy, and this function blends
/// that energy into the composite.
pub fn apply_energy_to_convergence(
    convergence_scores: &mut HashMap<Symbol, crate::graph::convergence::ConvergenceScore>,
    momentum: &EnergyMomentum,
) {
    for (symbol, score) in convergence_scores.iter_mut() {
        let energy = momentum.momentum_for(symbol);
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

        // Build an EnergyMomentum with pre-seeded state via update
        let mut momentum = EnergyMomentum::default();
        let mut flux = HashMap::new();
        flux.insert(Symbol("700.HK".into()), dec!(0.8));
        let energy_map = NodeEnergyMap { flux };
        // Use decay=0 so momentum = 0*existing + 1.0*new = new energy exactly
        momentum.update(&energy_map, dec!(0.0));

        let baseline = scores[&Symbol("700.HK".into())].composite;
        apply_energy_to_convergence(&mut scores, &momentum);
        let adjusted = scores[&Symbol("700.HK".into())].composite;

        assert!(adjusted > baseline, "energy should increase composite");
        assert_eq!(adjusted, dec!(0.425));
    }

    // ── EnergyMomentum tests ──

    #[test]
    fn momentum_resonates_on_consecutive_same_direction() {
        let mut momentum = EnergyMomentum::default();
        let sym = Symbol("700.HK".into());
        let decay = dec!(0.7);

        // Tick 1: inject 0.5 energy
        let mut flux1 = HashMap::new();
        flux1.insert(sym.clone(), dec!(0.5));
        let map1 = NodeEnergyMap { flux: flux1 };
        momentum.update(&map1, decay);
        let after_1 = momentum.momentum_for(&sym);

        // Tick 2: inject another 0.5 same direction
        let mut flux2 = HashMap::new();
        flux2.insert(sym.clone(), dec!(0.5));
        let map2 = NodeEnergyMap { flux: flux2 };
        momentum.update(&map2, decay);
        let after_2 = momentum.momentum_for(&sym);

        // Tick 3: inject another 0.5
        momentum.update(&map2, decay);
        let after_3 = momentum.momentum_for(&sym);

        assert!(after_2 > after_1, "momentum should grow: {} > {}", after_2, after_1);
        assert!(after_3 > after_2, "momentum should keep growing: {} > {}", after_3, after_2);
    }

    #[test]
    fn momentum_decays_without_new_energy() {
        let mut momentum = EnergyMomentum::default();
        let sym = Symbol("700.HK".into());
        let decay = dec!(0.7);

        // Inject energy once
        let mut flux = HashMap::new();
        flux.insert(sym.clone(), dec!(0.5));
        let map = NodeEnergyMap { flux };
        momentum.update(&map, decay);
        let after_inject = momentum.momentum_for(&sym);

        // Two ticks with no energy
        let empty = NodeEnergyMap::default();
        momentum.update(&empty, decay);
        let after_1_decay = momentum.momentum_for(&sym);
        momentum.update(&empty, decay);
        let after_2_decay = momentum.momentum_for(&sym);

        assert!(after_1_decay < after_inject, "should decay");
        assert!(after_2_decay < after_1_decay, "should decay further");
    }

    #[test]
    fn momentum_as_energy_map_works() {
        let mut momentum = EnergyMomentum::default();
        let sym = Symbol("700.HK".into());
        let mut flux = HashMap::new();
        flux.insert(sym.clone(), dec!(0.4));
        let map = NodeEnergyMap { flux };
        momentum.update(&map, dec!(0.7));

        let converted = momentum.as_energy_map();
        assert!(converted.energy_for(&sym) > Decimal::ZERO);
    }
}
