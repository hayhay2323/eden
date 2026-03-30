#!/usr/bin/env python3
"""Record all HK market data to Parquet: WebSocket ticks + REST snapshots."""

import os
import re
import signal
import time
import threading
from datetime import datetime, timezone
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq
from longport.openapi import (
    Config, QuoteContext, SubType, Market, CalcIndex,
    PushQuote, PushDepth, PushBrokers, PushTrades,
)

# ── Config ──────────────────────────────────────────────────────────────────

DEFAULT_OUT = "/Volumes/LaCie 1/eden/hk-ticks-full"
FLUSH_INTERVAL = 60       # seconds between parquet flushes
REST_POLL_INTERVAL = 120  # seconds between REST snapshots
REST_BATCH_SIZE = 50      # symbols per REST API call


def load_watchlist():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    watchlist_path = os.path.join(script_dir, "..", "src", "hk", "watchlist.rs")
    with open(watchlist_path) as f:
        content = f.read()
    syms = re.findall(r'"(\d+\.HK)"', content)
    seen = set()
    unique = []
    for s in syms:
        if s not in seen:
            seen.add(s)
            unique.append(s)
    return unique


WATCHLIST = load_watchlist()

CALC_INDEXES = [
    CalcIndex.LastDone, CalcIndex.ChangeRate, CalcIndex.Volume,
    CalcIndex.Turnover, CalcIndex.TurnoverRate, CalcIndex.VolumeRatio,
    CalcIndex.FiveMinutesChangeRate, CalcIndex.Amplitude,
    CalcIndex.PeTtmRatio, CalcIndex.PbRatio, CalcIndex.DividendRatioTtm,
    CalcIndex.TotalMarketValue, CalcIndex.CapitalFlow,
]


# ── Buffers ─────────────────────────────────────────────────────────────────

class TickBuffers:
    def __init__(self):
        # WebSocket push data
        self.quotes = []
        self.depths = []
        self.brokers = []
        self.trades = []
        # REST snapshot data
        self.capital_dist = []
        self.capital_flow = []
        self.calc_indexes = []
        self.market_temp = []
        self.intraday = []
        self.start_time = time.time()

    def ws_count(self):
        return len(self.quotes) + len(self.depths) + len(self.brokers) + len(self.trades)

    def rest_count(self):
        return (len(self.capital_dist) + len(self.capital_flow) +
                len(self.calc_indexes) + len(self.market_temp) + len(self.intraday))

    def count(self):
        return self.ws_count() + self.rest_count()


# ── Parquet Schemas ─────────────────────────────────────────────────────────

QUOTE_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("received_at", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("last_done", pa.float64()),
    ("open", pa.float64()),
    ("high", pa.float64()),
    ("low", pa.float64()),
    ("volume", pa.int64()),
    ("turnover", pa.float64()),
    ("timestamp", pa.string()),
])

DEPTH_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("received_at", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("side", pa.string()),
    ("position", pa.int32()),
    ("price", pa.float64()),
    ("volume", pa.int64()),
    ("order_count", pa.int32()),
])

BROKER_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("received_at", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("side", pa.string()),
    ("position", pa.int32()),
    ("broker_id", pa.int32()),
])

TRADE_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("received_at", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("price", pa.float64()),
    ("volume", pa.int64()),
    ("direction", pa.string()),
    ("trade_type", pa.string()),
    ("trade_session", pa.int32()),
])

CAPITAL_DIST_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("data_timestamp", pa.string()),
    ("large_in", pa.float64()),
    ("large_out", pa.float64()),
    ("medium_in", pa.float64()),
    ("medium_out", pa.float64()),
    ("small_in", pa.float64()),
    ("small_out", pa.float64()),
])

CAPITAL_FLOW_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("flow_timestamp", pa.string()),
    ("inflow", pa.float64()),
])

CALC_INDEX_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("last_done", pa.float64()),
    ("change_rate", pa.float64()),
    ("volume", pa.int64()),
    ("turnover", pa.float64()),
    ("turnover_rate", pa.float64()),
    ("volume_ratio", pa.float64()),
    ("five_min_change_rate", pa.float64()),
    ("amplitude", pa.float64()),
    ("pe_ttm", pa.float64()),
    ("pb", pa.float64()),
    ("dividend_yield", pa.float64()),
    ("total_market_value", pa.float64()),
    ("capital_flow", pa.float64()),
])

MARKET_TEMP_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("temperature", pa.int32()),
    ("description", pa.string()),
    ("valuation", pa.int32()),
    ("sentiment", pa.int32()),
    ("data_timestamp", pa.string()),
])

INTRADAY_SCHEMA = pa.schema([
    ("ts", pa.timestamp("us", tz="UTC")),
    ("symbol", pa.string()),
    ("price", pa.float64()),
    ("volume", pa.int64()),
    ("turnover", pa.float64()),
    ("avg_price", pa.float64()),
    ("data_timestamp", pa.string()),
])


# ── WebSocket Handlers ─────────────────────────────────────────────────────

def make_quote_handler(buffers):
    def on_quote(symbol, event):
        now = datetime.now(timezone.utc)
        buffers.quotes.append({
            "ts": now, "received_at": now, "symbol": symbol,
            "last_done": float(event.last_done), "open": float(event.open),
            "high": float(event.high), "low": float(event.low),
            "volume": int(event.volume), "turnover": float(event.turnover),
            "timestamp": str(event.timestamp) if hasattr(event, "timestamp") else "",
        })
    return on_quote


def make_depth_handler(buffers):
    def on_depth(symbol, event):
        now = datetime.now(timezone.utc)
        for i, ask in enumerate(event.asks):
            buffers.depths.append({
                "ts": now, "received_at": now, "symbol": symbol,
                "side": "ask", "position": i, "price": float(ask.price),
                "volume": int(ask.volume), "order_count": int(ask.order_num),
            })
        for i, bid in enumerate(event.bids):
            buffers.depths.append({
                "ts": now, "received_at": now, "symbol": symbol,
                "side": "bid", "position": i, "price": float(bid.price),
                "volume": int(bid.volume), "order_count": int(bid.order_num),
            })
    return on_depth


def make_broker_handler(buffers):
    def on_brokers(symbol, event):
        now = datetime.now(timezone.utc)
        for i, bid_brokers in enumerate(event.bid_brokers):
            for broker_id in bid_brokers.broker_ids:
                buffers.brokers.append({
                    "ts": now, "received_at": now, "symbol": symbol,
                    "side": "bid", "position": i, "broker_id": int(broker_id),
                })
        for i, ask_brokers in enumerate(event.ask_brokers):
            for broker_id in ask_brokers.broker_ids:
                buffers.brokers.append({
                    "ts": now, "received_at": now, "symbol": symbol,
                    "side": "ask", "position": i, "broker_id": int(broker_id),
                })
    return on_brokers


def make_trade_handler(buffers):
    def on_trades(symbol, event):
        now = datetime.now(timezone.utc)
        for trade in event.trades:
            buffers.trades.append({
                "ts": now, "received_at": now, "symbol": symbol,
                "price": float(trade.price), "volume": int(trade.volume),
                "direction": str(trade.direction) if hasattr(trade, "direction") else "",
                "trade_type": str(trade.trade_type) if hasattr(trade, "trade_type") else "",
                "trade_session": int(trade.trade_session) if hasattr(trade, "trade_session") else 0,
            })
    return on_trades


# ── REST Pollers ───────────────────────────────────────────────────────────

def poll_rest_data(ctx, buffers, watchlist, shutdown_event):
    """Background thread: poll REST APIs every REST_POLL_INTERVAL seconds."""
    while not shutdown_event.is_set():
        now = datetime.now(timezone.utc)
        polled = 0

        # 1. Capital distribution (per symbol)
        for i in range(0, len(watchlist), REST_BATCH_SIZE):
            if shutdown_event.is_set():
                break
            batch = watchlist[i:i + REST_BATCH_SIZE]
            for sym in batch:
                try:
                    cd = ctx.capital_distribution(sym)
                    buffers.capital_dist.append({
                        "ts": now, "symbol": sym,
                        "data_timestamp": str(cd.timestamp),
                        "large_in": float(cd.capital_in.large),
                        "large_out": float(cd.capital_out.large),
                        "medium_in": float(cd.capital_in.medium),
                        "medium_out": float(cd.capital_out.medium),
                        "small_in": float(cd.capital_in.small),
                        "small_out": float(cd.capital_out.small),
                    })
                    polled += 1
                except Exception:
                    pass
            time.sleep(0.5)  # rate limit between batches

        # 2. Capital flow (per symbol, just latest point)
        for sym in watchlist[:100]:  # top 100 only to avoid rate limits
            if shutdown_event.is_set():
                break
            try:
                flows = ctx.capital_flow(sym)
                if flows:
                    latest = flows[-1]
                    buffers.capital_flow.append({
                        "ts": now, "symbol": sym,
                        "flow_timestamp": str(latest.timestamp),
                        "inflow": float(latest.inflow),
                    })
                    polled += 1
            except Exception:
                pass
        time.sleep(0.2)

        # 3. Calc indexes (batched)
        for i in range(0, len(watchlist), REST_BATCH_SIZE):
            if shutdown_event.is_set():
                break
            batch = watchlist[i:i + REST_BATCH_SIZE]
            try:
                results = ctx.calc_indexes(batch, CALC_INDEXES)
                for item in results:
                    buffers.calc_indexes.append({
                        "ts": now, "symbol": item.symbol,
                        "last_done": float(item.last_done) if item.last_done else 0.0,
                        "change_rate": float(item.change_rate) if item.change_rate else 0.0,
                        "volume": int(item.volume) if item.volume else 0,
                        "turnover": float(item.turnover) if item.turnover else 0.0,
                        "turnover_rate": float(item.turnover_rate) if item.turnover_rate else 0.0,
                        "volume_ratio": float(item.volume_ratio) if item.volume_ratio else 0.0,
                        "five_min_change_rate": float(item.five_minutes_change_rate) if item.five_minutes_change_rate else 0.0,
                        "amplitude": float(item.amplitude) if item.amplitude else 0.0,
                        "pe_ttm": float(item.pe_ttm_ratio) if item.pe_ttm_ratio else 0.0,
                        "pb": float(item.pb_ratio) if item.pb_ratio else 0.0,
                        "dividend_yield": float(item.dividend_ratio_ttm) if item.dividend_ratio_ttm else 0.0,
                        "total_market_value": float(item.total_market_value) if item.total_market_value else 0.0,
                        "capital_flow": float(item.capital_flow) if item.capital_flow else 0.0,
                    })
                    polled += 1
            except Exception:
                pass
            time.sleep(0.3)

        # 4. Market temperature
        try:
            mt = ctx.market_temperature(Market.HK)
            buffers.market_temp.append({
                "ts": now,
                "temperature": int(mt.temperature),
                "description": str(mt.description),
                "valuation": int(mt.valuation),
                "sentiment": int(mt.sentiment),
                "data_timestamp": str(mt.timestamp),
            })
            polled += 1
        except Exception:
            pass

        # 5. Intraday lines (top 50 symbols only)
        for sym in watchlist[:50]:
            if shutdown_event.is_set():
                break
            try:
                lines = ctx.intraday(sym)
                if lines:
                    latest = lines[-1]
                    buffers.intraday.append({
                        "ts": now, "symbol": sym,
                        "price": float(latest.price),
                        "volume": int(latest.volume),
                        "turnover": float(latest.turnover),
                        "avg_price": float(latest.avg_price),
                        "data_timestamp": str(latest.timestamp),
                    })
                    polled += 1
            except Exception:
                pass
        time.sleep(0.2)

        print(f"  [REST] Polled {polled} data points")

        # Wait for next poll interval
        for _ in range(REST_POLL_INTERVAL):
            if shutdown_event.is_set():
                break
            time.sleep(1)


# ── Flush to Parquet ────────────────────────────────────────────────────────

def flush_buffers(buffers, out_dir, session_id):
    flushed = 0
    ts = int(time.time())

    def write(name, data, schema):
        nonlocal flushed
        if data:
            table = pa.Table.from_pylist(data, schema=schema)
            path = out_dir / f"{name}_{session_id}_{ts}.parquet"
            pq.write_table(table, path, compression="zstd")
            n = len(data)
            data.clear()
            flushed += n

    write("quotes", buffers.quotes, QUOTE_SCHEMA)
    write("depths", buffers.depths, DEPTH_SCHEMA)
    write("brokers", buffers.brokers, BROKER_SCHEMA)
    write("trades", buffers.trades, TRADE_SCHEMA)
    write("capital_dist", buffers.capital_dist, CAPITAL_DIST_SCHEMA)
    write("capital_flow", buffers.capital_flow, CAPITAL_FLOW_SCHEMA)
    write("calc_indexes", buffers.calc_indexes, CALC_INDEX_SCHEMA)
    write("market_temp", buffers.market_temp, MARKET_TEMP_SCHEMA)
    write("intraday", buffers.intraday, INTRADAY_SCHEMA)

    return flushed


# ── Main ────────────────────────────────────────────────────────────────────

def main():
    import argparse
    parser = argparse.ArgumentParser(description="Record HK tick + REST data to Parquet")
    parser.add_argument("--out", default=DEFAULT_OUT, help="Output directory")
    args = parser.parse_args()

    out_dir = Path(args.out)
    today = datetime.now().strftime("%Y-%m-%d")
    session_dir = out_dir / today
    session_dir.mkdir(parents=True, exist_ok=True)
    session_id = datetime.now().strftime("%H%M%S")

    print("=== Eden HK Full Recorder ===")
    print(f"Output:    {session_dir}")
    print(f"Symbols:   {len(WATCHLIST)}")
    print(f"WS flush:  every {FLUSH_INTERVAL}s")
    print(f"REST poll: every {REST_POLL_INTERVAL}s")
    print(f"Channels:  Quote + Depth + Broker + Trade (WS)")
    print(f"           CapitalDist + CapitalFlow + CalcIndex + MarketTemp + Intraday (REST)")
    print()

    config = Config.from_env()
    ctx = QuoteContext(config)
    buffers = TickBuffers()

    # Register WebSocket handlers
    ctx.set_on_quote(make_quote_handler(buffers))
    ctx.set_on_depth(make_depth_handler(buffers))
    ctx.set_on_brokers(make_broker_handler(buffers))
    ctx.set_on_trades(make_trade_handler(buffers))

    # Subscribe WebSocket
    print("Subscribing WebSocket...")
    sub_types = [SubType.Quote, SubType.Depth, SubType.Brokers, SubType.Trade]
    batch_size = 50
    for i in range(0, len(WATCHLIST), batch_size):
        batch = WATCHLIST[i:i + batch_size]
        ctx.subscribe(batch, sub_types)
        print(f"  Subscribed batch {i // batch_size + 1}: {len(batch)} symbols")

    print(f"Subscribed to {len(WATCHLIST)} symbols x 4 WS channels.")
    print()

    # Start REST poller thread
    shutdown_event = threading.Event()
    rest_thread = threading.Thread(
        target=poll_rest_data,
        args=(ctx, buffers, WATCHLIST, shutdown_event),
        daemon=True,
    )
    rest_thread.start()
    print("REST poller started.")
    print("Recording... (Ctrl+C to stop)")
    print()

    # Graceful shutdown
    def handle_signal(sig, frame):
        shutdown_event.set()
        print("\nShutting down...")

    signal.signal(signal.SIGINT, handle_signal)
    signal.signal(signal.SIGTERM, handle_signal)

    total_flushed = 0
    flush_count = 0

    try:
        while not shutdown_event.is_set():
            time.sleep(FLUSH_INTERVAL)
            if buffers.count() > 0:
                n = flush_buffers(buffers, session_dir, session_id)
                flush_count += 1
                total_flushed += n
                elapsed = time.time() - buffers.start_time
                rate = total_flushed / elapsed if elapsed > 0 else 0
                print(
                    f"  Flush #{flush_count}: {n:,} rows "
                    f"(total: {total_flushed:,}, rate: {rate:.0f}/s)"
                )
            else:
                print(f"  Flush #{flush_count + 1}: no data (waiting for market)")
    finally:
        shutdown_event.set()
        rest_thread.join(timeout=5)
        if buffers.count() > 0:
            n = flush_buffers(buffers, session_dir, session_id)
            total_flushed += n
            print(f"  Final flush: {n:,} rows")

        print()
        print("=== Recording Complete ===")
        print(f"Total rows:  {total_flushed:,}")
        print(f"Flushes:     {flush_count}")
        print(f"Output:      {session_dir}")
        files = sorted(session_dir.glob("*.parquet"))
        total_size = sum(f.stat().st_size for f in files)
        print(f"Files:       {len(files)}")
        print(f"Total size:  {total_size / 1024 / 1024:.1f} MB")


if __name__ == "__main__":
    main()
