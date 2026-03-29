    use std::collections::{HashMap, HashSet};

    use petgraph::graph::DiGraph;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use super::propagation::canonicalize_paths;
    use super::support::summarize_evidence_weights;
    use crate::action::narrative::Regime;
    use crate::graph::decision::{
        ConvergenceScore, MarketRegimeBias, MarketRegimeFilter, OrderDirection, OrderSuggestion,
    };
    use crate::graph::graph::{
        BrainGraph, EdgeKind, InstitutionNode, InstitutionToStock, NodeKind, SectorNode, StockNode,
        StockToSector, StockToStock,
    };
    use crate::graph::insights::{
        GraphInsights, MarketStressIndex, RotationPair, SharedHolderAnomaly,
    };
    use crate::ontology::domain::{DerivedSignal, Event, ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
    use crate::ontology::reasoning::{
        DecisionLineage, EvidencePolarity, HypothesisTrackStatus, PropagationStep,
        ReasoningEvidence, ReasoningEvidenceKind, ReasoningScope,
    };
    use crate::pipeline::dimensions::SymbolDimensions;
    use crate::pipeline::signals::{
        DerivedSignalKind, DerivedSignalRecord, EventSnapshot, MarketEventRecord,
        MarketEventKind, SignalScope,
    };

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn prov(trace_id: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(trace_id)
            .with_inputs([trace_id.to_string()])
    }

    fn make_brain_for_diffusion() -> BrainGraph {
        let timestamp = OffsetDateTime::UNIX_EPOCH;
        let mut graph = DiGraph::new();

        let stock_a = graph.add_node(NodeKind::Stock(StockNode {
            symbol: sym("700.HK"),
            regime: Regime::CoherentBullish,
            coherence: dec!(0.6),
            mean_direction: dec!(0.5),
            dimensions: SymbolDimensions::default(),
        }));
        let stock_b = graph.add_node(NodeKind::Stock(StockNode {
            symbol: sym("9988.HK"),
            regime: Regime::CoherentNeutral,
            coherence: dec!(0.2),
            mean_direction: dec!(0.1),
            dimensions: SymbolDimensions::default(),
        }));
        let sector = graph.add_node(NodeKind::Sector(SectorNode {
            sector_id: SectorId("tech".into()),
            stock_count: 2,
            mean_coherence: dec!(0.4),
            mean_direction: dec!(0.3),
        }));
        let institution = graph.add_node(NodeKind::Institution(InstitutionNode {
            institution_id: InstitutionId(100),
            stock_count: 2,
            bid_stock_count: 2,
            ask_stock_count: 0,
            net_direction: dec!(1),
        }));

        graph.add_edge(
            stock_a,
            stock_b,
            EdgeKind::StockToStock(StockToStock {
                similarity: dec!(0.8),
                timestamp,
                provenance: prov("edge:700:9988"),
            }),
        );
        graph.add_edge(
            stock_b,
            stock_a,
            EdgeKind::StockToStock(StockToStock {
                similarity: dec!(0.8),
                timestamp,
                provenance: prov("edge:9988:700"),
            }),
        );
        graph.add_edge(
            stock_a,
            sector,
            EdgeKind::StockToSector(StockToSector {
                weight: dec!(0.6),
                timestamp,
                provenance: prov("edge:700:tech"),
            }),
        );
        graph.add_edge(
            stock_b,
            sector,
            EdgeKind::StockToSector(StockToSector {
                weight: dec!(0.4),
                timestamp,
                provenance: prov("edge:9988:tech"),
            }),
        );
        graph.add_edge(
            institution,
            stock_a,
            EdgeKind::InstitutionToStock(InstitutionToStock {
                direction: dec!(1),
                seat_count: 8,
                timestamp,
                provenance: prov("edge:i100:700"),
            }),
        );
        graph.add_edge(
            institution,
            stock_b,
            EdgeKind::InstitutionToStock(InstitutionToStock {
                direction: dec!(1),
                seat_count: 8,
                timestamp,
                provenance: prov("edge:i100:9988"),
            }),
        );

        BrainGraph {
            timestamp,
            graph,
            market_temperature: None,
            stock_nodes: HashMap::from([(sym("700.HK"), stock_a), (sym("9988.HK"), stock_b)]),
            institution_nodes: HashMap::from([(InstitutionId(100), institution)]),
            sector_nodes: HashMap::from([(SectorId("tech".into()), sector)]),
        }
    }

    #[test]
    fn reasoning_snapshot_builds_open_hypothesis_and_setup() {
        let event = Event::new(
            MarketEventRecord {
                scope: SignalScope::Symbol(sym("700.HK")),
                kind: MarketEventKind::InstitutionalFlip,
                magnitude: dec!(0.7),
                summary: "alignment flipped".into(),
            },
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
        );
        let events = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![event],
        };
        let signals = DerivedSignalSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: vec![DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(sym("700.HK")),
                    kind: DerivedSignalKind::Convergence,
                    strength: dec!(0.5),
                    summary: "convergence remains positive".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
            )],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: SectorId("energy".into()),
                to_sector: SectorId("shipping".into()),
                spread: dec!(0.4),
                spread_delta: dec!(0.1),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![SharedHolderAnomaly {
                symbol_a: sym("700.HK"),
                symbol_b: sym("9988.HK"),
                sector_a: None,
                sector_b: None,
                jaccard: dec!(0.5),
                shared_institutions: 2,
            }],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.2),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.3),
                composite_stress: dec!(0.2),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.6),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.4),
                    edge_stability: None,
                    institutional_edge_age: None,
                    new_edge_fraction: None,
                    microstructure_confirmation: None,
                    component_spread: None,
                    temporal_weight: None,
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![OrderSuggestion {
                symbol: sym("700.HK"),
                direction: OrderDirection::Buy,
                convergence: ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.6),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.4),
                    edge_stability: None,
                    institutional_edge_age: None,
                    new_edge_fraction: None,
                    microstructure_confirmation: None,
                    component_spread: None,
                    temporal_weight: None,
                },
                suggested_quantity: 100,
                price_low: Some(dec!(500)),
                price_high: Some(dec!(501)),
                estimated_cost: dec!(0.002),
                heuristic_edge: dec!(0.398),
                requires_confirmation: false,
                convergence_score: dec!(0.4),
                effective_confidence: dec!(0.4),
                external_confirmation: None,
                external_conflict: None,
                external_support_slug: None,
                external_support_probability: None,
                external_conflict_slug: None,
                external_conflict_probability: None,
            }],
            degradations: HashMap::new(),
        };

        let reasoning =
            ReasoningSnapshot::derive(&events, &signals, &insights, &decision, &[], &[]);
        assert!(reasoning.hypotheses.len() >= 3);
        assert!(!reasoning.tactical_setups.is_empty());
        assert!(!reasoning.propagation_paths.is_empty());
        assert!(!reasoning.hypothesis_tracks.is_empty());
        assert!(!reasoning.case_clusters.is_empty());
        assert!(reasoning
            .hypotheses
            .iter()
            .any(|hypothesis| hypothesis.statement.contains("directed flow repricing")));
        let mut ranked = reasoning
            .hypotheses
            .iter()
            .filter(|hypothesis| hypothesis.scope == ReasoningScope::Symbol(sym("700.HK")))
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
        });
        assert_eq!(
            reasoning.tactical_setups[0].hypothesis_id,
            ranked[0].hypothesis_id
        );
        assert!(reasoning
            .hypotheses
            .iter()
            .any(|hypothesis| hypothesis.local_support_weight > Decimal::ZERO));
    }

    #[test]
    fn hypothesis_tracks_capture_strengthening_and_invalidation() {
        let previous_timestamp = OffsetDateTime::UNIX_EPOCH;
        let current_timestamp = previous_timestamp + time::Duration::seconds(2);
        let previous_setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.58),
            confidence_gap: dec!(0.09),
            heuristic_edge: dec!(0.05),
            convergence_score: Some(dec!(0.40)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
            policy_verdict: None,
        };
        let previous_track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: previous_setup.setup_id.clone(),
            hypothesis_id: previous_setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: previous_setup.runner_up_hypothesis_id.clone(),
            scope: previous_setup.scope.clone(),
            title: previous_setup.title.clone(),
            action: previous_setup.action.clone(),
            status: HypothesisTrackStatus::New,
            age_ticks: 1,
            status_streak: 1,
            confidence: previous_setup.confidence,
            previous_confidence: None,
            confidence_change: Decimal::ZERO,
            confidence_gap: previous_setup.confidence_gap,
            previous_confidence_gap: None,
            confidence_gap_change: Decimal::ZERO,
            heuristic_edge: previous_setup.heuristic_edge,
            policy_reason: "new case seeded".into(),
            transition_reason: None,
            first_seen_at: previous_timestamp,
            last_updated_at: previous_timestamp,
            invalidated_at: None,
        };
        let current_setup = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.12),
            action: "enter".into(),
            ..previous_setup.clone()
        };

        let tracks = derive_hypothesis_tracks(
            current_timestamp,
            &[current_setup.clone()],
            &[previous_setup.clone()],
            &[previous_track.clone()],
        );
        let strengthening = tracks
            .iter()
            .find(|track| track.track_id == "track:700.HK")
            .expect("current track");
        assert_eq!(strengthening.status, HypothesisTrackStatus::Strengthening);
        assert_eq!(strengthening.age_ticks, 2);
        assert_eq!(strengthening.status_streak, 1);
        assert_eq!(strengthening.previous_confidence, Some(dec!(0.58)));

        let invalidated =
            derive_hypothesis_tracks(current_timestamp, &[], &[previous_setup], &[previous_track]);
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0].status, HypothesisTrackStatus::Invalidated);
        assert_eq!(invalidated[0].invalidated_at, Some(current_timestamp));
    }

    #[test]
    fn track_policy_promotes_after_strengthening_streak() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.64),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.11),
            convergence_score: Some(dec!(0.45)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
            policy_verdict: None,
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.04),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let previous_track = HypothesisTrack {
            action: "review".into(),
            ..track.clone()
        };

        let updated = apply_track_action_policy(
            &[setup],
            &[track],
            &[previous_track],
            OffsetDateTime::UNIX_EPOCH,
            &MarketRegimeFilter::neutral(),
            &[],
        );
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].action, "enter");
        assert!(updated[0]
            .entry_rationale
            .contains("promoted by strengthening streak"));
    }

    #[test]
    fn track_policy_blocks_low_edge_enter() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.64),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.003),
            convergence_score: Some(dec!(0.45)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
            policy_verdict: None,
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.04),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let updated = apply_track_action_policy(
            &[setup],
            &[track],
            &[],
            OffsetDateTime::UNIX_EPOCH,
            &MarketRegimeFilter::neutral(),
            &[],
        );
        assert_eq!(updated[0].action, "review");
    }

    #[test]
    fn track_policy_blocks_long_enter_when_market_regime_is_risk_off() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.12),
            convergence_score: Some(dec!(0.52)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
            policy_verdict: None,
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.06),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.07),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let market_regime = MarketRegimeFilter {
            bias: MarketRegimeBias::RiskOff,
            confidence: dec!(0.82),
            breadth_up: dec!(0.12),
            breadth_down: dec!(0.78),
            average_return: dec!(-0.023),
            leader_return: Some(dec!(-0.041)),
            directional_consensus: dec!(-0.48),
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        };

        let updated = apply_track_action_policy(
            &[setup],
            &[track],
            &[],
            OffsetDateTime::UNIX_EPOCH,
            &market_regime,
            &[],
        );
        assert_eq!(updated[0].action, "review");
        assert!(updated[0]
            .risk_notes
            .iter()
            .any(|note| note.contains("market regime risk_off blocks long entries")));
    }

    #[test]
    fn case_clusters_group_related_members() {
        let hypothesis_a = Hypothesis {
            hypothesis_id: "hyp:700.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:700.HK:flow"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.68),
            local_support_weight: Decimal::ZERO,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:shared_holder:700.HK:9988.HK".into()],
            expected_observations: vec![],
        };
        let hypothesis_b = Hypothesis {
            hypothesis_id: "hyp:9988.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:9988.HK:flow"),
            scope: ReasoningScope::Symbol(sym("9988.HK")),
            statement: "9988.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.62),
            local_support_weight: Decimal::ZERO,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:shared_holder:700.HK:9988.HK".into()],
            expected_observations: vec![],
        };
        let path = PropagationPath {
            path_id: "path:shared_holder:700.HK:9988.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK"
                .into(),
            confidence: dec!(0.5),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("700.HK")),
                to: ReasoningScope::Symbol(sym("9988.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.5),
                references: vec![],
            }],
        };
        let setup_a = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: hypothesis_a.hypothesis_id.clone(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:enter"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.68),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.12),
            convergence_score: Some(dec!(0.61)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "strong case".into(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let setup_b = TacticalSetup {
            setup_id: "setup:9988.HK:review".into(),
            hypothesis_id: hypothesis_b.hypothesis_id.clone(),
            runner_up_hypothesis_id: Some("hyp:9988.HK:risk".into()),
            provenance: prov("setup:9988.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("9988.HK")),
            title: "Long 9988.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.62),
            confidence_gap: dec!(0.12),
            heuristic_edge: dec!(0.07),
            convergence_score: Some(dec!(0.48)),
            workflow_id: Some("order:9988.HK:buy".into()),
            entry_rationale: "secondary case".into(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let track_a = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup_a.setup_id.clone(),
            hypothesis_id: setup_a.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup_a.runner_up_hypothesis_id.clone(),
            scope: setup_a.scope.clone(),
            title: setup_a.title.clone(),
            action: setup_a.action.clone(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup_a.confidence,
            previous_confidence: Some(dec!(0.61)),
            confidence_change: dec!(0.07),
            confidence_gap: setup_a.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup_a.heuristic_edge,
            policy_reason: "strengthening".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let track_b = HypothesisTrack {
            track_id: "track:9988.HK".into(),
            setup_id: setup_b.setup_id.clone(),
            hypothesis_id: setup_b.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup_b.runner_up_hypothesis_id.clone(),
            scope: setup_b.scope.clone(),
            title: setup_b.title.clone(),
            action: setup_b.action.clone(),
            status: HypothesisTrackStatus::Stable,
            age_ticks: 2,
            status_streak: 1,
            confidence: setup_b.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.02),
            confidence_gap: setup_b.confidence_gap,
            previous_confidence_gap: Some(dec!(0.10)),
            confidence_gap_change: dec!(0.02),
            heuristic_edge: setup_b.heuristic_edge,
            policy_reason: "stable".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let clusters = derive_case_clusters(
            &[hypothesis_a, hypothesis_b],
            &[path],
            &[setup_a, setup_b],
            &[track_a, track_b],
        );

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].member_count, 2);
        assert_eq!(clusters[0].trend, HypothesisTrackStatus::Strengthening);
        assert_eq!(clusters[0].strongest_title, "Long 700.HK");
    }

    #[test]
    fn derive_propagation_paths_builds_two_hop_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![
                RotationPair {
                    from_sector: SectorId("energy".into()),
                    to_sector: SectorId("shipping".into()),
                    spread: dec!(0.6),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("shipping".into()),
                    to_sector: SectorId("ports".into()),
                    spread: dec!(0.5),
                    spread_delta: dec!(0.1),
                    widening: true,
                },
            ],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let two_hop = paths
            .iter()
            .find(|path| path.steps.len() == 2)
            .expect("two-hop path");

        assert_eq!(
            two_hop.steps[0].from,
            ReasoningScope::Sector("energy".into())
        );
        assert_eq!(two_hop.steps[1].to, ReasoningScope::Sector("ports".into()));
        assert!(two_hop.path_id.contains("path:2hop:"));
        assert!(two_hop.summary.contains("via"));
    }

    #[test]
    fn derive_propagation_paths_builds_three_hop_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![
                RotationPair {
                    from_sector: SectorId("energy".into()),
                    to_sector: SectorId("shipping".into()),
                    spread: dec!(0.8),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("shipping".into()),
                    to_sector: SectorId("ports".into()),
                    spread: dec!(0.7),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("ports".into()),
                    to_sector: SectorId("logistics".into()),
                    spread: dec!(0.6),
                    spread_delta: dec!(0.1),
                    widening: true,
                },
            ],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let three_hop = paths
            .iter()
            .find(|path| path.steps.len() == 3)
            .expect("three-hop path");

        assert_eq!(
            three_hop.steps[0].from,
            ReasoningScope::Sector("energy".into())
        );
        assert_eq!(
            three_hop.steps[2].to,
            ReasoningScope::Sector("logistics".into())
        );
        assert!(three_hop.path_id.contains("path:3hop:"));
    }

    #[test]
    fn canonicalize_paths_dedupes_symmetric_shared_holder_paths() {
        let path_ab = PropagationPath {
            path_id: "path:shared_holder:700.HK:9988.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK"
                .into(),
            confidence: dec!(0.8),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("700.HK")),
                to: ReasoningScope::Symbol(sym("9988.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.8),
                references: vec![],
            }],
        };
        let path_ba = PropagationPath {
            path_id: "path:shared_holder:9988.HK:700.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 9988.HK and 700.HK"
                .into(),
            confidence: dec!(0.8),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("9988.HK")),
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.8),
                references: vec![],
            }],
        };

        let canonical = canonicalize_paths(vec![path_ab, path_ba]);
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn derive_propagation_paths_builds_mixed_mechanism_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: SectorId("energy".into()),
                to_sector: SectorId("shipping".into()),
                spread: dec!(0.7),
                spread_delta: dec!(0.2),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![SharedHolderAnomaly {
                symbol_a: sym("883.HK"),
                symbol_b: sym("1308.HK"),
                sector_a: Some(SectorId("energy".into())),
                sector_b: Some(SectorId("shipping".into())),
                jaccard: dec!(0.8),
                shared_institutions: 4,
            }],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.4),
                pressure_consensus: dec!(0.4),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.6),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let mixed = paths
            .iter()
            .find(|path| {
                path.steps.len() == 2
                    && path_is_mixed_multi_hop(path)
                    && path
                        .steps
                        .iter()
                        .any(|step| mechanism_family(&step.mechanism) == "rotation")
            })
            .expect("mixed 2-hop path");

        let families = mixed
            .steps
            .iter()
            .map(|step| mechanism_family(&step.mechanism))
            .collect::<HashSet<_>>();
        assert!(families.contains("rotation"));
        assert!(families.contains("sector_symbol_bridge") || families.contains("shared_holder"));
    }

    #[test]
    fn cluster_title_uses_solo_case_for_single_member() {
        let title = cluster_title("propagation", "symbol:1177.HK", 1, None);
        assert!(title.contains("solo case"));
        assert!(!title.contains("cluster x1"));
    }

    #[test]
    fn propagated_path_evidence_penalizes_longer_hops() {
        let scope = ReasoningScope::Sector("shipping".into());
        let two_hop = PropagationPath {
            path_id: "path:2hop:test".into(),
            summary: "two hop".into(),
            confidence: dec!(0.70),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::market(),
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.8),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.7),
                    references: vec![],
                },
            ],
        };
        let three_hop = PropagationPath {
            path_id: "path:3hop:test".into(),
            summary: "three hop".into(),
            confidence: dec!(0.70),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::market(),
                    to: ReasoningScope::Sector("materials".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.8),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("materials".into()),
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.7),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
            ],
        };

        let evidence = vec![ReasoningEvidence {
            statement: "local support".into(),
            kind: ReasoningEvidenceKind::LocalEvent,
            polarity: EvidencePolarity::Supports,
            weight: dec!(0.4),
            references: vec![],
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
        }];

        let (weight, ids) = propagated_path_evidence(&scope, &evidence, &[three_hop, two_hop]);
        assert_eq!(ids[0], "path:2hop:test");
        assert!(weight > Decimal::ZERO);
    }

    #[test]
    fn propagated_path_evidence_rewards_local_confirmation() {
        let scope = ReasoningScope::Sector("shipping".into());
        let path = PropagationPath {
            path_id: "path:2hop:test".into(),
            summary: "two hop".into(),
            confidence: dec!(0.50),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::market(),
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.5),
                    references: vec![],
                },
            ],
        };
        let no_local = propagated_path_evidence(&scope, &[], std::slice::from_ref(&path)).0;
        let supporting_local = propagated_path_evidence(
            &scope,
            &[ReasoningEvidence {
                statement: "local support".into(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.6),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            }],
            &[path],
        )
        .0;

        assert!(supporting_local > no_local);
    }

    #[test]
    fn summarize_evidence_weights_splits_local_and_propagated() {
        let summary = summarize_evidence_weights(&[
            ReasoningEvidence {
                statement: "event support".into(),
                kind: ReasoningEvidenceKind::LocalEvent,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.4),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
            ReasoningEvidence {
                statement: "signal contradict".into(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Contradicts,
                weight: dec!(0.2),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
            ReasoningEvidence {
                statement: "path support".into(),
                kind: ReasoningEvidenceKind::PropagatedPath,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.3),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
        ]);

        assert_eq!(summary.local_support, dec!(0.4));
        assert_eq!(summary.local_contradict, dec!(0.2));
        assert_eq!(summary.propagated_support, dec!(0.3));
        assert_eq!(summary.propagated_contradict, Decimal::ZERO);
    }

    #[test]
    fn diffusion_paths_emerge_from_graph_and_stock_deltas() {
        let brain = make_brain_for_diffusion();
        let deltas = HashMap::from([(sym("700.HK"), dec!(0.12)), (sym("9988.HK"), dec!(0.01))]);

        let paths = derive_diffusion_propagation_paths(&brain, &deltas, OffsetDateTime::UNIX_EPOCH);

        assert!(paths.iter().any(|path| {
            path.steps.iter().any(|step| {
                step.from == ReasoningScope::Symbol(sym("700.HK"))
                    && step.to == ReasoningScope::Symbol(sym("9988.HK"))
                    && mechanism_family(&step.mechanism) == "stock_diffusion"
            })
        }));
        assert!(paths.iter().any(|path| {
            path.steps.iter().any(|step| mechanism_family(&step.mechanism) == "sector_diffusion")
        }));
        assert!(!paths.iter().any(|path| {
            path.steps.iter().any(|step| {
                step.from == ReasoningScope::Symbol(sym("9988.HK"))
                    && step.to == ReasoningScope::Symbol(sym("700.HK"))
            })
        }));
    }
