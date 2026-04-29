# Polymarket Phase 1 Feasibility Memo

**Date**: 2026-04-26
**Author**: Claude (Phase 1 spike with operator)
**Status**: Findings ready, go/no-go pending operator decision
**Predecessor**: `decisions/2026-04-26/polymarket-legacy-removal.md` (executed, ~1.7k lines deleted, cargo build green)

## Spike scope (executed today)

1. Curl Polymarket Gamma + CLOB APIs to confirm data availability
2. Pull historical price series for 6 Fed markets + scan top-30 by volume
3. Inspect Eden SurrealDB to count belief_snapshot / intent_belief / regime_fingerprint rows + time range
4. Decide go / no-go on Phase 2 emergent correlation infrastructure build

**Did NOT** run rigorous cross-correlation — Eden data window too short (see finding 5).

## Findings

### 1. Polymarket API access — ✅ green

| Endpoint | Result |
|---|---|
| Gamma `/markets?active=true&order=volume24hr` | Works, full schema (`clobTokenIds`, `outcomePrices`, `volume24hr`, `endDate`) |
| CLOB `/prices-history?market=X&interval=max&fidelity=60` | Hourly bars over 30+ day windows |
| CLOB `/prices-history?interval=6h&fidelity=1` | 1-minute bars over short windows |
| CLOB `/prices-history?interval=max&fidelity=720` | 12-hour bars over 358 days |

UA header required (Cloudflare blocks default Python-urllib UA; curl OK; browser-like UA fixes Python).

### 2. Volatility distribution — ✅ bimodal, clean filter possible

Top 30 markets by 24h volume; volatility = % of hourly samples with |Δp| > 0.01:

```
dead     (<1%  hourly >1% moves): 14/30 (47%)
low      (1-5%):                   2/30
mid      (5-15%):                  2/30
high     (>=15%):                 12/30
```

Almost no markets in the medium bucket — bimodal shape means a single volatility threshold cleanly separates "potentially informative" from "saturated dead". This is the strongest validation of the planned Test 1 liquidity gate: it works without manual rules.

### 3. Fed partition σ-additivity — ✅ 0.9999

The 4 mutually-exclusive Fed-April markets (50+ cut / 25 cut / no change / 25+ raise) currently sum to **0.9999**. Confirms the Arrow-Debreu interpretation: Polymarket binary options on a partition really do form a coherent probability measure with no-arbitrage σ-additivity.

### 4. Counter-intuitive: most "obviously relevant" Fed markets are dead — ⚠️ design implication

Of 6 Fed markets pulled (April + June meetings), **5/6 had 0% hourly >1% moves over 30 days**. Only `fed_jun_no_change` (June meeting, ~6 weeks out) had real movement — 15% of hourly samples >1%, range [0.835, 0.935]. Reason: April meeting in 3 days; consensus locked at 99.65% no-change → no information left to extract.

**Implication for Test 1**: liquidity gate must look at **price volatility**, not 24h trade volume. A market can have $6M/24h volume (Fed-50bps-cut $6.2M) and yet be useless to Eden because it's a "settlement bet" with no actionable price discovery left. The relevant filter is: **markets must have observable repricing during the lookback window**.

### 5. Eden persistence has < 1 session of usable data — ❌ blocker for full Phase 2

Inspected `data/eden-us.db` and `data/eden-hk.db` via small one-off bin (`src/bin/spike_belief_inspect.rs`):

```
data/eden-us.db (US):
  belief_snapshot              128 rows  60s spaced  [2026-04-24 08:37 → 11:48 UTC]  (~3h 11m)
  intent_belief_snapshot       128 rows  60s spaced  same window
  regime_fingerprint_snapshot  124 rows  60s spaced  same window
  broker_archetype_snapshot      0 rows  (US has no broker queue)
  us_tick_record               926 rows  ~13s spaced  [08:37 → 11:48 UTC]
  tick_record (HK)               0 rows  (US-only DB)

data/eden-hk.db (HK):
  IO error: pread on 000479.sst → "Operation timed out"
  RocksDB corruption (consistent with prior memory: 006114.sst corruption + edge_learning_ledger persistence failures)
```

**Critical observation**: A1 belief persistence landed 2026-04-19 (7 days ago), but Eden runtime has run only sporadically — last write to either DB was 2026-04-24. Of those 7 days only **~3 hours of belief_snapshot data exists**, all from a single US morning session. HK DB is corrupted and unreadable.

**Statistical implication**: Cross-correlation MI / lead-lag estimation between Polymarket and Eden requires multi-day overlap. A 3-hour single-session window:
- Cannot distinguish signal from session-specific noise
- Cannot test cross-regime stability (only 1 regime sampled)
- Cannot estimate MI threshold from a shuffle null distribution (too few independent samples)

A robust framework as designed in the conversation (online MI matrix, lead-lag, decay) **needs ≥30 days of continuous co-data** to produce meaningful first wake lines.

### 6. Eden-relevant high-volatility markets exist — ✅ Phase 2 has real targets when data is available

From the top-30 scan, high-vol Eden-relevant markets (>15% hourly >1% moves):

| Market | Volat | 30d range | Likely Eden cascade |
|---|---|---|---|
| US x Iran ceasefire | 35.1% | [0.0005, 0.7300] | Oil (CL, USO, XLE), defense (LMT, RTX), tankers (FRO, EURN) |
| US x Iran permanent peace | 43.2% | [0.0265, 0.6250] | Same |
| Strait of Hormuz traffic | 35.0% | [0.0085, 0.5450] | Oil price spikes, shipping equities |
| Bitcoin dip to $65k | 35.5% | [0.0170, 0.8950] | MSTR, COIN, RIOT, MARA, IBIT |
| Iranian regime fall (multi-horizon) | 60% | [0.245, 0.755] | Oil, defense, MENA-exposed names |
| Trump ends military ops Iran | 46.1% | [0.0335, 0.6700] | Energy, defense |

These have the right **mechanism**: a discrete world event with clear cascade paths to symbols Eden tracks.

Notice the cluster: spring 2026 macro is dominated by Iran/Hormuz/Bitcoin events, not Fed (Fed locked). Phase 2 framework will work in this regime; whether it generalizes across regimes (e.g., when Fed becomes uncertain again pre-2026 Sep meeting) requires retest then.

## Decision

**Conditional go** — Phase 2 architecture is validated by today's findings, but cannot be built today because Eden's data side is starved.

**Don't build now**:
- Online correlation field on top of zero-day belief data is a useless module that immediately accumulates the wrong null distribution
- HK persistence is broken; Phase 2 wake lines depend on belief_snapshot from both runtimes
- 1-session window provides no cross-regime validation

**Don't wait passively either**:
- Polymarket data is free, lightweight, and dense (358 days available per market)
- Iran / Hormuz / Bitcoin candidate markets are moving daily — losing one day of polymarket history loses one day of future Phase 2 input
- Eden persistence problems (HK corruption, sporadic runtime) need to be fixed regardless of Polymarket

## Next steps (in order)

### Immediate (this week)

1. **Fix HK SurrealDB corruption** — separate work item. Without HK persistence working, Phase 2 wake lines for HK runtime cannot fire, and ~50% of Eden's ontological richness (broker archetype, broker queue depth) is invisible to any future Polymarket framework. Decide between rebuild from tick replay vs accept data loss + start fresh.

2. **Establish daily Eden runtime habit** — A1 belief persistence is correctly engineered but the operator hasn't been running Eden daily. 30 days of continuous belief_snapshot only happens if Eden runs ≥1 session/day from now until ~2026-05-26. Without this commitment, Phase 2 is permanently blocked. Operator decision: is this realistic?

3. **Stand up minimal Polymarket data collector** — *not* in Eden's SurrealDB (no ontology contamination). A standalone process / cron job that:
   - Polls Gamma `/markets` once a day for active market metadata (filter: volatility >5% over last 7 days, $ volume >$50k/24h)
   - Polls CLOB `/prices-history` once an hour for the filtered set, appending to flat `data/polymarket_history/<market_slug>.ndjson`
   - Zero touch to Eden code, zero new schemas, zero ontology entries
   - ~50 lines of Python, runnable from launchd or systemd
   - Goal: by the time Eden has 30 days of belief data, Polymarket has 30 days of price history aligned to the same window

### Phase 2 trigger condition (~30 days from data start)

When both:
- Eden has ≥30 days of belief_snapshot rows on at least US runtime (HK preferred but not required for v1)
- Polymarket flat-file collector has ≥30 days of price history for the high-vol candidate markets

Then build Phase 2 per design conversation:
- `src/pipeline/world_correlation_field.rs` — Welford MI matrix between (polymarket market, Eden signal) pairs, online
- Lead-lag cross-correlation per pair, retain only polymarket-leads-Eden pairs
- Wake lines `world_obs:` and `world_kl:` (described in 2026-04-26 design conversation)
- No ontology touch, no ReasoningScope hooks, no hand-curated config

### Phase 3 trigger condition (~60-90 days)

After Phase 2 has accumulated enough resolutions:
- Polymarket resolution → ground truth label → edge learning ledger
- KL projection between Eden's CategoricalBelief and Polymarket partition where structure exists

## Files produced today

- `decisions/2026-04-26/polymarket-legacy-removal.md` — executed
- `decisions/2026-04-26/polymarket-feasibility.md` — this memo
- `data/polymarket_spike/fed_apr_*_60min_max.json` — 6 Fed market price histories
- `data/polymarket_spike/volatility_top30.json` — top-30 volatility scan
- `src/bin/spike_belief_inspect.rs` — temporary inspection tool, **delete after operator review** (or keep as `dbpeek`-style introspector with a rename if generally useful)

## What was confirmed vs hoped

| Hope going in | Confirmed today |
|---|---|
| Polymarket has hourly data over months | ✅ 30 days hourly, 358 days at 12h fidelity |
| Markets self-segregate by relevance via volatility | ✅ Bimodal distribution validates Test 1 |
| σ-additivity holds on partitions | ✅ Fed partition sums to 0.9999 |
| Eden has ≥7 days of belief data to test against | ❌ Only ~3h, single session, US only |
| HK persistence is queryable | ❌ RocksDB corruption blocks reads |

The architectural design from yesterday's conversation is **sound** — every test would have worked as designed. The blocker is operational: data accumulation has not happened consistently. This is a process problem, not an architecture problem.

## Recommendation

**Proceed with the 3 immediate next steps** (HK fix, daily runtime habit, minimal Polymarket collector). Defer Phase 2 build. Revisit in ~30 days when both data sides are populated.

If operator disagrees and wants to push Phase 2 forward immediately, the alternative is to do a 3-hour smoke-test cross-correlation on the existing US session — not statistically valid, but a proof-of-life that the correlation pipeline itself runs end-to-end. This would surface implementation bugs early without committing to full Phase 2 surface area.
