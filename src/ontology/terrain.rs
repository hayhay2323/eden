//! Terrain Builder — Terminal CLI → Ontology dynamic terrain
//!
//! Invokes Longbridge Terminal CLI as a shell subprocess to fetch structural
//! data (shareholders, fund holders, valuation peers, ratings, calendar,
//! insider trades, 13F holdings) and injects it into Eden's ontology.

use std::collections::HashMap;
use std::process::Command as StdCommand;

use serde::{Deserialize, Serialize};

use crate::ontology::objects::{Market, Symbol};

// ── CLI JSON response types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareholderEntry {
    #[serde(default)]
    pub shareholder_name: String,
    #[serde(default)]
    pub percent_of_shares: String,
    #[serde(default)]
    pub report_date: String,
    #[serde(default)]
    pub stocks: Vec<ShareholderStock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareholderStock {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub market: String,
    #[serde(default)]
    pub counter_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareholderResponse {
    #[serde(default)]
    pub shareholder_list: Vec<ShareholderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundHolderEntry {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub counter_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub position_ratio: String,
    #[serde(default)]
    pub report_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundHolderResponse {
    #[serde(default)]
    pub lists: Vec<FundHolderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationPeer {
    #[serde(default)]
    pub counter_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationPeerGroup {
    #[serde(default)]
    pub industry_median: String,
    #[serde(default)]
    pub list: Vec<ValuationPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuationResponse {
    #[serde(default)]
    pub peers: HashMap<String, ValuationPeerGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingSnapshot {
    #[serde(default)]
    pub recommend: String,
    #[serde(default)]
    pub target_price: String,
    #[serde(default)]
    pub buy_count: u32,
    #[serde(default)]
    pub hold_count: u32,
    #[serde(default)]
    pub sell_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsiderTrade {
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, rename = "type")]
    pub trade_type: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub shares: f64,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorRanking {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cik: String,
    #[serde(default)]
    pub aum_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsArticle {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub published_at: String,
}

// ── Terrain snapshot (output) ──

#[derive(Debug, Clone, Default)]
pub struct TerrainSnapshot {
    /// Institution name → Vec<(Symbol, ownership_pct)>
    pub institutional_holdings: HashMap<String, Vec<(Symbol, f64)>>,
    /// Symbol → peer symbols from valuation + fund co-holdings
    pub peer_groups: HashMap<Symbol, Vec<Symbol>>,
    /// Symbol → upcoming events
    pub upcoming_events: HashMap<Symbol, Vec<CalendarEvent>>,
    /// Symbol → analyst ratings
    pub ratings: HashMap<Symbol, RatingSnapshot>,
    /// Symbol → insider trades (US only)
    pub insider_activity: HashMap<Symbol, Vec<InsiderTrade>>,
    /// Fund/ETF code → Vec<Symbol> it holds
    pub fund_holdings: HashMap<String, Vec<Symbol>>,
}

#[derive(Debug, Clone, Default)]
pub struct VortexContext {
    pub news: Vec<NewsArticle>,
    pub rating: Option<RatingSnapshot>,
    pub upcoming_events: Vec<CalendarEvent>,
}

// ── CLI wrapper ──

fn cli_path() -> String {
    std::env::var("LONGBRIDGE_CLI_PATH").unwrap_or_else(|_| "longbridge".to_string())
}

fn cli_call_sync(command: &str, args: &[&str]) -> Option<serde_json::Value> {
    let mut cmd = StdCommand::new(cli_path());
    cmd.arg(command);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg("--format").arg("json");
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

async fn cli_call(command: &str, args: Vec<String>) -> Option<serde_json::Value> {
    let command = command.to_string();
    tokio::task::spawn_blocking(move || {
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        cli_call_sync(&command, &arg_refs)
    })
    .await
    .ok()?
}

async fn throttle() {
    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
}

// ── Data fetchers ──

async fn fetch_shareholders(symbol: &Symbol) -> Option<Vec<ShareholderEntry>> {
    let val = cli_call("shareholder", vec![symbol.0.clone()]).await?;
    let resp: ShareholderResponse = serde_json::from_value(val).ok()?;
    Some(resp.shareholder_list)
}

async fn fetch_fund_holders(symbol: &Symbol) -> Option<Vec<FundHolderEntry>> {
    let val = cli_call(
        "fund-holder",
        vec![symbol.0.clone(), "--count".into(), "20".into()],
    )
    .await?;
    let resp: FundHolderResponse = serde_json::from_value(val).ok()?;
    Some(resp.lists)
}

async fn fetch_valuation_peers(symbol: &Symbol) -> Option<Vec<ValuationPeer>> {
    let val = cli_call("valuation", vec![symbol.0.clone()]).await?;
    let resp: ValuationResponse = serde_json::from_value(val).ok()?;
    for (_metric, group) in &resp.peers {
        if !group.list.is_empty() {
            return Some(group.list.clone());
        }
    }
    None
}

async fn fetch_calendar(market: Market) -> Vec<CalendarEvent> {
    let market_str = match market {
        Market::Hk => "HK",
        Market::Us => "US",
    };
    let mut events = Vec::new();
    for event_type in &["financial", "dividend"] {
        if let Some(v) = cli_call(
            "finance-calendar",
            vec![
                event_type.to_string(),
                "--market".into(),
                market_str.into(),
                "--count".into(),
                "100".into(),
            ],
        )
        .await
        {
            if let Ok(parsed) = serde_json::from_value::<Vec<CalendarEvent>>(v) {
                events.extend(parsed);
            }
        }
        throttle().await;
    }
    events
}

async fn fetch_investor_rankings() -> Option<Vec<InvestorRanking>> {
    let val = cli_call("investors", vec![]).await?;
    serde_json::from_value(val).ok()
}

async fn fetch_news(symbol: &Symbol) -> Option<Vec<NewsArticle>> {
    let val = cli_call(
        "news",
        vec![symbol.0.clone(), "--count".into(), "10".into()],
    )
    .await?;
    serde_json::from_value(val).ok()
}

// ── TerrainBuilder ──

pub struct TerrainBuilder {
    symbols_hk: Vec<Symbol>,
    symbols_us: Vec<Symbol>,
}

impl TerrainBuilder {
    pub fn new(symbols_hk: Vec<Symbol>, symbols_us: Vec<Symbol>) -> Self {
        Self {
            symbols_hk,
            symbols_us,
        }
    }

    /// Build full terrain at startup. Pulls Tier 1 + Tier 2 data.
    /// For per-symbol data, pulls top 50 by market cap to keep startup fast.
    pub async fn build_terrain(&self) -> TerrainSnapshot {
        let mut snapshot = TerrainSnapshot::default();

        // Limit per-symbol pulls to top 50 to keep startup < 2 minutes
        let us_top: Vec<&Symbol> = self.symbols_us.iter().take(50).collect();
        let hk_top: Vec<&Symbol> = self.symbols_hk.iter().take(50).collect();
        let all_top: Vec<&Symbol> = hk_top.iter().chain(us_top.iter()).copied().collect();

        // Tier 1: Shareholders (top 50 symbols)
        eprintln!(
            "[terrain] fetching shareholders for {} symbols...",
            all_top.len()
        );
        for sym in &all_top {
            if let Some(holders) = fetch_shareholders(sym).await {
                for h in &holders {
                    let pct: f64 = h.percent_of_shares.parse().unwrap_or(0.0);
                    if pct > 0.5 {
                        snapshot
                            .institutional_holdings
                            .entry(h.shareholder_name.clone())
                            .or_default()
                            .push(((*sym).clone(), pct));
                    }
                }
            }
            throttle().await;
        }
        eprintln!(
            "[terrain] shareholders: {} institutions found",
            snapshot.institutional_holdings.len()
        );

        // Tier 1: Fund holders (top 50 symbols)
        eprintln!("[terrain] fetching fund holders...");
        for sym in &all_top {
            if let Some(funds) = fetch_fund_holders(sym).await {
                for f in &funds {
                    snapshot
                        .fund_holdings
                        .entry(f.code.clone())
                        .or_default()
                        .push((*sym).clone());
                }
            }
            throttle().await;
        }
        eprintln!(
            "[terrain] fund holders: {} funds found",
            snapshot.fund_holdings.len()
        );

        // Build peer groups from fund co-holdings
        for (_fund, members) in &snapshot.fund_holdings {
            if members.len() >= 2 && members.len() <= 30 {
                for member in members {
                    let peers: Vec<Symbol> =
                        members.iter().filter(|m| *m != member).cloned().collect();
                    snapshot
                        .peer_groups
                        .entry(member.clone())
                        .or_default()
                        .extend(peers);
                }
            }
        }

        // Tier 1: Valuation peers (top 50 symbols)
        eprintln!("[terrain] fetching valuation peers...");
        for sym in &all_top {
            if let Some(peers) = fetch_valuation_peers(sym).await {
                let peer_symbols: Vec<Symbol> = peers
                    .iter()
                    .filter_map(|p| {
                        let parts: Vec<&str> = p.counter_id.split('/').collect();
                        if parts.len() >= 3 {
                            Some(Symbol(format!("{}.{}", parts[2], parts[1])))
                        } else {
                            None
                        }
                    })
                    .filter(|p| p != *sym)
                    .collect();
                snapshot
                    .peer_groups
                    .entry((*sym).clone())
                    .or_default()
                    .extend(peer_symbols);
            }
            throttle().await;
        }

        // Deduplicate peer groups
        for peers in snapshot.peer_groups.values_mut() {
            peers.sort_by(|a, b| a.0.cmp(&b.0));
            peers.dedup();
        }
        eprintln!(
            "[terrain] peer groups: {} symbols with peers",
            snapshot.peer_groups.len()
        );

        // Tier 1: 13F top investors
        eprintln!("[terrain] fetching 13F investor rankings...");
        if let Some(rankings) = fetch_investor_rankings().await {
            eprintln!("[terrain] found {} top investors", rankings.len());
            // Just store the ranking info, skip per-CIK holdings to save time
            for investor in rankings.iter().take(10) {
                snapshot
                    .institutional_holdings
                    .entry(investor.name.clone())
                    .or_default(); // placeholder, holdings fetched on demand
            }
        }

        // Tier 2: Calendar (fast — just 2 API calls per market)
        eprintln!("[terrain] fetching calendar events...");
        let us_events = fetch_calendar(Market::Us).await;
        for e in us_events {
            let sym = Symbol(e.symbol.clone());
            snapshot.upcoming_events.entry(sym).or_default().push(e);
        }
        let hk_events = fetch_calendar(Market::Hk).await;
        for e in hk_events {
            let sym = Symbol(e.symbol.clone());
            snapshot.upcoming_events.entry(sym).or_default().push(e);
        }
        eprintln!(
            "[terrain] calendar: {} symbols with events",
            snapshot.upcoming_events.len()
        );

        snapshot
    }

    /// On-demand enrichment when a vortex is detected.
    pub async fn enrich_for_vortex(&self, symbol: &Symbol) -> VortexContext {
        let mut ctx = VortexContext::default();
        if let Some(news) = fetch_news(symbol).await {
            ctx.news = news;
        }
        ctx
    }
}

// ── Peer resolution ──

impl TerrainSnapshot {
    /// Resolve peers for a symbol from terrain data.
    pub fn resolve_peers(&self, symbol: &Symbol) -> Vec<Symbol> {
        self.peer_groups.get(symbol).cloned().unwrap_or_default()
    }

    /// Check if a symbol has upcoming events.
    pub fn has_upcoming_event(&self, symbol: &Symbol) -> bool {
        self.upcoming_events
            .get(symbol)
            .map(|e| !e.is_empty())
            .unwrap_or(false)
    }

    /// Get institutional holders for a symbol.
    pub fn holders_of(&self, symbol: &Symbol) -> Vec<(&str, f64)> {
        let mut result = Vec::new();
        for (name, holdings) in &self.institutional_holdings {
            for (held_sym, pct) in holdings {
                if held_sym == symbol {
                    result.push((name.as_str(), *pct));
                }
            }
        }
        result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shareholder_response() {
        let json = r#"{"shareholder_list": [{"shareholder_name": "Vanguard", "percent_of_shares": "6.90", "report_date": "2025-12-31", "stocks": []}]}"#;
        let resp: ShareholderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.shareholder_list.len(), 1);
        assert_eq!(resp.shareholder_list[0].shareholder_name, "Vanguard");
    }

    #[test]
    fn parse_fund_holder_response() {
        let json = r#"{"lists": [{"code": "XLY", "counter_id": "ETF/US/XLY", "name": "Consumer ETF", "position_ratio": "17.87", "report_date": "2026.04.07"}]}"#;
        let resp: FundHolderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.lists.len(), 1);
        assert_eq!(resp.lists[0].code, "XLY");
    }

    #[test]
    fn counter_id_to_symbol() {
        let counter_id = "ST/US/TSLA";
        let parts: Vec<&str> = counter_id.split('/').collect();
        let sym = format!("{}.{}", parts[2], parts[1]);
        assert_eq!(sym, "TSLA.US");
    }

    #[test]
    fn terrain_snapshot_resolve_peers() {
        let mut snapshot = TerrainSnapshot::default();
        let tsla = Symbol("TSLA.US".into());
        let aapl = Symbol("AAPL.US".into());
        snapshot
            .peer_groups
            .insert(tsla.clone(), vec![aapl.clone()]);
        assert_eq!(snapshot.resolve_peers(&tsla), vec![aapl]);
    }
}
