# Polymarket Legacy Removal Plan

**Date**: 2026-04-26  
**Author**: Claude (architecture pass with operator)  
**Status**: Awaiting operator approval before execution  
**Trigger**: Polymarket integration design conversation surfaced existing 13k-line dormant integration that conflicts with newly-agreed architectural principles.

## Why this exists

Today's design conversation produced four principles for any future Polymarket integration:

1. **Don't pollute ontology** — Polymarket markets are observations, not entities
2. **No human-curated mapping** — relevance must emerge from MI / lead-lag statistics
3. **Filter = use** — single state object (correlation matrix), not separate filter step
4. **Polymarket-leads-Eden test** — only surface markets that contain information Eden's microstructure has not already absorbed

A pre-existing integration was discovered (commit `16123a6`, 2026-03-20, 13,153 LOC across 22 files) that violates **all four**:

| Principle | Existing implementation |
|---|---|
| No ontology pollution | `PolymarketPrior.scope: ReasoningScope` + `target_scopes: Vec<ReasoningScope>` directly attaches priors to symbols/sectors/themes |
| Emergent relevance | `config/polymarket_markets.json` with hand-keyed slug → bias → scope mapping |
| Statistical filter | `conviction_threshold: Decimal::new(6, 1)` hardcoded 0.6 binary gate |
| Pre-encoded direction | `enum PolymarketBias { RiskOn, RiskOff, Neutral }` — direction declared, not derived |
| Lead-lag | None — `is_material()` only checks `probability >= threshold` |
| Single-state-object | Separate `PolymarketSnapshot` + `PolymarketPrior` + `decision::apply_polymarket_snapshot` + `compute_polymarket_dynamics` paths |

The integration is currently **dormant** — `config/polymarket_markets.json` does not exist (only `.example`), so `load_polymarket_configs()` returns `Vec::new()` on every tick and all downstream paths receive empty priors. Eden's last ~5 weeks of operation have effectively been "polymarket-free" through this dead-but-still-compiled-in code.

This is exactly the kind of rule-based template-injection module that the **2026-04-25 diet** removed (case_narrative, intent_modulation, sector_alignment, broker_alignment, symbol_inference, option_inference — total 2,869 lines). Polymarket integration is the largest remaining instance and was missed by that pass.

## Decision

**Delete all existing Polymarket integration in a single cleanup commit.** Phase 1 of any new integration (post-hoc dreaming-only backfill, no runtime touch, no ontology touch) starts from a clean surface afterwards.

## Scope of removal

### Files to delete entirely (4)

| Path | Lines | Note |
|---|---|---|
| `src/external/polymarket.rs` | 509 | Core module: HTTP fetch, config loader, `PolymarketBias/ScopeKind/Prior/Snapshot/MarketConfig`, `parse_target_scope` |
| `src/external/mod.rs` | 2 | Only contains `pub mod polymarket;` |
| `src/external/` directory itself | — | Becomes empty after the two above |
| `config/polymarket_markets.json.example` | 23 | Example config never instantiated |

### Files to edit (25)

Sorted by occurrence count. All edits remove polymarket usage entirely; no replacement logic added in this commit.

| File | Refs | Surgical scope |
|---|---|---|
| `src/pipeline/world.rs` | 30 | Remove `polymarket: Option<&PolymarketSnapshot>` param from worldview/reasoning entry points; remove `strongest_market_prior`, `append_polymarket_entities`, `polymarket:` driver text; clean up entity formation that read polymarket priors |
| `src/hk/runtime/state.rs` | 16 | Remove `fetch_polymarket_snapshot` future from runtime state composition; remove polymarket field from runtime state struct |
| `src/hk/runtime.rs` | 15 | Remove `merge_external_priors`, `apply_polymarket_snapshot` call site, `previous_polymarket` history reads, `compute_polymarket_dynamics` invocation, all polymarket bridge plumbing |
| `src/cli/query.rs` | 11 | Remove `query polymarket` subcommands |
| `src/bridges/us_to_hk.rs` | 11 | Remove `to_polymarket_snapshot` and supporting helpers |
| `src/temporal/analysis.rs` | 9 | Remove `compute_polymarket_dynamics`; clean up reads of `record.polymarket_priors`; update tests |
| `src/graph/decision_tests.rs` | 9 | Drop polymarket test setups |
| `src/api/core/health.rs` | 9 | Remove `/polymarket/snapshot` (or equivalent) endpoint handler |
| `src/graph/decision/orders.rs` | 8 | Remove polymarket-aware ordering logic |
| `src/pipeline/world_tests.rs` | 7 | Drop polymarket fixtures |
| `src/hk/runtime/startup.rs` | 7 | Remove `load_polymarket_configs` call + warmup |
| `src/graph/decision/regime.rs` | 6 | Remove `MarketRegime::apply_polymarket_snapshot` |
| `src/temporal/record.rs` | 4 | Remove `polymarket_priors: Vec<PolymarketPrior>` field on `TickRecord`; remove from constructor signature |
| `src/temporal/lineage_tests.rs` | 4 | Drop empty-vec test placeholders |
| `src/hk/runtime/display/microstructure/market.rs` | 4 | Remove polymarket display surface |
| `src/graph/decision.rs` | 4 | Remove `apply_polymarket_snapshot` method on decision; clean import |
| `src/cli/render.rs` | 4 | Remove polymarket render path |
| `src/core/market_snapshot.rs` | 3 | Drop polymarket field from market snapshot |
| `src/cli/parser.rs` | 3 | Remove polymarket subcommand parser |
| `src/cli/commands.rs` | 3 | Remove polymarket command dispatch |
| `src/hk/runtime/display.rs` | 2 | Remove polymarket display call |
| `src/cli.rs` | 2 | Remove polymarket re-export |
| `src/api/core/router.rs` | 2 | Drop polymarket route registration |
| `src/temporal/causality.rs` | 1 | Remove empty placeholder |
| `src/temporal/buffer.rs` | 1 | Remove empty placeholder |
| `src/pipeline/signals.rs` | 1 | Remove empty placeholder |
| `src/persistence/store/tests.rs` | 1 | Drop test fixture |

### Schema migration

Add `MIGRATION_042` (current head is `MIGRATION_041`):

```sql
REMOVE FIELD polymarket_priors ON TABLE tick_record;
```

Existing rows lose the field on the next schema apply. No data migration needed — every persisted row has `polymarket_priors = []` (empty array) because the integration was dormant.

`src/persistence/schema.rs` line 29 (`DEFINE FIELD polymarket_priors ON tick_record TYPE array;`) stays in `MIGRATION_001` (migrations are append-only per CLAUDE.md convention) — the new `MIGRATION_042` removes it.

### Cargo.toml

**No changes.** `reqwest` and `rust_decimal` (only deps that the original commit added) are used pervasively elsewhere in Eden (Longport HTTP, decimal math throughout pipeline). They stay.

## Verification steps

Run in order; each must pass before the next.

1. `cargo check --lib -q` — compilation passes after edits
2. `cargo check --tests -q` — test compilation passes
3. `cargo build --bin eden --features persistence -q` — production binary builds
4. `cargo test --lib -q` — default test suite passes (currently 999 passing per memory `project_belief_persistence_a1.md`)
5. Run `eden` once with `--features persistence` against test SurrealDB → confirm `MIGRATION_042` applies cleanly + tick loop runs without panic
6. Grep `grep -ri polymarket src/` returns 0 matches (final cleanup check)

If any step fails, abort and investigate before continuing — do not amend half-finished cleanups.

## Rollback

Single commit; `git revert <hash>` restores everything. Persistence schema concern: if the migration applied and then we revert, `tick_record.polymarket_priors` will be missing on existing tables. Reversion of `MIGRATION_042` would require an additional `MIGRATION_043` re-defining the field. In practice, since dormant data never populated this field, the revert is safe but cosmetically requires either:
- Wipe SurrealDB (acceptable — eden-hk.db / eden-us.db are reproducible from tick replay)
- Or add a re-define migration

Recommend: do not revert; if the new emergent design lands as planned (Phase 1 backfill → Phase 2 online correlation → Phase 3 KL projection), no rollback is needed.

## Out of scope (explicitly NOT doing)

- ❌ Building any new Polymarket integration in this commit. This is **delete only**.
- ❌ Touching `Cargo.toml` deps (`reqwest`, `rust_decimal` used elsewhere).
- ❌ Modifying belief field, ontology, or any non-Polymarket Eden module.
- ❌ Adding `world_belief_field.rs`, `world_correlation_field.rs`, or any Phase-2/3 module.
- ❌ Documenting future Polymarket design here. That goes in a separate decision doc when Phase 1 spike returns go/no-go data.

## Estimated size

- ~13,000 LOC removed (consistent with original commit `16123a6` adding 13,153)
- ~25 files edited, 4 files deleted, 1 directory removed
- 1 schema migration added (`MIGRATION_042`, ~3 lines)
- Single cleanup commit

## Sequencing after merge

1. Decision doc approved (this file) — operator sign-off
2. Execute removal — one commit, six verification gates (above)
3. Begin Phase 1 spike: pull Polymarket Gamma + CLOB historical for 5-10 high-prior markets (Fed-April-50bps-cut already verified σ-additivity ≈ 0.9999 across 4-market partition); load into Python notebook; cross-correlate against `belief_snapshot` rows (A1, started 2026-04-19, ~7 days available); produce 1-page feasibility memo as `decisions/2026-04-XX/polymarket-feasibility.md`
4. Go/no-go on Phase 2 based on feasibility memo

---

## Appendix: spike findings that triggered this doc

- Polymarket Gamma + CLOB APIs work; 1-minute fidelity available over short windows, 12-hour fidelity over 358 days, hourly default — sufficient resolution for lead-lag against Eden microstructure
- Top-20 markets by 24h volume self-sort into Eden-relevant (Fed × 4 markets, Bitcoin × 2, Iran/Hormuz oil-relevant) and Eden-irrelevant (Nuggets vs Timberwolves, FIFA, UFC, Phillies vs Braves) — confirms framework's core claim that volume-gating + MI eliminates noise without manual filtering
- Fed-April 4-market partition (50+ cut / 25 cut / no change / 25+ raise) currently sums to **0.9999** — clean σ-additivity confirms binary options form A-D security partitions as theorized
- Existing `polymarket_priors` field on `tick_record`: every persisted row contains empty array → integration confirmed dormant
