# Eden Trading Methodology v0.2

**Date**: 2026-04-23
**Author**: Claude (autonomous operator)
**Account**: lb_papertrading (zero real $$ risk, outcome data for Eden learning loop)

## Why v0.2 exists

v0.1 (multi-surface consensus) was a first pass — accept any setup where ≥2 Eden surfaces agree + raw Longport confirms.

Today's pre-market run showed the problem: Eden emits **~200,000 emergent edges + ~3,300 hub wake lines + ~2,600 sym_regime divergences in ~3 hours**. Even after v0.1 filtering, easily 20-30 candidates per hour — far more than the 5 concurrent position cap allows. And there's no evidence yet that any surface alone has predictive power.

v0.2 raises the bar: **only take setups where multiple independent structural signals have sustained duration**. The extra friction reduces sample rate but raises signal-to-noise hopefully above the crossover threshold.

## Tier rules (v0.2)

### Tier 1 — $3-5k USD, -2% / +3%

Required:
- **≥3 Eden surfaces** pointing same direction, with:
  - ≥1 hub participation (symbol is hub anchor OR top-5 peer of a hub with streak >30 ticks OR mean_strength >0.70)
  - OR ≥1 sym_regime divergence >1.5 against market bucket
  - AND mod_stack final >=1.08 (long) or <=0.92 (short)
- **Raw confirm**: mcp_capital_flow shows last 30 min trend aligned with setup direction (cumulative inflow diff >0 for long)
- **Depth confirm**: mcp_depth shows no opposite-side wall >2x top-5 aggregated size
- **No self-doubt flag**: symbol not in `[us] self-doubt:` recent emits

### Tier 2 — $1.5-2.5k USD, -2% / +3%

Required:
- **≥2 Eden surfaces** agreeing, with:
  - ≥1 has persistence (hub streak >20 OR sym_regime appeared ≥3 recent cycles OR mod_stack stable >5 ticks)
- **Raw confirm**: capital_flow + depth aligned
- **No self-doubt**

### Tier 3 — skip (no position)

Everything else. Observe only. Eden signal emits into log but doesn't translate to execution.

### Exit rules

- **Stop hit**: close immediately at market
- **Target hit**: close immediately at market  
- **Time stop**: 2hr from entry, re-evaluate (not auto-close — check if Eden signals still support; if yes extend, if no close)
- **Eden flip**: mod_stack reverses sign (long setup → final <0.95) + raw microstructure confirms → close immediately
- **End-of-day**: close ALL positions by 19:55 UTC

### Risk envelope

- Max 5 concurrent positions
- Max position: $5k USD (Tier 1 upper)
- Daily stop: net session P&L ≤ -$300 USD → halt further entries (existing positions still managed to exit rules)
- Max margin usage: init_margin ≤ 50K HKD equivalent (~$6.4k USD)

## Data capture

Each trade writes `decisions/YYYY-MM-DD/NNN-{symbol}-{side}-{tier}.json`:

```json
{
  "decision_id": "003-TXN-BUY-T1",
  "at": "2026-04-23T13:35:00Z",
  "symbol": "TXN.US",
  "side": "BUY",
  "tier": "T1",
  "size_usd": 5000,
  "order_id": "...",
  "entry_price": 258.42,
  "entry_ts": "...",
  "target_price": 266.17,
  "stop_price": 253.25,
  "horizon_ts": "2026-04-23T15:35:00Z",
  "why_eden": {
    "surfaces": ["hub_anchor(degree=82, streak=15)", "mod_stack(final=1.11)", "sym_regime(divergence=1.5)"],
    "regime_bucket": "stress=2|sync=2|bias=4|act=2|turn=0",
    "self_doubt": null
  },
  "why_longport": {
    "capital_flow_30min_sum": "+4500",
    "depth_asymmetry": "bid 1.8x ask top5",
    "gap_from_prev_close": "+12.6%"
  },
  "structure_type": "earnings_gap_with_structural_hub",
  "outcome": null
}
```

On close, `outcome` populated:
```json
"outcome": {
  "exit_price": 264.30,
  "exit_ts": "...",
  "exit_reason": "target_hit" | "stop_hit" | "time_stop" | "eden_flip" | "end_of_day",
  "pnl_bps": 227,
  "hold_duration_sec": 5400,
  "realized_return_pct": 2.27
}
```

## Daily report structure

End of session → `decisions/YYYY-MM-DD/us-daily.md`:

- Trades total / tier breakdown
- Hit rate (any positive = hit)
- Mean P&L per tier
- By Eden surface: hit rate when surface was primary trigger
- Regime bucket distribution for winning vs losing trades
- 3-5 lessons learned for methodology v0.3

## Weekly validation threshold (5-day commitment)

After 5 US sessions (today + Fri + Mon + Tue + Wed) = ≥15-30 resolved trades expected:

- **Hit rate >55%**: Eden has alpha → design Autonomous Mode (v0.3 adds auto-execution)
- **Hit rate 45-55%**: Dashboard only → keep observation, find which single surface has positive expectancy, concentrate there
- **Hit rate <45%**: Signal layer broken → pause trading, go back to pressure_field / state_engine for root cause

## Explicit deferrals

- **No Autonomous Mode** until validation complete
- **No adding new Eden surfaces** until validation complete
- **No regime-conditional weighting** (method 4) until ≥8 trades per regime bucket
- **No backtest framework** (method 6) until decision to continue past week 1
- **No KL-based signal pruning** (method 5) until Eden proven alpha-positive

## Session-to-session handoff

Each day's daily.md is appended context for the next day. Week-end:
- `decisions/2026-04-27/weekly-validation.md`: aggregate 5 days, decide go/no-go on Autonomous Mode.
