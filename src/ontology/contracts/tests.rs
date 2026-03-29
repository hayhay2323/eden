    use rust_decimal::Decimal;

    use super::*;
    use crate::agent::{
        AgentActionExpectancies, AgentRecommendations, AgentSession, AgentStructureState,
        AgentSymbolState, AgentThread, AgentWakeState,
    };
    use crate::live_snapshot::LiveScorecard;
    use crate::ontology::AgentKnowledgeLink;
    use crate::ontology::world::{
        BackwardInvestigation, BackwardReasoningSnapshot, CausalContestState,
    };
    use crate::agent_llm::AgentNarration;
    use crate::live_snapshot::{LiveStressSnapshot, LiveMarketRegime};
    use crate::ontology::ReasoningScope;
    use time::OffsetDateTime;

    #[test]
    fn builds_operational_snapshot_from_projection_views() {
        let live_snapshot = LiveSnapshot {
            tick: 7,
            timestamp: "2026-03-29T15:00:00Z".into(),
            market: LiveMarket::Hk,
            stock_count: 1,
            edge_count: 1,
            hypothesis_count: 0,
            observation_count: 0,
            active_positions: 0,
            active_position_nodes: vec![],
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
            scorecard: LiveScorecard {
                total_signals: 0,
                resolved_signals: 0,
                hits: 0,
                misses: 0,
                hit_rate: Decimal::ZERO,
                mean_return: Decimal::ZERO,
            },
            tactical_cases: vec![crate::live_snapshot::LiveTacticalCase {
                setup_id: "setup:700.HK".into(),
                symbol: "700.HK".into(),
                title: "Long 700".into(),
                action: "enter".into(),
                confidence: Decimal::ZERO,
                confidence_gap: Decimal::ZERO,
                heuristic_edge: Decimal::ZERO,
                entry_rationale: "test".into(),
                family_label: Some("Flow".into()),
                counter_label: None,
            }],
            hypothesis_tracks: vec![],
            top_signals: vec![],
            convergence_scores: vec![],
            pressures: vec![],
            backward_chains: vec![],
            causal_leaders: vec![],
            events: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            structural_deltas: vec![],
            propagation_senses: vec![],
            lineage: vec![],
        };
        let snapshot = AgentSnapshot {
            tick: 7,
            timestamp: "2026-03-29T15:00:00Z".into(),
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
                headline: Some("headline".into()),
                summary: vec![],
                focus_symbols: vec!["700.HK".into()],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: None,
            backward_reasoning: None,
            notices: vec![],
            active_structures: vec![AgentStructureState {
                symbol: "700.HK".into(),
                sector: Some("Technology".into()),
                setup_id: Some("setup:700.HK".into()),
                title: "Long 700".into(),
                action: "enter".into(),
                status: None,
                age_ticks: None,
                status_streak: None,
                confidence: Decimal::ZERO,
                confidence_change: None,
                confidence_gap: Some(Decimal::ZERO),
                transition_reason: None,
                contest_state: None,
                current_leader: None,
                leader_streak: None,
                leader_transition_summary: None,
                thesis_family: Some("Flow".into()),
                action_expectancies: AgentActionExpectancies::default(),
                expected_net_alpha: Some(Decimal::ZERO),
                alpha_horizon: Some("intraday:10t".into()),
                invalidation_rule: Some("bid disappears".into()),
            }],
            recent_transitions: vec![],
            sector_flows: vec![],
            symbols: vec![AgentSymbolState {
                symbol: "700.HK".into(),
                sector: Some("Technology".into()),
                structure: None,
                signal: None,
                depth: None,
                brokers: None,
                invalidation: None,
                pressure: None,
                active_position: None,
                latest_events: vec![],
            }],
            events: vec![],
            cross_market_signals: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![],
            macro_events: vec![],
            knowledge_links: vec![AgentKnowledgeLink {
                link_id: "link:1".into(),
                relation: crate::ontology::KnowledgeRelation::ImpactsSymbol,
                source: crate::ontology::macro_event_knowledge_node_ref("macro:1", "headline"),
                target: crate::ontology::symbol_knowledge_node_ref("700.HK"),
                confidence: Decimal::ZERO,
                attributes: crate::ontology::KnowledgeLinkAttributes::Generic,
                rationale: None,
            }],
        };
        let session = AgentSession {
            tick: 7,
            timestamp: "2026-03-29T15:00:00Z".into(),
            market: LiveMarket::Hk,
            should_speak: false,
            active_thread_count: 1,
            focus_symbols: vec!["700.HK".into()],
            active_threads: vec![AgentThread {
                symbol: "700.HK".into(),
                sector: Some("Technology".into()),
                status: "active".into(),
                first_tick: 7,
                last_tick: 7,
                idle_ticks: 0,
                turns_observed: 1,
                priority: Decimal::ZERO,
                title: Some("700 thread".into()),
                headline: None,
                latest_summary: Some("watch".into()),
                last_transition: None,
                current_leader: None,
                invalidation_status: None,
                reasons: vec![],
            }],
            recent_turns: vec![],
        };
        let recommendations = AgentRecommendations {
            tick: 7,
            timestamp: "2026-03-29T15:00:00Z".into(),
            market: LiveMarket::Hk,
            regime_bias: "neutral".into(),
            total: 0,
            market_recommendation: None,
            decisions: vec![],
            items: vec![],
            knowledge_links: vec![],
        };
        let narration = AgentNarration {
            tick: 7,
            timestamp: "2026-03-29T15:00:00Z".into(),
            market: LiveMarket::Hk,
            should_alert: false,
            alert_level: "normal".into(),
            source: "local".into(),
            headline: None,
            message: None,
            bullets: vec![],
            focus_symbols: vec![],
            tags: vec![],
            primary_action: None,
            confidence_band: None,
            what_changed: vec![],
            why_it_matters: None,
            watch_next: vec![],
            what_not_to_do: vec![],
            fragility: vec![],
            recommendation_ids: vec![],
            market_summary_5m: Some("summary".into()),
            market_recommendation: None,
            dominant_lenses: vec![],
            action_cards: vec![],
        };

        let operational =
            build_operational_snapshot(
                &live_snapshot,
                &snapshot,
                &session,
                &recommendations,
                Some(&narration),
            )
            .expect("operational snapshot");

        assert_eq!(operational.market_session.id.0, "market_session:hk:7");
        assert_eq!(operational.symbols.len(), 1);
        assert_eq!(operational.cases.len(), 1);
        assert_eq!(operational.threads.len(), 1);
        assert_eq!(operational.workflows.len(), 1);
        assert_eq!(operational.market_session.market_summary.as_deref(), Some("summary"));
        assert_eq!(operational.market_session.focus_symbol_refs.len(), 1);
        assert_eq!(operational.market_session.relationships.focus_symbols.len(), 1);
        assert!(operational.market_session.navigation.self_ref.is_some());
        assert_eq!(
            operational.market_session.focus_symbol_refs[0].endpoint.as_str(),
            "/api/ontology/hk/symbols/700.HK"
        );
        assert_eq!(
            operational.symbols[0].graph_ref.endpoint.as_str(),
            "/api/ontology/hk/graph/node/symbol:700.hk"
        );
        assert!(operational.symbols[0].navigation.graph.is_some());
        assert_eq!(
            operational.cases[0].graph_ref.endpoint.as_str(),
            "/api/ontology/hk/graph/node/setup:700.HK"
        );
        assert_eq!(
            operational.cases[0].symbol_ref.endpoint.as_str(),
            "/api/ontology/hk/symbols/700.HK"
        );
        assert_eq!(
            operational.cases[0].relationships.symbol.endpoint.as_str(),
            "/api/ontology/hk/symbols/700.HK"
        );
        assert!(operational.cases[0].navigation.neighborhood_endpoint.is_some());
        assert_eq!(
            operational.cases[0]
                .history_refs
                .reasoning
                .as_ref()
                .map(|item| item.endpoint.as_str()),
            Some("/api/ontology/hk/cases/setup:700.HK/history/reasoning")
        );
        assert_eq!(
            operational.cases[0]
                .history_refs
                .outcomes
                .as_ref()
                .map(|item| item.endpoint.as_str()),
            Some("/api/ontology/hk/cases/setup:700.HK/history/outcomes")
        );
        assert_eq!(
            operational.workflows[0]
                .history_refs
                .events
                .as_ref()
                .map(|item| item.endpoint.as_str()),
            Some("/api/ontology/hk/workflows/workflow:setup:700.HK/history")
        );
        assert_eq!(operational.workflows[0].case_refs.len(), 1);
        assert_eq!(operational.workflows[0].relationships.cases.len(), 1);
        assert!(operational.workflows[0].navigation.self_ref.is_some());
        let neighborhood = operational
            .neighborhood(OperationalObjectKind::Case, "setup:700.HK")
            .expect("case neighborhood");
        assert_eq!(neighborhood.relationships.len(), 3);
        assert_eq!(neighborhood.relationships[0].name, "symbol");
    }

    #[test]
    fn operational_snapshot_exposes_sidecar_views() {
        let live_snapshot = LiveSnapshot {
            tick: 9,
            timestamp: "2026-03-29T16:00:00Z".into(),
            market: LiveMarket::Hk,
            stock_count: 1,
            edge_count: 0,
            hypothesis_count: 0,
            observation_count: 0,
            active_positions: 0,
            active_position_nodes: vec![],
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
            scorecard: LiveScorecard {
                total_signals: 0,
                resolved_signals: 0,
                hits: 0,
                misses: 0,
                hit_rate: Decimal::ZERO,
                mean_return: Decimal::ZERO,
            },
            tactical_cases: vec![],
            hypothesis_tracks: vec![],
            top_signals: vec![],
            convergence_scores: vec![],
            pressures: vec![],
            backward_chains: vec![],
            causal_leaders: vec![],
            events: vec![],
            cross_market_signals: vec![],
            cross_market_anomalies: vec![],
            structural_deltas: vec![],
            propagation_senses: vec![],
            lineage: vec![],
        };
        let snapshot = AgentSnapshot {
            tick: 9,
            timestamp: "2026-03-29T16:00:00Z".into(),
            market: LiveMarket::Hk,
            market_regime: live_snapshot.market_regime.clone(),
            stress: live_snapshot.stress.clone(),
            wake: AgentWakeState {
                should_speak: false,
                priority: Decimal::ZERO,
                headline: None,
                summary: vec![],
                focus_symbols: vec!["700.HK".into()],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: Some(WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            }),
            backward_reasoning: Some(BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![BackwardInvestigation {
                    investigation_id: "backward:700.HK".into(),
                    leaf_scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
                    leaf_label: "700.HK".into(),
                    leaf_regime: "flow-led".into(),
                    contest_state: CausalContestState::Stable,
                    leading_cause_streak: 2,
                    previous_leading_cause_id: None,
                    leading_cause: None,
                    runner_up_cause: None,
                    cause_gap: None,
                    leading_support_delta: None,
                    leading_contradict_delta: None,
                    leader_transition_summary: None,
                    leading_falsifier: Some("flow disappears".into()),
                    candidate_causes: vec![],
                }],
            }),
            notices: vec![],
            active_structures: vec![],
            recent_transitions: vec![],
            sector_flows: vec![crate::agent::AgentSectorFlow {
                sector: "Technology".into(),
                member_count: 3,
                average_composite: Decimal::ZERO,
                average_capital_flow: Decimal::ZERO,
                leaders: vec!["700.HK".into()],
                exceptions: vec![],
                summary: "technology leadership".into(),
            }],
            symbols: vec![AgentSymbolState {
                symbol: "700.HK".into(),
                sector: Some("Technology".into()),
                structure: None,
                signal: None,
                depth: None,
                brokers: None,
                invalidation: None,
                pressure: None,
                active_position: None,
                latest_events: vec![],
            }],
            events: vec![],
            cross_market_signals: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![crate::ontology::AgentMacroEventCandidate {
                candidate_id: "candidate:1".into(),
                tick: 9,
                market: LiveMarket::Hk,
                source_kind: "news".into(),
                source_name: "wire".into(),
                event_type: "policy".into(),
                authority_level: "rumor".into(),
                headline: "Policy rumor".into(),
                summary: "Policy rumor".into(),
                confidence: Decimal::ZERO,
                novelty_score: Decimal::ZERO,
                jurisdictions: vec![],
                entities: vec![],
                impact: crate::ontology::AgentEventImpact {
                    primary_scope: "market".into(),
                    secondary_scopes: vec![],
                    affected_markets: vec!["hk".into()],
                    affected_sectors: vec![],
                    affected_symbols: vec!["700.HK".into()],
                    preferred_expression: "index".into(),
                    requires_market_confirmation: true,
                    decisive_factors: vec![],
                },
            }],
            macro_events: vec![],
            knowledge_links: vec![crate::ontology::AgentKnowledgeLink {
                link_id: "link:1".into(),
                relation: crate::ontology::KnowledgeRelation::ImpactsSymbol,
                source: crate::ontology::macro_event_knowledge_node_ref("macro:1", "headline"),
                target: crate::ontology::symbol_knowledge_node_ref("700.HK"),
                confidence: Decimal::ZERO,
                attributes: crate::ontology::KnowledgeLinkAttributes::Generic,
                rationale: None,
            }],
        };
        let session = AgentSession {
            tick: 9,
            timestamp: "2026-03-29T16:00:00Z".into(),
            market: LiveMarket::Hk,
            should_speak: false,
            active_thread_count: 0,
            focus_symbols: vec!["700.HK".into()],
            active_threads: vec![],
            recent_turns: vec![],
        };
        let recommendations = AgentRecommendations {
            tick: 9,
            timestamp: "2026-03-29T16:00:00Z".into(),
            market: LiveMarket::Hk,
            regime_bias: "neutral".into(),
            total: 0,
            market_recommendation: None,
            decisions: vec![],
            items: vec![],
            knowledge_links: vec![],
        };

        let operational =
            build_operational_snapshot(&live_snapshot, &snapshot, &session, &recommendations, None)
                .expect("operational snapshot");

        assert!(operational.sector_flow("technology").is_some());
        assert!(operational.backward_investigation("700.hk").is_some());
        assert!(operational.world_state().is_some());
        assert_eq!(operational.sidecars.macro_event_candidates.len(), 1);
        assert_eq!(operational.sidecars.knowledge_links.len(), 1);
        assert_eq!(
            operational.sidecars.knowledge_links[0].link_id.as_str(),
            "link:1"
        );
    }
