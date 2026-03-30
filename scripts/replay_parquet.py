#!/usr/bin/env python3
"""Replay Parquet tick data through Eden's pipeline.

Converts Parquet files to TickArchive JSON, then calls Eden's replay pipeline.
Can run on live-recording data (watches for new files) or on historical data.

Usage:
    # Replay historical data
    python scripts/replay_parquet.py --dir "/Volumes/LaCie 1/eden/hk-ticks-full"

    # Live mode: watch for new files while recorder runs
    python scripts/replay_parquet.py --dir "/Volumes/LaCie 1/eden/hk-ticks-full" --live

    # Limit ticks
    python scripts/replay_parquet.py --dir "/Volumes/LaCie 1/eden/hk-ticks-full" --limit 50
"""

import json
import os
import re
import signal
import sys
import time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import pyarrow.parquet as pq

DEFAULT_DIR = "/Volumes/LaCie 1/eden/hk-ticks-full"


def get_timestamp_keys(session_dir):
    """Extract unique (session_id, unix_ts) from filenames, sorted by time."""
    keys = set()
    for f in os.listdir(session_dir):
        if not f.endswith(".parquet") or f.startswith("._"):
            continue
        parts = f.replace(".parquet", "").split("_")
        if len(parts) >= 3:
            keys.add((parts[1], parts[2]))
    return sorted(keys, key=lambda k: int(k[1]))


def find_file(session_dir, prefix, ts_key):
    pattern = f"{prefix}_{ts_key}"
    for f in os.listdir(session_dir):
        if f.startswith(pattern) and f.endswith(".parquet") and not f.startswith("._"):
            return os.path.join(session_dir, f)
    return None


def safe_read(path):
    """Read parquet, skip if file is still being written."""
    try:
        if time.time() - os.path.getmtime(path) < 2:
            return None
        return pq.read_table(path).to_pandas()
    except Exception:
        return None


def build_tick_archive(session_dir, ts_key, tick_number):
    """Convert one set of Parquet files into a TickArchive dict."""
    now_iso = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")

    archive = {
        "tick_number": tick_number,
        "timestamp": now_iso,
        "quotes": [],
        "order_books": [],
        "broker_queues": [],
        "trades": [],
        "candlesticks": [],
        "capital_flows": [],
        "capital_distributions": [],
    }

    # Quotes — keep latest per symbol
    f = find_file(session_dir, "quotes", ts_key)
    if f:
        df = safe_read(f)
        if df is not None and not df.empty:
            archive["timestamp"] = df.ts.max().isoformat().replace("+00:00", "Z")
            latest = df.sort_values("ts").drop_duplicates("symbol", keep="last")
            for _, r in latest.iterrows():
                archive["quotes"].append({
                    "symbol": r["symbol"],
                    "timestamp": r["ts"].isoformat().replace("+00:00", "Z"),
                    "last_done": str(r["last_done"]),
                    "prev_close": "0",
                    "open": str(r["open"]),
                    "high": str(r["high"]),
                    "low": str(r["low"]),
                    "volume": int(r["volume"]),
                    "turnover": str(r["turnover"]),
                })

    # Depths → order_books — keep latest per (symbol, side, position)
    f = find_file(session_dir, "depths", ts_key)
    if f:
        df = safe_read(f)
        if df is not None and not df.empty:
            latest = df.sort_values("ts").drop_duplicates(
                ["symbol", "side", "position"], keep="last"
            )
            by_sym = defaultdict(lambda: {"asks": [], "bids": []})
            for _, r in latest.iterrows():
                level = {
                    "position": int(r["position"]),
                    "price": float(r["price"]) if r["price"] else None,
                    "volume": int(r["volume"]),
                    "order_num": int(r["order_count"]),
                }
                key = "asks" if r["side"] == "ask" else "bids"
                by_sym[r["symbol"]][key].append(level)

            for sym, data in by_sym.items():
                archive["order_books"].append({
                    "symbol": sym,
                    "timestamp": archive["timestamp"],
                    "ask_levels": sorted(data["asks"], key=lambda l: l["position"]),
                    "bid_levels": sorted(data["bids"], key=lambda l: l["position"]),
                })

    # Brokers → broker_queues — keep latest per (symbol, side, position, broker_id)
    f = find_file(session_dir, "brokers", ts_key)
    if f:
        df = safe_read(f)
        if df is not None and not df.empty:
            latest = df.sort_values("ts").drop_duplicates(
                ["symbol", "side", "position", "broker_id"], keep="last"
            )
            for _, r in latest.iterrows():
                archive["broker_queues"].append({
                    "symbol": r["symbol"],
                    "broker_id": int(r["broker_id"]),
                    "side": r["side"],
                    "position": int(r["position"]),
                })

    # Trades
    f = find_file(session_dir, "trades", ts_key)
    if f:
        df = safe_read(f)
        if df is not None and not df.empty:
            for _, r in df.iterrows():
                d = str(r.get("direction", ""))
                direction = "Down" if "Down" in d else ("Neutral" if "Neutral" in d else "Up")
                archive["trades"].append({
                    "symbol": r["symbol"],
                    "timestamp": r["ts"].isoformat().replace("+00:00", "Z"),
                    "price": str(r["price"]),
                    "volume": int(r["volume"]),
                    "direction": direction,
                    "session": "Normal",
                    "trade_type": "",
                })

    # Capital distribution
    f = find_file(session_dir, "capital_dist", ts_key)
    if f:
        df = safe_read(f)
        if df is not None and not df.empty:
            latest = df.sort_values("ts").drop_duplicates("symbol", keep="last")
            for _, r in latest.iterrows():
                archive["capital_distributions"].append({
                    "symbol": r["symbol"],
                    "timestamp": archive["timestamp"],
                    "large_in": str(r["large_in"]),
                    "large_out": str(r["large_out"]),
                    "medium_in": str(r["medium_in"]),
                    "medium_out": str(r["medium_out"]),
                    "small_in": str(r["small_in"]),
                    "small_out": str(r["small_out"]),
                })

    return archive


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Replay Parquet tick data through Eden")
    parser.add_argument("--dir", default=DEFAULT_DIR, help="Parquet data directory")
    parser.add_argument("--date", default=None, help="Date subdirectory (default: today)")
    parser.add_argument("--limit", type=int, default=0, help="Max ticks (0=all)")
    parser.add_argument("--live", action="store_true", help="Watch for new files")
    parser.add_argument("--out", default="data/parquet_replay", help="Output directory for archives")
    args = parser.parse_args()

    date_str = args.date or datetime.now().strftime("%Y-%m-%d")
    session_dir = os.path.join(args.dir, date_str)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    print("=== Eden Parquet Replay ===")
    print(f"Source:  {session_dir}")
    print(f"Output:  {out_dir}")
    print(f"Mode:    {'live (watching)' if args.live else 'batch'}")
    if args.limit:
        print(f"Limit:   {args.limit} ticks")
    print()

    if not os.path.isdir(session_dir):
        print(f"ERROR: {session_dir} not found")
        sys.exit(1)

    all_keys = get_timestamp_keys(session_dir)
    print(f"Found {len(all_keys)} file sets in {session_dir}")

    # Stats
    total_quotes = 0
    total_orderbooks = 0
    total_brokers = 0
    total_trades = 0
    total_capital = 0
    symbols_seen = set()

    shutdown = False
    def handle_signal(sig, frame):
        nonlocal shutdown
        shutdown = True
    signal.signal(signal.SIGINT, handle_signal)
    signal.signal(signal.SIGTERM, handle_signal)

    processed = set()
    tick = 0

    if not args.live:
        # Batch mode: process all existing keys
        keys_to_process = all_keys
        if args.limit:
            keys_to_process = keys_to_process[:args.limit]

        for session_id, unix_ts in keys_to_process:
            if shutdown:
                break
            tick += 1
            ts_key = f"{session_id}_{unix_ts}"

            archive = build_tick_archive(session_dir, ts_key, tick)

            # Write JSON for Eden replay binary
            archive_path = out_dir / f"tick_{tick:06d}.json"
            with open(archive_path, "w") as f:
                json.dump(archive, f, separators=(",", ":"))

            nq = len(archive["quotes"])
            nob = len(archive["order_books"])
            nbq = len(archive["broker_queues"])
            ntr = len(archive["trades"])
            ncd = len(archive["capital_distributions"])
            total_quotes += nq
            total_orderbooks += nob
            total_brokers += nbq
            total_trades += ntr
            total_capital += ncd
            for q in archive["quotes"]:
                symbols_seen.add(q["symbol"])

            if tick <= 5 or tick % 10 == 0:
                print(f"  tick {tick:>4}: q={nq:<4} ob={nob:<4} bq={nbq:<6} tr={ntr:<5} cd={ncd:<3} | {ts_key}")

        print()
        print("=== Replay Complete ===")
        print(f"Ticks:         {tick}")
        print(f"Symbols:       {len(symbols_seen)}")
        print(f"Quotes:        {total_quotes:,}")
        print(f"Order books:   {total_orderbooks:,}")
        print(f"Broker queue:  {total_brokers:,}")
        print(f"Trades:        {total_trades:,}")
        print(f"Capital dist:  {total_capital:,}")
        print(f"Archives in:   {out_dir}")

        # Summary of data size
        json_files = list(out_dir.glob("tick_*.json"))
        total_size = sum(f.stat().st_size for f in json_files)
        print(f"JSON files:    {len(json_files)} ({total_size / 1024 / 1024:.1f} MB)")

    else:
        # Live mode: watch for new files
        for key in all_keys:
            processed.add(key)
        print(f"Skipping {len(processed)} existing sets. Watching for new data...")
        print()

        while not shutdown:
            time.sleep(10)
            current_keys = get_timestamp_keys(session_dir)
            new_keys = [k for k in current_keys if k not in processed]

            for session_id, unix_ts in new_keys:
                if shutdown:
                    break
                if args.limit and tick >= args.limit:
                    shutdown = True
                    break

                tick += 1
                ts_key = f"{session_id}_{unix_ts}"

                archive = build_tick_archive(session_dir, ts_key, tick)

                archive_path = out_dir / f"tick_{tick:06d}.json"
                with open(archive_path, "w") as f:
                    json.dump(archive, f, separators=(",", ":"))

                nq = len(archive["quotes"])
                nbq = len(archive["broker_queues"])
                ntr = len(archive["trades"])
                print(f"  tick {tick}: q={nq} bq={nbq} tr={ntr} → {archive_path.name}")

                processed.add((session_id, unix_ts))

        print(f"\nLive replay stopped after {tick} ticks.")


if __name__ == "__main__":
    main()
