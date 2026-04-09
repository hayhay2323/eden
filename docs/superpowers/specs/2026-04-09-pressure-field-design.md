# Eden Pressure Field — First Principles Redesign

> **Status:** Phase 1 — Delete template/hypothesis system. Phase 2 — Build pressure field.

## Vision

Eden is a pressure field over a knowledge graph. Data flows in, propagates through the topology, and vortices emerge where multiple independent information streams converge. No templates. No predefined patterns. The topology determines what matters.

## What Gets Deleted (Phase 1)

### Reasoning pipeline (entire hypothesis/template system)
- `src/pipeline/reasoning/support.rs` — TEMPLATE_REGISTRY, 15 template metadata records
- `src/pipeline/reasoning/synthesis.rs` — derive_hypotheses, template matching
- `src/pipeline/reasoning/policy.rs` — promote/observe/enter policies, ReviewerDoctrinePressure
- `src/pipeline/reasoning/family_gate.rs` — FamilyAlphaGate
- `src/pipeline/reasoning/context.rs` — AbsenceMemory, FamilyBoostLedger, ReasoningContext
- `src/pipeline/reasoning/clustering.rs` — case clustering
- `src/pipeline/reasoning/signal_aggregator.rs` — signal aggregation

### US reasoning parallel
- `src/us/pipeline/reasoning/synthesis.rs`
- `src/us/pipeline/reasoning/policy.rs`
- `src/us/pipeline/reasoning/support.rs`
- `src/us/pipeline/reasoning/vortex.rs`

### Evolution/governance
- `src/temporal/lineage/evolution.rs` — evolution cycle
- `src/temporal/lineage/schema.rs` — causal schema extraction
- `src/temporal/lineage/vortex.rs` — vortex fingerprinting

### Runtime calls
- HK runtime: delete reasoning derivation block, evolution cycle, absence memory
- US runtime: delete US reasoning snapshot derivation, evolution cycle

## What Gets Kept

### Data structures (persistence + API compatibility)
- `src/ontology/reasoning.rs` — Hypothesis, TacticalSetup, HypothesisTrack, PropagationPath structs
- Persistence records for these types
- API endpoints that serve them (return empty until pressure field generates them)

### Core infrastructure
- Event detection (VolumeSpike, CapitalFlow, etc.)
- Dimension computation (5-dim for US, multi-dim for HK)
- Graph computation (BrainGraph, UsGraph)
- Tick loop structure
- Edge learning (EdgeLearningLedger)
- Persistence store + schema
- API layer + frontend
- Cross-market bridge

## What Gets Built (Phase 2)

### Pressure Field Engine
1. **Pressure Sources** — each data point creates pressure at a graph node
2. **Multi-Scale Fields** — tick/minute/hour/day layers with different decay rates
3. **Propagation** — pressure flows along graph edges per tick
4. **Anomaly Detection** — deviation from baseline pressure
5. **Output** — pressure concentrations, conflicts, isolation, waves
6. **Learning** — record field shape → outcome, update edge weights

### Shared between HK and US
Same engine, different pressure sources and edge types based on available data.
