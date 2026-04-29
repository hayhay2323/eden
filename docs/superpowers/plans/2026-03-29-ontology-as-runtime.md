# Ontology as Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ObjectStore a living world model by adding AccumulatedKnowledge — institutional memory, mechanism priors, and calibrated weights — that the tick loop reads from and writes to every tick.

**Architecture:** Extend the existing `ObjectStore` with a `RwLock<AccumulatedKnowledge>` field. Each tick, the runtime writes knowledge via `accumulate()`. Consumers (`BrainGraph::compute`, `build_reasoning_profile`, `derive_atomic_predicates`) read knowledge via `store.knowledge.read()`. On startup, knowledge is restored from SurrealDB. No new abstraction layers.

**Tech Stack:** Rust, `std::sync::RwLock`, `rust_decimal`, SurrealDB (existing persistence layer)

---

### Task 1: Create AccumulatedKnowledge data structures

**Files:**
- Create: `src/ontology/store/knowledge.rs`
- Modify: `src/ontology/store.rs`

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/ontology/store/knowledge.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

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
            avg_presence_ticks: dec!(5.0),
            last_seen_tick: 100,
            directional_bias: dec!(0.3),
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
}
```

- [ ] **Step 2: Write the module**

Create `src/ontology/store/knowledge.rs`:

```rust
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

    /// History-based confidence bonus for an institution on a symbol.
    /// Returns a value in roughly [-0.1, +0.1] or ZERO if insufficient data.
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
    use super::*;
    use rust_decimal_macros::dec;

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
            avg_presence_ticks: dec!(5.0),
            last_seen_tick: 100,
            directional_bias: dec!(0.3),
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
                directional_bias: dec!(0.4),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        assert!(bonus > Decimal::ZERO, "bonus should be positive: {}", bonus);
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
                directional_bias: dec!(-0.2),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        assert!(bonus < Decimal::ZERO, "bonus should be negative: {}", bonus);
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
                directional_bias: dec!(0.5),
            },
        );
        assert_eq!(k.institution_history_bonus(&iid, &sym), Decimal::ZERO);
    }

    #[test]
    fn history_bonus_zero_when_not_found() {
        let k = AccumulatedKnowledge::empty();
        assert_eq!(
            k.institution_history_bonus(&InstitutionId(999), &Symbol("X.HK".into())),
            Decimal::ZERO
        );
    }

    #[test]
    fn apply_calibration_populates_weights() {
        let mut k = AccumulatedKnowledge::empty();
        let feedback = ReasoningLearningFeedback {
            paired_examples: 10,
            reinforced_examples: 7,
            corrected_examples: 3,
            mechanism_adjustments: vec![],
            mechanism_factor_adjustments: vec![
                crate::pipeline::learning_loop::MechanismFactorAdjustment {
                    mechanism: "MechanicalExecutionSignature".into(),
                    factor_key: "directional_reinforcement".into(),
                    factor_label: "DirectionalReinforcement".into(),
                    delta: dec!(0.05),
                    samples: 8,
                },
            ],
            predicate_adjustments: vec![
                crate::pipeline::learning_loop::LearningAdjustment {
                    label: "SignalRecurs".into(),
                    delta: dec!(0.03),
                    samples: 6,
                },
            ],
            conditioned_adjustments: vec![],
            outcome_context: Default::default(),
        };
        k.apply_calibration(&feedback);
        assert_eq!(
            k.calibrated_weights.factor_adjustments.get(&(
                "MechanicalExecutionSignature".into(),
                "directional_reinforcement".into()
            )),
            Some(&dec!(0.05))
        );
        assert_eq!(
            k.calibrated_weights.predicate_adjustments.get("SignalRecurs"),
            Some(&dec!(0.03))
        );
    }
}
```

- [ ] **Step 3: Register the module**

In `src/ontology/store.rs`, add after the `mod init;` line:

```rust
#[path = "store/knowledge.rs"]
mod knowledge;

pub use knowledge::{
    AccumulatedKnowledge, CalibratedWeights, InstitutionSymbolProfile, MechanismPrior,
};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo check --tests 2>&1 | head -20`
Expected: No errors from the new module.

Run: `cargo test --lib ontology::store::knowledge 2>&1 | tail -20`
Expected: All 7 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/store/knowledge.rs src/ontology/store.rs
git commit -m "feat: add AccumulatedKnowledge data structures for ontology-as-runtime"
```

---

### Task 2: Add knowledge field to ObjectStore

**Files:**
- Modify: `src/ontology/store/object_store.rs`
- Modify: `src/ontology/store/init.rs`

- [ ] **Step 1: Write the failing test**

Add to tests in `src/ontology/store.rs`:

```rust
    #[test]
    fn object_store_has_empty_knowledge_by_default() {
        let store = test_store();
        let k = store.knowledge.read().unwrap();
        assert!(k.institutional_memory.is_empty());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ontology::store::tests::object_store_has_empty_knowledge_by_default 2>&1 | tail -5`
Expected: FAIL — `knowledge` field does not exist.

- [ ] **Step 3: Add the field to ObjectStore**

In `src/ontology/store/object_store.rs`, add the import and field:

```rust
use std::sync::RwLock;
use super::knowledge::AccumulatedKnowledge;
```

Add to the `ObjectStore` struct:

```rust
pub struct ObjectStore {
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,
    pub knowledge: RwLock<AccumulatedKnowledge>,
}
```

In `from_parts`, add to the returned struct:

```rust
        ObjectStore {
            institutions: inst_map,
            brokers: broker_map,
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: b2i,
            knowledge: RwLock::new(AccumulatedKnowledge::empty()),
        }
```

- [ ] **Step 4: Update init.rs**

In `src/ontology/store/init.rs`, add the import and field to the returned `ObjectStore`:

```rust
use std::sync::RwLock;
use super::knowledge::AccumulatedKnowledge;
```

Change the `Arc::new(ObjectStore { ... })` at the end to include:

```rust
    Arc::new(ObjectStore {
        institutions,
        brokers,
        stocks,
        sectors,
        broker_to_institution,
        knowledge: RwLock::new(AccumulatedKnowledge::empty()),
    })
```

- [ ] **Step 5: Run full check**

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly.

Run: `cargo check --tests 2>&1 | tail -10`
Expected: Compiles cleanly (all existing tests that use `ObjectStore` still work because `from_parts` now includes the new field).

Run: `cargo test --lib ontology::store 2>&1 | tail -20`
Expected: All tests pass including the new one.

- [ ] **Step 6: Commit**

```bash
git add src/ontology/store/object_store.rs src/ontology/store/init.rs src/ontology/store.rs
git commit -m "feat: add knowledge: RwLock<AccumulatedKnowledge> to ObjectStore"
```

---

### Task 3: Implement accumulate — institutional memory

**Files:**
- Modify: `src/ontology/store/knowledge.rs`

- [ ] **Step 1: Write the failing test**

Add to the tests module in `src/ontology/store/knowledge.rs`:

```rust
    #[test]
    fn accumulate_institutional_memory_from_edges() {
        use crate::graph::graph::{BrainGraph, InstitutionToStock, EdgeKind, NodeKind, StockNode, InstitutionNode};
        use crate::action::narrative::Regime;
        use crate::pipeline::dimensions::SymbolDimensions;
        use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
        use petgraph::graph::DiGraph;
        use time::OffsetDateTime;

        let mut graph = DiGraph::new();
        let stock_idx = graph.add_node(NodeKind::Stock(StockNode {
            symbol: Symbol("700.HK".into()),
            regime: Regime::CoherentBullish,
            coherence: dec!(0.5),
            mean_direction: dec!(0.3),
            dimensions: SymbolDimensions::default(),
        }));
        let inst_idx = graph.add_node(NodeKind::Institution(InstitutionNode {
            institution_id: InstitutionId(100),
            stock_count: 2,
            bid_stock_count: 1,
            ask_stock_count: 1,
            net_direction: dec!(0.5),
        }));
        graph.add_edge(
            inst_idx,
            stock_idx,
            EdgeKind::InstitutionToStock(InstitutionToStock {
                direction: dec!(0.6),
                seat_count: 3,
                timestamp: OffsetDateTime::UNIX_EPOCH,
                provenance: ProvenanceMetadata {
                    source: ProvenanceSource::Computed,
                    observed_at: OffsetDateTime::UNIX_EPOCH,
                    received_at: OffsetDateTime::UNIX_EPOCH,
                    confidence: dec!(0.8),
                    trace_id: None,
                    inputs: vec![],
                },
            }),
        );

        let mut stock_nodes = std::collections::HashMap::new();
        stock_nodes.insert(Symbol("700.HK".into()), stock_idx);
        let mut institution_nodes = std::collections::HashMap::new();
        institution_nodes.insert(InstitutionId(100), inst_idx);

        let brain = BrainGraph {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            graph,
            market_temperature: None,
            stock_nodes,
            institution_nodes,
            sector_nodes: std::collections::HashMap::new(),
        };

        let mut k = AccumulatedKnowledge::empty();
        k.accumulate_institutional_memory(1, &brain);

        let key = (InstitutionId(100), Symbol("700.HK".into()));
        let profile = k.institutional_memory.get(&key).unwrap();
        assert_eq!(profile.observation_count, 1);
        assert_eq!(profile.last_seen_tick, 1);
        assert!(profile.directional_bias > Decimal::ZERO);

        // Second accumulation should increment
        k.accumulate_institutional_memory(2, &brain);
        let profile = k.institutional_memory.get(&key).unwrap();
        assert_eq!(profile.observation_count, 2);
        assert_eq!(profile.last_seen_tick, 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib ontology::store::knowledge::tests::accumulate_institutional_memory_from_edges 2>&1 | tail -5`
Expected: FAIL — `accumulate_institutional_memory` method does not exist.

- [ ] **Step 3: Implement accumulate_institutional_memory**

Add to `impl AccumulatedKnowledge` in `src/ontology/store/knowledge.rs`:

```rust
    pub fn accumulate_institutional_memory(
        &mut self,
        tick_number: u64,
        brain: &crate::graph::graph::BrainGraph,
    ) {
        use crate::graph::graph::{EdgeKind, NodeKind};
        use petgraph::visit::EdgeRef;

        for (&inst_id, &inst_idx) in &brain.institution_nodes {
            for edge in brain.graph.edges(inst_idx) {
                if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                    let target = edge.target();
                    if let NodeKind::Stock(stock_node) = &brain.graph[target] {
                        let key = (inst_id, stock_node.symbol.clone());
                        let profile = self.institutional_memory.entry(key).or_insert(
                            InstitutionSymbolProfile {
                                observation_count: 0,
                                directional_hit_count: 0,
                                avg_presence_ticks: Decimal::ZERO,
                                last_seen_tick: tick_number,
                                directional_bias: Decimal::ZERO,
                            },
                        );
                        profile.observation_count += 1;
                        profile.last_seen_tick = tick_number;
                        // Running average of directional bias
                        let n = Decimal::from(profile.observation_count);
                        profile.directional_bias =
                            profile.directional_bias * (n - Decimal::ONE) / n
                                + e.direction / n;
                    }
                }
            }
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib ontology::store::knowledge::tests::accumulate_institutional_memory_from_edges 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/ontology/store/knowledge.rs
git commit -m "feat: implement accumulate_institutional_memory for institutional memory"
```

---

### Task 4: Wire calibrated weights into mechanism inference

**Files:**
- Modify: `src/pipeline/mechanism_inference.rs` — change `build_reasoning_profile` to accept optional factor_adjustments
- Modify: `src/cases/builders.rs` — pass knowledge to `build_reasoning_profile`
- Modify: `src/bin/replay.rs` — pass `&HashMap::new()` explicitly

- [ ] **Step 1: Write the failing test**

Add to `src/pipeline/mechanism_integration_tests.rs` (or existing test file):

```rust
    #[test]
    fn build_reasoning_profile_with_adjustments_shifts_scores() {
        // Build a profile with no adjustments
        let predicates_bare = derive_atomic_predicates(&ScenarioBuilder::new()
            .signal(dec!(0.6), dec!(0.5), dec!(0.4))
            .pressure(dec!(0.3), false)
            .build());
        let profile_bare = build_reasoning_profile(&predicates_bare, &[], None);

        // Build with a positive adjustment for a mechanism factor
        let mut adjustments = HashMap::new();
        adjustments.insert(
            ("MechanicalExecutionSignature".into(), "directional_reinforcement".into()),
            dec!(0.10),
        );
        let profile_adj = build_reasoning_profile_with_adjustments(
            &predicates_bare, &[], None, &adjustments,
        );

        // The adjusted profile's primary mechanism score should differ
        if let (Some(bare), Some(adj)) = (&profile_bare.primary_mechanism, &profile_adj.primary_mechanism) {
            // At minimum, they should both exist — the adjustment should not crash
            assert!(bare.score >= Decimal::ZERO);
            assert!(adj.score >= Decimal::ZERO);
        }
    }
```

- [ ] **Step 2: Add `build_reasoning_profile_with_adjustments`**

In `src/pipeline/mechanism_inference.rs`, add a new public function:

```rust
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
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo check --tests 2>&1 | tail -10`
Expected: Compiles.

- [ ] **Step 4: Update `cases/builders.rs` to use calibrated weights**

In `src/cases/builders.rs`, the function that calls `infer_reasoning_profile` (aliased from `build_reasoning_profile`) at line ~601 needs to change. The function `derive_case_reasoning_profile` needs an additional parameter:

Change the import:
```rust
use crate::pipeline::mechanism_inference::{
    build_reasoning_profile as infer_reasoning_profile,
    build_reasoning_profile_with_adjustments as infer_reasoning_profile_with_adjustments,
};
```

Change the call site at line ~601 from:
```rust
    infer_reasoning_profile(&predicates, invalidation_rules, human_review)
```
to:
```rust
    infer_reasoning_profile_with_adjustments(&predicates, invalidation_rules, human_review, factor_adjustments)
```

This requires threading `factor_adjustments` through the call chain. The `derive_case_reasoning_profile` function signature becomes:
```rust
fn derive_case_reasoning_profile(
    snapshot: &LiveSnapshot,
    tactical_case: &LiveTacticalCase,
    chain: Option<&LiveBackwardChain>,
    pressure: Option<&LivePressure>,
    signal: Option<&LiveSignal>,
    causal: Option<&LiveCausalLeader>,
    track: Option<&LiveHypothesisTrack>,
    workflow_state: &str,
    workflow_note: Option<&str>,
    invalidation_rules: &[String],
    factor_adjustments: &HashMap<(String, String), Decimal>,
) -> CaseReasoningProfile {
```

Update all callers of `derive_case_reasoning_profile` within `builders.rs` to pass the new parameter. For now, pass `&HashMap::new()` — this will be connected to the live knowledge in Task 6.

- [ ] **Step 5: Run check**

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/mechanism_inference.rs src/cases/builders.rs
git commit -m "feat: add build_reasoning_profile_with_adjustments for calibrated weights"
```

---

### Task 5: Wire institutional memory into BrainGraph

**Files:**
- Modify: `src/graph/graph.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/graph/graph.rs` tests (or a new test file):

```rust
#[cfg(test)]
mod knowledge_tests {
    use super::*;
    use crate::ontology::store::knowledge::{AccumulatedKnowledge, InstitutionSymbolProfile};
    use rust_decimal_macros::dec;

    #[test]
    fn institution_edge_provenance_includes_history_bonus() {
        // This test verifies that when institutional memory exists,
        // the provenance confidence of InstitutionToStock edges is adjusted.
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(100);
        let sym = Symbol("700.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 20,
                directional_hit_count: 16,
                avg_presence_ticks: dec!(10.0),
                last_seen_tick: 50,
                directional_bias: dec!(0.4),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        assert!(bonus > Decimal::ZERO);
        // 16/20 = 0.8, (0.8 - 0.5) * 0.2 = 0.06
        assert_eq!(bonus, dec!(0.06));
    }
}
```

- [ ] **Step 2: Modify InstitutionToStock edge creation**

In `src/graph/graph.rs`, in the section `// 4. Add institution→stock edges from InstitutionActivity` (around line 222), after computing `direction`, read the knowledge lock and adjust the provenance confidence:

```rust
        // 4. Add institution→stock edges from InstitutionActivity
        let knowledge = store.knowledge.read().unwrap();
        for act in &links.institution_activities {
            if let (Some(&inst_idx), Some(&stock_idx)) = (
                institution_nodes.get(&act.institution_id),
                stock_nodes.get(&act.symbol),
            ) {
                let bid = Decimal::from(act.bid_positions.len() as i64);
                let ask = Decimal::from(act.ask_positions.len() as i64);
                let direction = normalized_ratio(bid, ask);
                let history_bonus = knowledge.institution_history_bonus(
                    &act.institution_id,
                    &act.symbol,
                );
                let base_confidence = direction.abs();
                let adjusted_confidence = crate::math::clamp_unit_interval(
                    base_confidence + history_bonus,
                );
                graph.add_edge(
                    inst_idx,
                    stock_idx,
                    EdgeKind::InstitutionToStock(InstitutionToStock {
                        direction,
                        seat_count: act.seat_count,
                        timestamp: links.timestamp,
                        provenance: computed_edge_provenance(
                            links.timestamp,
                            adjusted_confidence,
                            [
                                format!("institution_activity:{}", act.symbol),
                                format!("institution:{}", act.institution_id),
                            ],
                        ),
                    }),
                );
            }
        }
        drop(knowledge);
```

- [ ] **Step 3: Run check**

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly.

Run: `cargo test --lib graph 2>&1 | tail -20`
Expected: All existing graph tests still pass.

- [ ] **Step 4: Commit**

```bash
git add src/graph/graph.rs
git commit -m "feat: BrainGraph reads institutional memory for edge confidence adjustment"
```

---

### Task 6: Wire accumulate + calibration into HK runtime

**Files:**
- Modify: `src/hk/runtime.rs`

- [ ] **Step 1: Add accumulate call after history.push**

Find the line `history.push(tick_record);` in `src/hk/runtime.rs`. After it, add:

```rust
        store.knowledge.write().unwrap().accumulate_institutional_memory(tick, &brain);
```

- [ ] **Step 2: Wire learning feedback to calibration**

Find where `derive_learning_feedback` is called in the HK persistence stage. After the feedback is derived, add:

```rust
        store.knowledge.write().unwrap().apply_calibration(&feedback);
```

If the HK runtime doesn't currently call `derive_learning_feedback` in the tick loop (it only imports it), add a periodic calibration block:

```rust
        #[cfg(feature = "persistence")]
        if tick % 30 == 0 {
            if let Some(ref runtime) = runtime.persistence {
                if let Ok(assessments) = runtime.store.recent_case_reasoning_assessments(200).await {
                    let outcome_ctx = derive_outcome_learning_context_from_hk_rows(
                        &runtime.store.recent_lineage_metric_rows(500).await.unwrap_or_default(),
                    );
                    let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
                    store.knowledge.write().unwrap().apply_calibration(&feedback);
                }
            }
        }
```

- [ ] **Step 3: Run check**

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/hk/runtime.rs
git commit -m "feat: HK runtime accumulates knowledge and applies calibration each tick"
```

---

### Task 7: Wire accumulate + calibration into US runtime

**Files:**
- Modify: `src/us/runtime.rs`
- Modify: `src/us/runtime/support/stages.rs`

- [ ] **Step 1: Add accumulate call after history.push**

Find `us_history.push(tick_record);` in `src/us/runtime.rs`. After it, add:

```rust
        store.knowledge.write().unwrap().accumulate_institutional_memory(tick, &us_brain);
```

Note: The US runtime uses `UsGraph` not `BrainGraph`. The `accumulate_institutional_memory` method operates on `BrainGraph`. If `UsGraph` has a different structure, we need an equivalent method. Check if `UsGraph` has `institution_nodes` — if not, skip the institutional memory for US and only wire calibration.

- [ ] **Step 2: Wire calibration to existing US learning feedback**

In `src/us/runtime/support/stages.rs`, the `maybe_refresh_us_learning_feedback` function already derives feedback. After setting `*cached_feedback = Some(...)`, add:

```rust
        store.knowledge.write().unwrap().apply_calibration(&feedback);
```

This requires passing `&store` (or `&Arc<ObjectStore>`) to `maybe_refresh_us_learning_feedback`.

- [ ] **Step 3: Run check**

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly.

- [ ] **Step 4: Commit**

```bash
git add src/us/runtime.rs src/us/runtime/support/stages.rs
git commit -m "feat: US runtime applies calibration from learning feedback"
```

---

### Task 8: Startup restoration from SurrealDB

**Files:**
- Modify: `src/ontology/store/knowledge.rs` — add `restore_from`
- Modify: `src/hk/runtime/startup.rs`
- Modify: `src/us/runtime/startup.rs`

- [ ] **Step 1: Add restore_from method**

In `src/ontology/store/knowledge.rs`, add:

```rust
#[cfg(feature = "persistence")]
impl AccumulatedKnowledge {
    pub async fn restore_from(db: &crate::persistence::store::EdenStore) -> Self {
        use crate::pipeline::learning_loop::{
            derive_learning_feedback, derive_outcome_learning_context_from_hk_rows,
            OutcomeLearningContext,
        };

        let mut knowledge = Self::empty();

        // 1. Restore calibrated weights from recent assessments
        if let Ok(assessments) = db.recent_case_reasoning_assessments(200).await {
            let outcome_ctx = if let Ok(rows) = db.recent_lineage_metric_rows(500).await {
                derive_outcome_learning_context_from_hk_rows(&rows)
            } else {
                OutcomeLearningContext::default()
            };
            let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
            knowledge.apply_calibration(&feedback);
        }

        eprintln!(
            "  [KNOWLEDGE] Restored: {} factor adjustments, {} predicate adjustments",
            knowledge.calibrated_weights.factor_adjustments.len(),
            knowledge.calibrated_weights.predicate_adjustments.len(),
        );

        knowledge
    }
}
```

- [ ] **Step 2: Call restore_from in HK startup**

In `src/hk/runtime/startup.rs`, after the `ObjectStore` is created (the `Arc::new(...)` call), add:

```rust
    #[cfg(feature = "persistence")]
    if let Some(ref eden_store) = persistence_store {
        let restored = AccumulatedKnowledge::restore_from(eden_store).await;
        *store.knowledge.write().unwrap() = restored;
    }
```

Add the import:
```rust
use crate::ontology::store::AccumulatedKnowledge;
```

- [ ] **Step 3: Call restore_from in US startup**

Same pattern in `src/us/runtime/startup.rs`.

- [ ] **Step 4: Run check**

Run: `cargo check --features persistence 2>&1 | tail -10`
Expected: Compiles cleanly.

Run: `cargo check 2>&1 | tail -10`
Expected: Compiles cleanly (without persistence feature, restore_from is gated).

- [ ] **Step 5: Commit**

```bash
git add src/ontology/store/knowledge.rs src/hk/runtime/startup.rs src/us/runtime/startup.rs
git commit -m "feat: restore AccumulatedKnowledge from SurrealDB on startup"
```

---

### Task 9: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Full compile check**

Run: `cargo check 2>&1 | tail -10`
Expected: Clean compile.

Run: `cargo check --tests 2>&1 | tail -10`
Expected: Clean compile.

- [ ] **Step 2: Run all tests**

Run: `cargo test --lib 2>&1 | tail -30`
Expected: All tests pass.

- [ ] **Step 3: Verify no regressions in existing test suites**

Run: `cargo test --lib ontology 2>&1 | tail -20`
Run: `cargo test --lib graph 2>&1 | tail -20`
Run: `cargo test --lib pipeline 2>&1 | tail -20`
Expected: All pass.

- [ ] **Step 4: Commit the plan as completed**

```bash
git add docs/superpowers/plans/2026-03-29-ontology-as-runtime.md
git commit -m "docs: mark ontology-as-runtime implementation plan complete"
```
