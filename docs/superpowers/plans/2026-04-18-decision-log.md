# Decision Log Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a machine-parseable decision log format + backfill 3 historical decisions, closing the first piece of Eden ↔ Claude Code feedback loop.

**Architecture:** Per-decision JSON files organized by date, with JSON Schema validators and a per-day `index.jsonl`. Zero Rust code changes — this creates a structured contract that a future Eden ingestor will consume. The work is fully isolated from existing branch changes.

**Tech Stack:**
- JSON Schema Draft-07 for validation
- `jsonschema` CLI (Python, `pip install jsonschema[format]`) for validation
- `jq` for bash-level index generation
- Bash shell scripts under `scripts/ops/`

**Spec:** `docs/superpowers/specs/2026-04-18-decision-log-design.md`

---

## File Structure

**New files created:**
```
decisions/
├── README.md                                     # usage guide for Claude Code
├── schemas/
│   ├── v1.json                                   # decision record schema
│   ├── session-recap-v1.json                     # daily recap schema
│   └── samples/
│       ├── entry-example.json                    # schema demo — entry
│       ├── exit-example.json                     # schema demo — exit with outcome+retrospective
│       ├── skip-example.json                     # schema demo — skip
│       ├── session-recap-example.json            # schema demo — recap
│       └── invalid-example.json                  # negative test for validator
└── 2026/
    └── 04/
        └── 15/                                   # real backfilled session
            ├── index.jsonl                       # generated
            ├── 164600Z-US-KC.US-entry.json       # from operator_session_us_2026-04-15_v2.md R1
            ├── 164900Z-US-KC.US-exit.json        # from same, R2
            └── 164900Z-US-HUBS.US-entry.json     # from same, R2

scripts/
└── ops/
    └── generate_decision_index.sh                # bash + jq index generator
```

**Files modified:** None. (No Rust code touched.)

---

## Branch Decision

Commits go on **current branch** (`codex/polymarket-convergence`). The new files are fully isolated from in-progress Rust work. No merge conflict risk.

---

## Task 1: Scaffold directory tree + README + validator

**Files:**
- Create: `decisions/README.md`
- Create: `decisions/schemas/` (empty — populated later)
- Create: `decisions/schemas/samples/` (empty — populated later)

- [ ] **Step 1: Create directory skeleton**

```bash
mkdir -p decisions/schemas/samples
mkdir -p decisions/2026/04/15
mkdir -p scripts/ops
```

- [ ] **Step 2: Verify `jsonschema` CLI available**

Run: `python3 -c "import jsonschema; print(jsonschema.__version__)"`
Expected: prints version (e.g. `4.x.x`). If ModuleNotFoundError:

```bash
pip3 install 'jsonschema[format]'
```

- [ ] **Step 3: Write `decisions/README.md`**

```markdown
# Decisions — Eden ↔ Claude Code Log

Structured JSON record of every decision Claude Code makes while reading Eden wake output.

See spec: `../docs/superpowers/specs/2026-04-18-decision-log-design.md`.

## Layout

```
decisions/
├── schemas/v1.json                    # JSON Schema for decision records
├── schemas/session-recap-v1.json      # JSON Schema for daily recap
├── schemas/samples/                   # illustrative examples
└── YYYY/MM/DD/
    ├── HHMMSSZ-MARKET-SYMBOL-ACTION.json    # one file per decision
    ├── session-recap.json                    # end-of-session aggregate
    └── index.jsonl                           # flat index (generated)
```

## Filename convention

`HHMMSSZ-MARKET-SYMBOL-ACTION.json` where:
- `HHMMSSZ`: UTC time, Zulu-suffixed (e.g. `093147Z`)
- `MARKET`: `HK` or `US`
- `SYMBOL`: e.g. `0700.HK`, `NVDA.US`
- `ACTION`: `entry` | `exit` | `skip` | `size_change`

Sortable + unique per decision.

## Writing a decision — quick guide

1. Eden emits wake. Claude Code reads it and decides to act (or skip).
2. Claude Code writes one JSON file to `decisions/YYYY/MM/DD/` following `schemas/v1.json`.
3. At exit: write a second JSON (same layout, `action: "exit"`), populate `outcome` and `retrospective`, link via `execution.linked_entry_id`.
4. For skip: `action: "skip"`, `execution: null`, `claude.reasoning` required.
5. End of session: write `session-recap.json` following `schemas/session-recap-v1.json`.
6. Regenerate index: `./scripts/ops/generate_decision_index.sh decisions/YYYY/MM/DD`.

## Validating a decision file

```bash
python3 -m jsonschema -i decisions/2026/04/15/164600Z-US-KC.US-entry.json decisions/schemas/v1.json
```

Exit 0 = valid. Non-zero + stderr message = invalid.

## Future: Eden ingestor

A future spec (`2026-04-19-belief-persistence-design.md`) will add a Rust-side ingestor that reads this tree daily and feeds decisions into Eden's belief field. Contract:
- `decision_id` unique + sortable
- `timestamp` ISO8601 + Z (UTC)
- `schema_version` explicit
- `eden_session.tick_seq` alignable with tick archives
```

- [ ] **Step 4: Verify files exist + commit**

```bash
ls -la decisions/
ls -la decisions/schemas/
ls -la decisions/schemas/samples/
```

Expected: all three directories listed. `README.md` inside `decisions/`.

```bash
git add decisions/README.md
git commit -m "$(cat <<'EOF'
feat(decisions): scaffold directory + README for Eden↔CC decision log

First piece of bidirectional Eden↔Claude Code loop. No Rust changes.
Spec: docs/superpowers/specs/2026-04-18-decision-log-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Write decision JSON Schema v1

**Files:**
- Create: `decisions/schemas/v1.json`

- [ ] **Step 1: Write schema file**

Create `decisions/schemas/v1.json` with full content:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Eden Decision Record v1",
  "description": "A single decision made by Claude Code while reading Eden wake output. See docs/superpowers/specs/2026-04-18-decision-log-design.md",
  "type": "object",
  "required": [
    "schema_version",
    "decision_id",
    "timestamp",
    "market",
    "symbol",
    "action",
    "eden_session",
    "claude",
    "metadata"
  ],
  "additionalProperties": false,
  "properties": {
    "schema_version": { "type": "integer", "const": 1 },
    "decision_id": {
      "type": "string",
      "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}-[0-9]{2}-[0-9]{2}Z-(HK|US)-[A-Z0-9.]+-(entry|exit|skip|size_change)$"
    },
    "timestamp": { "type": "string", "format": "date-time" },
    "market": { "type": "string", "enum": ["HK", "US"] },
    "symbol": { "type": "string", "minLength": 1 },
    "action": { "type": "string", "enum": ["entry", "exit", "skip", "size_change"] },
    "direction": { "type": ["string", "null"], "enum": ["long", "short", null] },
    "eden_session": {
      "type": "object",
      "required": ["binary", "tick_seq", "wake_excerpt"],
      "additionalProperties": false,
      "properties": {
        "binary": { "type": "string", "enum": ["hk", "us"] },
        "tick_seq": { "type": "integer", "minimum": 0 },
        "stress_composite": { "type": ["number", "null"] },
        "wake_excerpt": { "type": "string", "minLength": 1 },
        "wake_context": { "type": "array", "items": { "type": "string" } },
        "supporting_evidence": { "type": "array", "items": { "type": "string" } },
        "opposing_evidence": { "type": "array", "items": { "type": "string" } },
        "missing_evidence": { "type": "array", "items": { "type": "string" } }
      }
    },
    "claude": {
      "type": "object",
      "required": ["reasoning", "confidence"],
      "additionalProperties": false,
      "properties": {
        "reasoning": { "type": "string", "minLength": 1 },
        "concerns": { "type": "array", "items": { "type": "string" } },
        "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
        "prior_knowledge_used": { "type": "array", "items": { "type": "string" } },
        "alternatives_considered": { "type": "array", "items": { "type": "string" } },
        "decision_rationale": { "type": "string" }
      }
    },
    "execution": {
      "type": ["object", "null"],
      "additionalProperties": false,
      "properties": {
        "price": { "type": "string" },
        "size_bps": { "type": "integer" },
        "size_notional_hkd": { "type": ["number", "null"] },
        "size_notional_usd": { "type": ["number", "null"] },
        "linked_entry_id": { "type": ["string", "null"] },
        "broker_order_id": { "type": ["string", "null"] },
        "paper_or_real": { "type": "string", "enum": ["paper", "real", "simulated", "backtest"] }
      }
    },
    "outcome": {
      "type": ["object", "null"],
      "additionalProperties": false,
      "properties": {
        "exit_timestamp": { "type": "string", "format": "date-time" },
        "exit_price": { "type": "string" },
        "hold_duration_sec": { "type": "integer", "minimum": 0 },
        "pnl_bps": { "type": "number" },
        "pnl_abs_hkd": { "type": ["number", "null"] },
        "pnl_abs_usd": { "type": ["number", "null"] },
        "closing_reason": { "type": "string" }
      }
    },
    "retrospective": {
      "type": ["object", "null"],
      "additionalProperties": false,
      "properties": {
        "what_worked": { "type": "string" },
        "what_didnt": { "type": "string" },
        "would_do_differently": { "type": "string" },
        "new_pattern_observed": { "type": "string" },
        "eden_gap": { "type": "string" }
      }
    },
    "metadata": {
      "type": "object",
      "required": ["backfilled", "created_at"],
      "additionalProperties": false,
      "properties": {
        "backfilled": { "type": "boolean" },
        "backfill_source": { "type": ["string", "null"] },
        "created_at": { "type": "string", "format": "date-time" },
        "updated_at": { "type": ["string", "null"], "format": "date-time" }
      }
    }
  }
}
```

- [ ] **Step 2: Syntax-validate the schema itself**

Run: `python3 -c "import json; json.load(open('decisions/schemas/v1.json'))"`
Expected: no output, exit 0 (valid JSON)

- [ ] **Step 3: Meta-validate as a JSON Schema**

Run:
```bash
python3 -c "
import json
from jsonschema import Draft7Validator
schema = json.load(open('decisions/schemas/v1.json'))
Draft7Validator.check_schema(schema)
print('schema is a valid Draft-07 JSON Schema')
"
```

Expected: `schema is a valid Draft-07 JSON Schema`

- [ ] **Step 4: Commit**

```bash
git add decisions/schemas/v1.json
git commit -m "$(cat <<'EOF'
feat(decisions): add JSON Schema v1 for decision records

Draft-07 schema covering eden_session (input), claude (reasoning),
execution (action), outcome (result), retrospective (ground truth).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Write session-recap JSON Schema v1

**Files:**
- Create: `decisions/schemas/session-recap-v1.json`

- [ ] **Step 1: Write schema file**

Create `decisions/schemas/session-recap-v1.json`:

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "Eden Session Recap v1",
  "description": "End-of-session aggregate across all decisions in a single day. Cross-references decision files + summarizes recurring themes and eden_gap patterns.",
  "type": "object",
  "required": [
    "schema_version",
    "session_date",
    "market",
    "eden_session_stats",
    "decisions_summary",
    "recurring_themes",
    "eden_gap_patterns",
    "metadata"
  ],
  "additionalProperties": false,
  "properties": {
    "schema_version": { "type": "integer", "const": 1 },
    "session_date": { "type": "string", "format": "date" },
    "market": { "type": "string", "enum": ["HK", "US"] },
    "eden_session_stats": {
      "type": "object",
      "required": ["first_tick", "last_tick", "total_ticks"],
      "additionalProperties": false,
      "properties": {
        "first_tick": { "type": "integer", "minimum": 0 },
        "last_tick": { "type": "integer", "minimum": 0 },
        "total_ticks": { "type": "integer", "minimum": 0 },
        "binary_version": { "type": ["string", "null"] }
      }
    },
    "decisions_summary": {
      "type": "object",
      "required": ["total", "entries", "exits", "skips"],
      "additionalProperties": false,
      "properties": {
        "total": { "type": "integer", "minimum": 0 },
        "entries": { "type": "integer", "minimum": 0 },
        "exits": { "type": "integer", "minimum": 0 },
        "skips": { "type": "integer", "minimum": 0 },
        "size_changes": { "type": "integer", "minimum": 0 },
        "net_pnl_bps": { "type": ["number", "null"] },
        "hit_rate": { "type": ["number", "null"], "minimum": 0, "maximum": 1 }
      }
    },
    "recurring_themes": {
      "type": "array",
      "description": "Patterns that appeared multiple times across decisions today",
      "items": {
        "type": "object",
        "required": ["theme", "occurrence_count", "decision_ids"],
        "additionalProperties": false,
        "properties": {
          "theme": { "type": "string" },
          "occurrence_count": { "type": "integer", "minimum": 1 },
          "decision_ids": { "type": "array", "items": { "type": "string" } }
        }
      }
    },
    "eden_gap_patterns": {
      "type": "array",
      "description": "Unique eden_gap values across today's retrospectives, ranked by recurrence",
      "items": {
        "type": "object",
        "required": ["gap", "occurrence_count", "decision_ids"],
        "additionalProperties": false,
        "properties": {
          "gap": { "type": "string" },
          "occurrence_count": { "type": "integer", "minimum": 1 },
          "decision_ids": { "type": "array", "items": { "type": "string" } }
        }
      }
    },
    "session_reflection": { "type": "string" },
    "metadata": {
      "type": "object",
      "required": ["created_at"],
      "additionalProperties": false,
      "properties": {
        "created_at": { "type": "string", "format": "date-time" },
        "backfilled": { "type": "boolean" },
        "backfill_source": { "type": ["string", "null"] }
      }
    }
  }
}
```

- [ ] **Step 2: Syntax + meta-validate**

Run:
```bash
python3 -c "
import json
from jsonschema import Draft7Validator
schema = json.load(open('decisions/schemas/session-recap-v1.json'))
Draft7Validator.check_schema(schema)
print('session-recap schema valid')
"
```

Expected: `session-recap schema valid`

- [ ] **Step 3: Commit**

```bash
git add decisions/schemas/session-recap-v1.json
git commit -m "$(cat <<'EOF'
feat(decisions): add JSON Schema v1 for session recap

Aggregates decisions across a single session day, surfaces recurring
themes and eden_gap patterns for Eden ingestor to learn attention priors.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Write schema samples + negative test

**Files:**
- Create: `decisions/schemas/samples/entry-example.json`
- Create: `decisions/schemas/samples/exit-example.json`
- Create: `decisions/schemas/samples/skip-example.json`
- Create: `decisions/schemas/samples/session-recap-example.json`
- Create: `decisions/schemas/samples/invalid-example.json`

- [ ] **Step 1: Write entry sample**

Create `decisions/schemas/samples/entry-example.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-18T09-31-47Z-HK-0700.HK-entry",
  "timestamp": "2026-04-18T09:31:47Z",
  "market": "HK",
  "symbol": "0700.HK",
  "action": "entry",
  "direction": "long",
  "eden_session": {
    "binary": "hk",
    "tick_seq": 12,
    "stress_composite": 0.48,
    "wake_excerpt": "inference: 0700.HK broad institutional deployment conf=0.72",
    "wake_context": [
      "inference: 0700.HK broad institutional deployment conf=0.72",
      "institution rotation: BOCI +0.21, JPM +0.18",
      "hidden forces confirmed: 47 symbols"
    ],
    "supporting_evidence": ["BOCI rotation", "broker 157/26 buy skew"],
    "opposing_evidence": ["peer 3690.HK no sync move"],
    "missing_evidence": ["no matching option flow HK-side"]
  },
  "claude": {
    "reasoning": "T22 chain at 0.72 + institutional rotation confirms buy side. Broker queue 157/26 is raw evidence of distribution to accumulation flip.",
    "concerns": ["3690.HK lag suggests single-name story not sector"],
    "confidence": 0.78,
    "prior_knowledge_used": [
      "HK edge in raw_microstructure (CLAUDE.md)",
      "T22 conf>0.7 historically 65%+ hit"
    ],
    "alternatives_considered": ["wait for 3690.HK confirmation", "size half"],
    "decision_rationale": "T22 signal + institutional rotation dual-confirm outweighs single-name concern; size reduced to half as concession"
  },
  "execution": {
    "price": "342.80",
    "size_bps": 25,
    "size_notional_hkd": 25000,
    "size_notional_usd": null,
    "linked_entry_id": null,
    "broker_order_id": null,
    "paper_or_real": "paper"
  },
  "outcome": null,
  "retrospective": null,
  "metadata": {
    "backfilled": false,
    "backfill_source": null,
    "created_at": "2026-04-18T09:31:47Z",
    "updated_at": null
  }
}
```

- [ ] **Step 2: Write exit sample (linked, with outcome + retrospective)**

Create `decisions/schemas/samples/exit-example.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-18T10-15-33Z-HK-0700.HK-exit",
  "timestamp": "2026-04-18T10:15:33Z",
  "market": "HK",
  "symbol": "0700.HK",
  "action": "exit",
  "direction": null,
  "eden_session": {
    "binary": "hk",
    "tick_seq": 87,
    "stress_composite": 0.41,
    "wake_excerpt": "backward: 0700.HK broker queue dominance fading",
    "wake_context": [
      "backward: 0700.HK broker queue dominance fading",
      "stable: BOCI rotation now flat"
    ],
    "supporting_evidence": ["queue dominance drop"],
    "opposing_evidence": [],
    "missing_evidence": []
  },
  "claude": {
    "reasoning": "Entry thesis (institutional deployment) confirmed via price move; queue dominance fading = signal peak. Follow exit-on-fade discipline.",
    "concerns": [],
    "confidence": 0.82,
    "prior_knowledge_used": [
      "Exit on momentum derivative (feedback memory)",
      "Never use arbitrary % stops; use Eden signals"
    ],
    "alternatives_considered": ["hold for afternoon session"],
    "decision_rationale": "Eden signal faded first — honor exit discipline even with small gain"
  },
  "execution": {
    "price": "345.60",
    "size_bps": 25,
    "size_notional_hkd": 25000,
    "size_notional_usd": null,
    "linked_entry_id": "2026-04-18T09-31-47Z-HK-0700.HK-entry",
    "broker_order_id": null,
    "paper_or_real": "paper"
  },
  "outcome": {
    "exit_timestamp": "2026-04-18T10:15:33Z",
    "exit_price": "345.60",
    "hold_duration_sec": 2626,
    "pnl_bps": 82,
    "pnl_abs_hkd": 204.5,
    "pnl_abs_usd": null,
    "closing_reason": "eden_signal_faded"
  },
  "retrospective": {
    "what_worked": "T22 signal direction correct; broker queue dominance was effective early read",
    "what_didnt": "Entry was 5 min early — T22 first fire without persistence confirmation",
    "would_do_differently": "Wait one tick persistence after T22 first fire before entry",
    "new_pattern_observed": "BOCI rotation leads JPM rotation by ~3 ticks — unnoticed before",
    "eden_gap": "missing_evidence did not include 'peer_synchronization' — should be surfaced as opposing dimension"
  },
  "metadata": {
    "backfilled": false,
    "backfill_source": null,
    "created_at": "2026-04-18T10:15:33Z",
    "updated_at": null
  }
}
```

- [ ] **Step 3: Write skip sample**

Create `decisions/schemas/samples/skip-example.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-18T11-55-00Z-HK-3690.HK-skip",
  "timestamp": "2026-04-18T11:55:00Z",
  "market": "HK",
  "symbol": "3690.HK",
  "action": "skip",
  "direction": null,
  "eden_session": {
    "binary": "hk",
    "tick_seq": 151,
    "stress_composite": 0.52,
    "wake_excerpt": "inference: 3690.HK cross-sector rotation conf=0.61",
    "wake_context": [
      "inference: 3690.HK cross-sector rotation conf=0.61",
      "shared holders anomaly: 5 funds shifted"
    ],
    "supporting_evidence": ["5 fund shifts"],
    "opposing_evidence": ["conf below 0.7 threshold", "peer 9618.HK flat"],
    "missing_evidence": ["no broker queue confirmation"]
  },
  "claude": {
    "reasoning": "Confidence 0.61 below threshold; fund shift is 1 of 3 needed signals; peer ambiguous. Insufficient conviction.",
    "concerns": ["FOMO if this is early signal"],
    "confidence": 0.35,
    "prior_knowledge_used": [
      "Conf>=0.7 required for entry (v2 discipline)"
    ],
    "alternatives_considered": ["observe-tier tracking without entry"],
    "decision_rationale": "Discipline over FOMO; record as skip for Eden to learn which sub-threshold signals turn real"
  },
  "execution": null,
  "outcome": null,
  "retrospective": null,
  "metadata": {
    "backfilled": false,
    "backfill_source": null,
    "created_at": "2026-04-18T11:55:00Z",
    "updated_at": null
  }
}
```

- [ ] **Step 4: Write session-recap sample**

Create `decisions/schemas/samples/session-recap-example.json`:

```json
{
  "schema_version": 1,
  "session_date": "2026-04-18",
  "market": "HK",
  "eden_session_stats": {
    "first_tick": 1,
    "last_tick": 197,
    "total_ticks": 197,
    "binary_version": "e18c34c"
  },
  "decisions_summary": {
    "total": 3,
    "entries": 1,
    "exits": 1,
    "skips": 1,
    "size_changes": 0,
    "net_pnl_bps": 82,
    "hit_rate": 1.0
  },
  "recurring_themes": [
    {
      "theme": "institutional_rotation_as_confirmation",
      "occurrence_count": 2,
      "decision_ids": [
        "2026-04-18T09-31-47Z-HK-0700.HK-entry",
        "2026-04-18T10-15-33Z-HK-0700.HK-exit"
      ]
    }
  ],
  "eden_gap_patterns": [
    {
      "gap": "peer_synchronization missing from missing_evidence surface",
      "occurrence_count": 1,
      "decision_ids": ["2026-04-18T10-15-33Z-HK-0700.HK-exit"]
    }
  ],
  "session_reflection": "Single clean setup on 0700.HK demonstrated T22 + institutional rotation as viable dual-confirm. Skip on 3690.HK was correct — sub-threshold signal did not materialize. Recurring pattern: peer_synchronization should be first-class in Eden's opposing_evidence surface.",
  "metadata": {
    "created_at": "2026-04-18T16:00:00Z",
    "backfilled": false,
    "backfill_source": null
  }
}
```

- [ ] **Step 5: Write invalid sample (negative test)**

Create `decisions/schemas/samples/invalid-example.json`:

```json
{
  "schema_version": 2,
  "decision_id": "bad-id-format",
  "timestamp": "not-a-timestamp",
  "market": "INVALID_MARKET",
  "symbol": "",
  "action": "teleport",
  "eden_session": {
    "binary": "xx",
    "tick_seq": -5,
    "wake_excerpt": ""
  },
  "claude": {
    "reasoning": "",
    "confidence": 1.5
  },
  "metadata": {
    "backfilled": "not-a-boolean",
    "created_at": "bad"
  }
}
```

Five intentional violations: bad schema_version, bad decision_id pattern, bad timestamp, bad market enum, bad action enum (+ others).

- [ ] **Step 6: Validate all valid samples pass**

Run:
```bash
for sample in entry exit skip; do
  python3 -m jsonschema -i "decisions/schemas/samples/${sample}-example.json" decisions/schemas/v1.json && echo "${sample}: PASS" || echo "${sample}: FAIL"
done
python3 -m jsonschema -i decisions/schemas/samples/session-recap-example.json decisions/schemas/session-recap-v1.json && echo "recap: PASS" || echo "recap: FAIL"
```

Expected:
```
entry: PASS
exit: PASS
skip: PASS
recap: PASS
```

- [ ] **Step 7: Validate invalid sample FAILS**

Run:
```bash
python3 -m jsonschema -i decisions/schemas/samples/invalid-example.json decisions/schemas/v1.json
```

Expected: non-zero exit + errors printed to stderr (at least one validation failure).

- [ ] **Step 8: Commit**

```bash
git add decisions/schemas/samples/
git commit -m "$(cat <<'EOF'
feat(decisions): add schema samples + negative test

Four positive samples (entry, exit, skip, session-recap) demonstrate
full schema coverage. One invalid sample verifies schema rejects bad input.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Write index.jsonl generator script

**Files:**
- Create: `scripts/ops/generate_decision_index.sh`

- [ ] **Step 1: Verify `jq` available**

Run: `jq --version`
Expected: prints version (e.g. `jq-1.6` or `jq-1.7`). If not found:

```bash
brew install jq
```

- [ ] **Step 2: Write generator script**

Create `scripts/ops/generate_decision_index.sh`:

```bash
#!/usr/bin/env bash
# Generate index.jsonl for a decisions/YYYY/MM/DD directory.
#
# Usage:
#   ./scripts/ops/generate_decision_index.sh decisions/2026/04/15
#
# For each *.json file in the directory (excluding index.jsonl and
# session-recap.json), emit one line in index.jsonl with a compact
# summary: decision_id, timestamp, action, market, symbol, file.
#
# Exit codes:
#   0 on success
#   1 on bad usage
#   2 on invalid input directory
#   3 if any decision file fails to parse

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <day_dir>" >&2
  echo "Example: $0 decisions/2026/04/15" >&2
  exit 1
fi

DAY_DIR="$1"

if [[ ! -d "$DAY_DIR" ]]; then
  echo "error: not a directory: $DAY_DIR" >&2
  exit 2
fi

INDEX_PATH="$DAY_DIR/index.jsonl"
TMP_PATH="$(mktemp)"
trap 'rm -f "$TMP_PATH"' EXIT

count=0
for f in "$DAY_DIR"/*.json; do
  # Skip non-files (e.g. empty glob expansion when no .json files)
  [[ -e "$f" ]] || continue

  base="$(basename "$f")"

  # Skip the index itself and session-recap (different schema)
  case "$base" in
    index.jsonl) continue ;;
    session-recap.json) continue ;;
  esac

  if ! jq -c '{
    decision_id,
    timestamp,
    action,
    market,
    symbol,
    file: input_filename | split("/") | last
  }' "$f" >> "$TMP_PATH" 2>/dev/null; then
    echo "error: failed to parse $f" >&2
    exit 3
  fi

  count=$((count + 1))
done

# Sort by timestamp for deterministic output
sort -t '"' -k8 "$TMP_PATH" > "$INDEX_PATH"

echo "wrote $count decisions → $INDEX_PATH"
```

- [ ] **Step 3: Make executable**

```bash
chmod +x scripts/ops/generate_decision_index.sh
```

- [ ] **Step 4: Test on schemas/samples dir (should generate index of 3 decisions, skipping recap)**

Setup test:
```bash
mkdir -p /tmp/decision-test/
cp decisions/schemas/samples/entry-example.json /tmp/decision-test/
cp decisions/schemas/samples/exit-example.json /tmp/decision-test/
cp decisions/schemas/samples/skip-example.json /tmp/decision-test/
cp decisions/schemas/samples/session-recap-example.json /tmp/decision-test/session-recap.json
```

Run:
```bash
./scripts/ops/generate_decision_index.sh /tmp/decision-test
cat /tmp/decision-test/index.jsonl
```

Expected output of `index.jsonl`:
```
{"decision_id":"2026-04-18T09-31-47Z-HK-0700.HK-entry","timestamp":"2026-04-18T09:31:47Z","action":"entry","market":"HK","symbol":"0700.HK","file":"entry-example.json"}
{"decision_id":"2026-04-18T10-15-33Z-HK-0700.HK-exit","timestamp":"2026-04-18T10:15:33Z","action":"exit","market":"HK","symbol":"0700.HK","file":"exit-example.json"}
{"decision_id":"2026-04-18T11-55-00Z-HK-3690.HK-skip","timestamp":"2026-04-18T11:55:00Z","action":"skip","market":"HK","symbol":"3690.HK","file":"skip-example.json"}
```

Expected: 3 lines, sorted chronologically, session-recap excluded.

- [ ] **Step 5: Test error handling — empty directory**

Run:
```bash
mkdir -p /tmp/decision-test-empty
./scripts/ops/generate_decision_index.sh /tmp/decision-test-empty
cat /tmp/decision-test-empty/index.jsonl
```

Expected: `wrote 0 decisions → /tmp/decision-test-empty/index.jsonl`, empty file written.

- [ ] **Step 6: Test error handling — bad argument**

Run:
```bash
./scripts/ops/generate_decision_index.sh /nonexistent/path
```

Expected: stderr "error: not a directory: /nonexistent/path", exit code 2.

- [ ] **Step 7: Cleanup test dirs**

```bash
rm -rf /tmp/decision-test /tmp/decision-test-empty
```

- [ ] **Step 8: Commit**

```bash
git add scripts/ops/generate_decision_index.sh
git commit -m "$(cat <<'EOF'
feat(decisions): add index.jsonl generator script

Bash + jq script generates per-day index.jsonl from decision JSON files.
Sorted chronologically, excludes session-recap.json and index itself.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Backfill 3 decisions from 2026-04-15 US session

**Files:**
- Create: `decisions/2026/04/15/164600Z-US-KC.US-entry.json`
- Create: `decisions/2026/04/15/164900Z-US-KC.US-exit.json`
- Create: `decisions/2026/04/15/164900Z-US-HUBS.US-entry.json`

**Source:** `docs/operator_session_us_2026-04-15_v2.md` R1 (KC entry), R2 (KC exit + HUBS entry).

- [ ] **Step 1: Re-read source for ground truth**

Read `docs/operator_session_us_2026-04-15_v2.md` lines 23-77 to confirm:
- KC.US entry: 16:46 UTC, short 180 @ $16.65, conf 1.0, support_fraction 0.75, rrc stale_symbol_confirmation
- KC.US exit: 16:49 UTC, cover 180 @ $16.68, PnL -$5.40, hold 3 min, reason "Eden signal faded"
- HUBS.US entry: 16:49 UTC, long 14 @ $214.55, conf 1.0, sf 0.75

- [ ] **Step 2: Write KC entry backfill**

Create `decisions/2026/04/15/164600Z-US-KC.US-entry.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-15T16-46-00Z-US-KC.US-entry",
  "timestamp": "2026-04-15T16:46:00Z",
  "market": "US",
  "symbol": "KC.US",
  "action": "entry",
  "direction": "short",
  "eden_session": {
    "binary": "us",
    "tick_seq": 39,
    "stress_composite": null,
    "wake_excerpt": "KC.US Short qualifying case (conf=1.0, sf=0.75, rrc=stale_symbol_confirmation)",
    "wake_context": [
      "Eden US restart tick 39, scorecard 0/0 pre-first-resolution",
      "position_in_range 66.7% (not at day low)"
    ],
    "supporting_evidence": [
      "confidence 1.0",
      "support_fraction 0.75",
      "position_in_range 66.7% (valid timing — not at day low)"
    ],
    "opposing_evidence": [
      "rrc=stale_symbol_confirmation (soft block, overridable in v2)"
    ],
    "missing_evidence": []
  },
  "claude": {
    "reasoning": "v2 discipline qualifies: conf>=0.7, sf>=0.67, rrc allowed override. Position_in_range shows Short not at day low so timing valid. Session 1 had 120 rounds of 0-enter streak; this is first actual trade.",
    "concerns": [
      "Fresh restart, scorecard 0/0 — no realized baseline yet"
    ],
    "confidence": 0.85,
    "prior_knowledge_used": [
      "v2 discipline rules (conf>=0.7 + sf>=0.67 + soft-block rrc override)",
      "size $3k fixed"
    ],
    "alternatives_considered": [],
    "decision_rationale": "All v2 gates pass + first qualifier after 120-round streak — execute per discipline"
  },
  "execution": {
    "price": "16.65",
    "size_bps": null,
    "size_notional_hkd": null,
    "size_notional_usd": 2997,
    "linked_entry_id": null,
    "broker_order_id": "1229110611837726720",
    "paper_or_real": "real"
  },
  "outcome": null,
  "retrospective": null,
  "metadata": {
    "backfilled": true,
    "backfill_source": "docs/operator_session_us_2026-04-15_v2.md#R1",
    "created_at": "2026-04-18T00:00:00Z",
    "updated_at": null
  }
}
```

- [ ] **Step 3: Write KC exit backfill (with outcome + retrospective)**

Create `decisions/2026/04/15/164900Z-US-KC.US-exit.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-15T16-49-00Z-US-KC.US-exit",
  "timestamp": "2026-04-15T16:49:00Z",
  "market": "US",
  "symbol": "KC.US",
  "action": "exit",
  "direction": null,
  "eden_session": {
    "binary": "us",
    "tick_seq": 195,
    "stress_composite": null,
    "wake_excerpt": "KC.US case disappeared from roster → v2 exit rule 1 triggered",
    "wake_context": [
      "AHR 49.12% / excess 8.50pp / baseline 40.62% / ares 3288",
      "excess_over_baseline first populated — 8.50pp selectivity edge"
    ],
    "supporting_evidence": [
      "Eden case roster drop (v2 exit rule 1)"
    ],
    "opposing_evidence": [],
    "missing_evidence": [
      "no price-action-based exit trigger"
    ]
  },
  "claude": {
    "reasoning": "v2 exit discipline: case disappearance from roster triggers cover regardless of P&L.",
    "concerns": [
      "3-min hold is very short — case roster churn could be noise, not real signal fade"
    ],
    "confidence": 0.6,
    "prior_knowledge_used": [
      "Eden-signal exit rule (never use arbitrary % stops)",
      "KC 3-min whipsaw lesson — case roster churn ≠ signal fade"
    ],
    "alternatives_considered": [
      "hold until price-action confirms fade",
      "switch to momentum-derivative exit instead of roster churn"
    ],
    "decision_rationale": "Follow v2 rules as written but flag the whipsaw concern for post-session retrospective"
  },
  "execution": {
    "price": "16.68",
    "size_bps": null,
    "size_notional_hkd": null,
    "size_notional_usd": 3002.4,
    "linked_entry_id": "2026-04-15T16-46-00Z-US-KC.US-entry",
    "broker_order_id": null,
    "paper_or_real": "real"
  },
  "outcome": {
    "exit_timestamp": "2026-04-15T16:49:00Z",
    "exit_price": "16.68",
    "hold_duration_sec": 180,
    "pnl_bps": -18,
    "pnl_abs_hkd": null,
    "pnl_abs_usd": -5.40,
    "closing_reason": "eden_signal_faded"
  },
  "retrospective": {
    "what_worked": "v2 exit rule fired deterministically — discipline honored",
    "what_didnt": "3-min hold was too short; case roster churn may not reflect real signal fade",
    "would_do_differently": "Add velocity+acceleration check before exit on roster churn (per feedback_exit_on_momentum_derivative)",
    "new_pattern_observed": "Case roster can flip within 3 min without price fade — roster is noisier than momentum signal",
    "eden_gap": "Eden emits case-roster event but not signal-momentum-derivative; exit decisions lack second-order signal read"
  },
  "metadata": {
    "backfilled": true,
    "backfill_source": "docs/operator_session_us_2026-04-15_v2.md#R2",
    "created_at": "2026-04-18T00:00:00Z",
    "updated_at": null
  }
}
```

- [ ] **Step 4: Write HUBS entry backfill**

Create `decisions/2026/04/15/164900Z-US-HUBS.US-entry.json`:

```json
{
  "schema_version": 1,
  "decision_id": "2026-04-15T16-49-00Z-US-HUBS.US-entry",
  "timestamp": "2026-04-15T16:49:00Z",
  "market": "US",
  "symbol": "HUBS.US",
  "action": "entry",
  "direction": "long",
  "eden_session": {
    "binary": "us",
    "tick_seq": 195,
    "stress_composite": null,
    "wake_excerpt": "HUBS.US Long qualifying (conf=1, sf=0.75, rrc=stale_symbol_confirmation, pos_in_range 44.1%)",
    "wake_context": [
      "AHR 49.12% / excess 8.50pp / baseline 40.62% / ares 3288 (same tick as KC exit)"
    ],
    "supporting_evidence": [
      "confidence 1.0",
      "support_fraction 0.75",
      "position_in_range 44.1% (mid-range — valid long timing)"
    ],
    "opposing_evidence": [
      "rrc=stale_symbol_confirmation (soft block, overridable)"
    ],
    "missing_evidence": []
  },
  "claude": {
    "reasoning": "v2 discipline qualifies: conf>=0.7, sf>=0.67, rrc allowed override. Mid-range timing valid for long entry.",
    "concerns": [
      "Just exited KC with -$5.40 on roster churn — same mechanism could whipsaw HUBS"
    ],
    "confidence": 0.7,
    "prior_knowledge_used": [
      "v2 discipline rules",
      "size $3k fixed"
    ],
    "alternatives_considered": [
      "skip given KC whipsaw just occurred",
      "size smaller"
    ],
    "decision_rationale": "v2 rules apply uniformly — don't skip based on previous trade outcome. But flag concern for retrospective."
  },
  "execution": {
    "price": "214.55",
    "size_bps": null,
    "size_notional_hkd": null,
    "size_notional_usd": 3003.70,
    "linked_entry_id": null,
    "broker_order_id": "1229111467756756992",
    "paper_or_real": "real"
  },
  "outcome": null,
  "retrospective": null,
  "metadata": {
    "backfilled": true,
    "backfill_source": "docs/operator_session_us_2026-04-15_v2.md#R2",
    "created_at": "2026-04-18T00:00:00Z",
    "updated_at": null
  }
}
```

- [ ] **Step 5: Validate all 3 backfilled files**

Run:
```bash
for f in decisions/2026/04/15/*.json; do
  [[ "$(basename "$f")" == "index.jsonl" ]] && continue
  python3 -m jsonschema -i "$f" decisions/schemas/v1.json && echo "$(basename "$f"): PASS" || echo "$(basename "$f"): FAIL"
done
```

Expected:
```
164600Z-US-KC.US-entry.json: PASS
164900Z-US-HUBS.US-entry.json: PASS
164900Z-US-KC.US-exit.json: PASS
```

- [ ] **Step 6: Commit**

```bash
git add decisions/2026/04/15/
git commit -m "$(cat <<'EOF'
feat(decisions): backfill 3 decisions from 2026-04-15 US session

KC.US entry + exit (3-min whipsaw, -$5.40) and HUBS.US entry,
extracted from docs/operator_session_us_2026-04-15_v2.md. All marked
backfilled=true with source pointer. Proves schema covers real data.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Generate index.jsonl for 2026/04/15

**Files:**
- Create: `decisions/2026/04/15/index.jsonl` (generated)

- [ ] **Step 1: Run generator**

Run:
```bash
./scripts/ops/generate_decision_index.sh decisions/2026/04/15
```

Expected: `wrote 3 decisions → decisions/2026/04/15/index.jsonl`

- [ ] **Step 2: Verify content**

Run: `cat decisions/2026/04/15/index.jsonl`

Expected (exact content, sorted by timestamp):
```
{"decision_id":"2026-04-15T16-46-00Z-US-KC.US-entry","timestamp":"2026-04-15T16:46:00Z","action":"entry","market":"US","symbol":"KC.US","file":"164600Z-US-KC.US-entry.json"}
{"decision_id":"2026-04-15T16-49-00Z-US-HUBS.US-entry","timestamp":"2026-04-15T16:49:00Z","action":"entry","market":"US","symbol":"HUBS.US","file":"164900Z-US-HUBS.US-entry.json"}
{"decision_id":"2026-04-15T16-49-00Z-US-KC.US-exit","timestamp":"2026-04-15T16:49:00Z","action":"exit","market":"US","symbol":"KC.US","file":"164900Z-US-KC.US-exit.json"}
```

3 lines. If ordering differs but content matches line-by-line, acceptable (secondary sort by file name).

- [ ] **Step 3: Verify line count**

Run: `wc -l decisions/2026/04/15/index.jsonl`
Expected: `3 decisions/2026/04/15/index.jsonl`

- [ ] **Step 4: Commit**

```bash
git add decisions/2026/04/15/index.jsonl
git commit -m "$(cat <<'EOF'
chore(decisions): add index.jsonl for 2026-04-15 backfilled session

Generated via scripts/ops/generate_decision_index.sh.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Final acceptance check + wrap up

- [ ] **Step 1: Verify spec acceptance criteria**

Check each criterion from `docs/superpowers/specs/2026-04-18-decision-log-design.md § 驗證`:

```bash
# AC1: JSON schema exists + validates samples
python3 -m jsonschema -i decisions/schemas/samples/entry-example.json decisions/schemas/v1.json && echo "AC1 part-a: PASS"

# AC2: README exists
[[ -f decisions/README.md ]] && echo "AC2: PASS"

# AC3: full-lifecycle samples exist (entry + exit linked)
jq -r '.execution.linked_entry_id' decisions/schemas/samples/exit-example.json
# Expected: "2026-04-18T09-31-47Z-HK-0700.HK-entry"
echo "AC3: verified"

# AC4: ≥3 backfilled decisions from 1 session file
ls decisions/2026/04/15/*.json | grep -v index.jsonl | wc -l
# Expected: 3
echo "AC4: verified"

# AC5: index.jsonl auto-generated
[[ -f decisions/2026/04/15/index.jsonl ]] && [[ $(wc -l < decisions/2026/04/15/index.jsonl) -eq 3 ]] && echo "AC5: PASS"
```

Expected: 5× PASS messages.

- [ ] **Step 2: Verify no unrelated damage**

Run:
```bash
git status
```

Expected: Working tree clean, on current branch. Commits from this plan visible in `git log --oneline -10`.

```bash
git log --oneline -8
```

Expected: last 7 commits are the ones from this plan (Tasks 1, 2, 3, 4, 5, 6, 7).

- [ ] **Step 3: Verify no Rust changes made inadvertently**

Run:
```bash
git log --oneline -7 -- 'src/**'
```

Expected: no results from this plan's 7 commits. (If any Rust files changed, this plan made a mistake — revert.)

- [ ] **Step 4: Print summary**

Print to stdout:
```
✓ Plan complete
  - decisions/README.md
  - decisions/schemas/v1.json (decision schema)
  - decisions/schemas/session-recap-v1.json (recap schema)
  - decisions/schemas/samples/ (4 valid + 1 invalid)
  - scripts/ops/generate_decision_index.sh
  - decisions/2026/04/15/ (3 backfilled + index.jsonl)

Next spec: 2026-04-19-belief-persistence-design.md (Eden ingestor)
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Task |
|------------------|------|
| JSON schema v1 exists + validates | Task 2, 4 |
| README with usage guide | Task 1 |
| Full entry/exit lifecycle sample | Task 4 (steps 1-2, linked via `linked_entry_id`) |
| ≥3 backfilled decisions | Task 6 |
| index.jsonl auto-generated | Task 5 (script) + Task 7 (run) |
| session-recap schema | Task 3 |
| skip decision support | Task 4 step 3, Task 6 could also but not needed |
| Decision on: retrospective timing | Schema supports both (per-exit + session-recap), demonstrated in Task 4 |
| Decision on: skip decisions recorded | Task 4 step 3 sample |
| Decision on: confidence not calibrated | v1 schema takes raw 0-1, no calibration step |
| Decision on: commit all to repo | Every task commits |
| Decision on: paper_or_real enum open | Schema step in Task 2 uses `enum` with 4 values, extensible |

All spec sections covered.

**Placeholder scan:** No TBDs, TODOs, "similar to" references, or missing code blocks. Every JSON sample has full content. Every schema is complete. Every command has expected output.

**Type consistency:**
- `decision_id` pattern used consistently (Task 2 defines, Tasks 4 & 6 produce matching IDs)
- `linked_entry_id` in exit always references valid entry decision_id (Task 4 step 2, Task 6 step 3)
- `schema_version: 1` everywhere
- `paper_or_real` used consistently: `"paper"` in 2026-04-18 samples, `"real"` in 2026-04-15 backfill (matches operator_session source where actual orders were placed)

No type or naming inconsistencies.
