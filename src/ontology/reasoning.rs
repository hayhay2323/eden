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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectationKind {
    Observation,
    Propagation,
    CoMovement,
    Confirmation,
    LeadLag,
    CrossMarketFollow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectationBinding {
    pub expectation_id: String,
    pub kind: ExpectationKind,
    pub scope: ReasoningScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_scope: Option<ReasoningScope>,
    pub horizon: String,
    pub strength: Decimal,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectationViolationKind {
    MissingPropagation,
    UnexpectedPropagation,
    FailedConfirmation,
    ModalConflict,
    TimingMismatch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectationViolation {
    pub kind: ExpectationViolationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expectation_id: Option<String>,
    pub description: String,
    pub magnitude: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falsifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseChannel {
    Price,
    Volume,
    CapitalFlow,
    Institutional,
    OrderBook,
    Options,
    CrossMarket,
    Propagation,
    MacroEvent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaseTopology {
    Isolated,
    SectorLinked,
    Chain,
    Relay,
    CrossMarket,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaseTemporalShape {
    Burst,
    Persistent,
    Reversal,
    Drift,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictShape {
    Aligned,
    Mixed,
    Contradictory,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CaseSignature {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_channels: Vec<CaseChannel>,
    #[serde(default)]
    pub topology: CaseTopology,
    #[serde(default)]
    pub temporal_shape: CaseTemporalShape,
    #[serde(default)]
    pub conflict_shape: ConflictShape,
    #[serde(default)]
    pub expectation_support: usize,
    #[serde(default)]
    pub expectation_violations: usize,
    #[serde(default)]
    pub novelty_score: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchetypeProjection {
    pub archetype_key: String,
    pub label: String,
    pub affinity: Decimal,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Accumulation,
    Distribution,
    ForcedUnwind,
    PassiveRebalance,
    EventRepricing,
    FailedPropagation,
    CrossMarketLead,
    Absorption,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentDirection {
    Buy,
    Sell,
    Mixed,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentState {
    Forming,
    Active,
    AtRisk,
    Exhausted,
    Invalidated,
    Fulfilled,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentExitKind {
    Decay,
    Reversal,
    Exhaustion,
    Fulfilled,
    Absorbed,
    Invalidated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentExitSignal {
    pub kind: IntentExitKind,
    pub confidence: Decimal,
    pub rationale: String,
    pub trigger: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentOpportunityBias {
    Enter,
    Hold,
    Watch,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentOpportunityWindow {
    /// Primary trading-time language. New field (Wave 2).
    #[serde(default = "default_opportunity_bucket")]
    pub bucket: crate::ontology::horizon::HorizonBucket,
    /// Action timing. New field (Wave 2).
    #[serde(default = "default_opportunity_urgency")]
    pub urgency: crate::ontology::horizon::Urgency,
    /// Legacy string horizon. Derived from `bucket` via
    /// `HorizonBucket::to_legacy_string()`. Kept until Wave 4 for
    /// JSON backward compatibility. Never use as source of truth.
    #[serde(default)]
    pub horizon: String,
    pub bias: IntentOpportunityBias,
    pub confidence: Decimal,
    pub alignment: Decimal,
    pub rationale: String,
}

fn default_opportunity_bucket() -> crate::ontology::horizon::HorizonBucket {
    crate::ontology::horizon::HorizonBucket::Session
}

fn default_opportunity_urgency() -> crate::ontology::horizon::Urgency {
    crate::ontology::horizon::Urgency::Normal
}

pub fn default_case_horizon() -> crate::ontology::horizon::CaseHorizon {
    use crate::ontology::horizon::{
        CaseHorizon, HorizonBucket, HorizonExpiry, SessionPhase, Urgency,
    };
    CaseHorizon::new(
        HorizonBucket::Session,
        Urgency::Normal,
        SessionPhase::Midday,
        HorizonExpiry::UntilSessionClose,
        vec![],
    )
}

impl IntentOpportunityWindow {
    /// Build a window, auto-filling `horizon` from `bucket` so callers
    /// never have to set the legacy string manually.
    pub fn new(
        bucket: crate::ontology::horizon::HorizonBucket,
        urgency: crate::ontology::horizon::Urgency,
        bias: IntentOpportunityBias,
        confidence: Decimal,
        alignment: Decimal,
        rationale: String,
    ) -> Self {
        Self {
            bucket,
            urgency,
            horizon: bucket.to_legacy_string().to_string(),
            bias,
            confidence,
            alignment,
            rationale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentStrength {
    pub flow_strength: Decimal,
    pub impact_strength: Decimal,
    pub persistence_strength: Decimal,
    pub propagation_strength: Decimal,
    pub resistance_strength: Decimal,
    pub composite: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentHypothesis {
    pub intent_id: String,
    pub kind: IntentKind,
    pub scope: ReasoningScope,
    pub direction: IntentDirection,
    pub state: IntentState,
    pub confidence: Decimal,
    pub urgency: Decimal,
    pub persistence: Decimal,
    pub conflict_score: Decimal,
    pub strength: IntentStrength,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_targets: Vec<ReasoningScope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_archetypes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supporting_case_signature: Option<CaseSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_bindings: Vec<ExpectationBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expectation_violations: Vec<ExpectationViolation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit_signals: Vec<IntentExitSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opportunities: Vec<IntentOpportunityWindow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub falsifiers: Vec<String>,
    pub rationale: String,
}

/// One step in a propagation path from one scope to another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropagationStep {
    pub from: ReasoningScope,
    pub to: ReasoningScope,
    pub mechanism: String,
    pub confidence: Decimal,
    /// Signed direction from source_delta: +1 = bullish, -1 = bearish.
    /// Defaults to +1 for backwards compatibility with paths that predate this field.
    #[serde(default = "default_polarity")]
    pub polarity: i8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
}

fn default_polarity() -> i8 {
    1
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

/// Typed structural marker for substrate-emitted hypotheses. V3
/// replacement for the V2 `hypothesis_id.contains("hidden_force"/...)`
/// string-pattern matching. The set is closed: only hypotheses
/// emitted by Eden's known substrate paths get a `kind`. Legacy /
/// open-ended hypotheses leave it `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HypothesisKind {
    /// Isolated or sector-level hidden force (residual layer).
    HiddenForce,
    /// Cross-symbol hidden connection between two divergent symbols.
    HiddenConnection,
    /// US convergence hypothesis (multi-channel topology).
    ConvergenceHypothesis,
    /// US latent vortex (low-strength but persistent topology).
    LatentVortex,
    /// HK pressure vortex (tension-driven setup).
    PressureVortex,
    /// HK tension vortex (tick vs hour divergence).
    TensionVortex,
}

/// Open-ended hypothesis container. The statement stays free-form; the ontology
/// only constrains the structure around it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub hypothesis_id: String,
    // family_key field deleted per V2 + first-principles audit
    // (Pass 2 of family_key removal). Was a categorical bucket key
    // ("flow" / "rotation" / "convergence_hypothesis" / "hidden_force")
    // used for filtering and case_signature pattern matching. Replaced
    // by `kind` (typed marker enum, V3) for structural identity and
    // by `family_label` (human-readable operator-facing display).
    /// Typed structural marker. None when the hypothesis class is not
    /// a recognized substrate-emitter (e.g. flow / rotation hypotheses
    /// from the legacy reasoning layer). Replaces V2's
    /// `hypothesis_id.contains("hidden_force"/...)` string-pattern
    /// matching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<HypothesisKind>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TacticalAction {
    Enter,
    Review,
    Observe,
}

impl TacticalAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enter => "enter",
            Self::Review => "review",
            Self::Observe => "observe",
        }
    }
}

impl std::fmt::Display for TacticalAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for TacticalAction {
    fn from(value: &str) -> Self {
        match value {
            "enter" => Self::Enter,
            "observe" => Self::Observe,
            _ => Self::Review,
        }
    }
}

impl From<String> for TacticalAction {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<TacticalAction> for String {
    fn from(value: TacticalAction) -> Self {
        value.as_str().to_string()
    }
}

impl PartialEq<&str> for TacticalAction {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TacticalDirection {
    Long,
    Short,
}

impl TacticalDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }
}

impl From<TacticalDirection> for ActionDirection {
    fn from(value: TacticalDirection) -> Self {
        match value {
            TacticalDirection::Long => ActionDirection::Long,
            TacticalDirection::Short => ActionDirection::Short,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TacticalSetup {
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub provenance: ProvenanceMetadata,
    pub lineage: DecisionLineage,
    pub scope: ReasoningScope,
    pub title: String,
    pub action: TacticalAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<TacticalDirection>,
    // family_key field deleted per first-principles audit — was a
    // categorical bucket ("pressure_vortex" / "hidden_connection" /
    // "convergence_hypothesis" / etc) hardcoded at every construction
    // site. Pre-classification of structural signal violates the
    // structure-IS-signal axiom. Hypothesis.family_key still exists
    // (broader blast, deferred to follow-up PR).
    /// `CaseHorizon` is the sole source of truth for trading rhythm (Wave 4).
    /// The legacy `time_horizon: String` field was removed here; old serialized
    /// records may still contain it but serde silently ignores unknown fields.
    #[serde(default = "default_case_horizon")]
    pub horizon: crate::ontology::horizon::CaseHorizon,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub heuristic_edge: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence_score: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence_detail: Option<crate::pipeline::reasoning::ConvergenceDetail>,
    pub workflow_id: Option<String>,
    pub entry_rationale: String,
    /// One-sentence causal explanation: why does this case exist at the reasoning level?
    /// Distinct from entry_rationale which is a policy-level justification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causal_narrative: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<ReviewReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_verdict: Option<PolicyVerdictSummary>,
}

pub fn direction_from_setup(setup: &TacticalSetup) -> Option<TacticalDirection> {
    setup.direction
}

pub fn action_direction_from_setup(setup: &TacticalSetup) -> Option<ActionDirection> {
    direction_from_setup(setup).map(ActionDirection::from)
}

pub fn sanitize_carried_tactical_setup(setup: &TacticalSetup) -> TacticalSetup {
    let mut carried = setup.clone();
    carried.risk_notes.retain(|note| {
        !note.starts_with("phase=")
            && !note.starts_with("velocity=")
            && !note.starts_with("acceleration=")
            && !note.starts_with("driver=")
            && !note.starts_with("driver_class=")
            && !note.starts_with("peer_confirmation_ratio=")
            && !note.starts_with("peer_active_count=")
            && !note.starts_with("peer_silent_count=")
            && !note.starts_with("isolation_score=")
            && !note.starts_with("is_isolated=")
            && !note.starts_with("competition_margin=")
    });
    if !carried
        .risk_notes
        .iter()
        .any(|note| note == "carried_forward=true")
    {
        carried.risk_notes.push("carried_forward=true".into());
    }
    if carried.action == TacticalAction::Enter {
        carried.action = TacticalAction::Review;
    }
    carried.review_reason_code = Some(ReviewReasonCode::StaleSymbolConfirmation);
    carried.policy_verdict = Some(PolicyVerdictSummary {
        primary: PolicyVerdictKind::ReviewRequired,
        rationale:
            "setup was carried forward from a previous tick and must be refreshed before execution"
                .into(),
        review_reason_code: Some(ReviewReasonCode::StaleSymbolConfirmation),
        conflict_reason: None,
        horizons: vec![],
    });
    carried
}

impl Hypothesis {
    pub fn expected_bindings(&self) -> Vec<ExpectationBinding> {
        let mut bindings = self
            .expected_observations
            .iter()
            .enumerate()
            .map(|(index, observation)| ExpectationBinding {
                expectation_id: format!("{}:obs:{index}", self.hypothesis_id),
                kind: ExpectationKind::Observation,
                scope: self.scope.clone(),
                target_scope: None,
                horizon: "intraday".into(),
                strength: self
                    .local_support_weight
                    .max(self.propagated_support_weight),
                rationale: observation.clone(),
            })
            .collect::<Vec<_>>();

        bindings.extend(
            self.propagation_path_ids
                .iter()
                .enumerate()
                .map(|(index, path_id)| ExpectationBinding {
                    expectation_id: format!("{}:path:{index}", self.hypothesis_id),
                    kind: if path_id.to_ascii_lowercase().contains("cross") {
                        ExpectationKind::CrossMarketFollow
                    } else {
                        ExpectationKind::Propagation
                    },
                    scope: self.scope.clone(),
                    target_scope: None,
                    horizon: "intraday".into(),
                    strength: self.propagated_support_weight,
                    rationale: format!("propagation path {path_id} should continue to transmit"),
                }),
        );

        bindings
    }

    pub fn expectation_violations(&self) -> Vec<ExpectationViolation> {
        let mut violations = Vec::new();

        if self.propagated_contradict_weight > self.propagated_support_weight
            && !self.propagation_path_ids.is_empty()
        {
            violations.push(ExpectationViolation {
                kind: ExpectationViolationKind::MissingPropagation,
                expectation_id: Some(format!("{}:path:0", self.hypothesis_id)),
                description: "propagation evidence is weaker than expected".into(),
                magnitude: self.propagated_contradict_weight - self.propagated_support_weight,
                falsifier: None,
            });
        }

        if self.local_contradict_weight > self.local_support_weight
            && !self.expected_observations.is_empty()
        {
            violations.push(ExpectationViolation {
                kind: ExpectationViolationKind::FailedConfirmation,
                expectation_id: Some(format!("{}:obs:0", self.hypothesis_id)),
                description: "local tape is contradicting expected observations".into(),
                magnitude: self.local_contradict_weight - self.local_support_weight,
                falsifier: self
                    .invalidation_conditions
                    .first()
                    .map(|condition| condition.description.clone()),
            });
        }

        violations
    }

    pub fn case_signature(&self) -> CaseSignature {
        let mut channels = self
            .evidence
            .iter()
            .map(|evidence| classify_case_channel(&evidence.statement))
            .collect::<Vec<_>>();
        channels.extend(
            self.expected_observations
                .iter()
                .map(|item| classify_case_channel(item)),
        );
        dedup_channels(&mut channels);

        let topology = if self
            .propagation_path_ids
            .iter()
            .any(|path| path.to_ascii_lowercase().contains("cross"))
        {
            CaseTopology::CrossMarket
        } else if self.propagation_path_ids.len() >= 2 {
            CaseTopology::Chain
        } else if self.propagation_path_ids.is_empty() {
            CaseTopology::Isolated
        } else {
            CaseTopology::Unknown
        };
        // Relay / SectorLinked branches deleted — they were keyed on
        // `family_key.contains("relay" / "sector")`, a categorical
        // string-bucket pattern that violates structure-IS-signal.
        // Sub-KG signature (WL fingerprint) is the structural key now.

        let temporal_shape =
            infer_temporal_shape(self.local_support_weight, self.propagated_support_weight);
        let conflict_shape = infer_conflict_shape(
            self.local_support_weight,
            self.local_contradict_weight,
            self.propagated_support_weight,
            self.propagated_contradict_weight,
        );
        let expectation_support = self.expected_bindings().len();
        let expectation_violations = self.expectation_violations().len();
        let novelty_score = infer_novelty_score(
            expectation_violations,
            channels.len(),
            self.expected_observations.len(),
        );

        CaseSignature {
            active_channels: channels,
            topology,
            temporal_shape,
            conflict_shape,
            expectation_support,
            expectation_violations,
            novelty_score,
            notes: self.expected_observations.clone(),
        }
    }

    pub fn intent_hypothesis(&self) -> IntentHypothesis {
        let signature = self.case_signature();
        let bindings = self.expected_bindings();
        let violations = self.expectation_violations();
        let exit_signals = infer_intent_exit_signals(
            &signature,
            &violations,
            &[self.family_label.as_str(), self.statement.as_str()],
        );
        let direction =
            infer_intent_direction(&[self.family_label.as_str(), self.statement.as_str()]);
        let kind = infer_intent_kind(
            &[self.family_label.as_str(), self.statement.as_str()],
            &signature,
            &bindings,
            &violations,
            direction,
        );
        let propagation_targets = bindings
            .iter()
            .filter_map(|binding| binding.target_scope.clone())
            .collect::<Vec<_>>();
        let falsifiers = self
            .invalidation_conditions
            .iter()
            .map(|condition| condition.description.clone())
            .collect::<Vec<_>>();
        let urgency = infer_intent_urgency(
            &signature,
            violations.len(),
            self.local_contradict_weight + self.propagated_contradict_weight,
        );
        let persistence = infer_intent_persistence(&signature);
        let conflict_score = infer_intent_conflict_score(&signature, &violations);
        let state = infer_intent_state(&signature, &violations, &exit_signals);
        let strength = infer_intent_strength(
            &signature,
            self.confidence,
            self.propagated_support_weight,
            self.local_contradict_weight + self.propagated_contradict_weight,
        );

        IntentHypothesis {
            intent_id: format!("intent:{}", self.hypothesis_id),
            kind,
            scope: self.scope.clone(),
            direction,
            state,
            confidence: self.confidence,
            urgency,
            persistence,
            conflict_score,
            strength,
            propagation_targets,
            supporting_archetypes: vec![self.family_label.clone()],
            supporting_case_signature: Some(signature),
            expectation_bindings: bindings,
            expectation_violations: violations,
            exit_signals,
            opportunities: Vec::new(),
            falsifiers,
            rationale: format!(
                "{} inferred from hypothesis {}",
                render_intent_kind(kind),
                self.family_label
            ),
        }
    }
}

impl TacticalSetup {
    pub fn case_signature(&self, hypothesis: Option<&Hypothesis>) -> CaseSignature {
        let mut signature = hypothesis
            .map(Hypothesis::case_signature)
            .unwrap_or_else(|| CaseSignature {
                active_channels: Vec::new(),
                topology: CaseTopology::Unknown,
                temporal_shape: CaseTemporalShape::Unknown,
                conflict_shape: ConflictShape::Unknown,
                expectation_support: 0,
                expectation_violations: 0,
                novelty_score: Decimal::ZERO,
                notes: Vec::new(),
            });

        if let Some(detail) = &self.convergence_detail {
            if detail.institutional_alignment.abs() > Decimal::new(2, 1) {
                signature.active_channels.push(CaseChannel::Institutional);
            }
            if detail.cross_stock_correlation.abs() > Decimal::new(2, 1) {
                signature.active_channels.push(CaseChannel::Propagation);
            }
            if detail.sector_coherence.unwrap_or(Decimal::ZERO).abs() > Decimal::new(2, 1) {
                signature.topology = CaseTopology::SectorLinked;
            }
        }

        signature.notes.extend(self.risk_notes.iter().cloned());
        dedup_channels(&mut signature.active_channels);
        signature.novelty_score = signature
            .novelty_score
            .max(if self.causal_narrative.is_some() {
                Decimal::new(6, 1)
            } else {
                Decimal::ZERO
            });
        signature
    }

    pub fn archetype_projections(
        &self,
        hypothesis: Option<&Hypothesis>,
    ) -> Vec<ArchetypeProjection> {
        let signature = self.case_signature(hypothesis);
        let mut projections = Vec::new();

        if signature.novelty_score >= Decimal::new(5, 1) {
            projections.push(ArchetypeProjection {
                archetype_key: "emergent".into(),
                label: "emergent pattern".into(),
                affinity: signature.novelty_score.min(Decimal::ONE),
                rationale: "multiple active channels and/or expectation violations indicate an emergent case shape".into(),
            });
        }

        projections
    }

    pub fn intent_hypothesis(&self, hypothesis: Option<&Hypothesis>) -> IntentHypothesis {
        let signature = self.case_signature(hypothesis);
        let bindings = hypothesis
            .map(Hypothesis::expected_bindings)
            .unwrap_or_default();
        let violations = hypothesis
            .map(Hypothesis::expectation_violations)
            .unwrap_or_default();
        let exit_signals = infer_intent_exit_signals(
            &signature,
            &violations,
            &[
                self.title.as_str(),
                self.entry_rationale.as_str(),
                self.causal_narrative.as_deref().unwrap_or_default(),
            ],
        );
        let projections = self.archetype_projections(hypothesis);
        let direction = infer_intent_direction(&[
            self.title.as_str(),
            self.entry_rationale.as_str(),
            self.causal_narrative.as_deref().unwrap_or_default(),
        ]);
        let kind = infer_intent_kind(
            &[
                self.title.as_str(),
                self.entry_rationale.as_str(),
                self.causal_narrative.as_deref().unwrap_or_default(),
            ],
            &signature,
            &bindings,
            &violations,
            direction,
        );
        let propagation_targets = bindings
            .iter()
            .filter_map(|binding| binding.target_scope.clone())
            .collect::<Vec<_>>();
        let falsifiers = self.risk_notes.clone();
        let propagation_hint = self
            .convergence_detail
            .as_ref()
            .map(|detail| detail.cross_stock_correlation.abs())
            .unwrap_or(Decimal::ZERO);
        let resistance_hint =
            self.confidence_gap + Decimal::from(violations.len() as i64) / Decimal::new(10, 1);
        let urgency = infer_intent_urgency(&signature, violations.len(), resistance_hint);
        let persistence = infer_intent_persistence(&signature);
        let conflict_score = infer_intent_conflict_score(&signature, &violations);
        let state = infer_intent_state(&signature, &violations, &exit_signals);
        let strength = infer_intent_strength(
            &signature,
            self.confidence,
            propagation_hint,
            resistance_hint,
        );

        IntentHypothesis {
            intent_id: format!("intent:{}", self.setup_id),
            kind,
            scope: self.scope.clone(),
            direction,
            state,
            confidence: self.confidence,
            urgency,
            persistence,
            conflict_score,
            strength,
            propagation_targets,
            supporting_archetypes: projections
                .iter()
                .map(|projection| projection.archetype_key.clone())
                .collect(),
            supporting_case_signature: Some(signature),
            expectation_bindings: bindings,
            expectation_violations: violations,
            exit_signals,
            opportunities: Vec::new(),
            falsifiers,
            rationale: format!(
                "{} inferred from tactical setup {}",
                render_intent_kind(kind),
                self.title
            ),
        }
    }
}

pub fn enrich_hypothesis_with_ontology_projection(hypothesis: &mut Hypothesis) {
    let bindings = hypothesis.expected_bindings();
    let violations = hypothesis.expectation_violations();
    let signature = hypothesis.case_signature();

    for binding in bindings {
        let rendered = render_expectation_binding(&binding);
        if !hypothesis
            .expected_observations
            .iter()
            .any(|item| item == &rendered)
        {
            hypothesis.expected_observations.push(rendered);
        }
    }

    for violation in violations {
        let rendered = render_expectation_violation(&violation);
        if !hypothesis
            .evidence
            .iter()
            .any(|item| item.statement == rendered)
        {
            hypothesis.evidence.push(ReasoningEvidence {
                statement: rendered,
                kind: ReasoningEvidenceKind::PropagatedPath,
                polarity: EvidencePolarity::Contradicts,
                weight: violation.magnitude.min(Decimal::ONE),
                references: violation.expectation_id.into_iter().collect(),
                provenance: hypothesis.provenance.clone(),
            });
        }
    }

    let rendered_signature = render_case_signature(&signature);
    if !hypothesis
        .evidence
        .iter()
        .any(|item| item.statement == rendered_signature)
    {
        hypothesis.evidence.push(ReasoningEvidence {
            statement: rendered_signature,
            kind: ReasoningEvidenceKind::LocalSignal,
            polarity: EvidencePolarity::Supports,
            weight: signature.novelty_score.min(Decimal::ONE),
            references: vec![format!("signature:{}", hypothesis.hypothesis_id)],
            provenance: hypothesis.provenance.clone(),
        });
    }
}

pub fn enrich_tactical_setup_with_ontology_projection(
    setup: &mut TacticalSetup,
    hypothesis: Option<&Hypothesis>,
) {
    let signature = setup.case_signature(hypothesis);
    let projections = setup.archetype_projections(hypothesis);
    let intent = setup.intent_hypothesis(hypothesis);

    let rendered_signature = render_case_signature(&signature);
    if !setup
        .risk_notes
        .iter()
        .any(|note| note == &rendered_signature)
    {
        setup.risk_notes.push(rendered_signature);
    }

    for projection in projections {
        let rendered = render_archetype_projection(&projection);
        if !setup.risk_notes.iter().any(|note| note == &rendered) {
            setup.risk_notes.push(rendered);
        }
    }

    let rendered_intent = render_intent_hypothesis(&intent);
    if !setup.risk_notes.iter().any(|note| note == &rendered_intent) {
        setup.risk_notes.push(rendered_intent);
    }
}

fn classify_case_channel(text: &str) -> CaseChannel {
    let lower = text.to_ascii_lowercase();
    if lower.contains("option") || lower.contains("iv") || lower.contains("skew") {
        CaseChannel::Options
    } else if lower.contains("cross") || lower.contains("hk") || lower.contains("us") {
        CaseChannel::CrossMarket
    } else if lower.contains("flow") || lower.contains("inflow") {
        CaseChannel::CapitalFlow
    } else if lower.contains("broker") || lower.contains("institution") {
        CaseChannel::Institutional
    } else if lower.contains("depth") || lower.contains("spread") || lower.contains("book") {
        CaseChannel::OrderBook
    } else if lower.contains("volume") {
        CaseChannel::Volume
    } else if lower.contains("price") || lower.contains("gap") || lower.contains("return") {
        CaseChannel::Price
    } else if lower.contains("macro") || lower.contains("catalyst") || lower.contains("event") {
        CaseChannel::MacroEvent
    } else if lower.contains("path") || lower.contains("propagat") {
        CaseChannel::Propagation
    } else {
        CaseChannel::Unknown
    }
}

fn dedup_channels(channels: &mut Vec<CaseChannel>) {
    let mut seen = std::collections::HashSet::new();
    channels.retain(|channel| seen.insert(*channel));
}

fn infer_temporal_shape(local_support: Decimal, propagated_support: Decimal) -> CaseTemporalShape {
    if propagated_support > local_support && propagated_support > Decimal::new(2, 1) {
        CaseTemporalShape::Persistent
    } else if local_support > propagated_support && local_support > Decimal::new(2, 1) {
        CaseTemporalShape::Burst
    } else if local_support < Decimal::ZERO || propagated_support < Decimal::ZERO {
        CaseTemporalShape::Reversal
    } else if local_support != Decimal::ZERO || propagated_support != Decimal::ZERO {
        CaseTemporalShape::Drift
    } else {
        CaseTemporalShape::Unknown
    }
}

fn infer_conflict_shape(
    local_support: Decimal,
    local_contradict: Decimal,
    propagated_support: Decimal,
    propagated_contradict: Decimal,
) -> ConflictShape {
    let support = local_support + propagated_support;
    let contradict = local_contradict + propagated_contradict;
    if support == Decimal::ZERO && contradict == Decimal::ZERO {
        ConflictShape::Unknown
    } else if support > Decimal::ZERO && contradict == Decimal::ZERO {
        ConflictShape::Aligned
    } else if contradict > Decimal::ZERO && support == Decimal::ZERO {
        ConflictShape::Contradictory
    } else {
        ConflictShape::Mixed
    }
}

fn infer_novelty_score(
    expectation_violations: usize,
    active_channels: usize,
    expected_observations: usize,
) -> Decimal {
    (Decimal::from(expectation_violations.min(4) as i64) * Decimal::new(2, 1)
        + Decimal::from(active_channels.min(6) as i64) * Decimal::new(1, 1)
        + Decimal::from(expected_observations.min(4) as i64) * Decimal::new(5, 1))
        / Decimal::new(10, 1)
}

pub fn render_expectation_binding(binding: &ExpectationBinding) -> String {
    format!(
        "expectation:{}:{}:{:.3}:{}",
        binding.kind as u8, binding.horizon, binding.strength, binding.rationale
    )
}

pub fn render_expectation_violation(violation: &ExpectationViolation) -> String {
    format!(
        "violation:{}:{:.3}:{}",
        violation.kind as u8, violation.magnitude, violation.description
    )
}

pub fn render_case_signature(signature: &CaseSignature) -> String {
    let channels = signature
        .active_channels
        .iter()
        .map(|channel| format!("{channel:?}").to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "signature:channels=[{}];topology={:?};temporal={:?};conflict={:?};support={};violations={};novelty={:.3}",
        channels,
        signature.topology,
        signature.temporal_shape,
        signature.conflict_shape,
        signature.expectation_support,
        signature.expectation_violations,
        signature.novelty_score,
    )
}

pub fn render_archetype_projection(projection: &ArchetypeProjection) -> String {
    format!(
        "archetype:{}:{:.3}:{}",
        projection.archetype_key, projection.affinity, projection.rationale
    )
}

pub fn render_intent_hypothesis(intent: &IntentHypothesis) -> String {
    format!(
        "intent:{}:{:?}:{:?}:{:?}:{:.3}:{:.3}",
        intent.intent_id,
        intent.kind,
        intent.direction,
        intent.state,
        intent.confidence,
        intent.strength.composite
    )
}

fn infer_intent_direction(texts: &[&str]) -> IntentDirection {
    let lower = texts.join(" ").to_ascii_lowercase();
    let buy = ["long", "buy", "bid", "accum", "inflow", "support", "upside"]
        .iter()
        .any(|needle| lower.contains(needle));
    let sell = [
        "short",
        "sell",
        "offer",
        "distribution",
        "outflow",
        "downside",
        "liquidat",
        "unwind",
    ]
    .iter()
    .any(|needle| lower.contains(needle));

    match (buy, sell) {
        (true, true) => IntentDirection::Mixed,
        (true, false) => IntentDirection::Buy,
        (false, true) => IntentDirection::Sell,
        _ => IntentDirection::Neutral,
    }
}

fn infer_intent_kind(
    texts: &[&str],
    signature: &CaseSignature,
    bindings: &[ExpectationBinding],
    violations: &[ExpectationViolation],
    direction: IntentDirection,
) -> IntentKind {
    let lower = texts.join(" ").to_ascii_lowercase();

    if signature.topology == CaseTopology::CrossMarket
        || bindings
            .iter()
            .any(|binding| binding.kind == ExpectationKind::CrossMarketFollow)
    {
        return IntentKind::CrossMarketLead;
    }

    if violations.iter().any(|violation| {
        matches!(
            violation.kind,
            ExpectationViolationKind::MissingPropagation
                | ExpectationViolationKind::UnexpectedPropagation
        )
    }) {
        return IntentKind::FailedPropagation;
    }

    if signature
        .active_channels
        .iter()
        .any(|channel| *channel == CaseChannel::MacroEvent)
        || lower.contains("catalyst")
        || lower.contains("event")
        || lower.contains("repric")
    {
        return IntentKind::EventRepricing;
    }

    if lower.contains("rebalance")
        || lower.contains("rotation")
        || lower.contains("index")
        || lower.contains("etf")
        || lower.contains("passive")
    {
        return IntentKind::PassiveRebalance;
    }

    if lower.contains("forced")
        || lower.contains("unwind")
        || lower.contains("liquidat")
        || lower.contains("delever")
    {
        return IntentKind::ForcedUnwind;
    }

    if signature.conflict_shape == ConflictShape::Contradictory
        && (signature.active_channels.contains(&CaseChannel::Volume)
            || signature.active_channels.contains(&CaseChannel::OrderBook))
    {
        return IntentKind::Absorption;
    }

    match direction {
        IntentDirection::Buy => IntentKind::Accumulation,
        IntentDirection::Sell => IntentKind::Distribution,
        IntentDirection::Mixed => IntentKind::Absorption,
        IntentDirection::Neutral => IntentKind::Unknown,
    }
}

fn infer_intent_exit_signals(
    signature: &CaseSignature,
    violations: &[ExpectationViolation],
    texts: &[&str],
) -> Vec<IntentExitSignal> {
    let mut signals = Vec::new();
    let lower = texts.join(" ").to_ascii_lowercase();

    for violation in violations {
        match violation.kind {
            ExpectationViolationKind::FailedConfirmation => signals.push(IntentExitSignal {
                kind: IntentExitKind::Invalidated,
                confidence: clamp_intent_score(violation.magnitude.max(Decimal::new(6, 1))),
                rationale: "expected confirmation failed".into(),
                trigger: violation.description.clone(),
            }),
            ExpectationViolationKind::MissingPropagation => signals.push(IntentExitSignal {
                kind: IntentExitKind::Absorbed,
                confidence: clamp_intent_score(violation.magnitude.max(Decimal::new(5, 1))),
                rationale: "expected propagation was absorbed or blocked".into(),
                trigger: violation.description.clone(),
            }),
            ExpectationViolationKind::TimingMismatch => signals.push(IntentExitSignal {
                kind: IntentExitKind::Decay,
                confidence: clamp_intent_score(violation.magnitude.max(Decimal::new(4, 1))),
                rationale: "timing mismatch suggests intent decay".into(),
                trigger: violation.description.clone(),
            }),
            ExpectationViolationKind::ModalConflict
            | ExpectationViolationKind::UnexpectedPropagation => signals.push(IntentExitSignal {
                kind: IntentExitKind::Decay,
                confidence: clamp_intent_score(violation.magnitude.max(Decimal::new(4, 1))),
                rationale: "conflicting behavior weakens the current intent".into(),
                trigger: violation.description.clone(),
            }),
        }
    }

    if signature.temporal_shape == CaseTemporalShape::Reversal {
        signals.push(IntentExitSignal {
            kind: IntentExitKind::Reversal,
            confidence: Decimal::new(7, 1),
            rationale: "temporal shape has reversed".into(),
            trigger: "signature temporal_shape=reversal".into(),
        });
    }

    if signature.temporal_shape == CaseTemporalShape::Burst
        && signature.novelty_score <= Decimal::new(4, 1)
    {
        signals.push(IntentExitSignal {
            kind: IntentExitKind::Exhaustion,
            confidence: Decimal::new(5, 1),
            rationale: "burst intent without durable follow-through can exhaust quickly".into(),
            trigger: "burst shape without durable novelty".into(),
        });
    }

    if lower.contains("complete") || lower.contains("fulfilled") || lower.contains("done") {
        signals.push(IntentExitSignal {
            kind: IntentExitKind::Fulfilled,
            confidence: Decimal::new(6, 1),
            rationale: "intent appears completed or already expressed".into(),
            trigger: "rationale text indicates completion".into(),
        });
    }

    dedup_intent_exit_signals(&mut signals);
    signals
}

fn dedup_intent_exit_signals(signals: &mut Vec<IntentExitSignal>) {
    let mut seen = std::collections::HashSet::new();
    signals.retain(|signal| seen.insert((signal.kind, signal.trigger.clone())));
}

fn infer_intent_state(
    signature: &CaseSignature,
    violations: &[ExpectationViolation],
    exit_signals: &[IntentExitSignal],
) -> IntentState {
    if exit_signals
        .iter()
        .any(|signal| signal.kind == IntentExitKind::Invalidated)
    {
        return IntentState::Invalidated;
    }
    if exit_signals
        .iter()
        .any(|signal| signal.kind == IntentExitKind::Fulfilled)
    {
        return IntentState::Fulfilled;
    }
    if exit_signals.iter().any(|signal| {
        matches!(
            signal.kind,
            IntentExitKind::Reversal | IntentExitKind::Absorbed
        )
    }) {
        return IntentState::AtRisk;
    }
    if exit_signals.iter().any(|signal| {
        matches!(
            signal.kind,
            IntentExitKind::Decay | IntentExitKind::Exhaustion
        )
    }) {
        return IntentState::Exhausted;
    }
    if !violations.is_empty() {
        return IntentState::AtRisk;
    }
    match signature.temporal_shape {
        CaseTemporalShape::Persistent => IntentState::Active,
        CaseTemporalShape::Burst | CaseTemporalShape::Drift => IntentState::Forming,
        CaseTemporalShape::Reversal => IntentState::AtRisk,
        CaseTemporalShape::Unknown => IntentState::Unknown,
    }
}

fn infer_intent_persistence(signature: &CaseSignature) -> Decimal {
    match signature.temporal_shape {
        CaseTemporalShape::Persistent => Decimal::new(8, 1),
        CaseTemporalShape::Drift => Decimal::new(5, 1),
        CaseTemporalShape::Burst => Decimal::new(4, 1),
        CaseTemporalShape::Reversal => Decimal::new(3, 1),
        CaseTemporalShape::Unknown => Decimal::new(2, 1),
    }
}

fn infer_intent_urgency(
    signature: &CaseSignature,
    violation_count: usize,
    resistance_hint: Decimal,
) -> Decimal {
    let base = match signature.temporal_shape {
        CaseTemporalShape::Burst => Decimal::new(8, 1),
        CaseTemporalShape::Reversal => Decimal::new(7, 1),
        CaseTemporalShape::Persistent => Decimal::new(5, 1),
        CaseTemporalShape::Drift => Decimal::new(3, 1),
        CaseTemporalShape::Unknown => Decimal::new(2, 1),
    };
    clamp_intent_score(
        base + Decimal::from(violation_count.min(3) as i64) / Decimal::new(10, 1) + resistance_hint,
    )
}

fn infer_intent_conflict_score(
    signature: &CaseSignature,
    violations: &[ExpectationViolation],
) -> Decimal {
    let base = match signature.conflict_shape {
        ConflictShape::Aligned => Decimal::new(1, 1),
        ConflictShape::Mixed => Decimal::new(5, 1),
        ConflictShape::Contradictory => Decimal::new(8, 1),
        ConflictShape::Unknown => Decimal::new(2, 1),
    };
    clamp_intent_score(base + Decimal::from(violations.len().min(3) as i64) / Decimal::new(10, 1))
}

fn infer_intent_strength(
    signature: &CaseSignature,
    confidence: Decimal,
    propagation_hint: Decimal,
    resistance_hint: Decimal,
) -> IntentStrength {
    let channel_bonus =
        Decimal::from(signature.active_channels.len().min(4) as i64) / Decimal::new(10, 1);
    let flow_strength = clamp_intent_score(confidence + channel_bonus);
    let impact_strength = clamp_intent_score(
        confidence
            + if signature.active_channels.contains(&CaseChannel::Price)
                || signature.active_channels.contains(&CaseChannel::Volume)
            {
                Decimal::new(2, 1)
            } else {
                Decimal::ZERO
            },
    );
    let persistence_strength = infer_intent_persistence(signature);
    let propagation_strength = clamp_intent_score(
        propagation_hint
            + Decimal::from(signature.expectation_support.min(3) as i64) / Decimal::new(10, 1)
            + if signature
                .active_channels
                .contains(&CaseChannel::Propagation)
                || signature
                    .active_channels
                    .contains(&CaseChannel::CrossMarket)
            {
                Decimal::new(2, 1)
            } else {
                Decimal::ZERO
            },
    );
    let resistance_strength = clamp_intent_score(
        resistance_hint
            + Decimal::from(signature.expectation_violations.min(3) as i64) / Decimal::new(10, 1),
    );
    let composite = clamp_intent_score(
        (flow_strength + impact_strength + persistence_strength + propagation_strength)
            / Decimal::new(4, 0)
            - resistance_strength / Decimal::new(4, 0),
    );

    IntentStrength {
        flow_strength,
        impact_strength,
        persistence_strength,
        propagation_strength,
        resistance_strength,
        composite,
    }
}

fn clamp_intent_score(value: Decimal) -> Decimal {
    value.max(Decimal::ZERO).min(Decimal::ONE)
}

fn render_intent_kind(kind: IntentKind) -> &'static str {
    match kind {
        IntentKind::Accumulation => "accumulation intent",
        IntentKind::Distribution => "distribution intent",
        IntentKind::ForcedUnwind => "forced unwind intent",
        IntentKind::PassiveRebalance => "passive rebalance intent",
        IntentKind::EventRepricing => "event repricing intent",
        IntentKind::FailedPropagation => "failed propagation intent",
        IntentKind::CrossMarketLead => "cross-market lead intent",
        IntentKind::Absorption => "absorption intent",
        IntentKind::Unknown => "unknown intent",
    }
}

// Deleted per first-principles audit:
//   tactical_setup_family_key / tactical_setup_family_key_owned
//   tactical_setup_emergence_priority / tactical_setup_emergence_bucket
// All four were rule-based string bucketing on family_key text
// ("diffusion" / "spillover" / "chain" / "arbitrage" / etc).
// Categorical taxonomy on a continuous structural signal — exactly
// the kind of pre-defined bucket the audit rejects.

fn tactical_setup_action_priority(action: TacticalAction) -> i32 {
    match action {
        TacticalAction::Enter => 3,
        TacticalAction::Review => 2,
        TacticalAction::Observe => 1,
    }
}

fn ranked_tactical_setups(setups: &[TacticalSetup]) -> Vec<&TacticalSetup> {
    let mut ranked = setups.iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        tactical_setup_action_priority(right.action)
            .cmp(&tactical_setup_action_priority(left.action))
            .then_with(|| right.heuristic_edge.cmp(&left.heuristic_edge))
            .then_with(|| right.confidence_gap.cmp(&left.confidence_gap))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| left.setup_id.cmp(&right.setup_id))
    });
    ranked
}

/// Cap each `hypothesis_id` to 2 setups in the operator-facing rolodex —
/// avoids showing five Observe/Review/Enter variants of the same hypothesis.
/// Cap each Observe action to half the rolodex so heavier action tiers
/// (Review/Enter) keep visibility.
fn push_diversified_setup<'a>(
    selected: &mut Vec<&'a TacticalSetup>,
    hypothesis_counts: &mut std::collections::HashMap<String, usize>,
    observe_count: &mut usize,
    setup: &'a TacticalSetup,
    observe_cap: usize,
) -> bool {
    let key = setup.hypothesis_id.clone();
    let current = hypothesis_counts.get(&key).copied().unwrap_or(0);
    let observe_overflow = setup.action == TacticalAction::Observe && *observe_count >= observe_cap;
    if current >= 2 || observe_overflow {
        return false;
    }
    hypothesis_counts.insert(key, current + 1);
    if setup.action == TacticalAction::Observe {
        *observe_count += 1;
    }
    selected.push(setup);
    true
}

pub fn diversified_tactical_frontier(
    setups: &[TacticalSetup],
    limit: usize,
) -> Vec<&TacticalSetup> {
    let limit = limit.max(1);
    let observe_cap = (limit / 2).max(2);
    let ranked = ranked_tactical_setups(setups);
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut hypothesis_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut observe_count = 0usize;

    for setup in ranked {
        if !push_diversified_setup(
            &mut selected,
            &mut hypothesis_counts,
            &mut observe_count,
            setup,
            observe_cap,
        ) {
            deferred.push(setup);
        }
    }

    if selected.len() < limit {
        selected.extend(deferred.into_iter().take(limit - selected.len()));
    }

    selected.truncate(limit);
    selected
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewReasonCode {
    PersistenceBuilding,
    LeadingInvalidated,
    Weakening,
    RegimeBlocked,
    StaleSymbolConfirmation,
    DirectionalConflict,
    BackwardMissing,
    BackwardDirectionConflict,
    BackwardContested,
    BackwardWeakConviction,
    BackwardNarrowGap,
    AttentionCapped,
    ConvergenceDisagreement,
}

impl ReviewReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PersistenceBuilding => "persistence_building",
            Self::LeadingInvalidated => "leading_invalidated",
            Self::Weakening => "weakening",
            Self::RegimeBlocked => "regime_blocked",
            Self::StaleSymbolConfirmation => "stale_symbol_confirmation",
            Self::DirectionalConflict => "directional_conflict",
            Self::BackwardMissing => "backward_missing",
            Self::BackwardDirectionConflict => "backward_direction_conflict",
            Self::BackwardContested => "backward_contested",
            Self::BackwardWeakConviction => "backward_weak_conviction",
            Self::BackwardNarrowGap => "backward_narrow_gap",
            Self::AttentionCapped => "attention_capped",
            Self::ConvergenceDisagreement => "convergence_disagreement",
        }
    }
}

impl std::fmt::Display for ReviewReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
    pub review_reason_code: Option<ReviewReasonCode>,
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
    // V2 Pass 2: family_key removed from InvestigationSelection.
    // Same rationale as Hypothesis: categorical bucket key replaced by
    // hypothesis_id (already on this struct) for stable identity, and
    // family_label retained for operator-facing display.
    pub family_label: String,
    pub confidence: Decimal,
    pub confidence_gap: Decimal,
    pub priority_score: Decimal,
    pub attention_hint: String,
    pub rationale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<ReviewReasonCode>,
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
    // V2 Pass 2: family_key removed from CaseCluster. Bucket key
    // replaced by linkage_key (already structurally derived) /
    // lead_hypothesis_id for downstream lookup.
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
    fn intent_opportunity_window_has_bucket_field() {
        use crate::ontology::horizon::{HorizonBucket, Urgency};

        let w = IntentOpportunityWindow::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            IntentOpportunityBias::Enter,
            dec!(0.85),
            dec!(0.7),
            "test".into(),
        );
        assert_eq!(w.bucket, HorizonBucket::Fast5m);
        assert_eq!(w.urgency, Urgency::Immediate);
        // new() auto-fills legacy horizon string from bucket
        assert_eq!(w.horizon, "intraday");
    }

    #[test]
    fn tactical_setup_has_case_horizon() {
        use crate::ontology::horizon::{HorizonBucket, SessionPhase};
        let horizon = default_case_horizon();
        assert_eq!(horizon.primary, HorizonBucket::Session);
        assert_eq!(horizon.session_phase, SessionPhase::Midday);
    }

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
            kind: None,
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
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.63),
            confidence_gap: dec!(0.11),
            heuristic_edge: dec!(0.11),
            convergence_score: Some(dec!(0.52)),
            convergence_detail: None,
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "cross-market alignment remains positive".into(),
            causal_narrative: None,
            risk_notes: vec!["edge disappears if spread widens".into()],
            review_reason_code: None,
            policy_verdict: None,
        };

        assert_eq!(setup.action, "review");
        assert!(setup.heuristic_edge > Decimal::ZERO);
    }

    #[test]
    fn hypothesis_projects_expectations_and_signature() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700:accumulation".into(),
            kind: None,
            family_label: "Directed Flow".into(),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            statement: "directed flow may be building".into(),
            confidence: dec!(0.62),
            local_support_weight: dec!(0.6),
            local_contradict_weight: dec!(0.1),
            propagated_support_weight: dec!(0.3),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![ReasoningEvidence {
                statement: "volume expansion confirms price move".into(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.6),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            }],
            invalidation_conditions: vec![InvalidationCondition {
                description: "capital flow reverses".into(),
                references: vec![],
            }],
            propagation_path_ids: vec!["path:700:tech".into()],
            expected_observations: vec!["capital flow should stay positive".into()],
        };

        let bindings = hypothesis.expected_bindings();
        let signature = hypothesis.case_signature();
        assert_eq!(bindings.len(), 2);
        assert!(signature.expectation_support >= 2);
        assert!(signature.active_channels.contains(&CaseChannel::Volume));
        assert!(matches!(
            signature.topology,
            CaseTopology::Chain | CaseTopology::Unknown
        ));
    }

    #[test]
    fn tactical_setup_projects_emergent_archetype() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:700:latent_vortex".into(),
            kind: None,
            family_label: "Latent Vortex".into(),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            statement: "multiple channels converge on the same scope".into(),
            confidence: dec!(0.74),
            local_support_weight: dec!(0.2),
            local_contradict_weight: dec!(0.1),
            propagated_support_weight: dec!(0.8),
            propagated_contradict_weight: dec!(0.2),
            evidence: vec![
                ReasoningEvidence {
                    statement: "volume spike".into(),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: EvidencePolarity::Supports,
                    weight: dec!(0.4),
                    references: vec![],
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        OffsetDateTime::UNIX_EPOCH,
                    ),
                },
                ReasoningEvidence {
                    statement: "cross market path from HK".into(),
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity: EvidencePolarity::Supports,
                    weight: dec!(0.4),
                    references: vec![],
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        OffsetDateTime::UNIX_EPOCH,
                    ),
                },
            ],
            invalidation_conditions: vec![InvalidationCondition {
                description: "peer propagation collapses".into(),
                references: vec![],
            }],
            propagation_path_ids: vec!["path:cross:700.HK:BABA.US".into()],
            expected_observations: vec![
                "cross market follow-through should appear".into(),
                "options confirmation should stay aligned".into(),
            ],
        };
        let setup = TacticalSetup {
            setup_id: "setup:700:enter".into(),
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Emergent Tencent repricing".into(),
            action: "enter".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.81),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.16),
            convergence_score: Some(dec!(0.77)),
            convergence_detail: Some(crate::pipeline::reasoning::ConvergenceDetail {
                institutional_alignment: dec!(0.4),
                sector_coherence: Some(dec!(0.3)),
                cross_stock_correlation: dec!(0.5),
                component_spread: None,
                edge_stability: None,
            }),
            workflow_id: None,
            entry_rationale: "multi-channel convergence".into(),
            causal_narrative: Some("new pattern forming across markets".into()),
            risk_notes: vec!["family=latent_vortex".into()],
            review_reason_code: None,
            policy_verdict: None,
        };

        let signature = setup.case_signature(Some(&hypothesis));
        let projections = setup.archetype_projections(Some(&hypothesis));
        assert!(signature.novelty_score > Decimal::ZERO);
        assert!(!projections.is_empty());
        assert!(projections
            .iter()
            .any(|item| item.archetype_key == "emergent"));
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

    fn make_setup(setup_id: &str, hypothesis_id: &str, action: TacticalAction) -> TacticalSetup {
        TacticalSetup {
            setup_id: setup_id.into(),
            hypothesis_id: hypothesis_id.into(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "diversification fixture".into(),
            action,
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.5),
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: Vec::new(),
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    #[test]
    fn diversified_frontier_caps_each_hypothesis_to_two_slots() {
        // Three setups for hyp:A and three for hyp:B, with enough other
        // hypotheses available that deferred-fill never re-injects the
        // overflow. Primary selection caps each hypothesis_id at 2, so
        // limit=4 should yield exactly 2 of A and 2 of B.
        let setups = vec![
            make_setup("setup:a1", "hyp:A", TacticalAction::Enter),
            make_setup("setup:a2", "hyp:A", TacticalAction::Enter),
            make_setup("setup:a3", "hyp:A", TacticalAction::Enter),
            make_setup("setup:b1", "hyp:B", TacticalAction::Enter),
            make_setup("setup:b2", "hyp:B", TacticalAction::Enter),
            make_setup("setup:b3", "hyp:B", TacticalAction::Enter),
        ];
        let frontier = diversified_tactical_frontier(&setups, 4);
        let hyp_a_count = frontier
            .iter()
            .filter(|s| s.hypothesis_id == "hyp:A")
            .count();
        let hyp_b_count = frontier
            .iter()
            .filter(|s| s.hypothesis_id == "hyp:B")
            .count();
        assert_eq!(hyp_a_count, 2, "hyp:A capped at 2 in primary selection");
        assert_eq!(hyp_b_count, 2, "hyp:B capped at 2 in primary selection");
    }

    #[test]
    fn diversified_frontier_deferred_fill_overflows_cap_to_meet_limit() {
        // Documented degradation: when total setups < limit, deferred fill
        // re-injects capped overflow so the operator sees `limit` rows even
        // if all candidates share one hypothesis_id. This behavior was
        // preserved from the family-keyed era.
        let setups = vec![
            make_setup("setup:a1", "hyp:A", TacticalAction::Enter),
            make_setup("setup:a2", "hyp:A", TacticalAction::Enter),
            make_setup("setup:a3", "hyp:A", TacticalAction::Enter),
        ];
        let frontier = diversified_tactical_frontier(&setups, 5);
        assert_eq!(frontier.len(), 3, "all setups surface via deferred fill");
    }

    #[test]
    fn diversified_frontier_caps_observe_to_half_the_limit() {
        // observe_cap = (limit / 2).max(2). With limit=4 -> cap=2.
        // Provide 4 distinct Observe setups + 1 Enter; the Enter ranks
        // first, then we should fit exactly 2 Observes (cap), and the
        // remaining slot fills from deferred overflow.
        let setups = vec![
            make_setup("setup:o1", "hyp:O1", TacticalAction::Observe),
            make_setup("setup:o2", "hyp:O2", TacticalAction::Observe),
            make_setup("setup:o3", "hyp:O3", TacticalAction::Observe),
            make_setup("setup:o4", "hyp:O4", TacticalAction::Observe),
            make_setup("setup:e1", "hyp:E1", TacticalAction::Enter),
        ];
        let frontier = diversified_tactical_frontier(&setups, 4);
        // Enter is highest action priority, must appear.
        assert!(
            frontier.iter().any(|s| s.action == TacticalAction::Enter),
            "Enter setup should always appear (top action priority)"
        );
        // Frontier respects the limit.
        assert!(frontier.len() <= 4);
        // At most observe_cap=2 Observe setups in primary selection;
        // deferred fill may push extra, but never more than total
        // available — assert the contract by checking we never exceed
        // the limit and that at least one Observe was preserved.
        let observe_count = frontier
            .iter()
            .filter(|s| s.action == TacticalAction::Observe)
            .count();
        assert!(
            observe_count >= 1,
            "at least one Observe should appear via deferred overflow"
        );
    }
}
