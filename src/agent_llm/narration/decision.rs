use super::*;

pub(super) fn narration_decision_tag(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => format!("market_{}", item.best_action),
        AgentDecision::Sector(item) => format!("sector_{}", item.best_action),
        AgentDecision::Symbol(item) => item.action.clone(),
    }
}

pub(super) fn narration_decision_alert_level(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => {
            if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            }
        }
        AgentDecision::Sector(item) => {
            if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            }
        }
        AgentDecision::Symbol(item) => item.severity.clone(),
    }
}

pub(super) fn narration_decision_watch_next(decision: &AgentDecision) -> Vec<String> {
    match decision {
        AgentDecision::Market(item) => item.decisive_factors.iter().take(3).cloned().collect(),
        AgentDecision::Sector(item) => item.decisive_factors.iter().take(3).cloned().collect(),
        AgentDecision::Symbol(item) => item.watch_next.clone(),
    }
}

pub(super) fn narration_decision_do_not(decision: &AgentDecision) -> Vec<String> {
    match decision {
        AgentDecision::Market(item) => item
            .why_not_single_name
            .clone()
            .into_iter()
            .collect::<Vec<_>>(),
        AgentDecision::Sector(_) => vec![],
        AgentDecision::Symbol(item) => item.do_not.clone(),
    }
}

pub(super) fn narration_decision_fragility(decision: &AgentDecision) -> Vec<String> {
    match decision {
        AgentDecision::Symbol(item) => item.fragility.clone(),
        _ => vec![],
    }
}

pub(super) fn narration_decision_confidence_band(decision: &AgentDecision) -> Option<String> {
    let score = match decision {
        AgentDecision::Market(item) => item.market_impulse_score,
        AgentDecision::Sector(item) => item.sector_impulse_score,
        AgentDecision::Symbol(item) => item.confidence,
    };
    Some(
        if score >= rust_decimal::Decimal::new(8, 2) {
            "high"
        } else if score >= rust_decimal::Decimal::new(4, 2) {
            "medium"
        } else {
            "low"
        }
        .to_string(),
    )
}

pub(super) fn narration_decision_should_alert(decision: &AgentDecision) -> bool {
    match decision {
        AgentDecision::Market(item) => item.best_action != "wait",
        AgentDecision::Sector(item) => item.best_action != "wait",
        AgentDecision::Symbol(item) => item.action != "ignore",
    }
}

pub(super) fn narration_decision_headline(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => {
            format!(
                "{} {} via {}",
                market_label(item.market),
                item.best_action,
                item.preferred_expression
            )
        }
        AgentDecision::Sector(item) => {
            format!(
                "{} sector {} via {}",
                item.sector, item.best_action, item.preferred_expression
            )
        }
        AgentDecision::Symbol(item) => format!(
            "{} {} {}",
            item.symbol,
            item.action_label
                .clone()
                .unwrap_or_else(|| item.action.clone()),
            item.regime_bias
        ),
    }
}

pub(super) fn narration_decision_primary_action(decision: &AgentDecision) -> Option<String> {
    match decision {
        AgentDecision::Market(item) => {
            (item.best_action != "wait").then(|| format!("market_{}", item.best_action))
        }
        AgentDecision::Sector(item) => {
            (item.best_action != "wait").then(|| format!("sector_{}", item.best_action))
        }
        AgentDecision::Symbol(item) => Some(item.action.clone()),
    }
}

pub(super) fn narration_decision_why(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => item.summary.clone(),
        AgentDecision::Sector(item) => item.summary.clone(),
        AgentDecision::Symbol(item) => item.why.clone(),
    }
}

pub(super) fn narration_decision_id(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => item.recommendation_id.clone(),
        AgentDecision::Sector(item) => item.recommendation_id.clone(),
        AgentDecision::Symbol(item) => item.recommendation_id.clone(),
    }
}

pub(super) fn narration_action_card(decision: &AgentDecision) -> AgentNarrationActionCard {
    match decision {
        AgentDecision::Market(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:market", item.tick),
            scope_kind: "market".into(),
            symbol: market_label(item.market).into(),
            sector: None,
            edge_layer: Some(item.edge_layer.clone()),
            setup_id: Some(item.recommendation_id.clone()),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            severity: if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            },
            title: Some(format!(
                "{} macro / market setup",
                market_label(item.market)
            )),
            summary: item.summary.clone(),
            why_now: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            confidence_band: narration_decision_confidence_band(decision),
            watch_next: item.decisive_factors.iter().take(3).cloned().collect(),
            do_not: item
                .why_not_single_name
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow")
                    .then_some(item.expected_net_alpha)
                    .flatten(),
                fade_expectancy: (item.best_action == "fade")
                    .then_some(item.expected_net_alpha)
                    .flatten(),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
        AgentDecision::Sector(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:sector:{}", item.tick, item.sector),
            scope_kind: "sector".into(),
            symbol: item.sector.clone(),
            sector: Some(item.sector.clone()),
            edge_layer: Some(item.edge_layer.clone()),
            setup_id: Some(item.recommendation_id.clone()),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            severity: if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            },
            title: Some(format!("{} sector setup", item.sector)),
            summary: item.summary.clone(),
            why_now: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            confidence_band: narration_decision_confidence_band(decision),
            watch_next: item.decisive_factors.iter().take(3).cloned().collect(),
            do_not: vec![],
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow")
                    .then_some(item.expected_net_alpha)
                    .flatten(),
                fade_expectancy: (item.best_action == "fade")
                    .then_some(item.expected_net_alpha)
                    .flatten(),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
        AgentDecision::Symbol(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:{}", item.tick, item.symbol),
            scope_kind: "symbol".into(),
            symbol: item.symbol.clone(),
            sector: item.sector.clone(),
            edge_layer: None,
            setup_id: Some(item.recommendation_id.clone()),
            action: item.action.clone(),
            action_label: item.action_label.clone(),
            severity: item.severity.clone(),
            title: item.title.clone(),
            summary: item.transition.clone().unwrap_or_else(|| item.why.clone()),
            why_now: item.why.clone(),
            why_components: item.why_components.clone(),
            primary_lens: item.primary_lens.clone(),
            supporting_lenses: item.supporting_lenses.clone(),
            review_lens: item.review_lens.clone(),
            confidence_band: narration_decision_confidence_band(decision),
            watch_next: item.watch_next.clone(),
            do_not: item.do_not.clone(),
            thesis_family: item.thesis_family.clone(),
            state_transition: item.state_transition.clone(),
            best_action: item.best_action.clone(),
            action_expectancies: item.action_expectancies.clone(),
            decision_attribution: item.decision_attribution.clone(),
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: None,
            reference_symbols: vec![item.symbol.clone()],
            invalidation_rule: item.invalidation_rule.clone(),
            invalidation_components: item.invalidation_components.clone(),
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
    }
}

pub(super) fn market_label(market: crate::live_snapshot::LiveMarket) -> &'static str {
    match market {
        crate::live_snapshot::LiveMarket::Hk => "HK",
        crate::live_snapshot::LiveMarket::Us => "US",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::workflow::{
        ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode,
    };
    use crate::agent::{
        AgentActionExpectancies, AgentDecisionAttribution, AgentLensComponent, AgentRecommendation,
    };
    use rust_decimal_macros::dec;

    #[test]
    fn symbol_action_card_preserves_structured_lens_components() {
        let decision = AgentDecision::Symbol(AgentRecommendation {
            recommendation_id: "rec:1:700.HK:enter".into(),
            tick: 1,
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            title: Some("Long 700.HK".into()),
            action: "enter".into(),
            action_label: Some("Enter".into()),
            bias: "long".into(),
            severity: "high".into(),
            confidence: dec!(0.8),
            score: dec!(0.85),
            horizon_ticks: 8,
            regime_bias: "neutral".into(),
            status: Some("new".into()),
            why: "偵測到2次冰山回補 | 結構 strengthening (streak=3)".into(),
            why_components: vec![AgentLensComponent {
                lens_name: "iceberg".into(),
                confidence: dec!(0.72),
                content: "偵測到2次冰山回補".into(),
                tags: vec!["iceberg".into()],
            }],
            primary_lens: Some("iceberg".into()),
            supporting_lenses: vec!["structural".into()],
            review_lens: Some("iceberg".into()),
            watch_next: vec![],
            do_not: vec![],
            fragility: vec![],
            transition: None,
            thesis_family: Some("Directed Flow".into()),
            state_transition: None,
            best_action: "follow".into(),
            action_expectancies: AgentActionExpectancies::default(),
            decision_attribution: AgentDecisionAttribution::default(),
            expected_net_alpha: Some(dec!(0.02)),
            alpha_horizon: "intraday:8t".into(),
            price_at_decision: None,
            resolution: None,
            invalidation_rule: Some("冰山回補停止".into()),
            invalidation_components: vec![AgentLensComponent {
                lens_name: "iceberg".into(),
                confidence: dec!(0.72),
                content: "冰山回補停止".into(),
                tags: vec!["iceberg".into()],
            }],
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance: ActionGovernanceContract::for_recommendation(
                ActionExecutionPolicy::ReviewRequired,
            ),
            governance_reason_code: ActionGovernanceReasonCode::SeverityRequiresReview,
            governance_reason: "severity=`high` forces human review before `enter` can execute"
                .into(),
        });

        let card = narration_action_card(&decision);
        assert_eq!(card.why_components.len(), 1);
        assert_eq!(card.invalidation_components.len(), 1);
        assert_eq!(card.why_components[0].lens_name, "iceberg");
        assert_eq!(card.invalidation_components[0].content, "冰山回補停止");
        assert_eq!(card.primary_lens.as_deref(), Some("iceberg"));
        assert_eq!(card.supporting_lenses, vec!["structural"]);
        assert_eq!(card.review_lens.as_deref(), Some("iceberg"));
    }
}
