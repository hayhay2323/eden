use crate::agent::{self, AgentDecision, AgentRecommendationJournalRecord};
use crate::cases::CaseMarket;

use super::foundation::ApiError;

pub(in crate::api) async fn load_recommendation_journal_records(
    market: CaseMarket,
) -> Result<Vec<AgentRecommendationJournalRecord>, ApiError> {
    let (env_var, default_path) = agent::load_recommendation_journal_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ApiError::internal(format!(
                "failed to load recommendation journal: {error}"
            )))
        }
    };

    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AgentRecommendationJournalRecord>(line).ok())
        .collect())
}

pub(in crate::api) fn journal_matches_recommendation(
    row: &AgentRecommendationJournalRecord,
    recommendation_id: &str,
) -> bool {
    row.market_recommendation
        .as_ref()
        .map(|item| item.recommendation_id.eq_ignore_ascii_case(recommendation_id))
        .unwrap_or(false)
        || row
            .decisions
            .iter()
            .any(|item| decision_matches_recommendation(item, recommendation_id))
        || row
            .items
            .iter()
            .any(|item| item.recommendation_id.eq_ignore_ascii_case(recommendation_id))
}

fn decision_matches_recommendation(
    decision: &AgentDecision,
    recommendation_id: &str,
) -> bool {
    match decision {
        AgentDecision::Market(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
        AgentDecision::Sector(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
        AgentDecision::Symbol(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
    }
}
