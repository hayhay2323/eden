# Correctness Fixes — Energy Polarity, AbsenceMemory Wiring, US Depth Cleanup

## Goal

Fix 3 correctness issues identified in the code review that affect reasoning quality and API efficiency.

## Issue 1: Energy Polarity Carries Real Direction

**Problem:** `NodeEnergyMap::from_propagation_paths` derives polarity from `step.confidence.signum()`, but confidence is always `source_delta.abs().min(1.0)` — always positive. Bearish direction is lost; all energy is positive.

**Fix:**

1. Add `polarity: i8` field to `PropagationStep` in `src/ontology/reasoning.rs` (default `1`, serde default).
2. In `push_diffusion_path()` at `src/pipeline/reasoning/propagation.rs:349-360`, capture `source_delta.signum()` as `i8` and store in the step.
3. In `NodeEnergyMap::from_propagation_paths()` at `src/graph/energy.rs:28-33`, use `step.polarity` instead of `step.confidence.signum()`.

**Impact:** Energy map now correctly represents bearish diffusion as negative energy, enabling directional reasoning in convergence scores.

## Issue 2: AbsenceMemory::record_propagation Wired Into Runtime

**Problem:** `record_propagation(&sector)` exists but is never called. AbsenceMemory accumulates suppression but never clears it on successful propagation — suppression is sticky for 30 minutes.

**Fix:**

In `src/hk/runtime.rs`, after `derive_diffusion_propagation_paths()` (~line 595-609), extract sectors from successful propagation paths and call `absence_memory.record_propagation(&sector)` for each.

Logic: iterate the returned paths, collect unique `SectorId` from path references, call `record_propagation` for each.

**Impact:** Sectors recover from suppression when propagation actually fires instead of waiting 30 minutes.

## Issue 3: US Removes Unused DEPTH Subscription

**Problem:** US subscribes to `SubFlags::DEPTH` but `UsLiveState::apply()` drops depth events via `_ => {}`. US pipeline has no depth processing. This wastes Longport API quota.

**Fix:**

In `src/us/runtime/startup.rs:65`, change:
```rust
SubFlags::QUOTE | SubFlags::TRADE | SubFlags::DEPTH
```
to:
```rust
SubFlags::QUOTE | SubFlags::TRADE
```

**Impact:** Reduces WebSocket bandwidth. US pipeline doesn't use depth data (confirmed: `UsSymbolDimensions` has no depth fields, `dimensions.rs` documents this explicitly).

## Issue 4: REST Merge Spike — Accepted Tradeoff

The 15.8s spike when REST data merges into the pipeline (1 tick per 60s cycle) is accepted. The spike is bounded, predictable, and the alternative (incremental merge) adds significant complexity for marginal benefit.

## Testing

- Issue 1: Existing `energy.rs` tests should be updated to verify negative polarity propagates.
- Issue 2: The existing `absence_memory_clears_on_propagation` test already covers the method; verify it's exercised in integration.
- Issue 3: No test needed — subscription flag change only.
- All: `cargo check --lib -q` must pass, `cargo test --lib` must pass (780/780).
