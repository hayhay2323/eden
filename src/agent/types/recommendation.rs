use super::*;
use crate::action::workflow::{
    ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode, ActionStage,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentActionExpectancies {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_expectancy: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_expectancy: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_expectancy: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentDecisionAttribution {
    #[serde(default)]
    pub historical_expectancies: AgentActionExpectancies,
    #[serde(default)]
    pub live_expectancy_shift: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisive_factors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLensComponent {
    pub lens_name: String,
    pub confidence: Decimal,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWatchlist {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<AgentWatchlistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWatchlistEntry {
    pub rank: usize,
    #[serde(default)]
    pub scope_kind: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_label: Option<String>,
    pub bias: String,
    pub severity: String,
    pub score: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub why: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why_components: Vec<AgentLensComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_lenses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_next: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub do_not: Vec<String>,
    pub recommendation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_transition: Option<String>,
    pub best_action: String,
    #[serde(flatten)]
    pub action_expectancies: AgentActionExpectancies,
    #[serde(default)]
    pub decision_attribution: AgentDecisionAttribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    pub alpha_horizon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalidation_components: Vec<AgentLensComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance: Option<ActionGovernanceContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecommendations {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_recommendation: Option<AgentMarketRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<AgentDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AgentRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_links: Vec<AgentKnowledgeLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "scope_kind", content = "data", rename_all = "snake_case")]
pub enum AgentDecision {
    Market(AgentMarketRecommendation),
    Sector(AgentSectorRecommendation),
    Symbol(AgentRecommendation),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMarketRecommendation {
    pub recommendation_id: String,
    pub tick: u64,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub edge_layer: String,
    pub bias: String,
    pub best_action: String,
    pub preferred_expression: String,
    pub market_impulse_score: Decimal,
    pub macro_regime_discontinuity: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    pub horizon_ticks: u64,
    pub alpha_horizon: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_not_single_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_sectors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisive_factors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
    pub average_return_at_decision: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<AgentRecommendationResolution>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance: ActionGovernanceContract,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSectorRecommendation {
    pub recommendation_id: String,
    pub tick: u64,
    pub market: LiveMarket,
    pub sector: String,
    pub regime_bias: String,
    pub edge_layer: String,
    pub bias: String,
    pub best_action: String,
    pub preferred_expression: String,
    pub sector_impulse_score: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    pub horizon_ticks: u64,
    pub alpha_horizon: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisive_factors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
    pub average_return_at_decision: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<AgentRecommendationResolution>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance: ActionGovernanceContract,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecommendationJournalRecord {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub composite_stress: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_headline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_recommendation: Option<AgentMarketRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<AgentDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AgentRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_links: Vec<AgentKnowledgeLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecommendationResolution {
    pub resolved_tick: u64,
    pub ticks_elapsed: u64,
    pub status: String,
    pub price_return: Decimal,
    pub follow_realized_return: Decimal,
    pub fade_realized_return: Decimal,
    pub wait_regret: Decimal,
    pub counterfactual_best_action: String,
    pub best_action_was_correct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecommendation {
    pub recommendation_id: String,
    pub tick: u64,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_label: Option<String>,
    pub bias: String,
    pub severity: String,
    pub confidence: Decimal,
    pub score: Decimal,
    pub horizon_ticks: u64,
    pub regime_bias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub why: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why_components: Vec<AgentLensComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_lenses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_next: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub do_not: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragility: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_transition: Option<String>,
    pub best_action: String,
    #[serde(flatten)]
    pub action_expectancies: AgentActionExpectancies,
    #[serde(default)]
    pub decision_attribution: AgentDecisionAttribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    pub alpha_horizon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_at_decision: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<AgentRecommendationResolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalidation_components: Vec<AgentLensComponent>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance: ActionGovernanceContract,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContextPrior {
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub resolved: usize,
    pub hit_rate: Decimal,
    pub expected_net_alpha: Decimal,
    #[serde(flatten)]
    pub action_expectancies: AgentActionExpectancies,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_through_rate: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rate: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure_retention_rate: Option<Decimal>,
}

impl AgentDecision {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        match self {
            AgentDecision::Market(item) => item.governance_contract(),
            AgentDecision::Sector(item) => item.governance_contract(),
            AgentDecision::Symbol(item) => item.governance_contract(),
        }
    }
}

impl AgentWatchlistEntry {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        governance_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
        )
    }

    pub fn governance_reason(&self) -> String {
        governance_reason_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
            self.execution_policy.unwrap_or(ActionExecutionPolicy::ReviewRequired),
        )
    }

    pub fn governance_reason_code(&self) -> ActionGovernanceReasonCode {
        governance_reason_code_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
            self.execution_policy.unwrap_or(ActionExecutionPolicy::ReviewRequired),
        )
    }
}

impl AgentMarketRecommendation {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        ActionGovernanceContract::for_recommendation(ActionExecutionPolicy::ManualOnly)
    }

    pub fn governance_reason(&self) -> String {
        governance_reason_for_signal_action(
            self.best_action.as_str(),
            "high",
            None,
            self.expected_net_alpha,
            self.execution_policy,
        )
    }

    pub fn governance_reason_code(&self) -> ActionGovernanceReasonCode {
        governance_reason_code_for_signal_action(
            self.best_action.as_str(),
            "high",
            None,
            self.expected_net_alpha,
            self.execution_policy,
        )
    }
}

impl AgentSectorRecommendation {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        ActionGovernanceContract::for_recommendation(ActionExecutionPolicy::ReviewRequired)
    }

    pub fn governance_reason(&self) -> String {
        governance_reason_for_signal_action(
            self.best_action.as_str(),
            "high",
            None,
            self.expected_net_alpha,
            self.execution_policy,
        )
    }

    pub fn governance_reason_code(&self) -> ActionGovernanceReasonCode {
        governance_reason_code_for_signal_action(
            self.best_action.as_str(),
            "high",
            None,
            self.expected_net_alpha,
            self.execution_policy,
        )
    }
}

impl AgentRecommendation {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        governance_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
        )
    }

    pub fn governance_reason(&self) -> String {
        governance_reason_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
            self.execution_policy,
        )
    }

    pub fn governance_reason_code(&self) -> ActionGovernanceReasonCode {
        governance_reason_code_for_signal_action(
            self.best_action.as_str(),
            self.severity.as_str(),
            self.invalidation_rule.as_deref(),
            self.expected_net_alpha,
            self.execution_policy,
        )
    }
}

pub(crate) fn governance_for_signal_action(
    best_action: &str,
    severity: &str,
    invalidation_rule: Option<&str>,
    expected_net_alpha: Option<Decimal>,
) -> ActionGovernanceContract {
    let policy = if matches!(best_action, "wait" | "ignore" | "review" | "observe") {
        ActionExecutionPolicy::ManualOnly
    } else if matches!(severity, "high" | "critical") {
        ActionExecutionPolicy::ReviewRequired
    } else if invalidation_rule
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        ActionExecutionPolicy::ReviewRequired
    } else if expected_net_alpha.unwrap_or(Decimal::ZERO) <= Decimal::ZERO {
        ActionExecutionPolicy::ReviewRequired
    } else {
        ActionExecutionPolicy::AutoEligible
    };

    let mut contract = ActionGovernanceContract::for_recommendation(policy);
    contract.allowed_transitions = vec![ActionStage::Suggest];
    contract
}

pub(crate) fn governance_reason_for_signal_action(
    best_action: &str,
    severity: &str,
    invalidation_rule: Option<&str>,
    expected_net_alpha: Option<Decimal>,
    policy: ActionExecutionPolicy,
) -> String {
    match governance_reason_code_for_signal_action(
        best_action,
        severity,
        invalidation_rule,
        expected_net_alpha,
        policy,
    ) {
        ActionGovernanceReasonCode::AdvisoryAction => {
            if matches!(best_action, "wait" | "ignore" | "review" | "observe") {
                format!("best_action=`{best_action}` stays advisory and does not open an execution workflow")
            } else {
                format!("policy=`manual_only` requires explicit operator action before `{best_action}` can progress")
            }
        }
        ActionGovernanceReasonCode::SeverityRequiresReview => {
            if matches!(severity, "high" | "critical") {
                format!("severity=`{severity}` forces human review before `{best_action}` can execute")
            }
            else {
                format!("policy=`review_required` requires confirmation before `{best_action}` can execute")
            }
        }
        ActionGovernanceReasonCode::InvalidationRuleMissing => {
            "missing invalidation rule keeps this recommendation in review-required mode".into()
        }
        ActionGovernanceReasonCode::NonPositiveExpectedAlpha => {
            "non-positive expected alpha keeps this recommendation in review-required mode".into()
        }
        ActionGovernanceReasonCode::OperatorActionRequired => {
            format!("policy=`manual_only` requires explicit operator action before `{best_action}` can progress")
        }
        ActionGovernanceReasonCode::AutoExecutionEligible => {
            "explicit invalidation rule and positive expected alpha make this recommendation auto-execute eligible".into()
        }
        ActionGovernanceReasonCode::WorkflowNotCreated
        | ActionGovernanceReasonCode::WorkflowTransitionWindow
        | ActionGovernanceReasonCode::AssignmentLockedDuringExecution
        | ActionGovernanceReasonCode::TerminalReviewStage => {
            format!("policy=`{policy}` governs `{best_action}`")
        }
    }
}

pub(crate) fn governance_reason_code_for_signal_action(
    best_action: &str,
    severity: &str,
    invalidation_rule: Option<&str>,
    expected_net_alpha: Option<Decimal>,
    policy: ActionExecutionPolicy,
) -> ActionGovernanceReasonCode {
    match policy {
        ActionExecutionPolicy::ManualOnly => {
            if matches!(best_action, "wait" | "ignore" | "review" | "observe") {
                ActionGovernanceReasonCode::AdvisoryAction
            } else {
                ActionGovernanceReasonCode::OperatorActionRequired
            }
        }
        ActionExecutionPolicy::ReviewRequired => {
            if matches!(severity, "high" | "critical") {
                ActionGovernanceReasonCode::SeverityRequiresReview
            } else if invalidation_rule
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                ActionGovernanceReasonCode::InvalidationRuleMissing
            } else if expected_net_alpha.unwrap_or(Decimal::ZERO) <= Decimal::ZERO {
                ActionGovernanceReasonCode::NonPositiveExpectedAlpha
            } else {
                ActionGovernanceReasonCode::OperatorActionRequired
            }
        }
        ActionExecutionPolicy::AutoEligible => ActionGovernanceReasonCode::AutoExecutionEligible,
    }
}
