# Intent System Roadmap

This document defines the repo convergence path from the current signal/case-heavy shape
to an intent-centered backend.

The target system language is:

`Observation -> Intent -> Case -> Outcome -> Memory`

Everything else should be treated as a supporting layer.

## North Star

Eden should become:

- an observation engine that normalizes market evidence
- an intent inference engine that estimates hidden market processes
- a case operating layer that packages intent into reviewable actions
- an outcome and memory loop that updates future intent priors

Not the primary product language:

- `signal`
- `pressure`
- `family`
- `mechanism`
- `archetype`

These remain useful, but only as supporting structures.

## Layer Map

### Observation

Purpose:
- represent what the market actually emitted

Primary modules:
- [src/core/market_snapshot.rs](/Users/hayhay2323/Desktop/eden/src/core/market_snapshot.rs)
- [src/ontology/snapshot.rs](/Users/hayhay2323/Desktop/eden/src/ontology/snapshot.rs)
- [src/ontology/links.rs](/Users/hayhay2323/Desktop/eden/src/ontology/links.rs)
- [src/pipeline/signals/observations.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/signals/observations.rs)
- [src/us/pipeline/signals.rs](/Users/hayhay2323/Desktop/eden/src/us/pipeline/signals.rs)
- [src/us/pipeline/dimensions.rs](/Users/hayhay2323/Desktop/eden/src/us/pipeline/dimensions.rs)

Supporting concepts:
- `signal`
- `event`
- `dimension`

Rule:
- observations describe facts, not strategy language

### Intent

Purpose:
- infer the hidden process behind observations

Primary modules:
- [src/ontology/reasoning.rs](/Users/hayhay2323/Desktop/eden/src/ontology/reasoning.rs)

Primary concepts:
- `IntentHypothesis`
- `IntentKind`
- `IntentDirection`
- `IntentStrength`

Supporting concepts:
- `ExpectationBinding`
- `ExpectationViolation`
- `CaseSignature`
- `pressure`
- `propagation`

Rule:
- intent is the new center of reasoning

### Case

Purpose:
- package an inferred intent into an operational object

Primary modules:
- [src/live_snapshot.rs](/Users/hayhay2323/Desktop/eden/src/live_snapshot.rs)
- [src/cases/types.rs](/Users/hayhay2323/Desktop/eden/src/cases/types.rs)
- [src/cases/builders.rs](/Users/hayhay2323/Desktop/eden/src/cases/builders.rs)
- [src/ontology/contracts/types.rs](/Users/hayhay2323/Desktop/eden/src/ontology/contracts/types.rs)
- [src/ontology/contracts/build.rs](/Users/hayhay2323/Desktop/eden/src/ontology/contracts/build.rs)

Primary concepts:
- `LiveTacticalCase`
- `CaseSummary`
- `CaseContract`
- `RecommendationContract`

Rule:
- case is not world truth
- case is the operational envelope for an intent

### Outcome

Purpose:
- record what actually happened after a case existed

Primary modules:
- [src/persistence/case_realized_outcome.rs](/Users/hayhay2323/Desktop/eden/src/persistence/case_realized_outcome.rs)
- [src/persistence/case_reasoning_assessment.rs](/Users/hayhay2323/Desktop/eden/src/persistence/case_reasoning_assessment.rs)
- [src/cases/review_analytics.rs](/Users/hayhay2323/Desktop/eden/src/cases/review_analytics.rs)

Rule:
- outcome should always be traceable back to case shape and inferred intent

### Memory

Purpose:
- compress repeated intent shapes into reusable priors

Primary modules:
- [src/persistence/discovered_archetype.rs](/Users/hayhay2323/Desktop/eden/src/persistence/discovered_archetype.rs)
- [src/pipeline/learning_loop/types.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/learning_loop/types.rs)
- [src/pipeline/learning_loop/feedback.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/learning_loop/feedback.rs)
- [src/graph/edge_learning.rs](/Users/hayhay2323/Desktop/eden/src/graph/edge_learning.rs)

Supporting concepts:
- `ArchetypeProjection`
- `DiscoveredArchetypeRecord`
- signature adjustments
- violation adjustments

Rule:
- memory stores recurring intent structure, not narrative flavor

## What Stays, What Changes

### Keep as core

- `CanonicalMarketSnapshot`
- `ObservationSnapshot`
- `IntentHypothesis`
- `TacticalSetup`
- `CaseSummary`
- `CaseRealizedOutcomeRecord`
- `CaseReasoningAssessmentRecord`
- `DiscoveredArchetypeRecord`

### Keep but demote

- `signal`
- `pressure`
- `family`
- `mechanism`
- `archetype`

New interpretation:

- `signal` = observation feature
- `pressure` = latent intent field estimate
- `family` = human label
- `mechanism` = explanatory fragment
- `archetype` = memory-compressed intent shape

### Gradually replace

- `Hypothesis` should gradually stop being the top-level public reasoning object
- `IntentHypothesis` should become the first-class external reasoning object

Do not do a hard rename immediately.
Shift usage first, rename later.

## Phase Plan

### Phase 0: Done / In Flight

Current state already completed or partly completed:

- canonical observation substrate exists
- expectation / violation exists
- case signature exists
- archetype memory exists
- outcome loop exists
- `IntentHypothesis` exists
- `inferred_intent` is already flowing into:
  - live tactical case
  - case summary
  - case/recommendation contracts
  - tactical setup persistence
  - reasoning assessment persistence

Exit condition:
- default and persistence compiles stay green

### Phase 1: Make Intent the First Sort Key After Governance

Required changes:

- rank cases by:
  - workflow/governance state
  - intent priority
  - structure priority
  - confidence
  - edge

- make briefing show:
  - dominant intents
  - strongest conflicting intents

Primary files:
- [src/cases/builders.rs](/Users/hayhay2323/Desktop/eden/src/cases/builders.rs)
- [src/cases/types.rs](/Users/hayhay2323/Desktop/eden/src/cases/types.rs)

Exit condition:
- review and briefing outputs read naturally in intent language

### Phase 2: Move Learning to Intent-Level Memory

Required changes:

- learning feedback should not only say:
  - archetype adjustment
  - signature adjustment

- it should also say:
  - intent-kind adjustment
  - intent-strength regime adjustment
  - conflict-pattern adjustment

Primary files:
- [src/pipeline/learning_loop/types.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/learning_loop/types.rs)
- [src/pipeline/learning_loop/feedback.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/learning_loop/feedback.rs)
- [src/cases/review_analytics.rs](/Users/hayhay2323/Desktop/eden/src/cases/review_analytics.rs)

Exit condition:
- memory can answer:
  - which intents historically worked
  - under which context
  - with which conflict/violation shapes

### Phase 3: Rename the Public Reasoning Surface

Required changes:

- begin shifting public language:
  - `hypothesis` -> `intent hypothesis`
  - `case signature` -> `intent signature`
  - `archetype projection` -> `intent memory projection`

Do this in:
- API output fields
- briefing text
- review analytics labels
- operator-facing summaries

Do not rename core Rust types all at once.
Start with outward surfaces first.

Primary files:
- [src/ontology/contracts/types.rs](/Users/hayhay2323/Desktop/eden/src/ontology/contracts/types.rs)
- [src/ontology/contracts/build.rs](/Users/hayhay2323/Desktop/eden/src/ontology/contracts/build.rs)
- [src/cases/reasoning_story/shared.rs](/Users/hayhay2323/Desktop/eden/src/cases/reasoning_story/shared.rs)

Exit condition:
- product outputs stop sounding like a mixed vocabulary of signal/case/family/mechanism

### Phase 4: Collapse Duplicate Reasoning Language

Required changes:

- remove duplicated “what is happening” language spread across:
  - family
  - mechanism
  - pressure
  - signal labels

Target:
- one central explanation path:
  - observation summary
  - inferred intent
  - intent strength/conflict
  - memory support
  - recommended action

Primary files:
- [src/cases/builders.rs](/Users/hayhay2323/Desktop/eden/src/cases/builders.rs)
- [src/ontology/reasoning.rs](/Users/hayhay2323/Desktop/eden/src/ontology/reasoning.rs)
- [src/pipeline/pressure/bridge.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/pressure/bridge.rs)
- [src/pipeline/residual.rs](/Users/hayhay2323/Desktop/eden/src/pipeline/residual.rs)

Exit condition:
- a case can be understood without reading five parallel vocabularies

## Engineering Rules

Every new backend change must declare which layer it strengthens:

- Observation
- Intent
- Case
- Outcome
- Memory

If it does not clearly strengthen one of those, it should not be merged.

Every ranking or review change must explain:

- which intent it prioritizes
- why that intent is stronger, riskier, more conflicting, or more historically predictive

Every new persistence field should answer one of:

- what was observed
- what intent was inferred
- what happened next
- what memory should change

## Immediate Next Tasks

1. Make `dominant_intents` part of all primary analyst outputs, not only case briefing.
2. Add intent-level learning adjustments to `ReasoningLearningFeedback`.
3. Add intent-level regression tests for:
   - contract serialization
   - case ranking shifts
   - persistence round-trip
4. Treat stable discovered archetypes as intent memory, not standalone labels.

## Non-Goals

Do not:

- reintroduce fixed family enums as the center of the system
- let `mechanism` replace intent as the top-level explanation
- add more ontology types unless they map directly to one of the five layers

## Summary

The repo should converge to this shape:

- Observation is how the market is seen
- Intent is how the market is interpreted
- Case is how intent is operationalized
- Outcome is how intent is judged
- Memory is how future intent inference changes

That is the target architecture.
