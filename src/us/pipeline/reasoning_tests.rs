use std::collections::HashMap;

use super::policy::{
    apply_us_case_budget, diversify_us_case_surface, prune_us_stale_cases, setup_family_key,
};
use super::support::{event_polarity, signal_polarity, template_applicable};
use super::*;
use crate::ontology::domain::{DerivedSignal, Event, ProvenanceMetadata, ProvenanceSource};
use crate::ontology::objects::{SectorId, Symbol};
use crate::ontology::reasoning::{DecisionLineage, PropagationStep};
use crate::us::graph::graph::UsGraph;
use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
use crate::us::pipeline::signals::{
    UsDerivedSignalKind, UsDerivedSignalRecord, UsEventKind, UsEventRecord,
};
use rust_decimal_macros::dec;

fn sym(s: &str) -> Symbol {
    Symbol(s.into())
}

fn ts() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
}

fn prov() -> ProvenanceMetadata {
    ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
}

fn make_event(symbol: &str, kind: UsEventKind, magnitude: Decimal) -> Event<UsEventRecord> {
    Event::new(
        UsEventRecord {
            scope: UsSignalScope::Symbol(sym(symbol)),
            kind,
            magnitude,
            summary: "test event".into(),
        },
        prov(),
    )
}

fn make_signal(
    symbol: &str,
    kind: UsDerivedSignalKind,
    strength: Decimal,
) -> DerivedSignal<UsDerivedSignalRecord> {
    DerivedSignal::new(
        UsDerivedSignalRecord {
            scope: UsSignalScope::Symbol(sym(symbol)),
            kind,
            strength,
            summary: "test signal".into(),
        },
        prov(),
    )
}

fn make_dims(
    flow: Decimal,
    momentum: Decimal,
    volume: Decimal,
    prepost: Decimal,
    val: Decimal,
) -> UsSymbolDimensions {
    UsSymbolDimensions {
        capital_flow_direction: flow,
        price_momentum: momentum,
        volume_profile: volume,
        pre_post_market_anomaly: prepost,
        valuation: val,
        multi_horizon_momentum: Decimal::ZERO,
    }
}

fn make_graph(entries: Vec<(Symbol, UsSymbolDimensions)>) -> UsGraph {
    let snapshot = UsDimensionSnapshot {
        timestamp: ts(),
        dimensions: entries.into_iter().collect(),
    };
    let sector_map = HashMap::from([
        (sym("NET.US"), SectorId("tech".into())),
        (sym("DDOG.US"), SectorId("tech".into())),
    ]);
    UsGraph::compute(&snapshot, &sector_map, &HashMap::new())
}

// ── Template applicability ──

#[test]
fn pre_market_template_requires_dislocation_or_gap() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "TSLA.US",
            UsEventKind::PreMarketDislocation,
            dec!(0.03),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };
    let scope = ReasoningScope::Symbol(sym("TSLA.US"));
    assert!(template_applicable(
        &TEMPLATES[0],
        &scope,
        &events,
        &signals,
        &[]
    ));
}

#[test]
fn cross_market_template_requires_divergence_or_propagation() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "BABA.US",
            UsEventKind::CrossMarketDivergence,
            dec!(0.05),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };
    let scope = ReasoningScope::Symbol(sym("BABA.US"));
    assert!(template_applicable(
        &TEMPLATES[1],
        &scope,
        &events,
        &signals,
        &[]
    ));
}

#[test]
fn catalyst_template_requires_activation() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "NVDA.US",
            UsEventKind::CatalystActivation,
            dec!(0.6),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };
    let scope = ReasoningScope::Symbol(sym("NVDA.US"));
    let template = TEMPLATES
        .iter()
        .find(|template| template.key == TEMPLATE_CATALYST_REPRICING)
        .expect("catalyst repricing template");
    assert!(template_applicable(
        template,
        &scope,
        &events,
        &signals,
        &[]
    ));
}

#[test]
fn momentum_template_requires_event_and_signal() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event("NVDA.US", UsEventKind::VolumeSpike, dec!(0.8))],
    };
    let scope = ReasoningScope::Symbol(sym("NVDA.US"));

    // Event alone → not applicable
    let empty_signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };
    assert!(!template_applicable(
        &TEMPLATES[2],
        &scope,
        &events,
        &empty_signals,
        &[]
    ));

    // Event + signal → applicable
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "NVDA.US",
            UsDerivedSignalKind::StructuralComposite,
            dec!(0.5),
        )],
    };
    assert!(template_applicable(
        &TEMPLATES[2],
        &scope,
        &events,
        &signals,
        &[]
    ));
}

#[test]
fn template_not_applicable_for_unrelated_events() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event("AAPL.US", UsEventKind::VolumeSpike, dec!(0.5))],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };
    // Pre-market template should not match volume spike
    let scope = ReasoningScope::Symbol(sym("AAPL.US"));
    assert!(!template_applicable(
        &TEMPLATES[0],
        &scope,
        &events,
        &signals,
        &[]
    ));
}

// ── Evidence polarity ──

#[test]
fn pre_market_supports_dislocation_contradicts_reversal() {
    assert_eq!(
        event_polarity(
            TEMPLATE_PRE_MARKET_POSITIONING,
            &UsEventKind::PreMarketDislocation
        ),
        Some(EvidencePolarity::Supports)
    );
    assert_eq!(
        event_polarity(
            TEMPLATE_PRE_MARKET_POSITIONING,
            &UsEventKind::CapitalFlowReversal
        ),
        Some(EvidencePolarity::Contradicts)
    );
}

#[test]
fn momentum_valuation_extreme_contradicts() {
    assert_eq!(
        signal_polarity(
            TEMPLATE_MOMENTUM_CONTINUATION,
            &UsDerivedSignalKind::ValuationExtreme
        ),
        Some(EvidencePolarity::Contradicts)
    );
}

#[test]
fn cross_market_propagation_supports() {
    assert_eq!(
        signal_polarity(
            TEMPLATE_CROSS_MARKET_ARBITRAGE,
            &UsDerivedSignalKind::CrossMarketPropagation
        ),
        Some(EvidencePolarity::Supports)
    );
}

// ── Full derivation ──

#[test]
fn derive_produces_hypothesis_from_pre_market_event() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "TSLA.US",
            UsEventKind::PreMarketDislocation,
            dec!(0.04),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "TSLA.US",
            UsDerivedSignalKind::PreMarketConviction,
            dec!(0.6),
        )],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    let hyp = snap
        .hypotheses
        .iter()
        .find(|h| h.family_key == TEMPLATE_PRE_MARKET_POSITIONING);
    assert!(hyp.is_some());
    let hyp = hyp.unwrap();
    assert!(hyp.confidence > Decimal::ZERO);
    assert!(!hyp.evidence.is_empty());
}

#[test]
fn derive_produces_cross_market_hypothesis() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "BABA.US",
            UsEventKind::CrossMarketDivergence,
            dec!(0.05),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "BABA.US",
            UsDerivedSignalKind::CrossMarketPropagation,
            dec!(0.42),
        )],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    assert!(snap
        .hypotheses
        .iter()
        .any(|h| h.family_key == TEMPLATE_CROSS_MARKET_ARBITRAGE));
}

#[test]
fn derive_produces_catalyst_hypothesis() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "NVDA.US",
            UsEventKind::CatalystActivation,
            dec!(0.06),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "NVDA.US",
            UsDerivedSignalKind::StructuralComposite,
            dec!(0.4),
        )],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    assert!(snap
        .hypotheses
        .iter()
        .any(|h| h.family_key == TEMPLATE_CATALYST_REPRICING));
}

#[test]
fn us_symbol_hypotheses_are_capped_to_top_three() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![
            make_event("BABA.US", UsEventKind::CrossMarketDivergence, dec!(0.05)),
            make_event("BABA.US", UsEventKind::VolumeSpike, dec!(0.08)),
            make_event("BABA.US", UsEventKind::SectorMomentumShift, dec!(0.04)),
            make_event("BABA.US", UsEventKind::CatalystActivation, dec!(0.06)),
            make_event("BABA.US", UsEventKind::PreMarketDislocation, dec!(0.03)),
        ],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![
            make_signal(
                "BABA.US",
                UsDerivedSignalKind::StructuralComposite,
                dec!(0.45),
            ),
            make_signal(
                "BABA.US",
                UsDerivedSignalKind::CrossMarketPropagation,
                dec!(0.42),
            ),
            make_signal(
                "BABA.US",
                UsDerivedSignalKind::PreMarketConviction,
                dec!(0.35),
            ),
        ],
    };

    let hypotheses = derive_hypotheses(&events, &signals, &[]);
    let symbol_hypotheses = hypotheses
        .iter()
        .filter(|hypothesis| hypothesis.scope == ReasoningScope::Symbol(sym("BABA.US")))
        .collect::<Vec<_>>();

    assert_eq!(symbol_hypotheses.len(), 3);
    assert!(symbol_hypotheses
        .iter()
        .any(|item| item.family_key == TEMPLATE_MOMENTUM_CONTINUATION));
    assert!(symbol_hypotheses
        .iter()
        .any(|item| item.family_key == TEMPLATE_CROSS_MARKET_ARBITRAGE));
}

#[test]
fn convergence_hypothesis_emerges_from_us_diffusion_topology() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![
            make_event("BABA.US", UsEventKind::CrossMarketDivergence, dec!(0.60)),
            make_event("BABA.US", UsEventKind::VolumeSpike, dec!(0.55)),
            make_event("BABA.US", UsEventKind::CatalystActivation, dec!(0.45)),
        ],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![
            make_signal(
                "BABA.US",
                UsDerivedSignalKind::StructuralComposite,
                dec!(0.50),
            ),
            make_signal(
                "BABA.US",
                UsDerivedSignalKind::CrossMarketPropagation,
                dec!(0.48),
            ),
        ],
    };
    let paths = vec![
        PropagationPath {
            path_id: "path:us:cross-market".into(),
            summary: "9988.HK may diffuse into BABA.US".into(),
            confidence: dec!(0.55),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("9988.HK")),
                to: ReasoningScope::Symbol(sym("BABA.US")),
                mechanism: "cross-market diffusion".into(),
                confidence: dec!(0.55),
                references: vec![],
            }],
        },
        PropagationPath {
            path_id: "path:us:sector".into(),
            summary: "sector pressure may diffuse into BABA.US".into(),
            confidence: dec!(0.50),
            steps: vec![PropagationStep {
                from: ReasoningScope::Sector(SectorId("china-tech".into())),
                to: ReasoningScope::Symbol(sym("BABA.US")),
                mechanism: "sector diffusion".into(),
                confidence: dec!(0.50),
                references: vec![],
            }],
        },
    ];

    let hypotheses = derive_hypotheses(&events, &signals, &paths);
    let convergence = hypotheses
        .iter()
        .find(|item| {
            item.scope == ReasoningScope::Symbol(sym("BABA.US"))
                && item.family_key == "convergence_hypothesis"
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
}

#[test]
fn convergence_hypothesis_requires_three_us_channels() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "BABA.US",
            UsEventKind::CrossMarketDivergence,
            dec!(0.55),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "BABA.US",
            UsDerivedSignalKind::CrossMarketPropagation,
            dec!(0.50),
        )],
    };
    let paths = vec![PropagationPath {
        path_id: "path:us:cross-market".into(),
        summary: "9988.HK may diffuse into BABA.US".into(),
        confidence: dec!(0.52),
        steps: vec![PropagationStep {
            from: ReasoningScope::Symbol(sym("9988.HK")),
            to: ReasoningScope::Symbol(sym("BABA.US")),
            mechanism: "cross-market diffusion".into(),
            confidence: dec!(0.52),
            references: vec![],
        }],
    }];

    let hypotheses = derive_hypotheses(&events, &signals, &paths);

    assert!(hypotheses
        .iter()
        .all(|item| item.family_key != "convergence_hypothesis"));
}

#[test]
fn learned_us_convergence_pattern_feedback_promotes_convergence_hypothesis() {
    let convergence_hypothesis = Hypothesis {
        hypothesis_id: "hyp:BABA.US:convergence_hypothesis".into(),
        family_key: "convergence_hypothesis".into(),
        family_label: "Convergence Hypothesis".into(),
        provenance: prov().with_note(
            "family=Convergence Hypothesis; vortex_strength=0.44; channel_diversity=3; coherence=0.70; dominant_channels=cross-market|pre-market|sector rotation",
        ),
        scope: ReasoningScope::Symbol(sym("BABA.US")),
        statement: "BABA.US shows an emergent convergence vortex".into(),
        confidence: dec!(0.55),
        local_support_weight: dec!(0.45),
        local_contradict_weight: Decimal::ZERO,
        propagated_support_weight: dec!(0.20),
        propagated_contradict_weight: Decimal::ZERO,
        evidence: vec![],
        invalidation_conditions: vec![],
        propagation_path_ids: vec!["path:us:cross-market".into()],
        expected_observations: vec![],
    };
    let cross_market_hypothesis = Hypothesis {
        hypothesis_id: "hyp:BABA.US:cross_market_arbitrage".into(),
        family_key: TEMPLATE_CROSS_MARKET_ARBITRAGE.into(),
        family_label: "Cross-Market Arbitrage".into(),
        provenance: prov(),
        scope: ReasoningScope::Symbol(sym("BABA.US")),
        statement: "BABA.US may follow HK counterpart".into(),
        confidence: dec!(0.56),
        local_support_weight: dec!(0.40),
        local_contradict_weight: Decimal::ZERO,
        propagated_support_weight: dec!(0.10),
        propagated_contradict_weight: Decimal::ZERO,
        evidence: vec![],
        invalidation_conditions: vec![],
        propagation_path_ids: vec![],
        expected_observations: vec![],
    };
    let mut snapshot = UsReasoningSnapshot {
        timestamp: ts(),
        hypotheses: vec![convergence_hypothesis.clone(), cross_market_hypothesis],
        propagation_paths: vec![],
        investigation_selections: vec![],
        tactical_setups: vec![],
        hypothesis_tracks: vec![],
    };

    let changed = apply_us_convergence_success_pattern_feedback(
        &mut snapshot,
        1,
        &[],
        &[],
        Some(UsMarketRegimeBias::Neutral),
        None,
        None,
        None,
        None,
        &[crate::us::temporal::lineage::UsConvergenceSuccessPattern {
            channel_signature: "cross-market|pre-market|sector rotation".into(),
            dominant_channels: vec![
                "cross-market".into(),
                "pre-market".into(),
                "sector rotation".into(),
            ],
            top_family: "Convergence Hypothesis".into(),
            samples: 2,
            mean_net_return: dec!(0.06),
            mean_strength: dec!(0.45),
            mean_coherence: dec!(0.70),
            mean_channel_diversity: dec!(3),
        }],
    );

    assert!(changed);
    let boosted = snapshot
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
        .contains("learned_convergence_boost="));
    assert_eq!(snapshot.tactical_setups.len(), 1);
    assert_eq!(
        snapshot.tactical_setups[0].hypothesis_id,
        convergence_hypothesis.hypothesis_id
    );
    assert!(snapshot.tactical_setups[0]
        .risk_notes
        .iter()
        .any(|note| note.contains("learned_convergence_boost=")));
}

#[test]
fn doctrine_pressure_tightens_us_enter_promotions() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event(
            "BABA.US",
            UsEventKind::CrossMarketDivergence,
            dec!(0.05),
        )],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "BABA.US",
            UsDerivedSignalKind::CrossMarketPropagation,
            dec!(0.42),
        )],
    };
    let previous_setup = TacticalSetup {
        setup_id: "setup:BABA.US:review".into(),
        hypothesis_id: "hyp:BABA.US:cross_market_arbitrage".into(),
        runner_up_hypothesis_id: None,
        provenance: prov(),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("BABA.US")),
        title: "BABA.US - Cross-Market Arbitrage".into(),
        action: "review".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.70),
        confidence_gap: dec!(0.10),
        heuristic_edge: dec!(0.25),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "prior".into(),
        causal_narrative: None,
        risk_notes: vec!["family=cross_market_arbitrage".into()],
        review_reason_code: None,
        policy_verdict: None,
    };
    let doctrine = crate::pipeline::reasoning::ReviewerDoctrinePressure {
        updates: 8,
        reflexive_correction_rate: dec!(0.30),
        narrative_failure_rate: dec!(0.15),
        dominant_mechanism: Some("Arbitrage Convergence".into()),
        dominant_rejection_reason: Some("Evidence Too Weak".into()),
        family_pressure_overrides: HashMap::new(),
    };

    let snapshot = UsReasoningSnapshot::derive_with_policy(
        &events,
        &signals,
        10,
        std::slice::from_ref(&previous_setup),
        &[],
        Some(UsMarketRegimeBias::Neutral),
        None,
        None,
        None,
        Some(&doctrine),
    );

    assert!(snapshot
        .investigation_selections
        .iter()
        .all(|item| item.attention_hint != "enter"));
}

#[test]
fn doctrine_pressure_targets_timing_sensitive_us_families() {
    let doctrine = crate::pipeline::reasoning::ReviewerDoctrinePressure {
        updates: 8,
        reflexive_correction_rate: dec!(0.25),
        narrative_failure_rate: dec!(0.10),
        dominant_mechanism: Some("Event-driven Dislocation".into()),
        dominant_rejection_reason: Some("Timing Mismatch".into()),
        family_pressure_overrides: HashMap::new(),
    };

    let pre_market = doctrine.pressure_for_family(Some(TEMPLATE_PRE_MARKET_POSITIONING));
    let momentum = doctrine.pressure_for_family(Some(TEMPLATE_MOMENTUM_CONTINUATION));

    assert!(pre_market > momentum);
}

#[test]
fn stale_us_observe_cases_are_pruned() {
    let previous_setup = TacticalSetup {
        setup_id: "setup:NKE.US:observe".into(),
        hypothesis_id: "hyp:NKE.US:momentum".into(),
        runner_up_hypothesis_id: None,
        provenance: prov(),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym("NKE.US")),
        title: "NKE.US - Momentum".into(),
        action: "observe".into(),
        time_horizon: "intraday".into(),
        confidence: dec!(0.54),
        confidence_gap: dec!(0.08),
        heuristic_edge: dec!(0.05),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "watch".into(),
        causal_narrative: None,
        risk_notes: vec![format!("family={}", TEMPLATE_MOMENTUM_CONTINUATION)],
        review_reason_code: None,
        policy_verdict: None,
    };
    let current_setup = TacticalSetup {
        confidence: dec!(0.55),
        confidence_gap: dec!(0.09),
        ..previous_setup.clone()
    };
    let previous_track = crate::ontology::reasoning::HypothesisTrack {
        track_id: "track:NKE.US".into(),
        setup_id: previous_setup.setup_id.clone(),
        hypothesis_id: previous_setup.hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        scope: previous_setup.scope.clone(),
        title: previous_setup.title.clone(),
        action: "observe".into(),
        status: crate::ontology::reasoning::HypothesisTrackStatus::Stable,
        age_ticks: 7,
        status_streak: 4,
        confidence: previous_setup.confidence,
        previous_confidence: Some(dec!(0.53)),
        confidence_change: dec!(0.01),
        confidence_gap: previous_setup.confidence_gap,
        previous_confidence_gap: Some(dec!(0.07)),
        confidence_gap_change: dec!(0.01),
        heuristic_edge: previous_setup.heuristic_edge,
        policy_reason: "stale".into(),
        transition_reason: None,
        first_seen_at: ts(),
        last_updated_at: ts(),
        invalidated_at: None,
    };

    let pruned = prune_us_stale_cases(
        vec![current_setup],
        std::slice::from_ref(&previous_setup),
        std::slice::from_ref(&previous_track),
    );
    assert!(pruned.is_empty());
}

#[test]
fn derive_produces_momentum_hypothesis_with_contradiction() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event("NVDA.US", UsEventKind::VolumeSpike, dec!(0.8))],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![
            make_signal(
                "NVDA.US",
                UsDerivedSignalKind::StructuralComposite,
                dec!(0.5),
            ),
            make_signal("NVDA.US", UsDerivedSignalKind::ValuationExtreme, dec!(0.7)),
        ],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    let hyp = snap
        .hypotheses
        .iter()
        .find(|h| h.family_key == TEMPLATE_MOMENTUM_CONTINUATION);
    assert!(hyp.is_some());
    let hyp = hyp.unwrap();
    // Should have both supporting and contradicting evidence
    assert!(hyp
        .evidence
        .iter()
        .any(|e| e.polarity == EvidencePolarity::Supports));
    assert!(hyp
        .evidence
        .iter()
        .any(|e| e.polarity == EvidencePolarity::Contradicts));
    // Confidence should be reduced due to contradiction
    assert!(hyp.confidence < Decimal::ONE);
}

#[test]
fn derive_skips_hypothesis_without_supporting_evidence() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    assert!(snap.hypotheses.is_empty());
}

// ── Tactical setups ──

#[test]
fn tactical_setup_generated_from_hypothesis() {
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![make_event("TSLA.US", UsEventKind::GapOpen, dec!(0.04))],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![make_signal(
            "TSLA.US",
            UsDerivedSignalKind::PreMarketConviction,
            dec!(0.8),
        )],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    assert!(!snap.tactical_setups.is_empty());
    let setup = &snap.tactical_setups[0];
    assert!(!setup.hypothesis_id.is_empty());
    assert!(setup.confidence > Decimal::ZERO);
}

#[test]
fn tactical_setup_action_is_review_when_gap_small() {
    // Two competing hypotheses for same scope => small gap => "review"
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![
            make_event("NVDA.US", UsEventKind::VolumeSpike, dec!(0.8)),
            make_event("NVDA.US", UsEventKind::PreMarketDislocation, dec!(0.75)),
        ],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![
            make_signal(
                "NVDA.US",
                UsDerivedSignalKind::StructuralComposite,
                dec!(0.5),
            ),
            make_signal(
                "NVDA.US",
                UsDerivedSignalKind::PreMarketConviction,
                dec!(0.6),
            ),
        ],
    };

    let snap = UsReasoningSnapshot::derive(&events, &signals, &[], &[]);
    // With two competing hypotheses both supported, gap should be small
    let nvda_setups: Vec<_> = snap
        .tactical_setups
        .iter()
        .filter(|s| matches!(&s.scope, ReasoningScope::Symbol(sym) if sym.0 == "NVDA.US"))
        .collect();
    assert!(!nvda_setups.is_empty());
}

#[test]
fn competing_confidence_pure_support_is_one() {
    // (0.5 - 0) / 0.5 = 1.0 → mapped to (1+1)/2 = 1.0
    let evidence = vec![ReasoningEvidence {
        statement: "test".into(),
        kind: ReasoningEvidenceKind::LocalEvent,
        polarity: EvidencePolarity::Supports,
        weight: dec!(0.5),
        references: vec![],
        provenance: prov(),
    }];
    assert_eq!(competing_confidence(&evidence), dec!(1));
}

#[test]
fn competing_confidence_balanced_is_half() {
    // (0.5 - 0.5) / 1.0 = 0 → mapped to (0+1)/2 = 0.5
    let evidence = vec![
        ReasoningEvidence {
            statement: "for".into(),
            kind: ReasoningEvidenceKind::LocalEvent,
            polarity: EvidencePolarity::Supports,
            weight: dec!(0.5),
            references: vec![],
            provenance: prov(),
        },
        ReasoningEvidence {
            statement: "against".into(),
            kind: ReasoningEvidenceKind::LocalEvent,
            polarity: EvidencePolarity::Contradicts,
            weight: dec!(0.5),
            references: vec![],
            provenance: prov(),
        },
    ];
    assert_eq!(competing_confidence(&evidence), dec!(0.5));
}

#[test]
fn competing_confidence_pure_contradict_is_zero() {
    // (0 - 0.8) / 0.8 = -1 → mapped to (-1+1)/2 = 0.0
    let evidence = vec![ReasoningEvidence {
        statement: "bad".into(),
        kind: ReasoningEvidenceKind::LocalEvent,
        polarity: EvidencePolarity::Contradicts,
        weight: dec!(0.8),
        references: vec![],
        provenance: prov(),
    }];
    assert_eq!(competing_confidence(&evidence), dec!(0));
}

#[test]
fn competing_confidence_empty_is_zero() {
    assert_eq!(competing_confidence(&[]), dec!(0));
}

#[test]
fn diffusion_paths_seed_structural_diffusion_hypothesis() {
    let graph = make_graph(vec![
        (
            sym("NET.US"),
            make_dims(dec!(0.4), dec!(0.6), dec!(0.2), dec!(0), dec!(0.1)),
        ),
        (
            sym("DDOG.US"),
            make_dims(dec!(0.35), dec!(0.5), dec!(0.2), dec!(0), dec!(0.1)),
        ),
    ]);
    let structural_metrics = HashMap::from([
        (
            sym("NET.US"),
            UsStructuralRankMetrics {
                composite_delta: dec!(0.12),
                composite_acceleration: dec!(0.08),
                capital_flow_delta: dec!(0.05),
                flow_persistence: 4,
                flow_reversal: false,
            },
        ),
        (
            sym("DDOG.US"),
            UsStructuralRankMetrics {
                composite_delta: dec!(0.01),
                composite_acceleration: Decimal::ZERO,
                capital_flow_delta: Decimal::ZERO,
                flow_persistence: 1,
                flow_reversal: false,
            },
        ),
    ]);
    let events = UsEventSnapshot {
        timestamp: ts(),
        events: vec![],
    };
    let signals = UsDerivedSignalSnapshot {
        timestamp: ts(),
        signals: vec![],
    };

    let snapshot = UsReasoningSnapshot::derive_with_diffusion(
        &events,
        &signals,
        1,
        &[],
        &[],
        Some(UsMarketRegimeBias::Neutral),
        None,
        None,
        Some(&structural_metrics),
        &graph,
        &[],
        None,
    );

    let hyp = snapshot
        .hypotheses
        .iter()
        .find(|item| item.family_key == TEMPLATE_STRUCTURAL_DIFFUSION)
        .expect("structural diffusion hypothesis");
    assert!(!hyp.propagation_path_ids.is_empty());
    assert!(hyp.propagated_support_weight > Decimal::ZERO);
}

#[test]
fn lineage_feedback_can_flip_scope_winner() {
    let make_hyp = |id: &str, family_key: &str, confidence: Decimal| Hypothesis {
        hypothesis_id: id.into(),
        family_key: family_key.into(),
        family_label: family_key.into(),
        provenance: prov(),
        scope: ReasoningScope::Symbol(sym("AAPL.US")),
        statement: family_key.into(),
        confidence,
        local_support_weight: Decimal::ZERO,
        local_contradict_weight: Decimal::ZERO,
        propagated_support_weight: Decimal::ZERO,
        propagated_contradict_weight: Decimal::ZERO,
        evidence: vec![],
        invalidation_conditions: vec![],
        propagation_path_ids: vec![],
        expected_observations: vec![],
    };

    let stats = UsLineageStats {
        by_template: vec![
            crate::us::temporal::lineage::UsLineageContextStats {
                template: TEMPLATE_MOMENTUM_CONTINUATION.into(),
                session: String::new(),
                market_regime: String::new(),
                total: 40,
                resolved: 40,
                hits: 10,
                hit_rate: dec!(0.25),
                mean_return: dec!(-0.02),
                follow_expectancy: dec!(-0.02),
                fade_expectancy: dec!(0.01),
                wait_expectancy: Decimal::ZERO,
            },
            crate::us::temporal::lineage::UsLineageContextStats {
                template: TEMPLATE_SECTOR_ROTATION.into(),
                session: String::new(),
                market_regime: String::new(),
                total: 18,
                resolved: 18,
                hits: 13,
                hit_rate: dec!(0.72),
                mean_return: dec!(0.028),
                follow_expectancy: dec!(0.028),
                fade_expectancy: dec!(-0.01),
                wait_expectancy: Decimal::ZERO,
            },
        ],
        by_context: vec![],
    };

    let hypotheses = [
        make_hyp("hyp:momentum", TEMPLATE_MOMENTUM_CONTINUATION, dec!(0.68)),
        make_hyp("hyp:rotation", TEMPLATE_SECTOR_ROTATION, dec!(0.60)),
    ];
    let investigations = derive_investigation_selections(
        &hypotheses,
        1,
        &[],
        ts(),
        Some(UsMarketRegimeBias::Neutral),
        Some(&stats),
        None,
        None,
        None,
    );
    let setups = derive_tactical_setups(&hypotheses, &investigations, &[], Some(&stats));

    assert_eq!(setups.len(), 1);
    assert_eq!(setups[0].hypothesis_id, "hyp:rotation");
    assert!(setups[0].confidence > dec!(0.60));
}

#[test]
fn attention_budget_caps_overrepresented_negative_family() {
    let make_setup = |id: &str, family: &str, confidence: Decimal| TacticalSetup {
        setup_id: id.into(),
        hypothesis_id: id.into(),
        runner_up_hypothesis_id: None,
        provenance: prov(),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym(id)),
        title: id.into(),
        action: "review".into(),
        time_horizon: "intraday".into(),
        confidence,
        confidence_gap: dec!(0.20),
        heuristic_edge: confidence * dec!(0.20),
        convergence_score: None,
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: String::new(),
        causal_narrative: None,
        risk_notes: vec![format!("family={family}")],
        review_reason_code: None,
        policy_verdict: None,
    };

    let stats = UsLineageStats {
        by_template: vec![
            crate::us::temporal::lineage::UsLineageContextStats {
                template: TEMPLATE_MOMENTUM_CONTINUATION.into(),
                session: String::new(),
                market_regime: String::new(),
                total: 32,
                resolved: 32,
                hits: 11,
                hit_rate: dec!(0.34),
                mean_return: dec!(-0.015),
                follow_expectancy: dec!(-0.015),
                fade_expectancy: dec!(0.008),
                wait_expectancy: Decimal::ZERO,
            },
            crate::us::temporal::lineage::UsLineageContextStats {
                template: TEMPLATE_SECTOR_ROTATION.into(),
                session: String::new(),
                market_regime: String::new(),
                total: 8,
                resolved: 8,
                hits: 5,
                hit_rate: dec!(0.62),
                mean_return: dec!(0.01),
                follow_expectancy: dec!(0.01),
                fade_expectancy: dec!(-0.003),
                wait_expectancy: Decimal::ZERO,
            },
        ],
        by_context: vec![],
    };

    let setups = apply_us_case_budget(
        vec![
            make_setup("M1.US", TEMPLATE_MOMENTUM_CONTINUATION, dec!(0.82)),
            make_setup("M2.US", TEMPLATE_MOMENTUM_CONTINUATION, dec!(0.80)),
            make_setup("M3.US", TEMPLATE_MOMENTUM_CONTINUATION, dec!(0.78)),
            make_setup("R1.US", TEMPLATE_SECTOR_ROTATION, dec!(0.74)),
        ],
        &HashMap::new(),
        Some(&stats),
    );

    let momentum_attention = setups
        .iter()
        .filter(|setup| {
            setup.action != "observe"
                && setup_family_key(setup) == Some(TEMPLATE_MOMENTUM_CONTINUATION)
        })
        .count();
    assert!(momentum_attention <= 1);
}

#[test]
fn surface_diversification_caps_pre_market_front_row() {
    let make_setup = |id: &str, family: &str, edge: Decimal| TacticalSetup {
        setup_id: id.into(),
        hypothesis_id: id.into(),
        runner_up_hypothesis_id: None,
        provenance: prov(),
        lineage: DecisionLineage::default(),
        scope: ReasoningScope::Symbol(sym(id)),
        title: id.into(),
        action: "observe".into(),
        time_horizon: "intraday".into(),
        confidence: edge,
        confidence_gap: edge,
        heuristic_edge: edge,
        convergence_score: None,
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: String::new(),
        causal_narrative: None,
        risk_notes: vec![format!("family={family}")],
        review_reason_code: None,
        policy_verdict: None,
    };

    let setups = diversify_us_case_surface(
        vec![
            make_setup("P1.US", TEMPLATE_PRE_MARKET_POSITIONING, dec!(0.95)),
            make_setup("P2.US", TEMPLATE_PRE_MARKET_POSITIONING, dec!(0.94)),
            make_setup("P3.US", TEMPLATE_PRE_MARKET_POSITIONING, dec!(0.93)),
            make_setup("P4.US", TEMPLATE_PRE_MARKET_POSITIONING, dec!(0.92)),
            make_setup("D1.US", TEMPLATE_STRUCTURAL_DIFFUSION, dec!(0.91)),
            make_setup("D2.US", TEMPLATE_STRUCTURAL_DIFFUSION, dec!(0.90)),
            make_setup("C1.US", TEMPLATE_CROSS_MARKET_ARBITRAGE, dec!(0.89)),
        ],
        None,
    );

    let front = &setups[..setups.len().min(5)];
    let pre_market_front = front
        .iter()
        .filter(|setup| setup_family_key(setup) == Some(TEMPLATE_PRE_MARKET_POSITIONING))
        .count();
    assert!(pre_market_front <= 2);
    assert!(front
        .iter()
        .any(|setup| setup_family_key(setup) == Some(TEMPLATE_STRUCTURAL_DIFFUSION)));
}

#[test]
fn market_scope_does_not_bleed_into_symbol_scope() {
    assert!(!scope_matches(
        &ReasoningScope::market(),
        &ReasoningScope::Symbol(sym("AAPL.US"))
    ));
    assert!(!scope_matches(
        &ReasoningScope::Symbol(sym("AAPL.US")),
        &ReasoningScope::market()
    ));
    assert!(scope_matches(
        &ReasoningScope::Symbol(sym("AAPL.US")),
        &ReasoningScope::Symbol(sym("AAPL.US"))
    ));
}
