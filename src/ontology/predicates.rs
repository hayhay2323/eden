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
    HumanRejected,
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
            Self::HumanRejected => "Human Rejected",
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
            Self::LeaderFlipDetected | Self::CounterevidencePresent => {
                GoverningLawKind::Competition
            }
            Self::HumanRejected => GoverningLawKind::ReflexiveCalibration,
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
