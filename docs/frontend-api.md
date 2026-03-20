# Eden Frontend API

This project now includes a dedicated HTTP API binary for frontend integration: `eden-api`.

## Environment

Required:

- `EDEN_API_MASTER_KEY`: master secret used to encrypt and validate API keys

Optional:

- `EDEN_API_BIND`: bind address for the HTTP server, default `0.0.0.0:8787`
- `EDEN_API_ALLOWED_ORIGINS`: comma-separated CORS allowlist, default `*`
- `EDEN_DB_PATH`: SurrealDB path, default `data/eden.db`
- `POLYMARKET_MARKETS_FILE` or `POLYMARKET_MARKETS`: Polymarket config source

## Start the API

If you need the persisted lineage/causal endpoints, build with persistence:

```bash
cargo run --bin eden-api --features persistence -- serve
```

Without persistence, the server still exposes:

- `GET /health`
- `GET /api/polymarket`

## Mint an encrypted frontend API key

```bash
cargo run --bin eden-api -- mint-key --label frontend --ttl-hours 720
```

Example response:

```json
{
  "api_key": "eden_pk_...",
  "label": "frontend",
  "scope": "frontend:readonly",
  "issued_at": "2026-03-20T10:00:00Z",
  "expires_at": "2026-04-19T10:00:00Z"
}
```

## Request format

Send the key in either header:

- `Authorization: Bearer <api_key>`
- `x-api-key: <api_key>`

## Endpoints

- `GET /health`
- `GET /api/polymarket`
- `GET /api/lineage`
- `GET /api/lineage/history`
- `GET /api/lineage/rows`
- `GET /api/causal/flips`
- `GET /api/causal/timeline/:leaf_scope_key`

## Frontend example

```ts
const response = await fetch("http://localhost:8787/api/lineage?limit=120&top=10", {
  headers: {
    Authorization: `Bearer ${apiKey}`,
  },
});

const data = await response.json();
```

## Security note

If the API key is shipped directly inside a browser bundle, users can extract it. For public production deployments, prefer a short TTL or a server-side proxy/BFF in front of this API.
