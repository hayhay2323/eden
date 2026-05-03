use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::agent::AgentDecision;
use crate::cases::CaseSummary;
use crate::live_snapshot::LiveMarket;
use crate::ontology::{
    backward_cause_knowledge_node_ref, decision_knowledge_node_ref, knowledge_node_ref,
    mechanism_node_id, position_knowledge_node_ref, scope_node_id, scope_node_label,
    sector_knowledge_node_ref, setup_node_id, symbol_knowledge_node_ref,
    world_entity_knowledge_node_ref, ActionDirection, ActionNode, ActionNodeStage,
    AgentKnowledgeEvent, AgentKnowledgeLink, AgentKnowledgeNodeRef, AgentMacroEvent,
    BackwardReasoningSnapshot, EvidenceRef, EvidenceRefKind, Hypothesis, KnowledgeEventAttributes,
    KnowledgeEventKind, KnowledgeLinkAttributes, KnowledgeNodeAttributes, KnowledgeRelation,
    Market, TacticalSetup, WorldStateSnapshot,
};

#[path = "agent_graph/nodes.rs"]
mod nodes;
#[path = "agent_graph/reasoning.rs"]
mod reasoning;
#[path = "agent_graph/records.rs"]
mod records;
#[path = "agent_graph/runtime.rs"]
mod runtime;

pub use nodes::build_knowledge_node_records;
pub use reasoning::{reasoning_knowledge_events, reasoning_knowledge_links};
pub use records::*;
pub use runtime::{build_runtime_knowledge_events, build_runtime_knowledge_links};

pub fn market_slug(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract};
    use crate::agent::{AgentDecision, AgentRecommendation};
    use crate::ontology::reasoning::default_case_horizon;
    use crate::ontology::{
        ActionDirection, ActionNode, ActionNodeStage, AgentEventImpact, AgentKnowledgeNodeRef,
        BackwardCause, BackwardInvestigation, BackwardReasoningSnapshot, CaseReasoningProfile,
        DecisionLineage, EntityState, Hypothesis, Market, MechanismCandidate,
        MechanismCandidateKind, ProvenanceMetadata, ProvenanceSource, ReasoningScope, Symbol,
        TacticalSetup, WorldLayer, WorldStateSnapshot,
    };

    #[test]
    fn macro_event_record_captures_impact_fields() {
        let record = MacroEventHistoryRecord::from_agent_event(
            &AgentMacroEvent {
                event_id: "macro_event:1".into(),
                tick: 12,
                market: LiveMarket::Hk,
                event_type: "rates_macro".into(),
                authority_level: "high".into(),
                headline: "Fed repricing".into(),
                summary: "rates higher".into(),
                confidence: dec!(0.82),
                confirmation_state: "confirmed".into(),
                impact: AgentEventImpact {
                    primary_scope: "market".into(),
                    secondary_scopes: vec![],
                    affected_markets: vec!["hk".into()],
                    affected_sectors: vec!["Property".into()],
                    affected_symbols: vec!["700.HK".into()],
                    preferred_expression: "risk_off".into(),
                    requires_market_confirmation: true,
                    decisive_factors: vec!["yield shock".into()],
                },
                supporting_notice_ids: vec!["notice:1".into()],
                promotion_reasons: vec!["high authority".into()],
            },
            OffsetDateTime::UNIX_EPOCH,
        );

        assert_eq!(record.market, "hk");
        assert_eq!(record.tick_number, 12);
        assert_eq!(record.primary_scope, "market");
        assert_eq!(record.affected_symbols, vec!["700.HK"]);
    }

    #[test]
    fn knowledge_link_record_preserves_endpoints() {
        let record = KnowledgeLinkHistoryRecord::from_agent_link(
            LiveMarket::Us,
            22,
            OffsetDateTime::UNIX_EPOCH,
            &AgentKnowledgeLink {
                link_id: "macro_event:1:symbol:NVDA.US".into(),
                relation: KnowledgeRelation::ImpactsSymbol,
                source: AgentKnowledgeNodeRef {
                    node_kind: "macro_event".into(),
                    node_id: "macro_event:1".into(),
                    label: "Fed repricing".into(),
                },
                target: AgentKnowledgeNodeRef {
                    node_kind: "symbol".into(),
                    node_id: "symbol:NVDA.US".into(),
                    label: "NVDA.US".into(),
                },
                confidence: dec!(0.7),
                attributes: KnowledgeLinkAttributes::ImpactsSymbol {
                    event_type: "rates_macro".into(),
                    authority_level: "high".into(),
                    primary_scope: "market".into(),
                    preferred_expression: "risk_off".into(),
                },
                rationale: Some("rates hit growth".into()),
            },
        );

        assert_eq!(record.market, "us");
        assert_eq!(record.source_node_id, "macro_event:1");
        assert_eq!(record.target_node_id, "symbol:NVDA.US");
    }

    #[test]
    fn structured_knowledge_node_records_capture_reasoning_semantics() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700.HK:flow".into(),
            kind: None,
            family_label: "Flow".into(),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            statement: "flow remains dominant".into(),
            confidence: dec!(0.7),
            local_support_weight: dec!(0.4),
            local_contradict_weight: dec!(0.1),
            propagated_support_weight: dec!(0.2),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:1".into()],
            expected_observations: vec!["follow through".into()],
        };
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.65),
            confidence_gap: dec!(0.15),
            heuristic_edge: dec!(0.08),
            convergence_score: Some(dec!(0.33)),
            convergence_detail: None,
            workflow_id: Some("wf:1".into()),
            entry_rationale: "flow leads".into(),
            causal_narrative: None,
            risk_notes: vec!["watch stress".into()],
            review_reason_code: None,
            policy_verdict: None,
        };
        let case = CaseSummary {
            case_id: "case:1".into(),
            setup_id: setup.setup_id.clone(),
            workflow_id: setup.workflow_id.clone(),
            execution_policy: None,
            owner: None,
            reviewer: None,
            queue_pin: None,
            workflow_actor: None,
            workflow_note: None,
            symbol: "700.HK".into(),
            title: setup.title.clone(),
            sector: Some("Technology".into()),
            market: LiveMarket::Hk,
            recommended_action: "enter".into(),
            workflow_state: "suggest".into(),
            governance: None,
            governance_bucket: "review_required".into(),
            governance_reason_code: None,
            governance_reason: None,
            market_regime_bias: "neutral".into(),
            market_regime_confidence: Decimal::ZERO,
            market_breadth_delta: Decimal::ZERO,
            market_average_return: Decimal::ZERO,
            market_directional_consensus: None,
            confidence: setup.confidence,
            confidence_gap: setup.confidence_gap,
            heuristic_edge: setup.heuristic_edge,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            freshness_state: None,
            first_enter_tick: None,
            ticks_since_first_enter: None,
            ticks_since_first_seen: None,
            timing_state: None,
            timing_position_in_range: None,
            local_state: None,
            local_state_confidence: None,
            actionability_score: None,
            actionability_state: None,
            confidence_velocity_5t: None,
            support_fraction_velocity_5t: None,
            driver_confidence: None,
            absence_summary: None,
            competition_summary: None,
            competition_winner: None,
            competition_runner_up: None,
            priority_rank: None,
            state_persistence_ticks: None,
            direction_stability_rounds: None,
            state_reason_codes: vec![],
            why_now: "why".into(),
            primary_lens: None,
            primary_driver: None,
            family_label: Some("Flow".into()),
            counter_label: None,
            hypothesis_status: None,
            current_leader: None,
            flip_count: 0,
            leader_streak: None,
            case_signature: None,
            archetype_projections: vec![],
            inferred_intent: None,
            intent_opportunities: vec![],
            expectation_binding_count: 0,
            expectation_violation_count: 0,
            key_evidence: vec![],
            invalidation_rules: vec![],
            reasoning_profile: CaseReasoningProfile {
                laws: vec![],
                predicates: vec![],
                composite_states: vec![],
                human_review: None,
                primary_mechanism: Some(MechanismCandidate {
                    kind: MechanismCandidateKind::CapitalRotation,
                    label: "Capital Rotation".into(),
                    score: dec!(0.6),
                    summary: "rotation".into(),
                    supporting_states: vec![],
                    invalidation: vec!["if spread collapses".into()],
                    human_checks: vec!["check peer bid".into()],
                    factors: vec![],
                    counterfactuals: vec![],
                }),
                competing_mechanisms: vec![],
                automated_invalidations: vec![],
            },
            updated_at: "2026-03-25T00:00:00Z".into(),
            case_resolution: None,
            horizon_breakdown: None,
        };
        let decision = AgentDecision::Symbol(AgentRecommendation {
            recommendation_id: "rec:1".into(),
            tick: 12,
            symbol: "700.HK".into(),
            sector: Some("Technology".into()),
            title: Some("Long 700.HK".into()),
            action: "enter".into(),
            action_label: Some("Enter".into()),
            bias: "long".into(),
            severity: "high".into(),
            confidence: dec!(0.7),
            score: dec!(0.75),
            horizon_ticks: 8,
            regime_bias: "neutral".into(),
            status: Some("new".into()),
            why: "why".into(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            watch_next: vec![],
            do_not: vec![],
            fragility: vec![],
            transition: None,
            thesis_family: Some("Flow".into()),
            state_transition: None,
            best_action: "follow".into(),
            action_expectancies: crate::agent::AgentActionExpectancies::default(),
            decision_attribution: crate::agent::AgentDecisionAttribution::default(),
            expected_net_alpha: None,
            alpha_horizon: "intraday:8t".into(),
            price_at_decision: None,
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
            matched_success_pattern_signature: None,
        });

        let (history, _) = build_knowledge_node_records(
            LiveMarket::Hk,
            12,
            OffsetDateTime::UNIX_EPOCH,
            &[],
            &[decision],
            &[hypothesis],
            &[setup],
            &[case],
            None,
            None,
            &[],
            &[],
        );

        assert!(history.iter().any(|record| matches!(
            record.attributes,
            KnowledgeNodeAttributes::Hypothesis { .. }
        )));
        assert!(history
            .iter()
            .any(|record| matches!(record.attributes, KnowledgeNodeAttributes::Setup { .. })));
        assert!(history
            .iter()
            .any(|record| matches!(record.attributes, KnowledgeNodeAttributes::Mechanism { .. })));
        assert!(history
            .iter()
            .any(|record| matches!(record.attributes, KnowledgeNodeAttributes::Decision { .. })));
    }

    #[test]
    fn runtime_knowledge_node_records_capture_world_cause_and_position_semantics() {
        let world = WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![EntityState {
                entity_id: "state:700.HK".into(),
                scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
                layer: WorldLayer::Leaf,
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
                label: "700.HK flow state".into(),
                regime: "flow-led".into(),
                confidence: dec!(0.62),
                local_support: dec!(0.4),
                propagated_support: dec!(0.2),
                drivers: vec!["depth confirms".into()],
            }],
            world_intents: vec![],
            perceptual_states: vec![],
            vortices: vec![],
        };
        let backward = BackwardReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            investigations: vec![BackwardInvestigation {
                investigation_id: "backward:700.HK".into(),
                leaf_scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
                leaf_label: "Long 700.HK".into(),
                leaf_regime: "flow-led".into(),
                contest_state: Default::default(),
                leading_cause_streak: 1,
                previous_leading_cause_id: None,
                leading_cause: Some(BackwardCause {
                    cause_id: "cause:market:700.HK".into(),
                    scope: ReasoningScope::market(),
                    layer: WorldLayer::Forest,
                    depth: 2,
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        OffsetDateTime::UNIX_EPOCH,
                    ),
                    explanation: "market stress dominates".into(),
                    chain_summary: Some("leaf -> market".into()),
                    confidence: dec!(0.7),
                    support_weight: dec!(0.5),
                    contradict_weight: dec!(0.1),
                    net_conviction: dec!(0.4),
                    competitive_score: dec!(0.66),
                    falsifier: Some("stress fades".into()),
                    supporting_evidence: vec![],
                    contradicting_evidence: vec![],
                    references: vec!["ref:1".into()],
                }),
                runner_up_cause: None,
                cause_gap: None,
                leading_support_delta: None,
                leading_contradict_delta: None,
                leader_transition_summary: None,
                leading_falsifier: None,
                candidate_causes: vec![],
            }],
        };
        let positions = vec![ActionNode {
            workflow_id: "wf:700".into(),
            symbol: Symbol("700.HK".into()),
            market: Market::Hk,
            sector: Some("Technology".into()),
            stage: ActionNodeStage::Monitoring,
            direction: ActionDirection::Long,
            entry_confidence: dec!(0.6),
            current_confidence: dec!(0.7),
            entry_price: Some(dec!(95)),
            pnl: Some(dec!(0.03)),
            age_ticks: 12,
            degradation_score: Some(dec!(0.2)),
            exit_forming: false,
        }];

        let runtime_links =
            build_runtime_knowledge_links(Some(&world), Some(&backward), &positions);
        let (history, _) = build_knowledge_node_records(
            LiveMarket::Hk,
            12,
            OffsetDateTime::UNIX_EPOCH,
            &[],
            &[],
            &[],
            &[],
            &[],
            Some(&world),
            Some(&backward),
            &positions,
            &runtime_links,
        );

        assert!(history.iter().any(|record| matches!(
            record.attributes,
            KnowledgeNodeAttributes::WorldEntity { .. }
        )));
        assert!(history.iter().any(|record| matches!(
            record.attributes,
            KnowledgeNodeAttributes::BackwardCause { .. }
        )));
        assert!(history
            .iter()
            .any(|record| matches!(record.attributes, KnowledgeNodeAttributes::Position { .. })));
    }
}
