#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

try:
    from zoneinfo import ZoneInfo
except ImportError:
    ZoneInfo = None


TZ_NAME = "Asia/Hong_Kong"


def load_dotenv(dotenv_path: Path) -> None:
    if not dotenv_path.exists():
        return
    for line in dotenv_path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in stripped:
            continue
        key, value = stripped.split("=", 1)
        value = value.strip()
        if not value:
            continue
        os.environ.setdefault(key.strip(), value)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Eden day session with runtimes, API, and Codex analyst logging."
    )
    parser.add_argument("--workspace", default=".")
    parser.add_argument("--markets", default="hk,us")
    parser.add_argument("--provider", choices=["cloud", "oss"], default="cloud")
    parser.add_argument("--local-provider", choices=["ollama", "lmstudio"])
    parser.add_argument("--model")
    parser.add_argument("--poll-seconds", type=int, default=30)
    parser.add_argument("--min-seconds-between-runs", type=int, default=180)
    parser.add_argument("--skip-if-silent", action="store_true", default=True)
    parser.add_argument("--run-silent-too", action="store_true")
    parser.add_argument("--end-at", help="ISO8601 timestamp in Asia/Hong_Kong")
    parser.add_argument("--log-root", default="logs/dayruns")
    return parser.parse_args()


def hk_now() -> datetime:
    if ZoneInfo is None:
        return datetime.now()
    return datetime.now(ZoneInfo(TZ_NAME))


def parse_end_at(raw: Optional[str]) -> datetime:
    if raw:
        if ZoneInfo is None:
            return datetime.fromisoformat(raw)
        dt = datetime.fromisoformat(raw)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=ZoneInfo(TZ_NAME))
        return dt

    now = hk_now()
    end = now.replace(hour=23, minute=59, second=59, microsecond=0)
    if end <= now:
        end = end + timedelta(days=1)
    return end


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def append_jsonl(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload, ensure_ascii=False) + "\n")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def build_paths(root: Path, market: str) -> dict:
    if market == "hk":
        return {
            "briefing": root / os.getenv("EDEN_AGENT_BRIEFING_PATH", "data/agent_briefing.json"),
            "analysis": root / os.getenv("EDEN_AGENT_ANALYSIS_PATH", "data/agent_analysis.json"),
            "narration": root / os.getenv("EDEN_AGENT_NARRATION_PATH", "data/agent_narration.json"),
            "runtime_narration": root
            / os.getenv("EDEN_AGENT_RUNTIME_NARRATION_PATH", "data/agent_runtime_narration.json"),
            "session": root / os.getenv("EDEN_AGENT_SESSION_PATH", "data/agent_session.json"),
            "snapshot": root / os.getenv("EDEN_AGENT_SNAPSHOT_PATH", "data/agent_snapshot.json"),
            "watchlist": root / os.getenv("EDEN_AGENT_WATCHLIST_PATH", "data/agent_watchlist.json"),
            "recommendations": root
            / os.getenv("EDEN_AGENT_RECOMMENDATIONS_PATH", "data/agent_recommendations.json"),
            "scoreboard": root / os.getenv("EDEN_AGENT_SCOREBOARD_PATH", "data/agent_scoreboard.json"),
            "eod_review": root / os.getenv("EDEN_AGENT_EOD_REVIEW_PATH", "data/agent_eod_review.json"),
            "analyst_review": root
            / os.getenv("EDEN_AGENT_ANALYST_REVIEW_PATH", "data/agent_analyst_review.json"),
            "analyst_scoreboard": root
            / os.getenv("EDEN_AGENT_ANALYST_SCOREBOARD_PATH", "data/agent_analyst_scoreboard.json"),
        }
    return {
        "briefing": root
        / os.getenv("EDEN_US_AGENT_BRIEFING_PATH", "data/us_agent_briefing.json"),
        "analysis": root
        / os.getenv("EDEN_US_AGENT_ANALYSIS_PATH", "data/us_agent_analysis.json"),
        "narration": root
        / os.getenv("EDEN_US_AGENT_NARRATION_PATH", "data/us_agent_narration.json"),
        "runtime_narration": root
        / os.getenv("EDEN_US_AGENT_RUNTIME_NARRATION_PATH", "data/us_agent_runtime_narration.json"),
        "session": root / os.getenv("EDEN_US_AGENT_SESSION_PATH", "data/us_agent_session.json"),
        "snapshot": root / os.getenv("EDEN_US_AGENT_SNAPSHOT_PATH", "data/us_agent_snapshot.json"),
        "watchlist": root / os.getenv("EDEN_US_AGENT_WATCHLIST_PATH", "data/us_agent_watchlist.json"),
        "recommendations": root
        / os.getenv("EDEN_US_AGENT_RECOMMENDATIONS_PATH", "data/us_agent_recommendations.json"),
        "scoreboard": root / os.getenv("EDEN_US_AGENT_SCOREBOARD_PATH", "data/us_agent_scoreboard.json"),
        "eod_review": root / os.getenv("EDEN_US_AGENT_EOD_REVIEW_PATH", "data/us_agent_eod_review.json"),
        "analyst_review": root
        / os.getenv("EDEN_US_AGENT_ANALYST_REVIEW_PATH", "data/us_agent_analyst_review.json"),
        "analyst_scoreboard": root
        / os.getenv(
            "EDEN_US_AGENT_ANALYST_SCOREBOARD_PATH",
            "data/us_agent_analyst_scoreboard.json",
        ),
    }


def analyst_command(root: Path, market: str, args: argparse.Namespace) -> list[str]:
    cmd = [
        str(root / "scripts" / "run_codex_analyst.py"),
        "--market",
        market,
        "--provider",
        args.provider,
        "--context-source",
        "api",
    ]
    if args.local_provider:
        cmd.extend(["--local-provider", args.local_provider])
    if args.model:
        cmd.extend(["--model", args.model])
    if args.skip_if_silent and not args.run_silent_too:
        cmd.append("--skip-if-silent")
    cmd.extend(["--workspace", str(root)])
    return cmd


def _list_strings(payload: dict, key: str) -> list[str]:
    values = payload.get(key, [])
    if not isinstance(values, list):
        return []
    return [str(item) for item in values if isinstance(item, (str, int, float)) and str(item)]


def build_analyst_review(market: str, paths: dict) -> dict:
    analysis = load_json(paths["analysis"])
    narration = load_json(paths["narration"])
    runtime = load_json(paths["runtime_narration"])

    runtime_focus = _list_strings(runtime, "focus_symbols")
    final_focus = _list_strings(narration, "focus_symbols")
    runtime_primary = runtime.get("primary_action")
    final_primary = narration.get("primary_action")
    runtime_watch_next = _list_strings(runtime, "watch_next")
    final_watch_next = _list_strings(narration, "watch_next")
    runtime_not_to_do = _list_strings(runtime, "what_not_to_do")
    final_not_to_do = _list_strings(narration, "what_not_to_do")
    runtime_fragility = _list_strings(runtime, "fragility")
    final_fragility = _list_strings(narration, "fragility")

    changes: list[str] = []
    if runtime.get("should_alert") != narration.get("should_alert"):
        changes.append(
            f"should_alert {runtime.get('should_alert')} -> {narration.get('should_alert')}"
        )
    if runtime.get("alert_level") != narration.get("alert_level"):
        changes.append(
            f"alert_level {runtime.get('alert_level')} -> {narration.get('alert_level')}"
        )
    if runtime_primary != final_primary:
        changes.append(f"primary_action {runtime_primary} -> {final_primary}")
    if runtime_focus != final_focus:
        changes.append(f"focus_symbols {runtime_focus} -> {final_focus}")
    if runtime.get("confidence_band") != narration.get("confidence_band"):
        changes.append(
            f"confidence_band {runtime.get('confidence_band')} -> {narration.get('confidence_band')}"
        )
    if runtime_watch_next != final_watch_next:
        changes.append("watch_next refined")
    if runtime_not_to_do != final_not_to_do:
        changes.append("what_not_to_do refined")
    if runtime_fragility != final_fragility:
        changes.append("fragility refined")
    if (analysis.get("final_action") or "unknown") not in ("unknown", None):
        changes.append(f"analysis_final_action={analysis.get('final_action')}")

    core_changed = any(
        change.startswith(prefix)
        for change in changes
        for prefix in ("should_alert", "primary_action", "focus_symbols")
    )
    framing_changed = any(
        change in {"watch_next refined", "what_not_to_do refined", "fragility refined"}
        for change in changes
    )
    cosmetic_only = not core_changed and not framing_changed and bool(changes)

    if core_changed and runtime.get("should_alert") is False and narration.get("should_alert") is True:
        lift_assessment = "upgraded_attention"
    elif core_changed and runtime_primary != final_primary:
        lift_assessment = "decision_changed"
    elif framing_changed:
        lift_assessment = "decision_framing_improved"
    elif cosmetic_only:
        lift_assessment = "cosmetic_rewrite"
    elif changes:
        lift_assessment = "minor_refinement"
    else:
        lift_assessment = "no_material_change"

    notes: list[str] = []
    if final_watch_next and not runtime_watch_next:
        notes.append("LLM added concrete watch-next conditions.")
    if final_not_to_do and not runtime_not_to_do:
        notes.append("LLM added explicit do-not-do guardrails.")
    if final_fragility and not runtime_fragility:
        notes.append("LLM exposed fragility that runtime narration did not show.")
    if runtime_primary == final_primary and runtime_focus == final_focus:
        notes.append("Primary decision stayed aligned with deterministic output.")

    return {
        "tick": int(narration.get("tick") or analysis.get("tick") or runtime.get("tick") or 0),
        "timestamp": narration.get("timestamp") or analysis.get("timestamp") or runtime.get("timestamp"),
        "market": narration.get("market") or analysis.get("market") or market.upper(),
        "provider": analysis.get("provider") or narration.get("source") or "unknown",
        "model": analysis.get("model") or "unknown",
        "final_action": analysis.get("final_action") or "unknown",
        "runtime_should_alert": bool(runtime.get("should_alert", False)),
        "final_should_alert": bool(narration.get("should_alert", False)),
        "runtime_alert_level": runtime.get("alert_level") or "normal",
        "final_alert_level": narration.get("alert_level") or "normal",
        "runtime_primary_action": runtime_primary,
        "final_primary_action": final_primary,
        "runtime_focus_symbols": runtime_focus,
        "final_focus_symbols": final_focus,
        "decision_changed": core_changed,
        "cosmetic_only": cosmetic_only,
        "changes": changes,
        "lift_assessment": lift_assessment,
        "notes": notes,
    }


def decimal_string(value: float) -> str:
    return f"{value:.6f}".rstrip("0").rstrip(".") or "0"


def build_analyst_scoreboard(review: dict, existing: Optional[dict]) -> dict:
    existing = existing or {}
    total_reviews = int(existing.get("total_reviews", 0)) + 1
    upgraded_attention = int(existing.get("upgraded_attention", 0))
    decision_changed = int(existing.get("decision_changed", 0))
    decision_framing_improved = int(existing.get("decision_framing_improved", 0))
    cosmetic_rewrite = int(existing.get("cosmetic_rewrite", 0))
    minor_refinement = int(existing.get("minor_refinement", 0))
    no_material_change = int(existing.get("no_material_change", 0))

    lift = review.get("lift_assessment")
    if lift == "upgraded_attention":
        upgraded_attention += 1
    elif lift == "decision_changed":
        decision_changed += 1
    elif lift == "decision_framing_improved":
        decision_framing_improved += 1
    elif lift == "cosmetic_rewrite":
        cosmetic_rewrite += 1
    elif lift == "minor_refinement":
        minor_refinement += 1
    else:
        no_material_change += 1

    material_change_count = upgraded_attention + decision_changed + decision_framing_improved
    cosmetic_only_count = cosmetic_rewrite

    return {
        "tick": review.get("tick", 0),
        "timestamp": review.get("timestamp"),
        "market": review.get("market"),
        "total_reviews": total_reviews,
        "upgraded_attention": upgraded_attention,
        "decision_changed": decision_changed,
        "decision_framing_improved": decision_framing_improved,
        "cosmetic_rewrite": cosmetic_rewrite,
        "minor_refinement": minor_refinement,
        "no_material_change": no_material_change,
        "material_change_rate": decimal_string(material_change_count / max(total_reviews, 1)),
        "cosmetic_only_rate": decimal_string(cosmetic_only_count / max(total_reviews, 1)),
        "latest_lift_assessment": review.get("lift_assessment"),
        "latest_changes": review.get("changes", []),
        "latest_notes": review.get("notes", []),
    }


def enrich_eod_review_with_analyst_lift(paths: dict, analyst_scoreboard: dict) -> None:
    eod_path = paths.get("eod_review")
    if eod_path is None or not eod_path.exists():
        return
    eod_review = load_json(eod_path)
    eod_review["analyst_lift"] = {
        "total_reviews": analyst_scoreboard.get("total_reviews", 0),
        "upgraded_attention": analyst_scoreboard.get("upgraded_attention", 0),
        "decision_changed": analyst_scoreboard.get("decision_changed", 0),
        "decision_framing_improved": analyst_scoreboard.get("decision_framing_improved", 0),
        "cosmetic_rewrite": analyst_scoreboard.get("cosmetic_rewrite", 0),
        "minor_refinement": analyst_scoreboard.get("minor_refinement", 0),
        "no_material_change": analyst_scoreboard.get("no_material_change", 0),
        "material_change_rate": analyst_scoreboard.get("material_change_rate", "0"),
        "cosmetic_only_rate": analyst_scoreboard.get("cosmetic_only_rate", "0"),
        "latest_lift_assessment": analyst_scoreboard.get("latest_lift_assessment"),
        "latest_changes": analyst_scoreboard.get("latest_changes", []),
        "latest_notes": analyst_scoreboard.get("latest_notes", []),
    }

    conclusions = eod_review.get("conclusions", [])
    if not isinstance(conclusions, list):
        conclusions = []
    conclusions = [str(item) for item in conclusions if isinstance(item, str)]
    conclusions = [
        item
        for item in conclusions
        if not item.startswith("analyst lift:")
    ]
    total_reviews = int(analyst_scoreboard.get("total_reviews", 0))
    if total_reviews > 0:
        conclusions.append(
            "analyst lift: "
            f"{analyst_scoreboard.get('latest_lift_assessment')}, "
            f"material_change_rate={analyst_scoreboard.get('material_change_rate')}, "
            f"cosmetic_only_rate={analyst_scoreboard.get('cosmetic_only_rate')}"
        )
    eod_review["conclusions"] = conclusions
    write_json(eod_path, eod_review)


@dataclass
class ManagedProcess:
    name: str
    command: list[str]
    log_path: Path
    cwd: Path
    process: Optional[subprocess.Popen] = None

    def start(self, event_log: Path) -> None:
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        log_handle = self.log_path.open("a", encoding="utf-8")
        self.process = subprocess.Popen(
            self.command,
            cwd=self.cwd,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            text=True,
            start_new_session=True,
        )
        append_jsonl(
            event_log,
            {
                "ts": hk_now().isoformat(),
                "event": "process_started",
                "name": self.name,
                "pid": self.process.pid,
                "command": self.command,
            },
        )

    def ensure_running(self, event_log: Path) -> None:
        if self.process is None:
            self.start(event_log)
            return
        code = self.process.poll()
        if code is None:
            return
        append_jsonl(
            event_log,
            {
                "ts": hk_now().isoformat(),
                "event": "process_exited",
                "name": self.name,
                "code": code,
            },
        )
        time.sleep(3)
        self.start(event_log)

    def stop(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        try:
            os.killpg(self.process.pid, signal.SIGTERM)
        except Exception:
            pass


@dataclass
class AnalystJob:
    market: str
    tick: int
    headline: Optional[str]
    command: list[str]
    log_path: Path
    started_at: str
    process: subprocess.Popen


class Supervisor:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.root = Path(args.workspace).resolve()
        load_dotenv(self.root / ".env")
        self.end_at = parse_end_at(args.end_at)
        self.markets = [item.strip() for item in args.markets.split(",") if item.strip()]
        run_id = hk_now().strftime("%Y%m%d-%H%M%S")
        self.log_dir = self.root / args.log_root / run_id
        self.archive_dir = self.log_dir / "archive"
        self.event_log = self.log_dir / "events.jsonl"
        self.status_path = self.log_dir / "status.json"
        self.pid_path = self.log_dir / "supervisor.pid"
        self.last_run_at: dict[str, float] = {}
        self.last_seen_tick: dict[str, int] = {}
        self.last_success_tick: dict[str, int] = {}
        self.jobs: dict[str, AnalystJob] = {}
        self.missing_artifacts_reported: set[tuple[str, str]] = set()

        self.processes = [
            ManagedProcess(
                name="eden-api",
                command=[str(self.root / "target" / "debug" / "eden-api"), "serve"],
                log_path=self.log_dir / "eden-api.log",
                cwd=self.root,
            ),
            ManagedProcess(
                name="eden-hk",
                command=[str(self.root / "target" / "debug" / "eden")],
                log_path=self.log_dir / "eden-hk.log",
                cwd=self.root,
            ),
            ManagedProcess(
                name="eden-us",
                command=[str(self.root / "target" / "debug" / "eden"), "us"],
                log_path=self.log_dir / "eden-us.log",
                cwd=self.root,
            ),
        ]

    def build_binaries(self) -> None:
        subprocess.run(
            ["cargo", "build", "--bin", "eden", "--bin", "eden-api"],
            cwd=self.root,
            check=True,
        )

    def run(self) -> int:
        self.log_dir.mkdir(parents=True, exist_ok=True)
        write_text(self.pid_path, str(os.getpid()))
        append_jsonl(
            self.event_log,
            {
                "ts": hk_now().isoformat(),
                "event": "supervisor_started",
                "markets": self.markets,
                "provider": self.args.provider,
                "end_at": self.end_at.isoformat(),
            },
        )

        self.build_binaries()
        for process in self.processes:
            process.start(self.event_log)

        try:
            while hk_now() < self.end_at:
                for process in self.processes:
                    process.ensure_running(self.event_log)
                self.reap_jobs()
                for market in self.markets:
                    self.handle_market(market)
                self.write_status()
                time.sleep(max(5, self.args.poll_seconds))
        finally:
            for job in self.jobs.values():
                try:
                    os.killpg(job.process.pid, signal.SIGTERM)
                except Exception:
                    pass
            for process in self.processes:
                process.stop()
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "supervisor_stopped",
                },
            )
        return 0

    def handle_market(self, market: str) -> None:
        paths = build_paths(self.root, market)
        briefing_path = paths["briefing"]
        if not briefing_path.exists():
            key = (market, "briefing")
            if key not in self.missing_artifacts_reported:
                append_jsonl(
                    self.event_log,
                    {
                        "ts": hk_now().isoformat(),
                        "event": "artifact_missing",
                        "market": market,
                        "artifact": "briefing",
                        "path": str(briefing_path),
                    },
                )
                self.missing_artifacts_reported.add(key)
            return
        self.missing_artifacts_reported.discard((market, "briefing"))
        try:
            briefing = load_json(briefing_path)
        except Exception as error:
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "briefing_read_error",
                    "market": market,
                    "error": str(error),
                },
            )
            return

        tick = int(briefing.get("tick", 0))
        if tick <= self.last_seen_tick.get(market, 0):
            return
        self.last_seen_tick[market] = tick

        if market in self.jobs:
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "analyst_skipped_running",
                    "market": market,
                    "tick": tick,
                },
            )
            return

        if briefing.get("should_speak") is not True and self.args.skip_if_silent and not self.args.run_silent_too:
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "analyst_skipped_silent",
                    "market": market,
                    "tick": tick,
                },
            )
            return

        now_ts = time.time()
        last_run = self.last_run_at.get(market, 0.0)
        if now_ts - last_run < self.args.min_seconds_between_runs:
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "analyst_skipped_cooldown",
                    "market": market,
                    "tick": tick,
                    "cooldown_seconds": self.args.min_seconds_between_runs,
                },
            )
            return

        self.last_run_at[market] = now_ts
        cmd = analyst_command(self.root, market, self.args)
        log_path = self.log_dir / f"codex-{market}-tick{tick}.log"
        log_handle = log_path.open("w", encoding="utf-8")
        process = subprocess.Popen(
            cmd,
            cwd=self.root,
            text=True,
            stdout=log_handle,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        self.jobs[market] = AnalystJob(
            market=market,
            tick=tick,
            headline=briefing.get("headline"),
            command=cmd,
            log_path=log_path,
            started_at=hk_now().isoformat(),
            process=process,
        )
        append_jsonl(
            self.event_log,
            {
                "ts": hk_now().isoformat(),
                "event": "analyst_started",
                "market": market,
                "tick": tick,
                "headline": briefing.get("headline"),
                "command": cmd,
                "pid": process.pid,
                "log_path": str(log_path),
            },
        )

    def reap_jobs(self) -> None:
        completed_markets = []
        for market, job in self.jobs.items():
            code = job.process.poll()
            if code is None:
                continue
            completed_markets.append(market)
            output = ""
            if job.log_path.exists():
                output = job.log_path.read_text(encoding="utf-8", errors="replace")
            append_jsonl(
                self.event_log,
                {
                    "ts": hk_now().isoformat(),
                    "event": "analyst_finished",
                    "market": market,
                    "tick": job.tick,
                    "headline": job.headline,
                    "returncode": code,
                    "command": job.command,
                    "log_path": str(job.log_path),
                    "output": output,
                },
            )
            if code == 0:
                self.last_success_tick[market] = job.tick
                paths = build_paths(self.root, market)
                try:
                    review = build_analyst_review(market, paths)
                    write_json(paths["analyst_review"], review)
                    existing_scoreboard = (
                        load_json(paths["analyst_scoreboard"])
                        if paths["analyst_scoreboard"].exists()
                        else None
                    )
                    analyst_scoreboard = build_analyst_scoreboard(review, existing_scoreboard)
                    write_json(paths["analyst_scoreboard"], analyst_scoreboard)
                    enrich_eod_review_with_analyst_lift(paths, analyst_scoreboard)
                    append_jsonl(
                        self.event_log,
                        {
                            "ts": hk_now().isoformat(),
                            "event": "analyst_review_written",
                            "market": market,
                            "tick": job.tick,
                            "lift_assessment": review.get("lift_assessment"),
                            "decision_changed": review.get("decision_changed"),
                            "cosmetic_only": review.get("cosmetic_only"),
                            "path": str(paths["analyst_review"]),
                            "scoreboard_path": str(paths["analyst_scoreboard"]),
                        },
                    )
                except Exception as error:
                    append_jsonl(
                        self.event_log,
                        {
                            "ts": hk_now().isoformat(),
                            "event": "analyst_review_failed",
                            "market": market,
                            "tick": job.tick,
                            "error": str(error),
                        },
                    )
                self.archive_outputs(market, job.tick, paths)
        for market in completed_markets:
            self.jobs.pop(market, None)

    def archive_outputs(self, market: str, tick: int, paths: dict) -> None:
        market_dir = self.archive_dir / market
        market_dir.mkdir(parents=True, exist_ok=True)
        stamp = hk_now().strftime("%Y%m%d-%H%M%S")
        for key in (
            "analysis",
            "narration",
            "runtime_narration",
            "briefing",
            "session",
            "snapshot",
            "watchlist",
            "recommendations",
            "scoreboard",
            "eod_review",
            "analyst_review",
            "analyst_scoreboard",
        ):
            source = paths.get(key)
            if source is None or not source.exists():
                continue
            target = market_dir / f"{stamp}-tick{tick}-{key}.json"
            shutil.copy2(source, target)

    def write_status(self) -> None:
        payload = {
            "now": hk_now().isoformat(),
            "end_at": self.end_at.isoformat(),
            "markets": self.markets,
            "last_seen_tick": self.last_seen_tick,
            "last_success_tick": self.last_success_tick,
            "last_run_at_epoch": self.last_run_at,
            "running_jobs": {
                market: {
                    "tick": job.tick,
                    "pid": job.process.pid,
                    "started_at": job.started_at,
                    "headline": job.headline,
                    "log": str(job.log_path),
                }
                for market, job in self.jobs.items()
            },
            "artifacts": {
                market: self.describe_artifacts(market)
                for market in self.markets
            },
            "processes": [
                {
                    "name": process.name,
                    "pid": process.process.pid if process.process else None,
                    "running": process.process.poll() is None if process.process else False,
                    "log": str(process.log_path),
                }
                for process in self.processes
            ],
        }
        write_text(self.status_path, json.dumps(payload, ensure_ascii=False, indent=2))

    def describe_artifacts(self, market: str) -> dict:
        result = {}
        for name, path in build_paths(self.root, market).items():
            if not path.exists():
                result[name] = {"exists": False, "path": str(path)}
                continue
            try:
                payload = load_json(path)
            except Exception:
                payload = {}
            result[name] = {
                "exists": True,
                "path": str(path),
                "mtime": datetime.fromtimestamp(path.stat().st_mtime, tz=hk_now().tzinfo).isoformat()
                if hk_now().tzinfo
                else datetime.fromtimestamp(path.stat().st_mtime).isoformat(),
                "tick": payload.get("tick"),
            }
        return result


def main() -> int:
    args = parse_args()
    supervisor = Supervisor(args)
    return supervisor.run()


if __name__ == "__main__":
    raise SystemExit(main())
