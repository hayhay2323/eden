# Review Fix Progress

## P0
- [x] P0-1: resolved_tick/net_return 語義混亂
2026-04-03 note: `src/temporal/lineage/outcomes.rs` 改成先找 peak tick，再以 bounded records 重算 outcome，避免 `resolved_tick` 與 `net_return` 來自不同 horizon。驗證已通過：`cargo check --lib -q` 與 `cargo test --lib adaptive_peak_tick --jobs 1`。
- [ ] P0-2: evaluation/ 死碼分叉
- [ ] P0-3: RwLock unwrap
- [ ] P0-4: broker_confirms_bias short
- [ ] P0-5: governance_reason_code
- [ ] P0-6: market_why_not_single_name
- [ ] P0-7: query.rs impl block
- [ ] P0-8: 重複 causal_scope_key

## P1
- [ ] P1-1: confidence 二次 boost
- [ ] P1-2: FamilyAlphaGate 缺失
- [ ] P1-3: consecutive_misses 不遞增
- [ ] P1-4: setup_direction 用 title.starts_with("Short ")
- [ ] P1-5: TacticalSetup.action String / priority 體系衝突
- [ ] P1-6: family_key 透過 risk_notes 隱式傳遞
- [ ] P1-7: extract_sectors_from_fingerprints 盲目加 Branch entities
- [ ] P1-8: candidate mechanism promote / decay 不互斥
- [ ] P1-9: BrainGraph knowledge.read().unwrap()
- [ ] P1-10: case_realized_outcome schema 缺少 primary_lens

## Notes
2026-04-03 | 初始化工作流文件 | `CODEX.md`, `BACKLOG.md`, `PROGRESS.md` | 建立持續開發循環與 review fix 追蹤格式。
2026-04-03 | P0-1 完成 | `src/temporal/lineage/outcomes.rs`, `src/runtime_loop.rs`, `BACKLOG.md`, `PROGRESS.md` | adaptive outcome 現在以 peak tick 為 resolved horizon 重新計算 outcome；runtime loop 測試移除 Tokio `test-util` 依賴。驗證通過：`cargo check --lib -q`、`cargo test --lib adaptive_peak_tick --jobs 1`。
