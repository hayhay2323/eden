#!/usr/bin/env python3
import argparse
import json
from collections import Counter, defaultdict
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Analyze Eden recommendation journal and emit wait-miss / calibration report."
    )
    parser.add_argument("journal_path", help="Path to agent_recommendation_journal.jsonl")
    parser.add_argument(
        "--output-prefix",
        help="Write <prefix>.json and <prefix>.md with the report",
    )
    parser.add_argument("--top", type=int, default=10, help="Number of top items to include")
    return parser.parse_args()


def d(value) -> Decimal:
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return Decimal("0")


@dataclass
class FamilyStats:
    total: int = 0
    resolved: int = 0
    wait_total: int = 0
    wait_hit: int = 0
    wait_miss: int = 0
    wait_flat: int = 0
    wait_regret_sum: Decimal = Decimal("0")
    wait_regret_max: Decimal = Decimal("0")


@dataclass
class ScopeStats:
    total: int = 0
    resolved: int = 0
    hits: int = 0
    misses: int = 0
    flats: int = 0
    regret_sum: Decimal = Decimal("0")
    regret_max: Decimal = Decimal("0")


def load_items(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        payload = json.loads(line)
        decisions = payload.get("decisions")
        if isinstance(decisions, list) and decisions:
            for decision in decisions:
                if not isinstance(decision, dict):
                    continue
                scope_kind = decision.get("scope_kind")
                data = decision.get("data", {})
                if not isinstance(data, dict):
                    continue
                item = dict(data)
                item["_scope_kind"] = scope_kind
                item["_tick"] = payload.get("tick")
                item["_timestamp"] = payload.get("timestamp")
                rows.append(item)
        else:
            for item in payload.get("items", []):
                item = dict(item)
                item["_scope_kind"] = "symbol"
                item["_tick"] = payload.get("tick")
                item["_timestamp"] = payload.get("timestamp")
                rows.append(item)
    return rows


def top_wait_miss(items: list[dict], top: int) -> list[dict]:
    misses = [
        item
        for item in items
        if item.get("_scope_kind") == "symbol"
        and item.get("best_action") == "wait"
        and item.get("resolution", {}).get("status") == "miss"
    ]
    misses.sort(
        key=lambda item: d(item.get("resolution", {}).get("wait_regret", 0)),
        reverse=True,
    )
    trimmed = []
    for item in misses[:top]:
        trimmed.append(
            {
                "tick": item.get("_tick"),
                "timestamp": item.get("_timestamp"),
                "symbol": item.get("symbol"),
                "family": item.get("thesis_family") or "unknown",
                "action": item.get("action"),
                "counterfactual_best_action": item.get("resolution", {}).get(
                    "counterfactual_best_action"
                ),
                "wait_regret": item.get("resolution", {}).get("wait_regret"),
                "follow_realized_return": item.get("resolution", {}).get(
                    "follow_realized_return"
                ),
                "fade_realized_return": item.get("resolution", {}).get("fade_realized_return"),
                "decisive_factors": item.get("decision_attribution", {}).get(
                    "decisive_factors", []
                )[:5],
            }
        )
    return trimmed


def top_decision_miss(items: list[dict], top: int) -> list[dict]:
    misses = [
        item
        for item in items
        if item.get("resolution", {}).get("status") == "miss"
    ]
    misses.sort(
        key=lambda item: d(item.get("resolution", {}).get("wait_regret", 0)),
        reverse=True,
    )
    trimmed = []
    for item in misses[:top]:
        trimmed.append(
            {
                "tick": item.get("_tick"),
                "timestamp": item.get("_timestamp"),
                "scope_kind": item.get("_scope_kind"),
                "label": item.get("symbol")
                or item.get("sector")
                or item.get("preferred_expression")
                or item.get("edge_layer")
                or "unknown",
                "family": item.get("thesis_family") or item.get("edge_layer") or "unknown",
                "best_action": item.get("best_action"),
                "counterfactual_best_action": item.get("resolution", {}).get(
                    "counterfactual_best_action"
                ),
                "wait_regret": item.get("resolution", {}).get("wait_regret"),
                "follow_realized_return": item.get("resolution", {}).get(
                    "follow_realized_return"
                ),
                "fade_realized_return": item.get("resolution", {}).get("fade_realized_return"),
            }
        )
    return trimmed


def family_summary(items: list[dict]) -> list[dict]:
    stats = defaultdict(FamilyStats)
    for item in items:
        family = item.get("thesis_family") or item.get("edge_layer") or "unknown"
        bucket = stats[family]
        bucket.total += 1
        resolution = item.get("resolution")
        if not resolution:
            continue
        bucket.resolved += 1
        if item.get("_scope_kind") != "symbol" or item.get("best_action") != "wait":
            continue
        bucket.wait_total += 1
        status = resolution.get("status")
        if status == "hit":
            bucket.wait_hit += 1
        elif status == "miss":
            bucket.wait_miss += 1
        else:
            bucket.wait_flat += 1
        regret = d(resolution.get("wait_regret", 0))
        bucket.wait_regret_sum += regret
        bucket.wait_regret_max = max(bucket.wait_regret_max, regret)

    rows = []
    for family, bucket in stats.items():
        avg_regret = (
            bucket.wait_regret_sum / bucket.wait_total
            if bucket.wait_total
            else Decimal("0")
        )
        miss_rate = (
            Decimal(bucket.wait_miss) / Decimal(bucket.wait_total)
            if bucket.wait_total
            else Decimal("0")
        )
        rows.append(
            {
                "family": family,
                "total": bucket.total,
                "resolved": bucket.resolved,
                "wait_total": bucket.wait_total,
                "wait_hit": bucket.wait_hit,
                "wait_miss": bucket.wait_miss,
                "wait_flat": bucket.wait_flat,
                "wait_miss_rate": str(miss_rate.quantize(Decimal("0.0001"))),
                "avg_wait_regret": str(avg_regret.quantize(Decimal("0.0001"))),
                "max_wait_regret": str(bucket.wait_regret_max.quantize(Decimal("0.0001"))),
            }
        )
    rows.sort(key=lambda row: (-row["total"], row["family"]))
    return rows


def scope_summary(items: list[dict]) -> list[dict]:
    stats = defaultdict(ScopeStats)
    for item in items:
        scope = item.get("_scope_kind") or "unknown"
        bucket = stats[scope]
        bucket.total += 1
        resolution = item.get("resolution")
        if not resolution:
            continue
        bucket.resolved += 1
        status = resolution.get("status")
        if status == "hit":
            bucket.hits += 1
        elif status == "miss":
            bucket.misses += 1
        else:
            bucket.flats += 1
        regret = d(resolution.get("wait_regret", 0))
        bucket.regret_sum += regret
        bucket.regret_max = max(bucket.regret_max, regret)

    rows = []
    for scope, bucket in stats.items():
        hit_rate = (
            Decimal(bucket.hits) / Decimal(bucket.resolved)
            if bucket.resolved
            else Decimal("0")
        )
        avg_regret = (
            bucket.regret_sum / Decimal(bucket.resolved)
            if bucket.resolved
            else Decimal("0")
        )
        rows.append(
            {
                "scope_kind": scope,
                "total": bucket.total,
                "resolved": bucket.resolved,
                "hits": bucket.hits,
                "misses": bucket.misses,
                "flats": bucket.flats,
                "hit_rate": str(hit_rate.quantize(Decimal("0.0001"))),
                "avg_regret": str(avg_regret.quantize(Decimal("0.0001"))),
                "max_regret": str(bucket.regret_max.quantize(Decimal("0.0001"))),
            }
        )
    rows.sort(key=lambda row: (-row["resolved"], row["scope_kind"]))
    return rows


def calibration_candidates(family_rows: list[dict], top: int) -> list[dict]:
    candidates = []
    for row in family_rows:
        wait_total = row["wait_total"]
        if wait_total < 10:
            continue
        miss_rate = d(row["wait_miss_rate"])
        avg_regret = d(row["avg_wait_regret"])
        score = miss_rate * Decimal("0.7") + avg_regret * Decimal("30")
        if miss_rate < Decimal("0.08") and avg_regret < Decimal("0.0015"):
            continue
        candidates.append(
            {
                "family": row["family"],
                "wait_total": wait_total,
                "wait_miss": row["wait_miss"],
                "wait_miss_rate": row["wait_miss_rate"],
                "avg_wait_regret": row["avg_wait_regret"],
                "max_wait_regret": row["max_wait_regret"],
                "calibration_score": str(score.quantize(Decimal("0.0001"))),
            }
        )
    candidates.sort(
        key=lambda row: (
            -d(row["calibration_score"]),
            -row["wait_total"],
            row["family"],
        )
    )
    return candidates[:top]


def build_report(items: list[dict], top: int) -> dict:
    resolved = [item for item in items if item.get("resolution")]
    best_action_counts = Counter(item.get("best_action", "unknown") for item in items)
    resolved_by_action = Counter(item.get("best_action", "unknown") for item in resolved)
    status_by_action = defaultdict(Counter)
    for item in resolved:
        status_by_action[item.get("best_action", "unknown")][
            item["resolution"].get("status", "unknown")
        ] += 1

    family_rows = family_summary(items)
    report = {
        "journal_rows": len({item["_tick"] for item in items}),
        "journal_items": len(items),
        "resolved_items": len(resolved),
        "scope_kind_counts": dict(Counter(item.get("_scope_kind", "unknown") for item in items)),
        "best_action_counts": dict(best_action_counts),
        "resolved_by_action": dict(resolved_by_action),
        "status_by_action": {
            action: dict(counter) for action, counter in status_by_action.items()
        },
        "scope_summary": scope_summary(items),
        "top_decision_miss": top_decision_miss(items, top),
        "top_wait_miss": top_wait_miss(items, top),
        "family_summary": family_rows[:top],
        "family_calibration_candidates": calibration_candidates(family_rows, top),
    }
    return report


def to_markdown(report: dict, journal_path: Path) -> str:
    lines = []
    lines.append(f"# Recommendation Journal Report")
    lines.append(f"")
    lines.append(f"Source: `{journal_path}`")
    lines.append(f"")
    lines.append(f"- Journal rows: {report['journal_rows']}")
    lines.append(f"- Journal items: {report['journal_items']}")
    lines.append(f"- Resolved items: {report['resolved_items']}")
    lines.append(f"- Scope counts: {report['scope_kind_counts']}")
    lines.append(f"- Best action counts: {report['best_action_counts']}")
    lines.append(f"- Resolved by action: {report['resolved_by_action']}")
    lines.append(f"")

    lines.append("## Scope Summary")
    for row in report["scope_summary"]:
        lines.append(
            f"- {row['scope_kind']}: total={row['total']} resolved={row['resolved']} "
            f"hits={row['hits']} misses={row['misses']} hit_rate={row['hit_rate']} "
            f"avg_regret={row['avg_regret']}"
        )
    lines.append("")

    lines.append("## Family Summary")
    for row in report["family_summary"]:
        lines.append(
            f"- {row['family']}: total={row['total']} resolved={row['resolved']} "
            f"wait_total={row['wait_total']} wait_miss={row['wait_miss']} "
            f"wait_miss_rate={row['wait_miss_rate']} avg_wait_regret={row['avg_wait_regret']}"
        )
    lines.append("")

    lines.append("## Calibration Candidates")
    if report["family_calibration_candidates"]:
        for row in report["family_calibration_candidates"]:
            lines.append(
                f"- {row['family']}: wait_total={row['wait_total']} wait_miss={row['wait_miss']} "
                f"miss_rate={row['wait_miss_rate']} avg_regret={row['avg_wait_regret']} "
                f"score={row['calibration_score']}"
            )
    else:
        lines.append("- None")
    lines.append("")

    lines.append("## Top Decision Miss")
    for row in report["top_decision_miss"]:
        lines.append(
            f"- tick={row['tick']} {row['scope_kind']} {row['label']} family={row['family']} "
            f"best_action={row['best_action']} counterfactual={row['counterfactual_best_action']} "
            f"regret={row['wait_regret']} follow={row['follow_realized_return']} "
            f"fade={row['fade_realized_return']}"
        )
    lines.append("")

    lines.append("## Top Wait Miss")
    for row in report["top_wait_miss"]:
        lines.append(
            f"- tick={row['tick']} {row['symbol']} family={row['family']} "
            f"counterfactual={row['counterfactual_best_action']} regret={row['wait_regret']} "
            f"follow={row['follow_realized_return']} fade={row['fade_realized_return']}"
        )
        for factor in row["decisive_factors"]:
            lines.append(f"  factor: {factor}")
    lines.append("")
    return "\n".join(lines) + "\n"


def main() -> int:
    args = parse_args()
    journal_path = Path(args.journal_path).resolve()
    items = load_items(journal_path)
    report = build_report(items, args.top)
    print(json.dumps(report, ensure_ascii=False, indent=2))

    if args.output_prefix:
        prefix = Path(args.output_prefix)
        prefix.parent.mkdir(parents=True, exist_ok=True)
        prefix.with_suffix(".json").write_text(
            json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        prefix.with_suffix(".md").write_text(
            to_markdown(report, journal_path), encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
