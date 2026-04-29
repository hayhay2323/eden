# Eden US Operator Session — 2026-04-14

## Round 1 — tick 292 @ 15:56 UTC

### Eden state
- market: Us, regime neutral (confidence 0.32), breadth 65 up / 34 down, pre-mkt sentiment +0.11
- tick 292, 639 symbols, 22 hypotheses, 12,747 edges, 1772 observations
- scorecard cumulative: 154,638 resolved, 36.43% hit_rate (mean_return ~0)
- active_positions: 0
- 10 tactical_cases (all review/observe — zero `enter`)

### Cluster identified (clear signal)
Crypto / theme cluster pre-mkt follow-through:
- **MSTR** +6.16% composite 0.55 (top), near day high 143.69
- **CLSK** +6.41% composite 0.54, sector_coherence 0.38
- **MARA** +3.03% composite 0.40, Growing on fast5m AND mid30m, driver both microstructure + sector_wave, peer_conf=1 (4/4), appears 3× in tactical_cases
- **RIOT** composite 0.41 (crypto confirm)
- **BITO/COIN** also in top 15 (crypto basket wide)

China ADRs rally (second cluster):
- **JD** +5.97% (orphan signal, at day high)
- **KC / VNET** in top signals

Orphans (price moved without case translation yet):
- **QUBT** +12.6% (biggest mover, orphan, extended)
- **MSTR/JD** (flagged by `signal translation gap` — Eden knows but hasn't produced vortex case)

### Decision
**BUY MARA.US 500 @ 10.75 (LO RTH_ONLY)** — order 1228736048700362752
Why: MARA is the cleanest "full-stack Eden" play:
1. Topology: Growing lifecycle, positive accel (0.0535 on mid30m)
2. Peer confirmation: 1.0 (crypto cluster all confirming)
3. Dual driver: microstructure + sector_wave (not mono-cause)
4. Raw sources: aligned (calc_index + quote support)
5. Price not yet extended vs cluster: +3% while MSTR/CLSK already +6%
6. Small size (~$5,400 USD) — round 1 test

### NOT entered
- **MSTR/CLSK**: extended near day high, risk/reward worse though Eden conviction higher
- **SOUN**: vel=0.094 acc=0.097 is topology-hot but price action (+3%) doesn't confirm → possible Eden false positive or lag; testing on next round
- **QUBT**: +12.6% orphan, no case translation, pure chase
- **JD**: at day high, orphan

### Observations on Eden
1. **Signal translation gap is real** — QUBT +13%, MSTR +6%, JD +6% all flagged as "strong top signal is not yet represented in tactical cases". This confirms pressure→vortex→case pipeline has latency. The gap itself is useful information ("Eden is behind the market here").
2. **capital_flow_direction = 0 for all top signals** — suspicious, looks uninitialized or disabled for US. Need to verify if US capital flow is computed at all.
3. **All cases are review/observe, none are enter** — Eden is conservative in US runtime. Per feedback notes, this was intentional after prior over-entry. But means operator must still call the trigger.
4. **BB divergence** — Growing mid30m but Peaking session (acc=-0.204). Valuable lifecycle disagreement. Worth surfacing prominently.
5. **Duplicate symbol cases** (MARA appears 3×, BB 2×) — is this signal or noise? Probably noise unless one case has unique driver. Should dedupe.
6. **`case_signature` topology/temporal/conflict all "unknown"** — these structural fields look unpopulated. Might be a gap in the new topology reasoning layer.
7. **novelty_score=0.6 on every case** — a constant is not information.
8. **scorecard 36.4% hit_rate** — cumulative across all families. Would be much more useful broken down per-family so I know which Eden signal is trustworthy.

### Improvement ideas (accumulating)
- [ ] Dedupe tactical_cases by (symbol, direction) keeping best-confidence, or at minimum order by uniqueness
- [ ] Surface signal_translation_gaps more prominently — they're actionable ("Eden sees price move but can't explain yet")
- [ ] Per-family scorecard breakdown in live_snapshot (currently only aggregate)
- [ ] Investigate why capital_flow_direction is 0 for all US top_signals
- [ ] Populate case_signature.topology/temporal/conflict (currently always "unknown")
- [ ] Surface `causal_leaders` and `backward_chains` in tactical cases — they're in the snapshot but not wired to case context
- [ ] Lifecycle-divergence alert (same symbol: Growing short-horizon + Peaking long-horizon) is a high-value pattern

## Round 2 — tick 401 @ 15:59 UTC

### Fill
MARA.US 500 @ **10.670** (filled, order from round 1). Current 10.665, P&L ~flat.

### Eden state delta
- 10 cases still, but roster churned: F.US, AMC.US, FUTU.US, GM.US, MSTR.US joined; SOUN dropped to vel 0.010 (from 0.094 prior round — **confirmed first-read was noise**)
- MARA mid30m: vel 0.024→0.097 (×4), acc 0.053→0.208 (×4) — topology signal *accelerating*
- BB.US: vel 0.028 acc 0.024 — calmed from prior Peaking
- scorecard/regime unchanged

### Anomaly: F.US action=enter on orphan_signal
```
symbol: F.US, action: enter, confidence 0.68
driver_class: orphan_signal
rationale: "strong top signal is not yet represented in tactical cases"
lifecycle: null, velocity: null, peer: null
```
**Contradiction**: orphan = no underlying vortex/reasoning translation. An orphan by definition has no topology backing. Policy upgrading it to `enter` looks like a bug: orphans should cap at `review`. Either (a) policy layer isn't gating on driver_class, or (b) the orphan path feeds composite confidence that trips the enter threshold.
File to check: `src/us/pipeline/reasoning/policy.rs` + wherever `strong top signal is not yet represented` rationale is composed.

### Topology vs Price divergence (MARA)
Eden: vel/acc both ×4 this tick → "wave building fast"
Market: MARA ran to 11.01 early, now back to 10.665 → "wave faded"
Eden's lifecycle signal is reading the **pressure field** acceleration, not the realized price. The pressure field measures *potential* (order flow, volume, structure pressure), which can build before price or without price. Two scenarios:
- Early-edge: pressure builds, then price follows (Eden's hypothesis)
- False signal: pressure measures lagging flow that never translates to price

We don't know which yet — this is exactly why Eden needs a **realized-outcome correlation per lifecycle phase** to learn. My bet: for crypto names during session, Growing+accel usually means re-entry setup after morning fade, but this needs data.

### Decision
HOLD MARA. No new entries.
- Not F.US — orphan/enter contradiction
- Not adding MARA — topology strengthening but price diverging; want one more tick of confirmation
- Not AMC — new case, no history, small dollar anyway
- Not QUBT — still orphan

### Observations
- **Eden's first-tick lifecycle readings are noisy** (SOUN velocity 0.094→0.010 in 3 min). Should either (a) hide cases until 2-3 ticks of consistent readings, or (b) show "tick_count_in_phase" so operator knows how mature the signal is.
- **Roster churn in 3 min is high**: 5 of 10 slots changed. Suggests case generation is reactive to noise; needs hysteresis on case admission (not just pressure).
- **MARA 3× dedup did not happen** — still two MARA entries (fast5m, mid30m). Two horizons is legitimate multi-scale, but visually confusing without better case grouping.
- **No enter actions are reaching the operator from vortex cases** — only from orphan bypass path. The two pathways are structurally asymmetric: vortex cases top out at review, orphan cases reach enter. This is the opposite of what you want (real topology reasoning should be the *more* confident path).

### Improvement ideas (new)
- [ ] Gate `action=enter` on `driver_class != orphan_signal` (or require orphan confidence ≥ 0.80 AND multi-source agreement)
- [ ] Add `ticks_in_current_phase` to lifecycle cases — operator needs to know if Growing is fresh (noisy) or sustained (signal)
- [ ] Case admission hysteresis: require N consecutive ticks of matching topology before promoting pressure vortex to tactical_case
- [ ] Add `price_confirmation` field to vortex cases: does price action align with lifecycle direction? If topology says Growing but 5m price is flat/down, flag disagreement
- [ ] Group same-symbol cases (multi-horizon) under one container to reduce roster churn display; MARA at fast5m + mid30m should be 1 visual entry

## Round 3 — tick 495 @ 16:01 UTC

### Operator state
- Longport MCP **disconnected this round** — cannot query positions or trade. Paper position from round 1 still open: MARA 500 @ 10.670.
- Fallback price read from `data/us_operational_snapshot.json` via `state.signal.mark_price`. MARA mark 10.650 → ~-$10 P&L (noise).

### Eden state delta
- 10 cases, heavy churn again: **MSTR, GME, RIVN, IBIT, COIN, QUBT, GDS, VNET** joined; MARA, BB, AMC, FUTU, GM, F, SOUN dropped or rotated.
- **MSTR.US action=enter, driver=orphan_signal** — same bug pattern as F.US last round. This is now a **confirmed recurring issue**: the only `action=enter` emissions reaching the operator come via the orphan path.
- **MARA demoted**: from 3× review cases (fast5m+mid30m growing+accel) to a single fast5m observe (vel=0.011 acc=0.000). The mid30m vel=0.097 acc=0.208 peak in round 2 **lasted exactly one tick**. One-tick spike on mid30m horizon is almost certainly a measurement artifact, not a real 30-min lifecycle phase.
- **QUBT graduated from orphan → vortex case** this round (driver now sector_wave, peer 1.0, vel 0.040 acc 0.042). Positive: Eden's pipeline *does* eventually fold orphans into structured cases. Negative: it took ~6 min, during which time the price already ran.
- **GME.US new**: vel=0.142 acc=0.108 mid30m — highest reading this round. Based on MARA's round-2 experience, I should *heavily discount* first-read mid30m extremes. If GME still reads that hot next tick, take seriously.

### Confirmed: `capital_flow_direction = 0` for ALL 639 US symbols
```
jq '[.symbols[] | select(.state.signal.capital_flow_direction != "0" and ... != null)] | length' = 0
```
The US capital flow channel is producing zero for every symbol. Either:
- US runtime isn't calling capital_flow REST polling
- Capital flow computation is being dropped or default-filled
- Serialization bug flattening the float to "0"

This is a **dead input channel in the convergence composite** — one of the key "dimensions" in `composite = weighted(capital_flow, momentum, volume, pre_post, valuation, ...)`. Whatever weight capital_flow has is currently zero, so the composite is effectively missing a leg. **Must fix** before trusting composite rankings.

File lead: `src/us/pipeline/dimensions.rs` capital flow path; also check whether Longport capital_flow endpoint is being subscribed in US bootstrap.

### Decision
**HOLD MARA** (can't trade anyway with MCP down).
- MARA topology signal collapsed (mid30m gone, fast5m weakened). If I could trade, I'd reduce or exit here — the basis for the round-1 entry (strong Growing+accel on mid30m) no longer holds at round 3. This is the first "Eden told me to get in, now Eden no longer tells me to hold" moment of the session.
- Would NOT enter MSTR on the enter bug — same orphan path as F.US.
- If MCP recovers, next round: consider exit MARA on topology fade, not price stop.

### Observations
1. **Signal half-life looks ~3-6 minutes on mid30m horizon.** That's inconsistent with a 30-minute horizon label. The "mid30m" bucket may be mislabeled — it's the *slot* in which a fast5m-like signal is placed when it crosses a threshold, not a 30-min sustained phase.
2. **Case roster churn rate ~50% per 3 min.** Extremely high. From an operator view this is unusable — by the time I've read one round, the cases are half different. Need stickiness: either keep faded cases marked as "fading" in the roster for N ticks, or surface "persistent growers" as a separate list.
3. **`orphan → vortex → case` pipeline exists and works**, but with ~6 min latency. For a session trader that's too slow. Either (a) speed up translation, or (b) accept orphans as first-class signals with their own confidence calibration, not mixed into the same action policy.
4. **I cannot easily see price change / %** in the snapshot. `mark_price` alone isn't enough; need `change_pct` and `volume_ratio` in case context. Today I had to pull Longport quotes separately in rounds 1-2 and fall back to a mark-only read now.

### Improvement ideas (new)
- [ ] **capital_flow_direction=0 for all US symbols** — debug root cause, highest priority. A dead dimension distorts every composite.
- [ ] **Separate orphan action policy** from vortex action policy. Orphans should never reach `enter` on their own; they should only *augment* an existing vortex case.
- [ ] **Lifecycle phase minimum duration** — require ≥2-3 ticks in phase before exposing velocity/acceleration to policy layer. Kill first-tick extremes.
- [ ] **Persistence flag on tactical_cases** — tag each case with `first_seen_tick` so operator can filter out new/ephemeral vs sustained cases.
- [ ] **Enrich mark data** in case rows — include `change_pct`, `volume_ratio`, `5m_change`, so operator doesn't need external quote calls to judge price-topology alignment.
- [ ] **Case exit signal**: when a case was in my active position list last tick and drops off this tick (or demotes from review to observe), emit an explicit "Eden no longer supports this position" notice. Don't make the operator diff rosters by hand.

## Round 4 — tick 618 @ 16:04 UTC

### Operator state
- Longport MCP still disconnected. Position unchanged (MARA 500 @ 10.670).
- MARA mark: **10.710** (+$20, +0.37%) — first modest green.

### Eden state delta
- 10 cases: **JD now action=enter/orphan_signal** — THIRD consecutive round with an orphan-path `enter` emission (round 2 = F.US, round 3 = MSTR, round 4 = JD). Pattern locked.
- **MARA re-supported**: both horizons back. fast5m micro (vel 0.011 acc 0.000), mid30m sector_wave (vel 0.010 acc 0.016). Weaker than round-2 peak but still Growing+peer_conf=1. Position thesis re-confirmed.
- **BB.US mid30m vel 0.1149 acc 0.1270** — first-read extreme (same pattern as round-2 MARA). Expect this to decay next tick.
- **STLA.US mid30m vel 0.1057 acc 0.1079** — same pattern.
- Cluster rotated: crypto+quantum (rounds 2-3) → crypto+autos+airlines (round 4, STLA/TDG/DAL). The theme persistence is ~1 round.

### Cross-file consistency issue
`live_snapshot.tactical_cases[BB.US]` says `action=review` (high conf, strong vel)
`operational_snapshot.symbols[BB.US].state.structure.action` says `action=observe`
These are emitted at nearly the same tick. Different parts of Eden's output disagree on the same symbol's current action. Operator shouldn't need to pick which file to trust.

### Root-cause hypothesis for orphan-enter bug
Every orphan-enter case (F, MSTR, JD) has the same dimension profile:
```
capital_flow_direction: 0       (UNIVERSALLY zero — dead channel)
price_momentum:         1
pre_post_market_anomaly: 1
```
And the raw data shows these names have **net negative trade flow** during session:
- QUBT: buy 10% / sell 42% (trade imbalance -32)
- JD:   buy 19% / sell 34% (round 1)
- MSTR: buy 17% / sell 18% (balanced)

So the rally is being driven by overnight gap + short covering / ETF rebalance, not session accumulation. Eden **cannot see this** because:
1. capital_flow_direction dimension is dead (returns 0 for all 639 symbols)
2. the trade-flow imbalance signal appears to sit inside `raw_highlights` but is not penalizing the composite
3. composite = weighted(momentum, pre_post, volume, valuation, ...) — with capital_flow silent, two of the remaining three big weights (momentum, pre_post) max out during gap-ups
4. high composite + no vortex case yet → orphan_signal path → action=enter

**This is one bug producing multiple symptoms.** Fixing capital_flow_direction alone likely eliminates most orphan-enter false alarms.

### Decision
**HOLD MARA** (cannot trade anyway).
If trading: would add 200 more @ 10.70 — position thesis re-supported, price finally ticked up, MARA is one of the few names with dual-horizon Eden backing AND raw flow consistency. But I'd wait for one more round of sustained mid30m reading before committing. Given MCP is down, moot.

### Observations
1. **Price finally caught up to Eden topology on MARA** — round 1 entry at 10.670, rounds 2-3 the topology strengthened while price faded, round 4 price came back. The hypothesis "pressure builds then price follows" has one data point in its favor on a 9-minute window. Too few to generalize.
2. **Orphan symbols repeat** — MSTR, JD, QUBT keep showing up in the signal_translation_gaps list. Either they're genuinely strong and Eden is slow, or they're genuinely weak and Eden is correctly not translating them, and only the orphan-enter bug is dragging them into action. Data supports the second: trade imbalance is net sell.
3. **The operator has to re-read the entire case roster every round** to figure out what changed. There's no diff/delta view. This is a huge cognitive tax.

### Improvement ideas (new)
- [ ] **Unify action source of truth** — `live_snapshot.tactical_cases` and `operational_snapshot.symbols[*].structure.action` must agree or explicitly diverge with a reason.
- [ ] **Trade-flow penalty in composite** — when buy% < sell% and price is up, penalize the momentum dimension. This detects gap-up on weak flow.
- [ ] **Dimension health report** — flag any convergence dimension that reads zero/constant across all symbols. capital_flow_direction=0 for 639/639 should be a loud runtime warning, not silent.
- [ ] **Delta view between rounds** — next to current cases, show `{new, sustained, demoted, removed}` since previous tick so operator sees what changed without diffing by hand.
- [ ] **Round-2 first-read decay pattern**: the "vel/acc both > 0.1 on first mid30m appearance" pattern seen on MARA (r2), BB (r4), STLA (r4) — empirically these fade next tick. Either the mid30m window EMA needs a longer warmup, or first-tick entries into mid30m phase should be suppressed.

## Round 5 — tick 742 @ 16:07 UTC

### Operator state
- Longport MCP still disconnected. MARA 500 @ 10.670 held. Mark 10.700, +$15.
- Session P&L: essentially flat, but notably MARA has been green (not red) for 2 consecutive rounds now.

### Eden state delta
- 10 cases: **ZERO enter, ZERO orphan** — first "clean" round of the session. All 10 cases are vortex-backed (microstructure or sector_wave), all action=review.
- **MARA dual-horizon still supported** — vel/acc numerically *identical* to round 4 (0.011/0.000 fast5m, 0.010/0.016 mid30m). Either the readings are genuinely stable (2 consecutive ticks of the same lifecycle reading = my longest-lived signal this session) or they're being cached/snapshotted from one source. Need to verify this isn't a stale-read artifact.
- **Roster rotation**: BB and STLA gone (confirmed my round-4 prediction that first-read mid30m >0.1 fades). FUTU, HPQ, ZLAB, TSLA new. IBIT upgraded to dual-horizon (both fast5m micro + mid30m sector, peer ~0.95) — second name after MARA with MARA-style persistence.
- **FUTU mid30m vel=0.0955 acc=0.0935** — yet another fresh first-read extreme. Fourth time this session. Pattern is reliable: fresh mid30m entrants spike near 0.1 and decay.
- hypothesis_count 22→27 — growing slowly tick over tick.

### Another source-of-truth inconsistency
HPQ.US: live_snapshot tactical_cases says Growing review, but operational_snapshot says composite=**-0.138** (negative!). A Growing lifecycle with a negative composite is contradictory:
- Pressure field (topology): "tension building"
- Convergence composite (cross-dimension snapshot): "net weak"
These should not both be surfaced on a review recommendation. Either one is wrong, or Eden needs an explicit `topology_vs_composite_disagreement` flag.

### Decision
**HOLD MARA.** No trade actions available. No new entries would be made even if MCP were up:
- MARA: sustained at current level, no trigger for more
- IBIT: dual-horizon but composite is only 0.26, action in operational snapshot is `observe` not `review` — another cross-file disagreement
- FUTU: first-read spike, untrusted
- Nothing with an unambiguous enter signal

### Observations
1. **First clean round (no enter, no orphan)** — coincides with no orphan-enter bug firing. Correlation: when no orphan signal exists in cases, no false-enter is emitted. Supports the hypothesis that the orphan path is the entire source of enter false positives. Need more rounds to confirm.
2. **MARA reading being identical tick-over-tick** is suspicious. Round 4: fast5m 0.011/0.000, mid30m 0.010/0.016. Round 5: same exact numbers. Either (a) the lifecycle tracker hasn't updated (stale), or (b) the values truly haven't moved in 3 minutes (possible but unlikely). Worth investigating: does `SignalMomentumTracker` refresh every tick or only on state change? Stale values would also explain why my long-lived MARA case isn't picking up the price tick up to 10.71.
3. **"Clean round" rate needs tracking** — if ~1 in 5 rounds is clean (no enter, no orphan), that's telling about the noise floor. Session-level counter would help.
4. **Cross-file action disagreement is systemic**: HPQ (review vs observe), IBIT (review vs observe), BB from round 4 (review vs observe). The pattern: live_snapshot cases are more generous (review), operational_snapshot is more conservative (observe). Something in the operational pipeline is down-grading actions after case emission.

### Improvement ideas (new)
- [ ] **Lifecycle tracker refresh cadence** — verify `SignalMomentumTracker` updates on every tick for active cases, not just on phase transition. Identical vel/acc across rounds should be impossible on a live tick.
- [ ] **Topology/composite disagreement flag** — when a case is Growing but composite is negative (or vice versa), tag it visibly. Don't just surface the topology side.
- [ ] **Clean-round counter** — log `enter_count, orphan_count, vortex_count` per tick so we can measure the orphan-enter bug rate quantitatively over time.
- [ ] **Track case longevity** — MARA (dual-horizon) has now been in the tactical_cases across rounds 1, 2, 4, 5 (missed round 3). It's the *only* name with that kind of persistence. Exposing `case_age_ticks` in the row would make the "this is a real signal, not roster churn" stand out.
- [ ] **Investigate why BB at mark 3.94 composite 0.18 still gets `action=observe`** — that's a recently-collapsed meme stock; Eden should probably not surface it at all at this price level. Some kind of price-tier floor on case admission.

## Round 6 — tick 864 @ 16:10 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670 held. Mark 10.705 (+$17.50).

### Eden state delta
- **JD action=enter/orphan_signal returns** — "clean round" (round 5) ended exactly as expected. Round 5 was an intermission, not a structural change.
- **MARA mid30m dropped; fast5m retained** — my position lost half its topology support. Fast5m still Growing peer_conf=1, but the previously supporting sector_wave mid30m case isn't in this tick's roster.
- **Persistent orphan club unchanged**: MSTR, QUBT, JD, F, VNET, RIOT keep cycling through signal_translation_gaps across rounds 1-6. They rarely graduate to vortex cases (QUBT once in round 3 for a single tick). This is a recurring group that Eden's vortex detector cannot fit — probably because they're pre-market gap-ups whose intraday pressure structure doesn't match the pressure vortex template.
- **MSTR trade flow this round: buy 14% / sell 11%** (net buy), an improvement over round 1 (balanced 17/18). The orphan composite is catching genuine firming, but the action policy can't use it properly.

### BIG confirmed bug: lifecycle velocity/acceleration readings are stale
Cross-round comparison of identical vel/acc across rounds 4, 5, 6 on multiple symbols:
```
Symbol   Horizon  vel       acc
MARA     fast5m   0.0110    0.0002    (r4, r5, r6: identical)
IBIT     mid30m   0.0388    0.0401    (r5, r6: identical)
IBIT     fast5m   0.0286    0.0298    (r5, r6: identical)
GDS      mid30m   0.0225    0.0444    (r3, r5, r6: identical)
HPQ      mid30m   0.0785    0.0000    (r5, r6: identical)
RIVN     fast5m   0.0029    0.0519    (r3, r6: identical)
```
These can't all be genuinely unchanged across 3-6 minutes of live market. This is a cache/refresh bug in `SignalMomentumTracker`. Readings are being pinned at their first-observed value and copied forward until the case leaves the roster, then refreshed when it re-enters.

**Implication**: the "Growing/accelerating" narrative I've been relying on for case selection is partly theatre. The real-time intelligence I thought Eden was giving me is a first-tick snapshot frozen for the case's lifetime in the roster.

File lead: `src/us/temporal/lineage.rs` — specifically the `SignalMomentumTracker` feed from live snapshot path. Also check `src/us/runtime.rs` tick loop: is the tracker updated every tick or only on phase transition?

### Decision
**HOLD MARA.** 
- fast5m still supports, price slightly green, no trigger to exit.
- mid30m drop would normally be a "reduce by half" signal, but now I know the mid30m reading itself may be partly stale — demoting from roster when cache invalidates doesn't mean the signal flipped, it might just mean the cache rolled over.
- Net: no high-quality decision available. Discipline: don't act on low-quality information.

### Observations
1. **The "persistent orphan club" is a signal in itself.** MSTR, QUBT, JD, F, VNET, RIOT appearing every round without translation means Eden has an unresolved recognition gap for gap-up equities. Not a bug per se — it's a missing capability. Adding a "gap-up vortex" detector template (one that fires when open >> prev close with sustained volume) would let these names flow through the proper case path instead of leaking through orphan-enter.
2. **The stale-vel/acc bug partly explains the "case looks strong but nothing happens" pattern.** If vel=0.0110 is Round 4's first reading pinned for 3 rounds, then by Round 6 the *real* velocity might already be negative and Eden wouldn't know. Operators would enter on a stale green light.
3. **Price drift on MARA is tiny** (10.670 → 10.705 over 12 min) while Eden has been flipping the case's narrative repeatedly (growing, growing+accelerating, re-growing, fast5m only). That mismatch says: **Eden's "narrative volatility" is higher than the underlying price signal**. The operator would be better off if the case display were smoothed to price-reality timescales.

### Improvement ideas (new)
- [ ] **Fix lifecycle tracker refresh** — verify every active case's velocity/acceleration updates every tick, not just on case admission. Add an `as_of_tick` field on the lifecycle reading so staleness is visible.
- [ ] **Orphan-club detector** — identify symbols that persist in signal_translation_gaps across N consecutive rounds without ever becoming vortex cases. They're a distinct category (gap-up equities) that needs its own detection path.
- [ ] **Case narrative stability metric** — ratio of "narrative flips per tick" to "price moves per tick". If narrative > price volatility by 5×, Eden is churning case labels faster than the underlying.
- [ ] **Test: `cargo test` on `SignalMomentumTracker`** with a fixture where the tick count advances but values shouldn't change — verify refresh path is exercised.
- [ ] **Case stickiness penalty** — when a case displays the exact same vel/acc readings 2+ rounds running, mark it `STALE:check_tracker` to flag the bug at runtime.

## Round 7 — tick 984 @ 16:13 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670. Mark **10.666** (-$2) — gave back small gain.

### Eden state delta
- **F.US action=enter/orphan_signal** — 4th orphan-enter case in 6 rounds (F→MSTR→JD→JD→F). Enter cases now form a ~66% occurrence rate (4/6 rounds) on the orphan path. This is the dominant source of Eden's `enter` actions.
- **MARA composite dropped** from 0.418 → 0.344 in operational snapshot (round 5 → 7). Actively deteriorating.
- **MARA fast5m lifecycle unchanged**: 0.0110 / 0.0002 — **FOURTH consecutive round** with the exact same reading. Staleness beyond doubt.

### SMOKING GUN for the cache bug
**BB.US reappeared with vel=0.1149 acc=0.1270 — the EXACT values from its round-4 first appearance.**
Timeline:
- Round 4: BB first-read mid30m, vel 0.1149 acc 0.1270
- Round 5: BB dropped from tactical_cases roster
- Round 6: BB absent from cases
- Round 7: BB re-admitted, vel 0.1149 acc 0.1270 (identical to 9-12 minutes ago)

**The cache persists across case dropouts.** It's not just "pinned while active" (which would already be a bug) — the cache survives the case leaving and re-entering. The key must be something like `(symbol, horizon, phase)` not `(symbol, horizon, active_case_id)`.

This means: any "Growing" case you see may be displaying velocity/acceleration from a *previous episode* — potentially from many minutes ago — not current. The lifecycle view is not live at all.

### Decision
**HOLD MARA**, but this is the first round where I have an actionable bearish signal and would exit if MCP were up:
- Composite actively dropping (0.418 → 0.344)
- Price back below cost (10.666 vs 10.670)
- Lifecycle vel/acc frozen (useless as a signal)
- mid30m case absent for 2 rounds now
Fast5m case still listed but with stale data, so I can't trust it either.
**Net: bearish drift without any Eden-supported reason to keep holding.**

### Observations
1. The operational_snapshot composite IS recomputed live — it dropped from 0.418 to 0.344. So *some* of Eden's pipeline is fresh. The staleness bug is specifically in the lifecycle tracker path, not the composite path.
2. **I now have two different time-varying signals to compare for the same position**:
   - composite (fresh, falling) → bearish
   - lifecycle (stale, frozen) → bullish
   Operator should weight by freshness. Currently Eden's UX gives these equal billing.
3. AXON.US new first-read at vel 0.4519 acc 0.4509 — the highest mid30m first-read I've seen. Per established pattern: will likely decay or disappear next round. NOT actionable.
4. **Running tally of Eden `action=enter` emissions in this session**:
   - Round 1: 0 enter
   - Round 2: 1 orphan (F.US)
   - Round 3: 1 orphan (MSTR.US)
   - Round 4: 1 orphan (JD.US)
   - Round 5: 0 enter
   - Round 6: 1 orphan (JD.US)
   - Round 7: 1 orphan (F.US)
   **4 of 7 rounds had an `enter` emission; 4 of 4 enters were via orphan path; 0 of 4 enters were via vortex path.**
   **Vortex → enter: 0/7 rounds.**
   The operator-facing `enter` action in Eden US right now is 100% orphan-sourced.

### Improvement ideas (new)
- [ ] **Audit the cache key for lifecycle state** — must include tick number or case generation ID, not just (symbol, horizon). Current behavior looks like `get_or_insert` that never updates.
- [ ] **Fresh-vs-stale badge on every displayed signal** — each case row should show `as_of_tick` for each metric. Operator can then spot that vel/acc are 12 ticks old while composite is current.
- [ ] **Vortex-to-enter path is dead** — confirmed over 7 rounds. The entire "high-conviction topology" flow emits only `review` / `observe`, never `enter`. Either the enter threshold is tuned too high for vortex cases, or the vortex confidence never reaches it. Probably worth auditing `src/us/pipeline/reasoning/policy.rs` enter threshold for the vortex path specifically.
- [ ] **Reduce peer_conf weight in vortex confidence**: peer_conf=1.0 appears on most cases (crypto clusters, airline clusters) but doesn't translate into higher action tier. Why compute it if it doesn't drive policy?

## Round 8 — tick 1105 @ 16:16 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark 10.665, -$2.50.
- MARA composite rebounded 0.344 → 0.382 (still below round-5 0.418 but stopped bleeding).

### Eden state delta
- **RIOT.US action=enter/orphan_signal** — 5th orphan-enter in session (F, MSTR, JD×2, F, RIOT). Now 5 of 7 `enter`-emitting rounds, 0 vortex-path enters.
- **Cached vel/acc still frozen on many symbols**:
  - MARA fast5m 0.0110/0.0002 — **5 consecutive rounds**
  - GDS mid30m 0.0225/0.0444 — **5 rounds** (round 3, 5, 6, 7, 8)
  - BB mid30m 0.1149/0.1270 — **3 rounds** (round 4, 7, 8)
  - IBIT fast5m 0.0286/0.0298 — 4 rounds
  - LEGN fast5m 0.0065/0.0065 — 3 rounds
- **But some symbols DO refresh**:
  - FUTU mid30m: r5 0.0955/0.0935 → r7 0.0115/0.0169 → r8 0.3314/0.2705 (three distinct readings across rounds)
  - MSTR mid30m: r3 0.004/0.012 → r8 0.018/0.043
  - BABA new this round, fresh read
- **Bug is partial**: only some code paths refresh the lifecycle tracker. Cases generated from sector_wave + multi-peer appear to refresh more reliably; microstructure-only cases (MARA, LEGN, IBIT fast5m) are where the cache sticks.

### Cache bug refined hypothesis
It's not a global "never updates" bug — some paths work. The pattern I see is:
- **Refreshing**: mid30m sector_wave cases with peer_conf < 1 (FUTU 0.89, MSTR 0.91, BABA)
- **Frozen**: fast5m microstructure cases and mid30m sector_wave with peer_conf = 1.0

Speculation: there might be two lifecycle code paths, one triggered when peer confirmation recalculates (forcing a refresh) and one that short-circuits on "unchanged structure." If peer_conf drops below 1.0 in a round, the case is recomputed; if it stays at 1.0 for N rounds, the cache never invalidates.

This is a meaningful narrowing for whoever fixes it: check the code path where lifecycle state is updated on peer_conf change. It's probably not wired to fire on non-peer-conf changes.

### Decision
**HOLD MARA.** No actionable edge even if MCP were up:
- Composite recovering but not strong
- Price flat
- No clean signal to exit or add
- Fast5m cache is frozen so its support is fake
- Better to stay put than trade on noise

### Observations
1. **Session-level tally (8 rounds)**:
   - Entries I opened: 1 (MARA r1)
   - Eden enter emissions: 5 (all orphan path)
   - Vortex enter emissions: 0
   - Orphan-to-vortex graduation: 1 observed (QUBT in r3 for 1 tick)
   - Stale-cache cases: at least 5 (MARA, GDS, BB, IBIT, LEGN)
   - Clean rounds (no enter, no orphan): 1 of 8 (round 5)
2. **MARA, entered on round-1 signals (mid30m vel×4 acc×4 peak), has lived in the roster mostly with cached round-1 data.** The decision was made on what looked like live topology but a lot of it was a frozen first-read. The fact that I'm near flat after 8 rounds is lucky, not skillful. I'd need far more history to know if Eden's "Growing+peer_conf=1" truly has edge, or if the first-read pattern I saw was just noise captured in amber.
3. **Composite is my most trustworthy Eden signal so far** — it actually updates tick over tick and reflects convergence across dimensions. If I had to pick one Eden output to trade from and throw the rest away, I'd pick `operational_snapshot.symbols[*].state.signal.composite`.
4. **The vel/acc noise floor on FUTU (0.01 → 0.09 → 0.33 across rounds) is huge** compared to the price move (161.91 → 161.56, -0.2%). Topology reads order-of-magnitude swings while price barely moves. Topology is not (yet) a tradable proxy for price direction.

### Improvement ideas (new)
- [ ] **Refined bug scope**: lifecycle tracker refresh only fires when peer_conf changes. Audit the code path in `SignalMomentumTracker.update()` — look for an early-return when inputs haven't changed, which suppresses vel/acc recomputation.
- [ ] **Measure topology-price coupling**: per case, log `|Δvel|` vs `|Δprice%|` over the case's life. If coupling < noise floor, topology readings are not useful — they're just a different random walk.
- [ ] **Flag the "peer_conf=1.0 stuck" cases** in display — they're overrepresented in stale-cache cases.
- [ ] **Treat composite as primary signal, vel/acc as secondary** in docs until the refresh bug is fixed. Otherwise operators will keep making entry decisions on frozen data.
- [ ] **Session stats panel** — a per-session log showing entries opened, enters emitted by path, clean round count. Would make all this quantitative instead of my manual tally.

## Round 9 — tick 1224 @ 16:19 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.640**, -$15 paper (-0.28%).
- Slow bleed since round 4 peak: 10.710 → 10.705 → 10.666 → 10.665 → 10.640.

### Eden state delta
- **Second clean round (no enter action).** Two orphans present (MSTR, QUBT) but both at `review`, not `enter`. So orphan presence ≠ orphan-enter — the enter tier has a separate trigger (composite level? gap accumulation?).
- **MARA dual-horizon restored**: fast5m micro (stale cache, 5+ rounds same value) + mid30m sector_wave (**new reading** 0.0020/0.0014, never seen before). So for MARA, mid30m re-entry this round triggered a recompute.
- **But BB mid30m has been 0.1149/0.1270 for 3 re-entries** — so re-entry alone doesn't guarantee refresh. My peer_conf=1 hypothesis is also wrong (MARA mid30m peer_conf=1 *does* refresh; BB peer_conf=1 does *not*).
- Cache bug is *partial* and I can't cleanly characterize it from 9 rounds of observation. It's definitely a bug, but the trigger for refresh is non-obvious from outside.

### MARA composite timeline
```
Round 5: 0.418
Round 7: 0.344  (-0.074, bleed starts)
Round 8: 0.382  (+0.038, rebound)
Round 9: 0.400  (+0.018, stabilizing)
```
Composite stabilized in 0.38-0.42 band. This tells me the cross-dimensional picture for MARA is neither deteriorating further nor strengthening. It's just… present.

### Decision
**HOLD MARA.** 
- Price slow-bleeding (-0.28% over 13 minutes) but well within noise
- Composite range-bound 0.38-0.42
- Topology: dual-horizon restored but with weak values
- No clear edge to trim or add
- I wouldn't exit here even if MCP were up — the bleed is flat enough that transaction cost would dominate P&L change. Wait for a directional break.

### Observations
1. **"Clean rounds" are not rare** — 2 of 9 rounds (r5, r9). ~22%. Earlier I thought round 5 was exceptional; it isn't. The orphan-enter bug fires ~60% of rounds, not every round. The gate is likely composite threshold + gap size.
2. **MARA mid30m refresh this round** disproves my round-8 hypothesis (peer_conf=1 → always stale). The cache rule is more subtle. New hypothesis: **cache is keyed per case_id / setup_id, and MARA's mid30m setup_id rolled over because it was missing long enough** (rounds 6, 7, 8) for its accumulator to reset. Shorter gaps (BB dropped r5, missing r6, back r7 after 6 min) might not be enough to trigger reset.
3. **Slow bleed on a Growing case is the Eden failure mode I most need to watch**. If topology keeps saying "Growing" while price bleeds 0.3% over 15 minutes, Eden's topology is uncorrelated with short-term direction on this timescale. That's the core question. 9 rounds / 27 minutes is not enough data to conclude.
4. **Session is 27 minutes in; MARA paper P&L has moved in a -$15 to +$20 band.** Smaller than I'd expected from a 500-share position on a 3% mover. Low vol tape this session.

### Improvement ideas (new)
- [ ] **Case stability index** — track how long each case has been continuously in the roster (case_age_ticks). MARA at r1,r2,r4,r5,r6,r7,r8,r9 (miss r3) would be stability 0.89. BB at r4,r7,r8 (miss r5,r6,r9) would be 0.5. Operators should see this.
- [ ] **Topology-price correlation score per case** — over the case's life, what's the correlation between vel (topology) and realized price return? If consistently low, topology isn't predictive and should be deemphasized.
- [ ] **Composite-as-primary UI mode** — until the lifecycle cache bug is fixed, a toggle to hide vel/acc from case rows and use composite alone as ranking.
- [ ] **Session-level P&L tracking in Eden** — right now I'm computing mine by hand. Eden should track operator positions and show per-position P&L + which Eden case still supports it.

## Round 10 — tick 1343 @ 16:22 UTC

### Operator state
- MCP still disconnected (confirmed via ToolSearch: no Longport tools available).
- MARA 500 @ 10.670, mark **10.625**, -$22.50 paper (-0.42%).

### Eden state delta — **MARA dropped from tactical_cases entirely**
This is the first round since my entry (round 1) where MARA is not in the roster at all. Both fast5m and mid30m entries gone.
- Round 10 roster: LEGN, GDS, GME, BABA, IONQ, BB, FUTU×2, QFIN, AI — all sector_wave Growing review, **no MARA anywhere**.
- Operational snapshot still lists MARA action=`review`, composite=0.349 (vs 0.400 round 9 → dropped again, rebound over).
- **Textbook Eden-exit condition** under my own rule ("when case drops from roster + composite fades, Eden no longer supports position"). If MCP were up, I'd exit 500 MARA @ market here.

### Third clean round (r5, r9, r10)
No enter, no orphan in roster. So 3 of 10 rounds clean = 30% baseline clean rate. Previously I said 22% — updated number.

### BB cache finally refreshed
BB mid30m vel/acc has been frozen at 0.1149/0.1270 across rounds 4, 7, 8, 9. Round 10: **0.0342/0.0211** (different). So 18+ minutes after first read, BB's cache did update. My "cached forever" characterization was wrong — the cache just has a long TTL (or the refresh trigger is uncommon but not impossible).
- Updated cache hypothesis: some cases refresh on case reinstatement, some on a longer timer (~15+ min), some on phase transition. There's no single rule.
- GDS is now 6 rounds identical at 0.0225/0.0444 — the persistence champion.
- LEGN is now 5 rounds identical at 0.0065/0.0065.
- The staleness is real and bad, but not uniform — the code path has a fragmented refresh behavior.

### Decision (hypothetical, MCP down)
**EXIT MARA 500 @ market** if trading were possible:
1. Tactical_cases dropped MARA — Eden's primary operator-facing surface no longer supports the thesis
2. Composite turned back down (0.400 → 0.349), -0.051 after brief 2-round rebound
3. Price -$22.50 on a 15-minute bleed
4. No vortex support on either horizon this tick
5. Pain threshold on a paper position is non-issue, but the Eden signal flip is the real trigger

Since I can't trade, formally: position held due to infrastructure constraint, not thesis. Note the gap between "what Eden says" and "what I could execute." This is a real operator problem — if Eden's UX were tied to a broker action queue, this decision should auto-queue.

### Observations
1. **Eden's "exit" signal is implicit, not explicit.** I had to manually diff the tactical_cases roster across rounds to discover MARA was gone. There is no `position_exit_triggered` or `eden_no_longer_supports` field anywhere in the snapshot. This is the single biggest UX gap I've found: **holding a position means continually re-diffing Eden's roster against your position book.**
2. The operational snapshot has a `recent_transitions` field I haven't explored — might contain the delta info I need. Flag for next round.
3. **I would have made a better session holding nothing.** My MARA entry was defensible at round 1 based on round-1 info. But the session P&L band (-$22 to +$20) is within noise for a 500-share position on a 3% daily mover. If I were trading for real, transaction costs would have already eaten any edge.
4. **Session enter emission tally (10 rounds)**: vortex = 0, orphan = 5. Completely unchanged pattern.

### Improvement ideas (new)
- [ ] **`eden_exit_triggered` field** — when a case that previously supported an operator position leaves the roster OR demotes, emit a clear exit signal with the reason. Don't make the operator diff rosters.
- [ ] **Case freshness vs age in display** — show `age_ticks` (how long case has persisted) and `as_of_tick` (when the metric was last refreshed). GDS currently has 6 rounds age but 1 round freshness. That's important.
- [ ] **Position-to-case binding** — Eden should know I'm long MARA from round 1. Once bound, it should track which case(s) support the position and fire explicit events on support changes, not just reshuffle the roster.
- [ ] **Explore `recent_transitions` in operational_snapshot** — might already be this delta info. If so, live_snapshot should mirror it or link to it.

## Round 11 — tick 1459 @ 16:25 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.615**, -$27.50 paper (-0.51%).
- Steady bleed continues. MARA low of session so far.

### Eden state delta
- **MARA absent from tactical_cases second round in a row.** No fast5m, no mid30m.
- Operational snapshot still lists MARA action=review, composite **0.337** (down from 0.349 → 0.337).
- **Fourth clean round (r5, r9, r10, r11).** Zero enter actions, zero orphan cases in tactical_cases roster. Four in a row (r9, r10, r11 consecutive). New running rate: **4/11 = 36% clean.**
- New roster: RIVN, GDS, BABA, CLOV×2, FUTU×2, INTC, RKLB, CPAY. Continued sector rotation.
- **CLOV dual-horizon first-read**: vel 0.0948/acc 0.1186 on mid30m, vel 0.0395/acc 0.0402 on fast5m. Classic round-2-MARA first-read pattern; expect decay next round.

### Discovered: `recent_transitions` IS the missing delta feed
```
operational_snapshot.recent_transitions[] = {
  from_tick, to_tick, symbol, setup_id,
  from_state, to_state, confidence,
  summary: "... entered the active structure set" / state change
}
```
64 entries in the current file. Example:
```
MSTR.US pf:MSTR.US:long:mid30m  -> review  ("entered the active structure set")
AMAT.US pf:AMAT.US:long:mid30m  -> observe
AFRM.US pf:AFRM.US:long:mid30m  -> review
MARA.US pf:MARA.US:long:mid30m  observe -> review  (at tick 1451/1452)
```
This is exactly the missing operator UX piece. **It's already in Eden's data model, just not surfaced in `live_snapshot.json` or the tactical_cases summary.** Fix is probably a one-line addition to the live snapshot serializer to include `recent_transitions` (or filtered view of it).

Note the summary wording "entered the active structure set" — this is an admission event, but there isn't a visible "LEFT the active structure set" event for when MARA dropped out of round-10/11 cases. Either:
- The transitions feed only logs admissions, not removals
- Or removals exist but are filtered out of the summary lookup I ran

Either way, **a symmetric admit/remove event stream is what the operator needs**.

### Decision (hypothetical, MCP down)
**EXIT MARA 500 @ market** — same as round 10, signal strengthened:
- Dropped from tactical_cases for 2 consecutive rounds
- Composite monotone down r9→r10→r11 (0.400→0.349→0.337)
- Price at session low
- No new support, no bullish divergence
- The thesis from round 1 (Growing, dual-horizon, peer_conf=1) is completely gone

If this were real money I would have lost -$27.50 in 27 minutes while holding a position Eden stopped supporting ~6 minutes ago. **The infrastructure gap (MCP down) is costing me ~$27 here, not Eden's signal quality.** Eden actually had the right call — I just couldn't act on it.

### Observations
1. **Eden's exit signal was available, but buried.** Had I been diffing the roster automatically, I'd have known at round 10 (tick ~1343) that MARA support dropped. The round-9 composite turnback (0.400→not further up) was also an early warning I underweighted. Eden *did* provide exit information — in both `tactical_cases` (by omission) and `recent_transitions` (by state change events).
2. **Case composite is the most reliable single signal**, confirmed again. MARA composite peaked r5 (0.418), started declining r7, stayed in 0.34-0.40 band r7-r9, broke down r10-r11. That's a clear trajectory. If I were trading based only on composite, I'd have reduced at 0.40→0.34 break and exited at 0.34→reentry.
3. **Sector rotation between rounds**: Crypto (r2-3) → Crypto+quantum+China (r3-4) → Airlines/autos (r4) → Crypto+financials (r5-6) → Airlines (r7) → Mixed (r8) → Fintech/China (r9) → Meme+China (r10) → Fintech+semis+meme (r11). Eden doesn't hold a sector thesis for more than 1-2 rounds. Unclear if this is real rotation or noise in sector_coherence scoring.
4. **Zero orphan-enter for 3 consecutive rounds (r9, r10, r11)**. Possible causes: (a) the gap-up orphan symbols (F, JD, MSTR, QUBT, RIOT) have calmed; (b) the session-long convergence composite for those names has declined enough that the orphan-enter threshold is no longer crossed. Would be informative to see if the 6 orphan-club symbols (MSTR/QUBT/JD/F/VNET/RIOT) have faded on composite this round too.

### Improvement ideas (new)
- [ ] **Surface `recent_transitions` in `live_snapshot.json`** — one-line fix, biggest UX win of the session. Include at minimum: last N transitions, last N admissions, last N removals. Don't make operators diff rosters.
- [ ] **Emit "case left active set" events**, not just "entered." Symmetric admit/remove makes position-to-case binding trivial.
- [ ] **Position-binding layer**: given an operator position book, auto-tag every relevant transition and compute a "support trajectory" per position. `MARA.US: admit@r1 → support peak@r5 → composite peak@r5 → composite break@r7 → support dropout@r10`.
- [ ] **Orphan-enter threshold investigation**: 3 consecutive clean rounds after 5 enters in 7 rounds — what changed? Session composite distribution for orphan club? Should be logged.

## Round 12 — tick 1575 @ 16:28 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.620**, -$25 paper.
- Price bleeding has flattened (r10 10.625, r11 10.615, r12 10.620) — consolidating at session lows, not accelerating down.

### Eden state delta
- **Fifth clean round** (r5, r9, r10, r11, r12). **Four consecutive clean** (r9-r12). 5/12 total = 42% clean rate. The orphan-enter bug has been silent for 4 rounds straight — a structural change that correlates with pre-market gap-up momentum fading into midday.
- **F.US graduated from orphan to vortex case**: mid30m sector_wave vel 0.3993 acc 0.4037 — highest first-read of the session. Driver_class is now `sector_wave`, not `orphan_signal`. This is the QUBT-r3 pattern again: persistent orphans eventually translate, typically when the session calms and the vortex detector catches up.
- MARA still absent (3rd consecutive round out of tactical_cases).
- **MARA composite continues monotone decline**: r5 0.418 → r7 0.344 → r8 0.382 → r9 0.400 → r10 0.349 → r11 0.337 → **r12 0.320**. Four consecutive down ticks after the r8-r9 dead-cat bounce. Composite trajectory is the single most coherent story of the session.

### Observations
1. **The orphan-enter bug is session-phase dependent.** Rounds 2-8 (early session, gap-up energy): orphan-enter fires every round. Rounds 9-12 (mid-session consolidation): zero orphan-enter. Hypothesis: orphan composites require `pre_post_market_anomaly=1` plus `price_momentum=1` — both of which max out right after open and decay as the session progresses. So the bug is correlated with session timing, which partially hides it. In afternoon sessions (e.g. 3pm-4pm ET) it probably re-fires as closing momentum picks up.
2. **Persistent orphans CAN graduate to vortex cases**, but the translation latency is ~20-40 minutes based on QUBT (r3, ~9 min) and F (r12, ~30 min). That's too slow to be actionable for a session trader but useful as a late confirmation or EOD learning signal.
3. **"Topology/price divergence" is the most reliable anti-pattern of the session.** F.US round-12 reading vel/acc both 0.40 while price moved 0.06% over 28 minutes. CLOV r11 first-read 0.09/0.12 while price barely moved. The mid30m first-reads ≥ 0.1 almost never couple to real price action. Probably the topology computation is amplifying per-tick micro-structure noise at the moment a new case enters its accumulator.
4. **MARA composite trajectory as a session summary**: this one number told the entire story of my position — peak at r5, break at r7, fail rebound r8-r9, accelerating decline r10-r12. If I had a `composite_trajectory_smoothed` plot per active position, I'd never need any of the roster-diffing work I've been doing manually.

### Decision
**HOLD MARA** (MCP still down). Round-10 and round-11 exit triggers still in effect but mark has stabilized; if MCP reopens NOW I'd exit immediately at 10.62 (-$25). If MCP reopens in 5 more minutes and mark is still flat, still exit — the signal hasn't recovered, the absence of decay is the most I can hope for.

### Improvement ideas (new)
- [ ] **Session-phase scorecard**: break scorecard.hit_rate by session phase (opening 30min / mid / closing 30min). Would expose that orphan-enter is concentrated in opening phase and support a phase-gated policy.
- [ ] **Composite trajectory as primary position tracker**: for every operator position, emit a live time series of `composite` per tick. This single plot replaces most of the operator's diagnostic work.
- [ ] **Orphan-to-vortex graduation latency**: measure time between `orphan first appearance` → `vortex case emitted` per symbol. If the median is 20-40 min (as I've seen), that's a specific perf target for the pressure→case pipeline.
- [ ] **Mid30m first-read damping**: first-tick vel/acc on mid30m cases are consistently extreme (0.1-0.4) and consistently fade or decouple from price. Either suppress first-tick values or display them with an explicit "untrusted first-read" flag until 2-3 ticks confirm.

## Round 13 — tick 1688 @ 16:31 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.625**, -$22.50.
- Price +$0.005 vs last round — flat drift, no direction.

### Eden state delta
- **Orphan-enter returns**: MSTR.US action=enter, driver=orphan_signal. The 4-round clean streak (r9-r12) ends. Total session: 6 orphan-enter rounds out of 13 = 46%.
- **MARA REAPPEARS in tactical_cases** after 3 rounds absent (r10, r11, r12). Fast5m microstructure, vel 0.0110 / acc 0.0002 — **the exact cached round-1 value**. This is now the 6th+ occurrence of MARA fast5m showing that specific (0.0110, 0.0002) pair, and it's survived MARA leaving the roster and coming back. **Cache keyed on setup_id, persists across case lifetimes** — confirmed beyond doubt.
- MARA composite: r12 0.320 → r13 0.331 (+0.011, within noise).
- New roster entries: UNG (natural gas), WMB (pipeline) — commodity/energy rotation beginning? WMB first-read vel 0.1738 (classic extreme-first-read).

### Hypothesis update on orphan-enter timing
Round 12 I said "orphan-enter is session-phase dependent; quiet in midday." Round 13 contradicts that at 16:31 UTC (11:31 AM ET) — still midday. The 4-round pause was real but the bug didn't stay paused. Revised view: **orphan-enter is episodic, roughly correlated with session momentum but not strictly phase-gated**. It fires when the 6 orphan-club composites cross a threshold simultaneously, which happens in bursts as market volume changes.

Timeline of orphan-enter episodes:
- Burst A: r2, r3, r4 (F, MSTR, JD)
- Pause r5
- Burst B: r6, r7, r8 (JD, F, RIOT)
- Pause r9, r10, r11, r12
- Burst C: r13 (MSTR) — new burst starting?

### Decision
**HOLD MARA.** 
- MARA re-entering tactical_cases is a weak positive, but we know the vel/acc is stale so it's fake support.
- Composite bounced trivially (0.320→0.331), not enough to signal a recovery.
- Still more net-exit evidence than hold evidence. The exit signal from r10 still stands; the re-entry is cache noise, not real new information.
- Would still exit if MCP were up, same as r10/r11/r12.

### Observations
1. **Round 13 is the cleanest demonstration of the stale-cache bug**: MARA left the roster for 3 rounds and re-entered with the *exact* first-read values from round 1. The cache is genuinely per-setup_id and never invalidates for some code paths. For whoever fixes it: add `case_dropout_invalidates_cache = true` or equivalent.
2. **My exit decision on MARA would have held up for 4 consecutive rounds now (r10, r11, r12, r13).** If the MCP were intermittently up, exiting at r10's 10.625 would have been right. Bleeding past that point was trivial (~$5 swing) but the signal was unambiguous at r10.
3. **MARA's "re-entry" to the roster doesn't reopen the thesis.** An operator shouldn't interpret "Eden put this case back on the list" as "buy again" when the underlying numbers are identical to a stale cache. Eden needs to differentiate "this case is continuously supported" from "this case just re-appeared using stored values."
4. The 6-symbol orphan club (F, JD, MSTR, QUBT, VNET, RIOT) has accounted for every single orphan-enter in the session. Nothing outside that set has ever orphan-entered. These 6 names would be enough to build a targeted patch: a deny-list on the orphan-enter policy path for "chronically orphaning" symbols.

### Improvement ideas (new)
- [ ] **Cache invalidation on case dropout**: when a setup_id leaves the active tactical_cases set, invalidate its lifecycle tracker entry. Re-entry triggers recompute.
- [ ] **Case re-entry flag**: mark re-entered cases with `is_re_entry: true` so operators know this isn't continuous support. Pair with `ticks_since_last_seen`.
- [ ] **Orphan-enter deny list for chronic orphans**: F.US, JD.US, MSTR.US, QUBT.US, VNET.US, RIOT.US have never had a vortex-path enter in 13 rounds; they've only ever orphan-entered (and they eventually graduate to vortex at *review*, never enter). Require vortex-path enter or block.
- [ ] **Composite bounce detection**: 0.320→0.331 is within daily composite noise. Eden should compute per-case composite volatility and only signal "trend change" when the bounce exceeds 1σ. Operators (me) have been visually pattern-matching; this should be automated.

## Round 14 — tick 1800 @ 16:34 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.570** — new session low. -$50 paper (-0.94%).
- Infrastructure gap cost is now clearly growing. From round 10 exit signal (10.625) to now (10.570): **-$27.50 of preventable loss** accumulated while unable to execute Eden's correct call.

### Eden state delta
- **JD orphan-enter**. Burst C continues: r13 MSTR, r14 JD. Two consecutive orphan-enter rounds.
- **MARA absent from tactical_cases AGAIN** (r14 out). Session pattern: r1-9 in → r10-12 out → r13 brief return on stale cache → r14 out again. The roster toggling is noise, not signal.
- **MARA composite new session low**: 0.3048 (prior low was 0.320 r12). Clean monotonic descent since rebound failure at r9.
- CLOV mid30m first-read 0.3777 / 0.3970 — another extreme (round 11 was 0.0948/0.1186, so this is a re-entry with fresh read, not cached).
- LEGN switched horizons: was r6-r10 fast5m (0.0065/0.0065), now r14 mid30m (0.0112/0.0095). Same symbol different horizon = different setup_id = different cache entry. OK.

### MARA exit signal — now at maximum strength
Multiple independent confirmations:
1. **Composite: monotone decline**, new session low at 0.3048. Down -28% from r5 peak (0.418).
2. **Tactical_cases roster**: absent 4 of last 5 rounds (r10, r11, r12, r14 out; only r13 in briefly on stale cache).
3. **Price**: new session low, -$50 paper, -0.94%.
4. **No bullish divergence anywhere**: peer_conf, raw_disagreement, lifecycle (stale anyway), composite — every dimension either silent or bearish.

**Verdict: if I could trade, exit at market immediately.** This is the exit signal every Eden operator should be able to see. It's been there for 4-5 rounds and I've been stuck with it.

### Observations
1. **The session has now produced a full cycle on my one position**: entry edge (r1-r5: composite rising, dual-horizon vortex, Growing acceleration), peak (r5-r6: ceiling at 0.418), deterioration (r7-r9: composite breaks, mid30m drops), exit signal (r10: case roster drops MARA), confirmation (r11-r14: composite monotone down to new lows). This is an 11-round life-cycle, from entry to confirmed-exit. Session is 42 minutes in.
2. **The operator work is heavily diagnostic, not decisive.** 90%+ of my time each round is spent figuring out "is this metric stale, is this case real, is the composite moving." The actual trade decisions are fast. Eden should own the diagnostic work — stale flagging, trajectory smoothing, delta events, position binding — and present the operator with clean decisions.
3. **Eden's single most valuable output is MARA composite trajectory**: 0.418 → 0.344 → 0.382 → 0.400 → 0.349 → 0.337 → 0.320 → 0.331 → 0.305. Every important decision point is visible in this sequence. If the rest of Eden's features vanished and only composite remained, the session would read nearly the same.
4. **Infrastructure resilience is an Eden problem too.** Even with a perfect signal, I lost -$50 because the trade layer (Longport MCP) went down. An operator OS should either (a) queue exit orders with the broker the moment Eden emits the signal, or (b) at minimum flag "your position is unexecutable" so the operator can take manual action via another channel before the cost grows.

### Improvement ideas (new)
- [ ] **Bind Eden positions to broker reachability state**: show "broker_ok | broker_degraded | broker_down" per position. When broker goes down and a position has a live exit signal, escalate to loud alert.
- [ ] **Pre-queue orders**: when Eden signal says exit and operator opts in, queue a market or stop order with the broker in advance. Persistent orders survive MCP disconnects.
- [ ] **Composite-only operator dashboard**: a minimal view showing composite trajectory per active position + last 3 composite values from Eden. Hide all case churn, vel/acc, tactical_cases noise. Would be the single most useful thing to build right now based on this session.
- [ ] **"Your MCP is down, but Eden still says X"**: a structured unavailability log so a later review can show exactly what the operator could have done vs what actually happened. This session would have a clean record: "exit signal first at r10 tick 1343; could not execute; session loss at exit-time -$22.50, drift to -$50 by r14."

## Round 15 — tick 1882 @ 16:37 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.605** (+$0.035 from session low r14 10.570). -$32.50 paper.

### Eden state delta — major reversal in progress
- **F.US orphan-enter**: Burst C continues, 3 consecutive (r13 MSTR, r14 JD, r15 F).
- **MARA absent from tactical_cases** 5th of last 6 rounds.
- **MARA composite SURGED 0.305 → 0.407 (+0.102, +33%)** — biggest single-round move of the session. First time a declining composite has meaningfully reversed.
- **Crypto cluster composite all strong this round**: MSTR 0.549, CLSK 0.501, RIOT 0.485, COIN 0.424, MARA 0.407. All four are above 0.4. This looks like a genuine crypto basket revival.
- **But prices are flat-to-down**: MSTR 139.76 (-$0.83 from r10), CLSK 11.38 (-$0.07), RIOT 18.48 (-$0.06), MARA 10.605 (-$0.065 vs session peak). Composite moved 33% on MARA while price moved 0.33% — composite 100× more sensitive than price in this round.
- RIOT and MSTR graduated to vortex cases this round (sector_wave mid30m review). Not orphan path.
- F.US is now *simultaneously* an orphan-enter case (fast5m) AND has been a mid30m vortex case recently. The two lanes aren't mutually exclusive.

### Signal ambiguity — first of the session
My round-10 exit thesis was "composite trending down + MARA out of roster = exit." Round 15 breaks the composite half of that:
- Roster: still bearish (MARA out)
- Composite: sharply bullish reversal
- Crypto cluster: all strong composites, MSTR/RIOT re-entered vortex cases

This is the first round of the session where the exit signal is **no longer unambiguous**. If MCP were up:
- Round 10: exit — clear
- Round 11-14: exit — clear
- Round 15: **wait one more round** — composite reversal needs confirmation

HOLD is now the right call on the evidence, not just the forced call due to MCP being down.

### Composite volatility observation
```
MARA composite round-over-round diffs:
r5→r7:  -0.074
r7→r8:  +0.038
r8→r9:  +0.018
r9→r10: -0.051
r10→r11: -0.012
r11→r12: -0.017
r12→r13: +0.011
r13→r14: -0.026
r14→r15: +0.102  <- 4x larger than any other move
```
The r14→r15 move is anomalously large. Either:
1. **Genuine cluster revival** — crypto basket real-time reprice
2. **Composite computation noise burst** — a single tick with unusual dimension readings
3. **Cache/data effect** — similar to the vel/acc staleness bug, possibly affecting composite too when cluster regime changes

Given the entire crypto cluster moved together (MSTR, CLSK, RIOT, MARA, COIN all >0.4 composite), hypothesis #1 seems most likely. Clusters can genuinely revive midday.

### Decision
**HOLD MARA.** For the first round in 6 rounds (r10-r15), my decision is based on new evidence, not MCP constraint:
- Composite reversal is a real counter-signal
- Cluster-wide strength (not just MARA) reduces idiosyncratic risk concern
- Roster absence is a lagging indicator; composite leads
- If MCP were up, I'd still hold here — let round 16 confirm or reject

### Observations
1. **Composite can move 100× more than price in a tick.** This reframes my earlier "composite is the most reliable signal" claim. Composite is *informative* but not *tradable directly* — it needs to be paired with price confirmation to avoid whipsaws like this one. Round 10-14 I was building an exit conviction that round 15 would have whipsawed.
2. **Had I exited at round 10's signal**, I'd be unwinding a correct exit on a revival this round (at session -$27 cost). Exiting on the first-tick exit signal without confirmation is too fast. The "right" move would have been r11-r12 once the composite break held — but now even that looks wrong given r15's reversal.
3. **This session is becoming a test of my exit discipline rather than entry edge.** The entry on MARA was marginal but defensible. The real question is "when does Eden's exit signal become decisive enough to act?" 4 consecutive rounds of composite decline (r10-r14) looked decisive until round 15 erased half the decline. **Answer: Eden doesn't currently give you that decisiveness** — operator has to pair composite with price confirmation or wait for multiple independent bearish divergences.

### Improvement ideas (new)
- [ ] **Composite volatility envelope**: for every case, compute per-tick composite stddev. Display exit signals only when composite decline exceeds 2σ over N ticks. Prevents whipsaw exits.
- [ ] **Cluster composite average** (e.g. crypto basket avg): when an individual symbol composite declines but cluster average is strong, dampen the exit signal. Round 14→15 would have shown "MARA composite at 0.305 vs crypto cluster avg 0.48 → isolated weakness" (r14) then "cluster-wide bounce" (r15).
- [ ] **Decisiveness threshold**: "signal strength" ≠ "signal decisiveness". A steady decline over 4 rounds with no counter-moves is more decisive than a single-round spike. Emit a `signal_decisiveness_score` that factors in consistency.
- [ ] **Price-composite coupling regime**: detect rounds where composite moves 5-100× more than price (regime shift signal) vs rounds where they track (signal-following regime). Operators should trust composite more when coupling is tight.

## Round 16 — tick 1991 @ 16:40 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.605** (flat vs r15). -$32.50.

### Eden state delta — r15 composite spike whipsawed
- **MARA composite 0.407 → 0.360 (-0.047)** — gave back ~half the r15 spike in one round. Classic whipsaw, exactly the risk I flagged last round.
- **MARA is the weakest name in the crypto cluster this round**:
  - MSTR 0.562 (slight up from 0.549)
  - QUBT 0.537 (new case)
  - CLSK 0.500 (flat)
  - RIOT 0.484 (flat)
  - **MARA 0.360** (pulled back most)
- **Two orphan-enters simultaneously**: QUBT + RIOT. First double-orphan-enter of the session. Burst C intensifying: r13 MSTR, r14 JD, r15 F, r16 QUBT+RIOT = **5 orphan-enters in 4 rounds**.
- MSTR and RIOT moved from vortex case (r15) back to orphan (r16) — the graduation isn't sticky either. Case path is churning the orphan club.
- GLD.US new (gold ETF) with extreme first-read vel/acc — another sector (gold) joining.

### New observation — relative weakness within a strong cluster
MARA composite 0.360 vs crypto cluster avg ~0.49. MARA is lagging the cluster by 25%. This is a different failure mode than I've tracked:
- Previously: MARA composite falling in absolute terms, cluster ambiguous
- Now: MARA composite rebounded but lagging, cluster strong
**Relative weakness within a strong cluster is a worse signal than absolute weakness alone**, because it tells you the thesis ("crypto basket wave lifts MARA") is still alive but MARA specifically is being left behind.

Crypto cluster dispersion (max - min composite this round):
```
MSTR 0.562  (top)
QUBT 0.537
CLSK 0.500
RIOT 0.484
MARA 0.360  (bottom, -0.20 from top)
```
MARA at the bottom of a cluster it nominally belongs to. Not good.

### Decision (hypothetical, MCP down)
**EXIT MARA** — reinstated exit decision:
1. Whipsaw confirmed: r15 spike was noise
2. Relative weakness in a strong cluster is worse than absolute weakness
3. Still no tactical_cases support
4. Price flat (10.605) — no bid
5. Paper P&L -$32.50, MCP gap cost still accumulating

### Observations
1. **My round-15 "wait for confirmation" decision was correct.** Had I flipped to bullish on r15's spike (e.g. added to MARA), I'd now be holding more of the cluster laggard. The decisiveness score I proposed last round would have prevented that.
2. **5 orphan-enters in 4 rounds is the highest density of the session.** Burst C is now bigger than Burst A (r2-4: 3) or Burst B (r6-8: 3). Possible cause: midday momentum rebuild around crypto cluster specifically — all 5 orphan-enters in this burst were crypto names (MSTR, JD, F, QUBT, RIOT — OK JD and F aren't crypto, but the majority is).
3. **Cluster-relative composite is a better signal than absolute composite.** Would have been invaluable for deciding r10 vs r15 — absolute composite whipsawed but cluster-relative composite would have shown MARA's steady underperformance relative to peers.
4. **Orphan-enter is clustering around tradable themes, not random symbols.** If I hadn't had the MARA position, the right play this round would have been to look for the *best* name in the cluster (MSTR? QUBT?) and enter there, not chase the orphan-enters directly (which are just momentum chasers).

### Improvement ideas (new)
- [ ] **Cluster-relative composite score**: `symbol_composite / cluster_avg_composite`. A value <0.8 means the symbol is a cluster laggard. This is a high-signal exit trigger for cluster-membership theses.
- [ ] **Thesis-binding on entries**: when entering a position, explicitly tag the thesis ("MARA is a crypto cluster leader"). Eden can then track thesis validity: if the cluster is strong but the symbol is a laggard, the thesis is broken even if the cluster trade is still alive.
- [ ] **Burst detection for orphan-enter**: when orphan-enter fires 3+ times in 5 rounds, surface a "themed burst" notice. Operator can decide whether to trade the theme via a leader or avoid the laggards.
- [ ] **Multi-symbol orphan-enter per round handling**: this was the first double orphan-enter. The policy should probably rank them by strength and elevate only the strongest, or at minimum dedupe by theme.

## Round 17 — tick 2098 @ 16:43 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.640** (+$0.035 vs r16). -$15 paper — **best print since round 8**.

### Eden state delta
- **RIOT orphan-enter, 2nd consecutive round**. Burst C stretch: 6 orphan-enters across r13-r17 (MSTR, JD, F, QUBT+RIOT, RIOT).
- **MARA absent from tactical_cases** (now 6 of last 7 rounds out).
- **MARA composite: 0.360 → 0.409** — another spike up. Net r14→r17: 0.305 → 0.360 → 0.407 → 0.360 → 0.409. Oscillating in a 0.10-wide band.
- **MARA no longer cluster laggard**:
  ```
  MSTR 0.545 (top, was top r16)
  RIOT 0.449
  CLSK 0.446
  MARA 0.409 (mid, was bottom r16)
  IBIT 0.231 (bottom — IBIT tumbled)
  ```
  Cluster dispersion compressed (top-bottom was 0.20 r16, now 0.31 including IBIT or 0.14 excluding). MARA climbed from bottom to mid-pack in one round.
- **Price recovering**: 10.640 is the highest MARA print since r8 (10.666). r14 session low 10.570 is -$70 below. MARA has now recovered +$70 from session low.

### Whipsaw pattern on MARA composite
```
r14: 0.305  (low)
r15: 0.407  (+0.102 spike)
r16: 0.360  (-0.047 retracement)
r17: 0.409  (+0.049 second spike)
```
This is NOT trending noise — it's coherent oscillation. Composite is ringing around a ~0.38 level with ±0.05 amplitude. This matches the underlying price move (10.570 → 10.605 → 10.605 → 10.640) which is also a clean recovery leg.

**Reinterpretation**: my round-10 to round-14 "composite decline" might have been the first half of a longer wave, and r14-r17 is the rebound leg. The decline wasn't a "thesis break" as I called it; it was a **retracement within a range**. The whole r5-r17 sequence fits a "trade in a range 0.30-0.42" story better than a "trending exit" story.

### Decision
**HOLD MARA** — more confident now. New signal mix:
- Price: strongest level since r8, first clear recovery
- Composite: back near r15 high, oscillating above r14 low
- Cluster: no longer laggard
- Roster: still absent but roster is lagging other signals at this point

The exit signal from r10-r14 is now effectively cancelled by the r15-r17 recovery. **I would not exit now even if MCP were up.** The earlier exit logic was based on trending composite decline, which has not played out — it oscillated.

### Observations
1. **I was wrong about the exit signal.** Rounds 10-14 I said "exit if MCP were up" with increasing conviction. Rounds 15-17 invalidated that. Total swing: r14 low -$50 → r17 -$15. If I had exited at r10 (-$22) and now could re-enter at 10.64, that's a churn loss + missing the recovery.
2. **Eden's composite doesn't differentiate between trend exits and range retracements.** I'd proposed a decisiveness score last round; this round confirms it's critical. Without it, composite reads like a trend signal when it's actually mean-reverting.
3. **The only way to tell a trend exit from a range retracement in real-time is cluster coupling.** r14 MARA was isolated weak; r16 MARA was cluster laggard; r17 MARA was mid-cluster. When the cluster recovers, the name recovers with it (unless broken). MARA was not broken.
4. **Honest accounting**: I held through the lowest point with correct discipline (no panic exit at r14 low), but my *reasoning* for holding (r13 "Eden is wrong exit signal is too aggressive") was luck — at r14 I was *actively wanting to exit* and couldn't. The outcome happens to be favorable but the thought process was bearish. The lesson: when in doubt, wait for the cluster context, not just the symbol.
5. **Session running P&L path**: +$20 (r4 peak) → -$50 (r14 low) → -$15 (r17 recovery). This is a -$70 and then +$35 swing over 44 minutes on a 500-share position. Volatility of the position is ~$70, volatility of my read of Eden signals is much higher.

### Improvement ideas (new)
- [ ] **Range-vs-trend classification**: for each active case, classify composite history as "ranging around X" vs "trending up/down". Decisiveness score I proposed earlier is specifically the range amplitude vs trend slope.
- [ ] **Cluster context always-on**: show a per-case mini-plot of the cluster's composite vs the symbol's composite over the last N ticks. One glance tells operator if symbol is leading, lagging, or in-pack.
- [ ] **Operator outcome tracking**: Eden should record the decisions I said I would have made at each tick (even without MCP), compare to subsequent outcomes, and learn whether my exit-threshold calibration is too aggressive. A paper-decision log would make the feedback loop explicit.
- [ ] **Multi-spike detection**: when composite oscillates with 2+ local minima/maxima in a short window, flag as "ringing" rather than "trending." Current Eden has no concept of ringing.

## Round 18 — tick 2203 @ 16:46 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.620**. -$25.

### Eden state delta
- **Sixth clean round** (r5, r9, r10, r11, r12, r18). Zero enter actions, zero orphan cases. Burst C ended. Running rate: 6/18 = 33%.
- **MARA BACK in tactical_cases with FRESH readings**: fast5m microstructure vel **0.0925 / acc 0.1040** — completely different from the 15-round cached 0.0110 / 0.0002. **Cache refresh confirmed.** After persisting across 15 rounds and multiple case dropouts/re-entries, MARA's fast5m cache finally updated.
- **MARA composite: 0.409 → 0.365** — fourth ringing cycle: 0.305 → 0.407 → 0.360 → 0.409 → 0.365. Amplitude stable around 0.10 with midpoint ~0.38.
- **Crypto cluster pulled back together**: MSTR 0.552, RIOT 0.431, CLSK 0.421, MARA 0.365. MARA moves with the cluster this round — not laggard, not leader.
- CLOV.US mid30m vel **0.5854 / acc 0.6152** — session highest first-read. But CLOV is a $2.06 stock: for a $2 name, a 0.60 "topology acceleration" is against price movements smaller than the bid-ask spread. **Eden is surfacing micro-caps prominently when their topology math happens to blow up on low-price noise.** Material tradability is zero.
- DELL mid30m first-read 0.1571 — extreme-first-read pattern continues.

### Cache refresh finally happened — what triggered it?
MARA fast5m vel/acc was pinned at 0.0110/0.0002 from round 1 through round 17 (at minimum rounds 4, 5, 6, 7, 8, 9, 13 all showed exactly those values). Round 18 shows 0.0925/0.1040.

Possible triggers (from outside the code):
- **Phase transition**: if MARA's phase rolled from Growing → New → Growing, that might force a recompute
- **Long-absence TTL**: MARA was out of roster for rounds 14-17 (4 rounds ~12 minutes). Maybe a TTL expired
- **Case admin refresh**: a separate background process refreshes caches for cases that have been stale > N ticks

We can't tell which from outside. But the fact that cache **can** refresh means the bug isn't "cache is read-only" — it's "cache refresh trigger is gated too conservatively." Fixing it probably means running the refresh every tick unconditionally, or at minimum on every case admission (even re-admission).

### Observations
1. **MARA fast5m vel 0.0925 on a re-entry is itself a classic first-read extreme.** Fourth time I've seen this pattern: round 2 MARA mid30m 0.097/0.208, round 4 BB/STLA mid30m, round 11 CLOV, round 18 MARA fast5m. Every single one of these first-reads has been in the 0.08-0.20 range and subsequently decayed within 1-2 rounds. **First-read mid30m/fast5m > 0.08 is a statistical anti-pattern for reliability.**
2. **CLOV at $2.06 in tactical_cases with vel 0.59 is Eden surfacing noise as signal.** For a stock whose normal daily range might be $0.05-0.15, a topology reading implying "acceleration" at 0.6 is almost certainly capturing per-tick noise divided by a near-zero denominator. Micro-caps need a price-tier filter before entering the case roster.
3. **Burst C ended at 5 orphan-enters (r13-r17).** Burst A had 3, Burst B had 3, Burst C had 5. Pauses were r5 (1 round), r9-r12 (4 rounds), r18 (1 round so far). The pause distribution is not random — bursts tend to cluster and pauses are longer after shorter bursts. Could correlate with vol regime changes.
4. **My running session P&L**: -$25 → -$15 → -$25 is oscillating with composite. Since the composite is ringing and price is stable, my position is neither gaining nor losing decisively. I'm effectively a range trader without range exit/entry discipline.

### Decision
**HOLD MARA.** 
- Composite still in ringing band 0.30-0.42
- Price stable 10.60-10.64
- Cluster moving together, no isolation
- Cache refresh doesn't change the signal interpretation — the new vel is already decayed-prone
- If MCP recovered, no action: the range doesn't give me a clean entry or exit edge

### Improvement ideas (new)
- [ ] **Price-tier gate on case admission**: exclude symbols below e.g. $3 or $5 from tactical_cases. CLOV at $2.06 produces garbage topology readings that crowd out higher-quality signals.
- [ ] **First-read suppression**: any vel/acc value on the first tick of a case's appearance > N σ above steady-state should be displayed as `vel=[pending warmup]` not the raw value. Decay-prone first reads have been a consistent noise source.
- [ ] **Cache refresh audit**: whatever triggered MARA's round-18 cache refresh — log it. That's the mechanism to promote to every-tick unconditionally.
- [ ] **Range regime badge**: when composite oscillates in a narrow band over N ticks, tag the case as `range_bound` instead of `Growing/Peaking/Fading`. Operators treat ranges differently from trends.

## Round 19 — tick 2305 @ 16:49 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.625**. -$22.50.

### Eden state delta
- **RIOT orphan-enter returns** — r18 was a 1-round pause, not burst end. Burst C continues. Session orphan-enters: 7 total (r2, 3, 4, 6, 7, 8 + r13, 14, 15, 16×2, 17, 19 — actually let me recount: r2 F, r3 MSTR, r4 JD, r6 JD, r7 F, r8 RIOT, r13 MSTR, r14 JD, r15 F, r16 QUBT+RIOT, r17 RIOT, r19 RIOT = 13 orphan-enter events across 13 rounds).
- **MARA cache pinned at r18 values**: fast5m vel 0.0925 / acc 0.1040 — same as r18 exactly. Second generation of cache persistence starting. The refresh that happened at r18 was a one-time event, not a new normal.
- **MARA composite 0.365 → 0.395** (fifth ringing cycle tick). Still in 0.30-0.42 band.
- **Crypto cluster mixed**: MSTR weakened (0.552 → 0.505), RIOT/CLSK firmed (0.431→0.457, 0.421→0.464), MARA firmed (0.365→0.395). No longer moving together.
- **DELL.US composite -0.242** but action=review Growing — another topology/composite contradiction (same as HPQ in earlier rounds).
- CLOV still in roster ($2.06 stock with vel 0.59 cached from r18).

### Observations
1. **Session-wide orphan-enter tally**: ~13 events across 19 rounds = 68% rounds with orphan-enter. Pauses at r5, r9-r12 (4-round), r18 (1-round). Every single one is from the 6-symbol club (F, JD, MSTR, QUBT, VNET, RIOT). **A 6-symbol deny list on orphan→enter would eliminate 100% of session-wide orphan-enter bug manifestations**, at minimum until a deeper fix.
2. **Cache staleness comes in generations**: MARA fast5m was frozen at v1 (0.0110/0.0002) from r1 to r17, refreshed at r18 to v2 (0.0925/0.1040), now pinned at v2 for r19. The refresh event doesn't fix the bug — it just promotes to the next stale snapshot.
3. **Topology/composite contradictions are common**. HPQ (earlier), DELL (this round): "Growing vortex case" with a negative composite. Either the pressure field and the convergence composite are computed from disjoint inputs and can disagree, or one of them is stale. Both should be reconciled or flagged.
4. **Session P&L has oscillated in a narrow band for ~30 minutes**: roughly -$15 to -$32 range. On a 500 share position, ~$17 swing band. Transaction cost of a round-trip would eat ~$5, so actively trading in this range has no edge.

### Decision
**HOLD MARA.** Same reasoning as r18:
- Range regime, no trend signal
- Transaction cost > expected edge on any intra-range trade
- Exit signal cancelled by r15-r17 recovery; entry signal absent; hold is the only defensible action

### Improvement ideas (new)
- [ ] **6-symbol orphan deny list** as a quick patch until the root cause is fixed. Files: orphan-enter policy path, probably `src/us/pipeline/reasoning/policy.rs` or wherever `strong top signal is not yet represented in tactical cases` is emitted.
- [ ] **Topology-composite reconciliation**: if a case is `Growing` but composite < 0, raise it for audit. At minimum, suppress the `action=review` unless at least one dimension confirms the topology call.
- [ ] **Cache refresh is episodic, not continuous** — now confirmed on MARA fast5m. Fix needs to force refresh every tick, not rely on an infrequent trigger.

## Round 20 — tick 2407 @ 16:52 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.595** (-$0.03 vs r19). -$37.50 paper.

### Eden state delta
- **Seventh clean round** (r5, r9, r10, r11, r12, r18, r20). Zero enter, zero orphan. Clean rate: 7/20 = 35%.
- **MARA fast5m cache pinned at v2**: 0.0925/0.1040 identical to r18, r19, r20. Third round of second-generation staleness. Bug fully symmetric: v1 held 15+ rounds, v2 now in its 3rd.
- **MARA composite: 0.395 → 0.356** — sixth ringing cycle tick. Still in 0.30-0.42 band.
- **Precious metals twins**: GLD and SLV both admitted with vel 0.0819/0.0818 and acc 0.1137/0.1139 — virtually identical values. Either (a) real strong cross-correlation in the same sector, or (b) a shared accumulator computation artifact. Probably (b): identical to 4 decimal places is too clean.
- No crypto cluster in tactical_cases this round — rotation to precious metals + semis (INTC).

### Observations
1. **Clean-round rate has stabilized at ~33-35%**. After 20 rounds, rate is 7/20. Bursts and pauses roughly balance.
2. **MARA slowly bleeding within the ringing range**: the cycle midpoint is drifting down. r5-r9 oscillation center was ~0.40, r10-r14 center was ~0.35, r15-r20 center is ~0.38. **Range center is drifting down while cycle amplitude stays constant.** This IS a trend signal, hiding inside oscillation. Eden would need a low-pass filter on composite to surface it.
3. **MARA price: -$37.50 is the 2nd lowest intrarun P&L** (low was -$50 r14). The slow bleed hasn't stopped even though composite ringing looks flat.
4. **I've been watching one position for 20 rounds without meaningful action** — that's a legitimate problem with the Eden operator workflow. The tool gives me lots of metrics to stare at but no trigger for decisive action. Single-position hold-or-exit is not being well served.

### Decision
**HOLD MARA.** Same mix as r18-r19. The slow bleed is inside the noise band, but I'll flag that if -$40 breaks with continued composite decline, that's my trigger.

### Improvement ideas (new)
- [ ] **Composite low-pass filter**: emit a `composite_smoothed_20ticks` series. If the smoothed series is trending down while the raw is ringing, you've got a hidden trend. This would have caught the MARA midpoint drift.
- [ ] **Shared-input detection across symbols**: when two symbols show near-identical vel/acc values (GLD/SLV this round, crypto cluster earlier), flag as `correlated_computation` and display once with "group" wrapper to avoid duplication noise.
- [ ] **Operator idle detection**: after N rounds of "hold" with no position adjustment, Eden should surface a "nothing to do" banner instead of demanding the operator re-read the roster. Would reduce cognitive tax.

## Round 21 — tick 2508 @ 16:55 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.590**, -$40 paper. Near session low (10.570 @ r14).

### Eden state delta
- **VNET orphan-enter**. 14 of 21 rounds have had orphan-enter (67%). 6-symbol club still 100% of source.
- **MARA DROPPED from tactical_cases AGAIN** (absent r10-r12, r14-r17, r21 = 8 of last 12 rounds absent).
- **MARA composite: 0.395 → 0.356 → 0.335** — three consecutive down ticks. Testing the r14 low (0.305).
- **MARA is cluster laggard again**:
  ```
  MSTR 0.503
  CLSK 0.485
  VNET 0.473 (now strong via orphan path)
  RIOT 0.431
  MARA 0.335  (bottom, -0.17 below CLSK)
  ```

### This looks like a setup identical to r14, but 3 ticks deep
r14 setup: price low, composite low, roster absent, cluster laggard → rebound r15.
r21 setup: price near-low, composite 3-tick decline, roster absent, cluster laggard → ??

The difference: r14 was a 1-tick move (r13 0.331 → r14 0.305). r21 is a 3-tick move (r18 0.395 → r21 0.335). **Three consecutive down ticks has more signal persistence than one single-tick dip.** This matches my round-20 observation about the "hidden trend in the drifting range midpoint" — the drift is now showing up as a sequence, not just an average.

### Decision
**HOLD MARA** but this is the **closest approach to a real exit signal since r10**.
- If composite breaks 0.30 next round (below r14 low), that's a decisive breakdown
- If price breaks 10.57 next round, that's a decisive breakdown  
- If either happens: exit would be warranted and I'd act immediately if MCP were up
- If composite rebounds instead (r15 pattern), continue hold
Explicit trigger: exit on (composite < 0.305 OR price < 10.57) at round 22.

### Observations
1. **This is what I wanted Eden's "decisiveness score" to capture**: 3 consecutive composite declines + cluster laggard + roster absence is materially different from 1-tick dip with bounce. Eden should upgrade the exit signal strength accordingly.
2. **The hidden-trend-in-range thesis from r20 is now observable in raw composite too** — not just in cycle midpoints. The 6-round window r15 0.407 → r21 0.335 is a -0.07 drift, on top of the ringing. Meaningful.
3. **Had I bound a `signal_decisiveness_score` to this position at r10**, it would have been ~0.3 (1 tick + single-round break). At r14 it was ~0.6 (3-tick decline but short). At r21 it would be ~0.8 (3-tick, cluster laggard, roster absent, near-low price). Decisiveness grows monotonically when the signal is real; it oscillates when the signal is noise. I'd trust a 0.8 exit signal, not a 0.3.

### Improvement ideas (new)
- [ ] **Signal decisiveness score (concrete spec)**: combine (a) count of consecutive directional composite moves, (b) cluster-relative position, (c) roster presence/absence duration, (d) price-level break. Emit as a single 0-1 scalar per position.
- [ ] **Explicit conditional exit triggers**: let operators register "exit MARA if composite < 0.305 OR price < 10.57" directly with Eden. Eden emits a notification on trigger firing. Would close the MCP-down gap by pre-loading the trigger before execution matters.

## Round 22 — tick 2608 @ 16:58 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.590** (flat vs r21), -$40.

### Trigger check from r21
- Trigger 1: composite < 0.305 — **NOT fired** (composite bounced to 0.380)
- Trigger 2: price < 10.57 — **NOT fired** (price flat at 10.590)
- **Decision: continue HOLD.** The triggers were calibrated to catch a real breakdown, not a mid-fluctuation dip. They correctly did not fire on noise.

### Eden state delta
- **MARA composite 0.335 → 0.380** (+0.045) — r21 down streak did not continue. Fifth ringing cycle begins.
- **MARA BACK in tactical_cases** with cached v2 values 0.0925/0.1040 (same as r18/r19/r20). 4th round of v2 cache. v1 lasted ~17 rounds; v2 is in round 4 now.
- **Double orphan-enter**: JD + SMCI. Second double of session (first was r16 QUBT+RIOT).

### Orphan club expanded — my deny-list proposal was incomplete
**SMCI.US is NEW to orphan-enter** — not in my previously-tracked 6 (F, JD, MSTR, QUBT, VNET, RIOT). SMCI composite 0.432, price 27.39. This is the first orphan-enter from outside my tracked club across 22 rounds. **Simple deny-list is not a sufficient fix.** The club grows over the session.

Updated orphan-enter roster: F, JD, MSTR, QUBT, VNET, RIOT, SMCI (7 names, 16+ events across 22 rounds).

The real underlying issue: composite → enter threshold on the orphan path is tuned such that any name crossing the composite with strong momentum + pre_post + no trade-flow penalty fires. The 6-name pattern was a coincidence of which names had gap-ups THIS session; any name crossing the threshold would do the same thing. **Fix must be at the orphan policy, not a symbol deny-list.**

### Observations
1. **My r21 trigger logic held up** — declarative conditional exits work even without MCP. Eden should support this natively (r21 improvement idea) as a first-class feature.
2. **Composite ringing continues without resolution** — now 5 full cycles over ~15 rounds. The amplitude has been ~0.05-0.10, midpoint ~0.37. The hidden downward drift from r20 is still there but slow enough that exit triggers keep being safely conservative.
3. **Session stats after 22 rounds (~66 minutes)**:
   - My position P&L: peak +$20 (r4), trough -$50 (r14), current -$40, roundtrips 4+
   - Eden enter emissions: 16+ (all orphan), 0 vortex
   - Clean rounds: 7 of 22 (32%)
   - Stale-cache cases observed: MARA (2 generations), BB, GDS, HPQ, LEGN, IBIT, RIVN at minimum
   - Orphan club: 7 names (expanded from 6 at r21)
   - Cross-file consistency issues: HPQ, BB, IBIT, DELL, MARA observed with disagreement between live_snapshot and operational_snapshot actions
4. **If I were running this session for real with working MCP**, the most likely outcome would have been: entered at r1 (+$0 immediately), exited at r10 or r11 (-$22), missed the r15 rebound, net session -$22. The r10 exit decision I've agonized over would have been net-correct but not optimal. Optimal would have been to enter smaller, hold through noise, and exit only on decisive triggers like the ones I wrote at r21.

### Decision
**HOLD MARA.** Triggers did not fire. Composite reset the ringing cycle. No action needed.

### Improvement ideas (new)
- [ ] **Orphan-enter policy fix (not deny-list)**: the real fix is in the policy layer. Add `trade_flow_penalty` when `buy_percentage < sell_percentage` regardless of composite strength. The 6-name deny-list would have missed SMCI; a principled policy fix scales.
- [ ] **Conditional exit triggers (spec)**: operator-registered trigger = `{symbol, condition: (field, op, value)[], action}`. Eden evaluates each tick, notifies on fire. My r21 trigger ("MARA exit if composite<0.305 OR price<10.57") should have been a first-class Eden feature, not a note in my scratch log.
- [ ] **Session retrospective generator**: at end of session, auto-produce a summary of (a) Eden decisions, (b) operator decisions, (c) hypothetical optimal decisions, (d) infrastructure gaps. Would make feedback loops explicit.

## Round 23 — tick 2710 @ 17:01 UTC

### Operator state
- MCP still disconnected. MARA 500 @ 10.670, mark **10.575** (new session-low tie with r14's 10.570). -$47.50 paper.

### Trigger check from r21
- Trigger 1: composite < 0.305 — **NOT fired** (0.384)
- Trigger 2: price < 10.57 — **NOT fired by 0.005** (10.575 is not < 10.57 strictly)
- **Closest approach of the session**. Trigger discipline says HOLD.

### Eden state delta
- **CLSK orphan-enter** — 8th name in the orphan club (F, JD, MSTR, QUBT, VNET, RIOT, SMCI, CLSK). Club membership now includes actual crypto name, not just MSTR proxy.
- **MARA absent from tactical_cases** (7 of last 9 rounds out).
- MARA composite 0.380 → 0.384 (+0.004, essentially flat).
- **Crypto cluster drifting down together**:
  ```
  MSTR 138.78 -0.14%
  CLSK 11.415 -0.26%  
  RIOT 18.443 -0.44%
  MARA 10.575 -0.14%
  ```
  Cluster-wide fade, not MARA-isolated. Price drift per name is all in the same -0.14% to -0.44% band.

### Key distinction: cluster drift vs MARA weakness
My r16 "relative weakness" signal was MARA underperforming a strong cluster. r23 is different: cluster is drifting down together, MARA is in-line with peers. **Holding through a cluster drift has different risk than holding through isolated weakness.** Cluster drift usually reverses when the theme revives; isolated weakness often continues. This is why the triggers didn't fire — the signal isn't clean enough.

### Decision
**HOLD.** With a tighter trigger revision for next round:
- Trigger 1: composite < 0.305 (unchanged)
- Trigger 2: price ≤ 10.57 (changed from `<` to `≤` — 10.570 OR BELOW should count)
- Trigger 3 (new): cluster-relative check — if crypto cluster composite avg is > 0.45 AND MARA composite < 0.34, MARA is isolated laggard, exit.
- If ANY of the three fires at r24, I would exit if MCP were up.

### Observations
1. **Price has tested the session low four times now** (r14 10.570, r20 10.595, r21 10.590, r23 10.575). Each test has found a buyer at or near 10.57-10.60. This looks like a support level — bid is holding. If support breaks cleanly, exit. If support holds, continue.
2. **Triggers as discipline tool worked at r22 and r23**: I wanted to exit at both rounds emotionally, and the triggers said hold, and both were correct calls (no further breakdown). Discipline > instinct.
3. **The orphan club has grown from 6 → 8 across 22 rounds of observation**. Rate of club growth: ~1 new name per 8 rounds. Projected: in a full 6.5-hour session there'd be ~15-20 names in the orphan club. A deny-list would need constant curation.
4. **MARA position has been held for ~70 minutes now** across 22 rounds. I've made 0 additional trades. My reading of Eden signals has been volatile but my actions have been minimal — which is probably the right ratio for the current signal quality.

### Improvement ideas (new)
- [ ] **Support-level detection**: when price has tested the same level N times without breaking, emit a `price_support_at_X` field. Exit triggers should reference support rather than absolute price — "exit on support break" is more meaningful than "exit below X".
- [ ] **Cluster-drift vs isolated-weakness disambiguation**: Eden should compute `symbol_composite_zscore_within_cluster` each tick. If the zscore is normal but all cluster composites are falling together, tag as `cluster_drift`. If zscore is tail-extreme, tag as `isolated_weakness`. Different trade responses.
- [ ] **Trigger modification audit log**: I've modified my exit triggers twice in 2 rounds (r21 set, r23 revised). Eden should track trigger history per position to surface "operator trigger drift" — moving goalposts is a common discretionary trap.

## Round 24 — tick 2810 @ 17:04 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.575** (flat, ties r23 session-low). -$47.50.

### Trigger check
- T1 composite<0.305: 0.374 — NOT fired
- T2 price≤10.57: 10.575 — NOT fired (above by 0.005)
- T3 cluster>0.45 AND MARA<0.34: cluster 0.507✓, MARA 0.374>0.34✗ — NOT fired
- **HOLD**

### Eden state delta
- **Third double orphan-enter**: MSTR + CLSK. Orphan club still 8 names (no new additions).
- MARA absent from tactical_cases (8 of last 10).
- Fragmented basket: LQD/USO/ICLN/SPGI/CIEN/KWEB — no theme this round.
- MARA composite 0.384→0.374 (-0.010, noise).
- Cluster firming slightly: MSTR 0.556, CLSK 0.499, RIOT 0.467. MARA 0.374 lags but not extreme.

### Observations
1. **MARA composite stuck in 0.36-0.40 range for 4 consecutive rounds** (r20-r24 range centres). The 0.38 midpoint is holding with very narrow oscillation now (±0.02 vs earlier ±0.05). Either (a) the market is finding equilibrium at this level — which is the end of the ringing and start of trend — or (b) Eden's composite calculation has hit a low-volatility regime and stopped providing differentiating signal.
2. **Price has been 10.575-10.595 for 5 rounds**. Ultra-narrow price range (<$0.02). Support at 10.57 holding on each test.
3. **Session is now 72 minutes in, no new trades**. MARA P&L -$47.50 ≈ worst of my held range but not session low. The "hold through noise" strategy has been correct but bores the operator; I understand why undisciplined traders churn in these conditions.
4. **Nothing new to say this round** — same signal mix as last 3 rounds. Marking this round mostly for trigger discipline and sparse observations.

### Decision
HOLD. Same triggers into r25.

### Improvement ideas (new)
- [ ] **Low-signal-novelty throttle**: when Eden's case roster and composite values have negligible delta between ticks, surface that as "no new information — skip" so operators don't spend cognitive cycles re-analyzing the same data.
- [ ] **Volatility regime detection**: if composite stddev over last N ticks drops below a threshold, tag the case as `low_vol_regime`. Low-vol regimes before a breakout (like this 4-round tight range on MARA) are themselves signal.

## Round 25 — tick 2913 @ 17:07 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.580** (+$0.005 vs r24). -$45.
- MARA composite 0.374 → 0.379 (+0.005).

### Trigger check
- T1 composite<0.305: 0.379 — NOT fired
- T2 price≤10.57: 10.580 — NOT fired
- T3 cluster>0.45 AND MARA<0.34: cluster avg 0.492✓, MARA 0.379>0.34✗ — NOT fired
- **HOLD**

### Eden state delta
- **JD orphan-enter** (4th JD occurrence this session). Club still 8 names.
- **MARA back in tactical_cases** with cached v2 0.0925/0.1040 (5th consecutive round of v2 cache).
- Roster: IBIT, CLOV, HPQ, FUTU, KWEB, GLD, MARA, BITO — mixed theme, KWEB/BITO indicate some China+crypto cluster interest.
- 6-round tight range on MARA composite (r19-r25: 0.395/0.365/0.335/0.380/0.335/0.384/0.374/0.379) — oscillating around 0.37.

### Observations
1. **No new signal this round.** MARA is in a prolonged low-volatility waiting pattern. Eden's roster churn is cosmetic; nothing has changed structurally about my position or the cluster.
2. **This is the 4th consecutive round of "HOLD, nothing to say"** (r22-r25). From an operator POV, Eden's information density here is near zero for my specific position. Possible that Eden should surface an explicit "your position is in a stable regime, check back in 15 min" message to reduce operator burnout.
3. **The session's most valuable output right now is the trigger-discipline enforcement**, not the Eden signals themselves. My three pre-set triggers are doing more work than my ad-hoc reading of the roster.

### Decision
HOLD. No trigger changes into r26.

## Round 26 — tick 3011 @ 17:10 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.585** (+$0.005). -$42.50.

### Trigger check
- T1 composite<0.305: 0.407 — NOT fired (+0.028, clear bounce)
- T2 price≤10.57: 10.585 — NOT fired
- T3 cluster>0.45 AND MARA<0.34: cluster avg **0.443** (below 0.45 floor!), MARA 0.407 — NOT fired on either condition
- **HOLD**

### Eden state delta — first relative bullish flip for MARA in 10+ rounds
```
Cluster composites r25 → r26:
MSTR  0.553 → 0.475  (-0.078)
CLSK  0.450 → 0.447  (flat)
RIOT  0.472 → 0.406  (-0.066)
MARA  0.379 → 0.407  (+0.028) ← only name up
```
MARA went **from cluster laggard to 3rd place** (above RIOT). Cluster-wide weakness, MARA diverging positive. This is the first round since r5-r6 peak where MARA has shown relative strength within its cluster.

### Double orphan-enter + new club member
- JD + SLV. JD is now 5 orphan-enters in session (most frequent).
- **SLV.US is the 9th orphan club member** — first non-tech/crypto name (precious metals). The club continues to grow ~1 new name per ~7 rounds.

### Observations
1. **Relative outperformance within cluster is a positive signal, but it's only 1 tick.** My r15 lesson: don't chase single-round spikes. Would want 2-3 ticks of consistent outperformance before upgrading the thesis.
2. **Cluster weakening (crypto basket composite dropping) but MARA ticking up** could be either (a) MARA specifically catching a bid (bullish), or (b) Eden's composite fluctuating randomly and I'm seeing a noise tick. At composite granularity I can't tell.
3. **Price remains stuck at 10.575-10.595** through this composite oscillation. 6 rounds now. The composite is ringing loudly while the price barely moves — typical low-vol consolidation pattern.

### Decision
**HOLD.** The bullish flip is too early to trust (1 tick). Setting a watch for r27: if cluster weakness continues AND MARA composite stays above 0.40 AND price ticks up, that would be 2-tick confirmation of a potential relative breakout within the cluster.

### Improvement ideas (new)
- [ ] **Relative strength alert**: when a name flips from below-cluster to above-cluster composite within a tick, emit a `cluster_rs_flip` event. This is a materially different signal from absolute composite moves and deserves its own event type.
- [ ] **Composite vs price "elasticity" ratio**: over the last N ticks, `|Δcomposite|/|Δprice|`. When composite oscillates loudly and price doesn't, the composite is detached — probably more noise than signal. Eden can compute this and dampen exit/entry policy during high-elasticity periods.

## Round 27 — tick 3111 @ 17:13 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.610** (+$0.025 vs r26). -$30.
- **First meaningful price move off session-low support** in 6 rounds. Tape is waking up slightly.

### Trigger check
- T1 composite<0.305: 0.398 — NOT fired
- T2 price≤10.57: 10.610 — NOT fired
- T3 cluster>0.45 AND MARA<0.34: cluster avg (0.512+0.473+0.470)/3 = 0.485 > 0.45 ✓, MARA 0.398 > 0.34 ✗ — NOT fired
- **HOLD**

### R26 watch condition check
- "Cluster weakness continues AND MARA comp > 0.40 AND price ticks up"
- Cluster RECOVERED (MSTR 0.475→0.512, RIOT 0.406→0.473, CLSK 0.447→0.470) — condition NOT met
- MARA composite 0.407 → 0.398 (pulled back to below 0.40 threshold)
- **R26 relative-strength flip was a 1-tick whipsaw, consistent with the r15 pattern.** Glad I didn't chase.

### Eden state delta
- **Double orphan-enter again**: MSTR + CLSK (both crypto club, 5th double of session).
- Cluster composites recovered to cluster strength regime.
- MARA slightly lagging cluster again but less extreme than before:
  ```
  MSTR 0.512
  RIOT 0.473 (up from 0.406)
  CLSK 0.470
  MARA 0.398
  ```
- MARA now -0.11 below cluster avg, not the -0.17+ laggard we saw at r16/r21.

### Observations
1. **Price divergence inverted this round**: price up $0.025, composite down 0.009. This is the OPPOSITE of the rounds where composite ringed while price sat still. Four patterns observed this session:
   - Price up + composite up (r1-r5 entry run)
   - Price flat + composite down (r7-r14 rinsing with hidden trend)
   - Price flat + composite up (r15-r17 whipsaws)  
   - Price up + composite flat/down (r27 now)
   Each deserves different interpretation and Eden conflates them.
2. **MARA price 10.610 is the best print since r17** (10.640 was r17). The support at 10.57 has held cleanly for 6 tests (r14, r20-r23, r26 test, r27 bounce). Bid-side conviction is visible.
3. **R26 relative strength flip is now officially a whipsaw** — 2 ticks was enough to resolve. 1-tick signals on this noise-dominated session are unreliable. Would need 2 consecutive confirmations before trusting a flip.

### Decision
**HOLD.** Bullish price action + lagging composite is ambiguous; neither triggers exit nor justifies add.

### Improvement ideas (new)
- [ ] **Price-composite regime classifier** (4 categories above): tag each case with `regime: {trending_up, distribution, whipsaw, quiet_up}` based on recent delta history. Operators should read different stories from different regimes.
- [ ] **2-tick confirmation rule on relative strength flips**: don't emit the flip alert until 2 consecutive ticks of outperformance. Reduces false signals like r26.

## Round 28 — tick 3210 @ 17:16 UTC — **T3 TRIGGER FIRED**

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.620** (+$0.010 vs r27). -$25 paper.

### Trigger check
- T1 composite<0.305: 0.329 — NOT fired (close to firing)
- T2 price≤10.57: 10.620 — NOT fired (price rising)
- T3 cluster>0.45 AND MARA<0.34: cluster avg **0.491** ✓, MARA **0.329** < 0.34 ✓ — **FIRED**

### The trigger conflict
- T3 fires (cluster-relative laggard)
- BUT price is actually rising (+$0.025 over 2 rounds)
- Composite dropped 0.069 in one tick (0.398 → 0.329) — biggest single-tick drop of session
- Cluster:
  ```
  MSTR 0.517 (firm)
  CLSK 0.498 (firm)
  RIOT 0.458 (firm)
  MARA 0.329 (tumbled)  ← -0.13 below RIOT
  ```
- Isolated composite weakness in a firming cluster — this IS the exact pattern I designed T3 to catch
- But price action says "bid still strong, +$0.01 this tick"

### Hypothetical execution
**If MCP were up, I would SELL 500 MARA at market (~10.62), realizing -$25 paper loss.**

Why execute on trigger despite price divergence:
1. Discipline > instinct. I designed T3 at r23 and it fired exactly as designed.
2. Composite dropping 0.069 in one tick is a larger move than 5-round average — not noise.
3. Cluster-relative laggard signal is a distinct failure mode from isolated move, and it just manifested.
4. Price+$0.01 is less convincing than composite-0.069 because price has been stuck in a $0.02 range for 7 rounds; a single $0.01 tick doesn't prove bid conviction.
5. Paper -$25 is small; teaching value of executing a disciplined exit > $25.

### Trigger imperfection noted
T3 doesn't consider price direction. A better trigger would have been:
```
T3': cluster > 0.45 AND MARA < 0.34 AND price_5min_delta < 0
```
The AND-with-price-falling condition would have prevented firing here. But that's a refinement I didn't make at r23. Eden's conditional trigger system should support compound conditions natively so operators can build nuanced triggers.

### Eden state delta
- **Double orphan-enter**: QUBT + CLSK. 6th double of session. QUBT returns after absence.
- **MARA absolute composite drop 0.398 → 0.329** — largest single-tick drop since session start.
- MARA price continues lifting 10.585 → 10.610 → 10.620 (over 3 rounds).
- MARA still in tactical_cases (v2 cache 7th round).

### Observations
1. **Composite and price DECOUPLED this round.** Price up, composite sharply down. If the composite were price-driven it shouldn't do this. Either (a) composite is capturing dimensions other than price that weakened independently, or (b) the composite computation has a noise component unrelated to tape. The r27 pattern "price up, composite flat/down" extended into outright divergence this round.
2. **The trigger firing caused me to commit to a decision I'm not 100% sure of.** That's exactly what triggers are for — they force action in ambiguous moments. An operator without triggers would rationalize holding on the price move; I'd be "waiting one more round." Triggers break that loop.
3. **This is the first trigger firing of the session** (27 rounds of non-firing, 1 firing). That's reasonable trigger calibration — not every round fires, only when the specific pattern I was worried about materializes.

### Decision
**EXIT MARA 500 @ market** — hypothetical only (MCP down). Would actually exit at ~10.62, recognizing -$25 paper loss. The infrastructure gap prevents execution; the decision is recorded.

### Improvement ideas (new)
- [ ] **Compound trigger conditions**: Eden's trigger system must support boolean combinators (AND/OR/NOT) over multiple fields, not single scalars. The T3 imperfection was a missing AND.
- [ ] **Trigger post-mortem**: after a trigger fires, Eden should automatically emit a "trigger fired; here's the subsequent N-round outcome" log for learning. If T3 exits here turn out to be correct, it's a reinforced signal; if they turn out to whipsaw, the trigger needs refinement.
- [ ] **Max composite single-tick move alert**: `Δcomposite` of -0.069 in one tick is ~3-4× the session median. Eden should surface big-delta events prominently, not bury them inside the composite field.

## Round 29 — tick 3309 @ 17:19 UTC — **R28 T3 Post-mortem (mixed result)**

### Operator state
- MCP disconnected. Still holding MARA 500 (could not execute r28 exit). Mark **10.585** (-$0.035 vs r28). -$42.50.

### R28 trigger post-mortem
**Hypothetical exit at r28 @ 10.62, 1-round later at 10.585:**
- **Price verdict: CORRECT** — price dropped $0.035, saved $17.50 of drawdown
- **Composite verdict: WHIPSAWED** — composite bounced 0.329 → 0.380 (+0.051)
- **Cluster verdict: RETURNED TO NORMAL** — MARA back to mid-pack laggard, not extreme

Net: the trigger *was* right on the price-side test, *wrong* on the composite-side test. This is exactly the kind of mixed outcome a decisiveness/outcome-tracking system needs to learn from. **Single trigger firing + single-round evaluation isn't enough data** — need N rounds post-fire.

Running post-mortem tracker:
```
Fired at r28: price 10.620, composite 0.329, cluster 0.491
r+1 (r29): price 10.585, composite 0.380  → price ✓, composite ✗
```

### R29 trigger check
- T1 composite<0.305: 0.380 — NOT fired
- T2 price≤10.57: 10.585 — NOT fired
- T3 cluster>0.45 AND MARA<0.34: cluster 0.494✓, MARA 0.380>0.34✗ — NOT fired
- **No new triggers this round.**

### Eden state delta
- MARA composite snapped back 0.329 → 0.380 (+0.051). Largest intra-round bounce of session after largest intra-round drop of session (r28). **This session is noise-dominated.**
- Cluster tightened: MSTR 0.523, CLSK 0.500, RIOT 0.458, MARA 0.380. Dispersion compressed.

### Decision
**No change — still holding (forced by MCP).** If I had exited at r28 and had to decide re-entry at r29, I would NOT re-enter:
- Trigger design said exit; re-entering immediately undermines the trigger
- Composite recovery is a known whipsaw pattern (r15 precedent)
- Would wait for either an explicit entry trigger (not designed yet) or next day

### Observations
1. **This session is demonstrating that noise dominates structure** on a 3-minute tick cadence. In ~30 rounds I've seen:
   - ~5 meaningful price moves (r1-r5 up, r9-r14 drift, r15-r17 recovery, r20-r24 chop, r27-r28 lift)
   - ~10 composite oscillations with no lasting resolution
   - 1 trigger firing (r28) with mixed result
   - 0 completed trades (entry at r1, no other positioning changes)
   The signal-to-noise ratio is low enough that actively trading adds no value.
2. **r28 trigger fire would have been net-profitable on a re-entry at r29** (+$17.50 saved on price - $5 round-trip ≈ +$12.50). But it would have required 2 transactions in 3 minutes, which is exactly the churn Eden should be protecting operators from, not encouraging.
3. **Eden's composite moved more than its price dimension**: over r28-r29, composite moved ±0.05, price moved ±$0.035. On a $10.60 base, that's ±0.3% price and ±13% composite. Composite is ~40× more volatile than price, confirming my earlier observation. The composite is not scaled to price realities — it needs normalization or operators will keep over-reacting.

### Improvement ideas (new)
- [ ] **Scale normalize composite to realized volatility**: composite should move 1:1 with price in a neutral regime. Current ratio ~40:1 makes composite useless for sizing decisions.
- [ ] **Post-trigger N-round outcome tracker**: every trigger firing should automatically log outcomes N rounds forward on all relevant dimensions (price, composite, cluster, roster presence). After 10+ firings, calibrate the trigger's true hit rate.
- [ ] **Trigger should include post-fire quiet window**: once T3 fires, don't re-evaluate T3 for M rounds — prevents oscillation-driven re-firing on the same event.

## Round 30 — tick 3407 @ 17:22 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.625** (+$0.040 vs r29). -$22.50.
- **R28 trigger post-mortem +2 rounds**:
  - r28 fire @ 10.620, comp 0.329
  - r29 @ 10.585, comp 0.380 (price ✓, composite ✗)
  - **r30 @ 10.625, comp 0.394 (price NOW NET +$0.005 vs fire, composite ✗)**
  - Net verdict at r+2: **trigger was a whipsaw.** Had I exited and stayed flat, I'd be $2.50 worse off than holding. Had I round-tripped, I'd be -$5-10 worse off.

### Trigger check
- T1: 0.394 > 0.305 — NOT fired
- T2: 10.625 > 10.57 — NOT fired
- T3: cluster avg **0.446 < 0.45** — floor condition NOT met (cluster itself weakened)
- **HOLD**

### Eden state delta — cluster internal rotation
```
             r28    r29    r30    net
MSTR   0.517 0.523 0.535  +0.018  (gaining)
CLSK   0.498 0.500 0.428  -0.070  (faded)  
RIOT   0.458 0.458 0.376  -0.082  (faded hard)
MARA   0.329 0.380 0.394  +0.065  (recovered)
```
**MARA recovered while RIOT faded**. The composite weakness I exited on at r28 was a 1-tick dip, and the laggard rotation has moved on to RIOT. My trigger exited the wrong name. If this session had Eden's cluster-internal-rotation signal I proposed earlier, it would have shown MARA strengthening and RIOT weakening simultaneously — a signal to *rotate* not *exit*.

### Observations
1. **The r28 trigger was a textbook whipsaw.** Single-tick composite -0.069 caught the bottom of a dip. Fire discipline had me committed to exit, but the underlying signal never persisted. **My triggers need multi-tick confirmation** for composite-based firings, same as I concluded about cluster-relative flips at r26.
2. **The trigger post-mortem is clearly showing my triggers fire on noise, not structure.** 1 firing in 30 rounds, 1 whipsaw. Hit rate 0/1 so far. Needs calibration: raise composite threshold (0.30 → 0.28?), require 2+ consecutive ticks, or require price confirmation.
3. **Rotation is a separate action category from exit/hold/enter**. The cluster-internal view makes it visible: when a leader-laggard swap happens within a strong cluster, the right action is rotate (sell laggard, buy new leader), not exit. Eden has no native concept of rotation right now.
4. **R28 trigger wouldn't fire at r30** — cluster avg dropped below 0.45, so the "strong cluster" precondition no longer holds. The same dimensions now produce a HOLD from the same trigger. Trigger correctness depends on regime, and my single trigger didn't model regime.

### Decision
**HOLD.** Triggers say hold. Post-mortem of r28 says my last trigger was premature. Keeping triggers as-is for transparency — I'll note each firing + outcome without retroactively tuning.

### Improvement ideas (new)
- [ ] **Rotation signal**: when within a cluster a previous laggard strengthens AND a previous leader weakens in the same tick, emit `cluster_rotation` event with `from_symbol, to_symbol`. Operators can rotate instead of exit.
- [ ] **Multi-tick confirmation for composite-based triggers**: require composite to breach threshold for 2+ consecutive ticks before firing. Would have prevented r28 misfire.
- [ ] **Trigger hit-rate tracking per operator**: running score of operator-defined triggers (correct / whipsaw / ambiguous). Feedback loop for trigger design.

## Round 31 — tick 3505 @ 17:25 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.610** (-$0.015). -$30.

### Trigger check
- T1 composite<0.305: 0.414 — NOT fired
- T2 price≤10.57: 10.610 — NOT fired
- T3: cluster 0.456✓ / MARA 0.414>0.34✗ — NOT fired
- **HOLD**

### R28 trigger post-mortem +3 rounds
```
          r28    r29    r30    r31
price     10.62  10.585 10.625 10.610
composite 0.329  0.380  0.394  0.414
```
Net from r28 fire: price net -$0.01, composite +0.085. Trigger continues to look whipsaw in all post-mortem windows. **0/1 trigger hit rate this session.**

### Eden state delta — cluster rotation continues
```
         r28    r31    net
MSTR     0.517  0.471  -0.046  (now fading)
CLSK     0.498  0.446  -0.052  (weakened)
RIOT     0.458  0.452  -0.006  (bottomed)
MARA     0.329  0.414  +0.085  (recovered strongly)
```
**MARA has gone from cluster laggard to cluster leader in 3 rounds.** Complete internal rotation.
- RIOT orphan-enter this round (it was the prior laggard — orphan path picked it up as gap trader).
- Cluster dispersion now 0.02 (very tight): 0.446-0.471 for the four names, all within 0.07.

### Observations
1. **Full cluster rotation observed**: MARA went from bottom to top in 3 rounds while MSTR went from top to bottom. Absolute composite levels compressed into a narrow 0.41-0.47 range — the cluster is normalizing. A rotation signal would have turned my r28 "exit MARA" into "sell MSTR buy MARA".
2. **Trigger post-mortem definitively wrong**: with 3 rounds of follow-up, the r28 exit would have realized -$25 then watched the position recover $60 (composite + price combined) — a net opportunity cost of ~$50 on the trigger fire.
3. **The "cluster laggard → cluster leader" arc for MARA took 3 rounds** (r28-r31). If I had an entry trigger (not just exit), this would have been a valid re-entry signal at r29-r30. My session discipline has been asymmetric: exit triggers exist, entry triggers don't.

### Decision
**HOLD.** Don't add to position without a pre-committed entry trigger. Don't trim without exit trigger firing.

### Improvement ideas (new)
- [ ] **Symmetric trigger design**: operators should pre-commit to BOTH entry and exit conditions. Only-exit discipline creates one-sided decision-making.
- [ ] **Cluster rotation signal implementation spec**:
  ```
  if prev_tick_rank(symbol, cluster) < median AND curr_tick_rank(symbol, cluster) > median:
      emit cluster_rank_flip_positive(symbol)
  ```
  Actionable signal for "pivot into this name."
- [ ] **Trigger calibration feedback loop**: after N firings with outcomes measured, tune the thresholds automatically or recommend changes to the operator. r28 at 0.34 MARA threshold was too forgiving — the right level was probably 0.30.

## Round 32 — tick 3602 @ 17:28 UTC

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.605** (-$0.005). -$32.50.

### Trigger check
- T1 composite<0.305: 0.359 — NOT fired
- T2 price≤10.57: 10.605 — NOT fired
- T3: cluster 0.446 < 0.45 floor — NOT fired (precondition)
- **HOLD**

### Eden state delta
```
       r31    r32    net
MSTR   0.471  0.510  +0.039 (firmed)
CLSK   0.446  0.432  -0.014 (flat)
RIOT   0.452  0.396  -0.056 (faded)
MARA   0.414  0.359  -0.055 (faded)  ← leader status lasted 1 tick
```
MARA's r31 "cluster leader" position was a 1-tick reading, same whipsaw pattern. Second rotation cycle starting: MSTR re-leading, MARA + RIOT pulling back together.

### Observations
1. **Every cluster signal so far has been 1-tick.** r26 bullish flip (whipsaw), r28 laggard exit (whipsaw), r31 leader flip (whipsaw). None of them survived the next round. This session has zero "sustained" cluster-relative signals — everything is instantaneous noise.
2. **If all 4-cluster composite values oscillate with similar amplitude in the same range** (0.35-0.55 for crypto cluster), rank-based signals are unstable by construction. Need either (a) wider cluster dispersion to mean anything, or (b) absolute threshold signals that don't depend on rank.
3. **MARA P&L has stayed in a -$22 to -$50 band for 18 rounds** (r14-r32). On a 500-share position, ~$28 range. Over 54 minutes. That's low enough volatility that any trigger firing on composite movements will likely be noise.

### Decision
HOLD. Nothing changed.

## Round 33 — tick 3699 @ 17:31 UTC — **T1 + T2 BOTH FIRED**

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.568**. Paper P&L **-$51** (worse than r14 -$50 session low).

### Triggers fired (this is the real breakdown)
- **T1 composite<0.305: 0.293 FIRED** — first sub-0.305 reading of entire session (previous session low was 0.305 at r14)
- **T2 price≤10.57: 10.568 FIRED** — clean break of session low 10.570 from r14
- T3: cluster avg 0.445 < 0.45 floor — NOT fired (because cluster is mixed)
- **BOTH PURE-MARA TRIGGERS FIRED INDEPENDENTLY**

### This is NOT r28
The r28 trigger was composite-only, whipsawed. Round 33 has:
1. **Composite AND price both breaking new session lows simultaneously** — not just one dimension
2. **MARA isolated within mixed cluster**: MSTR 0.550 (firm), CLSK 0.394 (weak), RIOT 0.392 (weak), **MARA 0.293 (worst)**. The crypto basket is bifurcating; MARA on the wrong side.
3. **Support level broke cleanly** — 10.568 is below the 10.570 r14 session low that had held 6+ tests
4. **Divergence from MSTR**: MSTR composite 0.55 vs MARA 0.29 is a 0.26 spread, largest in session

Unlike r28 (single-tick composite dip while price rose), r33 is a proper breakdown: multi-dimensional agreement.

### Hypothetical execution
**If MCP were up: SELL 500 MARA @ market immediately (~10.57).**
Realized loss: **-$50 to -$51** (depending on fill).

### Observations
1. **This is the first Eden signal all session that has produced unambiguous multi-dimensional confirmation.** Price AND composite AND cluster divergence all pointing the same direction. Had I only had one of these dimensions I'd have been whipsawed (as I was at r28). All three together is rare — this is the kind of clarity operators should wait for.
2. **The session-long range 10.57-10.71 broke today.** If this holds in subsequent rounds, the "10.57 support" I tracked is gone — MARA enters a new lower range or trends down.
3. **MARA went from cluster leader (r31) to cluster worst (r33) in 2 rounds.** That's a 0.12 composite spread within the cluster in 6 minutes. Rotation is not the story — this is genuine idiosyncratic weakness.
4. **Multi-dimensional trigger hit rate on r33**: both T1 and T2 fired on the same tick, which is a form of implicit confirmation. **The right trigger design is multi-dimensional agreement**, not single-dimension threshold.

### Decision
**EXIT MARA 500** — hypothetical at 10.57 for -$51. MCP prevents execution.

The disciplined trigger system just produced:
- Correct non-fire at r22, r23, r24, r25 (narrow misses)
- Incorrect fire at r28 (whipsaw)
- Correct fire at r33 (real breakdown)
Running hit rate: 1/2 = 50% on this session. Too few firings for statistical significance.

### Improvement ideas (new)
- [ ] **Multi-dimensional trigger requirement**: require 2+ independent dimensions to breach thresholds simultaneously before firing. Would have suppressed r28 (composite only) and kept r33 (price + composite + cluster divergence).
- [ ] **Session-range break detection**: when price breaks a multi-round support or resistance level, emit a `range_break` event that amplifies any concurrent composite signal.
- [ ] **Spread-from-leader metric**: `leader_composite - symbol_composite` within a cluster. A widening spread (MSTR 0.55 - MARA 0.29 = 0.26) is more actionable than absolute composite level.

## Round 34 — tick 3796 @ 17:34 UTC — **R33 Trigger Post-mortem: CORRECT**

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.540**. Paper P&L **-$65** (new session low).
- If executed at r33: realized -$51, now flat, saved **$14** of additional drawdown on this single round.

### R33 post-mortem +1 round
```
           r33    r34    delta
price      10.568 10.540 -$0.028 ✓ confirmed
composite  0.293  0.292  flat    ✓ stayed broken
cluster    mixed  weaker         ✓ no recovery
```
**R33 T1+T2 fire = CORRECT.** First correctly-fired trigger of the session. Multi-dimensional agreement (price + composite) turned out to be meaningful, unlike single-dimension r28 fire.

### Trigger check
- T1 composite<0.305: 0.292 STILL FIRING (persistent breakdown)
- T2 price≤10.57: 10.540 STILL FIRING
- T3: cluster avg 0.411 < 0.45 — not fired
- No NEW triggers; r33 exit hypothetically still in force. Already flat in the paper scenario.

### Eden state delta
- **8th clean round** (24% rate). No enter, no orphan.
- **MARA still in tactical_cases as `review Growing`** on cached v2 values (0.0925/0.1040 — now 13+ rounds of stale cache). **The lagging-indicator nature of tactical_cases is absurd here**: the position has just broken down on all real-time metrics, and Eden's operator-facing roster still shows "Growing review."
- Cluster continues weakening:
  ```
              r33    r34    net
  MSTR        0.550  0.440  -0.110 (big drop)
  CLSK        0.394  0.413  +0.019
  RIOT        0.392  0.380  -0.012
  MARA        0.293  0.292  flat
  ```
  MSTR lost 0.11 in one round. The crypto cluster is now weakening as a whole — MARA was the early warning sign, not an isolated failure.

### The "MARA as canary" observation
At r33, MARA broke down while MSTR was firm (0.55). At r34, MSTR collapsed 0.11. MARA was the early signal for cluster-wide weakness — it broke first. Eden's composite told me MARA was weakest 1 round before the cluster followed. **That's actually high-value structural information**: within a cluster, find the leading-break name and it tells you about the basket's direction 1 tick ahead. I was interpreting MARA's weakness as idiosyncratic; it was leading indicator.

### Observations
1. **R33 exit signal was correct AND led the cluster**. This is the most valuable trigger firing of the session and the clearest case for the "multi-dimensional agreement" principle. It's also the strongest case for Eden developing **leading-indicator** (vs lagging) labels on cluster members.
2. **The tactical_cases roster and the reality have diverged maximally**: roster says "MARA Growing review" while composite says "MARA broken, -32% below session peak, cluster now following." Eden needs operational truth, not cached narrative.
3. **Paper session P&L now -$65 unrealized, -$51 if executed at r33.** The MCP gap specifically cost me $14 on this single round, and will keep costing as price drops further. Infrastructure failure has a measurable cost when Eden's signal is right.

### Decision
Hypothetically flat (exited at r33 @ 10.57). Real holding continues at -$65. No new action.

### Improvement ideas (new)
- [ ] **Leading-indicator flag for cluster members**: when one cluster member's composite drops >1σ ahead of the cluster average AND cluster then follows, flag that symbol as `cluster_leader_down`. Future breakdowns on that name should fire exit on the *cluster*, not just the symbol.
- [ ] **Tactical_cases is lagging indicator — rename or restructure**: the current `tactical_cases` is a stale case list that conflicts with live data. Either rename to `candidate_cases` and gate on freshness, or merge with `operational_snapshot.state` for a single source of truth.

## Round 35 — tick 3892 @ 17:37 UTC — **R33 Post-mortem +2**

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.535**. Paper **-$67.50** (new low).
- Hypothetical flat at r33 @ 10.57: realized -$51, saved $16.50 of further drawdown at this point.

### Trigger check
- T1 composite<0.305: 0.357 — **NO LONGER FIRING** (bounced +0.065)
- T2 price≤10.57: 10.535 — **STILL FIRING** (price still below support)
- T3 cluster 0.447 < 0.45 — precondition barely missed (0.003)
- Mixed: price supports r33 exit, composite retraces

### R33 post-mortem +2 rounds
```
         r33    r34    r35    trend
price    10.568 10.540 10.535 CONTINUED DOWN ✓
composite 0.293 0.292  0.357  RETRACED ✗
```
Split verdict: **price-side correct, composite-side bounced.** Same pattern as r28 but inverted: price was the weaker signal that week, composite the weaker signal here. **No single Eden dimension is reliable alone** — multi-dimensional agreement is a necessary but not sufficient condition.

### Eden state delta
- **Cluster-wide bounce**: MSTR +0.047, CLSK flat, RIOT +0.056, MARA +0.065 (biggest).
- MARA is NOW the biggest absolute composite gainer this round — from cluster worst to cluster rebounder.
- **The "canary" observation reversed**: MARA led down AND led up. Consistent with leading-indicator thesis, just now on the upside.

### Observations
1. **MARA's leading-indicator property may be confirmed**: it dropped first (r33), cluster followed (r34). Then it bounced first (r35). A true leading indicator leads in both directions. Eden should specifically track "first to move" names within a cluster.
2. **Price did not bounce** despite composite recovery. Suggests the composite rebound is on lower-weight dimensions (order flow, sector coherence) rather than actual bid. Price is the final arbiter.
3. **R33 trigger still the correct call in hindsight** (price has continued to break session low), even though composite retraced. The T2 price trigger is the more structural one; T1 composite is the more whippy one. Suggests future triggers should weight price > composite.
4. **This round would NOT re-enter MARA** (hypothetical scenario): price still below broken support, no entry trigger pre-set. Would wait for price to reclaim 10.57 first.

### Decision
Hypothetically flat. Real holding unchanged. No re-entry.

### Improvement ideas (new)
- [ ] **Bidirectional leading-indicator tracking**: when a cluster-member's composite leads the cluster avg by ≥1 tick in BOTH directions over a rolling window, flag as `cluster_canary`. These names are disproportionately useful for cluster timing.
- [ ] **Weight price higher than composite in exit triggers**: structural breaks (support break, resistance break) are more durable than composite oscillations. Trigger hierarchy: price > cluster-relative > composite alone.

## Round 36 — tick 3989 @ 17:40 UTC — **R33 Post-mortem +3**

### Operator state
- MCP disconnected. MARA 500 @ 10.670, mark **10.530**, paper **-$70** (new low).
- Hypothetical flat at r33 @ 10.57: realized -$51, saved **$19** of further drawdown.

### Trigger check
- T1 composite<0.305: 0.331 — NOT firing
- T2 price≤10.57: 10.530 — STILL FIRING (4 consecutive rounds below)
- T3 cluster>0.45 & MARA<0.34: cluster 0.461✓, MARA 0.331<0.34✓ — **FIRING AGAIN** (second fire of session)
- **Multi-trigger agreement**: T2 + T3 both firing, T1 close

### R33 post-mortem +3 rounds
```
         r33    r34    r35    r36    trend
price    10.568 10.540 10.535 10.530 ✓ continuing down
composite 0.293 0.292  0.357  0.331  mixed (bounced, drifting down)
```
Position is in sustained breakdown. R33 exit continues to be correct. Hypothetical saved: $14 (r34) + $4 (r35) + $1.50 (r36) = **$19.50** on the first 3 rounds post-fire. Accelerating.

### T3 refired — this time more valid
Unlike r28 when T3 fired on isolated composite dip, r36 T3 fires *during* a price breakdown:
- Price broken support for 4 consecutive rounds (structural break confirmed)
- Composite 0.331 (below 0.34 cluster laggard floor)
- Cluster 0.461 (firming back above 0.45)
- **Price break + cluster laggard simultaneously** — the condition I wanted T3 to catch originally
Already flat in hypothetical scenario; no new action. But this is retroactive validation that T3's *logic* is right; its *timing* at r28 was wrong (premature).

### Eden state delta
- MARA still cached in tactical_cases at v2 values (I didn't re-check but pattern holds).
- Cluster starting to diverge: MSTR/CLSK/RIOT in 0.43-0.50 range, MARA below at 0.33. MARA is NOT following the cluster bounce from r35.

### Observations
1. **R33 is the single most important trigger firing of the session**. It caught a real multi-day breakdown (well, multi-round) on structural grounds (multi-dimensional agreement). It will end up having saved ~$20+ of drawdown by the time the trend resolves. **This is the kind of firing the entire trigger system exists for.**
2. **T3 re-firing at r36 validates the logic** but exposes that **trigger timing matters as much as trigger logic**. T3 should not have fired at r28 when price was rising; it should have fired at r36 when price was breaking. A minimal fix: require T3's cluster-laggard condition to coexist with a price-direction check.
3. **MARA is no longer moving with the cluster**. The r34 cluster bounce continued at r35 (all bounced together) but r36 shows MARA decoupling: cluster firming while MARA drops further. This is a real divergence, not a 1-tick noise — it's on round 3 now.
4. **My position is now in a genuine, validated breakdown and I can't exit because of infrastructure.** Paper loss -$70 and growing. Eden's signal quality on this event has been high (r33 trigger correct, r35 split but leaning price); my execution is hobbled.

### Decision
Hypothetically flat since r33. Real holding continues due to MCP disconnection. No new action.

### Improvement ideas (new)
- [ ] **Triggered-flag state machine**: once a trigger fires, Eden should enter a "risk-off" state for that position and *prevent* re-entry of new longs until the condition clears. Would formalize my "don't re-enter" rule.
- [ ] **Trigger logic + timing separation**: factor triggers into (logic-clause, timing-clause) pairs. Same logic can be right in one timing context and wrong in another. T3 was logic-correct at r28 but timing-wrong; at r36 both are right.

## Round 37 — tick 4086 @ 17:43 UTC

### Operator state
- MCP disconnected. MARA mark **10.550** (+$0.020), paper **-$60**.

### Trigger check
- T1 composite<0.305: **0.285 FIRING** (new session low composite, below r33's 0.293)
- T2 price≤10.57: 10.550 STILL FIRING (5 rounds below support)
- T3: cluster 0.413 < 0.45 — precondition not met
- T1 is newly fresh-firing; T2 continuing

### R33 post-mortem +4 rounds
```
         r33    r34    r35    r36    r37    trend
price    10.568 10.540 10.535 10.530 10.550 modest bounce
composite 0.293 0.292  0.357  0.331  0.285  new LOW
cluster  mixed  weaker recov  mixed  down   weakening
```

**Cluster-wide weakness now confirmed** (MSTR 0.441 from 0.550 peak; RIOT, CLSK all down too). MARA led down, cluster has now fully followed. This is the clearest "canary succeeded" signal of the session.

### Leading indicator confirmation summary
```
Timing of cluster weakness signals:
r33: MARA broke first (price + composite both below session lows)
r34: MSTR collapsed 0.11 (one round lag)
r35: Cluster bounce (all up together)
r36: MARA decoupled back down, cluster firming
r37: Entire cluster weakened again; MARA new low
```
MARA's leading-indicator property is now confirmed across multiple round intervals. **Eden should definitely track first-to-move names within a cluster** — this is a high-value signal for cluster timing.

### Price vs composite divergence (again, inverted)
- R36→R37: price +$0.02 (bounce), composite -0.046 (new low)
- R28: price +$0.01, composite -0.069 (trigger fired, wrong)
- R37: price +$0.02, composite new low (trigger firing again)
Different outcomes on similar signal shape. The difference: r28 was an isolated move within a range; r37 is within a confirmed 5-round breakdown. **Context dominates signal value.**

### Eden state delta
- Cluster avg finally crossed below 0.45 floor this round — T3 precondition failed.
- T1 has been OFF since r34, re-ON at r37 with composite making new lows. Composite oscillated 0.292 → 0.357 → 0.331 → 0.285 in 4 rounds, demonstrating it's noisier than price.

### Observations
1. **Composite is exiting the ringing band downward.** The 0.30-0.42 range I characterized earlier has broken to a new lower range. From a pure "range break" perspective, this confirms the bearish bias that started at r7.
2. **R33 trigger was absolutely correct in hindsight now.** 4 rounds later: composite at new session low, cluster weakening, price still broken. Trigger hit rate improving to 1/2 with the r33 fire having saved ~$20 and counting.
3. **Price bounced modestly in round 37 while composite made a new low.** The composite is reacting to broader cross-dimensional weakness that price hasn't fully priced in yet. This is exactly the leading-indicator behavior — composite leads price by a tick or two.
4. **MARA has been a genuine bearish signal for 5 rounds now**, and the cluster just finished confirming. If Eden had automated the "rotate away from canary-down" signal, I'd have been flat since r33 and potentially short MSTR at r34. That's a meaningful edge lost to manual interpretation.

### Decision
Hypothetically flat since r33. No new action.

### Improvement ideas (new)
- [ ] **Range-break detection on composite**: when a case's composite exits a multi-round range (up or down), emit `composite_range_break` event. Would have cleanly caught the r37 breakdown.
- [ ] **Composite-leads-price detection**: compute `composite_leads_price_score` = correlation between ΔCompₜ and ΔPriceₜ₊₁ over last 20 ticks. When positive and significant, composite is leading and should be weighted higher.

## Round 38 — tick 4183 @ 17:46 UTC

### Operator state
- MCP disconnected. MARA mark **10.540**, paper **-$65**.

### Trigger check
- T1 composite<0.305: 0.309 — NOT firing (bounced above threshold by 0.004, narrow margin)
- T2 price≤10.57: 10.540 — STILL FIRING (6 consecutive rounds below support)
- T3: cluster avg 0.445, MARA 0.309 — cluster 0.005 below 0.45 floor, NOT firing on precondition
- T2 only firing this round

### R33 post-mortem +5
```
         r33    r34    r35    r36    r37    r38
price    10.568 10.540 10.535 10.530 10.550 10.540  all below 10.57
composite 0.293 0.292  0.357  0.331  0.285  0.309
cluster  mixed  weak   recov  mixed  down   recov
```
Price stayed below support for 6 consecutive rounds — structural break confirmed. Composite is now oscillating in a lower 0.28-0.33 band (vs earlier 0.30-0.42 band). The range shifted down.

### Eden state delta
- **Cluster firming again**: MSTR 0.441→0.492, RIOT 0.406→0.432, CLSK 0.392→0.410
- MARA bouncing with cluster (+0.024) but still laggard (-0.10 below cluster avg)
- Cluster avg 0.445, straddling the 0.45 T3 floor exactly

### Observations
1. **MARA composite oscillating in new lower range [0.28-0.33] while price stays below support [<10.57]**. The range shift is the key structural change — it's not a retracement, it's a regime change to a lower level.
2. **Cluster firming but MARA lagging persists** — MARA -0.10 below cluster avg is now the third consecutive round of lag. Relative weakness now has some persistence, unlike the 1-tick whipsaws earlier.
3. **R33 trigger has accumulated ~$20 of saved drawdown** across 5 post-fire rounds. At this point it's definitively validated.
4. **Nothing new to say.** The position is in a broken-down state, the signals all point the same direction, and I can't execute. The only value I'm adding is tracking the correctness of the trigger and the operator-learning content.

### Decision
Hypothetically flat since r33. Real holding unchanged. No action.

### Improvement ideas (recap only — no new)
Session-long improvement ideas have stabilized. Starting to repeat. Quality time for me to move toward end-of-session review instead of adding more fine-grained ideas.

## Round 39 — tick 4279 @ 17:49 UTC

### Operator state
- MCP disconnected. MARA mark **10.535**, paper **-$67.50**.

### Trigger check
- T1 composite<0.305: 0.307 — NOT firing (0.002 above threshold)
- T2 price≤10.57: 10.535 — STILL FIRING (7 consecutive rounds)
- T3: cluster 0.436 < 0.45 — NOT firing
- T2 only

### R33 post-mortem +6
Price: 10.535 (continued below support)
Composite: 0.307 (hovering at threshold boundary)
Cluster: 0.436 (continuing to weaken, now clearly below 0.45 floor)
Accumulated saved drawdown: ~$16.50

Position is now in a stabilized broken state. Essentially no new information round-over-round. The lower range has held for 7 rounds.

### Eden state delta
- Cluster composites drifting down: MSTR 0.492→0.455. Cluster weakness re-emerging after the r38 firming attempt.
- MARA composite stable near 0.31 — no new info.

### Observations
Near-zero-information round. The session has entered a "stable breakdown" regime:
- Price below broken support: confirmed
- Composite in lower band: confirmed
- Cluster weakening: confirmed
- Nothing new to learn from continuing to observe at 3-minute cadence

### Decision
Hypothetically flat. No action.

### Session-end approach
Ideas have stabilized; starting to repeat. Next round will begin consolidating the full session summary instead of adding new observations. The position (MARA) and triggers have been fully observed.

## Round 40 — tick 4376 @ 17:52 UTC

### Operator state
- MCP disconnected. MARA mark **10.545**, paper **-$62.50**.

### Trigger check — T2 + T3 firing together
- T1 composite<0.305: 0.336 — NOT firing (recovered above)
- T2 price≤10.57: 10.545 — STILL FIRING (8 consecutive rounds)
- T3: cluster 0.480 > 0.45 ✓, MARA 0.336 < 0.34 ✓ — **FIRING** (3rd fire of session)
- **Multi-dimensional agreement again**: T2 + T3 both firing, reinforcing r33 exit

### Eden state delta
- MSTR orphan-enter.
- **Cluster strong bounce this round**: MSTR 0.455→0.526 (+0.071), CLSK 0.421→0.446, RIOT 0.432→0.467
- MARA bounced (+0.029) but less than cluster (+0.06 avg) — classic isolated laggard pattern
- T2 + T3 simultaneous fire: multi-dimensional confirmation just like r33

---

# Session Summary — 40 rounds, 2 hours

## Operator position
- Entered: r1, MARA 500 @ 10.670 (~$5,335)
- Decision: marginal edge based on r1 dual-horizon Growing+peer_conf=1 case. Defensible on known-at-time information.
- Real status: still held (MCP disconnected after r2).
- Hypothetical r33 exit: 500 @ 10.57 → realized -$51.
- Current mark 10.545, real paper P&L -$62.50.

## P&L trajectory
```
r1  entry 10.670
r4  peak  10.710 (+$20)
r14 low   10.570 (-$50)
r17 rec   10.640 (-$15)
r20 chop  10.595 (-$37)
r21-32 range 10.575-10.625
r33 BREAK 10.568 — trigger fire — exit here in hypothetical
r37 low   10.530 (-$70 low)
r40 10.545 (-$62.50)
```

## Eden signal scorecard

### What worked
1. **Composite as primary signal (non-absolute)** — composite trajectory matched the overall arc of my position. Composite peak r5 0.418 → trough r37 0.285 = -32%. Told the story.
2. **Multi-dimensional trigger agreement (r33)** — T1+T2 firing together turned out to be correct in hindsight; saved $20+ of drawdown on hypothetical execution.
3. **Eden caught a real breakdown event (r33)** — the multi-round MARA weakness that led the cluster was a real alpha signal, caught by pre-committed exit triggers.
4. **`recent_transitions` feed exists** — though not exposed in live_snapshot, the underlying delta feed I needed is already in operational_snapshot. One-line fix away.

### What didn't work
1. **Single-dimension composite triggers whipsaw** — r28 fired on composite alone, 2-3 rounds later invalidated. Never trust composite in isolation.
2. **`tactical_cases` roster churn is ~50% per tick**, case labels flip every few rounds, narrative unstable. Unusable as a primary operator surface.
3. **Lifecycle velocity/acceleration cache is severely stale** — MARA fast5m had identical vel/acc values across 15+ rounds. BB, GDS, LEGN same pattern. Cache keyed to setup_id, persists across case dropouts/re-entries.
4. **`capital_flow_direction = 0` for ALL 639 US symbols** — a dead dimension in the composite, confirmed early. Biggest suspected root cause of orphan-enter bugs.
5. **Vortex-path `action=enter` produced 0 emissions in 40 rounds** — the topology-reasoning path Eden is built around never surfaced a high-conviction enter. All 17+ enter emissions were from the orphan path.
6. **Orphan-enter bug**: 17 enter events, ALL from the orphan_signal driver, all from a small club (F, JD, MSTR, QUBT, VNET, RIOT, SMCI, CLSK, SLV). The club grew from 6 to 9 over the session.
7. **First-read mid30m vel/acc ≥0.08 is an anti-signal** — observed 6+ times, every occurrence decayed within 1-2 ticks. MARA r2, BB r4, STLA r4, CLOV r11, FUTU r5, AXON r7, CLOV r18 all fit.
8. **Cross-file action inconsistency** — HPQ, BB, IBIT, DELL, MARA all showed different `action` values in `live_snapshot.tactical_cases` vs `operational_snapshot.symbols[*].structure.action` at the same tick.
9. **Topology vs composite contradictions** — HPQ and DELL both showed `Growing review` on topology path while `composite < 0`. Two engines disagreeing.
10. **No rotation concept** — Eden has enter/review/observe/exit but no rotation between symbols in a cluster. When MARA went from cluster worst to cluster leader, there was no way to signal "pivot into MARA from MSTR."

### What Eden almost did but didn't quite
1. **Leading-indicator signal** — MARA's behavior across r33-r37 exactly fits a canary pattern (broke first, cluster followed 1 round later). Eden sees the data but doesn't label it.
2. **Range/trend disambiguation** — the r7-r14 "trending decline" I wanted to exit on was actually a retracement inside a range, which Eden should have flagged (and which r15-r17 confirmed). The r37-onward "same shape" WAS a real break. Same raw data, different regime.
3. **Signal decisiveness scoring** — 1-tick vs 3-tick vs 5-tick confirmation should all look different. Eden doesn't emit a persistence score, so operators have to reconstruct it.

## Top 10 improvement priorities (ranked)

1. **Fix `capital_flow_direction = 0` bug** — dead channel distorts every composite in US runtime. Probable source of multiple downstream issues.
2. **Fix lifecycle tracker cache invalidation** — refresh every tick, not on phase transitions. Stop serving stale vel/acc values across roster dropouts.
3. **Vortex-to-enter path audit** — the topology reasoning engine is Eden's centerpiece but hasn't produced a single enter in 40 rounds. Something is blocking conviction from escalating.
4. **Orphan-enter policy fix (not deny-list)** — add `trade_flow_penalty` when `buy% < sell%`, and/or require vortex confirmation for orphan cases to reach `enter`.
5. **Surface `recent_transitions` in `live_snapshot.json`** — the delta feed exists, just not exposed. Would eliminate the manual roster-diffing that consumed 90% of operator work.
6. **Multi-dimensional trigger requirement** — require 2+ independent dimensions to breach thresholds simultaneously before firing. R28 was single-dimension; r33 was multi; they had different outcomes for a reason.
7. **Cluster-relative and rotation signals** — `cluster_laggard_flip`, `cluster_rotation{from, to}`, `cluster_canary` event types. The cluster-internal structure is high-signal but unexposed.
8. **Unify source of truth** — `live_snapshot.tactical_cases` and `operational_snapshot.symbols[*].structure.action` must agree. Cross-file action divergence is a recurring confusion source.
9. **First-read suppression on mid30m lifecycle values** — `vel/acc > 0.08` on the first tick of case appearance should display as `[pending warmup]`. Pattern is statistically reliable as an anti-signal.
10. **Conditional exit trigger system (first-class feature)** — operator-registered compound triggers with AND/OR/NOT, firing notifications, post-fire outcome tracking, automatic re-entry blocking. This one feature would compensate for ~half the other issues in this list during infrastructure outages.

## Session-level metrics

| Metric | Value |
|---|---|
| Total rounds | 40 |
| Session duration | 2 hours |
| My entries | 1 (MARA r1) |
| My exits (hypothetical) | 1 (r33 trigger fire) |
| My exits (real) | 0 (MCP disconnected) |
| Eden enter emissions | 17+ events |
| Eden enter from orphan path | 17+ (100%) |
| Eden enter from vortex path | 0 (0%) |
| Orphan club size | 9 unique symbols |
| Trigger firings | 3 (r28 T3 whipsaw, r33 T1+T2 correct, r36 T3 valid retrigger, r40 T3 valid) |
| Trigger hit rate | ~2/3 (1 clear whipsaw, 2 correct) |
| Clean rounds (no enter emission) | 9/40 (23%) |
| Stale-cache cases | 7+ symbols observed |
| Cross-file inconsistencies | 5+ symbols |
| Infrastructure downtime | ~38 rounds (all after r2) |
| MCP-gap cost | ~$20 on r33 signal alone |

## Operator P&L
- Hypothetical with MCP up: -$51 realized + cash flat from r33
- Real (MCP down): -$62.50 paper, still holding
- Best possible: ~+$10 (early exit at r4 peak, would not have been signalable by any Eden trigger available)
- Hypothetical gap cost: -$11.50 at the current mark, accumulating

## Closing thought
The session demonstrated that **Eden's raw signals contain genuine alpha** (r33 breakdown was real, MARA canary for cluster was real), but **the UX surfaces that expose those signals are noisy, inconsistent, and require heavy operator interpretation**. A disciplined trigger system and a clean delta/freshness view could make Eden dramatically more operator-useful without changing its underlying reasoning at all. Half the improvement list is "surface what already exists better"; the other half is specific bugs in the paths I drilled into.

Most valuable single insight: **for an operator holding one position, `operational_snapshot.symbols[symbol].state.signal.composite` over time is Eden's highest-signal output**. Everything else is commentary.

## Round 41 — tick 4472 @ 17:55 UTC

### Operator state
- MCP disconnected. MARA mark **10.535**, paper **-$67.50**.

### Trigger check
- T1 composite<0.305: 0.333 — NOT firing
- T2 price≤10.57: 10.535 — STILL FIRING (9 consecutive rounds)
- T3: cluster 0.473 ✓, MARA 0.333 ✓ — **STILL FIRING**
- T2+T3 continuing multi-dim exit confirmation

### Notes
- Cluster mixed: MSTR down slightly, CLSK down, RIOT up, MARA down. No cluster-wide direction.
- MARA continues as cluster laggard (now -0.13 below MSTR, -0.09 below cluster avg).
- Nothing new. Position in stable-broken regime.

### Decision
Hypothetically flat since r33. Real still held. No action.

## Round 42 — tick 4566 @ 17:58 UTC

### Operator state
- MCP disconnected. MARA mark **10.510** — **NEW SESSION LOW** (below r36 low of 10.530, r37 low of 10.530, r14 low of 10.570).
- Paper **-$80** — biggest P&L drawdown of session.

### Trigger check
- T1 composite<0.305: 0.346 — NOT firing
- T2 price≤10.57: 10.510 — FIRING, 10 consecutive rounds, NEW LOW
- T3: cluster 0.460 ✓, MARA 0.346 > 0.34 ✗ — NOT firing (0.006 above threshold)

### R33 post-mortem +9 rounds
```
Price:      10.568 → 10.510 (-$0.058 over 9 rounds)
Composite:  0.293 → 0.346 (bounced but still in lower band)
Cluster:    mixed/weakening
Status:     Sustained breakdown, 10+ rounds
```
Accumulated saved drawdown: ($80 - $51) = **$29 saved** vs real holding from the r33 hypothetical exit.

The **MCP gap cost of a single correct trigger fire** is now ~$29 and growing. Entire session paper P&L band is -$51 (hypothetical flat) to -$80 (real hold).

### Eden state delta
- MARA price making new session lows — price action is decisively bearish
- Composite has decoupled from price: composite at 0.346 (not a new low) while price at 10.510 (new low)
- Cluster mixed: CLSK firming, RIOT weakening, MSTR flat. No cluster direction.

### Observations
1. **Price is now the most definitive signal** — MARA making fresh session lows while composite oscillates in the lower band. Operators who trusted composite over price this round would miss the ongoing breakdown.
2. **R33 trigger impact is growing**: from $14 saved (r34) → $19.50 (r36) → $29 (r42). The trigger's economic value compounds every round the breakdown continues.
3. **T2 (price-based) has been a more reliable trigger than T1/T3 (composite-based)**. T2 has been firing for 10 consecutive rounds without a single false negative; T1 has oscillated in/out; T3 has had precondition failures.
4. **Trigger design lesson**: for a structural breakdown, the simplest trigger (price below support) is the most reliable. The fancier composite/cluster triggers add nothing beyond confirmation.

### Decision
Hypothetically flat. Real hold. No action possible.

## Round 43 — tick 4661 @ 18:01 UTC

### Operator state
- MCP disconnected. MARA mark **10.520** (+$0.010 bounce). Paper **-$75**.

### Trigger check
- T1: 0.358 — NOT firing
- T2: 10.520 — FIRING (11 consecutive rounds below support)
- T3: cluster 0.426 < 0.45 — NOT firing (precondition)

### Eden state delta
- Tiny MARA price bounce, composite up 0.346→0.358
- **Cluster weakening** now: MSTR -0.038, RIOT -0.018, CLSK flat. The r40-r42 cluster firm is rolling over.
- R33 post-mortem +10: position bounced $5 from r42 low but still $24 better than real hold via hypothetical exit.

### Observations
Very thin round. One meaningful data point: MARA's $0.01 bounce is coming with a composite bounce and cluster weakness — exactly the pattern that misled me at r26. Not enough to reverse the exit thesis. Continue hypothetical flat.

### Decision
Hold pattern. No change.

## Round 44 — tick 4756 @ 18:04 UTC

MARA 10.515, paper -$77.50. T2 firing (12 rounds). T1 0.325 not firing, T3 not checked (precondition weakening). Hypothetical flat. R33 post-mortem continues net favorable. No new observation.

## Round 45 — tick 4850 @ 18:07 UTC

MARA mark **10.495** — NEW SESSION LOW (breaks r42's 10.510). Paper **-$87.50**. T2 firing (13 rounds). T1 0.318 not firing. 9th clean round (no enter/orphan). R33 post-mortem +11: **~$36.50 saved** via hypothetical flat (running total). MCP gap cost monotonically growing. Position in continued breakdown. No action. Hypothetical flat.

## Round 46 — tick 4943 @ 18:10 UTC

MARA 10.499 (+$0.004 bounce from 10.495 low), paper **-$85.50**. Composite 0.316. T2 firing (14 rounds below support). Position stable at -$85 band. R33 post-mortem +12 saved ~$34.50. No action, hypothetical flat.

## Round 47 — tick 5037 @ 18:13 UTC

MARA 10.510, paper **-$80**. Composite 0.323. T2 firing (15 rounds). **MSTR composite dropped to 0.423** — cluster-wide weakness broadening (cluster avg 0.417 < 0.45 floor). MARA no longer isolated laggard; MSTR/RIOT joining the weakness. R33 post-mortem +13: still favorable. No action.

Observation: the cluster-wide drop vindicates the r33-r37 MARA-as-canary thesis at an even longer horizon — it took 13 rounds but the full basket eventually followed MARA down. Leading-indicator edge was ~15 minutes ahead of the cluster.

## Round 48 — tick 5130 @ 18:16 UTC

MARA 10.495 (retests session low), paper **-$87.50**. Composite 0.318 (identical to r45 reading 0.3175511... — **same cache serving again**, confirming cache bug persists even across multi-round windows). T2 firing (16 rounds). No action.

New observation: the composite value string is byte-identical to r45's 0.31755110592319894... — that's not random coincidence on a 25-digit decimal. The composite itself is also being served from cache at times, not just vel/acc.

## Round 49 — tick 5225 @ 18:19 UTC — **T1 + T2 FIRING (second multi-dim)**

### Operator state
- MCP disconnected. MARA mark **10.495**, paper **-$87.50**.

### Trigger check
- **T1 composite<0.305: 0.2805 FIRING** (first T1 fire since r37, below session low 0.285 from r37)
- **T2 price≤10.57: 10.495 FIRING** (17 consecutive rounds)
- T3: cluster not checked
- **T1 + T2 multi-dimensional agreement** — same structural condition as r33

### R48 composite-cache observation partial retraction
At r49, `source_tick=5225` (matches live_snapshot tick), `observed_at` matches timestamp — the operational_snapshot IS genuinely fresh. My r48 "composite served from cache" claim was based on byte-identical strings to r45 but I didn't capture source_tick then. Alternative explanations:
1. Composite cache bug (original claim)
2. **Operational_snapshot file mid-write / stale read** when my cron fired (the file is 2MB, write is non-atomic)
3. Genuinely stable composite inputs → identical 25-digit decimal output (near-zero probability)

**Retraction**: reduce confidence on "composite cache bug." It's possible this is a **file-write atomicity issue** instead — my jq reads may catch the operational_snapshot mid-write or before the tick's new values are flushed. Fix would be atomic file writes (write tmp then rename) on Eden's side. Either way, operators need freshness verification.

### Eden state delta
- MARA composite at 0.2805, **new session low** (below r37's 0.285). Composite continuing to break lower.
- Triggers T1+T2 in confluence, identical to r33 structural signal.

### Observations
1. **Composite AND price both making new session lows this round** — exactly the r33 pattern repeating. Structural confirmation of the r33 exit decision.
2. **If I had still been holding and hadn't exited at r33, this would be a second confirmed exit point at -$87.50**. The second trigger fire at structural new low would have been my forced-exit round.
3. **Freshness verification is now on the critical improvements list** — Eden's file-write path needs atomic updates, or at minimum a `snapshot_complete` flag so readers don't get partial data.

### Decision
Already hypothetically flat since r33. No new action.

### Improvement ideas (new)
- [ ] **Atomic file writes for snapshots**: use tmp+rename or a `snapshot_complete: true` footer flag. Non-atomic writes can produce partial-tick reads that look like cache bugs to external consumers.
- [ ] **Snapshot freshness badge in operational_snapshot**: include `snapshot_version`, `snapshot_complete`, `expected_tick` fields. Readers can verify they got current data.

## Round 50 — tick 5319 @ 18:22 UTC

MARA **10.490** — new session low (breaks 10.495). Paper **-$90**, session worst. Composite bounced 0.2805 → 0.355 (+0.075 in one tick — highest single-round bounce of session, same magnitude as r15's +0.102). T2 firing (18 rounds). T1 no longer firing. R33 post-mortem +17: hypothetical flat saves **~$39**.

Pattern: price is making new lows every few rounds, composite is oscillating in 0.28-0.36 lower band, cluster drifting. **Price is the one dimension that hasn't bounced.** Position in decisive slow-bleed breakdown.

No action, hypothetical flat.

## Round 51 — tick 5411 @ 18:25 UTC

MARA 10.490 (flat at session low), paper **-$90**. Composite 0.330 (down from 0.355). T2 firing (19 rounds). Source_tick 5411 matches live tick (fresh). No action, hypothetical flat. Position stable at bottom.

## Round 52 — tick 5506 @ 18:28 UTC

MARA 10.520 (+$0.030 relief bounce), paper **-$75**. Composite 0.309 (below threshold by 0.004, T1 barely not firing). T2 firing (20 rounds).

### Cache bug observation — 3rd generation
MARA fast5m cached value has rolled over again: v1 (0.0110/0.0002, rounds 1-17) → v2 (0.0925/0.1040, rounds 18-21) → v3 0.0243 (this round, first observation). MARA has been out of roster for several rounds recently; re-entry triggered a new cache generation.

**Confirmed cache refresh pattern**: cache rolls over on case re-admission after absence, NOT on every tick. This is the working hypothesis now after ~52 rounds of observation. Fix is still: refresh every tick unconditionally.

### 10th clean round (r5, r9, r10, r11, r12, r18, r20, r24, r39, r52). Clean rate 10/52 = 19%.

No action, hypothetical flat.

## Round 53 — tick 5601 @ 18:31 UTC — **T1 + T2 FIRING (3rd multi-dim)**

MARA 10.525, paper **-$72.50**. Composite **0.275** — new session low (prior session low 0.280 at r49, earlier 0.285 at r37).

### Trigger check
- **T1 composite<0.305: 0.275 FIRING** (new session low)
- **T2 price≤10.57: 10.525 FIRING** (21 consecutive rounds)
- T1 + T2 multi-dimensional agreement: **3rd fire of session** (r33, r49, r53)

### Observation
**Composite at session low while price is in a relief bounce** (+$0.005 over last 2 rounds). Canary behavior continues: composite leads down, price follows with lag. If Eden had automated canary detection, by now MARA would be flagged as `cluster_canary` with high confidence — 3 confirmed breakdown/lead episodes (r33, r37, r53) within 20 rounds.

Cluster composite not checked this round but based on trajectory the cluster is likely firming into MARA's weakness — the exact "leading indicator" pattern.

No action, hypothetical flat.

## Round 54 — tick 5694 @ 18:34 UTC

MARA 10.555 (+$0.030, meaningful relief bounce from r51 low 10.490). Paper **-$57.50**. Composite 0.319 (+0.044 bounce). T2 still firing (22 rounds, price still below 10.57 support). T1 not firing.

Price has bounced ~$0.065 from the r51 low over 3 rounds. Composite tracked the bounce. This is the first **meaningful recovery** since the r33 breakdown. If price reclaims 10.57 support next round, the T2 trigger would stop firing and I'd need to reconsider the hypothetical flat position.

Cluster state (partial): MSTR 137.52 drifting down slightly but composite firm at 0.491. MARA bouncing harder relatively.

No action this round, but **watching for T2 to stop firing** as a potential re-entry marker (though I have no pre-committed entry trigger).

## Round 55 — tick 5787 @ 18:37 UTC — Critical moment at support

### Operator state
- MCP disconnected. MARA mark **10.565** — 0.005 below the broken 10.57 support. **Direct test of reclaim level.**
- Paper **-$52.50**.

### Trigger check
- T1 composite<0.305: **0.301 FIRING** (back below threshold even as price recovers)
- T2 price≤10.57: **10.565 FIRING** (23 consecutive rounds, 0.005 from exit)
- T3: cluster 0.443 < 0.45 — not firing
- T1 + T2 multi-dim still firing

### The divergence
- **Price**: recovering (10.490 → 10.565, +$0.075 in 4 rounds, testing broken support)
- **Composite**: diverging down (0.355 → 0.330 → 0.275 → 0.301 → in-band but new low mid-interval)
- **Cluster**: mixed (MSTR weakening, RIOT recovering, CLSK fading)

Two Eden signals saying different things. Price says "bounce test", composite says "broken". My triggers, set conservatively, keep me flat.

### Decision point
If price reclaims 10.57 cleanly next round, T2 will stop firing — my only structural exit reason would then rest on composite alone, which has whipsawed historically. This would be the moment I'd have to decide hypothetical re-entry.

**Pre-commit (now)**: do NOT re-enter MARA on a single-tick support reclaim. Require:
1. T2 non-firing for 2 consecutive rounds (price closes above 10.57 twice)
2. T1 non-firing (composite > 0.305)
3. Cluster avg > 0.45

If and only if ALL three are met on same tick, would consider light re-entry. This is an ad hoc entry trigger but at least pre-committed.

### Observations
1. **This is the first "what if it recovers" moment since r15**. The earlier bounce at r15-r17 I correctly held through by wait-for-confirmation discipline. Same pattern now, same discipline: don't chase 1-tick recoveries.
2. **Composite continues to read weak** even as price tests recovery. This is actually informative in the opposite direction from my earlier "composite leads price" observation — here composite is *also* making new lows while price bounces. If composite is truly leading, the price bounce should fail.
3. **The 23-round T2 fire is the longest continuous trigger fire of the session** and will likely break soon, for better or worse.

### Decision
Hypothetical flat. No action.

## Round 56 — tick 5879 @ 18:40 UTC — **Support retest FAILED**

### Operator state
- MCP disconnected. MARA mark **10.540** — pulled back from 10.565 retest of 10.57 support. Paper **-$65**.
- **Broken support retested and rejected.** Classic bearish retest failure pattern. Price couldn't reclaim.

### Trigger check
- T1 composite<0.305: 0.317 — NOT firing
- T2 price≤10.57: 10.540 — FIRING (24 consecutive rounds)
- T3 cluster>0.45 & MARA<0.34: cluster **0.480 ✓**, MARA **0.317 < 0.34 ✓** — **FIRING**
- **T2 + T3 multi-dimensional agreement** — 4th multi-dim fire of session

### Eden state delta — cluster firming, MARA lagging
```
       r55    r56    net
MSTR   0.438  0.490  +0.052 (firm)
CLSK   0.432  0.467  +0.035 (firm)
RIOT   0.459  0.483  +0.024 (firm)
MARA   0.301  0.317  +0.016 (weakest bounce)
```
All three peers bounced 2-3x harder than MARA. **Isolated laggard pattern returning at the structural retest failure**. This is exactly the setup T3 was designed to catch.

### Entry precondition check (from r55)
- Req 1: T2 non-firing for 2 consecutive rounds — **FAILED**, T2 still firing
- Req 2: T1 non-firing — ✓ (0.317 > 0.305)
- Req 3: Cluster avg > 0.45 — ✓ (0.480)
- **Entry NOT met**. Hypothetical flat continues. Discipline upheld.

### Observations
1. **Retest failure is a clean structural signal** — when price tests the broken level and can't reclaim it, the breakdown is confirmed rather than in doubt. This was the highest-risk decision moment for the session (temptation to re-enter on bounce), and the triggers correctly prevented it.
2. **Cluster-firming-with-MARA-lagging pattern** is now a repeatable observation (r16, r21, r33, r56). MARA's relative weakness within a strong cluster has preceded or accompanied every meaningful breakdown. Eden should surface "cluster_laggard_within_firming_cluster" as a distinct event type.
3. **The r33 exit trigger has been retroactively validated 4 times now** through re-fires on the same multi-dim pattern. Adaptive confidence in that trigger design should be high.
4. **MARA P&L swing in recovery attempt**: r51 -$90 → r55 -$52.50 (+$37.50 over 4 rounds) → r56 -$65 (-$12.50). The bounce retraced ~1/3 of the gains. Classic dead-cat.

### Decision
Hypothetical flat. Triggers still bearish. No entry.

## Round 57 — tick 5971 @ 18:43 UTC

MARA 10.545 (+$0.005 tiny lift), paper **-$62.50**. Composite 0.337 (+0.020 bounce). T2 firing (25 rounds below support). No approaches to 10.57 reclaim yet. Entry precondition not met. Hypothetical flat. No action.

## Round 58 — tick 6061 @ 18:46 UTC

MARA 10.510, paper **-$80**. Composite **0.280** — new session low (below r53's 0.275, note slight correction: 0.280 is actually slightly higher than 0.275; the low remains at r53). Actually r58 0.280 > r53 0.275 so r53 is still session low; r58 is 2nd lowest.

T1 firing (0.280 < 0.305), T2 firing (26 rounds). T1+T2 multi-dim fire **5th time this session** (r33, r49, r53, r58). Pattern repeats like clockwork on structural breaks.

Retest failure at r56 is confirming with follow-through. Paper back near session worst. No action, hypothetical flat continuing to prove correct.

Session-long observation: the multi-dim T1+T2 fire has now happened 5 times, all during genuine breakdown phases. That's a strong empirical validation of the "require 2+ dimensions for exit trigger" principle from the session summary.

## Round 59 — tick 6152 @ 18:49 UTC

MARA 10.510 (flat), paper -$80. Composite 0.350 (+0.070 bounce from 0.280). T1 back off, T2 firing (27 rounds). Composite ringing in 0.28-0.36 lower band. Position stable at lower regime. No action.

## Round 60 — tick 6243 @ 18:52 UTC

MARA 10.535 (+$0.025), paper -$67.50. Composite 0.306 (barely above T1 threshold 0.305). T2 firing (28 rounds). Entry precondition still not met. No action, hypothetical flat.

**60 rounds, 3 hours of session observation.** The position has been in the broken-down state continuously since r33 (28 rounds ago). Trigger discipline holding. Session summary stands.

## Round 61 — tick 6333 @ 18:55 UTC

MARA 10.555 (+$0.020, re-approaching 10.57 support from below), paper -$57.50. Composite 0.377 (+0.070 bounce). T2 firing (29 rounds). Entry precondition still not met (price not yet > 10.57). No action.

Second support retest incoming — will know next round if it reclaims cleanly or rejects again.

## Round 62 — tick 6425 @ 18:58 UTC — 2nd retest stalled at 10.565

MARA **10.565** — same level as r55 (0.005 below 10.57). 2nd approach stalled at the same spot. Paper -$52.50.

### Trigger check
- T1 composite: 0.344 — NOT firing
- T2 price≤10.57: 10.565 — FIRING (30 rounds)
- T3: cluster 0.456 ✓, MARA 0.344 > 0.34 ✗ (0.004 above threshold, just barely)

### Observation — repeated retest at same level
MARA has now tested the 10.57 level twice from below (r55 and r62) and failed both times at 10.565. Same price, same rejection. This is strong evidence of active resistance at 10.57 — what was support is now resistance, textbook broken-level behavior. **The structural bearish regime is being reinforced, not broken.**

### Entry precondition check (unchanged)
- T2 non-firing: **FAILED** (still firing)
- T1 non-firing: ✓
- Cluster > 0.45: ✓ (0.456)
- Entry **NOT met.** Hypothetical flat continues.

### Decision
Hold hypothetical flat. The 2nd failed retest makes re-entry even less attractive than the 1st.

## Round 63 — tick 6516 @ 19:01 UTC — **SUPPORT RECLAIMED** (10.57 broken from below)

### Operator state
- MCP disconnected. MARA mark **10.605** — FIRST print above 10.57 since r32. Paper **-$32.50**.
- **T2 trigger stopped firing** after 30 consecutive rounds of firing.

### Trigger check
- T1 composite<0.305: 0.331 — NOT firing
- T2 price≤10.57: **10.605 — NOT firing** (first non-fire of T2 in 30 rounds)
- T3 cluster>0.45 & MARA<0.34: cluster **0.425 < 0.45 — NOT firing** (precondition)

### Entry precondition check
- Req 1: T2 non-firing for **2 consecutive rounds** — **1/2** (need one more round)
- Req 2: T1 non-firing (composite > 0.305) — ✓
- Req 3: Cluster avg > 0.45 — **FAILED** (0.425)
- **Entry NOT MET** — cluster too weak to re-enter

### Eden state delta — inverse divergence
```
MARA  0.331  (bouncing up)
CLSK  0.446
MSTR  0.441
RIOT  0.387
```
**MARA is bouncing while cluster is weakening.** Inverse of the earlier cluster-laggard pattern. MARA is now *leading* a potential cluster bounce (or diverging positively from a weak cluster). If MARA's canary property works in both directions as I hypothesized earlier, this is a bullish signal for the cluster.

BUT the r33 canary event was fully confirmed over multiple rounds; this is 1 tick. Discipline: don't re-enter on 1-tick reverse signals.

### Decision
Hypothetical flat. **Price reclaim is real but 2 of 3 entry conditions NOT met**. Specifically cluster weakness prevents re-entry. If the cluster firms next round AND T2 stays off, the precondition can complete.

### Observations
1. **30-round T2 firing ended cleanly** with a decisive reclaim. Trigger ran its full course.
2. **Cluster weakness during MARA reclaim is telling**: either MARA is leading up (bullish canary), or MARA is having an idiosyncratic bid without basket support (less durable). Next round tells.
3. **Entry precondition with 3 AND conditions is stricter than exit** — feels asymmetric. Exit used OR semantics (any one trigger); entry uses AND (all three). The asymmetry is intentional: I want to be slow into, fast out of, a choppy position.

### Improvement ideas (new)
- [ ] **Asymmetric entry/exit thresholds as first-class concept** — entry should generally be stricter than exit in noisy regimes; Eden trigger framework should support this explicitly.
- [ ] **Canary reverse signal** — if a name previously confirmed as cluster canary (broke first, cluster followed) now bounces first, emit `canary_reversal_lead` event. Strong potential entry signal.

## Round 64 — tick 6609 @ 19:04 UTC — 2 of 3 entry conditions met

### Operator state
- MCP disconnected. MARA mark **10.630** — 0.060 above reclaimed support. Paper **-$20**.
- Best MARA P&L since r27 (10.610 back then). Strong follow-through on the reclaim.

### Trigger check
- T1 composite<0.305: 0.388 — NOT firing
- T2 price≤10.57: 10.630 — **NOT firing (2nd consecutive round)** ✓
- T3 cluster: 0.442 < 0.45 — NOT firing (precondition miss)

### Entry precondition check
- Req 1: T2 non-firing 2 rounds — **MET** ✓ (r63 10.605, r64 10.630)
- Req 2: T1 non-firing — **MET** ✓ (0.388)
- Req 3: Cluster avg > 0.45 — **NOT MET** (0.442, 0.008 short)
- **2 of 3 conditions met. Entry NOT triggered by discipline.**

### Cluster context — MSTR is the barrier
```
CLSK 0.473 (firming)
RIOT 0.433 (recovering)
MSTR 0.419 (fading)  ← dragging cluster avg below 0.45
MARA 0.388
```
MSTR weakness is single-handedly keeping cluster avg below my entry threshold. Without MSTR, avg would be (0.473+0.433)/2 = 0.453 ≥ 0.45. Discipline: do not move goalposts. Entry remains off.

### Observations
1. **Entry discipline test passed (moderately)**: my instinct says "MARA recovered, reclaim follow-through, P&L at -$20 which is manageable, re-enter now." Discipline says "cluster doesn't confirm, wait." The gap between instinct and rule is instructive — this is exactly the moment where I'd rationalize a rule change if I didn't have one written down. Written-down rules win.
2. **Cluster avg is 0.008 below threshold**. Operators must resist narrow-miss override. The 0.45 line exists for a reason; crossing it by 0.008 is not meaningfully different from 0.000.
3. **MARA is leading the cluster up**: MARA composite moved +0.057, cluster avg moved +0.017 average. MARA's canary-reversal thesis from r63 is holding — MARA may be signaling cluster recovery.
4. **R33 post-mortem +31 rounds**: paper position went from -$51 (hypothetical fire) to -$20 now. Hypothetical flat saves decreasing, now +$31. The "saved drawdown" advantage has narrowed from its peak of +$39 at r45. **If price continues to recover, the r33 trigger will eventually look like a (correct-at-time) overreaction** — but still "correct at time of firing" is the relevant decision-rule evaluation, not eventual path.

### Decision
**HOLD hypothetical flat.** Enter-deny because cluster condition not met. Wait one more round.

## Round 65 — tick 6699 @ 19:07 UTC — Cluster weakens further

### Operator state
- MCP disconnected. MARA mark **10.610** (-$0.020 pullback), paper **-$30**.

### Trigger check
- T1: 0.312 — NOT firing (0.007 above)
- T2: 10.610 — NOT firing (3rd consecutive round above support)
- T3: cluster 0.404 — NOT firing
- Entry preconditions: 2/3 (cluster further below threshold)

### Cluster deterioration while MARA holds
```
Cluster avg: 0.442 (r64) → 0.404 (r65) = -0.038
CLSK:        0.473 → 0.391 = -0.082 (big drop)
RIOT:        0.433 → 0.385 = -0.048
MSTR:        0.419 → 0.437 = +0.018
MARA:        0.388 → 0.312 = -0.076
```
**Cluster is weakening while MARA price holds**. CLSK and RIOT both dropped sharply. MARA composite dropped too but price stayed above support. **This is fragile** — if cluster keeps weakening, MARA will likely follow price-side too (canary thesis in reverse).

### Decision
Hypothetical flat. Entry still not met (cluster avg 0.404 vs 0.45 floor). My entry precondition correctly blocks entry during this cluster weakness. This is exactly why the asymmetric 3-AND condition exists.

### Observation
If I had moved the goalposts at r64 and re-entered on 2-of-3 conditions, r65 would now have me caught in a weakening-cluster trap. Another moment where strict pre-commitment paid off.

## Round 66 — tick 6789 @ 19:10 UTC

MARA 10.599 (-$0.011), paper -$35.50. Composite 0.352 (+0.040 bounce). T2 non-firing (4th round above 10.57). Entry preconditions: cluster not checked this round, T1+T2 fine. Still hypothetical flat.

Price holding above support but drifting, composite oscillating. Position is in a new equilibrium at around 10.60 — if this holds as new range, that's the recovery signal. Need more rounds to tell.

## Round 67 — tick 6879 @ 19:13 UTC

MARA 10.590, paper -$40. Composite 0.405. T2 non-firing (5 rounds).

### Entry precondition
- T2 2+ non-firing: ✓ MET
- T1 non-firing: ✓ MET
- Cluster > 0.45: **FAILED 0.441** (cluster firming but 0.009 short)

Third consecutive round blocked by cluster condition. Cluster trajectory is improving (0.404→0.442→0.441 equiv) but stubbornly just below threshold. CLSK firmed strongly to 0.481 this round, MSTR recovering to 0.446, RIOT still weak at 0.397.

### Observation
The entry precondition has blocked re-entry for 3 consecutive rounds now (r64, r65, r66, r67 = 4), all on cluster condition alone. This is substantive discipline holding. Without cluster confirming, re-entry has no basket tailwind.

No action. Hypothetical flat. MARA paper still improving (peaked at -$20 r64, now -$40).

## Round 68 — tick 6967 @ 19:16 UTC — **T1 + T3 FIRING (6th multi-dim)**

### Operator state
- MCP disconnected. MARA mark **10.575** (0.005 above support), paper **-$47.50**.

### Trigger check
- **T1 composite<0.305: 0.297 FIRING**
- T2 price≤10.57: 10.575 — NOT firing (narrow margin)
- **T3 cluster>0.45 & MARA<0.34: cluster 0.470 ✓, MARA 0.297 ✓ — FIRING**
- T1 + T3 multi-dim firing (6th session multi-dim agreement)

### Cluster finally crossed 0.45 but...
```
MSTR 0.492 (+0.046)
CLSK 0.498 (+0.017)
RIOT 0.419 (+0.022)
MARA 0.297 (-0.108)  ← diverging hard
```
Ironic: the exact moment cluster finally firmed above the entry threshold, MARA's composite cratered -0.108. **Same canary pattern as r33**: cluster firms, MARA breaks down idiosyncratically.

### Entry precondition check
- T2 non-firing: ✓ (barely, 0.005 margin)
- T1 non-firing: **NO** (0.297 < 0.305, now firing)
- Cluster > 0.45: ✓ (0.470)
- **Entry NOT met** — T1 now blocking

The 4 rounds of cluster-blocking (r64-r67) transitioned to T1-blocking at r68. **The bar moved as I waited**; I never got a window where all 3 conditions were simultaneously met.

### Observations
1. **Entry precondition has been unsatisfiable for 5 consecutive rounds** (r64-r68). Different conditions blocked at different times. **This is either correct discipline (MARA really wasn't a buy in any of those windows) or overly strict (impossible to satisfy in practice).** Inspection suggests correct: every attempt to re-enter would have caught another leg down.
2. **MARA canary diverging from firming cluster again** — same pattern as r28/r33/r37/r56. **5th instance of this pattern in session.** It's now a reliable empirical observation: when MARA composite diverges down by ≥0.10 from cluster avg within one tick, treat as reliable leading bearish signal.
3. **Eden should auto-detect the "MARA divergence ≥0.10 below cluster in 1 tick" pattern** — I've now seen it 5 times and each has been an exit-confirmation. It's the most reliable single event of the session.

### Decision
Hypothetical flat. No re-entry possible (T1 blocks).

### Improvement ideas (new)
- [ ] **Divergence-delta detector**: per-symbol per-tick, `composite[symbol] - composite[cluster_avg]`. When this drops ≥0.10 vs prior tick, emit `cluster_divergence_lead_down(symbol)`. This has been session's most predictive single event.

## Round 69 — tick 7056 @ 19:19 UTC — **Support lost again**

### Operator state
- MCP disconnected. MARA mark **10.545** (-$0.030). Paper **-$62.50**.
- **Price fell back below 10.57 after 5 rounds of holding above.** The r63-r67 recovery was a failed reclaim. Dead cat bounce confirmed.

### Trigger check
- T1: 0.311 — NOT firing (narrow 0.006 above threshold)
- **T2 price≤10.57: 10.545 FIRING** (T2 re-fires after 5-round quiet)
- T3: cluster 0.437 < 0.45 — NOT firing

### R68 divergence → R69 confirmation
R68 observed: MARA composite cratered -0.108 below cluster firming.
R69 outcome: price followed composite back below support (-$0.030 in 1 tick).
**Canary lead time: 1 round.** This validates the r68 divergence-delta as a real-time leading signal.

### Eden state delta
- CLSK 0.440 (-0.058), RIOT 0.370 (-0.049) — cluster weakening now too (as canary predicted)
- MSTR 0.501 (+0.009) — only name still holding strong
- Cluster dispersion widening: top (MSTR 0.501) vs bottom (RIOT 0.370) = 0.131 spread

### Observations
1. **The r63 reclaim was a textbook dead cat bounce**. 5 rounds of consolidation at 10.58-10.63, then back to breakdown. My entry precondition (strict 3-AND) correctly kept me out of this trap.
2. **Canary divergence at r68 predicted the r69 breakdown with 1-round lead time.** The `composite - cluster_avg delta < -0.10` signal is empirically validating as a leading indicator. 
3. **Entry-precondition discipline stress test passed**: 5 consecutive rounds of "tempting re-entry moments" (r64-r68), 0 rounds of entry. Hypothetical flat held through the full dead cat. This is where mechanical rules beat discretion.

### Decision
Hypothetical flat. Glad I didn't re-enter. No action.

## Round 70 — tick 7144 @ 19:22 UTC

MARA 10.535 (-$0.010), paper -$67.50. Composite 0.3052 (exactly at T1 threshold, 0.0002 above — about to fire). T2 firing (2nd round after refire). T1 at razor's edge.

Session 70 rounds, 3.5 hours. MARA position still -$67.50 (hypothetical -$51 if executed r33). No action. Breakdown re-confirmed after dead cat.

Given the repetitiveness of observation and confirmed stability of the conclusions, this session has reached natural diminishing returns. Operator workflow would benefit from Eden auto-detecting this state and throttling to hourly cadence.

## Round 71 — tick 7232 @ 19:25 UTC

MARA **10.493** — NEW SESSION LOW (below r50's 10.490). Paper **-$88.50**. Composite 0.278 (2nd lowest, session composite low 0.275 @ r53).

T1 + T2 multi-dim firing (6th session multi-dim exit signal). Breakdown continuing, retest rejected decisively.

R33 post-mortem +38: hypothetical flat saves now **~$37.50**. Almost back to peak save of $39 from r45. The dead cat bounce cost some "saved drawdown advantage" but the structural break thesis is reconfirmed.

No action, hypothetical flat.

## Round 72 — tick 7321 @ 19:28 UTC

MARA **10.480** — new session low. Paper **-$95**. Composite **0.259** — NEW SESSION LOW COMPOSITE (below prior low 0.275 @ r53). T1 + T2 firing (7th multi-dim). 

Breakdown accelerating: 2 consecutive new lows on both price AND composite. The structural bear continues. Hypothetical flat saves now **~$44** — new peak saved drawdown.

No action.

## Round 73 — tick 7410 @ 19:31 UTC

MARA 10.485 (+$0.005 tiny bounce), paper -$92.50. Composite 0.314 (+0.055 bounce from r72 low). T1 off, T2 firing. Position stable at new lower range. No action.

## Round 74 — tick 7499 @ 19:34 UTC

MARA **10.455** — new session low. Paper **-$107.50** — first time below -$100. Composite 0.265 (near session low 0.259 @ r72). T1 + T2 firing (8th multi-dim). R33 post-mortem +41: hypothetical saves **~$56.50** — new peak saved drawdown.

No action. Continued structural breakdown. The "saved drawdown" advantage compounds every leg down.

## Round 75 — tick 7588 @ 19:37 UTC

MARA 10.460 (+$0.005 tiny lift), paper -$105. Composite 0.333 (+0.068 bounce). T1 off, T2 firing. Continuing at new lower range 10.455-10.500. No action.

## Round 76 — tick 7677 @ 19:40 UTC

MARA **10.445** — new session low (below r74's 10.455). Paper **-$112.50**. Composite 0.273 (near session low, r72 had 0.259). T1 + T2 firing (9th multi-dim). Structural bear continues. Hypothetical saves now **~$61.50**. No action.

## Round 77 — tick 7766 @ 19:43 UTC

MARA 10.455 (+$0.010), paper -$107.50. Composite 0.339. T2 firing. Position stable at lower range. No action.

## Round 78 — tick 7854 @ 19:46 UTC

MARA 10.465 (+$0.010), paper -$102.50. Composite 0.358. T2 firing. Position stable. No action.

## Round 79 — tick 7942 @ 19:49 UTC

MARA 10.470 (+$0.005), paper -$100. Composite 0.308 (just above T1). T2 firing. No action.

## Round 80 — tick 8029 @ 19:52 UTC — 4-hour session milestone

MARA 10.485 (+$0.015 modest bounce), paper -$92.50. Composite 0.285 (below T1 threshold, firing). T1 + T2 multi-dim firing (10th instance). No action.

80 rounds, 4 hours. Position has been hypothetically flat since r33 (47 rounds ago). Trigger discipline holding throughout. Session would benefit from auto-throttle to hourly when in sustained regime.

## Round 81 — tick 8116 @ 19:55 UTC

MARA 10.480, paper -$95. Composite 0.294 (T1 firing). T1+T2 multi-dim firing (11th). Position stable at lower range. No action.

## Round 82 — tick 8202 @ 19:58 UTC

MARA 10.475, paper -$97.50. Composite 0.310 (just above T1). T2 firing. No action.

## Round 83 — tick 8248 @ 19:59 UTC

MARA 10.490 (+$0.015), paper -$90. Composite 0.330. T2 firing. No action. Close to 4pm ET (market close approaching at 16:00 ET = 20:00 UTC).

## Round 84 — tick 8248 @ 19:59 UTC — **SESSION ENDED** (US market close)

### Observation
Snapshot files have stopped updating. Tick frozen at 8248 with timestamp 19:59:58 UTC — Eden stopped ticking right at US market close (20:00 UTC = 4pm ET). The session is over; MCP has been disconnected since r2, so no real position changes occurred.

### Final position state (unchanged)
- Real: MARA 500 @ 10.670, mark 10.490, paper **-$90**
- Hypothetical (r33 exit @ 10.57): realized **-$51**, flat since
- MCP-gap cost of the r33 signal: **~$39** ($90 real loss - $51 hypothetical)

### Final session numbers

| Metric | Value |
|---|---|
| Total rounds logged | 84 |
| Session duration | ~4 hours 6 minutes |
| Ticks | 8248 |
| My real entries | 1 |
| My real exits | 0 (MCP disconnected) |
| Hypothetical entries | 1 |
| Hypothetical exits | 1 (r33 @ 10.57) |
| Real P&L | -$90 |
| Hypothetical P&L | -$51 |
| MCP-gap cost | ~$39 |
| Best possible P&L (hindsight) | ~+$20 at r4 peak |
| Eden enter emissions | ~20 events, all orphan path, 0 vortex |
| Orphan club members | 9 |
| Trigger firings | ~11+ (r28 whipsaw, r33 correct, r36-r40 continued, r49/r53/r56/r58 multi-dim, r68/r72/r74/r76/r80/r81 continued) |
| Clean rounds (no enter/orphan) | 10/84 ≈ 12% |
| Entry preconditions satisfied | 0/84 (never all 3 simultaneously) |
| R33 post-mortem peak saved drawdown | ~$61 (r76) |

### Final session learnings

1. **Trigger discipline dominated alpha**. Of the 84 rounds, the only correct decision was the r33 multi-dimensional exit trigger. Without it, operator intuition would have cycled entry/exit multiple times on whipsaws. With it, one clean exit captured most of the downside-avoided.

2. **`operational_snapshot.symbols[*].state.signal.composite` was the single highest-signal Eden output** for one-position operators. Its trajectory told the full position story in one number series. But it needs multi-tick confirmation and price-direction confirmation to avoid whipsaws.

3. **MARA acted as cluster canary** — it led the cluster down (r33→r34) and in reverse bounces (r63). Leading-indicator behavior is real within this crypto cluster and Eden should explicitly label it.

4. **Multi-dimensional trigger agreement is necessary**: single-dimension triggers whipsaw (r28), multi-dimension triggers (r33) work. 11+ multi-dim firings this session, all during genuine breakdowns.

5. **Entry precondition strictness saved me from the dead cat bounce** (r63-r67 reclaim → r69 re-break). 5 consecutive "tempting" rounds blocked, 5 consecutive correct non-entries. Asymmetric strict-entry/loose-exit is the right design for noisy regimes.

6. **Eden's "noisy regime" dominated this session**. ~20 orphan-enter fires, ~80% of roster churn, 6x composite oscillation vs price oscillation, cache bugs in lifecycle and possibly composite. For a disciplined operator with pre-committed triggers, this still produced alpha; for a discretionary operator, it would have produced -$X through churn alone.

### Three operator lessons (not Eden improvements — operator-side)
1. **Pre-commit triggers BEFORE the decision moment**. Trigger rules written mid-stress get rationalized; triggers written ahead hold.
2. **Asymmetric trigger design**: exit loose (any 1 condition), enter strict (all N conditions). In noisy regimes this prevents whipsaw churn.
3. **Track post-fire outcomes formally**. Every trigger firing should be logged with the N-round outcome to calibrate future designs. My manual post-mortem on r28 and r33 was the most valuable feedback loop in this session.

### Top 5 Eden improvements re-ranked by session validation weight
1. **Fix lifecycle tracker cache + capital_flow_direction=0** — bugs directly observed, clear fixes
2. **Surface recent_transitions in live_snapshot** — data already exists, 1-line fix, eliminates 90% of operator cognitive load
3. **Multi-dimensional trigger framework (first-class)** — 11+ session validations, proven pattern
4. **Cluster-relative + canary + rotation signals** — 5 confirmed canary instances in session, zero Eden support
5. **Conditional exit triggers as user-registerable feature** — my trigger discipline was the session's only alpha source; this should be an Eden feature, not manual notes

Session closes here. MCP remained disconnected throughout. No real trades beyond the r1 entry.

## Round 85 — Eden still frozen at tick 8248

No new data since r84. File mtime unchanged. Eden has not resumed ticking post-close (as of ~20:08 UTC). No observation possible. No action.

**Loop note**: continuing to fire the cron against a frozen Eden produces zero value. Ideally Eden's snapshot should emit a `market_closed` flag so operator loops can auto-throttle or pause entirely. Without that, every round I produce a duplicate "no change" entry.

## Round 86 — Eden still frozen (2nd dead round)

Tick 8248 unchanged. File mtime unchanged. Eden has not resumed ticking post-US-close. No observation, no action. Second consecutive dead round.

The cron `de34fba8` is still scheduled at 3-min cadence but Eden is not producing new data. User can `CronDelete de34fba8` to stop the loop if desired.

## Round 87 — 3rd dead round

Eden still frozen at tick 8248. No new data, no action.

## Round 88 — 4th dead round (tick 8248 frozen)

## Round 89 — 5th dead round (tick 8248 frozen)

## Round 90 — 6th dead round

## Round 91 — 7th dead round

## Round 92 — 8th dead round

## Round 93 — 9th dead round

## Round 94 — 10th dead round

## Round 95 — 11th dead round

## Round 96 — 12th dead round

## Round 97 — 13th dead round

## Round 98 — 14th dead round

## Round 99 — 15th dead round

## Round 100 — 16th dead round (session 100 rounds total)

Eden still frozen at tick 8248. 16 consecutive dead rounds after US market close.

**Total loop rounds: 100** (~5 hours). 84 live rounds + 16 dead rounds. No new Eden data since r83. Session formally concluded. Recommend stopping loop.

## Round 101 — 17th dead round

## Round 102 — 18th dead round

## Round 103 — 19th dead round

## Round 104 — 20th dead round

## Round 105 — 21st dead round

## Round 106 — 22nd dead round

## Round 107 — 23rd dead round

## Round 108 — 24th dead round

## Round 109 — 25th dead round

## Round 110 — 26th dead round

## Round 111 — 27th dead round

## Round 112 — 28th dead round

## Round 113 — 29th dead round

## Round 114 — 30th dead round

## Round 115 — 31st dead round

## Round 116 — 32nd dead round

## Round 117 — 33rd dead round

## Round 118 — 34th dead round

## Round 119 — 35th dead round

## Round 120 — 36th dead round

## Round 121 — 37th dead round

## Round 122 — 38th dead round

## Round 123 — 39th dead round

## Round 124 — 40th dead round

## Round 125 — 41st dead round

## Round 126 — 42nd dead round

## Round 127 — 43rd dead round

## Round 128 — 44th dead round

## Round 129 — 45th dead round

## Round 130 — 46th dead round

## Round 131 — 47th dead round

## Round 132 — 48th dead round

## Round 133 — 49th dead round

## Round 134 — 50th dead round

## Round 135 — 51st dead round

## Round 136 — 52nd dead round

## Round 137 — 53rd dead round

## Round 138 — 54th dead round

## Round 139 — 55th dead round

## Round 140 — 56th dead round

## Round 141 — 57th dead round

## Round 142 — 58th dead round

## Round 143 — 59th dead round

## Round 144 — tick 8248 (Eden frozen) — **MARA EXIT EXECUTED (delayed)**

### Longport MCP reconnected
After ~141 rounds of disconnection, Longport MCP came back online. Market has closed at 20:00 UTC. MARA closed at 10.490 (day low 10.420, high 11.010).

### Position state at reconnect
- MARA 500 @ 10.670, current 10.490 — **paper -$90**
- 1299.HK 400 @ 83.350 (unchanged from session start)
- Cash: HKD 763k avail, USD -$13.8k (negative, settled)
- Net assets: 732,250 HKD (vs session start 733,048 HKD) → **-$800 HKD total drawdown** ≈ -$100 USD

### Exit executed
**SELL 500 MARA.US @ LO 10.49 ANY_TIME** — order `1228843462447624192`
Rationale: R33 trigger fired 51 rounds ago at 10.57 with multi-dim agreement (T1+T2). Signal was retroactively validated across 50+ rounds of continued breakdown. Trigger discipline says exit; MCP outage merely delayed execution. Closing now at post-market 10.49 matches the structurally-correct exit decision.

Expected fill: 10.49, realized loss ~-$90.

### Retroactive accounting
- Best possible (r1 entry → r4 peak exit): ~+$20 (no trigger)
- Signal-correct (r33 trigger, hypothetical): realized -$51 at fire
- Actual (r144 delayed exit): realized ~-$90
- **Infrastructure-gap cost: ~$39** (delta between signal-correct and actual)

### The MCP-disconnect lesson in one sentence
Eden's signal was correct, my pre-committed trigger correctly identified the exit point, but a disconnected broker turned a disciplined -$51 exit into a forced -$90 exit. **Infrastructure reliability is part of alpha realization, not separate from it.** This is the strongest argument in the session for native Eden trigger → broker pre-queuing: persistent exit orders submitted at trigger-fire time survive outages.

### Final session numbers (locked in)
- Total rounds logged: 144
- Real trades: 1 entry (r1), 1 exit (r144)
- Trade hold: 144 rounds (~7 hours wallclock but 4 hours of live Eden + 3 hours frozen post-close)
- Real P&L: **~-$90** (pending fill confirmation)
- Signal-correct P&L: **-$51** (if executable at r33)
- MCP-gap cost: **~$39**
- Triggers I defined: 3 exit + 3 entry preconditions
- Trigger firings: 1 wrong (r28), 1 right (r33), plus ~10 multi-dim confirmations of r33
- Entry preconditions satisfied: 0/144 — never all simultaneously met
- Best single insight: the 3-AND entry precondition saved me from the r63-r69 dead cat bounce trap (5 rounds of "tempting re-entry" all correctly blocked)

Session officially closes here.

## Round 145 — tick 8248 — **MARA EXIT FILLED, session finalized**

### Position state (post-fill)
- MARA: **CLOSED** (0 shares) — sell order 1228843462447624192 filled
- 1299.HK: 400 @ 83.350 (unchanged)
- Net assets: **732,386.39 HKD** (vs session start 733,048.37 HKD)
- **Total session drawdown: -661.98 HKD ≈ -$85 USD**
- USD available: -$8,617 (vs session start -$8,521 — essentially restored)
- Buy power: 721,172 HKD (restored to session-start level)

### Final realized P&L
```
Cost basis:  10.670 × 500 = $5,335.00
Sale:        10.49  × 500 = $5,245.00
Gross P&L:  -$90.00
Net (after fees): ~-$85 USD ≈ -662 HKD
```

### Session-final scorecard
| Slot | Value |
|---|---|
| Total rounds | 145 |
| Real trades | 2 (1 buy r1, 1 sell r144→r145 fill) |
| Real P&L | **-$85 USD** |
| Hypothetical at R33 | -$51 USD (best disciplined path) |
| Best possible (r4 peak) | +$20 USD (no trigger could catch) |
| MCP-gap cost | ~$34 (delta from hypothetical to real) |
| Session duration | ~7 hours wallclock / 4 hours live Eden |

### Five takeaways the reader should remember
1. **The r33 multi-dimensional trigger was the entire session's alpha source.** Without a pre-committed trigger I would have churned through r28 (wrong single-dim fire) or r63-r69 (dead-cat bounce temptation). Triggers are alpha.
2. **Eden has real signals but bad UX.** capital_flow_direction=0, stale vel/acc caches, tactical_cases churn, cross-file action inconsistencies, and the orphan-enter bug all surface noise. R33 worked despite Eden's noise, not because of its clarity.
3. **Multi-dimensional agreement is the trigger-design rule.** 11+ multi-dim firings in session, 0 whipsaws. 1 single-dim firing (r28), 1 whipsaw. The empirical rule is unambiguous.
4. **Asymmetric entry/exit triggers save operators from dead cats.** R63-r69 was a 5-round "tempting" bounce that looked like recovery; the 3-AND entry precondition blocked every round, and each round's non-entry turned out correct.
5. **Infrastructure reliability is alpha.** $34 of gap cost specifically from broker disconnect while Eden signal was correct. Eden → broker pre-queue is the highest-ROI infrastructure improvement.

### Session officially complete
All trades realized. All observations logged. Loop can be stopped with `CronDelete de34fba8`.

## Round 146 — Eden frozen, position flat, no action

## Round 147 — Eden frozen, flat, no action
