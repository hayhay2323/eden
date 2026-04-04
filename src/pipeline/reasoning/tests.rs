use std::collections::{HashMap, HashSet};

use petgraph::graph::DiGraph;
use rust_decimal_macros::dec;
use time::OffsetDateTime;

use super::context::{AbsenceMemory, FamilyBoostLedger};
use super::propagation::canonicalize_paths;
use super::support::{summarize_evidence_weights, FamilyAlphaGate};
use super::synthesis::derive_hypotheses;
use super::*;
use crate::action::narrative::Regime;
use crate::graph::decision::{
    ConvergenceScore, MarketRegimeBias, MarketRegimeFilter, OrderDirection, OrderSuggestion,
};
use crate::graph::graph::{
    BrainGraph, EdgeKind, InstitutionNode, InstitutionToStock, NodeKind, SectorNode, StockNode,
    StockToSector, StockToStock,
};
use crate::graph::insights::{GraphInsights, MarketStressIndex, RotationPair, SharedHolderAnomaly};
use crate::ontology::domain::{DerivedSignal, Event, ProvenanceMetadata, ProvenanceSource};
use crate::ontology::objects::{InstitutionId, SectorId, Symbol, ThemeId};
use crate::ontology::reasoning::{
    DecisionLineage, EvidencePolarity, HypothesisTrackStatus, PolicyVerdictKind,
    PolicyVerdictSummary, PropagationStep, ReasoningEvidence, ReasoningEvidenceKind,
    ReasoningScope,
};
use crate::pipeline::dimensions::SymbolDimensions;
use crate::pipeline::signals::{
    DerivedSignalKind, DerivedSignalRecord, EventSnapshot, MarketEventKind, MarketEventRecord,
    SignalScope,
};
use crate::temporal::lineage::FamilyContextLineageOutcome;

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

    let reasoning = ReasoningSnapshot::derive(&events, &signals, &insights, &decision, &[], &[]);
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
    assert!(reasoning.tactical_setups[0]
        .risk_notes
        .iter()
        .any(|note| note == "fresh_symbol_confirmation=true"));
    assert!(reasoning
        .hypotheses
        .iter()
        .any(|hypothesis| hypothesis.local_support_weight > Decimal::ZERO));
}

#[test]
fn family_alpha_gate_blocks_negative_risk_repricing_hypotheses() {
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![Event::new(
            MarketEventRecord {
                scope: SignalScope::Market,
                kind: MarketEventKind::MarketStressElevated,
                magnitude: dec!(0.8),
                summary: "market stress is elevated".into(),
            },
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
        )],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![],
    };
    let gate = FamilyAlphaGate::from_lineage_priors(
        &[FamilyContextLineageOutcome {
            family: "Risk Repricing".into(),
            session: "offhours".into(),
            market_regime: "neutral".into(),
            resolved: 252,
            mean_net_return: dec!(-0.0011),
            follow_through_rate: Decimal::ZERO,
            invalidation_rate: dec!(0.45),
            ..FamilyContextLineageOutcome::default()
        }],
        "offhours",
        "neutral",
    );

    let hypotheses = derive_hypotheses(&events, &signals, &[], Some(&gate), &AbsenceMemory::default(), None);

    assert!(hypotheses
        .iter()
        .all(|hypothesis| hypothesis.family_label != "Risk Repricing"));
}

#[test]
fn catalyst_activation_emits_catalyst_repricing_hypothesis() {
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![Event::new(
            MarketEventRecord {
                scope: SignalScope::Theme(ThemeId("tech_sector".into())),
                kind: MarketEventKind::CatalystActivation,
                magnitude: dec!(0.7),
                summary: "theme catalyst remains active".into(),
            },
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
        )],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![],
    };

    let hypotheses = derive_hypotheses(&events, &signals, &[], None, &AbsenceMemory::default(), None);

    assert!(hypotheses
        .iter()
        .any(|hypothesis| hypothesis.family_label == "Catalyst Repricing"));
    assert!(hypotheses
        .iter()
        .any(|hypothesis| matches!(hypothesis.scope, ReasoningScope::Theme(_))));
}

#[test]
fn convergence_hypothesis_emerges_from_vortex_topology() {
    let symbol_scope = sym("700.HK");
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::InstitutionalFlip,
                    magnitude: dec!(0.8),
                    summary: "institution flipped".into(),
                },
                prov("event:flip"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::CandlestickBreakout,
                    magnitude: dec!(0.7),
                    summary: "breakout".into(),
                },
                prov("event:breakout"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::CatalystActivation,
                    magnitude: dec!(0.6),
                    summary: "catalyst active".into(),
                },
                prov("event:catalyst"),
            ),
        ],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: DerivedSignalKind::Convergence,
                    strength: dec!(0.6),
                    summary: "convergence".into(),
                },
                prov("signal:convergence"),
            ),
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: DerivedSignalKind::SmartMoneyPressure,
                    strength: dec!(0.55),
                    summary: "pressure".into(),
                },
                prov("signal:pressure"),
            ),
        ],
    };
    let paths = vec![
        PropagationPath {
            path_id: "path:shared-rotation".into(),
            summary: "mixed chain".into(),
            confidence: dec!(0.6),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::Symbol(symbol_scope.clone()),
                    to: ReasoningScope::Symbol(sym("9988.HK")),
                    mechanism: "shared holder overlap".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Symbol(sym("9988.HK")),
                    to: ReasoningScope::Sector(SectorId("tech".into())),
                    mechanism: "rotation relay".into(),
                    confidence: dec!(0.55),
                    references: vec![],
                },
            ],
        },
        PropagationPath {
            path_id: "path:sector-bridge".into(),
            summary: "sector bridge".into(),
            confidence: dec!(0.5),
            steps: vec![PropagationStep {
                from: ReasoningScope::Sector(SectorId("tech".into())),
                to: ReasoningScope::Symbol(symbol_scope.clone()),
                mechanism: "sector spillover".into(),
                confidence: dec!(0.5),
                references: vec![],
            }],
        },
    ];

    let hypotheses = derive_hypotheses(&events, &signals, &paths, None, &AbsenceMemory::default(), None);
    let convergence = hypotheses
        .iter()
        .find(|hypothesis| {
            hypothesis.scope == ReasoningScope::Symbol(symbol_scope.clone())
                && hypothesis.family_key == "convergence_hypothesis"
        })
        .expect("convergence hypothesis");

    assert_eq!(convergence.family_label, "Convergence Hypothesis");
    assert!(convergence.statement.contains("convergence vortex"));
    assert_eq!(convergence.propagation_path_ids.len(), 2);
    assert!(convergence
        .provenance
        .note
        .as_deref()
        .unwrap_or_default()
        .contains("channel_diversity="));
    assert!(convergence
        .provenance
        .note
        .as_deref()
        .unwrap_or_default()
        .contains("vortex_strength="));
}

#[test]
fn convergence_hypothesis_requires_three_channels() {
    let symbol_scope = sym("700.HK");
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![Event::new(
            MarketEventRecord {
                scope: SignalScope::Symbol(symbol_scope.clone()),
                kind: MarketEventKind::InstitutionalFlip,
                magnitude: dec!(0.6),
                summary: "institution flipped".into(),
            },
            prov("event:flip"),
        )],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![DerivedSignal::new(
            DerivedSignalRecord {
                scope: SignalScope::Symbol(symbol_scope.clone()),
                kind: DerivedSignalKind::Convergence,
                strength: dec!(0.55),
                summary: "convergence".into(),
            },
            prov("signal:convergence"),
        )],
    };
    let paths = vec![PropagationPath {
        path_id: "path:relay".into(),
        summary: "institution relay".into(),
        confidence: dec!(0.5),
        steps: vec![PropagationStep {
            from: ReasoningScope::Symbol(sym("9988.HK")),
            to: ReasoningScope::Symbol(symbol_scope.clone()),
            mechanism: "institution diffusion".into(),
            confidence: dec!(0.5),
            references: vec![],
        }],
    }];

    let hypotheses = derive_hypotheses(&events, &signals, &paths, None, &AbsenceMemory::default(), None);

    assert!(hypotheses
        .iter()
        .all(|hypothesis| hypothesis.family_key != "convergence_hypothesis"));
}

#[test]
fn learned_vortex_pattern_feedback_promotes_convergence_hypothesis() {
    let symbol_scope = sym("700.HK");
    let convergence_hypothesis = Hypothesis {
        hypothesis_id: "hyp:700.HK:convergence_hypothesis".into(),
        family_key: "convergence_hypothesis".into(),
        family_label: "Convergence Hypothesis".into(),
        provenance: prov("hyp:700.HK:convergence"),
        scope: ReasoningScope::Symbol(symbol_scope.clone()),
        statement: "700.HK shows an emergent convergence vortex".into(),
        confidence: dec!(0.55),
        local_support_weight: dec!(0.45),
        local_contradict_weight: Decimal::ZERO,
        propagated_support_weight: dec!(0.20),
        propagated_contradict_weight: Decimal::ZERO,
        evidence: vec![],
        invalidation_conditions: vec![],
        propagation_path_ids: vec!["path:700".into()],
        expected_observations: vec![],
    };
    let flow_hypothesis = Hypothesis {
        hypothesis_id: "hyp:700.HK:flow".into(),
        family_key: "flow".into(),
        family_label: "Directed Flow".into(),
        provenance: prov("hyp:700.HK:flow"),
        scope: ReasoningScope::Symbol(symbol_scope.clone()),
        statement: "700.HK may reflect directed flow repricing".into(),
        confidence: dec!(0.56),
        local_support_weight: dec!(0.40),
        local_contradict_weight: Decimal::ZERO,
        propagated_support_weight: Decimal::ZERO,
        propagated_contradict_weight: Decimal::ZERO,
        evidence: vec![],
        invalidation_conditions: vec![],
        propagation_path_ids: vec![],
        expected_observations: vec![],
    };
    let decision = DecisionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        convergence_scores: HashMap::from([(
            symbol_scope.clone(),
            ConvergenceScore {
                symbol: symbol_scope.clone(),
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
            symbol: symbol_scope.clone(),
            direction: OrderDirection::Buy,
            convergence: ConvergenceScore {
                symbol: symbol_scope.clone(),
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
            convergence_score: dec!(0.40),
            effective_confidence: dec!(0.55),
            external_confirmation: None,
            external_conflict: None,
            external_support_slug: None,
            external_support_probability: None,
            external_conflict_slug: None,
            external_conflict_probability: None,
        }],
        degradations: HashMap::new(),
    };
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![],
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
            sector_synchrony: Decimal::ZERO,
            pressure_consensus: Decimal::ZERO,
            conflict_intensity_mean: Decimal::ZERO,
            market_temperature_stress: Decimal::ZERO,
            composite_stress: Decimal::ZERO,
        },
        institution_stock_counts: HashMap::new(),
        edge_profiles: vec![],
    };
    let mut reasoning = ReasoningSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        hypotheses: vec![convergence_hypothesis.clone(), flow_hypothesis],
        propagation_paths: vec![PropagationPath {
            path_id: "path:700".into(),
            summary: "convergence path".into(),
            confidence: dec!(0.40),
            steps: vec![],
        }],
        investigation_selections: vec![],
        tactical_setups: vec![],
        hypothesis_tracks: vec![],
        case_clusters: vec![],
    };
    let world_state = crate::ontology::world::WorldStateSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        entities: vec![],
        vortices: vec![crate::ontology::world::Vortex {
            vortex_id: "vortex:700.HK".into(),
            center_entity_id: "world:setup:700.HK".into(),
            center_scope: ReasoningScope::Symbol(symbol_scope.clone()),
            layer: crate::ontology::world::WorldLayer::Leaf,
            flow_paths: vec![
                crate::ontology::world::FlowPath {
                    source_entity_id: "edge:1".into(),
                    source_scope: ReasoningScope::Symbol(sym("9988.HK")),
                    channel: "broker_flow".into(),
                    weight: dec!(0.30),
                    polarity: crate::ontology::world::FlowPolarity::Confirming,
                },
                crate::ontology::world::FlowPath {
                    source_entity_id: "edge:2".into(),
                    source_scope: ReasoningScope::Market(Default::default()),
                    channel: "catalyst".into(),
                    weight: dec!(0.20),
                    polarity: crate::ontology::world::FlowPolarity::Confirming,
                },
            ],
            strength: dec!(0.26),
            channel_diversity: 2,
            coherence: dec!(0.52),
            narrative: None,
        }],
    };
    let patterns = vec![crate::temporal::lineage::VortexSuccessPattern {
        center_kind: "symbol".into(),
        role: "center".into(),
        channel_signature: "broker_flow|catalyst|propagation".into(),
        dominant_channels: vec!["broker_flow".into(), "catalyst".into()],
        top_family: "Convergence Hypothesis".into(),
        samples: 2,
        mean_net_return: dec!(0.03),
        mean_strength: dec!(0.45),
        mean_coherence: dec!(0.70),
        mean_channel_diversity: dec!(3),
    }];

    let changed = apply_vortex_success_pattern_feedback(
        &mut reasoning,
        &decision,
        &events,
        &insights,
        &[],
        &[],
        &[],
        None,
        None,
        None,
        &patterns,
        &world_state,
    );

    assert!(changed);
    let boosted = reasoning
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.hypothesis_id == convergence_hypothesis.hypothesis_id)
        .expect("boosted convergence hypothesis");
    assert!(boosted.confidence > convergence_hypothesis.confidence);
    assert!(boosted
        .provenance
        .note
        .as_deref()
        .unwrap_or_default()
        .contains("learned_vortex_boost="));
    assert_eq!(reasoning.tactical_setups.len(), 1);
    assert_eq!(
        reasoning.tactical_setups[0].hypothesis_id,
        convergence_hypothesis.hypothesis_id
    );
    assert!(reasoning.tactical_setups[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("learned_vortex_boost=")));
}

#[test]
fn shared_symbol_hypotheses_are_capped_to_top_three() {
    let symbol_scope = sym("700.HK");
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::InstitutionalFlip,
                    magnitude: dec!(0.8),
                    summary: "institution flipped".into(),
                },
                prov("event:flip"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::CandlestickBreakout,
                    magnitude: dec!(0.7),
                    summary: "breakout".into(),
                },
                prov("event:breakout"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: MarketEventKind::CatalystActivation,
                    magnitude: dec!(0.6),
                    summary: "catalyst".into(),
                },
                prov("event:catalyst"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::MarketStressElevated,
                    magnitude: dec!(0.5),
                    summary: "stress".into(),
                },
                prov("event:stress"),
            ),
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::StressRegimeShift,
                    magnitude: dec!(0.4),
                    summary: "regime shift".into(),
                },
                prov("event:shift"),
            ),
        ],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: DerivedSignalKind::Convergence,
                    strength: dec!(0.6),
                    summary: "convergence".into(),
                },
                prov("signal:convergence"),
            ),
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: DerivedSignalKind::SmartMoneyPressure,
                    strength: dec!(0.55),
                    summary: "pressure".into(),
                },
                prov("signal:pressure"),
            ),
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(symbol_scope.clone()),
                    kind: DerivedSignalKind::CandlestickConviction,
                    strength: dec!(0.5),
                    summary: "candles".into(),
                },
                prov("signal:candles"),
            ),
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Market,
                    kind: DerivedSignalKind::MarketStress,
                    strength: dec!(0.45),
                    summary: "market stress".into(),
                },
                prov("signal:stress"),
            ),
        ],
    };
    let paths = vec![
        PropagationPath {
            path_id: "path:shared-rotation".into(),
            summary: "mixed chain".into(),
            confidence: dec!(0.6),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::Symbol(symbol_scope.clone()),
                    to: ReasoningScope::Symbol(sym("9988.HK")),
                    mechanism: "shared holder overlap".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Symbol(sym("9988.HK")),
                    to: ReasoningScope::Sector(SectorId("tech".into())),
                    mechanism: "rotation relay".into(),
                    confidence: dec!(0.55),
                    references: vec![],
                },
            ],
        },
        PropagationPath {
            path_id: "path:sector-bridge".into(),
            summary: "sector bridge".into(),
            confidence: dec!(0.5),
            steps: vec![PropagationStep {
                from: ReasoningScope::Sector(SectorId("tech".into())),
                to: ReasoningScope::Symbol(symbol_scope.clone()),
                mechanism: "sector spillover".into(),
                confidence: dec!(0.5),
                references: vec![],
            }],
        },
        PropagationPath {
            path_id: "path:stress".into(),
            summary: "stress path".into(),
            confidence: dec!(0.45),
            steps: vec![PropagationStep {
                from: ReasoningScope::market(),
                to: ReasoningScope::Symbol(symbol_scope.clone()),
                mechanism: "market stress diffusion".into(),
                confidence: dec!(0.45),
                references: vec![],
            }],
        },
    ];

    let hypotheses = derive_hypotheses(&events, &signals, &paths, None, &AbsenceMemory::default(), None);
    let symbol_hypotheses = hypotheses
        .iter()
        .filter(|hypothesis| hypothesis.scope == ReasoningScope::Symbol(symbol_scope.clone()))
        .collect::<Vec<_>>();

    assert_eq!(symbol_hypotheses.len(), 3);
    assert!(symbol_hypotheses
        .iter()
        .any(|item| item.family_key == "convergence_hypothesis"));
    assert!(symbol_hypotheses
        .iter()
        .any(|item| item.family_key == "flow"));
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
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec!["local_support=0.40".into()],
        review_reason_code: None,
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
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0.24".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
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
        None,
        None,
        &FamilyBoostLedger::default(),
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
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0.24".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
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
        None,
        None,
        &FamilyBoostLedger::default(),
    );
    assert_eq!(updated[0].action, "review");
}

#[test]
fn track_policy_tightens_enter_under_reviewer_doctrine_pressure() {
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
        heuristic_edge: dec!(0.035),
        convergence_score: Some(dec!(0.45)),
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0.24".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
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
    let doctrine = ReviewerDoctrinePressure {
        updates: 8,
        reflexive_correction_rate: dec!(0.30),
        narrative_failure_rate: dec!(0.15),
        dominant_mechanism: Some("Narrative Failure".into()),
        dominant_rejection_reason: Some("Evidence Too Weak".into()),
        family_pressure_overrides: HashMap::new(),
    };

    let updated = apply_track_action_policy(
        &[setup],
        &[track],
        &[],
        OffsetDateTime::UNIX_EPOCH,
        &MarketRegimeFilter::neutral(),
        &[],
        None,
        Some(&doctrine),
        &FamilyBoostLedger::default(),
    );
    assert_eq!(updated[0].action, "review");
}

#[test]
fn reviewer_doctrine_pressure_targets_weaker_families() {
    let doctrine = ReviewerDoctrinePressure {
        updates: 8,
        reflexive_correction_rate: dec!(0.30),
        narrative_failure_rate: dec!(0.15),
        dominant_mechanism: Some("Narrative Failure".into()),
        dominant_rejection_reason: Some("Evidence Too Weak".into()),
        family_pressure_overrides: HashMap::new(),
    };

    let propagation_pressure = doctrine.pressure_for_family(Some("Propagation Chain"));
    let flow_pressure = doctrine.pressure_for_family(Some("Directed Flow"));

    assert!(propagation_pressure > flow_pressure);
}

#[test]
fn reviewer_doctrine_family_override_takes_precedence() {
    let doctrine = ReviewerDoctrinePressure {
        updates: 8,
        reflexive_correction_rate: dec!(0.30),
        narrative_failure_rate: dec!(0.15),
        dominant_mechanism: Some("Narrative Failure".into()),
        dominant_rejection_reason: Some("Evidence Too Weak".into()),
        family_pressure_overrides: HashMap::from([("directed-flow".into(), dec!(1.2))]),
    };

    assert_eq!(
        doctrine
            .pressure_for_family(Some("Directed Flow"))
            .round_dp(1),
        dec!(1.2)
    );
}

#[test]
fn stale_observe_cases_are_pruned() {
    let setup = TacticalSetup {
        setup_id: "setup:700.HK:observe".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:700.HK:observe"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "observe".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.52),
        confidence_gap: dec!(0.08),
        heuristic_edge: dec!(0.05),
        convergence_score: Some(dec!(0.22)),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "watch only".into(),
        causal_narrative: None,
        risk_notes: vec!["family=Directed Flow".into()],
        review_reason_code: None,
        policy_verdict: None,
    };
    let previous_track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "observe".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 7,
        status_streak: 4,
        confidence: dec!(0.52),
        previous_confidence: Some(dec!(0.51)),
        confidence_change: dec!(0.01),
        confidence_gap: dec!(0.08),
        previous_confidence_gap: Some(dec!(0.07)),
        confidence_gap_change: dec!(0.01),
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "stale".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let pruned = super::policy::prune_stale_tactical_setups(vec![setup], &[previous_track]);
    assert!(pruned.is_empty());
}

#[test]
fn low_quality_observe_cases_are_fast_expired() {
    let setup = TacticalSetup {
        setup_id: "setup:388.HK:observe".into(),
        hypothesis_id: "hyp:388.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:388.HK:observe"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("388.HK")),
        title: "Observe 388.HK".into(),
        action: "observe".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.45),
        confidence_gap: dec!(0.05),
        heuristic_edge: dec!(0.03),
        convergence_score: Some(dec!(0.15)),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "watch".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let previous_track = HypothesisTrack {
        track_id: "track:388.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "observe".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 3,
        status_streak: 2,
        confidence: dec!(0.45),
        previous_confidence: Some(dec!(0.44)),
        confidence_change: dec!(0.01),
        confidence_gap: dec!(0.05),
        previous_confidence_gap: Some(dec!(0.06)),
        confidence_gap_change: dec!(-0.01),
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "low quality".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let pruned = super::policy::prune_stale_tactical_setups(vec![setup], &[previous_track]);
    assert!(
        pruned.is_empty(),
        "low-quality observe (gap<0.10, edge<=0.05) should be fast-expired at age 3"
    );
}

#[test]
fn decent_observe_cases_survive_past_fast_expire_window() {
    let setup = TacticalSetup {
        setup_id: "setup:1211.HK:observe".into(),
        hypothesis_id: "hyp:1211.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:1211.HK:observe"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("1211.HK")),
        title: "Observe 1211.HK".into(),
        action: "observe".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.60),
        confidence_gap: dec!(0.20),
        heuristic_edge: dec!(0.12),
        convergence_score: Some(dec!(0.40)),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "good setup".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let previous_track = HypothesisTrack {
        track_id: "track:1211.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "observe".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 3,
        status_streak: 2,
        confidence: dec!(0.60),
        previous_confidence: Some(dec!(0.58)),
        confidence_change: dec!(0.02),
        confidence_gap: dec!(0.20),
        previous_confidence_gap: Some(dec!(0.18)),
        confidence_gap_change: dec!(0.02),
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "decent".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let pruned = super::policy::prune_stale_tactical_setups(vec![setup], &[previous_track]);
    assert_eq!(
        pruned.len(),
        1,
        "decent observe (gap=0.20, edge=0.12) should survive at age 3"
    );
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
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0.24".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
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
        None,
        None,
        &FamilyBoostLedger::default(),
    );
    assert_eq!(updated[0].action, "review");
    assert!(updated[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("market regime risk_off blocks long entries")));
}

#[test]
fn track_policy_reviews_stale_enter_without_fresh_confirmation() {
    let setup = TacticalSetup {
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
        provenance: prov("setup:700.HK:enter"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.66),
        confidence_gap: dec!(0.18),
        heuristic_edge: dec!(0.12),
        convergence_score: Some(dec!(0.52)),
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=false".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
        policy_verdict: None,
    };
    let track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "enter".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 8,
        status_streak: 4,
        confidence: setup.confidence,
        previous_confidence: Some(setup.confidence),
        confidence_change: Decimal::ZERO,
        confidence_gap: setup.confidence_gap,
        previous_confidence_gap: Some(setup.confidence_gap),
        confidence_gap_change: Decimal::ZERO,
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "holding enter".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let updated = apply_track_action_policy(
        &[setup],
        &[track.clone()],
        &[track],
        OffsetDateTime::UNIX_EPOCH,
        &MarketRegimeFilter::neutral(),
        &[],
        None,
        None,
        &FamilyBoostLedger::default(),
    );
    assert_eq!(updated[0].action, "review");
    assert!(updated[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("no fresh symbol-level confirmation remains")));
}

#[test]
fn track_policy_reviews_conflicted_enter() {
    let setup = TacticalSetup {
        setup_id: "setup:1093.HK:enter".into(),
        hypothesis_id: "hyp:1093.HK:propagation".into(),
        runner_up_hypothesis_id: Some("hyp:1093.HK:flow".into()),
        provenance: prov("setup:1093.HK:enter"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("1093.HK")),
        title: "Short 1093.HK".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(1),
        confidence_gap: dec!(0.40),
        heuristic_edge: dec!(0.05),
        convergence_score: Some(dec!(0.05)),
        convergence_detail: None,
        workflow_id: Some("order:1093.HK:sell".into()),
        entry_rationale: "propagation leads".into(),
        causal_narrative: None,
        risk_notes: vec![
            "local_support=5.11".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=true".into(),
            "directional_support=0".into(),
            "directional_conflict=0.21".into(),
            "symbol_event_count=0".into(),
        ],
        review_reason_code: None,
        policy_verdict: None,
    };
    let track = HypothesisTrack {
        track_id: "track:1093.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "enter".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 32,
        status_streak: 12,
        confidence: setup.confidence,
        previous_confidence: Some(setup.confidence),
        confidence_change: Decimal::ZERO,
        confidence_gap: setup.confidence_gap,
        previous_confidence_gap: Some(setup.confidence_gap),
        confidence_gap_change: Decimal::ZERO,
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "holding enter".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let updated = apply_track_action_policy(
        &[setup],
        &[track.clone()],
        &[track],
        OffsetDateTime::UNIX_EPOCH,
        &MarketRegimeFilter::neutral(),
        &[],
        None,
        None,
        &FamilyBoostLedger::default(),
    );
    assert_eq!(updated[0].action, "review");
    assert!(updated[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("confirmation now conflicts with the case")));
}

#[test]
fn backward_confirmation_gate_demotes_matching_investigation_selection() {
    let setup = TacticalSetup {
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:700.HK:enter"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.72),
        confidence_gap: dec!(0.21),
        heuristic_edge: dec!(0.10),
        convergence_score: Some(dec!(0.45)),
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "enter".into(),
        status: HypothesisTrackStatus::Stable,
        age_ticks: 4,
        status_streak: 2,
        confidence: setup.confidence,
        previous_confidence: Some(setup.confidence),
        confidence_change: Decimal::ZERO,
        confidence_gap: setup.confidence_gap,
        previous_confidence_gap: Some(setup.confidence_gap),
        confidence_gap_change: Decimal::ZERO,
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "holding enter".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };
    let selection = InvestigationSelection {
        investigation_id: "investigation:700.HK".into(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        provenance: prov("investigation:700.HK"),
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        family_key: "flow".into(),
        family_label: "Directed Flow".into(),
        confidence: setup.confidence,
        confidence_gap: setup.confidence_gap,
        priority_score: setup.heuristic_edge,
        attention_hint: "enter".into(),
        rationale: "flow leads".into(),
        review_reason_code: None,
        notes: vec![],
    };
    let mut reasoning = ReasoningSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        hypotheses: vec![],
        propagation_paths: vec![],
        investigation_selections: vec![selection],
        tactical_setups: vec![setup.clone()],
        hypothesis_tracks: vec![track.clone()],
        case_clusters: vec![],
    };
    let backward = crate::ontology::world::BackwardReasoningSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        investigations: vec![],
    };

    let changed = apply_backward_confirmation_gate(
        &mut reasoning,
        std::slice::from_ref(&setup),
        std::slice::from_ref(&track),
        &backward,
    );

    assert!(changed);
    assert_eq!(reasoning.tactical_setups[0].action, "review");
    assert_eq!(
        reasoning.investigation_selections[0].attention_hint,
        "review"
    );
    assert!(reasoning.investigation_selections[0]
        .notes
        .iter()
        .any(|note| note
            .contains("backward_confirmation_gate=no backward investigation is available")));
    assert!(reasoning.investigation_selections[0]
        .rationale
        .contains("no backward investigation is available"));
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
        summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK".into(),
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
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "strong case".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
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
        convergence_detail: None,
        workflow_id: Some("order:9988.HK:buy".into()),
        entry_rationale: "secondary case".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
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
        summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK".into(),
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
        summary: "shared-holder overlap may transmit repricing between 9988.HK and 700.HK".into(),
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
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
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
        path.steps
            .iter()
            .any(|step| mechanism_family(&step.mechanism) == "sector_diffusion")
    }));
    assert!(!paths.iter().any(|path| {
        path.steps.iter().any(|step| {
            step.from == ReasoningScope::Symbol(sym("9988.HK"))
                && step.to == ReasoningScope::Symbol(sym("700.HK"))
        })
    }));
}

#[test]
fn multi_horizon_gate_allows_cold_start_families() {
    use crate::temporal::lineage::{HorizonLineageMetric, MultiHorizonGate};

    let metrics = vec![
        HorizonLineageMetric {
            horizon: "5m".into(),
            template: "Directed Flow".into(),
            total: 30,
            resolved: 25,
            hits: 6,
            hit_rate: dec!(0.24),
            mean_return: dec!(-0.005),
        },
        HorizonLineageMetric {
            horizon: "session".into(),
            template: "Sector Rotation".into(),
            total: 40,
            resolved: 35,
            hits: 25,
            hit_rate: dec!(0.71),
            mean_return: dec!(0.015),
        },
    ];
    let gate = MultiHorizonGate::from_metrics(&metrics);

    // "Directed Flow" was attempted (resolved > 0) but failed (negative return) → block
    assert!(!gate.allows("Directed Flow"));
    // "Sector Rotation" was attempted and passed → allow
    assert!(gate.allows("Sector Rotation"));
    // "Catalyst Repricing" was never attempted → cold start → allow
    assert!(gate.allows("Catalyst Repricing"));
}

#[test]
fn cold_start_family_passes_track_action_policy() {
    use crate::temporal::lineage::{HorizonLineageMetric, MultiHorizonGate};

    let setup = TacticalSetup {
        setup_id: "setup:700.HK:review".into(),
        hypothesis_id: "hyp:700.HK:catalyst".into(),
        runner_up_hypothesis_id: Some("hyp:700.HK:alt".into()),
        provenance: prov("setup:700.HK:review"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Catalyst Repricing 700.HK".into(),
        action: "review".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.70),
        confidence_gap: dec!(0.20),
        heuristic_edge: dec!(0.10),
        convergence_score: Some(dec!(0.40)),
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "catalyst thesis".into(),
        causal_narrative: None,
        risk_notes: vec![
            "family=Catalyst Repricing".into(),
            "local_support=0.40".into(),
            "fresh_symbol_confirmation=true".into(),
            "directional_conflict_present=false".into(),
            "directional_support=0.24".into(),
            "directional_conflict=0".into(),
            "symbol_event_count=2".into(),
        ],
        review_reason_code: None,
        policy_verdict: None,
    };
    let track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: setup.setup_id.clone(),
        hypothesis_id: setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
        scope: setup.scope.clone(),
        title: setup.title.clone(),
        action: "observe".into(),
        status: HypothesisTrackStatus::Strengthening,
        age_ticks: 3,
        status_streak: 2,
        confidence: setup.confidence,
        previous_confidence: Some(dec!(0.64)),
        confidence_change: dec!(0.06),
        confidence_gap: setup.confidence_gap,
        previous_confidence_gap: Some(dec!(0.14)),
        confidence_gap_change: dec!(0.06),
        heuristic_edge: setup.heuristic_edge,
        policy_reason: "warming".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };
    let gate = MultiHorizonGate::from_metrics(&[HorizonLineageMetric {
        horizon: "5m".into(),
        template: "Directed Flow".into(),
        total: 30,
        resolved: 20,
        hits: 4,
        hit_rate: dec!(0.20),
        mean_return: dec!(-0.01),
    }]);
    let regime = MarketRegimeFilter {
        bias: MarketRegimeBias::Neutral,
        confidence: dec!(0.60),
        breadth_up: dec!(0.40),
        breadth_down: dec!(0.30),
        average_return: dec!(0.002),
        leader_return: None,
        directional_consensus: dec!(0.0),
        external_bias: None,
        external_confidence: None,
        external_driver: None,
    };

    let updated = apply_track_action_policy(
        &[setup.clone()],
        &[track.clone()],
        &[],
        OffsetDateTime::UNIX_EPOCH,
        &regime,
        &[],
        Some(&gate),
        None,
        &FamilyBoostLedger::default(),
    );
    // Cold-start "Catalyst Repricing" (never attempted) should NOT be blocked
    assert_ne!(
        updated[0].action, "observe",
        "cold-start family 'Catalyst Repricing' should NOT be blocked by multi_horizon gate"
    );

    // In contrast, a family that WAS attempted and failed SHOULD be blocked
    let mut blocked_setup = setup;
    blocked_setup.risk_notes = vec![
        "family=Directed Flow".into(),
        "local_support=0.40".into(),
        "fresh_symbol_confirmation=true".into(),
        "directional_conflict_present=false".into(),
        "directional_support=0.24".into(),
        "directional_conflict=0".into(),
        "symbol_event_count=2".into(),
    ];
    let updated_blocked = apply_track_action_policy(
        &[blocked_setup],
        &[track],
        &[],
        OffsetDateTime::UNIX_EPOCH,
        &regime,
        &[],
        Some(&gate),
        None,
        &FamilyBoostLedger::default(),
    );
    assert_eq!(
        updated_blocked[0].action, "observe",
        "attempted-but-failed family 'Directed Flow' SHOULD be blocked by multi_horizon gate"
    );
}

#[test]
fn propagation_absence_demotes_sector_propagation_cases() {
    use crate::ontology::objects::SectorId;

    let setup_propagation = TacticalSetup {
        setup_id: "setup:tech:propagation".into(),
        hypothesis_id: "hyp:tech:propagation".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:tech:propagation"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Sector(SectorId("technology".into())),
        title: "Propagation Chain from 700.HK into tech sector".into(),
        action: "review".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.65),
        confidence_gap: dec!(0.15),
        heuristic_edge: dec!(0.08),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: Some("order:tech:buy".into()),
        entry_rationale: "sector propagation".into(),
        causal_narrative: None,
        risk_notes: vec![
            "family=Propagation Chain".into(),
            "sector=technology".into(),
        ],
        review_reason_code: None,
        policy_verdict: None,
    };
    let setup_flow = TacticalSetup {
        setup_id: "setup:700.HK:flow".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:700.HK:flow"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Directed Flow 700.HK".into(),
        action: "review".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.68),
        confidence_gap: dec!(0.18),
        heuristic_edge: dec!(0.10),
        convergence_score: Some(dec!(0.40)),
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow thesis".into(),
        causal_narrative: None,
        risk_notes: vec!["family=Directed Flow".into()],
        review_reason_code: None,
        policy_verdict: None,
    };

    let absence_sectors = vec![SectorId("technology".into())];
    let result = super::policy::demote_on_propagation_absence(
        vec![setup_propagation, setup_flow],
        &absence_sectors,
    );

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[0].action, "observe",
        "Propagation Chain case for affected sector should be demoted"
    );
    assert!(result[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("propagation_absence")));
    assert_eq!(
        result[1].action, "review",
        "Directed Flow (non-propagation family) should NOT be demoted"
    );
}

#[test]
fn sustained_underperformer_is_blocked_by_alpha_gate() {
    // Propagation Chain profile: 261 resolved, -3% net, 13.4% follow, 11.9% invalidation
    // Previously slipped through because old formula weighted follow/invalidation equally
    let events = EventSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        events: vec![Event::new(
            MarketEventRecord {
                scope: SignalScope::Market,
                kind: MarketEventKind::CompositeAcceleration,
                magnitude: dec!(0.5),
                summary: "sector move".into(),
            },
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
        )],
    };
    let signals = DerivedSignalSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: vec![],
    };
    let gate = FamilyAlphaGate::from_lineage_priors(
        &[FamilyContextLineageOutcome {
            family: "Propagation Chain".into(),
            session: "morning".into(),
            market_regime: "neutral".into(),
            resolved: 261,
            mean_net_return: dec!(-0.0308),
            follow_through_rate: dec!(0.134),
            invalidation_rate: dec!(0.119),
            ..FamilyContextLineageOutcome::default()
        }],
        "morning",
        "neutral",
    );

    let hypotheses = derive_hypotheses(&events, &signals, &[], Some(&gate), &AbsenceMemory::default(), None);
    assert!(
        hypotheses
            .iter()
            .all(|h| h.family_label != "Propagation Chain"),
        "Propagation Chain with -3% net and 13% follow should be blocked"
    );
}

#[test]
fn tiered_lineage_prior_classifies_sustained_underperformance() {
    use super::policy::{classify_lineage_prior_for_test, PriorSignal};

    // 30+ resolved, net < 0, follow < 25% → Negative
    let prior_sustained = FamilyContextLineageOutcome {
        family: "Propagation Chain".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 100,
        mean_net_return: dec!(-0.0308),
        follow_through_rate: dec!(0.134),
        invalidation_rate: dec!(0.119),
        ..FamilyContextLineageOutcome::default()
    };
    assert_eq!(
        classify_lineage_prior_for_test(&prior_sustained),
        PriorSignal::Negative,
        "sustained underperformer should be Negative"
    );

    // 10 resolved, barely negative — too few samples for tier 2
    let prior_cold = FamilyContextLineageOutcome {
        family: "Some Family".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 10,
        mean_net_return: dec!(-0.005),
        follow_through_rate: dec!(0.20),
        invalidation_rate: dec!(0.10),
        ..FamilyContextLineageOutcome::default()
    };
    assert_eq!(
        classify_lineage_prior_for_test(&prior_cold),
        PriorSignal::Neutral,
        "10 resolved with marginal performance should stay Neutral"
    );
}

#[test]
fn alpha_boost_rewards_proven_families() {
    use super::policy::compute_alpha_boost_for_test;

    // Cold start — not enough data
    let cold = FamilyContextLineageOutcome {
        family: "Momentum Shift".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 10,
        mean_net_return: dec!(0.02),
        follow_through_rate: dec!(0.60),
        invalidation_rate: dec!(0.10),
        ..FamilyContextLineageOutcome::default()
    };
    assert_eq!(
        compute_alpha_boost_for_test(&cold),
        dec!(0),
        "cold start family should get zero boost"
    );

    // Proven with 30 resolved, good return and follow-through
    let proven = FamilyContextLineageOutcome {
        family: "Momentum Shift".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 35,
        mean_net_return: dec!(0.008),
        follow_through_rate: dec!(0.48),
        invalidation_rate: dec!(0.20),
        ..FamilyContextLineageOutcome::default()
    };
    let boost = compute_alpha_boost_for_test(&proven);
    assert!(
        boost >= dec!(0.5),
        "proven family with 35 resolved, good stats should get >= 0.5 boost, got {}",
        boost
    );

    // Large sample champion
    let champion = FamilyContextLineageOutcome {
        family: "Flow Divergence".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 120,
        mean_net_return: dec!(0.018),
        follow_through_rate: dec!(0.58),
        invalidation_rate: dec!(0.15),
        ..FamilyContextLineageOutcome::default()
    };
    let big_boost = compute_alpha_boost_for_test(&champion);
    assert_eq!(
        big_boost,
        dec!(1.0),
        "champion family with 120 resolved and strong stats should get maximum boost"
    );

    // Negative family — no boost
    let loser = FamilyContextLineageOutcome {
        family: "Risk Repricing".into(),
        session: "morning".into(),
        market_regime: "neutral".into(),
        resolved: 50,
        mean_net_return: dec!(-0.005),
        follow_through_rate: dec!(0.20),
        invalidation_rate: dec!(0.40),
        ..FamilyContextLineageOutcome::default()
    };
    assert_eq!(
        compute_alpha_boost_for_test(&loser),
        dec!(0),
        "losing family should get zero boost"
    );
}

#[test]
fn midflight_health_demotes_enter_on_confidence_drop() {
    use super::policy::apply_midflight_health_check;

    let setup = TacticalSetup {
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:700.HK:enter"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.55),
        confidence_gap: dec!(0.10),
        heuristic_edge: dec!(0.04),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: Some(PolicyVerdictSummary {
            primary: PolicyVerdictKind::EnterReady,
            rationale: "strengthened".into(),
            review_reason_code: None,
            conflict_reason: None,
            horizons: vec![],
        }),
    };

    let prev_track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        status: HypothesisTrackStatus::Strengthening,
        age_ticks: 3,
        status_streak: 3,
        confidence: dec!(0.68),
        previous_confidence: Some(dec!(0.65)),
        confidence_change: dec!(0.03),
        confidence_gap: dec!(0.22),
        previous_confidence_gap: Some(dec!(0.20)),
        confidence_gap_change: dec!(0.02),
        heuristic_edge: dec!(0.10),
        policy_reason: "promoted".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let result = apply_midflight_health_check(vec![setup], &[prev_track]);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].action, "review",
        "enter should be demoted to review when confidence drops significantly"
    );
    assert!(
        result[0]
            .risk_notes
            .iter()
            .any(|n| n.starts_with("midflight_health:")),
        "should have a midflight_health risk note"
    );
}

#[test]
fn midflight_health_leaves_stable_enter_alone() {
    use super::policy::apply_midflight_health_check;

    let setup = TacticalSetup {
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        provenance: prov("setup:700.HK:enter"),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.70),
        confidence_gap: dec!(0.22),
        heuristic_edge: dec!(0.10),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: Some("order:700.HK:buy".into()),
        entry_rationale: "flow leads".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: Some(PolicyVerdictSummary {
            primary: PolicyVerdictKind::EnterReady,
            rationale: "strengthened".into(),
            review_reason_code: None,
            conflict_reason: None,
            horizons: vec![],
        }),
    };

    let prev_track = HypothesisTrack {
        track_id: "track:700.HK".into(),
        setup_id: "setup:700.HK:enter".into(),
        hypothesis_id: "hyp:700.HK:flow".into(),
        runner_up_hypothesis_id: None,
        scope: ReasoningScope::Symbol(sym("700.HK")),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        status: HypothesisTrackStatus::Strengthening,
        age_ticks: 3,
        status_streak: 3,
        confidence: dec!(0.68),
        previous_confidence: Some(dec!(0.65)),
        confidence_change: dec!(0.03),
        confidence_gap: dec!(0.20),
        previous_confidence_gap: Some(dec!(0.18)),
        confidence_gap_change: dec!(0.02),
        heuristic_edge: dec!(0.10),
        policy_reason: "promoted".into(),
        transition_reason: None,
        first_seen_at: OffsetDateTime::UNIX_EPOCH,
        last_updated_at: OffsetDateTime::UNIX_EPOCH,
        invalidated_at: None,
    };

    let result = apply_midflight_health_check(vec![setup], &[prev_track]);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].action, "enter",
        "stable enter case should NOT be demoted"
    );
    assert!(
        !result[0]
            .risk_notes
            .iter()
            .any(|n| n.starts_with("midflight_health:")),
        "stable case should not have midflight_health note"
    );
}
