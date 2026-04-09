# Eden Scripts

Scripts are now grouped by role:

- `analysis/`
  offline analysis helpers
- `data/`
  recording and replay helpers
- `ops/`
  operational utilities and local run helpers
- `tests/`
  gate and smoke entrypoints

Current layout:

- `analysis/analyze_recommendation_journal.py`
- `data/record_hk_ticks.py`
- `data/replay_parquet.py`
- `ops/day_run_supervisor.py`
- `ops/health_report.mjs`
- `ops/run_codex_analyst.py`
- `tests/test_gate.sh`
- `tests/test_gate_persistence.sh`
