#!/usr/bin/env python3
"""Eden Signal Evaluator — analyze US live snapshots for alpha evidence."""

import json
import glob
import os
from collections import Counter, defaultdict

DATA_DIR = os.path.join(os.path.dirname(os.path.dirname(__file__)), "data")

def main():
    files = sorted(glob.glob(os.path.join(DATA_DIR, "us_live_snapshot*.json")))
    print(f"Loading {len(files)} US live snapshots...")

    snapshots = []
    for f in files:
        with open(f) as fh:
            snapshots.append(json.load(fh))
    snapshots.sort(key=lambda x: x.get("tick", 0))

    print(f"Ticks: {snapshots[0]['tick']}..{snapshots[-1]['tick']}")
    print(f"Time:  {snapshots[0]['timestamp'][:19]}..{snapshots[-1]['timestamp'][:19]}")
    print()

    # ── 1. All unique setups ──
    setups = {}
    for snap in snapshots:
        for c in snap.get("tactical_cases", []):
            sid = c.get("setup_id", "")
            if sid and sid not in setups:
                setups[sid] = {
                    "symbol": c.get("symbol", ""),
                    "action": c.get("action", ""),
                    "family": c.get("family_label", "") or c.get("family_key", ""),
                    "confidence": c.get("confidence", "0"),
                    "tick": snap["tick"],
                    "time": snap["timestamp"][:19],
                }

    print(f"═══ {len(setups)} UNIQUE SETUPS ═══\n")

    # ── 2. By family ──
    families = Counter(s["family"] for s in setups.values())
    print("BY FAMILY:")
    for fam, n in families.most_common(15):
        print(f"  {fam:45s} {n}")

    # ── 3. Funnel ──
    actions = Counter(s["action"] for s in setups.values())
    print(f"\nFUNNEL:")
    total = sum(actions.values())
    for a in ["observe", "review", "enter", "exit"]:
        n = actions.get(a, 0)
        pct = n / total * 100 if total else 0
        bar = "█" * int(pct / 2)
        print(f"  {a:10s} {n:5d} ({pct:5.1f}%) {bar}")

    # ── 4. Top symbols ──
    symbols = Counter(s["symbol"] for s in setups.values() if s["symbol"])
    print(f"\nTOP SYMBOLS:")
    for sym, n in symbols.most_common(20):
        print(f"  {sym:12s} {n}")

    # ── 5. Convergence scores across snapshots ──
    print(f"\n═══ CONVERGENCE SCORE ANALYSIS ═══\n")
    symbol_scores = defaultdict(list)
    for snap in snapshots:
        scores = snap.get("convergence_scores", {})
        if isinstance(scores, list):
            for s in scores:
                symbol_scores[s.get("symbol", "")].append(float(s.get("composite", 0)))
        elif isinstance(scores, dict):
            for sym, s in scores.items():
                val = float(s.get("composite", 0)) if isinstance(s, dict) else float(s)
                symbol_scores[sym].append(val)

    # Top by mean convergence
    ranked = []
    for sym, vals in symbol_scores.items():
        if len(vals) >= 3:
            ranked.append((sym, sum(vals)/len(vals), max(vals), min(vals), len(vals)))
    ranked.sort(key=lambda x: -x[1])

    print("TOP 20 BY MEAN CONVERGENCE:")
    for sym, mean, mx, mn, n in ranked[:20]:
        print(f"  {sym:12s} mean={mean:.4f} max={mx:.4f} min={mn:.4f} n={n}")

    # ── 6. Scorecard history ──
    print(f"\n═══ SCORECARD HISTORY ═══\n")
    for snap in snapshots:
        sc = snap.get("scorecard", {})
        resolved = sc.get("resolved_signals", 0)
        if resolved > 0:
            print(f"  tick={snap['tick']} signals={sc.get('total_signals',0)} resolved={resolved} hits={sc.get('hits',0)} misses={sc.get('misses',0)} hit_rate={sc.get('hit_rate','?')}")
    if all(snap.get("scorecard", {}).get("resolved_signals", 0) == 0 for snap in snapshots):
        print("  No resolved signals in any snapshot (scorecard always 0/0)")

    # ── 7. Cross-snapshot setup tracking ──
    print(f"\n═══ SETUP PERSISTENCE ═══\n")
    setup_appearances = defaultdict(int)
    for snap in snapshots:
        for c in snap.get("tactical_cases", []):
            setup_appearances[c.get("setup_id", "")] += 1
    persistence = Counter(setup_appearances.values())
    print("How many snapshots each setup appears in:")
    for n_appearances, count in sorted(persistence.items()):
        print(f"  {n_appearances} snapshots: {count} setups")

    # ── 8. Active positions ──
    print(f"\n═══ ACTIVE POSITIONS ═══\n")
    all_positions = []
    for snap in snapshots:
        for pos in snap.get("active_positions", []):
            all_positions.append((snap["tick"], pos))
    if all_positions:
        seen = set()
        for tick, pos in all_positions:
            key = json.dumps(pos) if isinstance(pos, dict) else str(pos)
            if key not in seen:
                seen.add(key)
                print(f"  tick={tick}: {str(pos)[:100]}")
    else:
        print("  No positions tracked across any snapshot")

if __name__ == "__main__":
    main()
