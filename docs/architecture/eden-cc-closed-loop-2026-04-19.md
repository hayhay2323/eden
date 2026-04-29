# Eden ↔ Claude Code Closed Loop — 2026-04-19 Reference

Single-file architecture reference for the work landed on 2026-04-19.
Read this before interpreting Monday's live wake output.

## One-line summary

Eden's tactical setups are now gated and ranked by **Eden's own
epistemic state** — `belief_field` uncertainty, historical decision
outcomes, and attention entropy — in addition to the pre-existing
pressure-field signals.

---

## The four loop stages (3 done today, 1 deferred)

```
┌──────────────────────────────────────────────────────────────┐
│                                                                │
│  (1) belief → setup.confidence                       ✅ BM     │
│  (2) belief → attention → next-tick compute          ✅ AB     │
│  (3) belief → intervention → actuator action         🔄 open   │
│  (4) decisions/outcomes → confidence modulation      ✅ OH     │
│                                                                │
└──────────────────────────────────────────────────────────────┘
```

Step 3 is gated on actuator infrastructure (paper trade injection
+ risk infra), a separate multi-session job.

---

## Module map

| Module | Role | LOC |
|--------|------|-----|
| `src/pipeline/belief_field.rs` | Cross-tick persistent belief (A1). `PressureBeliefField` + `top_notable_beliefs` + `top_attention` + `MAX_STATE_ENTROPY_NATS` | ~680 |
| `src/pipeline/belief_modulation.rs` | Step 1 + Step 2. `apply_belief_modulation`, `attention_boost` | ~430 |
| `src/pipeline/decision_ledger.rs` | A2. Reads `decisions/*.json`; per-symbol summaries; `summary_for` | ~330 |
| `src/pipeline/outcome_history.rs` | Step 4. `apply_outcome_history_modulation` | ~330 |
| `src/persistence/belief_snapshot.rs` | Serialize/restore `PressureBeliefField` | ~360 |
| `src/persistence/store/belief.rs` | SurrealDB CRUD for `belief_snapshot` table | ~75 |
| `src/dreaming/report.rs` + `src/bin/dream.rs` | A3α dreaming (offline snapshot diff) | ~650 |
| HK/US runtime integrations | Wire above into tick loop + wake | ~200 modifies |

---

## Wake line catalog (what Claude Code will see Monday)

Each new wake line below is a diagnostic surface. Order in wake.reasons
for any given tick: narrative/inference (existing) → belief: notable →
prior decisions → attention → (existing cluster/institution lines).

### `belief: 0700.HK orderbook μ=1.23 σ²=0.08 n=5840 informed (KL vs prev=0.82)`

From `format_wake_line` in `belief_field.rs`. Top 5 notable per tick.

- `μ` mean pressure value (signed, in channel units)
- `σ²` variance; high variance = this channel varies a lot for this symbol
- `n` sample count
- `informed` if `n >= BELIEF_INFORMED_MIN_SAMPLES` (=5), else `prior-heavy`
- `KL vs prev` = KL divergence from previous-tick belief → high = sudden shift

Emitted only when notable: significant KL shift OR just-crossed informed
threshold OR high posterior shift OR high uncertainty.

### `belief: 0700.HK state_posterior turning_point=0.62, latent=0.28, continuation=0.10 (n=3421)`

Categorical version. Top 3 variants shown sorted. `n` = total state
samples. Same notable filter as Gaussian.

### `attention: 0700.HK state_entropy=1.43 nats (n=487, 89% of max)`

From `format_attention_line`. Top 5 by entropy descending. These are
the symbols Eden is MOST UNSURE ABOUT right now. Parameter-free (entropy
is MI noiseless-observation upper bound).

Interpretation for me:
- 89% of max → posterior near uniform → Eden has little idea what state
  this symbol is in. Any observation will be highly informative.
- Low % → posterior concentrated → Eden is confident. Observation gives
  less new info.

### `prior decisions: KC.US 2 (exit @2026-04-15 -18bps); eden_gap: roster churn != signal fade`

From `format_prior_decisions_line`. Emitted only for symbols that are
in top_notable AND have ≥1 prior decision in the ledger.

- `N` = total decisions (entries + exits + skips)
- `(action @date ±bps)` = most recent action summary
- `eden_gap:` appears when past retrospectives had flagged missing
  dimensions. **This is the most important signal Claude Code wrote to
  itself** — read it carefully.

### Inside `TacticalSetup.risk_notes`: belief_modulation line

NOT a wake line, but persisted on every `TacticalSetup`:

```
belief_modulation: down_23% ×0.77 (entropy=1.43/1.61, min_n=8)
```

From `apply_belief_modulation`. Shows:
- Direction: `up_N%`, `down_N%`, `neutral`, or `unknown_prior`
- Multiplier (clamped [0.5, 1.1])
- Entropy & min gaussian sample count

**This modifies setup.confidence** — policy layer + Claude Code entry
sizing + persistence all see the modulated value.

### Inside `TacticalSetup.risk_notes`: outcome_history line

```
outcome_history: down_8% ×0.92 (hit=3/10, losses=7)
```

From `apply_outcome_history_modulation`. Shows:
- Direction + multiplier (clamped [0.85, 1.10])
- Hit record: wins/resolved

Only appears when resolved count ≥ 5. Insufficient history → no note
added, no modulation. **Today (3 backfilled decisions)** this will be
silent on nearly all symbols.

---

## How I (Claude Code) should use these Monday

Reading a setup with conf=0.54 means nothing without context. Check
`risk_notes`:

| If you see... | Interpretation |
|---------------|----------------|
| `belief_modulation: down_30% ×0.70` | Eden's model of this symbol is uncertain/cold. Be cautious even with high base conf. |
| `belief_modulation: up_5% ×1.05` | Eden's model has converged confidently. Slight trust boost. |
| `outcome_history: down_10% ×0.90 (hit=2/8)` | Historical trades on this symbol have gone poorly. Historical context argues against. |
| `outcome_history: up_8% ×1.08 (hit=7/9)` | Strong historical performance on this symbol. |
| `belief_modulation: unknown_prior` | First-ever observation of this symbol by belief. Discount accordingly. |
| Symbol in `attention:` top 5 | Eden wants more observations here — **good candidate for deeper research, less so for immediate action**. |
| Symbol in `belief:` notable | Recent shift → pay attention. |
| `eden_gap: ...` on prior decisions | Read it. This is what Eden noticed we missed historically. |

**Caution principle**: a setup with modulated down conf AND in attention
top (high uncertainty) should be treated as **information-worthy, not
action-worthy**. Size down or skip, but keep watching.

---

## Data flow summary

```
Per tick (HK/US runtimes):

  Longport push → LiveState
    ↓
  dimensions → pressure_field.tick()
    ↓
  belief_field.update_from_pressure_samples(tick_layer)   // writes
  belief_field.record_state_sample(symbol, state_kind)     // writes
    ↓
  vortex_insights → insights_to_tactical_setups()
    ↓
  for setup in vortex_setups:
      apply_belief_modulation(setup, &belief_field)       // step 1
      apply_outcome_history_modulation(setup, &ledger)    // step 4
    ↓
  top_notable_beliefs(5) + top_attention(5) → wake lines
    ↓
  For symbol in notable:
      ledger.summary_for(symbol) → prior decisions wake line
    ↓
  T22/T25 anchor sort: conf + attention_boost(field)      // step 2
    ↓
  artifact_projection dispatched
    ↓
  Every 60s:
      belief_snapshot → SurrealDB (async)
      decisions/ rescan today+yesterday
```

## Persistence
- `belief_snapshot` SurrealDB table (MIGRATION_035)
- `decisions/YYYY/MM/DD/*.json` filesystem tree
- Both restore/load on startup

## End-of-session analysis
```bash
cargo run --bin dream --features persistence --release -- \
    --market hk --date 2026-04-20
# → data/dreams/2026-04-20-hk.md
```

Report contains: attention arrivals/departures/persistent + high
posterior shifts + field growth between morning and evening snapshots.

---

## Test inventory (Monday verification gates)

| Suite | Count | Purpose |
|-------|-------|---------|
| `belief_field` | 18 | Core field + notable + attention |
| `belief_modulation` | 10 | Modulation + boost math |
| `belief_snapshot` | 7 | Serialize/restore |
| `decision_ledger` | 13 | Scanner + summary |
| `outcome_history` | 7 | Stat scaling |
| `dreaming` | 6 | Diff + markdown |
| `belief_field_integration` | 4 | Restart continuity |
| `decision_ledger_integration` | 3 | Real 2026-04-15 data |
| `dream_integration` | 2 | E2E snapshot diff |
| `closed_loop_integration` | 5 | **Stack composition** |

64 tests total. Run suites individually (RocksDB linker OOM):
```
export CARGO_TARGET_DIR=/tmp/eden-target
cargo test --lib -q belief_modulation
cargo test --test closed_loop_integration -q
# etc
```

---

## Known limitations going into Monday

1. **`outcome_history` will be mostly silent**: only 3 backfilled
   decisions today. Most symbols will have insufficient history (<5
   resolved), so no modulation. This is honest — mechanism is in place
   for future data to auto-activate.

2. **`belief_field` is cold on restart**: first Monday run will load
   the latest snapshot (which is `None` — we've never actually run
   Eden with belief_field in hot path yet). Everything starts uninformed.
   Expect wake lines to be sparse for the first ~5-10 ticks per symbol.

3. **Step 3 is not done**: Eden still 100% passive on market. No probes
   emitted, no paper trades injected. Step 3 is the next big spec.

4. **Y#0 ontology emergence untouched**: the real qualitative change
   point remains a future multi-session job.

5. **Pre-existing uncommitted files** (4 `#[allow(dead_code)]` + 1 test
   rename) sit in working tree from before the session. Not mine; not
   touched. User decides.

---

## Commit trail (this session)

```
a82d243 docs(CLAUDE.md): reflect A1+A2+B+A3α completion
5ae4aa1 chore(dream,us): gate dream binary on persistence
52a49e8 feat(belief_modulation): first capability→behavior bridge
3085da9 docs(CLAUDE.md): record BM as first closed-loop step
9bf984d feat(belief_modulation): attention_boost — step 2
ae09593 docs(CLAUDE.md): mark closed loop step 2 complete
fe6c7c0 feat(outcome_history): closed loop step 4
4f6a718 docs(CLAUDE.md): closed loop 3/4 done
11bcb7d test(closed_loop): end-to-end integration test
```

Plus earlier A1/A2/B/A3α commits (fd9a4f9 → 5be80fa).

Full today: 34 commits on `codex/polymarket-convergence`.
