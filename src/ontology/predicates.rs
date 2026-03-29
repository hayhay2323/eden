use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::laws::GoverningLawKind;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AtomicPredicateKind {
    SignalRecurs,
    ConfidenceBuilds,
    PressurePersists,
    CrossScopePropagation,
    CrossMarketLinkActive,
    SourceConcentrated,
    StructuralDegradation,
    StressAccelerating,
    PriceReasoningDivergence,
    EventCatalystActive,
    LiquidityImbalance,
    MeanReversionPressure,
    CrossMarketDislocation,
    SectorRotationPressure,
    LeaderFlipDetected,
    CounterevidencePresent,
    PositionConflict,
    PositionReinforcement,
    ConcentrationRisk,
    ExitConditionForming,
    HumanRejected,
    // Additional predicates to enrich thin governing laws
    RegimeStability,
    ConsolidationBeforeBreakout,
    BrokerReplenishActive,
    BrokerClusterAligned,
    BrokerConcentrationRisk,
}

impl AtomicPredicateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SignalRecurs => "Signal Recurs",
            Self::ConfidenceBuilds => "Confidence Builds",
            Self::PressurePersists => "Pressure Persists",
            Self::CrossScopePropagation => "Cross-scope Propagation",
            Self::CrossMarketLinkActive => "Cross-market Link Active",
            Self::SourceConcentrated => "Source Concentrated",
            Self::StructuralDegradation => "Structural Degradation",
            Self::StressAccelerating => "Stress Accelerating",
            Self::PriceReasoningDivergence => "Price / Reasoning Divergence",
            Self::EventCatalystActive => "Event Catalyst Active",
            Self::LiquidityImbalance => "Liquidity Imbalance",
            Self::MeanReversionPressure => "Mean Reversion Pressure",
            Self::CrossMarketDislocation => "Cross-market Dislocation",
            Self::SectorRotationPressure => "Sector Rotation Pressure",
            Self::LeaderFlipDetected => "Leader Flip Detected",
            Self::CounterevidencePresent => "Counterevidence Present",
            Self::PositionConflict => "Position Conflict",
            Self::PositionReinforcement => "Position Reinforcement",
            Self::ConcentrationRisk => "Concentration Risk",
            Self::ExitConditionForming => "Exit Condition Forming",
            Self::HumanRejected => "Human Rejected",
            Self::RegimeStability => "Regime Stability",
            Self::ConsolidationBeforeBreakout => "Consolidation Before Breakout",
            Self::BrokerReplenishActive => "broker_replenish_active",
            Self::BrokerClusterAligned => "broker_cluster_aligned",
            Self::BrokerConcentrationRisk => "broker_concentration_risk",
        }
    }

    pub fn law(self) -> GoverningLawKind {
        match self {
            Self::SignalRecurs | Self::ConfidenceBuilds | Self::PressurePersists => {
                GoverningLawKind::Persistence
            }
            Self::CrossScopePropagation
            | Self::CrossMarketLinkActive
            | Self::SourceConcentrated => GoverningLawKind::Propagation,
            Self::PriceReasoningDivergence => GoverningLawKind::CouplingDecoupling,
            Self::EventCatalystActive => GoverningLawKind::ThresholdTransition,
            Self::LiquidityImbalance => GoverningLawKind::AbsorptionRelease,
            Self::MeanReversionPressure => GoverningLawKind::Invariance,
            Self::CrossMarketDislocation => GoverningLawKind::CouplingDecoupling,
            Self::SectorRotationPressure => GoverningLawKind::Competition,
            Self::StructuralDegradation | Self::StressAccelerating => {
                GoverningLawKind::ThresholdTransition
            }
            Self::LeaderFlipDetected
            | Self::CounterevidencePresent
            | Self::PositionConflict
            | Self::ConcentrationRisk => GoverningLawKind::Competition,
            Self::PositionReinforcement => GoverningLawKind::Persistence,
            Self::ExitConditionForming => GoverningLawKind::ThresholdTransition,
            Self::HumanRejected => GoverningLawKind::ReflexiveCalibration,
            Self::RegimeStability => GoverningLawKind::Invariance,
            Self::ConsolidationBeforeBreakout => GoverningLawKind::AbsorptionRelease,
            Self::BrokerReplenishActive => GoverningLawKind::AbsorptionRelease,
            Self::BrokerClusterAligned => GoverningLawKind::Propagation,
            Self::BrokerConcentrationRisk => GoverningLawKind::Competition,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtomicPredicate {
    pub kind: AtomicPredicateKind,
    pub label: String,
    pub law: GoverningLawKind,
    pub score: Decimal,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}
