# Eden US Operator Session 2026-04-15 — v2 (post-restart)

## Setup
- Eden US restarted after R117 SIGTERM crash of session 1
- Discipline rules v2 active (3 rules: entry conf≥0.7 + support≥67% + soft-block rrc override, size $3k, Eden-signal exit)
- New scorecard field: `actionable_excess_hit_rate = AHR − baseline_hr`
- Cron loop every 3 min (job f9d0db73)

## v2 entry rules
Qualifying case:
- `confidence >= 0.7`
- `raw_disagreement.support_fraction >= 0.67`
- `review_reason_code` is null OR `stale_symbol_confirmation`

Hard-block rrc (never override):
- `directional_conflict`
- `late_signal_timing`
- `raw_persistence_insufficient`
- `freshness_decay_aging` / `freshness_decay_expired`

---

## R1 — first loop after restart @ 16:46 UTC

**Eden state**: tick 39 (fresh restart), scorecard still 0/0 (pre-first-resolution), cash_session.

**Qualifying case found**: 
- **KC.US Short** (Kingsoft Cloud, Chinese cloud ADR)
- conf = 1.0, support_fraction = 0.75, rrc = stale_symbol_confirmation (allowed override)
- position_in_range = 66.7% (Short not at day low — valid timing)

**🎯 TRADE EXECUTED**:
```
symbol: KC.US
side: Sell (short)
quantity: 180 shares
price: $16.65 (limit, filled immediately)
notional: $2,997 USD
order_id: 1229110611837726720
timestamp: 2026-04-15 16:46 UTC
```

**Position after**: KC.US -180 @ 16.65 + 1299.HK 400 @ 83.35.

**Exit triggers for KC**:
1. Eden signal disappears or action drops to observe
2. composite_score flips positive
3. Unrealized P&L < -$100 USD (~$0.56 price rise → cover at ≥ $17.21)
4. raw_support_fraction drops below 0.50

**Session 1 streak ended**: 120 → 0 rounds 0-enter streak. First actual trade of the day.

---

## Running log

(appended each cron fire)

## R2 — tick 195 @ 16:49 UTC

**Scorecard**: AHR 49.12% / excess **8.50pp** / baseline 40.62% / ares 3288

🎯 **excess_over_baseline 正式上線** — 新 field 首次 populated，8.50pp 正 selectivity edge on fresh 3288 sample.

**KC.US exit**: case 消失 roster → v2 exit rule 1 觸發 → cover 180 @ market。
- Entry: $16.65, Exit: $16.68, **Realized P&L: −$5.40** (−0.18% on notional, fees $0)
- Holding time: ~3 minutes, Eden signal faded faster than expected

**New entry**: HUBS.US Long, conf=1, sf=0.75, rrc stale_symbol_confirmation (override), pos_in_range 44.1%
- Submitted 14 shares @ $214.55 LO, notional $3,003.70
- order_id 1229111467756756992, status NewStatus pending

**Positions end-R2**:
- 1299.HK AIA 400 @ 83.35 (unchanged)
- HUBS.US 14 pending @ 214.55 (if fills)

**Trades count today**: 2 executions (KC short + cover), 1 pending (HUBS entry). Realized −$5.40.

## R3 — tick 325 @ 16:52 UTC

**Scorecard**: AHR **59.37%** / excess **14.20pp** / baseline 45.17% / ares 6478
- AHR 從 R2 49.12% → R3 59.37% (**+10.25pp** over 130 ticks, non-linear acceleration)
- excess 8.50 → 14.20 (+5.70pp): actionable tier gap widening

**HUBS pending still unfilled** (status NewStatus, limit $214.55 not crossing)

**User correction**: 前輪 "case 消失就 cover" 是 over-reaction。新 exit rule：
- **止損**: signal velocity 負 AND acceleration 負（加速消失）
- **止盈**: signal value 仍高 AND acceleration 從正轉負（peaking）
- Ignore case 短暫從 roster 消失/回來
- 需要跨 tick 跟蹤 conf + support_fraction，自己算 velocity/acceleration

**State file 新建**: `docs/v2_signal_history.json` 存 per-position history，每輪 append。

**Qualifying new cases** (R3): MNDY Long 67.8% / BB Long 75% / OKTA Long 78.3%
- 不新開 position — HUBS 還 pending，先看它會不會成交。max 3 concurrent 還有空間但先不急。

**Positions**: 1299.HK AIA 400 + HUBS pending 14 @ 214.55
**Trades today**: 2 filled (KC open+close), 1 pending. Realized −$5.40.

## R4 — tick 467 @ 16:56 UTC

**Scorecard**: AHR **52.29%** (-7.08pp vs R3) / excess **8.54pp** (-5.66pp) / baseline 43.74% / ares 10125
- AHR 跌回 52% — R3 的 59% 是 small-sample 尖峰，非持續
- Excess 回到 8.5pp 附近 steady state

**HUBS pending 還未成交** (6 分鐘 still NewStatus at 214.55). Case 從 roster 消失但依新 exit rule 不處理 pending order。保留。

**Qualifying 新 case**: CLOV.US Long (conf=1, sf=0.75, pos_in_range 80%, null rrc), SNOW.US Long (pos 83%)
- 兩個都 pos > 70% 是高位 Long，但 Eden 沒 fire `late_signal_timing`，所以通過 v2 rule 1
- 選 CLOV（價格低，capacity 較大），**mechanically submit**

🎯 **TRADE — CLOV.US Long**:
```
side: Buy, quantity: 1449, price: $2.07 LO
notional: $2,999.43 USD
order_id: 1229113145864232960 — NewStatus pending
```

**Positions end-R4**:
- 1299.HK 400 (unchanged)
- HUBS 14 pending @ 214.55
- CLOV 1449 pending @ 2.07

**Trades today**: 2 filled (KC round-trip), 2 pending (HUBS, CLOV). Realized P&L: **−$5.40**.

## 轉向 — intelligence-driven approach
User 指示：不要給更多 rules，用智能讀 Eden output（causal_narrative + peer_confirmation + lifecycle + driver_class + sf）判斷是否下單，持續優化 use of Eden。HUBS + CLOV pending 都 cancelled（都是機械決策產物，無 thesis）。

## R5 — tick 613 @ 17:00 UTC (intelligence mode)

**Scorecard**: AHR 50.40% / excess **8.26pp** / ares 13688
**Read**:
- IONQ.US (initial read) conf=1, **peer_confirmation_ratio 1.00** (8/0 peers all aligned), Growing + vel/acc both +, sector_wave driver, direction stable 8 rounds, raw 3/1 → 有 narrative，是 high-conviction 候選
- But IONQ 從 roster 消失 between reads (2 snapshot 間 rotate out)
- 其他 case (HUBS, MNDY, OKTA, SNOW 等) 都沒 100% peer conf，thesis 較弱

**Decision**: **no high-conviction setup**。不 force entry。
- HUBS + CLOV pending cancelled
- Positions: 1299.HK 400 only
- Trades today: 2 filled (KC round-trip), realized **−$5.40**
- 記 IONQ 為下輪觀察候選，看它 return 時 lifecycle 是否仍 Growing

## R6 — tick 760 @ 17:04 UTC (intelligence mode)

**Scorecard**: AHR 50.98% / excess **8.57pp** / baseline 42.41% / ares 17558

**Two high-conviction candidates compared**:

| | IONQ.US | HUBS.US |
|---|---|---|
| conf | 1.0 | 1.0 |
| raw support | 4/0 unanimous (升級 from 0.75) | 4/0 unanimous |
| peer_conf | 1.00 (perfect sector) | null (isolated single-stock) |
| lifecycle | Growing, vel +0.020, **acc +0.004 (下降 from 0.02)** | null |
| driver | sector_wave | 無 |
| pos_in_range | **80.1% (chase)** | **54.3% (clean mid)** |
| rrc | null | stale_symbol_confirmation (v2 override) |

**Judgment**: IONQ 雖然 peer 100% 但 **acceleration 從 0.02 降到 0.0045** = 開始 peaking (signal 還強但增速放緩) + pos 80% = 追高。HUBS 無 sector narrative 但 raw 完全 unanimous + mid-range + 單股 alpha setup。**IONQ 是 beta 追漲、HUBS 是個股 alpha**。選 HUBS。

🎯 **TRADE — HUBS.US Long** (intelligence pick):
```
side: Buy, quantity: 14, price: $216.20 LO
notional: $3,026.80 USD
order_id: 1229115078977056768 (pending NewStatus)
```

Thesis: 4/4 raw sources (trade + quote + candlestick + calc_index) 全部支持 Long + day-range mid (54%) + direction stable 背景 + v2 stale override allowed. Exit 看 sf 是否跌破 0.75 或 confidence 下降加速。

**Positions end-R6**: 1299.HK 400 + HUBS 14 pending @ 216.20
**Trades today**: 2 filled (KC round-trip), 1 pending (HUBS new entry after cancel)
**Realized P&L**: −$5.40

## R7 — tick 927 @ 17:07 UTC (intelligence mode)

**Scorecard**: AHR 49.93% / excess **7.47pp** (縮 −1.10) / baseline 42.46% / ares 21749

**HUBS filled** @ $216.20 (order 1229115078977056768 FilledStatus 14 shares). Current $216.07 → unrealized **−$1.82**（3 分鐘基本平）。HUBS case 從 roster 消失本輪 — **per exit rule 忽略**，不 cover。

**New candidates reviewed**:
- **AMC.US Long**: 4/0 raw unanimous + peer 1.00 + Growing + vel/acc 都正 + sector_wave driver → fundamentals 強，但 pos 80.7% 追高 + meme volatility + 與已有 HUBS 都是 risk-on → **skip** (correlation + chase)
- **SNAP.US Long**: peer 1.00 + Growing + pos 25% (best entry of session) + sector_wave → entry position 最好，但 **rrc=freshness_decay_aging 是 v2 hard block**，never override → **skip**
- **GDDY.US Long**: sf 1.0 but pos **98.7% extreme chase** → skip
- Others (BB, GME, DKNG, CLOV, SNOW, PLTR): all `stale_symbol_confirmation` + null peer/lifecycle, weak conviction

**Decision**: **no high-conviction new entry**. HUBS hold.

**Positions end-R7**: 1299.HK 400 + HUBS 14 @ 216.20
**Trades today**: 3 filled (KC open+close, HUBS open), 0 pending
**Unrealized**: HUBS −$1.82 USD
**Realized**: −$5.40 USD
**Total P&L today**: **−$7.22 USD**

## R8 — tick 1042 @ 17:10 UTC

**Scorecard**: AHR **47.65%** / excess **5.76pp** / baseline 41.89% / ares 24296
- AHR 3 輪跌: 50.98 → 49.93 → 47.65 (−3.33pp)
- Excess 3 輪跌: 8.57 → 7.47 → 5.76 (−2.81pp)
- 整體 actionable tier 品質下降 — fresh resolved samples landing 不利

**HUBS**: $216.055, unrealized **−$2.03**. Case absent 2 rounds 連續，但 price 平，momentum data 不可得 → hold per new exit rule.

**Candidates all weak**:
- GDDY pos 93%, ADSK pos 87%, CRM pos 84%, SNPS pos 74% — 4 個追高無 peer/lifecycle data
- MS.US pos 53% mid-range but sf 3/1 + null peer + no lifecycle + observe-tier = 缺 narrative

**Decision**: **no qualifying setup**。HUBS hold，不 force 進場。

**Positions end-R8**: 1299.HK 400 + HUBS 14 @ 216.20 (unrealized −$2.03)
**Trades today**: 3 filled, 0 pending
**P&L today**: Realized −$5.40 + Unrealized −$2.03 = **−$7.43**

## R9 — tick 1161 @ 17:13 UTC 🎯 first real high-conviction entry

**Scorecard**: AHR 47.41% / excess **5.89pp** (微升 from 5.76) / baseline 41.53% / ares 26981

**HUBS**: price $215.58, unrealized **−$8.68**. Case absent 3 輪連續 but price flat (−0.3%), no momentum signal → hold.

**🎯 SNAP.US entry** — 本 session 第一個真 high-conviction 進場:

```
Fields (R9 tick 1161):
  conf: 1.0
  sf: 0.75 (3 raw supporting / 1 contradicting)
  peer_confirmation_ratio: 0.9681 (96.8% across sector)
  lifecycle: Growing
  velocity: +0.0021
  acceleration: +0.0388  ← 18× velocity, 強力加速
  pos_in_range: 31.25%   ← low-range Long entry, 非追高
  rrc: null              ← R7 曾被 freshness_decay_aging block，本輪清掉
  driver: sector_wave (ad/social tech)
```

**Why this is different** from IONQ/U/AMC (之前都 skip 的 high-conviction candidates):
- 前 3 個都 pos 80%+ chase，SNAP 在 pos 31% low-range
- 前 3 個 velocity > acceleration (peaking), SNAP acceleration >> velocity (剛起步)
- SNAP 是 **signal 加速增強初期** — 符合 user 的「加速增強 take profit、加速消失 stop」exit rule 的 entry 側

**Trade executed**:
```
side: Buy, quantity: 506, price: $5.925 (filled at executed_price 5.9250)
notional: $2,998.05 USD
order_id: 1229117526160502784 — FilledStatus
```

Exit triggers for SNAP:
- signal velocity + acceleration 雙負 → stop
- signal 加速後 acceleration 反轉負且價格已漲 → take profit
- unrealized < −$100 → hard stop (~$0.20 price drop → cover ≤ $5.72)

**Positions end-R9**:
- 1299.HK AIA 400 @ 83.35
- HUBS.US 14 @ 216.20 (−$8.68 unrealized)
- SNAP.US 506 @ 5.925 (flat, just filled)

**Trades today**: 4 filled (KC round-trip, HUBS open, SNAP open)
**Realized**: −$5.40
**Unrealized**: HUBS −$8.68 + SNAP $0 = −$8.68
**Total P&L**: **−$14.08**

## R10 — tick 1277 @ 17:16 UTC

**Scorecard**: AHR 47.09% / excess 5.32pp / ares 29577

**Positions update** (both 轉正):
- HUBS: $216.20 → **$216.49** = **+$4.06** unrealized ✅ (recovered from -$8.68 R9)
- SNAP: $5.925 → **$5.935** = **+$5.06** unrealized ✅ (3 分鐘 +0.17%)

Both cases absent from roster but per new exit rule hold。兩個都 green 證明不用對 case 消失 panic。

**No high-conviction new candidates** — 無 conf ≥ 0.9 + null rrc + peer ≥ 0.85 的 case。

**Positions end-R10**: 1299.HK 400 + HUBS 14 + SNAP 506
**Trades today**: 4 filled
**Realized**: −$5.40
**Unrealized**: HUBS +$4.06 + SNAP +$5.06 = +$9.12
**Total P&L today**: **+$3.72** 🟢 (首次今日轉正)

## R11 — tick 1394 @ 17:19 UTC

**Scorecard**: AHR 48.03% / excess 6.15pp / ares 32373 (輕微反彈 from R10)
- HUBS: $215.91 → **−$4.06** (flipped from +$4.06, $0.58 price noise)
- SNAP: $5.935 → **+$5.06** (unchanged)
- No conviction new candidates。Both positions case absent。
**Total P&L**: **−$4.40** (Realized −$5.40 + Unrealized +$1.00)

## R12 — tick 1509 @ 17:22 UTC
AHR 47.73 / excess 6.08 / ares 35114. HUBS $216.34 +$1.96, SNAP $5.935 +$5.06. Both cases absent. No conviction. **no qualifying setup**. Total: **+$1.62**

## R13 — tick 1622 @ 17:25 UTC 🟢
AHR 48.43 / excess 6.59 / ares 37592. **兩個都拉**: HUBS $216.87 **+$9.38**, SNAP $5.955 **+$15.18**. Both absent from roster. No conviction candidates. **no qualifying setup**. Total: **+$19.16** (Realized −$5.40 + Unrealized +$24.56) 🟢

## R14 — tick 1737 @ 17:28 UTC 🎯 DUOL 最強 conviction entry

**Scorecard**: AHR 49.04% / excess **6.80pp** / ares 39934

**Positions update**:
- HUBS $217.77 → **+$21.98** 🟢
- SNAP $5.944 → **+$9.61** (輕微回吐 from +$15.18)

**3 new conviction candidates**:
- **DUOL Long**: sf 4/0 unanimous, peer 98.9%, **vel 0.1022 / acc 0.1146** (50× SNAP 量級, acc > vel 仍在加速增強), pos 69.5% borderline
- DLTR Long: sf 0.75, peer 93.5%, vel/acc 小一個量級, pos 64.3%
- OTIS Short pos 33.9% = wrong side for Short (要 high 進), skip

**選 DUOL** — 信號量級太明顯，acceleration > velocity 意味 move 還在 accelerating phase。

🎯 **TRADE — DUOL.US Long**:
```
side: Buy, quantity: 31, price: $96.67 (executed_price 96.6700)
notional: $2,996.77
order_id: 1229121327374319616 — FilledStatus
```

Thesis: Duolingo 可能有 catalyst，+5.7% day + 98.9% peer 暗示 sector-wide edu-tech 動。velocity 0.10 + accel 0.11 是本 session 最強 momentum signature。

**Positions end-R14**:
- 1299.HK AIA 400
- HUBS 14 @ 216.20 (+$21.98)
- SNAP 506 @ 5.925 (+$9.61)
- DUOL 31 @ 96.67 (flat just filled)

**Trades today**: 5 filled
**Realized**: −$5.40
**Unrealized**: +$31.59
**Total P&L**: **+$26.19** 🟢

## R15 — tick 1852 @ 17:31 UTC

**Scorecard**: AHR 49.11% / excess 6.69pp / ares 42126
**Positions**: HUBS +$22.82, SNAP +$5.06, DUOL -$0.31
**Candidate — GME Long**: vel 0.2492 ≈ acc 0.2498 (steady max momentum, acc 追上 vel = 不再加速), pos 73.4% chase, meme vol, 已 3 US positions → **skip**
**Decision**: no new entry, hold 3 US + HK
**Total P&L**: **+$22.17** 🟢 (Realized −$5.40 + Unrealized +$27.57)

## R16 — tick 1948 @ 17:34 UTC
AHR 48.88 / excess 6.43 / ares 43995. HUBS +$22.54, SNAP +$7.59, DUOL +$1.55. No conviction candidates. **no qualifying setup**. Total: **+$26.28** 🟢

## R17 — tick 2055 @ 17:37 UTC
AHR 48.85 / excess 6.03 / ares 45786. HUBS **+$28.49** (new high), SNAP +$2.53, DUOL **+$6.82**. No conviction. **no qualifying setup**. Total: **+$32.44** 🟢

## R18 — tick 2164 @ 17:40 UTC
AHR 48.65 / excess 5.94 / ares 47589. HUBS **+$29.33** (new high), SNAP +$2.53, DUOL +$3.10. No conviction. **no qualifying setup**. Total: **+$29.56** 🟢

## R19 — tick 2272 @ 17:43 UTC
AHR 48.46 / excess 5.65 / ares 49196. HUBS **+$33.95** (new high), SNAP +$5.06, DUOL +$1.55. No conviction. **no qualifying setup**. Total: **+$35.16** 🟢

## R20 — tick 2378 @ 17:46 UTC
AHR 48.10 / excess 5.37 / ares 50992. HUBS +$23.03 (pulled from +$33.95), SNAP +$5.06, DUOL **+$7.44** (new high). No conviction. **no qualifying setup**. Total: **+$30.13** 🟢

## R21 — tick 2485 @ 17:49 UTC
AHR 47.49 / excess 4.93 / ares 52782. HUBS +$20.16, SNAP **+$11.64** (new high), DUOL +$4.96. No conviction. **no qualifying setup**. Total: **+$31.36** 🟢

## R22 — tick 2593 @ 17:52 UTC 🟢 SNAP rally
AHR 47.08 / excess 4.69 / ares 54237. HUBS +$11.06, **SNAP $6.015 +$45.54** (breakout through $6, +34 in 3min), DUOL +$5.58.
Candidate RKLB Long pos 39.5% peer 98.8% but vel/acc 30× 小於 DUOL — skip (magnitude + 滿倉). **no qualifying setup**. Total: **+$56.78** 🟢🟢

## R23 — tick 2698 @ 17:55 UTC
AHR 46.78 / excess 4.41 / ares 56020. HUBS +$16.35, SNAP +$25.30 (pulled from +$45.54), DUOL +$4.50. No conviction. **no qualifying setup**. Total: **+$40.75** 🟢

## R24 — tick 2804 @ 17:58 UTC
AHR 46.28 / excess 4.06 / ares 57685. HUBS +$18.20, SNAP +$25.30, DUOL **−$1.55** (首次 red). No conviction. **no qualifying setup**. Total: **+$36.55** 🟢

## R25 — tick 2910 @ 18:01 UTC
AHR 46.29 / excess 4.02 / ares 59217. HUBS +$24.29, SNAP +$22.77, DUOL **−$5.58** (slowly bleeding). No conviction. **no qualifying setup**. Total: **+$36.08** 🟢

## R26 — tick 3005 @ 18:04 UTC 🚀 session high
AHR 46.56 / excess 4.04 / ares 61027. **HUBS $219.77 +$49.98 (new high!)**, SNAP +$40.48, DUOL −$0.16 (recovered). No conviction. **no qualifying setup**. Total: **+$84.90** 🟢🟢🟢 (session best)

## R27 — tick 3108 @ 18:07 UTC 🚀 new session high
AHR 46.75 / excess 4.06 / ares 63042. HUBS +$43.96, **SNAP $6.020 +$48.07** (new high), DUOL +$4.34. No conviction. **no qualifying setup**. Total: **+$90.97** 🟢🟢🟢

## R28 — tick 3212 @ 18:10 UTC 🚀🚀 ALL THREE new highs
AHR 47.51 / excess 4.54 / ares 65323. **HUBS $220.17 +$55.58** (new high), **SNAP $6.036 +$56.17** (new high), **DUOL $97.01 +$10.54** (new high). No conviction. **no qualifying setup**. Total: **+$116.89** 🟢🟢🟢🟢
