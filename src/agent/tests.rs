use super::*;
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract};
use super::attention::{build_sector_flows, build_wake_state};
use super::builders::build_broker_state;
use super::shared::extract_symbols;
use crate::ontology::{
    ActionDirection, ActionNodeStage, BackwardCause, CausalContestState, Market,
    ProvenanceMetadata, ProvenanceSource, WorldLayer,
};
use rust_decimal_macros::dec;
use time::OffsetDateTime;

fn signal_state(composite: Decimal, capital_flow: Decimal) -> AgentSignalState {
    AgentSignalState {
        composite,
        mark_price: None,
        capital_flow_direction: capital_flow,
        price_momentum: Decimal::ZERO,
        volume_profile: Decimal::ZERO,
        pre_post_market_anomaly: Decimal::ZERO,
        valuation: Decimal::ZERO,
        sector_coherence: None,
        cross_stock_correlation: None,
        cross_market_propagation: None,
    }
}

fn symbol_state(symbol: &str, sector: &str, composite: Decimal) -> AgentSymbolState {
    AgentSymbolState {
        symbol: symbol.into(),
        sector: Some(sector.into()),
        structure: None,
        signal: Some(signal_state(composite, composite)),
        depth: None,
        brokers: None,
        invalidation: None,
        pressure: None,
        active_position: None,
        latest_events: vec![],
    }
}

fn action_expectancies(
    follow_expectancy: Option<Decimal>,
    fade_expectancy: Option<Decimal>,
) -> AgentActionExpectancies {
    AgentActionExpectancies {
        follow_expectancy,
        fade_expectancy,
        wait_expectancy: Some(Decimal::ZERO),
    }
}

fn base_snapshot(symbols: Vec<AgentSymbolState>, bias: &str) -> AgentSnapshot {
    AgentSnapshot {
        tick: 10,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: bias.into(),
            confidence: dec!(0.7),
            breadth_up: if bias == "risk_on" {
                dec!(0.75)
            } else {
                dec!(0.04)
            },
            breadth_down: if bias == "risk_off" {
                dec!(0.93)
            } else {
                dec!(0.10)
            },
            average_return: if bias == "risk_off" {
                dec!(-0.03)
            } else {
                dec!(0.02)
            },
            directional_consensus: Some(if bias == "risk_off" {
                dec!(-0.1)
            } else {
                dec!(0.1)
            }),
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
            focus_symbols: symbols.iter().map(|item| item.symbol.clone()).collect(),
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        sector_flows: vec![],
        symbols,
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    }
}

fn trending_snapshot() -> AgentSnapshot {
    let mut snapshot = base_snapshot(vec![], "neutral");
    snapshot.market_regime.breadth_up = dec!(0.84);
    snapshot.market_regime.breadth_down = dec!(0.08);
    snapshot.market_regime.average_return = dec!(0.022);
    snapshot.stress.composite_stress = dec!(0.66);
    snapshot.stress.sector_synchrony = Some(dec!(0.96));
    snapshot.stress.pressure_consensus = Some(dec!(0.82));
    snapshot.recent_transitions = vec![
        AgentTransition {
            from_tick: 9,
            to_tick: 10,
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            setup_id: Some("s1".into()),
            title: "Tencent".into(),
            from_state: Some("review:stable".into()),
            to_state: "review:strengthening".into(),
            confidence: dec!(0.8),
            summary: "Tencent status stable -> strengthening".into(),
            transition_reason: None,
        },
        AgentTransition {
            from_tick: 9,
            to_tick: 10,
            symbol: "5.HK".into(),
            sector: Some("Finance".into()),
            setup_id: Some("s2".into()),
            title: "HSBC".into(),
            from_state: Some("review:stable".into()),
            to_state: "review:strengthening".into(),
            confidence: dec!(0.8),
            summary: "HSBC status stable -> strengthening".into(),
            transition_reason: None,
        },
    ];
    snapshot.sector_flows = vec![
        AgentSectorFlow {
            sector: "Finance".into(),
            member_count: 10,
            average_composite: dec!(0.4),
            average_capital_flow: dec!(0.2),
            leaders: vec!["5.HK".into()],
            exceptions: vec![],
            summary: "Finance strong".into(),
        },
        AgentSectorFlow {
            sector: "Property".into(),
            member_count: 10,
            average_composite: dec!(0.3),
            average_capital_flow: dec!(0.1),
            leaders: vec!["16.HK".into()],
            exceptions: vec![],
            summary: "Property strong".into(),
        },
    ];
    snapshot
}

#[test]
fn build_broker_state_detects_entries_exits_and_side_switches() {
    let previous = AgentBrokerState {
        current: vec![AgentBrokerInstitution {
            institution_id: 1,
            name: "Alpha".into(),
            bid_positions: vec![],
            ask_positions: vec![1],
            seat_count: 1,
        }],
        entered: vec![],
        exited: vec![],
        switched_to_bid: vec![],
        switched_to_ask: vec![],
    };

    let current = vec![
        AgentBrokerInstitution {
            institution_id: 1,
            name: "Alpha".into(),
            bid_positions: vec![1],
            ask_positions: vec![],
            seat_count: 1,
        },
        AgentBrokerInstitution {
            institution_id: 2,
            name: "Beta".into(),
            bid_positions: vec![2],
            ask_positions: vec![],
            seat_count: 1,
        },
    ];

    let state = build_broker_state(&current, Some(&previous));
    assert_eq!(state.entered, vec!["Beta"]);
    assert!(state.exited.is_empty());
    assert!(state.switched_to_bid.contains(&"Alpha".into()));
}

#[test]
fn sector_flows_capture_exceptions_against_sector_direction() {
    let flows = build_sector_flows(&[
        symbol_state("700.HK", "Technology", dec!(0.8)),
        symbol_state("9988.HK", "Technology", dec!(-0.6)),
        symbol_state("1810.HK", "Technology", dec!(0.4)),
    ]);

    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].sector, "Technology");
    assert!(flows[0].exceptions.contains(&"9988.HK".into()));
    assert_eq!(flows[0].leaders[0], "700.HK");
}

#[test]
fn extract_symbols_finds_hk_and_us_tokens() {
    let symbols = extract_symbols("9988.HK flipped while NVDA.US followed and 700.HK held.");
    assert!(symbols.contains(&"9988.HK".into()));
    assert!(symbols.contains(&"NVDA.US".into()));
    assert!(symbols.contains(&"700.HK".into()));
}

#[test]
fn wake_state_prefers_current_tick_transitions() {
    let wake = build_wake_state(
        10,
        &[AgentNotice {
            notice_id: "n1".into(),
            tick: 10,
            kind: "transition".into(),
            symbol: Some("9988.HK".into()),
            sector: Some("Technology".into()),
            title: "transition".into(),
            summary: "9988.HK shifted".into(),
            significance: dec!(0.8),
        }],
        &[AgentTransition {
            from_tick: 9,
            to_tick: 10,
            symbol: "9988.HK".into(),
            sector: Some("Technology".into()),
            setup_id: Some("setup-1".into()),
            title: "Alibaba".into(),
            from_state: Some("review:stable".into()),
            to_state: "enter:weakening".into(),
            confidence: dec!(0.7),
            summary: "Alibaba action review -> enter".into(),
            transition_reason: Some("rotation".into()),
        }],
        &[symbol_state("9988.HK", "Technology", dec!(0.5))],
        &[],
        &[],
    );

    assert!(wake.should_speak);
    assert_eq!(
        wake.headline.as_deref(),
        Some("Alibaba action review -> enter")
    );
    assert!(wake.focus_symbols.contains(&"9988.HK".into()));
    assert!(wake
        .suggested_tools
        .iter()
        .any(|item| item.tool == "transitions_since"));
}

#[test]
fn tool_catalog_includes_core_queries() {
    let catalog = tool_catalog();
    assert!(catalog.iter().any(|item| item.name == "wake"));
    assert!(catalog.iter().any(|item| item.name == "depth_change"));
    assert!(catalog
        .iter()
        .any(|item| item.name == "backward_investigation"));
    assert!(catalog.iter().any(|item| {
        item.name == "notices"
            && item.category == AgentToolCategory::Feed
            && item.route == "/api/feed/:market/notices"
    }));
    assert!(catalog.iter().any(|item| {
        item.name == "world_state"
            && item.category == AgentToolCategory::ObjectQuery
            && item.route == "/api/ontology/:market/world"
    }));
}

#[test]
fn execute_tool_reads_symbol_state() {
    let snapshot = AgentSnapshot {
        tick: 1,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
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
            should_speak: false,
            priority: Decimal::ZERO,
            headline: None,
            summary: vec![],
            focus_symbols: vec![],
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        sector_flows: vec![],
        symbols: vec![symbol_state("700.HK", "Technology", dec!(0.4))],
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    };

    let output = execute_tool(
        &snapshot,
        None,
        &AgentToolRequest {
            tool: "symbol_state".into(),
            symbol: Some("700.HK".into()),
            sector: None,
            since_tick: None,
            limit: None,
        },
    )
    .unwrap();

    match output {
        AgentToolOutput::Symbol(state) => assert_eq!(state.symbol, "700.HK"),
        other => panic!("unexpected output: {:?}", other),
    }
}

#[test]
fn build_briefing_uses_suggested_tool_previews() {
    let snapshot = AgentSnapshot {
        tick: 10,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
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
            priority: dec!(0.9),
            headline: Some("9988.HK shifted".into()),
            summary: vec!["9988.HK shifted".into()],
            focus_symbols: vec!["9988.HK".into()],
            reasons: vec!["transition".into()],
            suggested_tools: vec![AgentSuggestedToolCall {
                tool: "symbol_state".into(),
                args: json!({"symbol":"9988.HK"}),
                reason: "inspect".into(),
            }],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        sector_flows: vec![],
        symbols: vec![symbol_state("9988.HK", "Technology", dec!(0.7))],
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    };

    let briefing = build_briefing(&snapshot);
    assert!(briefing.should_speak);
    assert!(briefing
        .executed_tools
        .iter()
        .any(|item| item.tool == "symbol_state"));
    assert!(briefing.spoken_message.unwrap().contains("9988.HK"));
}

#[test]
fn build_session_creates_active_thread_for_focus_symbol() {
    let snapshot = AgentSnapshot {
        tick: 11,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
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
            headline: Some("700.HK held".into()),
            summary: vec!["700.HK held".into()],
            focus_symbols: vec!["700.HK".into()],
            reasons: vec!["700.HK held".into()],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![AgentNotice {
            notice_id: "n-700".into(),
            tick: 11,
            kind: "transition".into(),
            symbol: Some("700.HK".into()),
            sector: Some("Technology".into()),
            title: "700".into(),
            summary: "700.HK held".into(),
            significance: dec!(0.8),
        }],
        active_structures: vec![],
        recent_transitions: vec![AgentTransition {
            from_tick: 10,
            to_tick: 11,
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            setup_id: Some("s1".into()),
            title: "Tencent".into(),
            from_state: Some("review:stable".into()),
            to_state: "enter:strengthening".into(),
            confidence: dec!(0.75),
            summary: "Tencent action review -> enter".into(),
            transition_reason: None,
        }],
        sector_flows: vec![],
        symbols: vec![symbol_state("700.HK", "Technology", dec!(0.6))],
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    };
    let briefing = build_briefing(&snapshot);
    let session = build_session(&snapshot, &briefing, None);

    assert_eq!(session.active_thread_count, 1);
    assert_eq!(session.active_threads[0].symbol, "700.HK");
    assert_eq!(session.active_threads[0].status, "escalated");
    assert_eq!(session.recent_turns.len(), 1);
}

#[test]
fn execute_tool_reads_session_threads() {
    let session = AgentSession {
        tick: 2,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        should_speak: true,
        active_thread_count: 1,
        focus_symbols: vec!["700.HK".into()],
        active_threads: vec![AgentThread {
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            status: "active".into(),
            first_tick: 1,
            last_tick: 2,
            idle_ticks: 0,
            turns_observed: 2,
            priority: dec!(0.7),
            title: Some("Tencent".into()),
            headline: Some("700.HK active".into()),
            latest_summary: Some("Tencent enter conf=+0.700".into()),
            last_transition: Some("enter".into()),
            current_leader: None,
            invalidation_status: None,
            reasons: vec!["reason".into()],
        }],
        recent_turns: vec![],
    };
    let snapshot = AgentSnapshot {
        tick: 2,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
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
            priority: dec!(0.7),
            headline: Some("700.HK active".into()),
            summary: vec!["700.HK active".into()],
            focus_symbols: vec!["700.HK".into()],
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        sector_flows: vec![],
        symbols: vec![symbol_state("700.HK", "Technology", dec!(0.4))],
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    };

    let output = execute_tool(
        &snapshot,
        Some(&session),
        &AgentToolRequest {
            tool: "threads".into(),
            symbol: Some("700.HK".into()),
            sector: None,
            since_tick: None,
            limit: None,
        },
    )
    .unwrap();

    match output {
        AgentToolOutput::Thread(thread) => assert_eq!(thread.symbol, "700.HK"),
        other => panic!("unexpected output: {:?}", other),
    }
}

#[test]
fn recommendations_block_long_chasing_in_risk_off() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.09));
    state.structure = Some(AgentStructureState {
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:700.HK:review".into()),
        title: "Long 700.HK".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(10),
        status_streak: Some(2),
        confidence: dec!(0.09),
        confidence_change: Some(dec!(0.01)),
        confidence_gap: Some(dec!(1)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: action_expectancies(Some(dec!(0.03)), Some(dec!(-0.02))),
        expected_net_alpha: Some(dec!(0.03)),
        alpha_horizon: Some("intraday:10t".into()),
        invalidation_rule: Some("institutional alignment flips negative".into()),
    });
    state.depth = Some(AgentDepthState {
        imbalance: dec!(0.2),
        imbalance_change: dec!(0.1),
        bid_best_ratio: dec!(0.2),
        bid_best_ratio_change: dec!(0.1),
        ask_best_ratio: dec!(0.1),
        ask_best_ratio_change: dec!(0),
        bid_top3_ratio: dec!(0.3),
        bid_top3_ratio_change: dec!(0.1),
        ask_top3_ratio: dec!(0.1),
        ask_top3_ratio_change: dec!(0),
        spread: Some(dec!(0.01)),
        spread_change: Some(dec!(0)),
        bid_total_volume: 100,
        ask_total_volume: 80,
        bid_total_volume_change: 10,
        ask_total_volume_change: 0,
        summary: "bid heavy".into(),
    });

    let snapshot = base_snapshot(vec![state], "risk_off");
    let recommendations = build_recommendations(&snapshot, None);
    let recommendation = &recommendations.items[0];

    assert_eq!(recommendation.action, "review");
    assert!(recommendation
        .do_not
        .iter()
        .any(|item| item.contains("risk_off")));
}

#[test]
fn recommendations_allow_confirmed_short_entry_in_risk_off() {
    let mut state = symbol_state("9988.HK", "Technology", dec!(-0.11));
    state.structure = Some(AgentStructureState {
        symbol: "9988.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:9988.HK:review".into()),
        title: "Short 9988.HK".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(10),
        status_streak: Some(2),
        confidence: dec!(0.11),
        confidence_change: Some(dec!(0.02)),
        confidence_gap: Some(dec!(1)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: action_expectancies(Some(dec!(0.025)), Some(dec!(-0.03))),
        expected_net_alpha: Some(dec!(0.04)),
        alpha_horizon: Some("intraday:10t".into()),
        invalidation_rule: Some("depth no longer confirms".into()),
    });
    state.depth = Some(AgentDepthState {
        imbalance: dec!(-0.2),
        imbalance_change: dec!(-0.1),
        bid_best_ratio: dec!(0.1),
        bid_best_ratio_change: dec!(0),
        ask_best_ratio: dec!(0.2),
        ask_best_ratio_change: dec!(0.1),
        bid_top3_ratio: dec!(0.1),
        bid_top3_ratio_change: dec!(0),
        ask_top3_ratio: dec!(0.3),
        ask_top3_ratio_change: dec!(0.1),
        spread: Some(dec!(0.01)),
        spread_change: Some(dec!(0)),
        bid_total_volume: 80,
        ask_total_volume: 100,
        bid_total_volume_change: 0,
        ask_total_volume_change: 10,
        summary: "ask heavy".into(),
    });
    state.brokers = Some(AgentBrokerState {
        current: vec![],
        entered: vec!["BrokerX".into()],
        exited: vec![],
        switched_to_bid: vec![],
        switched_to_ask: vec!["BrokerX".into()],
    });
    let mut snapshot = base_snapshot(vec![state], "risk_off");
    snapshot.recent_transitions.push(AgentTransition {
        from_tick: 9,
        to_tick: 10,
        symbol: "9988.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:9988.HK:review".into()),
        title: "Short 9988.HK".into(),
        from_state: Some("review:stable".into()),
        to_state: "review:strengthening".into(),
        confidence: dec!(0.11),
        summary: "Short 9988.HK status stable -> strengthening".into(),
        transition_reason: Some("flow confirms".into()),
    });

    let recommendations = build_recommendations(&snapshot, None);
    let recommendation = &recommendations.items[0];

    assert_eq!(recommendation.action, "enter");
    assert_eq!(recommendation.bias, "short");
}

#[test]
fn recommendations_can_choose_fade_even_when_runtime_action_is_enter() {
    let mut state = symbol_state("388.HK", "Finance", dec!(0.10));
    state.structure = Some(AgentStructureState {
        symbol: "388.HK".into(),
        sector: Some("Finance".into()),
        setup_id: Some("setup:388.HK:enter".into()),
        title: "Long 388.HK".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(7),
        status_streak: Some(2),
        confidence: dec!(0.10),
        confidence_change: Some(dec!(0.02)),
        confidence_gap: Some(dec!(1)),
        transition_reason: Some("leader remains contested".into()),
        contest_state: Some("contested".into()),
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: Some("leader remains contested".into()),
        thesis_family: Some("Breakout Contagion".into()),
        action_expectancies: action_expectancies(Some(dec!(-0.0024)), Some(dec!(0.0024))),
        expected_net_alpha: Some(dec!(-0.0024)),
        alpha_horizon: Some("intraday:10t".into()),
        invalidation_rule: Some("breakout loses follow-through".into()),
    });
    state.depth = Some(AgentDepthState {
        imbalance: dec!(0.2),
        imbalance_change: dec!(0.1),
        bid_best_ratio: dec!(0.2),
        bid_best_ratio_change: dec!(0.1),
        ask_best_ratio: dec!(0.1),
        ask_best_ratio_change: dec!(0),
        bid_top3_ratio: dec!(0.3),
        bid_top3_ratio_change: dec!(0.1),
        ask_top3_ratio: dec!(0.1),
        ask_top3_ratio_change: dec!(0),
        spread: Some(dec!(0.01)),
        spread_change: Some(dec!(0)),
        bid_total_volume: 100,
        ask_total_volume: 80,
        bid_total_volume_change: 10,
        ask_total_volume_change: 0,
        summary: "bid heavy".into(),
    });

    let mut snapshot = base_snapshot(vec![state], "neutral");
    snapshot.recent_transitions.push(AgentTransition {
        from_tick: 9,
        to_tick: 10,
        symbol: "388.HK".into(),
        sector: Some("Finance".into()),
        setup_id: Some("setup:388.HK:enter".into()),
        title: "Long 388.HK".into(),
        from_state: Some("review:stable".into()),
        to_state: "enter:strengthening".into(),
        confidence: dec!(0.10),
        summary: "Long 388.HK leader remains contested".into(),
        transition_reason: Some("leader remains contested".into()),
    });

    let recommendations = build_recommendations(&snapshot, None);
    let recommendation = &recommendations.items[0];

    assert_eq!(recommendation.action, "enter");
    assert_eq!(recommendation.best_action, "fade");
    assert_eq!(
        recommendation.expected_net_alpha,
        recommendation.action_expectancies.fade_expectancy
    );
    assert!(recommendation.expected_net_alpha.unwrap() > Decimal::ZERO);
    assert_eq!(
        recommendation
            .decision_attribution
            .historical_expectancies
            .fade_expectancy,
        Some(dec!(0.0024))
    );
    assert!(recommendation
        .decision_attribution
        .decisive_factors
        .iter()
        .any(|item| item.contains("historical prior")));
    assert!(recommendation
        .decision_attribution
        .decisive_factors
        .iter()
        .any(|item| item.contains("fragile")));
}

#[test]
fn recommendations_default_to_wait_when_alpha_prior_is_too_thin() {
    let mut state = symbol_state("941.HK", "Telecom", dec!(0.09));
    state.structure = Some(AgentStructureState {
        symbol: "941.HK".into(),
        sector: Some("Telecom".into()),
        setup_id: Some("setup:941.HK:review".into()),
        title: "Long 941.HK".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(5),
        status_streak: Some(2),
        confidence: dec!(0.09),
        confidence_change: Some(dec!(0.01)),
        confidence_gap: Some(dec!(1)),
        transition_reason: Some("status stable -> strengthening".into()),
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: action_expectancies(None, None),
        expected_net_alpha: Some(dec!(0)),
        alpha_horizon: Some("intraday:10t".into()),
        invalidation_rule: Some("flow confirmation disappears".into()),
    });
    state.depth = Some(AgentDepthState {
        imbalance: dec!(0.2),
        imbalance_change: dec!(0.1),
        bid_best_ratio: dec!(0.2),
        bid_best_ratio_change: dec!(0.1),
        ask_best_ratio: dec!(0.1),
        ask_best_ratio_change: dec!(0),
        bid_top3_ratio: dec!(0.3),
        bid_top3_ratio_change: dec!(0.1),
        ask_top3_ratio: dec!(0.1),
        ask_top3_ratio_change: dec!(0),
        spread: Some(dec!(0.01)),
        spread_change: Some(dec!(0)),
        bid_total_volume: 100,
        ask_total_volume: 80,
        bid_total_volume_change: 10,
        ask_total_volume_change: 0,
        summary: "bid heavy".into(),
    });

    let recommendations = build_recommendations(&base_snapshot(vec![state], "neutral"), None);
    let recommendation = &recommendations.items[0];

    assert_eq!(recommendation.best_action, "wait");
    assert_eq!(recommendation.expected_net_alpha, None);
    assert_eq!(
        recommendation.action_expectancies.wait_expectancy,
        Some(Decimal::ZERO)
    );
    assert!(recommendation
        .decision_attribution
        .historical_expectancies
        .follow_expectancy
        .is_none());
}

#[test]
fn recommendations_compose_lens_fragments_for_hk_symbol() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.09));
    state.structure = Some(AgentStructureState {
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:700.HK:review".into()),
        title: "Long 700.HK".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(12),
        status_streak: Some(3),
        confidence: dec!(0.82),
        confidence_change: Some(dec!(0.03)),
        confidence_gap: Some(dec!(0.12)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: Some("leader remains flow".into()),
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: AgentActionExpectancies {
            follow_expectancy: Some(dec!(0.012)),
            fade_expectancy: Some(dec!(-0.003)),
            wait_expectancy: Some(Decimal::ZERO),
        },
        expected_net_alpha: Some(dec!(0.012)),
        alpha_horizon: Some("intraday:20t".into()),
        invalidation_rule: Some("institutional alignment flips negative".into()),
    });
    state.depth = Some(AgentDepthState {
        imbalance: dec!(0.3),
        imbalance_change: dec!(0.1),
        bid_best_ratio: dec!(0.22),
        bid_best_ratio_change: dec!(0.05),
        ask_best_ratio: dec!(0.11),
        ask_best_ratio_change: dec!(0),
        bid_top3_ratio: dec!(0.35),
        bid_top3_ratio_change: dec!(0.05),
        ask_top3_ratio: dec!(0.12),
        ask_top3_ratio_change: dec!(0),
        spread: Some(dec!(0.01)),
        spread_change: Some(dec!(0)),
        bid_total_volume: 120,
        ask_total_volume: 80,
        bid_total_volume_change: 10,
        ask_total_volume_change: 0,
        summary: "bid replenishes".into(),
    });
    state.brokers = Some(AgentBrokerState {
        current: vec![],
        entered: vec!["6998".into()],
        exited: vec![],
        switched_to_bid: vec!["6998".into()],
        switched_to_ask: vec![],
    });
    state.invalidation = Some(AgentInvalidationState {
        status: "armed".into(),
        invalidated: false,
        transition_reason: None,
        leading_falsifier: Some("alignment flips negative".into()),
        rules: vec!["sector coherence < 0.3".into()],
    });
    state.latest_events = vec![LiveEvent {
        kind: "IcebergDetected".into(),
        symbol: Some("700.HK".into()),
        magnitude: dec!(0.72),
        summary: "broker replenished quickly".into(),
    }];

    let mut snapshot = base_snapshot(vec![state], "neutral");
    snapshot.backward_reasoning = Some(BackwardReasoningSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        investigations: vec![BackwardInvestigation {
            investigation_id: "backward:700.HK".into(),
            leaf_scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            leaf_label: "Long 700.HK".into(),
            leaf_regime: "flow-led".into(),
            contest_state: CausalContestState::Stable,
            leading_cause_streak: 4,
            previous_leading_cause_id: None,
            leading_cause: Some(BackwardCause {
                cause_id: "cause:market:700.HK".into(),
                scope: ReasoningScope::market(),
                layer: WorldLayer::Forest,
                depth: 1,
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
                explanation: "institutional flow dominates".into(),
                chain_summary: None,
                confidence: dec!(0.6),
                support_weight: dec!(0.5),
                contradict_weight: dec!(0.1),
                net_conviction: dec!(0.4),
                competitive_score: dec!(0.7),
                falsifier: Some("alignment flips negative".into()),
                supporting_evidence: vec![],
                contradicting_evidence: vec![],
                references: vec![],
            }),
            runner_up_cause: None,
            cause_gap: None,
            leading_support_delta: None,
            leading_contradict_delta: None,
            leader_transition_summary: Some("leader remains market".into()),
            leading_falsifier: Some("alignment flips negative".into()),
            candidate_causes: vec![],
        }],
    });

    let recommendations = build_recommendations(&snapshot, None);
    let recommendation = &recommendations.items[0];

    assert!(recommendation.why.contains("偵測到1次冰山回補"));
    assert!(recommendation.why.contains("結構 strengthening (streak=3)"));
    assert!(recommendation.why.contains("主因: institutional flow dominates (連續4t領先)"));
    assert!(recommendation.why.contains("歷史先驗: Directed Flow"));
    assert_eq!(recommendation.invalidation_rule.as_deref(), Some("冰山回補停止"));
    assert_eq!(recommendation.primary_lens.as_deref(), Some("iceberg"));
    assert_eq!(recommendation.supporting_lenses, vec!["structural", "causal", "lineage_prior"]);
}

#[test]
fn recommendations_share_lens_engine_for_us_symbols_without_backward_data() {
    let mut state = symbol_state("AAPL", "Technology", dec!(0.09));
    state.structure = Some(AgentStructureState {
        symbol: "AAPL".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:AAPL:review".into()),
        title: "Long AAPL".into(),
        action: "review".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(6),
        status_streak: Some(2),
        confidence: dec!(0.74),
        confidence_change: Some(dec!(0.02)),
        confidence_gap: Some(dec!(0.10)),
        transition_reason: Some("status improving".into()),
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("US Growth".into()),
        action_expectancies: AgentActionExpectancies {
            follow_expectancy: Some(dec!(0.009)),
            fade_expectancy: Some(dec!(-0.002)),
            wait_expectancy: Some(Decimal::ZERO),
        },
        expected_net_alpha: Some(dec!(0.009)),
        alpha_horizon: Some("intraday:12t".into()),
        invalidation_rule: Some("growth tape softens".into()),
    });

    let mut snapshot = base_snapshot(vec![state], "risk_on");
    snapshot.market = LiveMarket::Us;
    snapshot.wake.focus_symbols = vec!["AAPL".into()];

    let recommendations = build_recommendations(&snapshot, None);
    let recommendation = &recommendations.items[0];

    assert!(recommendation.why.contains("結構 strengthening (streak=2)"));
    assert!(recommendation.why.contains("歷史先驗: US Growth"));
    assert!(!recommendation.why.contains("主因:"));
    assert_eq!(
        recommendation.invalidation_rule.as_deref(),
        Some("growth tape softens")
    );
    assert_eq!(recommendation.primary_lens.as_deref(), Some("structural"));
    assert_eq!(recommendation.supporting_lenses, vec!["lineage_prior"]);
}

#[test]
fn recommendations_preserve_fallback_why_when_lenses_emit_nothing() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.05));
    state.active_position = Some(ActionNode {
        workflow_id: "wf:700.HK".into(),
        symbol: Symbol("700.HK".into()),
        market: Market::Hk,
        sector: Some("Technology".into()),
        stage: ActionNodeStage::Monitoring,
        direction: ActionDirection::Long,
        entry_confidence: dec!(0.05),
        current_confidence: dec!(0.05),
        entry_price: Some(dec!(100)),
        pnl: None,
        age_ticks: 3,
        degradation_score: None,
        exit_forming: false,
    });
    let recommendations = build_recommendations(&base_snapshot(vec![state], "neutral"), None);
    let recommendation = &recommendations.items[0];

    assert_eq!(
        recommendation.why,
        "regime=neutral breadth_up=4% breadth_down=10%"
    );
    assert_eq!(recommendation.invalidation_rule, None);
}

#[test]
fn market_recommendation_detects_index_level_impulse() {
    let snapshot = trending_snapshot();
    let recommendations = build_recommendations(&snapshot, None);
    let market = recommendations
        .market_recommendation
        .as_ref()
        .expect("market recommendation should exist");
    assert_eq!(market.best_action, "follow");
    assert_eq!(market.preferred_expression, "index");
    assert_eq!(market.edge_layer, "market");
    assert_eq!(market.bias, "long");
    assert!(market.expected_net_alpha.is_some());
    assert_eq!(recommendations.total, recommendations.decisions.len());
}

#[test]
fn macro_event_candidates_route_global_headline_to_market_scope() {
    let snapshot = trending_snapshot();
    let notice = AgentNotice {
        notice_id: "event:10:macro".into(),
        tick: snapshot.tick,
        kind: "market_event".into(),
        symbol: None,
        sector: None,
        title: "Trump says Iran talks progressing".into(),
        summary: "Trump says Iran talks are progressing; oil drops and global equities rally"
            .into(),
        significance: dec!(0.9),
    };
    let wake = AgentWakeState {
        should_speak: true,
        priority: dec!(0.9),
        headline: Some(notice.title.clone()),
        summary: vec![notice.summary.clone()],
        focus_symbols: vec![],
        reasons: vec![notice.summary.clone()],
        suggested_tools: vec![],
    };

    let candidates = build_macro_event_candidates(
        snapshot.tick,
        snapshot.market,
        &snapshot.market_regime,
        &snapshot.stress,
        &wake,
        &[notice],
        &snapshot.sector_flows,
        &snapshot.symbols,
        &snapshot.cross_market_signals,
    );
    assert!(!candidates.is_empty());
    assert_eq!(candidates[0].impact.primary_scope, "market");
    assert!(candidates[0]
        .impact
        .affected_markets
        .iter()
        .any(|item| item == "US Equities"));

    let promoted = promote_macro_events(&snapshot.market_regime, &snapshot.stress, &candidates);
    assert!(!promoted.is_empty());
    assert_eq!(promoted[0].impact.primary_scope, "market");
}

#[test]
fn watchlist_surfaces_market_and_sector_decisions() {
    let snapshot = trending_snapshot();
    let recommendations = build_recommendations(&snapshot, None);
    let watchlist = build_watchlist(&snapshot, None, Some(&recommendations), 4);

    assert!(!watchlist.entries.is_empty());
    assert_eq!(watchlist.entries[0].scope_kind, "market");
    assert_eq!(watchlist.entries[0].symbol, "HK Market");
    assert_eq!(
        watchlist
            .entries
            .iter()
            .filter(|entry| entry.scope_kind == "sector")
            .count(),
        2
    );
}

#[test]
fn recommendations_emit_macro_event_decision_links() {
    let mut snapshot = trending_snapshot();
    snapshot.macro_events = vec![AgentMacroEvent {
        event_id: "macro_event:1".into(),
        tick: snapshot.tick,
        market: snapshot.market,
        event_type: "geopolitical_policy".into(),
        authority_level: "official".into(),
        headline: "Trump says Iran talks progressing".into(),
        summary: "Global risk repricing through lower oil and broader equities".into(),
        confidence: dec!(0.85),
        confirmation_state: "market_confirmed".into(),
        impact: AgentEventImpact {
            primary_scope: "market".into(),
            secondary_scopes: vec!["sector".into(), "symbol".into()],
            affected_markets: vec!["HK Equities".into(), "US Equities".into()],
            affected_sectors: vec!["Finance".into()],
            affected_symbols: vec!["5.HK".into()],
            preferred_expression: "index".into(),
            requires_market_confirmation: true,
            decisive_factors: vec!["broad repricing".into()],
        },
        supporting_notice_ids: vec!["notice:1".into()],
        promotion_reasons: vec!["confirmed".into()],
    }];
    snapshot.knowledge_links = build_macro_event_knowledge_links(&snapshot.macro_events);

    let recommendations = build_recommendations(&snapshot, None);
    assert!(!recommendations.knowledge_links.is_empty());
    assert!(recommendations
        .knowledge_links
        .iter()
        .any(|link| matches!(link.relation, KnowledgeRelation::SupportsDecision)));
}

#[test]
fn recommendation_resolution_marks_wait_as_miss_when_follow_wins() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.08));
    state.signal.as_mut().unwrap().mark_price = Some(dec!(105));
    let snapshot = base_snapshot(vec![state], "risk_on");
    let recommendation = AgentRecommendation {
        recommendation_id: "rec:1:700.HK:review".into(),
        tick: 1,
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        title: Some("Long 700.HK".into()),
        action: "review".into(),
        action_label: None,
        bias: "long".into(),
        severity: "high".into(),
        confidence: dec!(0.8),
        score: dec!(0.8),
        horizon_ticks: 2,
        regime_bias: "risk_on".into(),
        status: Some("stable".into()),
        why: "test".into(),
        why_components: vec![],
        primary_lens: None,
        supporting_lenses: vec![],
        review_lens: None,
        watch_next: vec![],
        do_not: vec![],
        fragility: vec![],
        transition: None,
        thesis_family: Some("Directed Flow".into()),
        state_transition: None,
        best_action: "wait".into(),
        action_expectancies: action_expectancies(Some(dec!(0.03)), Some(dec!(-0.02))),
        decision_attribution: AgentDecisionAttribution::default(),
        expected_net_alpha: None,
        alpha_horizon: "intraday:10t".into(),
        price_at_decision: Some(dec!(100)),
        resolution: None,
        invalidation_rule: None,
        invalidation_components: vec![],
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance: ActionGovernanceContract::for_recommendation(
            ActionExecutionPolicy::ReviewRequired,
        ),
        governance_reason_code: crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview,
        governance_reason: "execution requires review before it can advance".into(),
    };

    let resolution = resolve_recommendation_outcome(&snapshot, &recommendation)
        .expect("resolution should exist");
    assert_eq!(resolution.status, "miss");
    assert_eq!(resolution.counterfactual_best_action, "follow");
    assert!(!resolution.best_action_was_correct);
    assert_eq!(resolution.wait_regret, dec!(0.05));
}

#[test]
fn recommendation_and_watchlist_keep_governance_metadata() {
    let mut recommendation = AgentRecommendation {
        recommendation_id: "rec:gov:700.HK".into(),
        tick: 1,
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        title: Some("Long 700.HK".into()),
        action: "enter".into(),
        action_label: Some("Enter".into()),
        bias: "long".into(),
        severity: "medium".into(),
        confidence: dec!(0.7),
        score: dec!(0.75),
        horizon_ticks: 8,
        regime_bias: "neutral".into(),
        status: Some("new".into()),
        why: "why".into(),
        why_components: vec![AgentLensComponent {
            lens_name: "iceberg".into(),
            confidence: dec!(0.7),
            content: "偵測到2次冰山回補".into(),
            tags: vec!["iceberg".into(), "broker".into()],
        }],
        primary_lens: Some("iceberg".into()),
        supporting_lenses: vec!["structural".into()],
        review_lens: None,
        watch_next: vec![],
        do_not: vec![],
        fragility: vec![],
        transition: None,
        thesis_family: Some("Flow".into()),
        state_transition: None,
        best_action: "follow".into(),
        action_expectancies: AgentActionExpectancies::default(),
        decision_attribution: AgentDecisionAttribution::default(),
        expected_net_alpha: Some(dec!(0.02)),
        alpha_horizon: "intraday:8t".into(),
        price_at_decision: None,
        resolution: None,
        invalidation_rule: Some("if spread collapses".into()),
        invalidation_components: vec![AgentLensComponent {
            lens_name: "structural".into(),
            confidence: dec!(0.7),
            content: "if spread collapses".into(),
            tags: vec!["structure".into()],
        }],
        execution_policy: ActionExecutionPolicy::AutoEligible,
        governance: ActionGovernanceContract::for_recommendation(
            ActionExecutionPolicy::AutoEligible,
        ),
        governance_reason_code: crate::action::workflow::ActionGovernanceReasonCode::AutoExecutionEligible,
        governance_reason:
            "explicit invalidation rule and positive expected alpha make this recommendation auto-execute eligible"
                .into(),
    };

    assert_eq!(
        recommendation.governance_contract().execution_policy,
        ActionExecutionPolicy::AutoEligible
    );

    let watchlist_entry = AgentWatchlistEntry {
        rank: 1,
        scope_kind: "symbol".into(),
        symbol: recommendation.symbol.clone(),
        sector: recommendation.sector.clone(),
        edge_layer: None,
        title: recommendation.title.clone(),
        action: recommendation.action.clone(),
        action_label: recommendation.action_label.clone(),
        bias: recommendation.bias.clone(),
        severity: recommendation.severity.clone(),
        score: recommendation.score,
        status: recommendation.status.clone(),
        why: recommendation.why.clone(),
        why_components: recommendation.why_components.clone(),
        primary_lens: recommendation.primary_lens.clone(),
        supporting_lenses: recommendation.supporting_lenses.clone(),
        review_lens: recommendation.review_lens.clone(),
        transition: recommendation.transition.clone(),
        watch_next: recommendation.watch_next.clone(),
        do_not: recommendation.do_not.clone(),
        recommendation_id: recommendation.recommendation_id.clone(),
        thesis_family: recommendation.thesis_family.clone(),
        state_transition: recommendation.state_transition.clone(),
        best_action: recommendation.best_action.clone(),
        action_expectancies: recommendation.action_expectancies.clone(),
        decision_attribution: recommendation.decision_attribution.clone(),
        expected_net_alpha: recommendation.expected_net_alpha,
        alpha_horizon: recommendation.alpha_horizon.clone(),
        preferred_expression: None,
        reference_symbols: vec![recommendation.symbol.clone()],
        invalidation_rule: recommendation.invalidation_rule.clone(),
        invalidation_components: recommendation.invalidation_components.clone(),
        execution_policy: Some(recommendation.execution_policy),
        governance: Some(recommendation.governance.clone()),
        governance_reason_code: Some(recommendation.governance_reason_code),
        governance_reason: Some(recommendation.governance_reason.clone()),
    };

    assert_eq!(
        watchlist_entry.execution_policy,
        Some(ActionExecutionPolicy::AutoEligible)
    );
    assert_eq!(
        watchlist_entry
            .governance
            .as_ref()
            .map(|item| item.auto_execute_eligible),
        Some(true)
    );
    assert!(watchlist_entry
        .governance_reason
        .as_deref()
        .unwrap_or("")
        .contains("auto-execute eligible"));
    assert_eq!(watchlist_entry.why_components.len(), 1);
    assert_eq!(watchlist_entry.invalidation_components.len(), 1);
    assert_eq!(watchlist_entry.primary_lens.as_deref(), Some("iceberg"));
    assert_eq!(watchlist_entry.supporting_lenses, vec!["structural"]);
    assert_eq!(watchlist_entry.review_lens, None);

    recommendation.execution_policy = ActionExecutionPolicy::ReviewRequired;
    recommendation.severity = "high".into();
    recommendation.governance =
        ActionGovernanceContract::for_recommendation(ActionExecutionPolicy::ReviewRequired);
    recommendation.governance_reason_code =
        crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview;
    assert_eq!(
        recommendation.governance_contract().review_required,
        true
    );
    recommendation.governance_reason = crate::agent::governance_reason_for_signal_action(
        recommendation.best_action.as_str(),
        recommendation.severity.as_str(),
        recommendation.invalidation_rule.as_deref(),
        recommendation.expected_net_alpha,
        recommendation.execution_policy,
    );
    recommendation.review_lens = recommendation.primary_lens.clone();
    assert!(recommendation.governance_reason.contains("review"));
    assert_eq!(recommendation.review_lens.as_deref(), Some("iceberg"));
}

#[test]
fn journal_update_backfills_recommendation_resolution() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.08));
    state.signal.as_mut().unwrap().mark_price = Some(dec!(105));
    let snapshot = base_snapshot(vec![state], "risk_on");
    let current = AgentRecommendationJournalRecord {
        tick: 3,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
        breadth_up: snapshot.market_regime.breadth_up,
        breadth_down: snapshot.market_regime.breadth_down,
        composite_stress: snapshot.stress.composite_stress,
        wake_headline: snapshot.wake.headline.clone(),
        focus_symbols: vec!["700.HK".into()],
        market_recommendation: None,
        decisions: vec![],
        items: vec![],
        knowledge_links: vec![],
    };
    let previous = AgentRecommendationJournalRecord {
        tick: 1,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        regime_bias: "risk_on".into(),
        breadth_up: dec!(0.7),
        breadth_down: dec!(0.1),
        composite_stress: Decimal::ZERO,
        wake_headline: None,
        focus_symbols: vec!["700.HK".into()],
        market_recommendation: None,
        decisions: vec![],
        items: vec![AgentRecommendation {
            recommendation_id: "rec:1:700.HK:review".into(),
            tick: 1,
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            title: Some("Long 700.HK".into()),
            action: "review".into(),
            action_label: None,
            bias: "long".into(),
            severity: "high".into(),
            confidence: dec!(0.8),
            score: dec!(0.8),
            horizon_ticks: 2,
            regime_bias: "risk_on".into(),
            status: Some("stable".into()),
            why: "test".into(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            watch_next: vec![],
            do_not: vec![],
            fragility: vec![],
            transition: None,
            thesis_family: None,
            state_transition: None,
            best_action: "wait".into(),
            action_expectancies: action_expectancies(Some(dec!(0.03)), Some(dec!(-0.02))),
            decision_attribution: AgentDecisionAttribution::default(),
            expected_net_alpha: None,
            alpha_horizon: "intraday:10t".into(),
            price_at_decision: Some(dec!(100)),
            resolution: None,
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance: ActionGovernanceContract::for_recommendation(
                ActionExecutionPolicy::ReviewRequired,
            ),
            governance_reason_code:
                crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview,
            governance_reason: "execution requires review before it can advance".into(),
        }],
        knowledge_links: vec![],
    };
    let existing = serde_json::to_string(&previous).unwrap() + "\n";
    let updated = update_recommendation_journal(&existing, &snapshot, &current);
    let rows = updated
        .lines()
        .filter_map(|line| serde_json::from_str::<AgentRecommendationJournalRecord>(line).ok())
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].items[0].resolution.as_ref().unwrap().status, "miss");
}

#[test]
fn alert_scoreboard_resolves_follow_through() {
    let mut state = symbol_state("700.HK", "Technology", dec!(0.08));
    state.signal.as_mut().unwrap().mark_price = Some(dec!(105));
    let snapshot = base_snapshot(vec![state], "risk_on");
    let previous = AgentAlertScoreboard {
        tick: 7,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        regime_bias: "risk_on".into(),
        total: 1,
        alerts: vec![AgentAlertRecord {
            alert_id: "alert:7:700.HK:transition:enter".into(),
            tick: 7,
            scope_kind: "symbol".into(),
            symbol: Some("700.HK".into()),
            sector: Some("Technology".into()),
            kind: "transition".into(),
            severity: "high".into(),
            why: "test".into(),
            suggested_action: "enter".into(),
            action_label: None,
            horizon_ticks: 2,
            regime_bias: "risk_on".into(),
            price_at_alert: Some(dec!(100)),
            reference_value_at_alert: Some(dec!(100)),
            reference_symbols: vec!["700.HK".into()],
            action_bias: "long".into(),
            recommendation_id: Some("rec:7:700.HK:enter".into()),
            resolution: None,
            outcome_after_n_ticks: None,
        }],
        stats: AgentAlertStats {
            total_alerts: 1,
            resolved_alerts: 0,
            hits: 0,
            misses: 0,
            flats: 0,
            hit_rate: Decimal::ZERO,
            mean_oriented_return: Decimal::ZERO,
            false_positive_rate: Decimal::ZERO,
        },
        by_kind: vec![],
        by_action: vec![],
        by_scope: vec![],
        by_regime: vec![],
        by_sector: vec![],
    };

    let scoreboard = build_alert_scoreboard(&snapshot, None, Some(&previous));

    assert_eq!(scoreboard.stats.resolved_alerts, 1);
    assert_eq!(scoreboard.stats.hits, 1);
    let resolved = scoreboard
        .alerts
        .iter()
        .find(|item| item.alert_id == "alert:7:700.HK:transition:enter")
        .and_then(|item| item.outcome_after_n_ticks.as_ref())
        .expect("resolved outcome");
    assert_eq!(resolved.status, "hit");
}

#[test]
fn eod_review_surfaces_effective_and_noisy_slices() {
    let snapshot = base_snapshot(vec![], "risk_off");
    let scoreboard = AgentAlertScoreboard {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: "risk_off".into(),
        total: 3,
        alerts: vec![
            AgentAlertRecord {
                alert_id: "a1".into(),
                tick: 1,
                scope_kind: "symbol".into(),
                symbol: Some("700.HK".into()),
                sector: Some("Technology".into()),
                kind: "transition".into(),
                severity: "high".into(),
                why: "hit".into(),
                suggested_action: "enter".into(),
                action_label: None,
                horizon_ticks: 2,
                regime_bias: "risk_off".into(),
                price_at_alert: Some(dec!(100)),
                reference_value_at_alert: Some(dec!(100)),
                reference_symbols: vec!["700.HK".into()],
                action_bias: "short".into(),
                recommendation_id: None,
                resolution: Some(AgentRecommendationResolution {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "hit".into(),
                    price_return: dec!(-0.03),
                    follow_realized_return: dec!(0.03),
                    fade_realized_return: dec!(-0.03),
                    wait_regret: dec!(0.03),
                    counterfactual_best_action: "follow".into(),
                    best_action_was_correct: true,
                }),
                outcome_after_n_ticks: Some(AgentAlertOutcome {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "hit".into(),
                    price_return: Some(dec!(-0.03)),
                    oriented_return: Some(dec!(0.03)),
                    follow_through: "follow-through +3.00%".into(),
                }),
            },
            AgentAlertRecord {
                alert_id: "a2".into(),
                tick: 1,
                scope_kind: "symbol".into(),
                symbol: Some("9988.HK".into()),
                sector: Some("Consumer".into()),
                kind: "transition".into(),
                severity: "high".into(),
                why: "miss".into(),
                suggested_action: "enter".into(),
                action_label: None,
                horizon_ticks: 2,
                regime_bias: "risk_off".into(),
                price_at_alert: Some(dec!(100)),
                reference_value_at_alert: Some(dec!(100)),
                reference_symbols: vec!["9988.HK".into()],
                action_bias: "short".into(),
                recommendation_id: None,
                resolution: Some(AgentRecommendationResolution {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "miss".into(),
                    price_return: dec!(0.02),
                    follow_realized_return: dec!(-0.02),
                    fade_realized_return: dec!(0.02),
                    wait_regret: dec!(0.02),
                    counterfactual_best_action: "fade".into(),
                    best_action_was_correct: false,
                }),
                outcome_after_n_ticks: Some(AgentAlertOutcome {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "miss".into(),
                    price_return: Some(dec!(0.02)),
                    oriented_return: Some(dec!(-0.02)),
                    follow_through: "reversed +2.00%".into(),
                }),
            },
            AgentAlertRecord {
                alert_id: "a3".into(),
                tick: 1,
                scope_kind: "symbol".into(),
                symbol: Some("1810.HK".into()),
                sector: Some("Technology".into()),
                kind: "invalidation".into(),
                severity: "critical".into(),
                why: "hedge".into(),
                suggested_action: "hedge".into(),
                action_label: None,
                horizon_ticks: 2,
                regime_bias: "risk_off".into(),
                price_at_alert: Some(dec!(100)),
                reference_value_at_alert: Some(dec!(100)),
                reference_symbols: vec!["1810.HK".into()],
                action_bias: "long".into(),
                recommendation_id: None,
                resolution: Some(AgentRecommendationResolution {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "hit".into(),
                    price_return: dec!(-0.01),
                    follow_realized_return: dec!(-0.01),
                    fade_realized_return: dec!(0.01),
                    wait_regret: dec!(0.01),
                    counterfactual_best_action: "fade".into(),
                    best_action_was_correct: true,
                }),
                outcome_after_n_ticks: Some(AgentAlertOutcome {
                    resolved_tick: 3,
                    ticks_elapsed: 2,
                    status: "hit".into(),
                    price_return: Some(dec!(-0.01)),
                    oriented_return: Some(dec!(0.01)),
                    follow_through: "follow-through -1.00%".into(),
                }),
            },
        ],
        stats: AgentAlertStats {
            total_alerts: 3,
            resolved_alerts: 3,
            hits: 2,
            misses: 1,
            flats: 0,
            hit_rate: dec!(0.6666666667),
            mean_oriented_return: dec!(0.0066666667),
            false_positive_rate: dec!(0.3333333333),
        },
        by_kind: vec![
            AgentAlertSliceStat {
                key: "invalidation".into(),
                total_alerts: 1,
                resolved_alerts: 1,
                hits: 1,
                misses: 0,
                flats: 0,
                hit_rate: dec!(1),
                mean_oriented_return: dec!(0.01),
                false_positive_rate: dec!(0),
            },
            AgentAlertSliceStat {
                key: "transition".into(),
                total_alerts: 2,
                resolved_alerts: 2,
                hits: 1,
                misses: 1,
                flats: 0,
                hit_rate: dec!(0.5),
                mean_oriented_return: dec!(0.005),
                false_positive_rate: dec!(0.5),
            },
        ],
        by_action: vec![],
        by_scope: vec![],
        by_regime: vec![],
        by_sector: vec![
            AgentAlertSliceStat {
                key: "Technology".into(),
                total_alerts: 2,
                resolved_alerts: 2,
                hits: 2,
                misses: 0,
                flats: 0,
                hit_rate: dec!(1),
                mean_oriented_return: dec!(0.02),
                false_positive_rate: dec!(0),
            },
            AgentAlertSliceStat {
                key: "Consumer".into(),
                total_alerts: 1,
                resolved_alerts: 1,
                hits: 0,
                misses: 1,
                flats: 0,
                hit_rate: dec!(0),
                mean_oriented_return: dec!(-0.02),
                false_positive_rate: dec!(1),
            },
        ],
    };

    let review = build_eod_review(&snapshot, &scoreboard);

    assert_eq!(review.effective_kinds[0].key, "invalidation");
    assert_eq!(review.noisy_kinds[0].key, "transition");
    assert_eq!(review.effective_sectors[0].key, "Technology");
    assert_eq!(review.top_hits.len(), 2);
    assert_eq!(review.top_misses.len(), 1);
}
