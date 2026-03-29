    use std::collections::HashMap;

    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use crate::ontology::Symbol;
    use crate::ontology::{
        DecisionLineage, ProvenanceMetadata, ProvenanceSource, ReasoningScope, TacticalSetup,
    };
    use crate::temporal::record::{SymbolSignals, TickRecord};

    fn make_signal(vwap: Decimal) -> SymbolSignals {
        SymbolSignals {
            mark_price: Some(vwap),
            composite: Decimal::ZERO,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: Some(vwap),
            convergence_score: None,
            composite_degradation: None,
            institution_retention: None,
            edge_stability: None,
            temporal_weight: None,
            microstructure_confirmation: None,
            component_spread: None,
            institutional_edge_age: None,
        }
    }

    #[test]
    fn lineage_stats_counts_top_patterns() {
        let mut history = TickHistory::new(10);
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: None,
            provenance,
            lineage: DecisionLineage {
                based_on: vec!["hyp:700.HK:flow".into()],
                blocked_by: vec!["market regime risk_off blocks long entries".into()],
                promoted_by: vec!["review -> enter".into()],
                falsified_by: vec!["local flow flips negative".into()],
            },
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: Decimal::ZERO,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            workflow_id: None,
            entry_rationale: String::new(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let mut signals = HashMap::<Symbol, SymbolSignals>::new();
        signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(100)));
        history.push(TickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        });
        let mut latest_signals = HashMap::<Symbol, SymbolSignals>::new();
        latest_signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(110)));
        history.push(TickRecord {
            tick_number: 2,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: latest_signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        });

        let stats = compute_lineage_stats(&history, 5);
        assert_eq!(
            stats.blocked_by[0].0,
            "market regime risk_off blocks long entries"
        );
        assert_eq!(stats.promoted_by[0].1, 1);
        assert_eq!(stats.promoted_outcomes[0].resolved, 1);
        assert!(stats.promoted_outcomes[0].mean_return > Decimal::ZERO);
        assert_eq!(stats.promoted_contexts[0].family, "Unknown");
        assert_eq!(stats.promoted_contexts[0].market_regime, "unknown");
    }

    #[test]
    fn family_context_outcomes_group_by_family_session_and_regime() {
        let mut history = TickHistory::new(10);
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: None,
            provenance,
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: Decimal::ZERO,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            workflow_id: None,
            entry_rationale: String::new(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let mut signals = HashMap::<Symbol, SymbolSignals>::new();
        signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(100)));
        history.push(TickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        });
        let mut latest_signals = HashMap::<Symbol, SymbolSignals>::new();
        latest_signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(108)));
        history.push(TickRecord {
            tick_number: 2,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: latest_signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        });

        let outcomes = compute_family_context_outcomes(&history, 5);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].family, "Unknown");
        assert_eq!(outcomes[0].session, "offhours");
        assert_eq!(outcomes[0].market_regime, "unknown");
        assert_eq!(outcomes[0].resolved, 1);
        assert!(outcomes[0].mean_return > Decimal::ZERO);
        assert!(outcomes[0].follow_expectancy > Decimal::ZERO);
        assert!(outcomes[0].fade_expectancy < Decimal::ZERO);
        // wait_expectancy = mfe * follow_through_rate; both > 0 when setup followed through
        assert!(outcomes[0].wait_expectancy >= Decimal::ZERO);
    }

    #[test]
    fn fade_return_uses_material_reversal_when_structure_fails() {
        let realized = fade_return(
            dec!(0.03),
            dec!(-0.05),
            dec!(0.002),
            dec!(0.003),
            true,
            false,
            false,
        );

        assert_eq!(realized, dec!(0.048));
    }

    #[test]
    fn lineage_stats_filter_keeps_matching_contexts() {
        let stats = LineageStats {
            promoted_contexts: vec![ContextualLineageOutcome {
                label: "review -> enter".into(),
                family: "Directed Flow".into(),
                session: "opening".into(),
                market_regime: "risk_on".into(),
                total: 1,
                resolved: 1,
                hits: 1,
                hit_rate: Decimal::ONE,
                mean_return: Decimal::ZERO,
                mean_net_return: Decimal::ZERO,
                mean_mfe: Decimal::ZERO,
                mean_mae: Decimal::ZERO,
                follow_through_rate: Decimal::ONE,
                invalidation_rate: Decimal::ZERO,
                structure_retention_rate: Decimal::ONE,
                mean_convergence_score: Decimal::ZERO,
                mean_external_delta: Decimal::ZERO,
                external_follow_through_rate: Decimal::ZERO,
                follow_expectancy: Decimal::ZERO,
                fade_expectancy: Decimal::ZERO,
                wait_expectancy: Decimal::ZERO,
            }],
            ..LineageStats::default()
        };

        let filtered = stats.filtered(&LineageFilters {
            label: Some("review".into()),
            bucket: None,
            family: Some("flow".into()),
            session: Some("opening".into()),
            market_regime: Some("risk".into()),
        });

        assert_eq!(filtered.promoted_contexts.len(), 1);
        assert!(filtered.promoted_outcomes.is_empty());
    }

    #[test]
    fn setup_direction_uses_entry_composite_for_scope_level_cases() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "700.HK tactical case".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: Decimal::ZERO,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            workflow_id: None,
            entry_rationale: "700.HK directed flow repricing".into(),
            risk_notes: vec![],
            policy_verdict: None,
        };

        assert_eq!(setup_direction(&setup, Some(dec!(-0.2))), -1);
        assert_eq!(setup_direction(&setup, Some(dec!(0.2))), 1);
    }
