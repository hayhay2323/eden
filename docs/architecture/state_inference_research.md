# State Inference Research

## Purpose

Eden's next research target is not "more signals" or "more actions".
It is a narrower question:

Can Eden infer whether a symbol is in `continuation`, `turning_point`, or
`low_information` from local observations at tick `t`?

This document freezes the first research scaffold so the project does not
slide back into surface churn.

## North Star

For every setup observed at runtime, Eden should eventually be able to answer:

- Is the current state a `continuation` candidate?
- Is it a `turning_point` candidate?
- Is the signal `low_information` and therefore not worth acting on?

Execution policy is downstream of this question.

## Phase 1 Labeling

The first implementation uses realized outcomes to derive coarse state labels.
It does **not** attempt to infer state directly from raw ticks yet.

### Derived labels

- `continuation`
  - `followed_through == true`
  - `structure_retained == true`
  - `net_return > 0`
- `turning_point`
  - `invalidated == true`
  - or `followed_through == true && structure_retained == false`
  - or `net_return <= 0` with meaningful excursion
- `low_information`
  - no follow-through
  - no invalidation
  - weak excursion

### Horizon buckets

- `fast`: `ticks_to_resolution <= 5`
- `mid`: `6..=20`
- `late`: `> 20`

### Output fields

The research/export surface should carry:

- `state_label`
- `state_label_confidence`
- `state_label_horizon`
- `ticks_to_resolution`
- `state_label_reason_codes`

## Why this is useful

This phase is intentionally simple. It gives us:

- a repeatable way to label old cases
- a baseline dataset for evaluation
- a concrete target for future model-based state inference

Without this layer, `actionability_score` can be calibrated only against
returns, not against the more fundamental state distinctions we actually care
about.

## Non-goals

These are **not** part of phase 1:

- live trading policy changes
- contrarian execution logic
- broker routing / pre-queue orders
- UI polish
- adding more runtime display fields

## Immediate next steps

1. Export perception rows with derived state labels.
2. Build state-label summary reports by:
   - label
   - horizon
   - hit / invalidation / net return
3. Backfill or replay enough recent rows so the export has both:
   - perception fields
   - realized outcomes
4. Only then start evaluating whether runtime `local_state` aligns with the
   derived research labels.
