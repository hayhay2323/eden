#!/usr/bin/env python3
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path


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
        description="Run Eden analyst via Codex CLI and write analysis/narration artifacts."
    )
    parser.add_argument("--market", choices=["hk", "us"], default="hk")
    parser.add_argument(
        "--provider",
        choices=["oss", "cloud"],
        default="cloud",
        help="Use local OSS provider or the normal Codex cloud provider.",
    )
    parser.add_argument(
        "--local-provider",
        choices=["ollama", "lmstudio"],
        help="Used with --provider oss.",
    )
    parser.add_argument(
        "--model",
        help="Optional Codex model override. For --provider oss this is your local model name if configured.",
    )
    parser.add_argument(
        "--workspace",
        default=".",
        help="Workspace root. Defaults to current directory.",
    )
    parser.add_argument(
        "--context-source",
        choices=["api", "artifacts", "auto"],
        default="api",
        help="How Codex should gather context. `api` makes Codex query eden-api directly.",
    )
    parser.add_argument(
        "--api-base",
        default=os.getenv("EDEN_API_BASE_URL", "http://127.0.0.1:8787"),
        help="Base URL for eden-api when Codex should query the API directly.",
    )
    parser.add_argument(
        "--skip-if-silent",
        action="store_true",
        help="Skip Codex run when briefing.should_speak is false.",
    )
    parser.add_argument(
        "--print-command",
        action="store_true",
        help="Print the codex command before executing.",
    )
    return parser.parse_args()


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, ensure_ascii=False, indent=2)


def artifact_paths(root: Path, market: str) -> dict:
    if market == "hk":
        return {
            "snapshot": root / os.getenv("EDEN_AGENT_SNAPSHOT_PATH", "data/agent_snapshot.json"),
            "briefing": root / os.getenv("EDEN_AGENT_BRIEFING_PATH", "data/agent_briefing.json"),
            "session": root / os.getenv("EDEN_AGENT_SESSION_PATH", "data/agent_session.json"),
            "watchlist": root / os.getenv("EDEN_AGENT_WATCHLIST_PATH", "data/agent_watchlist.json"),
            "recommendations": root / os.getenv("EDEN_AGENT_RECOMMENDATIONS_PATH", "data/agent_recommendations.json"),
            "scoreboard": root / os.getenv("EDEN_AGENT_SCOREBOARD_PATH", "data/agent_scoreboard.json"),
            "analysis": root / os.getenv("EDEN_AGENT_ANALYSIS_PATH", "data/agent_analysis.json"),
            "narration": root / os.getenv("EDEN_AGENT_NARRATION_PATH", "data/agent_narration.json"),
        }
    return {
        "snapshot": root / os.getenv("EDEN_US_AGENT_SNAPSHOT_PATH", "data/us_agent_snapshot.json"),
        "briefing": root / os.getenv("EDEN_US_AGENT_BRIEFING_PATH", "data/us_agent_briefing.json"),
        "session": root / os.getenv("EDEN_US_AGENT_SESSION_PATH", "data/us_agent_session.json"),
        "watchlist": root / os.getenv("EDEN_US_AGENT_WATCHLIST_PATH", "data/us_agent_watchlist.json"),
        "recommendations": root / os.getenv("EDEN_US_AGENT_RECOMMENDATIONS_PATH", "data/us_agent_recommendations.json"),
        "scoreboard": root / os.getenv("EDEN_US_AGENT_SCOREBOARD_PATH", "data/us_agent_scoreboard.json"),
        "analysis": root / os.getenv("EDEN_US_AGENT_ANALYSIS_PATH", "data/us_agent_analysis.json"),
        "narration": root / os.getenv("EDEN_US_AGENT_NARRATION_PATH", "data/us_agent_narration.json"),
    }


def require_inputs(paths: dict) -> None:
    missing = [name for name in ("snapshot", "briefing", "session") if not paths[name].exists()]
    if missing:
        raise SystemExit(
            "Missing required input files: "
            + ", ".join(f"{name}={paths[name]}" for name in missing)
        )


def build_prompt_from_artifacts(
    snapshot: dict,
    briefing: dict,
    session: dict,
    watchlist: dict,
    recommendations: dict,
    scoreboard: dict,
) -> str:
    focus_symbols = briefing.get("focus_symbols", [])[:4]
    prompt = {
        "task": (
            "You are Eden decision analyst. Read the provided decision layer and return JSON only. "
            "Keep the output concise, action-oriented, and in Traditional Chinese. "
            "Do not invent facts. If nothing merits trader attention, set should_alert=false."
        ),
        "guidance": [
            "Prioritize the top watchlist names, regime-bound recommendations, and fresh alerts.",
            "Answer: what changed, why it matters, what to watch next, what not to do, and where the setup is fragile.",
            "Also produce a `market_summary_5m` that a trader can read in 5 seconds.",
            "Also produce `action_cards`: each card should be immediately understandable, human-facing, and ready for accept / keep / delete handling.",
            "headline and message should be short and trader-facing.",
            "Do not rewrite the whole snapshot. Stay narrow.",
        ],
        "market_context": {
            "tick": snapshot.get("tick"),
            "timestamp": snapshot.get("timestamp"),
            "market": snapshot.get("market"),
            "market_regime": snapshot.get("market_regime"),
            "stress": snapshot.get("stress"),
        },
        "wake_gate": {
            "should_speak": briefing.get("should_speak"),
            "priority": briefing.get("priority"),
            "headline": briefing.get("headline"),
            "summary": briefing.get("summary", [])[:6],
            "focus_symbols": focus_symbols,
            "reasons": briefing.get("reasons", [])[:6],
        },
        "decision_layer": {
            "watchlist": watchlist,
            "recommendations": recommendations,
            "recent_alerts": scoreboard.get("alerts", [])[:6],
        },
        "thread_memory": {
            "focus_symbols": session.get("focus_symbols", [])[:6],
            "active_threads": session.get("active_threads", [])[:6],
            "recent_turns": session.get("recent_turns", [])[-4:],
        },
    }
    return json.dumps(prompt, ensure_ascii=False, indent=2)


def ensure_api_available(api_base: str) -> None:
    import urllib.request

    health_url = api_base.rstrip("/") + "/health"
    with urllib.request.urlopen(health_url, timeout=3) as response:
        if response.status != 200:
            raise RuntimeError(f"eden-api health check failed: {health_url} -> {response.status}")


def mint_api_key(root: Path) -> str:
    binary = root / "target" / "debug" / "eden-api"
    if not binary.exists():
        raise RuntimeError(
            f"eden-api binary not found at {binary}; run `cargo build --bin eden-api` first"
        )
    if not os.getenv("EDEN_API_MASTER_KEY"):
        raise RuntimeError("EDEN_API_MASTER_KEY is required to mint a temporary frontend API key")

    result = subprocess.run(
        [str(binary), "mint-key", "--label", "codex-analyst", "--ttl-hours", "1"],
        cwd=root,
        text=True,
        capture_output=True,
        env=os.environ.copy(),
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            "failed to mint eden-api key\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return json.loads(result.stdout)["api_key"]


def build_prompt_from_api(market: str, api_base: str, api_key: str, briefing: dict) -> str:
    focus_symbols = briefing.get("focus_symbols", [])[:6]
    wake_gate = {
        "tick": briefing.get("tick"),
        "should_speak": briefing.get("should_speak"),
        "priority": briefing.get("priority"),
        "headline": briefing.get("headline"),
        "summary": briefing.get("summary", [])[:6],
        "focus_symbols": focus_symbols,
        "reasons": briefing.get("reasons", [])[:6],
    }
    return (
        "You are Eden decision analyst.\n"
        "Use the local Eden HTTP API directly instead of reading local artifact files.\n"
        "Reply with JSON only and obey the provided output schema.\n"
        "Write the final trader-facing text in Traditional Chinese.\n\n"
        f"API base:\n{api_base.rstrip('/')}\n\n"
        f"Readonly API key:\n{api_key}\n\n"
        "Access pattern:\n"
        f"1. Start with GET /api/ontology/{market}/market-session using header `x-api-key: <token>`.\n"
        f"2. Then GET /api/ontology/{market}/recommendations using the same header.\n"
        f"3. Then GET /api/feed/{market}/notices and /api/feed/{market}/transitions for recent operational movement.\n"
        "4. Then only fetch what you need, always with header auth:\n"
        f"   - /api/ontology/{market}/symbols?symbol=<SYMBOL>\n"
        f"   - /api/ontology/{market}/backward/<SYMBOL>\n"
        f"   - /api/ontology/{market}/sector-flows\n"
        f"   - /api/ontology/{market}/world\n"
        f"   - /api/ontology/{market}/cases\n"
        f"   - /api/ontology/{market}/workflows\n"
        "5. Only use compatibility agent routes if a required view does not exist on ontology/feed surfaces:\n"
        f"   - /api/agent/{market}/watchlist\n"
        f"   - /api/agent/{market}/narration\n"
        f"   - /api/agent/{market}/scoreboard\n"
        f"   - /api/agent/{market}/eod-review\n"
        f"   - /api/agent/{market}/analyst-review\n"
        f"   - /api/agent/{market}/analyst-scoreboard\n"
        f"   - /api/agent/{market}/depth/<SYMBOL>\n"
        f"   - /api/agent/{market}/brokers/<SYMBOL>\n\n"
        "Tool policy:\n"
        "1. Rank first, drill down second.\n"
        "2. Use ontology objects first; use compatibility agent views only when you need a derived analyst surface.\n"
        "3. Use symbol/depth/brokers only for the top 1-3 names that matter.\n"
        "4. Follow history refs on ontology objects when you need workflow or outcome context.\n"
        "5. Stop once you can answer what changed, why it matters, what to watch next, what not to do, and where the setup is fragile.\n"
        "6. Do not summarize every symbol in the watchlist or recommendation list.\n"
        "7. Do not read local JSON artifacts unless the API is unavailable.\n\n"
        "Output requirements:\n"
        "1. `market_summary_5m` must sound like a concise desk summary of the last few minutes.\n"
        "2. `action_cards` should be a stack-ranked queue of symbols worth operator attention.\n"
        "3. Each `action_card` must be short, obvious, and understandable without reading raw JSON.\n"
        "4. If a symbol is not worth operator attention, leave it out of `action_cards`.\n"
        "5. Prefer 3 to 7 good cards, not a dense list.\n\n"
        "Current deterministic gate:\n"
        f"{json.dumps(wake_gate, ensure_ascii=False, indent=2)}\n"
    )


def codex_command(args: argparse.Namespace, root: Path, schema_path: Path, output_path: Path) -> list[str]:
    cmd = [
        "codex",
        "exec",
        "-",
        "-C",
        str(root),
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--output-schema",
        str(schema_path),
        "-o",
        str(output_path),
    ]
    if args.provider == "oss":
        cmd.append("--oss")
        if args.local_provider:
            cmd.extend(["--local-provider", args.local_provider])
    if args.model:
        cmd.extend(["-m", args.model])
    return cmd


def normalize_provider_args(args: argparse.Namespace) -> None:
    if args.provider != "oss":
        return

    if args.local_provider is None:
        if shutil.which("ollama"):
            args.local_provider = "ollama"
        else:
            raise SystemExit(
                "No local provider detected for --provider oss. "
                "Install Ollama or pass --local-provider lmstudio."
            )

    if args.local_provider == "ollama" and shutil.which("ollama") is None:
        raise SystemExit("Requested --local-provider ollama but `ollama` is not installed.")


def build_analysis(snapshot: dict, result: dict, provider: str, model: str) -> dict:
    return {
        "tick": snapshot["tick"],
        "timestamp": snapshot["timestamp"],
        "market": snapshot["market"],
        "status": "ok",
        "should_speak": result["should_alert"],
        "provider": provider,
        "model": model,
        "message": result["message"] or result["headline"],
        "final_action": result["final_action"],
        "steps": [],
        "error": None,
    }


def build_narration(snapshot: dict, result: dict, source: str) -> dict:
    return {
        "tick": snapshot["tick"],
        "timestamp": snapshot["timestamp"],
        "market": snapshot["market"],
        "should_alert": result["should_alert"],
        "alert_level": result["alert_level"],
        "source": source,
        "headline": result["headline"],
        "message": result["message"],
        "bullets": result["bullets"],
        "focus_symbols": result["focus_symbols"],
        "tags": result["tags"],
        "primary_action": result["primary_action"],
        "confidence_band": result["confidence_band"],
        "what_changed": result["what_changed"],
        "why_it_matters": result["why_it_matters"],
        "watch_next": result["watch_next"],
        "what_not_to_do": result["what_not_to_do"],
        "fragility": result["fragility"],
        "recommendation_ids": result["recommendation_ids"],
        "market_summary_5m": result["market_summary_5m"],
        "action_cards": [
            {
                "card_id": f"card:{snapshot['tick']}:{idx}:{item['symbol']}",
                "symbol": item["symbol"],
                "setup_id": item.get("setup_id"),
                "action": item["action"],
                "action_label": item.get("action_label"),
                "severity": item["severity"],
                "title": item.get("title"),
                "summary": item["summary"],
                "why_now": item["why_now"],
                "confidence_band": item["confidence_band"],
                "watch_next": item["watch_next"],
                "do_not": item["do_not"],
            }
            for idx, item in enumerate(result["action_cards"])
        ],
    }


def main() -> int:
    args = parse_args()
    normalize_provider_args(args)
    root = Path(args.workspace).resolve()
    load_dotenv(root / ".env")
    paths = artifact_paths(root, args.market)
    require_inputs(paths)

    snapshot = load_json(paths["snapshot"])
    briefing = load_json(paths["briefing"])

    if args.skip_if_silent and not briefing.get("should_speak", False):
        print("briefing.should_speak=false, skipped")
        return 0

    schema_path = root / "config" / "codex_analyst_output.schema.json"
    if not schema_path.exists():
        raise SystemExit(f"Missing schema file: {schema_path}")

    with tempfile.NamedTemporaryFile(mode="w+", suffix=".json", delete=False) as handle:
        output_path = Path(handle.name)

    prompt = None
    if args.context_source in {"api", "auto"}:
        try:
            ensure_api_available(args.api_base)
            api_key = mint_api_key(root)
            prompt = build_prompt_from_api(args.market, args.api_base, api_key, briefing)
        except Exception as error:
            if args.context_source == "api":
                raise SystemExit(f"API context mode failed: {error}")
            print(f"[run_codex_analyst] API context unavailable, falling back to artifacts: {error}", file=sys.stderr)

    if prompt is None:
        session = load_json(paths["session"])
        watchlist = load_json(paths["watchlist"]) if paths["watchlist"].exists() else {}
        recommendations = load_json(paths["recommendations"]) if paths["recommendations"].exists() else {}
        scoreboard = load_json(paths["scoreboard"]) if paths["scoreboard"].exists() else {}
        prompt = build_prompt_from_artifacts(
            snapshot,
            briefing,
            session,
            watchlist,
            recommendations,
            scoreboard,
        )

    cmd = codex_command(args, root, schema_path, output_path)
    if args.print_command:
        print(" ".join(cmd), file=sys.stderr)

    try:
        child_env = os.environ.copy()
        isolate_codex_home = args.provider != "cloud"
        if args.provider == "cloud":
            if child_env.get("OPENAI_API_KEY"):
                child_env["OPENAI_API_KEY"] = child_env["OPENAI_API_KEY"]
            if not child_env.get("OPENAI_BASE_URL"):
                child_env.pop("OPENAI_BASE_URL", None)
            child_env.pop("OPENAI_MODEL", None)
        if isolate_codex_home:
            codex_home = Path(tempfile.gettempdir()) / f"eden-codex-home-{uuid.uuid4().hex}"
            codex_home.mkdir(parents=True, exist_ok=True)
            child_env["CODEX_HOME"] = str(codex_home)
        completed = subprocess.run(
            cmd,
            input=prompt,
            text=True,
            capture_output=True,
            env=child_env,
            check=False,
        )
        if completed.returncode != 0:
            raise SystemExit(
                "codex exec failed\n"
                f"exit={completed.returncode}\n"
                f"stdout:\n{completed.stdout}\n"
                f"stderr:\n{completed.stderr}"
            )

        result = load_json(output_path)
        provider = "codex-cli-oss" if args.provider == "oss" else "codex-cli"
        model = args.model or ("oss-default" if args.provider == "oss" else "cloud-default")
        analysis = build_analysis(snapshot, result, provider, model)
        narration = build_narration(snapshot, result, provider)

        write_json(paths["analysis"], analysis)
        write_json(paths["narration"], narration)

        print(json.dumps({"analysis": str(paths["analysis"]), "narration": str(paths["narration"])}, ensure_ascii=False))
        return 0
    finally:
        try:
            output_path.unlink(missing_ok=True)
        except Exception:
            pass
        try:
            if 'codex_home' in locals():
                shutil.rmtree(codex_home, ignore_errors=True)
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
