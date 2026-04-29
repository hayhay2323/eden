# Overnight Session 2026-04-17 — What I did while you slept

## Running right now
- **Process**: `./target/debug/eden us` (PID in `.run/eden-us.pid`)
- **Log**: `.run/eden-us.log`
- **Started**: 2026-04-17 01:33 HKT
- **Previous run log archived** to `.run/eden-us.log.prev_20260417`

Commands to check:
```bash
# Alive?
ps -p $(cat .run/eden-us.pid)

# What's happening
tail -60 .run/eden-us.log

# Stop if needed
kill $(cat .run/eden-us.pid)
```

## Commits on `codex/polymarket-convergence`

1. **`fdc9f9e`** feat(pipeline): state engine v1 + resolution wiring + session fixes
   - Baseline snapshot of Codex's state engine v1 + my P0/P2/P3/P4/P5-types work

2. **`094bf17`** feat(state_engine): Y#2 cluster/world persistent state + Y#3 absence first-class
   - Y#2 + Y#3 in one commit

## Y gap progress

| # | Status | Notes |
|---|--------|-------|
| 1 | deferred | Raw microstructure 直通 — advisor caught me about to build dimensions-on-raw (threshold scanners on raw events). Real Y-form #1 needs design discussion (embedding space? sequence similarity?) |
| 2 | ✅ | Cluster/world state now has age_ticks / state_persistence_ticks / trend / last_transition_summary, folded forward tick-to-tick. Persistence tables deferred (in-memory rolling) |
| 3 | ✅ | Absence first-class: CurrentStateFacts moved pre-classification; Continuation demoted to Latent when peer_missing; Latent demoted to LowInformation when peer_missing + raw_missing; peer_confirmation_withdrawn flips to TurningPoint. Logs show `demoted_by_absence` firing live |
| 4 | not started | Threshold removal — research-scale, needs Y-principled replacement |
| 5 | not started | Perception 回看 |
| 6 | not started | Intent-driven perception |
| 7 | not started | Cross-scale emergence |

## Signs in live log worth looking at

Working:
- `demoted_by_absence` evidence firing on VNET / OKTA / HUBS — Y#3 validating
- cluster `age_ticks` in snapshot ranges 0–46+ — Y#2 accumulating correctly
- runtime at tick ~70+, 692% CPU (7 cores), 3.97GB RSS — healthy

Known issue:
- **`sync_us_symbol_perception_states_failed: failed to sync US symbol perception states: The query was not executed due to a failed transaction`** fires every tick (~91 times in first 3 min)
  - Runtime keeps running, in-memory state engine works per tick
  - Cross-restart continuity for symbol states broken until this is fixed
  - Not a Y#2/Y#3 regression — was failing on prior runs too (likely pre-existing schema / serde interaction)
  - To investigate: `grep -B2 -A5 'sync_us_symbol' .run/eden-us.log | head -30`, check `data/eden.db` schema state, maybe fresh DB run to isolate

## Deferred tasks still on the list

- **P1 Resolution tick-loop settle path** — bootstrap hardcodes `all_settled=true`, skips Provisional→Final upgrade gate. Learning loop gets full credit instead of progressive 0.5→1.0. 1-2 day task, was too big for this session.
- **P2B HK SignalMomentumTracker** — needs HK-specific design (broker queue / depth microstructure, not US's convergence).
- **P5B Frontend perception UI cards** — TS types done, visual cards not attempted (can't visually verify from terminal).

## What I'd do next session (suggestion)

1. Fix the `sync_us_symbol_perception_states_failed` so Y#2/Y#3 state survives restart
2. Tackle Y#4 threshold removal — but only after a design conversation. The current `dec!(0.80)` / `dec!(0.67)` etc. encode real domain priors; naively removing them breaks the system
3. Look at overnight log — did `peer_confirmation_withdrawn` fire? Did any cluster `age_ticks` exceed 100? Did `demoted_by_absence` catch anything that looked like a real isolated event?
