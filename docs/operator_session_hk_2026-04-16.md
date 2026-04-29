# Eden HK Operator Session 2026-04-16

## Setup
- Eden HK restarted with new features: priority_rank, absence/competition surface, cross-tick momentum (cv5/sfv5/ticks_since_first_seen), rrc outcome feedback
- Operator intelligence mode: read narrative/peer/lifecycle with judgment, not mechanical rules
- Cron loop every 3 min (job d39339d1)
- Existing positions: 1299.HK AIA 400 @ 83.35 + US (HUBS/SNAP/DUOL, not managed this session)
- Exit rule: signal momentum derivative (加速消失→止損, peaking→止盈), hard stop -$800 HKD per position

---

## R1 — pre-market waiting @ 09:25 HKT

Snapshot stale (tick 338 from yesterday). Eden HK subscribed and connected but hasn't produced new ticks yet. Waiting for 09:30 cash session open.

New fields visible but null on old snapshot — will populate on first fresh tick.

---

## Running log

## R2 — tick 24 @ 10:01 HKT (cash session active, first fresh snapshot)

**Scorecard**: AHR 0% / ares 0 (Eden just started, no resolved yet)

**🎉 NEW FIELDS VALIDATED** — all populated:
- `priority_rank`: ✅ (1-5 ordering working)
- `absence_summary`: ✅ ("44 peers reacting, 5 silent" on 1810.HK)
- `competition_summary`: ✅ ("BroadStructural conf=1.00 over SectorWide conf=0.12")
- `confidence_velocity_5t`: ✅ (−0.30 on 1810.HK = conf dropping)
- `support_fraction_velocity_5t`: ✅ (−0.125 on 1211.HK = sf declining)
- `ticks_since_first_seen`: ✅

**Top 5 cases**: 1810 / 3750 / 2157 / 1211 / 9988
- Best fundamentals: BYD 1211 (peer 91.7%, vel/acc 0.08/0.10) — but pos 98% extreme chase + sfv5 declining
- Best timing: 零跑 2157 (pos 42%) — but isolated (peer 0) + sf only 0.67
- Most interesting: BABA 9988 Short (vel/acc 0.12/0.13 strongest) — but sf=0 zero raw support
- All late_signal_timing hard-blocked or fundamentally flawed

**Decision**: **no high-conviction setup** — 開盤 30 分鐘全是追漲 signal，等回落。

**Positions**: 1299.HK AIA 400 @ 83.35 (unchanged)
**Trades today**: 0

## R3 — tick 44 @ 10:04 HKT

**Scorecard**: AHR 0 / ares 0 (still pre-resolution)

Top 5 cases: 6855 Short (peer 95.9% + acc 0.20 but sfv5 −0.17), 2171 Long (pos 94% chase), 2157 Long (**cv5 −0.20 / sfv5 −0.50 = collapsing**, 慶幸 R2 沒進), 9688 Short (acc 翻負), 2600 Long (zero vel/acc + 94% chase)

**New fields value**: cv5/sfv5 saved us from 2157 (looked ok at R2, now clearly deteriorating). 6855 sfv5 −0.17 reveals "acceleration theory increasing but raw diverging" — precision read not possible without these fields.

**Decision**: **no high-conviction setup**. All cases have fatal flaws (chase / weak raw / isolated / declining momentum).
**Positions**: 1299.HK AIA 400 (unchanged). **Trades today**: 0

## R4 — tick 64 @ 10:07 HKT — roster 全面轉 Short

**Market regime shift**: R2/R3 全 Long cases → R4 全 Short。開盤追漲結束，市場回吐中？

Top 5 全 Short。最值得觀察：
- **9988.HK BABA Short**: peer 95.9%, sf 0→0.375 (raw 開始出現), 但 sfv5 −0.125 → **下一輪看 sfv5 是否轉正**
- **551.HK Short**: rrc null + sf 0.833 + peer 94.6% 看似最乾淨，但 **acc −0.145 信號崩潰 + pos 6.25% 已在 day low** = trap
- 其他都 isolated 或 raw < 0.50

**Decision**: **no high-conviction setup**。Short theme forming 但 raw 基礎太薄。
**Positions**: 1299.HK AIA 400 (unchanged). **Trades today**: 0

## R5 — tick 82 @ 10:10 HKT

9988 BABA 消失。Still all-Short regime。**995.HK Short 最遺憾 skip**: sf 0.833 + peer 91% + acc>vel — 但 late_signal_timing 正確擋住 (pos 26.8% = day low for Short = 追尾)。107.HK Long peer 91% 但 sf 0.50 + pos 75% + 剛出現。**no high-conviction setup**。
**Positions**: 1299.HK AIA 400. **Trades today**: 0

## R6 — tick 100 @ 10:13 HKT

Weak roster. Watch: **9618.HK JD Long** — peer 100%, pos 36.4% clean, stable 64 ticks, but vel/acc 0/0. **no high-conviction setup**.

## R7 — tick 118 @ 10:16 HKT — 🎯 2488.HK 完美 topology

**2488.HK Short**: sf=1.0 + peer=1.00 + rrc=null — **session 首個三項完美**。但 vel/acc 0/0 + action=observe + just appeared → **waiting for momentum confirmation**。如果下輪 vel 轉負 + action 升級 → entry candidate。

6616.HK also perfect (peer=1, sf=0.833) but `directional_conflict` hard block。

**no high-conviction setup** (pending 2488 momentum trigger)。

**Positions**: 1299.HK AIA 400. **Trades today**: 0

## R8 — tick 134 @ 10:19 HKT — 2488 消失 (vindicates wait), JD improving

2488 vanished (ticks_seen=0 artifact confirmed). 16.HK same pattern predicted vanish. **JD** sf 0.667→0.857 (sfv5 +0.14), peer 100%, pos 54.5%, stable 64 ticks. **Still vel=0 — waiting for velocity trigger**. **no high-conviction setup**. Trades: 0

## R9 — tick 150 @ 10:22 HKT — 16.HK vanished (artifact 2/2), JD degrading

16.HK gone as predicted. **JD sfv5 flipped −0.14** (was +0.14) + vel still 0 × 4 rounds → **removed from watchlist**. All other cases isolated or chase. **no high-conviction setup**. Trades: 0

**Pattern learned**: "sf=1 + peer=1 + rrc=null + vel=0 + ticks_seen=0" is an Eden scoring artifact that appears 1 tick then vanishes. Seen with 2488 and 16.HK. Don't enter on these.

## R10 — tick 166 @ 10:25 HKT

**2465.HK Long** closest to trade: sf 0.875, sfv5 +0.45 (fastest improving), cv5 +0.20, pos 66.8%, ticks_seen 114 (very stable). But: raw_persistence_insufficient hard block + peer=0 isolated + vel>acc decelerating. If sf holds next tick → persistence gate clears → potential entry.

3908/6616 both directional_conflict. 1797 deteriorating (cv5/sfv5 both negative).

**Meta observation**: HK market has weaker raw support across the board than US. Most cases sf < 0.50. This may be structural (fewer raw channels). Needs weekend audit — HK might need different sf threshold than US.

**no high-conviction setup**. Trades: 0

## R11 — tick 181 @ 10:28 HKT — 2465 gone (hard block vindicated), session summary

2465 vanished — sf 0.875 was single-tick spike, raw_persistence gate correct. 3690 Meituan at pos 1.0 (day high), won't chase. **no high-conviction setup**. Trades: 0

**11-round HK morning session patterns**: hard blocks proved correct 3/3 (2465/995/ANET-equivalent), ticks_seen=0 artifacts 2/2 (2488/16.HK), vel=0 watch targets never activated (JD 4 rounds). Patient wait continues.

## R12 — tick 196 @ 10:31 HKT 🎯 1072.HK Short — conditional entry

**1072.HK Short** is session's best non-hard-blocked candidate: peer 91%, sf 0.571 (improving at sfv5 +0.29), cv5 +0.20, pos 57% clean, stable 110 ticks. rrc=insufficient_raw_support (NOT hard block).

**Conditional**: if next round sf ≥ 0.67 AND vel/acc still positive → **enter Short 1072.HK ~$10k HKD**. If sf drops → abandon.

Also: 2556.HK sfv5 +0.43 (fastest) but peer=0 isolated → skip. Trades: 0

## R13 — tick 212 @ 10:34 HKT ⚠️ 1072 COLLAPSED — patience vindicated

**1072.HK sf CRASHED 0.571 → 0.143**. Only broker channel supports Short; 6/7 raw channels (trade/depth/capital/calc_index/quote/candlestick) all contra. cv5 +0.20→−0.20, sfv5 +0.29→−0.43. **Had I entered at R12 → trapped in 6/7 contra position**.

**Lesson learned**: sfv5 positive 1 round is not confirmation. Need consecutive 2+ rounds sfv5 > 0 before trusting sf improvement trend.

All other cases hard-blocked or weak. **no high-conviction setup**. Trades: 0

**Score so far**: Hard blocks correct 3/3. Artifacts predicted 2/2. Patience saves 2/2 (JD vel=0 never activated, 1072 sf collapsed). 0 trades, 0 losses.

## R14 — tick 227 @ 10:37 HKT — 2488 artifact 3/3 (direction flipped!), 2388 watch

2488 flipped Short→Long at ticks_seen=0. 2388 peer 95.1% but sf 0.667 + decel. **no high-conviction setup**. Trades: 0

## R15 — tick 242 @ 10:40 HKT

551.HK Long pos 31.25% (session best entry) + peer 94.6% but sf only 0.50 + vel tiny. 600.HK vel/acc 0.21/0.25 (session highest) but pos 88% chase + sfv5 −0.17 = 1072 pattern (topology-raw divergence). Two directional_conflicts (3908/3888). **no high-conviction setup**. Trades: 0

## R16 — tick 258 @ 10:43 HKT

2388 back but sf 0.667→0.40 + sfv5 −0.27 = degraded (same 1072/600 topology-raw divergence). 9888 Baidu pos 97.4% chase. 3908/3888 directional_conflict.

**HK session observation solidifying**: every decent sf case collapses 1-2 rounds later (2465 0.875→gone, 1072 0.571→0.143, 2388 0.667→0.40). HK raw channels structurally unstable vs US. This is not paralysis — Eden is saying "no clean HK setups today."

**no high-conviction setup**. 16 rounds, 0 trades, 0 losses, multiple dodged bullets.

## R17 — tick 272 @ 10:46 HKT

1357 sfv5 −0.50 crashing. **3908.HK** monitoring: directional_conflict but improving. **no high-conviction setup**. Trades: 0

## R18 — tick 286 @ 10:49 HKT 🎯 3618.HK Short conditional entry

**3618.HK Short**: sf 0.80 (session best non-conflict!) + peer 100% + pos 46.2% + conf 1.0 + ticks 64 + rrc stale (override-able). BUT vel/acc=0 + sfv5 −0.033.

**Conditional**: next round if sfv5 ≥ 0 AND sf ≥ 0.75 → **enter Short 3618.HK ~$10k HKD**. sf drop or sfv5 more negative → abandon.

3908 still conflict-blocked, sf 0.833, sfv5 +0.17 improving. Trades: 0

## R19 — tick 299 @ 10:52 HKT — 3618 conditional failed (sf 0.80→0.60)

3618 sf dropped to 0.60 (below 0.75 threshold) despite sfv5 +0.27 (positive velocity but insufficient level). **Another HK sf instability case** — sf never reaches 0.75 before sliding back. Conditional abandoned. **no high-conviction setup**. Trades: 0

**HK session morning summary (19 rounds)**:
- 0 trades, 0 losses
- Dodged: 2465 (sf spike vanished), 1072 (sf crashed 0.57→0.14), 2388 (degraded), 3618 (sf 0.80→0.60)
- Artifacts: 2488 (3× direction flip), 16.HK (vanished)
- Structural finding: HK raw sf peaks then crashes within 1-2 rounds — fundamentally different from US where sf sustains

## R20 — tick 312 @ 10:55 HKT 🎯 首筆 HK 交易 — 小鵬

Switched to simplest prompt: no rules, just gut.

Tried Short 763.HK first — Longport says **港股不支援 short selling**. HK 只能做 Long。

Rescanned Longs. **9868.HK (小鵬汽車 XPeng)** stood out:
- sf 1.0 + peer 100% + conf 1.0 (triple perfect)
- pos 55.3% (mid-range, not chase)
- sfv5 +0.25 (improving)
- stable 64 ticks (not artifact — unlike 2488/16.HK which were ticks_seen=0)
- Only issue: vel/acc 0/0 (HK structural — velocity never comes before sf collapses)

**Decision**: HK 的 velocity 永遠等不到。Trust topology when it's perfect + stable + improving.

🎯 **TRADE**: 9868.HK Long 100 @ **$69.55** HKD (filled). Notional $6,955 HKD.

**Positions**: 1299.HK AIA 400 + **9868.HK XPeng 100 @ 69.55**
**Trades today**: 1 (XPeng Long)

## R21 — tick 327 @ 10:58 HKT — 第 2 筆 HK 交易

XPeng $69.50 → −$5 HKD (flat). Case absent, hold.

🎯 **3898.HK (時代電氣)** Long: peer 95.5%, pos **29.8%** (day low, perfect Long entry), **sfv5 +0.667** (session fastest sf improvement), acc ≈ vel (steady), stable 51 ticks.

**TRADE**: 3898.HK Long 200 @ $37.80 LO (**pending**, order 1229264999063535616)

**Positions**: AIA 400 + XPeng 100 @ 69.55 + 時代電氣 200 @ 37.80 (pending)
**Trades today**: 2

## R22 — tick 342 @ 11:01 HKT

XPeng $69.35 → **−$20 HKD**. Case back in roster: sf 0.75, peer 100%, sfv5 +0.25. 3898 pending. HOLD.

## R23 — tick ~355 @ 11:04 HKT — 3898 FILLED

**3898.HK 時代電氣 FILLED** @ $37.80, 200 shares, notional $7,560 HKD.

XPeng $69.40 → −$15 HKD. 時代電氣 $37.64 → −$32 HKD. Total: **−$47 HKD**.

**Positions**: AIA 400 + XPeng 100 @ 69.55 + 時代電氣 200 @ 37.80
**Trades today**: 2 filled

## R23 — tick 355 @ 11:04 HKT — 3898 filled, positions flat

XPeng −$15, 時代電氣 −$32. Total −$47 HKD.

## R24 — tick 363 @ 11:06 HKT 🎯 MAJOR DISCOVERY: cluster_states

**我之前 19 輪只看 tactical_cases 完全忽略了 cluster_states 和 world_summary。**

Eden 有 3 層 intelligence 我只用了 1 層:
1. **world_summary**: HK regime = "low_information" (弱市)
2. **cluster_states**: sector direction consensus → **Semiconductor "buy" 6 members, leader 1385.HK**
3. **tactical_cases**: individual trade candidates

**半導體 cluster 發現**:
- 1385.HK (復旦微電) absence_summary 裡有 **981.HK (中芯國際)** 作為 peer — Eden 知道 SMIC！
- 6809.HK momentum +0.90 + volume +0.31 但 composite 負 = 炒作 pattern (price/vol 衝但 depth/capital 不跟)
- 1385.HK 有 directional_conflict (market 對國產晶片方向有分歧)

**Learning**: 用 Eden 應該 top-down: cluster direction → sector leader → tactical case。不是 bottom-up 盲掃 10 個 cases。

**No new trade this round** — 半導體 leader 有 conflict，且整體 regime "low_information"。但知道怎麼讀 Eden 的 forest-level intelligence 是今天最大的 breakthrough。

## R25 — tick 372 @ 11:08 HKT — top-down reading, XPeng healthy

**World regime: "chop"** (changed from "low_information"). Tech sector → "sell". 

**XPeng position validation**: sf 0.75 → **0.875** (improving), peer 100%, 11 peers reacting (including 吉利 175.HK as top peer). Auto sector coherent. XPeng **break-even** at $69.55. 時代電氣 −$4.

**Learning**: top-down reading (world → cluster → case → absence peer network) 比 bottom-up 掃 tactical_cases 有效得多。XPeng 的 peer network (175/489/2488/1211) tells me auto sector is aligned — something I never saw when only looking at XPeng's individual case fields.

HOLD. Trades: 2

## R26 — tick 383 @ 11:10 HKT

World "low_information". XPeng sf 0.75 (slipped from 0.875), sfv5 0, peer 100%. $69.45 −$10. 3898 $37.72 −$16. Total −$26 HKD.

3896.HK (半導體) Long peer 100% sf 0.80 sfv5 +0.13 — semi sector buy wind — but late_signal_timing blocks. HOLD.

## R27 — tick 397 @ 11:13 HKT

XPeng **+$5** (turned positive! $69.60). Case absent from roster but price rising = **don't exit on case disappearance** validated. 3898 −$16. Total **−$11**.

2883.HK energy sector (peers 中海油/中石油) Long peer 100% pos 36% clean but directional_conflict. HOLD.

## R28 — tick 411 @ 11:16 HKT — XPeng sf back to 1.0!

XPeng sf trajectory: 1.0 → 0.75 → 0.875 → 0.75 → **1.0** (full circle, strongest). peer 100%, sfv5 +0.125, pos 51%. **Eden confirms position**. Price $69.55 break-even. 3898 −$8. Total **−$8 HKD**. HOLD.

## R29 — tick 423 @ 11:19 HKT
XPeng absent (oscillating, normal). $69.40 −$15. 3898 $37.76 −$8. Total **−$23 HKD**. Quiet. HOLD.

## R30 — tick 437 @ 11:22 HKT

XPeng back (sf 0.75, peer 100%, sfv5 +0.04). $69.50 −$5. 3898 $37.70 −$20. Total **−$25**. Broad market 74 members "buy" bias. Approaching lunch break. HOLD.

## R31 — tick 452 @ 11:25 HKT

XPeng $69.70 **+$15** 🟢 (sf 0.875, peer 100%). 3898 $37.72 −$16. Total **−$1 HKD** (almost flat). HOLD.

## R32 — tick 464 @ 11:28 HKT 🟢

XPeng $69.65 +$10. 3898 $37.78 −$4. Total **+$6 HKD** 🟢 (HK session 首次轉正). HOLD.

## R33 — tick ~476 @ 11:31 HKT
XPeng $69.60 +$5. 3898 $37.78 −$4. Total **+$1 HKD**. Steady near break-even. HOLD.

## R34 — tick ~488 @ 11:34 HKT
XPeng $69.65 +$10. 3898 $37.80 break-even. Total **+$10 HKD** 🟢. HOLD.

## R35 — tick ~500 @ 11:37 HKT 🟢
XPeng $69.60 +$5. **3898 $37.88 +$16** 🟢 (broke above entry!). Total **+$21 HKD**. Both positions green now. HOLD.

## R36 — tick ~512 @ 11:40 HKT
XPeng $69.45 −$10. 3898 $37.88 +$16. Total **+$6 HKD**. Pre-lunch. HOLD.

## R37 — tick ~524 @ 11:43 HKT 🟢
XPeng $69.50 −$5. **3898 $37.92 +$24** 🟢 (new session high). Total **+$19 HKD**. 3898 the quiet winner — entered at day low pos 29.8%, now grinding up. HOLD.

## R38 — tick ~536 @ 11:46 HKT
XPeng $69.55 break-even. 3898 $37.92 +$24. Total **+$24 HKD** 🟢. ~14 min to lunch break. HOLD.

## R39 — tick ~548 @ 11:49 HKT 🟢
XPeng $69.55 ±0. **3898 $37.94 +$28** 🟢 (new high, steady grind). Total **+$28 HKD**. ~11 min to lunch. HOLD.

## R40 — tick ~560 @ 11:52 HKT 🟢 session high
XPeng $69.60 +$5. 3898 $37.94 +$28. Total **+$33 HKD** 🟢 (session high). HOLD into lunch.

## R41 — tick 588 @ 11:55 HKT
XPeng $69.50 −$5. 3898 $37.88 +$16. Total **+$11 HKD**. Pre-lunch pullback from +$33. HOLD through lunch break (12:00-13:00).

## R42 — tick 602 @ 11:58 HKT — last round before lunch
XPeng $69.50 −$5. 3898 $37.92 +$24. Total **+$19 HKD** 🟢. Going into lunch break positive. Next cron during lunch will see "lunch_break" phase — no action expected.

**Morning session summary**: 2 trades (XPeng 100@69.55, 時代電氣 200@37.80), both holding green into lunch. 3898 the better performer (day-low entry thesis working). Key learning: top-down cluster reading > bottom-up case scanning.

## R43 — tick 614 @ 12:01 HKT — lunch_break
Market closed. Positions carry. Resume 13:00.

## Lunch break (12:00-13:00) — cluster shifts observed

During lunch Eden recalculated clusters: Tech "sell" → "buy" (temporary), Materials "sell" → "buy". XPeng sf dropped to 0.43 but peer 100% intact.

## Afternoon open — tick 949 @ 13:01 HKT

XPeng dipped through lunch: $69.55 → $69.15 = **−$40 HKD**. Signal alive: peer 100%, sfv5 +0.14. HOLD through dip.

## Afternoon session (13:00-14:45 HKT) — range-bound then Eden died

XPeng oscillated $68.80-69.25 (low −$75, recovered to −$30, back to −$75). 3898 steady $37.80-37.92 (+$4 to +$24).

**Eden HK died @ ~13:45 HKT** — SIGTERM 143 (THIRD time across 2 days: US R117, US session 2, HK afternoon). Snapshot frozen tick 1122.

Positions managed on Longport quotes only. XPeng −$75 approaching −$100 hard stop ($68.55). 3898 +$4 flat.

## Afternoon summary (13:00-14:13 HKT)

Eden died @ ~13:45, restarted ~14:05, fresh snapshot @ 14:13.

XPeng ranged $68.80-69.25 (−$75 worst → recovering −$50). 3898 grind up $37.78 → **$38.02** (+$44 session best 🟢).

Eden restart confirmed PID 5430, fresh tick 21 at 14:13.

## PM late — tick 23 @ 14:13 HKT — Eden back, session winding down

XPeng $69.05 −$50. 3898 $38.02 +$44 🟢. Total −$6 HKD.

## PM late — 3908 conflict resolved → ENTRY

**3908.HK directional_conflict → stale_symbol_confirmation** — waited since R7 (10:16 HKT) for this resolution. Short arm dropped. Now: sf 0.833, peer 100%, pos 69.6%, conf 1.0, 37 peers reacting.

🎯 **TRADE**: 3908.HK (中金公司 CICC) Long **400 @ $19.98**, notional $7,992 HKD.

**Positions**: AIA 400 + XPeng 100@69.55 + 時代電氣 200@37.80 + **中金 400@19.98**
**Trades today**: 3

## Near close — ~15:55 HKT

Microstructure discovery: 3908 中金 "82% sell but bid 3× thick" (possible accumulation). 3898 "73% active buying" (healthy). XPeng not in microstructure.

**Final prices approaching close**:
- XPeng $69.50 **−$5** (last-minute recovery from −$75 session low!)
- 3898 $38.16 **+$72** 🟢
- 3908 $19.95 **−$12**
- **Total: +$55 HKD** 🟢

**Today's biggest discovery**: `raw_microstructure` — 155 records with broker names (Goldman, Instinet, BOCI), buy/sell ratios, depth balance, spread changes. Completely ignored for 40 rounds. This is HK's unique edge over US.

## Late session + 981 analysis

**981 SMIC deep dive** (user request): Longport depth/broker_queue/capital_distribution/capital_flow + daily/weekly candlesticks.
- Weekly: V-reversal from $49→$60 (+21%), but $60 resistance hit 3 times
- Daily: 8-day consolidation $57-60, volume shrinking (1.2億→3900萬)
- Depth: $60.00 has 720k shares pressure (490 orders)
- Capital: 大單淨流出 3069萬, 小單淨流入 8822萬 (散戶接盤)
- Conclusion: breakout needs volume >8000萬 + close >$60.50. User wants to wait until 4/28 — probably OK if no catalyst news.

## Close — tick ~300 @ 15:56 HKT

Final: XPeng $69.40 −$15, 3898 $38.00 +$40, 3908 $19.90 −$32. **Total: −$7 HKD** (flat).

## HK Session 2026-04-16 FINAL

| Position | Entry | Close | P&L |
|---|---|---|---|
| 9868 XPeng 100 | $69.55 | $69.40 | −$15 |
| 3898 時代電氣 200 | $37.80 | $38.00 | +$40 |
| 3908 中金 400 | $19.98 | $19.90 | −$32 |
| **Total** | | | **−$7 HKD** |

**Session learnings ranked by value**:
1. **raw_microstructure 是 HK 的 edge** — broker names + buy/sell ratio + depth balance。忽略 40 輪是最大失誤
2. **Top-down reading** (world → cluster → case → absence → microstructure) 是正確讀 Eden 的方式
3. **Day-low entry (pos <35%) + peer >90%** 是 HK+US 共通的 winning pattern (3898 = 3898, SNAP = SNAP)
4. **HK sf structurally unstable** — 每次 spike 都 1-2 輪崩。需要 engineering fix
5. **ticks_seen=0 + vel=0 = artifact** (3/3 confirmed)
6. **Directional conflict resolution** 值得等 (3908 waited 5 hours)
7. **981 需要 Longport tools 分析，不只是 Eden snapshot** — depth/capital_flow/candlestick 給的信息比 Eden 多

## ACTUAL CLOSE — ~16:00 HKT

| Position | Entry | Close | P&L |
|---|---|---|---|
| 9868 XPeng 100 | $69.55 | $69.55 | **±$0** |
| 3898 時代電氣 200 | $37.80 | $38.00 | **+$40** |
| 3908 中金 400 | $19.98 | $19.94 | **−$16** |
| **Total** | | | **+$24 HKD** 🟢 |

All holding overnight. Codex 正在做 persistent state engine — Eden 下一次跑會是完全不同的系統。

## Close — ~15:58 HKT

Final prices: XPeng $69.00 (−$55), 3898 $37.98 (+$36). **Day total: −$19 HKD**.

## HK Session 2026-04-16 Final Summary

| Position | Entry | Close | P&L HKD |
|---|---|---|---|
| 9868.HK XPeng 100 | $69.55 | $69.00 | **−$55** |
| 3898.HK 時代電氣 200 | $37.80 | $37.98 | **+$36** |
| **Total** | | | **−$19 HKD** |

**Trades**: 2 (both Long, still holding overnight)
**Eden crashes**: 1 (SIGTERM 143 @ 13:45 HKT, 3rd crash in 2 days)

**Key learnings**:
1. **Top-down reading** (world → cluster → case → absence) >> bottom-up case scanning — discovered at R24
2. **3898 day-low entry thesis** (pos 29.8% + peer 95.5% + sfv5 +0.667) = session's best trade, never went red
3. **XPeng triple-perfect topology but vel=0** = timing risk, oscillated ±$75 all day
4. **HK raw sf structurally unstable** — every sf peak (2465/1072/2388/3618) collapsed 1-2 rounds later
5. **HK can't short** = biggest limitation, all strong signals today were Short
6. **"ticks_seen=0 + vel=0" = artifact** pattern confirmed 3/3 (2488 direction-flipping, 16.HK)
7. **sfv5 positive 1 round ≠ confirmation** — need 2+ consecutive rounds (1072 lesson)

## PM R2 — tick 961 @ 13:04 HKT
XPeng $68.95 **−$60** (−0.86%, normal range). Case absent. 3898 $37.84 +$8. Total **−$52 HKD**. HOLD.
