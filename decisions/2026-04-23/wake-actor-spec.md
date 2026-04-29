# Wake Actor Spec — Eden's Operator Attention Bridge

**Author**: Claude (post-Day-1 validation)
**Date**: 2026-04-23
**Status**: Approved by operator, start implementation

## 1. Problem statement

Eden's perception layer is past threshold (MDB short signal correctly identified 3.5h before operator caught it; TXN hub correctly anchored; SNDK conflict correctly flagged). The bottleneck is the **operator interface**: wake.reasons is a flat unprioritized log requiring manual grep + cross-check + 5-gate evaluation per cycle.

Today's Day 1 gap: MDB SHORT first Eden signal at 13:30 UTC, operator entry 17:01 UTC = **3.5h latency**. Theoretical capture ~2%, actual capture 0.34% (20× underperformance attributable to operator latency, not Eden signal quality).

## 2. Goal

Build `src/pipeline/wake_actor.rs` — a top-K attention queue that:
1. Consumes the wake stream (currently emitted to log per tick by HK + US runtime)
2. Aggregates per-symbol surfaces into single candidate object
3. Scores by operator-utility (NOT entropy)
4. Attaches action verb (ENTER_LONG / ENTER_SHORT / EXIT / WATCH / SELF_DOUBT)
5. Pushes structured alerts to `.run/eden-alerts.ndjson` (NDJSON one line per alert)
6. Operator (Claude Code or human) subscribes via `tail -f` or HTTP `GET /alerts/stream`

## 3. Non-goals

- ❌ Auto-execute orders (still operator decision)
- ❌ Replace `wake.reasons` log (kept for forensics)
- ❌ Modify TacticalSetup, ConvergenceEvent, or upstream pipeline
- ❌ Cross-tick state persistence (stateless per-tick aggregation, future: add belief-decay)
- ❌ Frontend WebSocket (Phase 2 — file tail enough for Day 2 validation)

## 4. Architecture

```
HK runtime ──┐
             ├──► wake_reasons_stream ──► wake_actor ──► CandidateSetup ──► ranked alerts ──► .run/eden-alerts.ndjson
US runtime ──┘                              │
                                            ├── parser:    wake line → (symbol, surface_kind, direction, magnitude, persistence)
                                            ├── aggregator: group_by symbol, count per-direction surfaces
                                            ├── scorer:    operator_value(votes, persistence, surface_diversity, mod_stack_strength)
                                            ├── verb:      direction + score + tier → ENTER_LONG/SHORT/WATCH/SELF_DOUBT
                                            └── writer:    push top-K to NDJSON file (cap K=5 per tick)
```

## 5. Core types (Rust)

```rust
// src/pipeline/wake_actor.rs

use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use crate::ontology::Symbol;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum WakeSurfaceKind {
    ModStack,         // mod_stack: setup_id=pf:SYM:dir:window ... final=X.XXX
    SymRegime,        // [us] sym_regime: SYM action=enter ... divergence=X
    Hub,              // [us] hub: SYM anticorr_degree=N peers=... max_streak=N
    Composite,        // SYM composite=X.XXX (dim=... corr=... sec=...)
    SectorWave,       // [us] SYM | GROWING/FADING | sector_wave | dir=X
    OptionCross,      // option cross-validation: SYM Confirms/Contradicts (confidence=X)
    PressureAction,   // pressure→action: ... Long/Short SYM (enter vortex) conf=X edge=Y
    AttentionBoost,   // attention boost: SYM — hidden ...
    EmergentEdge,     // emergent edge: A ↔ B type=...
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WakeDirection {
    Long,
    Short,
    Flat,       // sector_wave dir=flat
    Conflicted, // half-conductor dir=conflicted
    Unknown,    // ambiguous, e.g., mod_stack downmod can be either side
}

#[derive(Debug, Clone, Serialize)]
pub struct WakeSignal {
    pub kind: WakeSurfaceKind,
    pub direction: WakeDirection,
    pub magnitude: Decimal,           // surface-specific normalized 0-1
    pub persistence_ticks: u64,       // streak / repetition count
    pub raw_excerpt: String,          // original wake line for forensics
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateSetup {
    pub symbol: Symbol,
    pub direction: WakeDirection,
    pub signals: Vec<WakeSignal>,     // all surfaces voting same direction
    pub vote_count: usize,
    pub max_persistence: u64,
    pub mean_magnitude: Decimal,
    pub diversity_score: Decimal,     // distinct surface kinds / total kinds
    pub operator_value: Decimal,      // final ranking score
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AlertVerb {
    EnterLong,
    EnterShort,
    Watch,        // structure forming, not yet 5-gate clean
    SelfDoubt,    // Eden flagged conflict
    Exit,         // existing position thesis flip (Phase 2)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AlertTier {
    T1, // 5+ surfaces aligned + persistence > 30
    T2, // 3+ surfaces + persistence > 15
    T3, // 2 surfaces, low persistence — WATCH only
}

#[derive(Debug, Clone, Serialize)]
pub struct EdenAlert {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub market: String, // "hk" or "us"
    pub symbol: String,
    pub verb: AlertVerb,
    pub tier: AlertTier,
    pub operator_value: f64,
    pub vote_count: usize,
    pub max_persistence: u64,
    pub surfaces_aligned: Vec<WakeSurfaceKind>,
    pub direction: WakeDirection,
    pub narrative: String, // one-sentence English summary
}
```

## 6. Parser

Each `wake.reasons` line gets dispatched to a per-kind parser. Examples:

- `mod_stack: setup_id=pf:MDB.US:short:mid30m ... final=0.855`
  → `WakeSignal { kind: ModStack, direction: Short, magnitude: 0.145 (= |1.0 - 0.855|), persistence_ticks: 0 }`
- `[us] hub: TXN.US anticorr_degree=25 ... max_streak=842`
  → `WakeSignal { kind: Hub, direction: Long (inferred from anti-corr center role), magnitude: 0.85, persistence_ticks: 842 }`
- `[us] sym_regime: MDB action=enter bucket=stress=0|sync=0|bias=0|act=3|turn=3 ... divergence=0.90`
  → `WakeSignal { kind: SymRegime, direction: Short (bias=0), magnitude: 0.9, persistence_ticks: 1 }` (cross-tick repetition counted by aggregator)

Parser implemented as `parse_wake_line(line: &str) -> Option<(Symbol, WakeSignal)>`.

**Direction inference rules**:
- `mod_stack short setup, final < 0.95` → Short (MDB pattern from Day 1, even if semantics ambiguous, follow empirical rule from successful sample)
- `mod_stack long setup, final < 0.95` → Unknown (downmod on long doesn't equal short)
- `pressure→action Long X` → Long, `Short X` → Short
- `composite < -0.3` → Short, `composite > +0.3` → Long
- `sector_wave dir=short` → Short, `dir=long` → Long, `dir=flat` → Flat (skips alignment)
- `option_cross Confirms positive` → Long, `Confirms negative` → Short, `Contradicts positive` → Short, `Neutral` → Unknown
- `hub` direction = sign of price change of the hub anchor (need to look up from latest tick state) — Phase 2; for v1, hub doesn't vote direction, just signals "structure exists"

## 7. Aggregator

```rust
pub fn aggregate_candidates(signals: &[(Symbol, WakeSignal)]) -> Vec<CandidateSetup> {
    let mut by_symbol: HashMap<Symbol, Vec<WakeSignal>> = HashMap::new();
    for (sym, sig) in signals {
        by_symbol.entry(sym.clone()).or_default().push(sig.clone());
    }

    let mut candidates = Vec::new();
    for (sym, sigs) in by_symbol {
        // Group by direction
        let mut by_dir: HashMap<WakeDirection, Vec<WakeSignal>> = HashMap::new();
        for s in sigs {
            if matches!(s.direction, WakeDirection::Long | WakeDirection::Short) {
                by_dir.entry(s.direction).or_default().push(s);
            }
        }
        // Take dominant direction
        if let Some((dir, sigs)) = by_dir.into_iter().max_by_key(|(_, s)| s.len()) {
            if sigs.len() >= 2 {
                candidates.push(build_candidate(sym, dir, sigs));
            }
        }
    }
    candidates
}
```

## 8. Scorer (operator_value)

```rust
fn operator_value(c: &CandidateSetup) -> Decimal {
    // Weights tuned to Day 1 MDB signal characteristics
    let vote_weight = Decimal::from(c.vote_count) * Decimal::new(25, 2); // 0.25 each
    let persistence_weight = (c.max_persistence.min(100) as i64).into();
    let persistence_norm: Decimal = persistence_weight / Decimal::from(100); // cap at 1.0
    let magnitude_weight = c.mean_magnitude;
    let diversity_weight = c.diversity_score; // 0-1

    // Geometric mean of (votes, persistence, magnitude, diversity)
    // High score requires ALL dimensions, not just one
    let product = vote_weight.min(Decimal::ONE)
        * persistence_norm
        * magnitude_weight
        * diversity_weight;

    // 4th root via log/exp would be ideal; for v1 use simple weighted sum:
    (vote_weight * Decimal::new(40, 2)
        + persistence_norm * Decimal::new(30, 2)
        + magnitude_weight * Decimal::new(20, 2)
        + diversity_weight * Decimal::new(10, 2))
        .min(Decimal::ONE)
}
```

## 9. Verb dispatcher + Tier assignment

```rust
fn classify(c: &CandidateSetup) -> (AlertVerb, AlertTier) {
    let tier = if c.vote_count >= 5 && c.max_persistence >= 30 {
        AlertTier::T1
    } else if c.vote_count >= 3 && c.max_persistence >= 15 {
        AlertTier::T2
    } else {
        AlertTier::T3
    };

    let verb = match (c.direction, tier) {
        (WakeDirection::Long, AlertTier::T1 | AlertTier::T2) => AlertVerb::EnterLong,
        (WakeDirection::Short, AlertTier::T1 | AlertTier::T2) => AlertVerb::EnterShort,
        (_, AlertTier::T3) => AlertVerb::Watch,
        _ => AlertVerb::Watch,
    };

    // SelfDoubt override: if any signal has Conflicted direction, force SelfDoubt
    if c.signals.iter().any(|s| s.direction == WakeDirection::Conflicted) {
        return (AlertVerb::SelfDoubt, AlertTier::T3);
    }

    (verb, tier)
}
```

## 10. Writer / Push API

V1: append-to-file NDJSON
- Path: `.run/eden-alerts-{market}.ndjson`
- One JSON object per line
- Operator subscribes: `tail -f .run/eden-alerts-us.ndjson | jq`

```rust
pub fn write_alerts(market: &str, alerts: &[EdenAlert]) -> std::io::Result<()> {
    let path = format!(".run/eden-alerts-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    for alert in alerts {
        let line = serde_json::to_string(alert).unwrap();
        writeln!(file, "{}", line)?;
    }
    Ok(())
}
```

V2 (deferred): SSE endpoint `GET /api/{hk|us}/alerts/stream` for frontend live feed.

## 11. Top-K cap + dedup

- Cap K = 5 per tick (avoid alert flood)
- Dedup: same (symbol, verb, tier) within last 30 ticks → suppress (only emit when alert content changes)
- Emit only if `operator_value >= 0.40`

## 12. Wiring into runtime

`src/hk/runtime.rs` — at end of each tick after wake.reasons emit:

```rust
let signals: Vec<(Symbol, WakeSignal)> = wake.reasons
    .iter()
    .filter_map(|line| wake_actor::parse_wake_line(line))
    .collect();
let candidates = wake_actor::aggregate_candidates(&signals);
let mut scored: Vec<_> = candidates.into_iter()
    .map(|c| { let v = wake_actor::operator_value(&c); (c, v) })
    .filter(|(_, v)| v >= Decimal::new(40, 2))
    .collect();
scored.sort_by(|a, b| b.1.cmp(&a.1));
scored.truncate(5);

let alerts: Vec<EdenAlert> = scored.into_iter().map(|(c, v)| {
    let (verb, tier) = wake_actor::classify(&c);
    wake_actor::build_alert("hk", c, verb, tier, v)
}).collect();

if !alerts.is_empty() {
    let _ = wake_actor::write_alerts("hk", &alerts);
}
```

US runtime gets symmetric snippet.

## 13. Testing

`#[cfg(test)] mod tests` in wake_actor.rs:

1. `parse_mod_stack_short_setup` — recognized, direction Short, magnitude correct
2. `parse_pressure_action_long` — recognized, direction Long
3. `parse_option_cross_confirms_negative` — direction Short
4. `parse_hub_with_streak` — kind Hub, persistence_ticks = streak
5. `aggregate_two_signals_same_dir` — single CandidateSetup with vote_count=2
6. `aggregate_conflicting_signals` — picks dominant direction
7. `classify_t1_long` — 6 long signals + persistence 50 → EnterLong T1
8. `classify_t3_watch` — 2 signals only → Watch T3
9. `classify_self_doubt` — any Conflicted signal → SelfDoubt T3
10. `operator_value_geometric_skew` — 1 surface even with high magnitude scores below 3 surfaces with mid magnitude
11. `top_k_cap` — 10 candidates → only top 5 emitted
12. `dedup_within_30_ticks` — same alert emitted twice → second suppressed (Phase 2)

## 14. Acceptance criteria

- All 12 unit tests pass
- `cargo check --lib --features persistence` clean
- Restart HK + US runtime, observe `.run/eden-alerts-{hk,us}.ndjson` populated within 5 ticks
- For Day 1 MDB-replay scenario: a `MDB.US ENTER_SHORT T1` alert emitted within 5 ticks of first mod_stack 0.855 fire (validate against today's log timestamps)

## 15. Implementation order

1. ✅ Spec written (this doc)
2. Create `src/pipeline/wake_actor.rs` with types + parser
3. Implement aggregator + scorer + classifier
4. Add 12 unit tests
5. `cargo test --lib --features persistence wake_actor::tests`
6. Add writer
7. Wire HK runtime hook
8. Wire US runtime hook
9. `cargo check --lib --features persistence` clean
10. Build + restart HK, validate `.run/eden-alerts-hk.ndjson` populates
11. Wait next US session, validate `.run/eden-alerts-us.ndjson`
12. Day 2 trade with alert-driven flow, measure operator latency reduction

## 16. Risk & mitigation

| Risk | Mitigation |
|---|---|
| Alert flood (>5/tick across markets) | Hard cap K=5 + operator_value>=0.40 floor |
| False positive verb (wrong direction) | Direction inference rules conservative; `Unknown` direction excludes from voting |
| Wake parser brittle to log format change | Tests pin exact regex; add log_format_version constant |
| NDJSON file unbounded growth | Caller's responsibility (logrotate or cron truncate); spec doesn't manage |
| Two markets racing on same file | Separate files per market (`.ndjson` suffix has market) |
| mod_stack direction semantics still wrong | Direction inference is **rule-based and overridable**; if Day 2 data shows MDB pattern is wrong, change rule in one place |

## 17. Out of scope (future iterations)

- Cross-tick belief-decay scoring (alert "freshness" decays over 15 min)
- Operator click-through learning (which alerts I act on → upweight that surface combo)
- Frontend SSE endpoint
- Auto-trigger order draft (alert → pre-filled `mcp_submit_order` params for one-click execute)
- Belief-update from outcome (A2.5)
