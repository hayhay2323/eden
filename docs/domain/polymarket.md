# Polymarket Integration

Eden reads Polymarket event priors from:

1. `POLYMARKET_MARKETS_FILE`, if set
2. `config/polymarket_markets.json`, if it exists
3. `POLYMARKET_MARKETS`, as a JSON string fallback

The config format is a JSON array. Example:

```json
[
  {
    "slug": "will-the-fed-cut-rates-by-september-2026",
    "label": "Fed Sep Cut",
    "scope_kind": "market",
    "bias": "risk_on",
    "conviction_threshold": "0.65"
  },
  {
    "slug": "will-us-tighten-ai-chip-export-controls-in-2026",
    "label": "AI Export Controls",
    "scope_kind": "theme",
    "scope_value": "ai_semis",
    "bias": "risk_off",
    "conviction_threshold": "0.60",
    "target_scopes": [
      "sector:semiconductor",
      "theme:ai_semis",
      "symbol:981.HK",
      "symbol:1347.HK"
    ]
  },
  {
    "slug": "will-china-announce-major-stimulus-before-q4-2026",
    "label": "China Stimulus",
    "scope_kind": "region",
    "scope_value": "china",
    "bias": "risk_on",
    "conviction_threshold": "0.60"
  }
]
```

Fields:

- `slug`: required Polymarket market slug.
- `label`: optional display name. Defaults to the Polymarket question.
- `scope_kind`: one of `market`, `sector`, `theme`, `region`, `custom`.
- `scope_value`: optional scope payload for non-market scopes.
- `bias`: one of `risk_on`, `risk_off`, `neutral`.
- `conviction_threshold`: decimal probability threshold required before Eden promotes the prior into `world_state`.
- `target_scopes`: optional explicit impact registry. Format examples:
  - `sector:semiconductor`
  - `theme:ai_semis`
  - `symbol:981.HK`
  - `region:china`

Behavior:

- Eden fetches configured markets during the regular REST refresh loop.
- Material priors are added into `world_state` as external event entities.
- Explicit `target_scopes` are expanded into additional target entities in `world_state`.
- Strong market-scoped priors can change the market canopy regime label to `event-risk-on (...)` or `event-risk-off (...)`.
- Market-scoped priors also soft-gate execution by making opposite-side order suggestions require confirmation when the external prior is strong enough.

CLI:

```bash
cargo run -- polymarket
cargo run -- polymarket --json
```

This prints:

- loaded market configs
- fetched probabilities
- scope / bias mapping
- whether each configured prior currently clears its conviction threshold
