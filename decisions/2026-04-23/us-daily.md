# Eden US Daily — 2026-04-23 (Validation Week Day 1)

**Account**: lb_papertrading  
**Session**: 13:30–19:55 UTC (regular hours)  
**Methodology**: v0.2 (stricter noise filter)

## Trades summary

| # | Symbol | Side | Tier | Entry | Exit | Hold | P&L | Reason |
|---|--------|------|------|-------|------|------|-----|--------|
| 003 | MDB.US | SELL (short) | T2 | 17:01 @ 254.76 | 17:26 @ ~253.90 | 25 min | **+$6.88 (+34 bps)** | raw_microstructure_fade_confirmed |

**Total trades**: 1 submitted, 1 resolved  
**Hit rate**: 100% (1/1)  
**Mean P&L**: +34 bps  
**Net USD P&L**: +$6.88

## Signal-level accounting (MDB #003)

**Eden surfaces that triggered entry**:
- `mod_stack(pf:MDB.US:short:mid30m final=0.855)` — 150+ occurrences <=0.92 all session
- `sym_regime bias=0 action=enter` — 5+ cycles repeated
- `hub CMCSA peer max_streak=75` — persistence anchor

**Why it worked**: MDB dropped from prev_close 269.54 → day low 250.50 (**-7.1% peak**). Eden signal was correct directionally. Capital flow cumulative -$2,470 at entry confirmed sustained outflow.

**Why only +34 bps**: Operator (me) had long-side bias and missed the first 3.5h of Eden's short signal. Entered at 17:01 when MDB was already -5.2% from prev_close; target 250.10 was only $0.40 below day low 250.50 (already priced in). Exited when capital_flow momentum derivative went flat (-$91 net outflow over 24 min vs prior -$3-5K/min pace) + sector_wave flipped GROWING→FADING. Locked small profit rather than ride mean-reversion bounce.

**Capture ratio**: Eden delivered ~7% move on prev_close. I captured ~0.34%. **20× under-capture** — dominant cause is late entry, secondary cause is early exit.

## Operator error log

1. **Long-side bias** (pre-session): scanned Eden only for Long setups despite 4,532 Short setups over pre-market. Missed HOOD (233× fires) and MDB (150× fires) entirely until user asked "為什麼不做空?".
2. **Sloppy T2 recommendation on SNDK** (17:32): flagged as cleanest T2 based on `pressure→action Long + sym_regime bias=4` only, pushed to user with A/B/C options; user picked A; THEN I ran raw confirm and found multi-surface conflict (composite -0.44, option_cross_validation confirms NEGATIVE force, sector_wave dir=flat, half-conductor sector conflicted, mod_stack final=1.000 neutral). Aborted pre-order. Should have cross-checked before recommending.

## Methodology v0.2 gaps identified

1. **`mod_stack` grep key wrong**: v0.2 says grep `[us] mod_stack:` but log format is `mod_stack: setup_id=pf:SYM:dir:window ... final=X.XXX`. 0 hits on the v0.2 pattern, N hits on the real pattern. Fix needed in v0.3.
2. **pressure→action单点可以骗人**: SNDK had conf=1 Long but composite/option/sector/mod_stack all disagreed. `pressure→action` must not count as a standalone surface; it must be cross-checked against composite direction + option_cross_validation + sector_wave dir + mod_stack final in same cycle.
3. **No pre-market bidirectional scan**: v0.2 only scans at session start. Needs pre-market scan for persistent <=0.92 OR >=1.08 mod_stack to avoid 3.5h entry delays (like MDB).
4. **No MCP resilience**: Longport MCP dropped 3 times during session, blocking both entry and monitoring. Need graceful degradation path or retry loop.

## For v0.3 (Day 2+ methodology)

**Hard requirements to add**:
- Gate 1 (Eden direction consensus): pressure→action dir == sign(composite) == sector_wave dir == option_cross_validation direction. All 4 must agree.
- Gate 2 (mod_stack absolute): final >=1.08 (long) or <=0.92 (short) in same cycle. No "equivalent surfaces" substitution.
- Gate 3 (persistence): ≥1 surface with streak>30 OR repeated 5+ cycles (raised from v0.2's 3+)
- Gate 4 (raw confirm): capital_flow last 30 min direction matches + depth no 2x opposite wall
- Gate 5 (anti-bias): pre-market scan BOTH directions; if session-open Eden shows >20 persistent short signals, operator must include short scan each cycle

**Soft additions**:
- Horizon-based target/stop derived from Eden horizon module (not ad-hoc -2%/+3%)
- Per-symbol regime memory to avoid chasing signals that Eden has flagged self-doubt on

## Existing positions (unchanged from previous sessions)

HK (not touched today — cross-market, no US session action needed):
- 3908.HK 400 @ 19.98 HKD
- 3898.HK 200 @ 37.80 HKD
- 1299.HK 200 @ 84.75 HKD

## Next session (Day 2 = 2026-04-24 Friday)

- Use v0.3 gates (5-gate hard, 2 soft) if drafted by 13:00 UTC
- Pre-market bidirectional scan 13:00–13:30 UTC
- Default: smaller T2 size ($1.5k) until ≥5 resolved trades accumulated
- MCP resilience: any MCP drop during open position → immediate manual Longport app check

---

**Validation week status**: 1/5 days done. 1 resolved trade. Need ≥14 more resolved trades over Days 2–5 to hit 15-trade minimum for weekly validation threshold (>55% hit rate = Autonomous Mode design greenlight).

---

## Session end (20:00 UTC / 04:00 HK)

**Final state**:
- US positions: 0 (MDB closed 17:26 UTC)
- HK positions: 3908/3898/1299 (untouched all day)
- Total US trades today: 1
- Hit rate: 100% (1/1)
- Net US P&L: +$6.88 USD (+34 bps on $2040 notional)
- Max drawdown during session: 0

**Post-17:26 loop observations (3 hours of v0.2.1 scanning, 0 new entries)**:
- mod_stack T1 threshold (final ≤0.92 short or ≥1.08 long) almost never fires cleanly
- Repeat offenders: BITO short final ~0.88 (Eden持續downmod做空BITO的信心), MU/MARA/BITO long final ~0.88-0.92 (Long setup被壓)
- 2-hour stretch 19:00-19:45 had 5/9 cycles with zero T1 signals
- Interpretation: v0.2.1 gates correctly filter noise — only high-conviction multi-surface setups trigger, and session-mid/late typically doesn't produce them (structural moves happen in first hour + earnings window)

**v0.3 推論**:
- Pre-market + first-hour (13:30-14:30 UTC) 是 T1 setup 爆發窗口。v0.3 要把這個時段 loop 跑密一點（1-2 min per cycle）
- Session-mid/late 可以 slow down loop（10-15 min per cycle）或 event-triggered only
- v0.2.1 的 mod_stack 單獨判讀方向仍有歧義（setup direction vs belief multiplier direction 區分不清）— 需 decision tree 明確畫出 "long downmod = bearish read" vs "long downmod = no long signal (neutral)"
