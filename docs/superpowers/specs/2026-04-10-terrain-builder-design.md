# Terrain Builder — Terminal CLI → Ontology 動態地形系統

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Use Longbridge Terminal CLI data to dynamically build and maintain Eden's ontology "terrain" — the graph edges, peer relationships, institutional holdings, and event context that the pressure field flows through.

**Architecture:** A `TerrainBuilder` module invokes Terminal CLI as a shell subprocess with `--format json`, parses responses into Rust structs, caches in SurrealDB with TTL-based expiry, and injects enriched relationships into ObjectStore and Graph structures. Both HK and US share the same TerrainBuilder; market determines which CLI commands are applicable.

**Tech Stack:** Rust, `std::process::Command` (shell subprocess), `serde_json`, existing SurrealDB persistence (`EdenStore`)

---

## Core Concept

Eden's ontology is currently static — sector catalogs are hardcoded arrays, institution relationships exist only for HK (via broker queue), and peer groups are sector-based only. Terminal CLI provides data that turns this static map into a living terrain:

- **Shareholder/13F/Fund-holder data** → real institutional edges (critical for US which has zero institution data)
- **Valuation peers** → industry-matched peer groups (replaces hand-maintained sector catalogs for peer comparison)
- **Finance calendar** → temporal context for pressure field anomalies
- **Ratings/News** → ground truth labels for edge learning validation

**Key insight:** This data updates quarterly/daily/weekly — NOT tick-by-tick. It's terrain, not signal. Pull at startup, cache with TTL, refresh when stale.

---

## Data Tiers

### Tier 1 — Terrain Skeleton (startup, quarterly refresh)

These build the graph edges that pressure propagates along.

#### `shareholder <SYMBOL>` (HK + US)
- **What:** Top institutional holders with % shares, share changes, report dates
- **Update rate:** Quarterly (report_date field)
- **TTL:** 30 days
- **Ontology impact:** Creates `Institution ↔ Symbol` edges with ownership weight
- **Critical for US:** Only source of institutional relationships (HK has broker queue as alternative)
- **Extra value:** `stocks` field on some holders links to their own listed entity (e.g., BlackRock → BLK.US)

#### `fund-holder <SYMBOL>` (HK + US)
- **What:** ETFs/funds holding a symbol, with position_ratio and report_date
- **Update rate:** Daily for ETFs, quarterly for mutual funds
- **TTL:** 30 days
- **Ontology impact:** Creates `Fund ↔ Symbol` edges; enables **co-holding peer discovery** (symbols in same fund = implicit peers)
- **Key use:** Absence reasoning — "ARKK holds both TSLA and COIN; if TSLA has a vortex, check COIN"

#### `investors [CIK]` (US only)
- **What:** Full 13F portfolio for a specific institution (by SEC CIK)
- **Update rate:** Quarterly (SEC filing deadline: 45 days after quarter end)
- **TTL:** 90 days
- **Ontology impact:** Complete institution → holdings map for top active managers
- **Strategy:** Pull top 20 active managers (from `investors` ranking), fetch their holdings, build cross-holding graph
- **Rate limit concern:** 20 CIKs × 1 call each = manageable at startup

#### `valuation <SYMBOL>` (HK + US)
- **What:** P/E, P/B, P/S + **peer comparison list** with counter_ids
- **Update rate:** Daily (price-dependent metrics)
- **TTL:** 7 days
- **Ontology impact:** `peers` field provides industry-matched peer group — **replaces sector catalog for Absence reasoning**
- **Key insight:** TSLA's valuation peers include Ferrari, BYD, Li Auto — cross-sector, cross-market peers that sector catalogs miss

### Tier 2 — Terrain Annotations (startup, daily/weekly refresh)

These add context and labels to the terrain.

#### `institution-rating <SYMBOL>` (HK + US)
- **What:** Analyst consensus (buy/hold/sell distribution), target price range, industry ranking
- **Update rate:** When analysts publish (irregular, ~weekly for active symbols)
- **TTL:** 1 day
- **Ontology impact:** Annotates symbol nodes with market expectations; provides edge learning labels
- **Key use:** If vortex appears → rating changes within 48h → confirms edge transmitted real info → credit edge weight

#### `finance-calendar <EVENT_TYPE>` (HK + US)
- **What:** Upcoming earnings, dividends, IPOs, macro events with dates
- **Update rate:** Calendar is known in advance
- **TTL:** 1 day (re-pull daily to catch new announcements)
- **Ontology impact:** Temporal annotation on symbol nodes — "AAPL has earnings in 3 days"
- **Key use:** Attribution context — same vortex means different things if earnings is imminent vs not
- **Event types to pull:** `financial` (earnings), `dividend`, `macrodata` (star >= 2 importance)

#### `insider-trades <SYMBOL>` (US only)
- **What:** SEC Form 4 filings — who bought/sold, how much, at what price
- **Update rate:** Filed within 2 business days of transaction
- **TTL:** 7 days
- **Ontology impact:** Directional signal on institutional channel; validates institutional pressure readings
- **Key use:** If pressure field shows institutional channel tension + insider just bought → stronger conviction

### Tier 3 — Terrain Validation (on-demand during trading)

#### `news <SYMBOL>` (HK + US)
- **What:** Recent news articles with titles, timestamps
- **Update rate:** Real-time
- **TTL:** No cache (always fresh)
- **Ontology impact:** Ground truth label for attribution reasoning
- **Key use:** After vortex detected → pull news → if news explains the vortex → validates attribution was correct
- **NOT for decision-making** — news is delayed info. Used only to label/validate pressure field detections.

---

## Module Design

### Module placement: `src/ontology/terrain.rs`

Following CLAUDE.md's principle of modifying existing code over creating new modules, terrain building lives within the ontology module since it enriches ontology data:

```
src/ontology/
  terrain.rs       -- TerrainBuilder struct, CLI wrapper, types, cache logic, enricher
                   -- Single file, ~500-700 lines (comparable to existing ontology files)
```

If the file grows beyond ~800 lines, split into `src/ontology/terrain/` with submodules. But start as one file.

### TerrainBuilder API

```rust
pub struct TerrainBuilder {
    store: EdenStore,          // existing SurrealDB
    cli_path: String,          // path to `longbridge` binary
    symbols_hk: Vec<Symbol>,   // from ObjectStore
    symbols_us: Vec<Symbol>,   // from ObjectStore
}

impl TerrainBuilder {
    /// Called at startup. Pulls all Tier 1 + Tier 2 data, respecting TTL cache.
    /// Batches requests at ~4/sec to avoid rate limits.
    pub async fn build_terrain(&self) -> TerrainSnapshot { ... }
    
    /// Called on-demand when vortex detected. Pulls Tier 3 (news) for specific symbol.
    pub async fn enrich_for_vortex(&self, symbol: &Symbol) -> VortexContext { ... }
    
    /// Called periodically (every 30 min) to refresh Tier 2 data.
    pub async fn refresh_annotations(&self) -> TerrainDelta { ... }
}
```

### TerrainSnapshot (output)

```rust
pub struct TerrainSnapshot {
    /// Institution → Vec<(Symbol, ownership_pct)> — from shareholder + 13F
    pub institutional_holdings: HashMap<String, Vec<(Symbol, f64)>>,
    
    /// Symbol → Vec<Symbol> — from valuation peers + fund co-holdings
    pub peer_groups: HashMap<Symbol, Vec<Symbol>>,
    
    /// Symbol → Vec<CalendarEvent> — from finance-calendar
    pub upcoming_events: HashMap<Symbol, Vec<CalendarEvent>>,
    
    /// Symbol → RatingSnapshot — from institution-rating
    pub ratings: HashMap<Symbol, RatingSnapshot>,
    
    /// Symbol → Vec<InsiderTrade> — from insider-trades
    pub insider_activity: HashMap<Symbol, Vec<InsiderTrade>>,
}
```

### CLI Wrapper

```rust
/// Execute a Terminal CLI command and parse JSON output
async fn cli_call(command: &str, args: &[&str]) -> Result<serde_json::Value> {
    let output = Command::new("longbridge")
        .arg(command)
        .args(args)
        .arg("--format").arg("json")
        .output()?;
    serde_json::from_slice(&output.stdout)
}
```

### Batching Strategy

```
Startup sequence (total ~3-5 minutes for 1134 symbols):
1. Check SurrealDB cache for each data type
2. Collect symbols needing refresh (TTL expired)
3. Batch at ~4 requests/sec:
   - shareholder: ~1134 calls → ~5 min (but only expired ones)
   - fund-holder: ~1134 calls → ~5 min (but only expired ones)
   - valuation: ~1134 calls → ~5 min (but only expired ones)
   - investors (13F): ~20 calls (top managers only)
   - ratings: ~1134 calls → ~5 min
   - calendar: 3 calls (financial + dividend + macrodata)
   - insider-trades: ~640 US calls → ~3 min
4. First run: ~15-20 min total (all symbols cold)
5. Subsequent runs: seconds (most cached)
```

---

## Integration with Pressure Field Reasoning

### Enhanced Absence Detection

Current code in `src/pipeline/pressure/reasoning.rs`:
```rust
// BEFORE: only sector peers
let peers = sector_members.get(sector_id);

// AFTER: multi-source peer resolution
let peers = terrain.resolve_peers(symbol);
// Returns union of:
//   1. valuation_peers (industry-matched, highest quality)
//   2. fund_co_holdings (shared ETF/fund exposure)
//   3. shareholder_overlap (same large holders)
//   4. sector_members (fallback)
// Deduplicated, ranked by relationship strength
```

### Enhanced Attribution Context

```rust
// BEFORE: just channel analysis
let driver = classify_driver(&channels);

// AFTER: add calendar context
let events = terrain.upcoming_events(symbol);
if events.iter().any(|e| e.days_until() <= 3) {
    attribution.context = Some(EventAnticipation(events[0].clone()));
}
```

### Enhanced Edge Learning Labels

```rust
// BEFORE: only hour-layer delta (self-supervised)
learn_from_hour_deltas(field, graph);

// AFTER: also use rating changes as labels
if let Some(rating_change) = terrain.detect_rating_change(symbol, since_vortex_time) {
    edge_learning.add_confirmed_label(vortex_edges, rating_change);
}
```

---

## HK vs US Command Matrix

| Command | HK | US | Notes |
|---------|----|----|-------|
| `shareholder` | Yes | Yes | US: only institutional source; HK: supplements broker queue |
| `fund-holder` | Yes | Yes | Both markets have ETF/fund data |
| `investors` (13F) | No | Yes | SEC-only filing |
| `insider-trades` | No | Yes | Form 4, SEC-only |
| `valuation` peers | Yes | Yes | Both markets get peer comparison |
| `institution-rating` | Yes | Yes | Both markets have analyst coverage |
| `finance-calendar` | Yes | Yes | Filter by market=HK or market=US |
| `news` | Yes | Yes | Per-symbol, both markets |

---

## SurrealDB Cache Schema

Using existing `EdenStore` with new tables:

```sql
DEFINE TABLE terrain_shareholder SCHEMAFULL;
DEFINE FIELD symbol ON terrain_shareholder TYPE string;
DEFINE FIELD market ON terrain_shareholder TYPE string;
DEFINE FIELD holders ON terrain_shareholder TYPE array;
DEFINE FIELD fetched_at ON terrain_shareholder TYPE datetime;

DEFINE TABLE terrain_fund_holder SCHEMAFULL;
DEFINE FIELD symbol ON terrain_fund_holder TYPE string;
DEFINE FIELD funds ON terrain_fund_holder TYPE array;
DEFINE FIELD fetched_at ON terrain_fund_holder TYPE datetime;

DEFINE TABLE terrain_13f SCHEMAFULL;
DEFINE FIELD cik ON terrain_13f TYPE string;
DEFINE FIELD name ON terrain_13f TYPE string;
DEFINE FIELD holdings ON terrain_13f TYPE array;
DEFINE FIELD period ON terrain_13f TYPE string;
DEFINE FIELD fetched_at ON terrain_13f TYPE datetime;

DEFINE TABLE terrain_valuation_peers SCHEMAFULL;
DEFINE FIELD symbol ON terrain_valuation_peers TYPE string;
DEFINE FIELD peers ON terrain_valuation_peers TYPE array;
DEFINE FIELD fetched_at ON terrain_valuation_peers TYPE datetime;

DEFINE TABLE terrain_ratings SCHEMAFULL;
DEFINE FIELD symbol ON terrain_ratings TYPE string;
DEFINE FIELD consensus ON terrain_ratings TYPE object;
DEFINE FIELD target_price ON terrain_ratings TYPE object;
DEFINE FIELD fetched_at ON terrain_ratings TYPE datetime;

DEFINE TABLE terrain_calendar SCHEMAFULL;
DEFINE FIELD event_type ON terrain_calendar TYPE string;
DEFINE FIELD market ON terrain_calendar TYPE string;
DEFINE FIELD events ON terrain_calendar TYPE array;
DEFINE FIELD fetched_at ON terrain_calendar TYPE datetime;

DEFINE TABLE terrain_insider SCHEMAFULL;
DEFINE FIELD symbol ON terrain_insider TYPE string;
DEFINE FIELD trades ON terrain_insider TYPE array;
DEFINE FIELD fetched_at ON terrain_insider TYPE datetime;
```

TTL check: `WHERE fetched_at > time::now() - <ttl_duration>`

---

## Rollout Order

1. **Phase 1: CLI wrapper + types + cache** — Can call Terminal CLI, parse JSON, store in SurrealDB
2. **Phase 2: TerrainBuilder startup flow** — Pull Tier 1 + 2 at startup, build TerrainSnapshot
3. **Phase 3: Peer resolution** — Multi-source peer groups fed into Absence reasoning
4. **Phase 4: Calendar context** — Event annotations fed into Attribution reasoning
5. **Phase 5: Edge learning labels** — Rating changes validate vortex predictions
6. **Phase 6: On-demand news** — Tier 3 news pull for vortex validation

Each phase is independently testable and deployable.

---

## Rate Limit Management

- Terminal CLI uses Longbridge web API (separate from SDK WebSocket/REST quotas)
- Observed: ~4 req/sec sustainable without throttling
- Strategy: `tokio::time::sleep(Duration::from_millis(250))` between sequential calls
- First-run cold cache: ~15-20 min for all 1134 symbols across all commands
- Subsequent runs with warm cache: seconds (only pull expired entries)
- Calendar and 13F ranking: few calls regardless of symbol count

---

## Success Criteria

1. **US Absence reasoning uses real peers** — not just sector catalog, but valuation peers + fund co-holdings
2. **HK institutional knowledge supplements broker queue** — shareholder data cross-validates broker queue inference
3. **Calendar events contextualize vortices** — "AAPL has earnings in 2 days" appears in Attribution output
4. **Edge learning has multi-source labels** — hour-delta + rating changes + news confirmation
5. **Cache works** — second startup pulls only stale data, completes in seconds
6. **No regression** — existing pressure field + reasoning works identically if Terminal CLI is unavailable (graceful fallback)
