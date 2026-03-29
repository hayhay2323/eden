use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GoverningLawKind {
    Persistence,
    Propagation,
    AbsorptionRelease,
    CouplingDecoupling,
    Competition,
    ThresholdTransition,
    Invariance,
    ReflexiveCalibration,
}

impl GoverningLawKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Persistence => "Persistence",
            Self::Propagation => "Propagation",
            Self::AbsorptionRelease => "Absorption / Release",
            Self::CouplingDecoupling => "Coupling / Decoupling",
            Self::Competition => "Competition",
            Self::ThresholdTransition => "Threshold / Transition",
            Self::Invariance => "Invariance",
            Self::ReflexiveCalibration => "Reflexive Calibration",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Persistence => "狀態是否正在持續、累積，還是只是一個瞬時噪音點。",
            Self::Propagation => "影響是否沿著 symbol / sector / market 的關係圖譜傳播。",
            Self::AbsorptionRelease => "壓力是否先被吸收，再轉成顯性價格或波動釋放。",
            Self::CouplingDecoupling => "關聯物件是同步、失耦，還是出現不合常態的背離。",
            Self::Competition => "多個解釋是否同時競爭，抑或已有明確主導敘事。",
            Self::ThresholdTransition => "系統是否在接近臨界點，或已進入相變與 regime 切換。",
            Self::Invariance => "某些解釋是否跨時間或情境仍保持穩定。",
            Self::ReflexiveCalibration => "人類校準是否正在反向修正系統對世界的解釋。",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LawActivation {
    pub kind: GoverningLawKind,
    pub label: String,
    pub score: Decimal,
    pub summary: String,
}
