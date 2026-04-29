use super::types::CaseMarket;
use crate::live_snapshot::LiveSnapshot;

pub(super) type CaseError = Box<dyn std::error::Error + Send + Sync>;

pub async fn load_snapshot(market: CaseMarket) -> Result<LiveSnapshot, CaseError> {
    let (env_var, default_path) = market.snapshot_path();
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}
