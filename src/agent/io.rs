use super::*;

pub async fn load_snapshot(
    market: CaseMarket,
) -> Result<AgentSnapshot, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_agent_snapshot_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_briefing(
    market: CaseMarket,
) -> Result<AgentBriefing, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_briefing_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_session(market: CaseMarket) -> Result<AgentSession, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_session_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_watchlist(
    market: CaseMarket,
) -> Result<AgentWatchlist, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_watchlist_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_recommendations(
    market: CaseMarket,
) -> Result<AgentRecommendations, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_recommendations_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_scoreboard(
    market: CaseMarket,
) -> Result<AgentAlertScoreboard, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_scoreboard_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_eod_review(
    market: CaseMarket,
) -> Result<AgentEodReview, Box<dyn std::error::Error>> {
    let (env_var, default_path) = load_eod_review_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}
