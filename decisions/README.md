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

Requires `jsonschema` Python package:

```bash
python3 -m pip install --user --break-system-packages jsonschema
```

## Future: Eden ingestor

A future spec (`2026-04-19-belief-persistence-design.md`) will add a Rust-side ingestor that reads this tree daily and feeds decisions into Eden's belief field. Contract:

- `decision_id` unique + sortable
- `timestamp` ISO8601 + Z (UTC)
- `schema_version` explicit
- `eden_session.tick_seq` alignable with tick archives
