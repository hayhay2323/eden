    use std::collections::HashMap;

    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use super::backward::select_backward_investigation_targets;
    use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
    use crate::graph::decision::{ConvergenceScore, OrderDirection, OrderSuggestion};
    use crate::graph::insights::{GraphInsights, MarketStressIndex, RotationPair};
    use crate::ontology::reasoning::InvestigationSelection;
    use crate::ontology::{Hypothesis, HypothesisTrack, HypothesisTrackStatus, TacticalSetup};
    use crate::ontology::{ReasoningEvidence, ReasoningEvidenceKind, Symbol};
    use crate::pipeline::reasoning::ReasoningSnapshot;
    use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot};

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn prov(trace_id: &str) -> crate::ontology::ProvenanceMetadata {
        crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
        )
        .with_trace_id(trace_id)
        .with_inputs([trace_id.to_string()])
    }

    #[test]
    fn world_state_derives_market_and_leaf_entities() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:700.HK:flow"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.64),
            local_support_weight: dec!(0.4),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.2),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![ReasoningEvidence {
                statement: "local flow still leads".into(),
                kind: ReasoningEvidenceKind::LocalEvent,
                polarity: crate::ontology::EvidencePolarity::Supports,
                weight: dec!(0.4),
                references: vec![],
                provenance: crate::ontology::ProvenanceMetadata::new(
                    crate::ontology::ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            }],
            invalidation_conditions: vec![crate::ontology::InvalidationCondition {
                description: "local flow turns net negative".into(),
                references: vec!["flow:700.HK".into()],
            }],
            propagation_path_ids: vec![],
            expected_observations: vec!["flow should persist".into()],
        };
        let reasoning = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![hypothesis.clone()],
            propagation_paths: vec![],
            investigation_selections: vec![InvestigationSelection {
                investigation_id: "investigation:700.HK".into(),
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                provenance: prov("investigation:700.HK"),
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                family_key: hypothesis.family_key.clone(),
                family_label: hypothesis.family_label.clone(),
                confidence: dec!(0.64),
                confidence_gap: dec!(0.14),
                priority_score: dec!(0.23),
                attention_hint: "review".into(),
                rationale: hypothesis.statement.clone(),
                notes: vec![],
            }],
            tactical_setups: vec![TacticalSetup {
                setup_id: "setup:700.HK:review".into(),
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                provenance: prov("setup:700.HK:review"),
                lineage: crate::ontology::DecisionLineage::default(),
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                action: "review".into(),
                time_horizon: "intraday".into(),
                confidence: dec!(0.64),
                confidence_gap: dec!(0.14),
                heuristic_edge: dec!(0.03),
                convergence_score: Some(dec!(0.41)),
                workflow_id: Some("order:700.HK:buy".into()),
                entry_rationale: "flow leads".into(),
                risk_notes: vec![],
                policy_verdict: None,
            }],
            hypothesis_tracks: vec![HypothesisTrack {
                track_id: "track:700.HK".into(),
                setup_id: "setup:700.HK:review".into(),
                hypothesis_id: hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                action: "review".into(),
                status: HypothesisTrackStatus::Stable,
                age_ticks: 2,
                status_streak: 1,
                confidence: dec!(0.64),
                previous_confidence: Some(dec!(0.62)),
                confidence_change: dec!(0.02),
                confidence_gap: dec!(0.14),
                previous_confidence_gap: Some(dec!(0.12)),
                confidence_gap_change: dec!(0.02),
                heuristic_edge: dec!(0.03),
                policy_reason: "case remains stable".into(),
                transition_reason: None,
                first_seen_at: OffsetDateTime::UNIX_EPOCH,
                last_updated_at: OffsetDateTime::UNIX_EPOCH,
                invalidated_at: None,
            }],
            case_clusters: vec![],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: crate::ontology::SectorId("tech".into()),
                to_sector: crate::ontology::SectorId("finance".into()),
                spread: dec!(0.5),
                spread_delta: dec!(0.1),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
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

        let snapshots = WorldSnapshots::derive(
            &EventSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                events: vec![],
            },
            &DerivedSignalSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                signals: vec![],
            },
            &insights,
            &DecisionSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                convergence_scores: HashMap::from([(
                    sym("700.HK"),
                    ConvergenceScore {
                        symbol: sym("700.HK"),
                        institutional_alignment: dec!(0.4),
                        sector_coherence: Some(dec!(0.2)),
                        cross_stock_correlation: dec!(0.1),
                        composite: dec!(0.5),
                        edge_stability: None,
                        institutional_edge_age: None,
                        new_edge_fraction: None,
                        microstructure_confirmation: None,
                        component_spread: None,
                        temporal_weight: None,
                    },
                )]),
                market_regime: crate::graph::decision::MarketRegimeFilter::neutral(),
                order_suggestions: vec![OrderSuggestion {
                    symbol: sym("700.HK"),
                    direction: OrderDirection::Buy,
                    convergence: ConvergenceScore {
                        symbol: sym("700.HK"),
                        institutional_alignment: dec!(0.4),
                        sector_coherence: Some(dec!(0.2)),
                        cross_stock_correlation: dec!(0.1),
                        composite: dec!(0.5),
                        edge_stability: None,
                        institutional_edge_age: None,
                        new_edge_fraction: None,
                        microstructure_confirmation: None,
                        component_spread: None,
                        temporal_weight: None,
                    },
                    suggested_quantity: 100,
                    price_low: None,
                    price_high: None,
                    estimated_cost: dec!(0.01),
                    heuristic_edge: dec!(0.04),
                    requires_confirmation: false,
                    convergence_score: dec!(0.5),
                    effective_confidence: dec!(0.5),
                    external_confirmation: None,
                    external_conflict: None,
                    external_support_slug: None,
                    external_support_probability: None,
                    external_conflict_slug: None,
                    external_conflict_probability: None,
                }],
                degradations: HashMap::new(),
            },
            &reasoning,
            None,
            None,
        );

        assert!(!snapshots.world_state.entities.is_empty());
        assert!(snapshots
            .world_state
            .entities
            .iter()
            .any(|entity| entity.layer == WorldLayer::Forest));
        assert!(snapshots
            .backward_reasoning
            .investigations
            .iter()
            .any(|item| item.leaf_label == "Long 700.HK"));
        let investigation = snapshots
            .backward_reasoning
            .investigations
            .iter()
            .find(|item| item.leaf_label == "Long 700.HK")
            .expect("backward investigation");
        assert!(investigation.leading_cause.is_some());
        assert!(investigation.runner_up_cause.is_some());
        assert!(investigation.cause_gap.is_some());
        assert!(investigation.leading_falsifier.is_some());
        assert!(investigation
            .leading_cause
            .as_ref()
            .is_some_and(|cause| !cause.supporting_evidence.is_empty()));
        assert!(investigation
            .leading_cause
            .as_ref()
            .is_some_and(|cause| cause.support_weight >= cause.contradict_weight));
        assert!(investigation
            .candidate_causes
            .windows(2)
            .all(|pair| pair[0].competitive_score >= pair[1].competitive_score));
    }

    #[test]
    fn world_state_adds_polymarket_entities() {
        let reasoning = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![],
            propagation_paths: vec![],
            investigation_selections: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.2),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.2),
                composite_stress: dec!(0.2),
            },
            institution_stock_counts: HashMap::new(),
            edge_profiles: vec![],
        };
        let polymarket = PolymarketSnapshot {
            fetched_at: OffsetDateTime::UNIX_EPOCH,
            priors: vec![PolymarketPrior {
                slug: "fed-cut".into(),
                label: "Fed cut in September".into(),
                question: "Will the Fed cut in September?".into(),
                scope: ReasoningScope::market(),
                target_scopes: vec![],
                bias: PolymarketBias::RiskOn,
                selected_outcome: "Yes".into(),
                probability: dec!(0.72),
                conviction_threshold: dec!(0.60),
                active: true,
                closed: false,
                category: Some("Macro".into()),
                volume: None,
                liquidity: None,
                end_date: None,
            }],
        };

        let snapshots = WorldSnapshots::derive(
            &EventSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                events: vec![],
            },
            &DerivedSignalSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                signals: vec![],
            },
            &insights,
            &DecisionSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                convergence_scores: HashMap::new(),
                market_regime: crate::graph::decision::MarketRegimeFilter::neutral(),
                order_suggestions: vec![],
                degradations: HashMap::new(),
            },
            &reasoning,
            Some(&polymarket),
            None,
        );

        assert!(snapshots
            .world_state
            .entities
            .iter()
            .any(|entity| entity.entity_id == "world:polymarket:fed-cut"));
        assert!(snapshots
            .world_state
            .entities
            .iter()
            .find(|entity| entity.entity_id == "world:market")
            .is_some_and(|entity| entity.regime.contains("event-risk-on")));
    }

    #[test]
    fn backward_reasoning_demotes_leading_cause_when_contradiction_pressure_rises() {
        let base_provenance = crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
        );
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:contest".into(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:700.HK:review"),
            lineage: crate::ontology::DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.06),
            convergence_score: Some(dec!(0.44)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "contest case".into(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let market_path = crate::ontology::PropagationPath {
            path_id: "path:market_stress:tech".into(),
            summary: "market stress may propagate into 700.HK".into(),
            confidence: dec!(0.62),
            steps: vec![crate::ontology::PropagationStep {
                from: ReasoningScope::market(),
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "market stress concentration".into(),
                confidence: dec!(0.62),
                references: vec!["graph_stress".into()],
            }],
        };
        let sector_path = crate::ontology::PropagationPath {
            path_id: "path:sector_spill:tech:700.HK".into(),
            summary: "tech regime may propagate into 700.HK".into(),
            confidence: dec!(0.58),
            steps: vec![crate::ontology::PropagationStep {
                from: ReasoningScope::Sector("tech".into()),
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "sector_symbol_spillover".into(),
                confidence: dec!(0.58),
                references: vec!["sector:tech".into()],
            }],
        };
        let world_state = WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![
                EntityState {
                    entity_id: "world:market".into(),
                    scope: ReasoningScope::market(),
                    layer: WorldLayer::Forest,
                    provenance: prov("world:market"),
                    label: "Market canopy".into(),
                    regime: "stress-dominant".into(),
                    confidence: dec!(0.72),
                    local_support: dec!(0.10),
                    propagated_support: dec!(0.64),
                    drivers: vec!["market stress=0.72".into(), "clusters=2".into()],
                },
                EntityState {
                    entity_id: "world:sector:tech".into(),
                    scope: ReasoningScope::Sector("tech".into()),
                    layer: WorldLayer::Trunk,
                    provenance: prov("world:sector:tech"),
                    label: "Sector tech".into(),
                    regime: "tech bid still coherent".into(),
                    confidence: dec!(0.64),
                    local_support: dec!(0.18),
                    propagated_support: dec!(0.52),
                    drivers: vec!["tech leadership persistent".into()],
                },
            ],
        };

        let leading_market_hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:contest".into(),
            family_key: "contest".into(),
            family_label: "Cause Contest".into(),
            provenance: prov("hyp:700.HK:contest"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK is being framed by broader stress".into(),
            confidence: dec!(0.66),
            local_support_weight: dec!(0.18),
            local_contradict_weight: dec!(0.06),
            propagated_support_weight: dec!(0.62),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![
                ReasoningEvidence {
                    statement: "market stress route remains active".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.58),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "tech spillover still supports repricing".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.32),
                    references: vec![sector_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "local bid still absorbs supply".into(),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.18),
                    references: vec!["depth:700.HK".into()],
                    provenance: base_provenance.clone(),
                },
            ],
            invalidation_conditions: vec![crate::ontology::InvalidationCondition {
                description: "market stress route deactivates".into(),
                references: vec![market_path.path_id.clone()],
            }],
            propagation_path_ids: vec![market_path.path_id.clone(), sector_path.path_id.clone()],
            expected_observations: vec![],
        };
        let reasoning_market = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![leading_market_hypothesis.clone()],
            propagation_paths: vec![market_path.clone(), sector_path.clone()],
            investigation_selections: vec![InvestigationSelection {
                investigation_id: "investigation:700.HK".into(),
                hypothesis_id: leading_market_hypothesis.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                provenance: prov("investigation:700.HK"),
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                family_key: leading_market_hypothesis.family_key.clone(),
                family_label: leading_market_hypothesis.family_label.clone(),
                confidence: setup.confidence,
                confidence_gap: setup.confidence_gap,
                priority_score: setup.heuristic_edge,
                attention_hint: setup.action.clone(),
                rationale: leading_market_hypothesis.statement.clone(),
                notes: vec![],
            }],
            tactical_setups: vec![setup.clone()],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let initial = derive_backward_reasoning(&reasoning_market, &world_state, None);
        let initial_investigation = &initial.investigations[0];
        assert_eq!(
            initial_investigation
                .leading_cause
                .as_ref()
                .map(|cause| cause.scope.clone()),
            Some(ReasoningScope::market())
        );

        let contradicted_market_hypothesis = Hypothesis {
            propagated_contradict_weight: dec!(0.44),
            evidence: vec![
                ReasoningEvidence {
                    statement: "market stress route remains active".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.42),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "tech spillover still supports repricing".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.48),
                    references: vec![sector_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "market path keeps failing follow-through".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: crate::ontology::EvidencePolarity::Contradicts,
                    weight: dec!(0.44),
                    references: vec![market_path.path_id.clone()],
                    provenance: base_provenance.clone(),
                },
                ReasoningEvidence {
                    statement: "local bid still absorbs supply".into(),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: crate::ontology::EvidencePolarity::Supports,
                    weight: dec!(0.18),
                    references: vec!["depth:700.HK".into()],
                    provenance: base_provenance,
                },
            ],
            ..leading_market_hypothesis
        };
        let reasoning_contradicted = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![contradicted_market_hypothesis],
            propagation_paths: vec![market_path, sector_path],
            investigation_selections: vec![InvestigationSelection {
                investigation_id: "investigation:700.HK".into(),
                hypothesis_id: setup.hypothesis_id.clone(),
                runner_up_hypothesis_id: None,
                provenance: prov("investigation:700.HK"),
                scope: setup.scope.clone(),
                title: setup.title.clone(),
                family_key: "flow-led".into(),
                family_label: "Flow".into(),
                confidence: setup.confidence,
                confidence_gap: setup.confidence_gap,
                priority_score: setup.heuristic_edge,
                attention_hint: setup.action.clone(),
                rationale: "investigate 700.HK".into(),
                notes: vec![],
            }],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let contradicted =
            derive_backward_reasoning(&reasoning_contradicted, &world_state, Some(&initial));
        let contradicted_investigation = &contradicted.investigations[0];
        assert_eq!(
            contradicted_investigation
                .leading_cause
                .as_ref()
                .map(|cause| cause.scope.clone()),
            Some(ReasoningScope::Sector("tech".into()))
        );
        assert!(contradicted_investigation
            .runner_up_cause
            .as_ref()
            .is_some_and(|cause| cause.scope == ReasoningScope::market()));
        assert!(contradicted_investigation
            .runner_up_cause
            .as_ref()
            .is_some_and(|cause| cause.contradict_weight > Decimal::ZERO));
    }

    #[test]
    fn backward_selection_can_promote_high_value_observe_case() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:observe".into(),
            family_key: "propagation".into(),
            family_label: "Propagation Chain".into(),
            provenance: prov("hyp:700.HK:observe"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK may currently reflect cross-scope propagation".into(),
            confidence: dec!(0.78),
            local_support_weight: dec!(0.12),
            local_contradict_weight: dec!(0.03),
            propagated_support_weight: dec!(0.44),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:diffusion:market:700.HK".into()],
            expected_observations: vec![],
        };
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:observe".into(),
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:700.HK:observe"),
            lineage: crate::ontology::DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "observe".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.78),
            confidence_gap: dec!(0.09),
            heuristic_edge: dec!(0.11),
            convergence_score: Some(dec!(0.42)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: hypothesis.statement.clone(),
            risk_notes: vec![],
            policy_verdict: None,
        };
        let reasoning = ReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![hypothesis],
            propagation_paths: vec![],
            investigation_selections: vec![InvestigationSelection {
                investigation_id: "investigation:700.HK".into(),
                hypothesis_id: "hyp:700.HK:observe".into(),
                runner_up_hypothesis_id: None,
                provenance: prov("investigation:700.HK"),
                scope: ReasoningScope::Symbol(sym("700.HK")),
                title: "Long 700.HK".into(),
                family_key: "flow-led".into(),
                family_label: "Flow".into(),
                confidence: dec!(0.78),
                confidence_gap: dec!(0.09),
                priority_score: dec!(0.11),
                attention_hint: "observe".into(),
                rationale: "investigate propagation".into(),
                notes: vec![],
            }],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
        };
        let hypothesis_map = reasoning
            .hypotheses
            .iter()
            .map(|item| (item.hypothesis_id.as_str(), item))
            .collect::<HashMap<_, _>>();
        let selected =
            select_backward_investigation_targets(&reasoning, &hypothesis_map, &HashMap::new());

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].attention_hint, "observe");
    }
