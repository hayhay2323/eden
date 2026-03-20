use std::env;
use std::fs;
use std::path::Path;

use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::ontology::ReasoningScope;

const POLYMARKET_GAMMA_URL: &str = "https://gamma-api.polymarket.com/markets/slug";
const DEFAULT_POLYMARKET_CONFIG_PATH: &str = "config/polymarket_markets.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolymarketBias {
    RiskOn,
    RiskOff,
    #[default]
    Neutral,
}

impl PolymarketBias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskOn => "risk_on",
            Self::RiskOff => "risk_off",
            Self::Neutral => "neutral",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolymarketScopeKind {
    Market,
    Sector,
    Theme,
    Region,
    #[default]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketMarketConfig {
    pub slug: String,
    pub label: Option<String>,
    #[serde(default)]
    pub scope_kind: PolymarketScopeKind,
    pub scope_value: Option<String>,
    #[serde(default)]
    pub bias: PolymarketBias,
    #[serde(default = "default_conviction_threshold")]
    pub conviction_threshold: Decimal,
    #[serde(default)]
    pub target_scopes: Vec<String>,
}

impl PolymarketMarketConfig {
    pub fn scope(&self) -> ReasoningScope {
        let value = self
            .scope_value
            .clone()
            .unwrap_or_else(|| self.slug.replace('-', "_"));
        match self.scope_kind {
            PolymarketScopeKind::Market => ReasoningScope::Market,
            PolymarketScopeKind::Sector => ReasoningScope::Sector(value),
            PolymarketScopeKind::Theme => ReasoningScope::Theme(value),
            PolymarketScopeKind::Region => ReasoningScope::Region(value),
            PolymarketScopeKind::Custom => ReasoningScope::Custom(value),
        }
    }
}

fn default_conviction_threshold() -> Decimal {
    Decimal::new(6, 1)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketSnapshot {
    pub fetched_at: OffsetDateTime,
    pub priors: Vec<PolymarketPrior>,
}

impl Default for PolymarketSnapshot {
    fn default() -> Self {
        Self {
            fetched_at: OffsetDateTime::UNIX_EPOCH,
            priors: vec![],
        }
    }
}

impl PolymarketSnapshot {
    pub fn is_empty(&self) -> bool {
        self.priors.is_empty()
    }

    pub fn strongest_by_bias(&self, bias: PolymarketBias) -> Option<&PolymarketPrior> {
        self.priors
            .iter()
            .filter(|prior| prior.bias == bias)
            .max_by(|a, b| a.probability.cmp(&b.probability))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolymarketPrior {
    pub slug: String,
    pub label: String,
    pub question: String,
    pub scope: ReasoningScope,
    #[serde(default)]
    pub target_scopes: Vec<String>,
    pub bias: PolymarketBias,
    pub selected_outcome: String,
    pub probability: Decimal,
    pub conviction_threshold: Decimal,
    pub active: bool,
    pub closed: bool,
    pub category: Option<String>,
    pub volume: Option<Decimal>,
    pub liquidity: Option<Decimal>,
    pub end_date: Option<String>,
}

impl PolymarketPrior {
    pub fn is_material(&self) -> bool {
        self.probability >= self.conviction_threshold
    }

    pub fn parsed_target_scopes(&self) -> Vec<ReasoningScope> {
        self.target_scopes
            .iter()
            .filter_map(|scope| parse_target_scope(scope))
            .collect()
    }

    pub fn driver_text(&self) -> String {
        format!(
            "polymarket {}={} on {}",
            self.selected_outcome,
            self.probability.round_dp(3),
            self.label
        )
    }
}

#[derive(Debug, Deserialize)]
struct GammaMarketResponse {
    slug: String,
    question: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default, rename = "endDate")]
    end_date: Option<String>,
    #[serde(default)]
    liquidity: Option<Value>,
    #[serde(default)]
    volume: Option<Value>,
    #[serde(default)]
    active: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    outcomes: Option<Value>,
    #[serde(default, rename = "outcomePrices")]
    outcome_prices: Option<Value>,
}

pub fn load_polymarket_configs() -> Result<Vec<PolymarketMarketConfig>, String> {
    if let Ok(path) = env::var("POLYMARKET_MARKETS_FILE") {
        if !path.trim().is_empty() {
            return load_polymarket_configs_from_path(&path);
        }
    }

    if Path::new(DEFAULT_POLYMARKET_CONFIG_PATH).exists() {
        return load_polymarket_configs_from_path(DEFAULT_POLYMARKET_CONFIG_PATH);
    }

    let raw = match env::var("POLYMARKET_MARKETS") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) | Err(env::VarError::NotPresent) => return Ok(vec![]),
        Err(env::VarError::NotUnicode(_)) => {
            return Err("POLYMARKET_MARKETS must be valid UTF-8".into())
        }
    };

    parse_polymarket_configs(&raw, "POLYMARKET_MARKETS")
}

fn load_polymarket_configs_from_path(path: &str) -> Result<Vec<PolymarketMarketConfig>, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read Polymarket config file {}: {}", path, error))?;
    parse_polymarket_configs(&raw, path)
}

fn parse_polymarket_configs(
    raw: &str,
    source: &str,
) -> Result<Vec<PolymarketMarketConfig>, String> {
    serde_json::from_str(raw)
        .map_err(|error| format!("failed to parse Polymarket config from {}: {}", source, error))
}

pub async fn fetch_polymarket_snapshot(
    configs: &[PolymarketMarketConfig],
) -> Result<PolymarketSnapshot, String> {
    let fetched_at = OffsetDateTime::now_utc();
    if configs.is_empty() {
        return Ok(PolymarketSnapshot {
            fetched_at,
            priors: vec![],
        });
    }

    let client = Client::builder()
        .user_agent("eden/0.1 polymarket-readonly")
        .build()
        .map_err(|error| format!("failed to build Polymarket client: {}", error))?;

    let futures = configs.iter().cloned().map(|config| {
        let client = client.clone();
        async move { fetch_market_prior(&client, config).await }
    });

    let results = futures::future::join_all(futures).await;
    let priors = results
        .into_iter()
        .filter_map(|result| match result {
            Ok(prior) => Some(prior),
            Err(error) => {
                eprintln!("Warning: Polymarket fetch failed: {}", error);
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(PolymarketSnapshot { fetched_at, priors })
}

async fn fetch_market_prior(
    client: &Client,
    config: PolymarketMarketConfig,
) -> Result<PolymarketPrior, String> {
    let response = client
        .get(format!("{}/{}", POLYMARKET_GAMMA_URL, config.slug))
        .send()
        .await
        .map_err(|error| format!("GET {} failed: {}", config.slug, error))?
        .error_for_status()
        .map_err(|error| format!("GET {} returned error: {}", config.slug, error))?;

    let market: GammaMarketResponse = response
        .json()
        .await
        .map_err(|error| format!("failed to decode market {}: {}", config.slug, error))?;

    let outcomes = parse_string_array(market.outcomes.as_ref())
        .ok_or_else(|| format!("market {} did not include outcomes", config.slug))?;
    let prices = parse_decimal_array(market.outcome_prices.as_ref())
        .ok_or_else(|| format!("market {} did not include outcomePrices", config.slug))?;
    let index = yes_outcome_index(&outcomes).unwrap_or(0);
    let probability = prices
        .get(index)
        .copied()
        .ok_or_else(|| format!("market {} missing selected outcome price", config.slug))?;

    Ok(PolymarketPrior {
        slug: market.slug.clone(),
        label: config.label.clone().unwrap_or_else(|| market.question.clone()),
        question: market.question,
        scope: config.scope(),
        target_scopes: config.target_scopes,
        bias: config.bias,
        selected_outcome: outcomes
            .get(index)
            .cloned()
            .unwrap_or_else(|| "Yes".into()),
        probability,
        conviction_threshold: config.conviction_threshold,
        active: market.active.unwrap_or(true),
        closed: market.closed.unwrap_or(false),
        category: market.category,
        volume: parse_decimal_value(market.volume.as_ref()),
        liquidity: parse_decimal_value(market.liquidity.as_ref()),
        end_date: market.end_date,
    })
}

fn parse_string_array(value: Option<&Value>) -> Option<Vec<String>> {
    match value? {
        Value::String(raw) => serde_json::from_str(raw).ok(),
        Value::Array(items) => Some(
            items.iter()
                .filter_map(|item| match item {
                    Value::String(value) => Some(value.clone()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

fn parse_decimal_array(value: Option<&Value>) -> Option<Vec<Decimal>> {
    match value? {
        Value::String(raw) => {
            if let Ok(items) = serde_json::from_str::<Vec<String>>(raw) {
                let decimals = items
                    .into_iter()
                    .filter_map(|item| item.parse::<Decimal>().ok())
                    .collect::<Vec<_>>();
                if decimals.is_empty() {
                    None
                } else {
                    Some(decimals)
                }
            } else if let Ok(items) = serde_json::from_str::<Vec<f64>>(raw) {
                let decimals = items
                    .into_iter()
                    .filter_map(Decimal::from_f64_retain)
                    .collect::<Vec<_>>();
                if decimals.is_empty() {
                    None
                } else {
                    Some(decimals)
                }
            } else {
                None
            }
        }
        Value::Array(items) => {
            let decimals = items
                .iter()
                .filter_map(|item| parse_decimal_value(Some(item)))
                .collect::<Vec<_>>();
            if decimals.is_empty() {
                None
            } else {
                Some(decimals)
            }
        }
        _ => None,
    }
}

fn parse_decimal_value(value: Option<&Value>) -> Option<Decimal> {
    match value? {
        Value::String(raw) => raw.parse::<Decimal>().ok(),
        Value::Number(number) => Decimal::from_f64_retain(number.as_f64()?),
        _ => None,
    }
}

fn yes_outcome_index(outcomes: &[String]) -> Option<usize> {
    outcomes
        .iter()
        .position(|outcome| outcome.eq_ignore_ascii_case("yes"))
}

pub fn parse_target_scope(scope: &str) -> Option<ReasoningScope> {
    let (prefix, value) = scope.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    match prefix.trim().to_ascii_lowercase().as_str() {
        "market" => Some(ReasoningScope::Market),
        "sector" => Some(ReasoningScope::Sector(value.into())),
        "theme" => Some(ReasoningScope::Theme(value.into())),
        "region" => Some(ReasoningScope::Region(value.into())),
        "custom" => Some(ReasoningScope::Custom(value.into())),
        "symbol" => Some(ReasoningScope::Symbol(crate::ontology::Symbol(value.into()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;

    #[test]
    fn config_maps_into_reasoning_scope() {
        let config = PolymarketMarketConfig {
            slug: "fed-cut".into(),
            label: Some("Fed cut".into()),
            scope_kind: PolymarketScopeKind::Theme,
            scope_value: Some("rates".into()),
            bias: PolymarketBias::RiskOn,
            conviction_threshold: dec!(0.65),
            target_scopes: vec![],
        };

        assert_eq!(config.scope(), ReasoningScope::Theme("rates".into()));
    }

    #[test]
    fn parses_string_arrays_from_gamma_payload() {
        let outcomes = parse_string_array(Some(&json!("[\"Yes\",\"No\"]"))).unwrap();
        let prices = parse_decimal_array(Some(&json!("[\"0.63\",\"0.37\"]"))).unwrap();

        assert_eq!(outcomes, vec!["Yes".to_string(), "No".to_string()]);
        assert_eq!(prices[0], dec!(0.63));
        assert_eq!(prices[1], dec!(0.37));
    }

    #[test]
    fn prior_materiality_uses_threshold() {
        let prior = PolymarketPrior {
            slug: "fed-cut".into(),
            label: "Fed cut".into(),
            question: "Will the Fed cut?".into(),
            scope: ReasoningScope::Market,
            bias: PolymarketBias::RiskOn,
            selected_outcome: "Yes".into(),
            probability: dec!(0.72),
            conviction_threshold: dec!(0.65),
            target_scopes: vec!["sector:semiconductor".into()],
            active: true,
            closed: false,
            category: Some("Politics".into()),
            volume: None,
            liquidity: None,
            end_date: None,
        };

        assert!(prior.is_material());
    }

    #[test]
    fn parses_target_scope_tags() {
        assert_eq!(
            parse_target_scope("sector:semiconductor"),
            Some(ReasoningScope::Sector("semiconductor".into()))
        );
        assert_eq!(
            parse_target_scope("symbol:981.HK"),
            Some(ReasoningScope::Symbol(crate::ontology::Symbol("981.HK".into())))
        );
    }

    #[test]
    fn parses_configs_from_json_source() {
        let configs = parse_polymarket_configs(
            r#"[{"slug":"fed-cut","bias":"risk_on","target_scopes":["market:global"]}]"#,
            "test",
        )
        .expect("config parses");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].slug, "fed-cut");
        assert_eq!(configs[0].bias, PolymarketBias::RiskOn);
        assert_eq!(configs[0].target_scopes, vec!["market:global"]);
    }
}
