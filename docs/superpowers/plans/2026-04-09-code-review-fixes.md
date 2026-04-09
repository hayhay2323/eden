# Code Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all Critical and Important issues identified in the 5-agent code review, restoring test compilation and runtime safety before data flow work begins.

**Architecture:** Pure fixes — no new modules, no new abstractions. Each task is a surgical repair to existing code. Tests must compile and pass after each task.

**Tech Stack:** Rust, SurrealDB schema migrations, TypeScript (frontend)

---

### Task 1: Implement `AbsenceMemory::record_propagation` + fix test

**Files:**
- Modify: `src/pipeline/reasoning/context.rs:31-63`

- [ ] **Step 1: Add `record_propagation` method to `AbsenceMemory`**

Add after the `decay` method (line 62), before the closing `}` of the impl block:

```rust
/// Clear absence state for a sector when propagation actually occurs.
/// Without this, suppression is sticky for 30 minutes even when propagation fires.
pub fn record_propagation(&mut self, sector: &SectorId) {
    self.entries.retain(|(s, _), _| s != &sector.0);
}
```

- [ ] **Step 2: Verify the existing test compiles**

Run: `cargo check --tests 2>&1 | grep "record_propagation"`
Expected: No errors for `record_propagation`

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/reasoning/context.rs
git commit -m "fix: implement AbsenceMemory::record_propagation — clear suppression on success"
```

---

### Task 2: Delete broken `residual_adjusted_propagation_strength` test

**Files:**
- Modify: `src/pipeline/residual.rs:1416-1435`

The function `residual_adjusted_propagation_strength` was removed from the codebase but the test remains. The test references a function that no longer exists and cannot be updated — delete it.

- [ ] **Step 1: Delete the broken test**

Remove the entire test function at lines 1416-1435:

```rust
    #[test]
    fn residual_propagation_strength() {
        let field = ResidualField {
            residuals: vec![],
            clustered_sectors: vec![SectorResidualCluster {
                sector: sector("tech"),
                mean_residual: dec!(-0.25),
                symbol_count: 5,
                coherence: dec!(0.8),
                dominant_dimension: ResidualDimension::Price,
            }],
            divergent_pairs: vec![],
        };

        let strength =
            residual_adjusted_propagation_strength(&field, &sector("tech"));
        assert!(strength.is_some());
        // -0.25 * 0.8 = -0.2 → sector propagation is failing
        assert_eq!(strength.unwrap(), dec!(-0.200));
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "residual_adjusted"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/residual.rs
git commit -m "fix(tests): remove test for deleted residual_adjusted_propagation_strength"
```

---

### Task 3: Delete broken `graph_edge_transitions_for_id` test

**Files:**
- Modify: `src/temporal/buffer.rs:221-243`

The method `graph_edge_transitions_for_id` was removed from `TickHistory` but the test remains.

- [ ] **Step 1: Delete the broken test**

Remove lines 221-243 (the `graph_edge_transitions_are_queryable_by_id` test function):

```rust
    #[test]
    fn graph_edge_transitions_are_queryable_by_id() {
        let mut h = TickHistory::new(10);
        let edge_id = GraphEdgeId {
            kind: GraphEdgeKind::InstitutionToStock,
            source_key: "institution:100".into(),
            target_key: "symbol:700.HK".into(),
        };

        let mut first = make_tick(1, "700.HK", dec!(0.1));
        first.graph_edge_transitions = vec![edge_transition(1, GraphEdgeTransitionKind::Appeared)];
        h.push(first);

        let mut second = make_tick(2, "700.HK", dec!(0.2));
        second.graph_edge_transitions =
            vec![edge_transition(2, GraphEdgeTransitionKind::Disappeared)];
        h.push(second);

        let transitions = h.graph_edge_transitions_for_id(&edge_id);
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[0].kind, GraphEdgeTransitionKind::Appeared);
        assert_eq!(transitions[1].kind, GraphEdgeTransitionKind::Disappeared);
    }
```

Also remove any now-unused imports in the test module (e.g., `GraphEdgeId`, `GraphEdgeKind`, `GraphEdgeTransitionKind`, `edge_transition`) if they are only used by this test. Check other tests in the same `#[cfg(test)]` module first.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "graph_edge_transitions_for_id"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/temporal/buffer.rs
git commit -m "fix(tests): remove test for deleted TickHistory::graph_edge_transitions_for_id"
```

---

### Task 4: Delete broken `active_cross_market_pairs` test

**Files:**
- Modify: `src/us/graph/graph.rs:650-669`

The method `active_cross_market_pairs` was removed from `UsGraph` but the test remains.

- [ ] **Step 1: Delete the broken test**

Remove lines 650-669 (the `graph_active_cross_market_pairs` test function):

```rust
    #[test]
    fn graph_active_cross_market_pairs() {
        let snap = make_snapshot(vec![
            (
                sym("BABA.US"),
                make_dims(dec!(0.1), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("JD.US"),
                make_dims(dec!(0.2), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("AAPL.US"),
                make_dims(dec!(0.3), dec!(0), dec!(0), dec!(0), dec!(0)),
            ),
        ]);
        let g = UsGraph::compute(&snap, &HashMap::new(), &HashMap::new());
        let pairs = g.active_cross_market_pairs();
        assert_eq!(pairs.len(), 2); // BABA + JD
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "active_cross_market_pairs"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/us/graph/graph.rs
git commit -m "fix(tests): remove test for deleted UsGraph::active_cross_market_pairs"
```

---

### Task 5: Fix `derive_with_diffusion` argument count in US reasoning test

**Files:**
- Modify: `src/us/pipeline/reasoning_tests.rs:1158-1171`

The function `derive_with_diffusion` gained a new parameter `convergence_scores: Option<&HashMap<Symbol, UsConvergenceScore>>` at position 10 (after `structural_metrics`, before `graph`). The test needs to pass `None` for it.

- [ ] **Step 1: Add missing argument**

Change the call at lines 1158-1171 from:

```rust
    let snapshot = UsReasoningSnapshot::derive_with_diffusion(
        &events,
        &signals,
        1,
        &[],
        &[],
        Some(UsMarketRegimeBias::Neutral),
        None,
        None,
        Some(&structural_metrics),
        &graph,
        &[],
        None,
    );
```

To:

```rust
    let snapshot = UsReasoningSnapshot::derive_with_diffusion(
        &events,
        &signals,
        1,
        &[],
        &[],
        Some(UsMarketRegimeBias::Neutral),
        None,
        None,
        Some(&structural_metrics),
        None,
        &graph,
        &[],
        None,
    );
```

(Added `None,` for `convergence_scores` between `Some(&structural_metrics)` and `&graph`)

- [ ] **Step 2: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "reasoning_tests"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/us/pipeline/reasoning_tests.rs
git commit -m "fix(tests): add missing convergence_scores argument to derive_with_diffusion call"
```

---

### Task 6: Delete broken `recent_leaders` test

**Files:**
- Modify: `src/us/temporal/causality.rs:389-420`

The method `recent_leaders` was removed from `UsCausalTimeline` but the test remains.

- [ ] **Step 1: Delete the broken test**

Remove lines 389-420 (the `recent_leaders_returns_distinct_in_recency_order` test function):

```rust
    #[test]
    fn recent_leaders_returns_distinct_in_recency_order() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        history.push(make_tick(
            1,
            vec![(
                sym("AAPL.US"),
                make_signals(dec!(0.5), dec!(0.1), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        history.push(make_tick(
            2,
            vec![(
                sym("AAPL.US"),
                make_signals(dec!(0.8), dec!(0.1), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        history.push(make_tick(
            3,
            vec![(
                sym("AAPL.US"),
                make_signals(dec!(0.1), dec!(0.9), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));

        let timelines = compute_causal_timelines(&history);
        let tl = timelines.get(&sym("AAPL.US")).unwrap();
        let leaders = tl.recent_leaders(5);
        // Most recent first: momentum, then capital_flow
        assert_eq!(leaders[0], "momentum");
        assert_eq!(leaders[1], "capital_flow");
    }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "recent_leaders"`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add src/us/temporal/causality.rs
git commit -m "fix(tests): remove test for deleted UsCausalTimeline::recent_leaders"
```

---

### Task 7: Full test compilation verification + cargo fix warnings

**Files:**
- Multiple files (auto-fix by cargo)

- [ ] **Step 1: Verify all 6 test errors are resolved**

Run: `cargo check --tests 2>&1 | grep "^error"`
Expected: No errors

- [ ] **Step 2: Auto-fix compiler warnings**

Run: `cargo fix --lib --allow-dirty 2>&1 | tail -20`
Expected: 23+ warnings fixed (mostly `let mut` → `let` in `src/pipeline/signals/events.rs`)

- [ ] **Step 3: Run tests**

Run: `cargo test --lib 2>&1 | tail -30`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "fix: cargo fix — remove unnecessary mut bindings and unused imports"
```

---

### Task 8: Watchdog — terminate on prolonged feed death

**Files:**
- Modify: `src/core/runtime_loop.rs:64-79`

- [ ] **Step 1: Make watchdog return error after max silence**

Change lines 64-79 from:

```rust
        match result {
            Ok(inner) => return inner,
            Err(_timeout) => {
                silent_rounds += 1;
                eprintln!(
                    "[runtime watchdog] no activity for {}s (silent_rounds={}). Data feed may be disconnected.",
                    ACTIVITY_TIMEOUT.as_secs() * u64::from(silent_rounds),
                    silent_rounds,
                );
                if silent_rounds >= 10 {
                    eprintln!("[runtime watchdog] 5 minutes with no data — feed is likely dead.");
                }
            }
        }
```

To:

```rust
        match result {
            Ok(inner) => return inner,
            Err(_timeout) => {
                silent_rounds += 1;
                eprintln!(
                    "[runtime watchdog] no activity for {}s (silent_rounds={}). Data feed may be disconnected.",
                    ACTIVITY_TIMEOUT.as_secs() * u64::from(silent_rounds),
                    silent_rounds,
                );
                if silent_rounds >= 10 {
                    eprintln!("[runtime watchdog] 5 minutes with no data — feed is likely dead. Terminating loop.");
                    return Err(());
                }
            }
        }
```

This requires changing the function return type. Check the function signature — if it currently returns `T`, change to `Result<T, ()>` and update callers accordingly. If callers already expect `Result`, just add the `return Err(())`.

- [ ] **Step 2: Update callers**

The callers in `src/hk/runtime.rs` and `src/us/runtime.rs` that call `next_tick` need to handle the `Err(())` case (log and break the loop, or attempt reconnection).

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib -q`
Expected: Clean

- [ ] **Step 4: Commit**

```bash
git add src/core/runtime_loop.rs src/hk/runtime.rs src/us/runtime.rs
git commit -m "fix(runtime): terminate loop after 5 minutes of feed silence"
```

---

### Task 9: HK runtime `.expect()` → defensive `if let`

**Files:**
- Modify: `src/hk/runtime.rs:1116-1118`

- [ ] **Step 1: Replace `.expect()` with `if let`**

Change lines 1110-1123 from:

```rust
        run_hk_persistence_stage(
            &runtime,
            tick,
            now,
            &raw,
            &links,
            history
                .latest()
                .expect("tick history contains latest record after push"),
            &action_stage.workflow_records,
            &action_stage.workflow_events,
            &reasoning_snapshot,
        )
        .await;
```

To:

```rust
        if let Some(latest_record) = history.latest() {
            run_hk_persistence_stage(
                &runtime,
                tick,
                now,
                &raw,
                &links,
                latest_record,
                &action_stage.workflow_records,
                &action_stage.workflow_events,
                &reasoning_snapshot,
            )
            .await;
        }
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q`
Expected: Clean

- [ ] **Step 3: Commit**

```bash
git add src/hk/runtime.rs
git commit -m "fix(hk): replace .expect() with defensive if-let in live tick loop"
```

---

### Task 10: Add `primary_lens` field to SurrealDB schema

**Files:**
- Modify: `src/persistence/schema.rs`

- [ ] **Step 1: Add migration 018**

After MIGRATION_017 definition (around line 707), add:

```rust
const MIGRATION_018: &str = r#"
DEFINE FIELD primary_lens ON case_realized_outcome TYPE option<string>;
"#;
```

- [ ] **Step 2: Update LATEST_SCHEMA_VERSION and MIGRATIONS array**

Change `LATEST_SCHEMA_VERSION` from 17 to 18.

Add to the MIGRATIONS array:

```rust
    SchemaMigration {
        version: 18,
        name: "case_realized_outcome_primary_lens",
        statements: MIGRATION_018,
    },
```

- [ ] **Step 3: Fix migration test parameter**

In `src/persistence/store/tests.rs:77`, change:

```rust
EdenStore::apply_schema_migrations(&db).await.unwrap();
```

To:

```rust
EdenStore::apply_schema_migrations(&db, path.to_str().unwrap()).await.unwrap();
```

- [ ] **Step 4: Update schema test assertions**

The test at line 823 (`assert_eq!(pending.last().unwrap().version, LATEST_SCHEMA_VERSION)`) should still pass since it references the constant.

- [ ] **Step 5: Verify compilation**

Run: `cargo check --tests 2>&1 | grep "schema\|migration"`
Expected: No errors

- [ ] **Step 6: Commit**

```bash
git add src/persistence/schema.rs src/persistence/store/tests.rs
git commit -m "fix(schema): add migration 018 — define primary_lens field on case_realized_outcome"
```

---

### Task 11: Fix `GPC.US` duplicate + `MarketId` dead wildcard

**Files:**
- Modify: `src/ontology/store/catalog.rs:171`
- Modify: `src/core/runtime/context.rs:62`

- [ ] **Step 1: Remove `GPC.US` from materials sector**

In `src/ontology/store/catalog.rs`, line 171, remove `"GPC.US"` from the materials match arm. GPC (Genuine Parts Company) is a consumer discretionary/industrial distributor, not materials.

- [ ] **Step 2: Remove dead wildcard from MarketId match**

In `src/core/runtime/context.rs:62`, change:

```rust
MarketId::Hk | MarketId::Us => 250, _ => 2_000
```

To:

```rust
MarketId::Hk | MarketId::Us => 250,
```

(Remove the `_ => 2_000` arm entirely since `MarketId` is exhaustive.)

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib -q`
Expected: Clean

- [ ] **Step 4: Commit**

```bash
git add src/ontology/store/catalog.rs src/core/runtime/context.rs
git commit -m "fix: remove duplicate GPC.US sector entry + dead MarketId wildcard"
```

---

### Task 12: Remove unused `task-lifecycle` feature flag

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Remove `task-lifecycle` from features section**

In `Cargo.toml`, find the `[features]` section and remove the `task-lifecycle` entry.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q`
Expected: Clean

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove unused task-lifecycle feature flag"
```

---

### Task 13: Frontend — fix 401 recursive prompt + missing QueryClient import

**Files:**
- Modify: `frontend/src/lib/api/client.ts:75-80`
- Modify: `frontend/src/lib/query/operational.ts:1`

- [ ] **Step 1: Fix recursive 401 handler**

In `frontend/src/lib/api/client.ts`, replace the 401 handler (lines 75-80) with:

```typescript
if (response.status === 401) {
    const currentKey = localStorage.getItem("eden_api_key");
    const key = prompt("Eden API Key:");
    if (key && key !== currentKey) {
      localStorage.setItem("eden_api_key", key);
      return fetchJson<T>(path, init);
    }
    throw new Error("Authentication failed");
  }
```

This prevents infinite recursion by checking if the new key is different from the current one, and throws on cancel or same-key retry.

- [ ] **Step 2: Fix missing QueryClient import**

In `frontend/src/lib/query/operational.ts`, line 1, add `QueryClient` to the import:

```typescript
import { useQuery, useQueryClient, QueryClient } from "@tanstack/react-query";
```

- [ ] **Step 3: Verify frontend compilation**

Run: `cd frontend && npx tsc --noEmit 2>&1 | head -20`

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/api/client.ts frontend/src/lib/query/operational.ts
git commit -m "fix(frontend): prevent 401 recursive prompt + add missing QueryClient import"
```
