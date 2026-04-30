use super::*;
use crate::action::workflow::{
    ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode,
};
use crate::agent::{
    AgentActionExpectancies, AgentBriefing, AgentDecision, AgentDecisionAttribution,
    AgentLensComponent, AgentRecommendation, AgentRecommendations, AgentSession, AgentSnapshot,
    AgentWakeState,
};
use crate::live_snapshot::{LiveMarket, LiveMarketRegime, LiveStressSnapshot};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[test]
fn parse_action_accepts_plain_json() {
    let action = parse_action(r#"{"action":"speak","message":"test"}"#).unwrap();
    assert_eq!(action.action, "speak");
    assert_eq!(action.message.as_deref(), Some("test"));
}

#[test]
fn parse_action_extracts_embedded_json() {
    let action = parse_action("```json\n{\"action\":\"silent\",\"reason\":\"none\"}\n```").unwrap();
    assert_eq!(action.action, "silent");
    assert_eq!(action.reason.as_deref(), Some("none"));
}

#[test]
fn parse_action_ignores_surrounding_nested_json() {
    let action = parse_action(
        "preface {\"meta\":{\"attempt\":1},\"action\":\"speak\",\"message\":\"ok\"} suffix",
    )
    .unwrap();
    assert_eq!(action.action, "speak");
    assert_eq!(action.message.as_deref(), Some("ok"));
}

#[test]
fn analysis_path_is_stable() {
    assert_eq!(
        analysis_path(CaseMarket::Hk),
        ("EDEN_AGENT_ANALYSIS_PATH", "data/agent_analysis.json")
    );
    assert_eq!(
        analysis_path(CaseMarket::Us),
        ("EDEN_US_AGENT_ANALYSIS_PATH", "data/us_agent_analysis.json")
    );
}

#[test]
fn narration_paths_are_stable() {
    assert_eq!(
        narration_path(CaseMarket::Hk),
        ("EDEN_AGENT_NARRATION_PATH", "data/agent_narration.json")
    );
    assert_eq!(
        narration_path(CaseMarket::Us),
        (
            "EDEN_US_AGENT_NARRATION_PATH",
            "data/us_agent_narration.json"
        )
    );
}

#[test]
fn first_present_env_returns_first_non_empty_value() {
    std::env::set_var("EDEN_AGENT_TEST_FIRST", "");
    std::env::set_var("EDEN_AGENT_TEST_SECOND", "abc");
    let value = first_present_env(&["EDEN_AGENT_TEST_FIRST", "EDEN_AGENT_TEST_SECOND"]);
    assert_eq!(value.as_deref(), Some("abc"));
    std::env::remove_var("EDEN_AGENT_TEST_FIRST");
    std::env::remove_var("EDEN_AGENT_TEST_SECOND");
}

#[test]
fn build_narration_emits_dominant_lenses_from_action_cards() {
    let snapshot = AgentSnapshot {
        tick: 10,
        timestamp: "2026-03-29T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: dec!(0.7),
            breadth_up: dec!(0.4),
            breadth_down: dec!(0.2),
            average_return: dec!(0.01),
            directional_consensus: Some(dec!(0.1)),
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        wake: AgentWakeState {
            should_speak: true,
            priority: dec!(0.8),
            headline: None,
            summary: vec![],
            focus_symbols: vec!["700.HK".into()],
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        perception: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        investigation_selections: vec![],
        sector_flows: vec![],
        symbols: vec![],
        events: vec![],
        cross_market_signals: vec![],
        raw_sources: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        perception_states: vec![],
        knowledge_links: vec![],
    };
    let briefing = AgentBriefing {
        tick: 10,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_speak: true,
        priority: dec!(0.7),
        headline: Some("headline".into()),
        summary: vec!["briefing".into()],
        dominant_intents: vec![],
        spoken_message: Some("spoken".into()),
        focus_symbols: vec!["700.HK".into()],
        reasons: vec![],
        current_investigations: vec![],
        current_judgments: vec![],
        executed_tools: vec![],
    };
    let session = AgentSession {
        tick: 10,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_speak: true,
        active_thread_count: 0,
        focus_symbols: vec!["700.HK".into()],
        active_threads: vec![],
        current_investigations: vec![],
        current_judgments: vec![],
        recent_turns: vec![],
    };
    let recommendations = AgentRecommendations {
        tick: 10,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: "neutral".into(),
        total: 1,
        market_recommendation: None,
        decisions: vec![AgentDecision::Symbol(AgentRecommendation {
            recommendation_id: "rec:10:700.HK:enter".into(),
            tick: 10,
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
            why_components: vec![
                AgentLensComponent {
                    lens_name: "iceberg".into(),
                    confidence: dec!(0.72),
                    content: "偵測到2次冰山回補".into(),
                    tags: vec!["iceberg".into()],
                },
                AgentLensComponent {
                    lens_name: "structural".into(),
                    confidence: dec!(0.68),
                    content: "結構 strengthening (streak=3)".into(),
                    tags: vec!["structure".into()],
                },
            ],
            primary_lens: Some("iceberg".into()),
            supporting_lenses: vec!["structural".into()],
            review_lens: Some("iceberg".into()),
            watch_next: vec![],
            do_not: vec![],
            fragility: vec![],
            transition: None,
            thesis_family: Some("Directed Flow".into()),
            matched_success_pattern_signature: None,
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
        })],
        items: vec![],
        knowledge_links: vec![],
    };

    let narration = build_narration(
        &snapshot,
        &briefing,
        &session,
        None,
        Some(&recommendations),
        None,
    );

    assert_eq!(narration.dominant_lenses.len(), 2);
    assert_eq!(narration.dominant_lenses[0].lens_name, "iceberg");
    assert_eq!(narration.dominant_lenses[0].card_count, 1);
    assert_eq!(narration.dominant_lenses[0].max_confidence, dec!(0.72));
    assert!(narration
        .bullets
        .iter()
        .any(|item| item.contains("Dominant lenses: Iceberg 72")));
}
