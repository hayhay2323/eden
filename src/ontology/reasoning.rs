use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use super::{
    CustomScopeId, InstitutionId, Market, MarketScopeId, ProvenanceMetadata, RegionId, SectorId,
    Symbol, ThemeId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DecisionLineage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub based_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promoted_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub falsified_by: Vec<String>,
}

/// Open-ended target for a hypothesis, propagation step, or tactical setup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum ReasoningScope {
    Market(MarketScopeId),
    Symbol(Symbol),
    Sector(SectorId),
    Institution(InstitutionId),
    Theme(ThemeId),
    Region(RegionId),
    Custom(CustomScopeId),
}

impl ReasoningScope {
    pub fn market() -> Self {
        Self::Market(MarketScopeId::default())
    }

    pub fn kind_slug(&self) -> &'static str {
        match self {
            Self::Market(_) => "market",
            Self::Symbol(_) => "symbol",
            Self::Sector(_) => "sector",
            Self::Institution(_) => "institution",
            Self::Theme(_) => "theme",
            Self::Region(_) => "region",
            Self::Custom(_) => "custom",
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Market(market_id) => market_id.to_string(),
            Self::Symbol(symbol) => symbol.0.clone(),
            Self::Sector(sector) => sector.to_string(),
            Self::Institution(institution) => institution.to_string(),
            Self::Theme(theme) => theme.to_string(),
            Self::Region(region) => region.to_string(),
            Self::Custom(value) => value.to_string(),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReasoningScopeWire {
    LegacyString(String),
    Tagged(ReasoningScopeTagged),
}

#[derive(Deserialize)]
enum ReasoningScopeTagged {
    Market(MarketScopeId),
    Symbol(Symbol),
    Sector(SectorId),
    Institution(InstitutionId),
    Theme(ThemeId),
    Region(RegionId),
    Custom(CustomScopeId),
}

impl<'de> Deserialize<'de> for ReasoningScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ReasoningScopeWire::deserialize(deserializer)?;
        match wire {
            ReasoningScopeWire::LegacyString(value) => match value.as_str() {
                "Market" => Ok(Self::market()),
                other => Err(serde::de::Error::unknown_variant(
                    other,
                    &[
                        "Market",
                        "Symbol",
                        "Sector",
                        "Institution",
                        "Theme",
                        "Region",
                        "Custom",
                    ],
                )),
            },
            ReasoningScopeWire::Tagged(tagged) => Ok(match tagged {
                ReasoningScopeTagged::Market(market_id) => Self::Market(market_id),
                ReasoningScopeTagged::Symbol(symbol) => Self::Symbol(symbol),
                ReasoningScopeTagged::Sector(sector) => Self::Sector(sector),
                ReasoningScopeTagged::Institution(institution) => Self::Institution(institution),
                ReasoningScopeTagged::Theme(theme) => Self::Theme(theme),
                ReasoningScopeTagged::Region(region) => Self::Region(region),
                ReasoningScopeTagged::Custom(value) => Self::Custom(value),
            }),
        }
    }
}

/// Whether a piece of evidence supports or contradicts a hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidencePolarity {
    Supports,
    Contradicts,
}

/// Source class for a reasoning evidence item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEvidenceKind {
    LocalEvent,
    LocalSignal,
    PropagatedPath,
}

/// A reusable evidence record that can support or weaken any open-ended hypothesis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningEvidence {
    pub statement: String,
    pub kind: ReasoningEvidenceKind,
    pub polarity: EvidencePolarity,
    pub weight: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
    pub provenance: ProvenanceMetadata,
}

/// A condition that would invalidate the current hypothesis if observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidationCondition {
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

/// One step in a propagation path from one scope to another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropagationStep {
    pub from: ReasoningScope,
    pub to: ReasoningScope,
    pub mechanism: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

/// A candidate transmission route for a market situation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropagationPath {
    pub path_id: String,
    pub summary: String,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<PropagationStep>,
}

/// Open-ended hypothesis container. The statement stays free-form; the ontology
/// only constrains the structure around it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub hypothesis_id: String,
    pub family_key: String,
    pub family_label: String,
    pub provenance: ProvenanceMetadata,
    pub scope: ReasoningScope,
    pub statement: String,
    pub confidence: Decimal,
    pub local_support_weight: Decimal,
    pub local_contradict_weight: Decimal,
    pub propagated_support_weight: Decimal,
    pub propagated_contradict_weight: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ReasoningEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalidation_conditions: Vec<InvalidationCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_path_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_observations: Vec<String>,
}

/// A tactical case distilled from one hypothesis and its current evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TacticalSetup {
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub provenance: ProvenanceMetadata,
    pub lineage: DecisionLineage,
    pub scope: ReasoningScope,
    pub title: String,
    pub action: String,
    pub time_horizon: String,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub heuristic_edge: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence_score: Option<Decimal>,
    pub workflow_id: Option<String>,
    pub entry_rationale: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_verdict: Option<PolicyVerdictSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVerdictKind {
    Avoid,
    PersistenceBuilding,
    LineageConflict,
    ReviewRequired,
    EnterReady,
    Active,
    ExitRequired,
    AttentionCapped,
}

impl PolicyVerdictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Avoid => "avoid",
            Self::PersistenceBuilding => "persistence_building",
            Self::LineageConflict => "lineage_conflict",
            Self::ReviewRequired => "review_required",
            Self::EnterReady => "enter_ready",
            Self::Active => "active",
            Self::ExitRequired => "exit_required",
            Self::AttentionCapped => "attention_capped",
        }
    }
}

impl std::fmt::Display for PolicyVerdictKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonPolicyVerdict {
    pub horizon: String,
    pub verdict: PolicyVerdictKind,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyVerdictSummary {
    pub primary: PolicyVerdictKind,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub horizons: Vec<HorizonPolicyVerdict>,
}

/// Investigation-first selection layer shared across markets.
///
/// This records which scope/hypothesis deserves analyst attention before
/// action policy or budgeting compresses it into a tactical setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestigationSelection {
    pub investigation_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub provenance: ProvenanceMetadata,
    pub scope: ReasoningScope,
    pub title: String,
    pub family_key: String,
    pub family_label: String,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub priority_score: Decimal,
    pub attention_hint: String,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionDirection {
    Long,
    Short,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionNodeStage {
    Suggested,
    Confirmed,
    Executed,
    Monitoring,
    Reviewed,
}

/// Market-neutral view of an active or historical action for reasoning/predicate inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionNode {
    pub workflow_id: String,
    pub symbol: Symbol,
    pub market: Market,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub stage: ActionNodeStage,
    pub direction: ActionDirection,
    pub entry_confidence: Decimal,
    pub current_confidence: Decimal,
    pub entry_price: Option<Decimal>,
    pub pnl: Option<Decimal>,
    pub age_ticks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degradation_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exit_forming: bool,
}

/// Cross-tick state for a tactical case's leading hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HypothesisTrackStatus {
    New,
    Strengthening,
    Weakening,
    Stable,
    Invalidated,
}

impl HypothesisTrackStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Strengthening => "strengthening",
            Self::Weakening => "weakening",
            Self::Stable => "stable",
            Self::Invalidated => "invalidated",
        }
    }
}

impl std::fmt::Display for HypothesisTrackStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Time-aware track that compares the current tactical case against the prior tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HypothesisTrack {
    pub track_id: String,
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub scope: ReasoningScope,
    pub title: String,
    pub action: String,
    pub status: HypothesisTrackStatus,
    pub age_ticks: u64,
    pub status_streak: u64,
    pub confidence: Decimal,
    pub previous_confidence: Option<Decimal>,
    pub confidence_change: Decimal,
    pub confidence_gap: Decimal,
    pub previous_confidence_gap: Option<Decimal>,
    pub confidence_gap_change: Decimal,
    pub heuristic_edge: Decimal,
    pub policy_reason: String,
    pub transition_reason: Option<String>,
    #[serde(with = "rfc3339")]
    pub first_seen_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub last_updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub invalidated_at: Option<OffsetDateTime>,
}

/// Clustered market narrative composed of multiple related tactical cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseCluster {
    pub cluster_id: String,
    pub family_key: String,
    pub linkage_key: String,
    pub title: String,
    pub lead_hypothesis_id: String,
    pub lead_statement: String,
    pub trend: HypothesisTrackStatus,
    pub member_setup_ids: Vec<String>,
    pub member_track_ids: Vec<String>,
    pub member_scopes: Vec<ReasoningScope>,
    pub propagation_path_ids: Vec<String>,
    pub strongest_setup_id: String,
    pub weakest_setup_id: String,
    pub strongest_title: String,
    pub weakest_title: String,
    pub member_count: usize,
    pub average_confidence: Decimal,
    pub average_gap: Decimal,
    pub average_edge: Decimal,
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::ontology::{ProvenanceSource, Symbol};

    #[test]
    fn hypothesis_holds_supporting_and_contradicting_evidence() {
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        let evidence_for = ReasoningEvidence {
            statement: "book replenishes on every sweep".into(),
            kind: ReasoningEvidenceKind::LocalEvent,
            polarity: EvidencePolarity::Supports,
            weight: dec!(0.6),
            references: vec!["depth:700.HK".into()],
            provenance: provenance.clone(),
        };
        let evidence_against = ReasoningEvidence {
            statement: "put skew widened".into(),
            kind: ReasoningEvidenceKind::PropagatedPath,
            polarity: EvidencePolarity::Contradicts,
            weight: dec!(0.4),
            references: vec!["options:700.HK".into()],
            provenance,
        };

        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700:accumulation".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_trace_id("hyp:700:accumulation")
            .with_inputs(["depth:700.HK", "options:700.HK"]),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            statement: "a patient buyer may be accumulating inventory".into(),
            confidence: dec!(0.55),
            local_support_weight: dec!(0.6),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: dec!(0.4),
            evidence: vec![evidence_for, evidence_against],
            invalidation_conditions: vec![InvalidationCondition {
                description: "institutional alignment flips negative".into(),
                references: vec!["institutional_alignment:700.HK".into()],
            }],
            propagation_path_ids: vec!["path:700:tech".into()],
            expected_observations: vec!["bid replenishment should persist".into()],
        };

        assert_eq!(hypothesis.evidence.len(), 2);
        assert_eq!(hypothesis.propagation_path_ids.len(), 1);
    }

    #[test]
    fn tactical_setup_links_back_to_hypothesis() {
        let setup = TacticalSetup {
            setup_id: "setup:700:observe".into(),
            hypothesis_id: "hyp:700:accumulation".into(),
            runner_up_hypothesis_id: Some("hyp:700:hedge".into()),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_trace_id("setup:700:observe")
            .with_inputs(["hyp:700:accumulation", "hyp:700:hedge"]),
            lineage: DecisionLineage {
                based_on: vec!["hyp:700:accumulation".into()],
                blocked_by: vec![],
                promoted_by: vec!["confidence_gap=0.11".into()],
                falsified_by: vec!["institutional alignment flips negative".into()],
            },
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Tencent accumulation watch".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.63),
            confidence_gap: dec!(0.11),
            heuristic_edge: dec!(0.11),
            convergence_score: Some(dec!(0.52)),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "cross-market alignment remains positive".into(),
            risk_notes: vec!["edge disappears if spread widens".into()],
            policy_verdict: None,
        };

        assert_eq!(setup.action, "review");
        assert!(setup.heuristic_edge > Decimal::ZERO);
    }

    #[test]
    fn hypothesis_track_exposes_status_string() {
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700:accumulation".into(),
            runner_up_hypothesis_id: Some("hyp:700:hedge".into()),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Tencent accumulation watch".into(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: dec!(0.68),
            previous_confidence: Some(dec!(0.61)),
            confidence_change: dec!(0.07),
            confidence_gap: dec!(0.12),
            previous_confidence_gap: Some(dec!(0.08)),
            confidence_gap_change: dec!(0.04),
            heuristic_edge: dec!(0.10),
            policy_reason: "strengthening streak supports escalation".into(),
            transition_reason: Some("promoted from review to enter".into()),
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        assert_eq!(track.status.as_str(), "strengthening");
        assert!(track.confidence_change > Decimal::ZERO);
    }

    #[test]
    fn case_cluster_tracks_cluster_shape() {
        let cluster = CaseCluster {
            cluster_id: "cluster:flow:path:shared_holder:700.HK:9988.HK".into(),
            family_key: "flow".into(),
            linkage_key: "path:shared_holder:700.HK:9988.HK".into(),
            title: "Flow cluster via shared-holder overlap".into(),
            lead_hypothesis_id: "hyp:700.HK:flow".into(),
            lead_statement: "700.HK may currently reflect directed flow repricing".into(),
            trend: HypothesisTrackStatus::Strengthening,
            member_setup_ids: vec!["setup:700.HK:enter".into(), "setup:9988.HK:review".into()],
            member_track_ids: vec!["track:700.HK".into(), "track:9988.HK".into()],
            member_scopes: vec![
                ReasoningScope::Symbol(Symbol("700.HK".into())),
                ReasoningScope::Symbol(Symbol("9988.HK".into())),
            ],
            propagation_path_ids: vec!["path:shared_holder:700.HK:9988.HK".into()],
            strongest_setup_id: "setup:700.HK:enter".into(),
            weakest_setup_id: "setup:9988.HK:review".into(),
            strongest_title: "Long 700.HK".into(),
            weakest_title: "Long 9988.HK".into(),
            member_count: 2,
            average_confidence: dec!(0.66),
            average_gap: dec!(0.14),
            average_edge: dec!(0.09),
        };

        assert_eq!(cluster.member_count, 2);
        assert_eq!(cluster.trend.as_str(), "strengthening");
        assert!(cluster.average_gap > Decimal::ZERO);
    }
}
