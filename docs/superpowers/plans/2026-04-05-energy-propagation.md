# Energy Propagation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make diffusion propagation paths inject energy into ConvergenceScore composites, so hypothesis generation reflects upstream energy flow, not just local neighborhood voting.

**Architecture:** New `NodeEnergyMap` accumulates energy flux per symbol from diffusion paths. After DecisionSnapshot is computed (baseline convergence) and diffusion paths are derived, `apply_energy_to_decision` adjusts convergence_scores in-place. This is a second-pass enrichment — not a change to ConvergenceScore::compute's signature. Also fixes the contradiction damping bug (1.15 → dampening).

**Tech Stack:** Rust, rust_decimal, time

**Spec:** `docs/superpowers/specs/2026-04-05-energy-propagation-design.md`

**Note on architecture:** DecisionSnapshot::compute runs BEFORE diffusion paths exist (paths depend on stock_deltas derived from convergence). So energy can't be injected at compute time. Instead, we apply energy as a post-computation adjustment to the decision, similar to how `apply_polymarket_snapshot` already enriches the decision after compute.

---

### Task 1: Create NodeEnergyMap with tests

**Files:**
- Create: `src/graph/energy.rs`
- Modify: `src/graph/mod.rs`

- [ ] **Step 1: Create energy.rs with tests**

Create `src/graph/energy.rs`:

```rust
use std::collections::HashMap;

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
            // Infer polarity from the first step's source and mechanism confidence sign.
            // Path confidence is always positive (magnitude). The direction comes from
            // whether the source was moving positively or negatively.
            // For simplicity: use the signed confidence of the first step as the polarity signal.
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
        // Blend energy into composite: treat it as an additional component.
        // Current composite is mean of N nonzero components.
        // We add clamped energy as (N+1)th component and recompute the mean.
        let clamped = energy.clamp(-Decimal::ONE, Decimal::ONE);
        let current = score.composite;
        // Simple blend: weighted average of current composite (weight=3, typical component count)
        // and energy (weight=1). This gives energy ~25% influence.
        let blended = (current * Decimal::from(3) + clamped) / Decimal::from(4);
        score.composite = blended;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::ProvenanceMetadata;
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
        // Two paths to 388.HK: 0.3 + 0.2 = 0.5
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
                composite: dec!(0.3), // baseline
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

        // Energy 0.8 blended: (0.3*3 + 0.8)/4 = 1.7/4 = 0.425
        assert!(adjusted > baseline, "energy should increase composite");
        assert_eq!(adjusted, dec!(0.425));
    }
}
```

- [ ] **Step 2: Add module to graph/mod.rs**

In `src/graph/mod.rs`, add:

```rust
pub mod energy;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib -- graph::energy::tests -q`
Expected: 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/graph/energy.rs src/graph/mod.rs
git commit -m "$(cat <<'EOF'
feat(graph): add NodeEnergyMap — diffusion path energy accumulation

NodeEnergyMap accumulates energy flux per symbol from PropagationPaths.
apply_energy_to_convergence blends energy into ConvergenceScore composites
as a second-pass enrichment after baseline computation.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Fix contradiction damping bug

**Files:**
- Modify: `src/pipeline/reasoning/propagation.rs`

- [ ] **Step 1: Fix diffusion_lag_factor**

In `src/pipeline/reasoning/propagation.rs`, find `diffusion_lag_factor` (line 403). Change the `opposite_direction_bonus` to a penalty:

Replace:

```rust
    let opposite_direction_bonus =
        if target_delta != Decimal::ZERO && source_delta.signum() != target_delta.signum() {
            Decimal::new(15, 2)
        } else {
            Decimal::ZERO
        };

    (Decimal::ONE - absorbed + opposite_direction_bonus).clamp(Decimal::new(15, 2), Decimal::ONE)
```

With:

```rust
    let opposite_direction_penalty =
        if target_delta != Decimal::ZERO && source_delta.signum() != target_delta.signum() {
            Decimal::new(15, 2)
        } else {
            Decimal::ZERO
        };

    (Decimal::ONE - absorbed - opposite_direction_penalty).clamp(Decimal::new(15, 2), Decimal::ONE)
```

The only change: `+` becomes `-`. Energy meeting contradiction is now dampened instead of amplified.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/reasoning/propagation.rs
git commit -m "$(cat <<'EOF'
fix(propagation): dampen energy on contradiction instead of amplifying

diffusion_lag_factor previously added 0.15 bonus when target moved
opposite to source (amplifying contradictions). Now subtracts 0.15
(dampening contradictions). Energy should dissipate at resistance, not grow.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Wire energy into reasoning pipeline

**Files:**
- Modify: `src/pipeline/reasoning.rs`

- [ ] **Step 1: Build NodeEnergyMap and apply to decision in derive_with_diffusion**

In `src/pipeline/reasoning.rs`, in `derive_with_diffusion` (line 184), after the propagation_paths are derived (line 195-196) and before hypotheses are generated, build the energy map and apply it to the decision:

Find:
```rust
        let propagation_paths =
            derive_diffusion_propagation_paths(brain, stock_deltas, decision.timestamp);
        let family_gate = (!ctx.lineage_priors.is_empty()).then(|| {
```

Change the method to take `decision` as mutable (`&mut DecisionSnapshot` instead of `&DecisionSnapshot`). Actually — `derive_with_diffusion` takes `decision: &DecisionSnapshot` (immutable). We need to clone and mutate. Add after the propagation_paths line:

```rust
        let propagation_paths =
            derive_diffusion_propagation_paths(brain, stock_deltas, decision.timestamp);

        // Apply diffusion energy to convergence scores
        let energy_map =
            crate::graph::energy::NodeEnergyMap::from_propagation_paths(&propagation_paths);
        let mut decision = decision.clone();
        if !energy_map.is_empty() {
            crate::graph::energy::apply_energy_to_convergence(
                &mut decision.convergence_scores,
                &energy_map,
            );
        }
```

Then update all subsequent references to `decision` in the function — they already use `decision.xxx` so the owned clone will work. The key references are:
- `decision.timestamp` (line ~various)
- `decision.convergence_scores` (in SetupSupportContext)
- `decision.market_regime` (in apply_track_action_policy)
- `decision.order_suggestions` (in derive_investigation_selections)

Since `decision` is now an owned `DecisionSnapshot` (cloned), all `&decision.xxx` references still work.

**Important**: Also need to update the function signature. Currently:

```rust
    pub fn derive_with_diffusion(
        ...
        decision: &DecisionSnapshot,
        ...
    ) -> Self {
```

This is fine — we clone inside. No signature change needed.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/reasoning.rs
git commit -m "$(cat <<'EOF'
feat(reasoning): apply diffusion energy to convergence scores

derive_with_diffusion now builds NodeEnergyMap from propagation paths
and blends energy into decision.convergence_scores before hypothesis
generation. Convergence composites reflect upstream energy flow.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Final verification

- [ ] **Step 1: Full cargo check**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 2: Run all energy + edge_learning tests**

Run: `cargo test --lib -- graph::energy -q` → expect 3 pass
Run: `cargo test --lib -- graph::edge_learning -q` → expect 6 pass

- [ ] **Step 3: Run full test suite**

Run: `cargo test --lib 2>&1 | grep "test result:"` → expect 760+ passed

- [ ] **Step 4: Push**

```bash
git push origin codex/polymarket-convergence:main
```

- [ ] **Step 5: Git log**

Expected 3 new commits:
1. `feat(graph): add NodeEnergyMap — diffusion path energy accumulation`
2. `fix(propagation): dampen energy on contradiction instead of amplifying`
3. `feat(reasoning): apply diffusion energy to convergence scores`
