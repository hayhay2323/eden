use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    DecisionLineage, EvidencePolarity, Hypothesis, InvalidationCondition, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope, TacticalSetup,
};

use super::signals::{
    UsDerivedSignalKind, UsDerivedSignalSnapshot, UsEventKind, UsEventSnapshot, UsSignalScope,
};

// ── Hypothesis template keys ──

const TEMPLATE_PRE_MARKET_POSITIONING: &str = "pre_market_positioning";
const TEMPLATE_CROSS_MARKET_ARBITRAGE: &str = "cross_market_arbitrage";
const TEMPLATE_MOMENTUM_CONTINUATION: &str = "momentum_continuation";
const TEMPLATE_SECTOR_ROTATION: &str = "sector_rotation";

// ── Template definition ──

struct HypothesisTemplate {
    key: &'static str,
    family_label: &'static str,
    thesis: &'static str,
    invalidation: &'static str,
    expected_observations: &'static [&'static str],
}

const TEMPLATES: &[HypothesisTemplate] = &[
    HypothesisTemplate {
        key: TEMPLATE_PRE_MARKET_POSITIONING,
        family_label: "Pre-Market Positioning",
        thesis: "pre-market move reflects institutional positioning before regular hours",
        invalidation: "capital flow during regular hours moves opposite to pre-market direction",
        expected_observations: &[
            "gap should hold through first 30 minutes",
            "volume should confirm direction",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_CROSS_MARKET_ARBITRAGE,
        family_label: "Cross-Market Arbitrage",
        thesis: "may follow HK counterpart's institutional-driven move",
        invalidation: "US capital flow moves opposite to HK signal",
        expected_observations: &[
            "price should converge toward HK-implied level",
            "arbitrage spread should narrow",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_MOMENTUM_CONTINUATION,
        family_label: "Momentum Continuation",
        thesis: "capital flow momentum suggests continuation",
        invalidation: "valuation extreme reached or flow direction reverses",
        expected_observations: &[
            "flow direction should persist",
            "volume should remain elevated",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_SECTOR_ROTATION,
        family_label: "Sector Rotation",
        thesis: "sector is gaining/losing relative to the broader market",
        invalidation: "individual stock diverges strongly from sector trend",
        expected_observations: &[
            "multiple stocks in the sector should move together",
            "sector ETF should confirm direction",
        ],
    },
];

// ── Public snapshot ──

#[derive(Debug, Clone)]
pub struct UsReasoningSnapshot {
    pub timestamp: OffsetDateTime,
    pub hypotheses: Vec<Hypothesis>,
    pub tactical_setups: Vec<TacticalSetup>,
}

impl UsReasoningSnapshot {
    pub fn derive(
        events: &UsEventSnapshot,
        derived_signals: &UsDerivedSignalSnapshot,
        previous_setups: &[TacticalSetup],
    ) -> Self {
        let hypotheses = derive_hypotheses(events, derived_signals);
        let tactical_setups = derive_tactical_setups(&hypotheses, previous_setups);

        Self {
            timestamp: events.timestamp,
            hypotheses,
            tactical_setups,
        }
    }
}

// ── Hypothesis derivation ──

fn derive_hypotheses(
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
) -> Vec<Hypothesis> {
    let mut scopes: Vec<ReasoningScope> = Vec::new();
    for event in &events.events {
        let scope = convert_scope(&event.value.scope);
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    for signal in &derived_signals.signals {
        let scope = convert_scope(&signal.value.scope);
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }

    let mut hypotheses = Vec::new();

    for scope in &scopes {
        for template in TEMPLATES {
            if !template_applicable(template, scope, events, derived_signals) {
                continue;
            }

            let evidence = gather_evidence(template, scope, events, derived_signals);
            let support_count = evidence
                .iter()
                .filter(|e| e.polarity == EvidencePolarity::Supports)
                .count();
            if support_count == 0 {
                continue;
            }

            let summary = summarize_evidence(&evidence);
            let confidence = competing_confidence(&evidence);

            // ── Primary hypothesis: the template's thesis ──
            hypotheses.push(Hypothesis {
                hypothesis_id: format!("hyp:{}:{}", scope_id(scope), template.key),
                family_key: template.key.to_string(),
                family_label: template.family_label.to_string(),
                provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, events.timestamp)
                    .with_trace_id(format!("hyp:{}:{}", scope_id(scope), template.key))
                    .with_inputs(
                        evidence
                            .iter()
                            .flat_map(|e| e.references.clone())
                            .collect::<Vec<_>>(),
                    ),
                scope: scope.clone(),
                statement: format!("{} {}", scope_label(scope), template.thesis),
                confidence,
                local_support_weight: summary.local_support,
                local_contradict_weight: summary.local_contradict,
                propagated_support_weight: summary.propagated_support,
                propagated_contradict_weight: summary.propagated_contradict,
                evidence: evidence.clone(),
                invalidation_conditions: vec![InvalidationCondition {
                    description: template.invalidation.to_string(),
                    references: vec![],
                }],
                propagation_path_ids: vec![],
                expected_observations: template
                    .expected_observations
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            });

            // ── Counter-hypothesis: flip all polarities ──
            // If the primary says "momentum will continue", the counter says
            // "momentum will reverse". Evidence that supports the primary
            // contradicts the counter, creating natural competition.
            let counter_evidence: Vec<ReasoningEvidence> = evidence
                .iter()
                .map(|e| ReasoningEvidence {
                    polarity: match e.polarity {
                        EvidencePolarity::Supports => EvidencePolarity::Contradicts,
                        EvidencePolarity::Contradicts => EvidencePolarity::Supports,
                    },
                    ..e.clone()
                })
                .collect();
            let counter_summary = summarize_evidence(&counter_evidence);
            let counter_confidence = competing_confidence(&counter_evidence);

            // Always emit the counter — "no contradicting evidence" ≠ "certainly correct".
            // The counter represents structural uncertainty: the fewer evidence sources
            // you have, the more weight the counter carries.
            // Base uncertainty: 1/(1 + evidence_count). More evidence → smaller counter.
            let evidence_count = Decimal::from(evidence.len() as i64);
            let base_uncertainty = Decimal::ONE / (Decimal::ONE + evidence_count);
            let counter_confidence = counter_confidence.max(base_uncertainty);
            {
                hypotheses.push(Hypothesis {
                    hypothesis_id: format!("hyp:{}:{}:counter", scope_id(scope), template.key),
                    family_key: format!("{}_reversal", template.key),
                    family_label: format!("{} Reversal", template.family_label),
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        events.timestamp,
                    )
                    .with_trace_id(format!(
                        "hyp:{}:{}:counter",
                        scope_id(scope),
                        template.key
                    )),
                    scope: scope.clone(),
                    statement: format!(
                        "{} counter-thesis: {} may reverse",
                        scope_label(scope),
                        template.family_label,
                    ),
                    confidence: counter_confidence,
                    local_support_weight: counter_summary.local_support,
                    local_contradict_weight: counter_summary.local_contradict,
                    propagated_support_weight: counter_summary.propagated_support,
                    propagated_contradict_weight: counter_summary.propagated_contradict,
                    evidence: counter_evidence,
                    invalidation_conditions: vec![InvalidationCondition {
                        description: format!(
                            "{} thesis holds — no reversal",
                            template.family_label
                        ),
                        references: vec![],
                    }],
                    propagation_path_ids: vec![],
                    expected_observations: vec![format!(
                        "{} signal should weaken",
                        template.family_label
                    )],
                });
            }
        }
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
    });
    hypotheses
}

// ── Template applicability ──

fn template_applicable(
    template: &HypothesisTemplate,
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
) -> bool {
    match template.key {
        TEMPLATE_PRE_MARKET_POSITIONING => has_event_for_scope(
            events,
            scope,
            &[UsEventKind::PreMarketDislocation, UsEventKind::GapOpen],
        ),
        TEMPLATE_CROSS_MARKET_ARBITRAGE => {
            has_event_for_scope(events, scope, &[UsEventKind::CrossMarketDivergence])
                || has_signal_for_scope(
                    derived_signals,
                    scope,
                    &[UsDerivedSignalKind::CrossMarketPropagation],
                )
        }
        TEMPLATE_MOMENTUM_CONTINUATION => {
            // Require cross-validation: at least one event AND one derived signal.
            // StructuralComposite alone is not enough — it fires for nearly all stocks.
            // This mirrors HK's multi-source convergence requirement.
            let has_event = has_event_for_scope(
                events,
                scope,
                &[UsEventKind::CapitalFlowReversal, UsEventKind::VolumeSpike],
            );
            let has_signal = has_signal_for_scope(
                derived_signals,
                scope,
                &[UsDerivedSignalKind::StructuralComposite],
            );
            has_event && has_signal
        }
        TEMPLATE_SECTOR_ROTATION => {
            has_event_for_scope(events, scope, &[UsEventKind::SectorMomentumShift])
                || matches!(scope, ReasoningScope::Sector(_))
        }
        _ => false,
    }
}

fn has_event_for_scope(
    events: &UsEventSnapshot,
    scope: &ReasoningScope,
    kinds: &[UsEventKind],
) -> bool {
    events.events.iter().any(|e| {
        scope_matches(&convert_scope(&e.value.scope), scope) && event_kind_in(&e.value.kind, kinds)
    })
}

fn has_signal_for_scope(
    signals: &UsDerivedSignalSnapshot,
    scope: &ReasoningScope,
    kinds: &[UsDerivedSignalKind],
) -> bool {
    signals.signals.iter().any(|s| {
        scope_matches(&convert_scope(&s.value.scope), scope) && signal_kind_in(&s.value.kind, kinds)
    })
}

fn event_kind_in(kind: &UsEventKind, kinds: &[UsEventKind]) -> bool {
    kinds
        .iter()
        .any(|k| std::mem::discriminant(k) == std::mem::discriminant(kind))
}

fn signal_kind_in(kind: &UsDerivedSignalKind, kinds: &[UsDerivedSignalKind]) -> bool {
    kinds
        .iter()
        .any(|k| std::mem::discriminant(k) == std::mem::discriminant(kind))
}

// ── Evidence gathering ──

fn gather_evidence(
    template: &HypothesisTemplate,
    scope: &ReasoningScope,
    events: &UsEventSnapshot,
    derived_signals: &UsDerivedSignalSnapshot,
) -> Vec<ReasoningEvidence> {
    let mut evidence = Vec::new();

    for event in &events.events {
        if !scope_matches(&convert_scope(&event.value.scope), scope) {
            continue;
        }
        if let Some(polarity) = event_polarity(template.key, &event.value.kind) {
            evidence.push(ReasoningEvidence {
                statement: event.value.summary.clone(),
                kind: ReasoningEvidenceKind::LocalEvent,
                polarity,
                weight: event.value.magnitude.min(Decimal::ONE),
                references: event.provenance.inputs.clone(),
                provenance: event.provenance.clone(),
            });
        }
    }

    for signal in &derived_signals.signals {
        if !scope_matches(&convert_scope(&signal.value.scope), scope) {
            continue;
        }
        if let Some(polarity) = signal_polarity(template.key, &signal.value.kind) {
            evidence.push(ReasoningEvidence {
                statement: signal.value.summary.clone(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity,
                weight: signal.value.strength.abs().min(Decimal::ONE),
                references: signal.provenance.inputs.clone(),
                provenance: signal.provenance.clone(),
            });
        }
    }

    evidence
}

fn event_polarity(template_key: &str, kind: &UsEventKind) -> Option<EvidencePolarity> {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => match kind {
            UsEventKind::PreMarketDislocation | UsEventKind::GapOpen => {
                Some(EvidencePolarity::Supports)
            }
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_CROSS_MARKET_ARBITRAGE => match kind {
            UsEventKind::CrossMarketDivergence => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_MOMENTUM_CONTINUATION => match kind {
            UsEventKind::VolumeSpike => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            UsEventKind::PreMarketDislocation | UsEventKind::GapOpen => {
                Some(EvidencePolarity::Contradicts)
            }
            _ => None,
        },
        TEMPLATE_SECTOR_ROTATION => match kind {
            UsEventKind::SectorMomentumShift => Some(EvidencePolarity::Supports),
            UsEventKind::CapitalFlowReversal => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        _ => None,
    }
}

fn signal_polarity(template_key: &str, kind: &UsDerivedSignalKind) -> Option<EvidencePolarity> {
    match template_key {
        TEMPLATE_PRE_MARKET_POSITIONING => match kind {
            UsDerivedSignalKind::PreMarketConviction => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_CROSS_MARKET_ARBITRAGE => match kind {
            UsDerivedSignalKind::CrossMarketPropagation => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_MOMENTUM_CONTINUATION => match kind {
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            UsDerivedSignalKind::PreMarketConviction => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        TEMPLATE_SECTOR_ROTATION => match kind {
            UsDerivedSignalKind::StructuralComposite => Some(EvidencePolarity::Supports),
            UsDerivedSignalKind::ValuationExtreme => Some(EvidencePolarity::Contradicts),
            _ => None,
        },
        _ => None,
    }
}

// ── Tactical setups ──

fn derive_tactical_setups(
    hypotheses: &[Hypothesis],
    previous_setups: &[TacticalSetup],
) -> Vec<TacticalSetup> {
    let previous_lookup: HashMap<&str, &TacticalSetup> = previous_setups
        .iter()
        .map(|s| (s.hypothesis_id.as_str(), s))
        .collect();

    let mut scope_ranked: HashMap<String, Vec<&Hypothesis>> = HashMap::new();
    for hyp in hypotheses {
        scope_ranked
            .entry(scope_id(&hyp.scope))
            .or_default()
            .push(hyp);
    }

    let mut setups = Vec::new();
    for (_, ranked) in &scope_ranked {
        if ranked.is_empty() {
            continue;
        }
        let winner = ranked[0];
        let runner_up = ranked.get(1);
        let gap = if let Some(ru) = runner_up {
            winner.confidence - ru.confidence
        } else {
            Decimal::ONE
        };

        let action = if gap < Decimal::new(1, 1) {
            "review"
        } else if winner.confidence >= Decimal::new(6, 1) {
            "enter"
        } else {
            "observe"
        };

        // Upgrade if previous setup was "observe" and confidence grew
        let action = if action == "observe" {
            if let Some(prev) = previous_lookup.get(winner.hypothesis_id.as_str()) {
                if winner.confidence > prev.confidence {
                    "review"
                } else {
                    action
                }
            } else {
                action
            }
        } else {
            action
        };

        let edge = gap * winner.confidence;

        setups.push(TacticalSetup {
            setup_id: format!("setup:{}:{}", scope_id(&winner.scope), action),
            hypothesis_id: winner.hypothesis_id.clone(),
            runner_up_hypothesis_id: runner_up.map(|h| h.hypothesis_id.clone()),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                winner.provenance.observed_at,
            )
            .with_trace_id(format!("setup:{}:{}", scope_id(&winner.scope), action))
            .with_inputs([
                winner.hypothesis_id.clone(),
                format!("confidence:{}", winner.confidence),
            ]),
            lineage: DecisionLineage {
                based_on: vec![winner.hypothesis_id.clone()],
                blocked_by: vec![],
                promoted_by: if action == "enter" {
                    vec![format!("confidence_gap={}", gap)]
                } else {
                    vec![]
                },
                falsified_by: winner
                    .invalidation_conditions
                    .iter()
                    .map(|ic| ic.description.clone())
                    .collect(),
            },
            scope: winner.scope.clone(),
            title: format!("{} — {}", scope_label(&winner.scope), winner.family_label),
            action: action.into(),
            time_horizon: "intraday".into(),
            confidence: winner.confidence,
            confidence_gap: gap,
            heuristic_edge: edge,
            workflow_id: None,
            entry_rationale: winner.statement.clone(),
            risk_notes: winner
                .invalidation_conditions
                .iter()
                .map(|ic| ic.description.clone())
                .collect(),
        });
    }

    setups.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.setup_id.cmp(&b.setup_id))
    });
    setups
}

// ── Helpers ──

fn convert_scope(scope: &UsSignalScope) -> ReasoningScope {
    match scope {
        UsSignalScope::Market => ReasoningScope::Market,
        UsSignalScope::Symbol(s) => ReasoningScope::Symbol(s.clone()),
        UsSignalScope::Sector(s) => ReasoningScope::Sector(s.clone()),
    }
}

fn scope_matches(a: &ReasoningScope, b: &ReasoningScope) -> bool {
    a == b || matches!(a, ReasoningScope::Market) || matches!(b, ReasoningScope::Market)
}

fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector:{}", s),
        ReasoningScope::Institution(s) => format!("inst:{}", s),
        ReasoningScope::Theme(s) => format!("theme:{}", s),
        ReasoningScope::Region(s) => format!("region:{}", s),
        ReasoningScope::Custom(s) => format!("custom:{}", s),
    }
}

fn scope_label(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "US market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector {}", s),
        ReasoningScope::Institution(s) => s.clone(),
        ReasoningScope::Theme(s) => s.clone(),
        ReasoningScope::Region(s) => s.clone(),
        ReasoningScope::Custom(s) => s.clone(),
    }
}

struct EvidenceSummary {
    local_support: Decimal,
    local_contradict: Decimal,
    propagated_support: Decimal,
    propagated_contradict: Decimal,
}

fn summarize_evidence(evidence: &[ReasoningEvidence]) -> EvidenceSummary {
    let mut summary = EvidenceSummary {
        local_support: Decimal::ZERO,
        local_contradict: Decimal::ZERO,
        propagated_support: Decimal::ZERO,
        propagated_contradict: Decimal::ZERO,
    };
    for item in evidence {
        match (item.polarity, item.kind) {
            (EvidencePolarity::Supports, ReasoningEvidenceKind::PropagatedPath) => {
                summary.propagated_support += item.weight;
            }
            (EvidencePolarity::Contradicts, ReasoningEvidenceKind::PropagatedPath) => {
                summary.propagated_contradict += item.weight;
            }
            (EvidencePolarity::Supports, _) => {
                summary.local_support += item.weight;
            }
            (EvidencePolarity::Contradicts, _) => {
                summary.local_contradict += item.weight;
            }
        }
    }
    summary
}

fn competing_confidence(evidence: &[ReasoningEvidence]) -> Decimal {
    // Same formula as HK: (support - contradict) / total, mapped to [0, 1].
    // No artificial prior — confidence is purely data-driven.
    // Differentiation comes from confidence_gap between competing hypotheses.
    let summary = summarize_evidence(evidence);
    let total_support = summary.local_support + summary.propagated_support;
    let total_contradict = summary.local_contradict + summary.propagated_contradict;
    let total = total_support + total_contradict;
    if total == Decimal::ZERO {
        return Decimal::ZERO;
    }
    (((total_support - total_contradict) / total + Decimal::ONE) / Decimal::TWO)
        .clamp(Decimal::ZERO, Decimal::ONE)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{DerivedSignal, Event};
    use crate::ontology::objects::Symbol;
    use rust_decimal_macros::dec;

    use crate::us::pipeline::signals::{UsDerivedSignalRecord, UsEventRecord};

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
            &signals
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
            &signals
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
            &empty_signals
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
            &signals
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
            &signals
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
        assert!(snap
            .hypotheses
            .iter()
            .any(|h| h.family_key == TEMPLATE_CROSS_MARKET_ARBITRAGE));
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
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

        let snap = UsReasoningSnapshot::derive(&events, &signals, &[]);
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
}
