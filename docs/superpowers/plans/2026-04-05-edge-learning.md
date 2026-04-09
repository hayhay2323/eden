# Edge Learning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make BrainGraph edge weights learn from historical outcomes — profitable edges get amplified, losing edges get dampened — so ConvergenceScore reflects learned experience, not just current microstructure.

**Architecture:** New `EdgeLearningLedger` struct accumulates credit per edge from resolved `CaseRealizedOutcomeRecord` using `ConvergenceDetail` to attribute credit to dominant component edges. `ConvergenceScore::compute()` applies `weight_multiplier` from the ledger to each edge aggregation. Runtime holds the ledger across ticks, backfills from history at startup, and decays stale entries.

**Tech Stack:** Rust, rust_decimal, time, petgraph (existing BrainGraph)

**Spec:** `docs/superpowers/specs/2026-04-05-edge-learning-design.md`

---

### Task 1: Create EdgeLearningLedger with tests

**Files:**
- Create: `src/graph/edge_learning.rs`
- Modify: `src/graph/mod.rs`

- [ ] **Step 1: Create edge_learning.rs with tests first**

Create `src/graph/edge_learning.rs`:

```rust
use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::pipeline::reasoning::ConvergenceDetail;

/// Per-edge accumulated learning signal from historical outcomes.
#[derive(Debug, Clone, Default)]
pub struct EdgeLearningLedger {
    entries: HashMap<String, EdgeCredit>,
}

#[derive(Debug, Clone)]
pub struct EdgeCredit {
    pub total_credit: Decimal,
    pub sample_count: u32,
    pub mean_credit: Decimal,
    pub last_updated: OffsetDateTime,
}

impl EdgeLearningLedger {
    /// Get the weight multiplier for a given edge. Range: [0.5, 1.5].
    /// Returns 1.0 (neutral) if no learning data exists.
    pub fn weight_multiplier(&self, edge_id: &str) -> Decimal {
        self.entries
            .get(edge_id)
            .map(|credit| {
                Decimal::ONE + credit.mean_credit.clamp(Decimal::new(-5, 1), Decimal::new(5, 1))
            })
            .unwrap_or(Decimal::ONE)
    }

    /// Credit edges based on a resolved outcome's convergence detail.
    ///
    /// Identifies the dominant convergence component (institutional_alignment,
    /// sector_coherence, or cross_stock_correlation) and distributes credit
    /// to the corresponding edge type.
    pub fn credit_from_outcome(
        &mut self,
        symbol: &Symbol,
        net_return: Decimal,
        detail: &ConvergenceDetail,
        now: OffsetDateTime,
        inst_edge_ids: &[String],
        stock_edge_ids: &[String],
        sector_edge_id: Option<&str>,
    ) {
        let inst_abs = detail.institutional_alignment.abs();
        let sector_abs = detail
            .sector_coherence
            .map(|v| v.abs())
            .unwrap_or(Decimal::ZERO);
        let cross_abs = detail.cross_stock_correlation.abs();
        let total_abs = inst_abs + sector_abs + cross_abs;

        if total_abs == Decimal::ZERO {
            return;
        }

        // Find dominant component
        let (target_ids, contribution_ratio) = if inst_abs >= sector_abs && inst_abs >= cross_abs {
            (inst_edge_ids.to_vec(), inst_abs / total_abs)
        } else if cross_abs >= inst_abs && cross_abs >= sector_abs {
            (stock_edge_ids.to_vec(), cross_abs / total_abs)
        } else {
            (
                sector_edge_id.map(|id| vec![id.to_string()]).unwrap_or_default(),
                sector_abs / total_abs,
            )
        };

        let credit = net_return * contribution_ratio;
        for edge_id in target_ids {
            let entry = self.entries.entry(edge_id).or_insert(EdgeCredit {
                total_credit: Decimal::ZERO,
                sample_count: 0,
                mean_credit: Decimal::ZERO,
                last_updated: now,
            });
            entry.total_credit += credit;
            entry.sample_count += 1;
            entry.mean_credit = entry.total_credit / Decimal::from(entry.sample_count);
            entry.last_updated = now;
        }
    }

    /// Decay stale entries. Entries older than 7 days get credit reduced by 5%.
    /// Entries with negligible credit are removed.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::days(7);
        for entry in self.entries.values_mut() {
            if entry.last_updated < cutoff {
                entry.total_credit *= Decimal::new(95, 2);
                entry.mean_credit = if entry.sample_count > 0 {
                    entry.total_credit / Decimal::from(entry.sample_count)
                } else {
                    Decimal::ZERO
                };
            }
        }
        self.entries
            .retain(|_, entry| entry.total_credit.abs() >= Decimal::new(1, 3));
    }

    /// Number of edges with learning data.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Build edge fingerprint strings for a symbol's edges in the BrainGraph.
pub fn edge_ids_for_symbol(
    symbol: &Symbol,
    brain: &crate::graph::graph::BrainGraph,
) -> (Vec<String>, Vec<String>, Option<String>) {
    use crate::graph::graph::{EdgeKind, NodeKind};
    use petgraph::Direction as GraphDirection;

    let Some(&stock_idx) = brain.stock_nodes.get(symbol) else {
        return (vec![], vec![], None);
    };

    let mut inst_ids = Vec::new();
    let mut stock_ids = Vec::new();
    let mut sector_id = None;

    // Institution→Stock edges (incoming)
    for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Incoming) {
        if let EdgeKind::InstitutionToStock(_) = edge.weight() {
            let source = edge.source();
            if let NodeKind::Institution(inst) = &brain.graph[source] {
                inst_ids.push(format!("inst:{}→stock:{}", inst.institution_id, symbol));
            }
        }
    }

    // Stock↔Stock edges (outgoing)
    for edge in brain.graph.edges_directed(stock_idx, GraphDirection::Outgoing) {
        match edge.weight() {
            EdgeKind::StockToStock(_) => {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    let mut pair = [symbol.to_string(), neighbor.symbol.to_string()];
                    pair.sort();
                    stock_ids.push(format!("stock:{}↔stock:{}", pair[0], pair[1]));
                }
            }
            EdgeKind::StockToSector(_) => {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_id = Some(format!("stock:{}→sector:{}", symbol, s.sector_id));
                }
            }
            _ => {}
        }
    }

    (inst_ids, stock_ids, sector_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn make_detail(inst: Decimal, sector: Decimal, cross: Decimal) -> ConvergenceDetail {
        ConvergenceDetail {
            institutional_alignment: inst,
            sector_coherence: Some(sector),
            cross_stock_correlation: cross,
            component_spread: None,
            edge_stability: None,
        }
    }

    #[test]
    fn credit_attribution_selects_dominant_component() {
        let mut ledger = EdgeLearningLedger::default();
        let symbol = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();
        // institutional_alignment is dominant (0.6 > 0.2 > 0.1)
        let detail = make_detail(dec!(0.6), dec!(0.2), dec!(0.1));
        ledger.credit_from_outcome(
            &symbol,
            dec!(0.05), // 5% return
            &detail,
            now,
            &["inst:1→stock:700.HK".into()],
            &["stock:700.HK↔stock:388.HK".into()],
            Some("stock:700.HK→sector:tech"),
        );
        // inst edges should have credit, others should not
        assert!(ledger.weight_multiplier("inst:1→stock:700.HK") > Decimal::ONE);
        assert_eq!(
            ledger.weight_multiplier("stock:700.HK↔stock:388.HK"),
            Decimal::ONE
        );
        assert_eq!(
            ledger.weight_multiplier("stock:700.HK→sector:tech"),
            Decimal::ONE
        );
    }

    #[test]
    fn weight_multiplier_positive_credit_amplifies() {
        let mut ledger = EdgeLearningLedger::default();
        let entry = EdgeCredit {
            total_credit: dec!(0.3),
            sample_count: 1,
            mean_credit: dec!(0.3),
            last_updated: OffsetDateTime::now_utc(),
        };
        ledger.entries.insert("test_edge".into(), entry);
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(1.3));
    }

    #[test]
    fn weight_multiplier_negative_credit_dampens() {
        let mut ledger = EdgeLearningLedger::default();
        let entry = EdgeCredit {
            total_credit: dec!(-0.3),
            sample_count: 1,
            mean_credit: dec!(-0.3),
            last_updated: OffsetDateTime::now_utc(),
        };
        ledger.entries.insert("test_edge".into(), entry);
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(0.7));
    }

    #[test]
    fn weight_multiplier_capped_at_50_pct() {
        let mut ledger = EdgeLearningLedger::default();
        let entry = EdgeCredit {
            total_credit: dec!(0.9),
            sample_count: 1,
            mean_credit: dec!(0.9),
            last_updated: OffsetDateTime::now_utc(),
        };
        ledger.entries.insert("test_edge".into(), entry);
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(1.5));
    }

    #[test]
    fn decay_reduces_stale_entries() {
        let mut ledger = EdgeLearningLedger::default();
        let now = OffsetDateTime::now_utc();
        let old = now - time::Duration::days(8);
        let entry = EdgeCredit {
            total_credit: dec!(0.10),
            sample_count: 1,
            mean_credit: dec!(0.10),
            last_updated: old,
        };
        ledger.entries.insert("stale_edge".into(), entry);
        ledger.decay(now);
        // 0.10 * 0.95 = 0.095
        assert!(ledger.weight_multiplier("stale_edge") < dec!(1.10));
    }

    #[test]
    fn no_learning_data_returns_neutral() {
        let ledger = EdgeLearningLedger::default();
        assert_eq!(ledger.weight_multiplier("unknown_edge"), Decimal::ONE);
    }
}
```

- [ ] **Step 2: Add module to graph/mod.rs**

In `src/graph/mod.rs`, add after the last `pub mod` line:

```rust
pub mod edge_learning;
```

- [ ] **Step 3: Verify tests pass**

Run: `cargo test --lib -- graph::edge_learning::tests -q`
Expected: 6 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/graph/edge_learning.rs src/graph/mod.rs
git commit -m "$(cat <<'EOF'
feat(graph): add EdgeLearningLedger — outcome-adaptive edge weights

EdgeCredit accumulates per-edge from resolved outcomes using
ConvergenceDetail to attribute credit to dominant component.
weight_multiplier range [0.5, 1.5], decay after 7 days.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Integrate into ConvergenceScore::compute

**Files:**
- Modify: `src/graph/convergence.rs`

- [ ] **Step 1: Add ledger parameter to compute()**

In `src/graph/convergence.rs`, change the `compute` signature (line 32):

```rust
    pub fn compute(
        symbol: &Symbol,
        brain: &BrainGraph,
        temporal_ctx: Option<&TemporalConvergenceContext>,
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Option<Self> {
```

- [ ] **Step 2: Apply weight_multiplier to institutional_alignment**

In the institutional_alignment loop (lines 42-51), change:

```rust
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let w = Decimal::from(e.seat_count as i64);
                let learned = edge_ledger
                    .map(|ledger| {
                        let source = edge.source();
                        if let NodeKind::Institution(inst) = &brain.graph[source] {
                            ledger.weight_multiplier(&format!(
                                "inst:{}→stock:{}",
                                inst.institution_id, symbol
                            ))
                        } else {
                            Decimal::ONE
                        }
                    })
                    .unwrap_or(Decimal::ONE);
                weighted_sum += e.direction * w * learned;
                weight_total += w;
            }
        }
```

- [ ] **Step 3: Apply weight_multiplier to cross_stock_correlation**

In the cross_stock loop (lines 77-88), change:

```rust
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    let learned = edge_ledger
                        .map(|ledger| {
                            let mut pair =
                                [symbol.to_string(), neighbor.symbol.to_string()];
                            pair.sort();
                            ledger.weight_multiplier(&format!(
                                "stock:{}↔stock:{}",
                                pair[0], pair[1]
                            ))
                        })
                        .unwrap_or(Decimal::ONE);
                    corr_sum += e.similarity * neighbor.mean_direction * learned;
                    corr_count += 1;
                }
            }
        }
```

- [ ] **Step 4: Apply weight_multiplier to sector_coherence**

After the sector_coherence loop (line 72), apply learned weight:

```rust
        // Apply learned weight to sector coherence
        if let Some(sc) = sector_coherence {
            let learned_sector = edge_ledger
                .map(|ledger| {
                    ledger.weight_multiplier(&format!("stock:{}→sector:unknown", symbol))
                })
                .unwrap_or(Decimal::ONE);
            sector_coherence = Some(sc * learned_sector);
        }
```

Note: The sector_id isn't directly available in this loop (we only have the SectorNode). For a more precise fingerprint, we'd need to extract sector_id. Use the format `"stock:{symbol}→sector:{sector_id}"` by reading the SectorNode:

```rust
        let mut sector_coherence = None;
        let mut sector_learned = Decimal::ONE;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_coherence = Some(s.mean_direction);
                    sector_learned = edge_ledger
                        .map(|ledger| {
                            ledger.weight_multiplier(&format!(
                                "stock:{}→sector:{}",
                                symbol, s.sector_id
                            ))
                        })
                        .unwrap_or(Decimal::ONE);
                }
            }
        }
        sector_coherence = sector_coherence.map(|sc| sc * sector_learned);
```

- [ ] **Step 5: Update all callers of ConvergenceScore::compute**

Search for `ConvergenceScore::compute(` in the codebase and add the new `None` parameter:

```bash
grep -rn "ConvergenceScore::compute(" src/ | grep -v test
```

In `src/graph/decision.rs` (line 75):
```rust
if let Some(score) = ConvergenceScore::compute(symbol, brain, temporal_ctx, None) {
```

This passes `None` for now — the runtime will pass the real ledger in Task 3.

Also fix any test files that call `ConvergenceScore::compute` — add `None` as the last argument.

- [ ] **Step 6: Verify compilation and tests**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0
Run: `cargo test --lib -- graph -q` → expect all pass

- [ ] **Step 7: Commit**

```bash
git add src/graph/convergence.rs src/graph/decision.rs
git commit -m "$(cat <<'EOF'
feat(graph): integrate EdgeLearningLedger into ConvergenceScore::compute

Each edge aggregation (institutional_alignment, cross_stock_correlation,
sector_coherence) now applies weight_multiplier from the ledger.
Passing None produces identical results to before (backward compatible).

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Wire EdgeLearningLedger into HK runtime

**Files:**
- Modify: `src/hk/runtime.rs`
- Modify: `src/graph/decision.rs`

- [ ] **Step 1: Add ledger to DecisionSnapshot::compute**

In `src/graph/decision.rs`, change `compute` signature (line 65):

```rust
    pub fn compute(
        brain: &BrainGraph,
        links: &LinkSnapshot,
        active_fingerprints: &[StructuralFingerprint],
        store: &ObjectStore,
        temporal_ctx: Option<&TemporalConvergenceContext>,
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Self {
```

Pass `edge_ledger` to `ConvergenceScore::compute` (line 75):

```rust
if let Some(score) = ConvergenceScore::compute(symbol, brain, temporal_ctx, edge_ledger) {
```

- [ ] **Step 2: Update all callers of DecisionSnapshot::compute**

Search for `DecisionSnapshot::compute(` and add `None` as the last argument to all existing call sites except the HK runtime main path (which will get the real ledger).

- [ ] **Step 3: Hold EdgeLearningLedger in HK runtime**

In `src/hk/runtime.rs`, before the tick loop (near line 204), add:

```rust
    let mut edge_ledger = eden::graph::edge_learning::EdgeLearningLedger::default();
```

- [ ] **Step 4: Pass ledger to DecisionSnapshot::compute**

Find the `DecisionSnapshot::compute(` call in HK runtime and add `Some(&edge_ledger)`:

```rust
let decision = DecisionSnapshot::compute(
    &brain,
    &links,
    &active_fps,
    &store,
    Some(&temporal_ctx),
    Some(&edge_ledger),
);
```

- [ ] **Step 5: Credit edges on outcome resolution**

Find where outcomes are resolved/persisted in HK runtime. After outcomes are computed, add:

```rust
// Credit edges from resolved outcomes
for outcome in &resolved_outcomes {
    if let Some(setup) = previous_setups.iter().find(|s| s.setup_id == outcome.setup_id) {
        if let Some(detail) = &setup.convergence_detail {
            let symbol = crate::ontology::objects::Symbol(
                outcome.symbol.clone().unwrap_or_default(),
            );
            let (inst_ids, stock_ids, sector_id) =
                eden::graph::edge_learning::edge_ids_for_symbol(&symbol, &brain);
            edge_ledger.credit_from_outcome(
                &symbol,
                outcome.net_return,
                detail,
                outcome.resolved_at,
                &inst_ids,
                &stock_ids,
                sector_id.as_deref(),
            );
        }
    }
}
```

Note: The exact location depends on where `resolved_outcomes` are computed. Search for `CaseRealizedOutcomeRecord` or `realized_outcome` in the runtime to find the right insertion point.

- [ ] **Step 6: Decay each tick**

After the absence_memory decay (which we added earlier), add:

```rust
edge_ledger.decay(deep_reasoning_decision.timestamp);
```

- [ ] **Step 7: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 8: Commit**

```bash
git add src/graph/decision.rs src/hk/runtime.rs
git commit -m "$(cat <<'EOF'
feat(runtime): wire EdgeLearningLedger into HK tick loop

Runtime holds ledger across ticks, passes to DecisionSnapshot::compute,
credits edges from resolved outcomes, decays stale entries each tick.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: US runtime symmetry

**Files:**
- Modify: `src/us/runtime.rs`

- [ ] **Step 1: Add EdgeLearningLedger to US runtime**

Follow the same pattern as HK (Task 3): hold ledger before tick loop, pass to decision computation, credit on outcomes, decay each tick. The US runtime may use different decision computation patterns — read the file to find the right insertion points.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 3: Commit**

```bash
git add src/us/runtime.rs
git commit -m "$(cat <<'EOF'
feat(us): wire EdgeLearningLedger into US runtime (symmetric with HK)

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Add convergence integration test

**Files:**
- Modify: `src/graph/convergence.rs` (test section) or `src/graph/decision_tests.rs`

- [ ] **Step 1: Write integration test**

Add a test that verifies learned weights change ConvergenceScore:

```rust
#[test]
fn convergence_score_uses_learned_edge_weights() {
    // 1. Build a minimal BrainGraph with one stock, one institution edge
    // 2. Compute ConvergenceScore with no ledger → get baseline composite
    // 3. Create a ledger with positive credit for the institution edge
    // 4. Compute ConvergenceScore with ledger → get boosted composite
    // 5. Assert boosted > baseline

    // Use existing test helper patterns from convergence tests or decision_tests.rs
    // to build the minimal graph.
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --lib -- convergence_score_uses_learned -q`
Expected: 1 test passes

- [ ] **Step 3: Commit**

```bash
git add src/graph/convergence.rs
git commit -m "$(cat <<'EOF'
test(graph): verify ConvergenceScore integrates learned edge weights

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Final verification

- [ ] **Step 1: Full cargo check**

Run: `cargo check --lib -q 2>&1 | grep -c "^error"` → expect 0

- [ ] **Step 2: Full test suite**

Run: `cargo test --lib 2>&1 | grep "test result:"` → expect 760+ passed

- [ ] **Step 3: Verify edge_learning tests**

Run: `cargo test --lib -- graph::edge_learning -q` → expect 6 passed

- [ ] **Step 4: Git log**

Expected 5 new commits:
1. `feat(graph): add EdgeLearningLedger — outcome-adaptive edge weights`
2. `feat(graph): integrate EdgeLearningLedger into ConvergenceScore::compute`
3. `feat(runtime): wire EdgeLearningLedger into HK tick loop`
4. `feat(us): wire EdgeLearningLedger into US runtime`
5. `test(graph): verify ConvergenceScore integrates learned edge weights`
