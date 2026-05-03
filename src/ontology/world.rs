use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use super::{
    reasoning::{IntentHypothesis, ReasoningScope},
    ProvenanceMetadata,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldLayer {
    Leaf,
    Branch,
    Trunk,
    Forest,
}

impl WorldLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Leaf => "leaf",
            Self::Branch => "branch",
            Self::Trunk => "trunk",
            Self::Forest => "forest",
        }
    }
}

impl std::fmt::Display for WorldLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CausalContestState {
    #[default]
    New,
    Stable,
    Eroding,
    Flipped,
    Contested,
}

impl CausalContestState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Stable => "stable",
            Self::Eroding => "eroding",
            Self::Flipped => "flipped",
            Self::Contested => "contested",
        }
    }
}

impl std::fmt::Display for CausalContestState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: String,
    pub scope: ReasoningScope,
    pub layer: WorldLayer,
    pub provenance: ProvenanceMetadata,
    pub label: String,
    pub regime: String,
    pub confidence: Decimal,
    pub local_support: Decimal,
    pub propagated_support: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drivers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerceptualEvidencePolarity {
    Supports,
    Contradicts,
    Missing,
}

impl PerceptualEvidencePolarity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Missing => "missing",
        }
    }
}

impl std::fmt::Display for PerceptualEvidencePolarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualEvidence {
    pub evidence_id: String,
    pub target_scope: ReasoningScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope: Option<ReasoningScope>,
    pub channel: String,
    pub polarity: PerceptualEvidencePolarity,
    pub weight: Decimal,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerceptualExpectationKind {
    PeerFollowThrough,
    RawChannelConfirmation,
    ClusterExpansion,
    PropagationFollowThrough,
}

impl PerceptualExpectationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PeerFollowThrough => "peer_follow_through",
            Self::RawChannelConfirmation => "raw_channel_confirmation",
            Self::ClusterExpansion => "cluster_expansion",
            Self::PropagationFollowThrough => "propagation_follow_through",
        }
    }
}

impl std::fmt::Display for PerceptualExpectationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerceptualExpectationStatus {
    Met,
    StillPending,
    Missed,
}

impl PerceptualExpectationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Met => "met",
            Self::StillPending => "still_pending",
            Self::Missed => "missed",
        }
    }
}

impl std::fmt::Display for PerceptualExpectationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualExpectation {
    pub expectation_id: String,
    pub target_scope: ReasoningScope,
    pub kind: PerceptualExpectationKind,
    pub status: PerceptualExpectationStatus,
    pub rationale: String,
    pub pending_ticks: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttentionAllocation {
    pub allocation_id: String,
    pub target_scope: ReasoningScope,
    pub channel: String,
    pub weight: Decimal,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualUncertainty {
    pub uncertainty_id: String,
    pub target_scope: ReasoningScope,
    pub level: Decimal,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptualState {
    pub state_id: String,
    pub scope: ReasoningScope,
    pub label: String,
    pub state_kind: String,
    pub trend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub age_ticks: u64,
    pub persistence_ticks: u16,
    pub direction_continuity_ticks: u16,
    pub confidence: Decimal,
    pub strength: Decimal,
    pub support_count: usize,
    pub contradict_count: usize,
    pub count_support_fraction: Decimal,
    pub weighted_support_fraction: Decimal,
    pub support_weight: Decimal,
    pub contradict_weight: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_evidence: Vec<PerceptualEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opposing_evidence: Vec<PerceptualEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_evidence: Vec<PerceptualEvidence>,
    pub conflict_age_ticks: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectations: Vec<PerceptualExpectation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention_allocations: Vec<AttentionAllocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uncertainties: Vec<PerceptualUncertainty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_setup_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_state: Option<String>,
    pub cluster_key: String,
    pub cluster_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
}

/// A convergence point where multiple causal flow paths meet.
/// Vortices emerge when independent leaf-level events (price moves, volume spikes,
/// broker activity) feed into the same branch/trunk-level entity through different
/// channels. The more independent paths converge, the stronger the vortex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vortex {
    pub vortex_id: String,
    pub center_entity_id: String,
    pub center_scope: ReasoningScope,
    pub layer: WorldLayer,
    pub flow_paths: Vec<FlowPath>,
    pub strength: Decimal,
    pub channel_diversity: usize,
    pub coherence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative: Option<String>,
}

/// A single causal flow path feeding into a vortex center.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowPath {
    pub source_entity_id: String,
    pub source_scope: ReasoningScope,
    pub channel: String,
    pub weight: Decimal,
    pub polarity: FlowPolarity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowPolarity {
    Confirming,
    Contradicting,
    Ambiguous,
}

impl FlowPolarity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirming => "confirming",
            Self::Contradicting => "contradicting",
            Self::Ambiguous => "ambiguous",
        }
    }
}

impl std::fmt::Display for FlowPolarity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldStateSnapshot {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub entities: Vec<EntityState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub world_intents: Vec<IntentHypothesis>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub perceptual_states: Vec<PerceptualState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vortices: Vec<Vortex>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackwardEvidenceItem {
    pub statement: String,
    pub weight: Decimal,
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackwardCause {
    pub cause_id: String,
    pub scope: ReasoningScope,
    pub layer: WorldLayer,
    pub depth: u8,
    #[serde(default)]
    pub provenance: ProvenanceMetadata,
    pub explanation: String,
    pub chain_summary: Option<String>,
    pub confidence: Decimal,
    #[serde(default)]
    pub support_weight: Decimal,
    #[serde(default)]
    pub contradict_weight: Decimal,
    #[serde(default)]
    pub net_conviction: Decimal,
    #[serde(default)]
    pub competitive_score: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falsifier: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_evidence: Vec<BackwardEvidenceItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradicting_evidence: Vec<BackwardEvidenceItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackwardInvestigation {
    pub investigation_id: String,
    pub leaf_scope: ReasoningScope,
    pub leaf_label: String,
    pub leaf_regime: String,
    #[serde(default)]
    pub contest_state: CausalContestState,
    #[serde(default)]
    pub leading_cause_streak: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_leading_cause_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_cause: Option<BackwardCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_up_cause: Option<BackwardCause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause_gap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_support_delta: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_contradict_delta: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_transition_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_falsifier: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_causes: Vec<BackwardCause>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackwardReasoningSnapshot {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub investigations: Vec<BackwardInvestigation>,
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use serde_json::json;
    use time::OffsetDateTime;

    use super::*;
    use crate::ontology::Symbol;

    #[test]
    fn world_layer_string_is_stable() {
        assert_eq!(WorldLayer::Forest.as_str(), "forest");
        assert_eq!(WorldLayer::Leaf.to_string(), "leaf");
    }

    #[test]
    fn backward_investigation_holds_leaf_and_causes() {
        let investigation = BackwardInvestigation {
            investigation_id: "backward:700.HK".into(),
            leaf_scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            leaf_label: "Long 700.HK".into(),
            leaf_regime: "flow-led".into(),
            contest_state: CausalContestState::Stable,
            leading_cause_streak: 3,
            previous_leading_cause_id: Some("cause:market:700.HK".into()),
            leading_cause: Some(BackwardCause {
                cause_id: "cause:market:700.HK".into(),
                scope: ReasoningScope::market(),
                layer: WorldLayer::Forest,
                depth: 2,
                provenance: ProvenanceMetadata::new(
                    crate::ontology::ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                )
                .with_trace_id("cause:market:700.HK")
                .with_inputs(["path:market_stress:tech"]),
                explanation: "market stress regime is dominating risk repricing".into(),
                chain_summary: Some("leaf -> sector -> market".into()),
                confidence: dec!(0.61),
                support_weight: dec!(0.52),
                contradict_weight: dec!(0.11),
                net_conviction: dec!(0.41),
                competitive_score: dec!(0.66),
                falsifier: Some("market stress stops dominating the tape".into()),
                supporting_evidence: vec![BackwardEvidenceItem {
                    statement: "market stress remains elevated".into(),
                    weight: dec!(0.52),
                    channel: "market-driver".into(),
                }],
                contradicting_evidence: vec![BackwardEvidenceItem {
                    statement: "idiosyncratic local bid still resists".into(),
                    weight: dec!(0.11),
                    channel: "local-counter".into(),
                }],
                references: vec!["path:market_stress:tech".into()],
            }),
            runner_up_cause: None,
            cause_gap: None,
            leading_support_delta: Some(dec!(0.05)),
            leading_contradict_delta: Some(dec!(-0.02)),
            leader_transition_summary: Some("leader remains market with widening edge".into()),
            leading_falsifier: Some("market stress stops dominating the tape".into()),
            candidate_causes: vec![BackwardCause {
                cause_id: "cause:market:700.HK".into(),
                scope: ReasoningScope::market(),
                layer: WorldLayer::Forest,
                depth: 2,
                provenance: ProvenanceMetadata::new(
                    crate::ontology::ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                )
                .with_trace_id("cause:market:700.HK")
                .with_inputs(["path:market_stress:tech"]),
                explanation: "market stress regime is dominating risk repricing".into(),
                chain_summary: Some("leaf -> sector -> market".into()),
                confidence: dec!(0.61),
                support_weight: dec!(0.52),
                contradict_weight: dec!(0.11),
                net_conviction: dec!(0.41),
                competitive_score: dec!(0.66),
                falsifier: Some("market stress stops dominating the tape".into()),
                supporting_evidence: vec![BackwardEvidenceItem {
                    statement: "market stress remains elevated".into(),
                    weight: dec!(0.52),
                    channel: "market-driver".into(),
                }],
                contradicting_evidence: vec![BackwardEvidenceItem {
                    statement: "idiosyncratic local bid still resists".into(),
                    weight: dec!(0.11),
                    channel: "local-counter".into(),
                }],
                references: vec!["path:market_stress:tech".into()],
            }],
        };

        let snapshot = WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![EntityState {
                entity_id: "state:700.HK".into(),
                scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
                layer: WorldLayer::Leaf,
                provenance: ProvenanceMetadata::new(
                    crate::ontology::ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                )
                .with_trace_id("state:700.HK")
                .with_inputs(["setup:700.HK:review"]),
                label: "Long 700.HK".into(),
                regime: "flow-led".into(),
                confidence: dec!(0.64),
                local_support: dec!(0.40),
                propagated_support: dec!(0.18),
                drivers: vec!["local flow stayed positive".into()],
            }],
            world_intents: vec![],
            perceptual_states: vec![],
            vortices: vec![],
        };

        assert_eq!(investigation.candidate_causes.len(), 1);
        assert!(investigation.leading_cause.is_some());
        assert_eq!(investigation.contest_state, CausalContestState::Stable);
        assert_eq!(investigation.leading_cause_streak, 3);
        assert_eq!(
            investigation.leading_falsifier.as_deref(),
            Some("market stress stops dominating the tape")
        );
        assert_eq!(
            investigation
                .leading_cause
                .as_ref()
                .map(|cause| cause.net_conviction),
            Some(dec!(0.41))
        );
        assert_eq!(snapshot.entities[0].layer, WorldLayer::Leaf);
    }

    #[test]
    fn backward_cause_deserializes_old_payload_without_competition_fields() {
        let cause: BackwardCause = serde_json::from_value(json!({
            "cause_id": "cause:market:700.HK",
            "scope": "Market",
            "layer": "Forest",
            "depth": 2,
            "explanation": "market stress regime is dominating risk repricing",
            "chain_summary": "leaf -> sector -> market",
            "confidence": "0.61",
            "support_weight": "0.52",
            "references": ["path:market_stress:tech"]
        }))
        .expect("deserialize old backward cause payload");

        assert_eq!(cause.competitive_score, Decimal::ZERO);
        assert_eq!(cause.contradict_weight, Decimal::ZERO);
        assert!(cause.supporting_evidence.is_empty());
        assert!(cause.falsifier.is_none());
    }

    #[test]
    fn backward_investigation_deserializes_old_payload_without_contest_memory() {
        let investigation: BackwardInvestigation = serde_json::from_value(json!({
            "investigation_id": "backward:700.HK",
            "leaf_scope": { "Symbol": "700.HK" },
            "leaf_label": "Long 700.HK",
            "leaf_regime": "review",
            "candidate_causes": []
        }))
        .expect("deserialize old backward investigation payload");

        assert_eq!(investigation.contest_state, CausalContestState::New);
        assert_eq!(investigation.leading_cause_streak, 0);
        assert!(investigation.previous_leading_cause_id.is_none());
        assert!(investigation.leader_transition_summary.is_none());
    }
}
