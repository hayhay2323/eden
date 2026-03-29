use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::predicates::AtomicPredicateKind;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CompositeStateKind {
    DirectionalReinforcement,
    CrossScopeContagion,
    StructuralFragility,
    MechanisticAmbiguity,
    ReflexiveCorrection,
    EventCatalyst,
    LiquidityConstraint,
    ReversionPressure,
    CrossMarketDislocation,
    SubstitutionFlow,
}

impl CompositeStateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirectionalReinforcement => "Directional Reinforcement",
            Self::CrossScopeContagion => "Cross-scope Contagion",
            Self::StructuralFragility => "Structural Fragility",
            Self::MechanisticAmbiguity => "Mechanistic Ambiguity",
            Self::ReflexiveCorrection => "Reflexive Correction",
            Self::EventCatalyst => "Event Catalyst",
            Self::LiquidityConstraint => "Liquidity Constraint",
            Self::ReversionPressure => "Reversion Pressure",
            Self::CrossMarketDislocation => "Cross-market Dislocation",
            Self::SubstitutionFlow => "Substitution Flow",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::DirectionalReinforcement => "多個基本力在同一方向上互相支持，形成持續推進。",
            Self::CrossScopeContagion => {
                "影響開始跨 symbol / sector / market 傳播，局部異常正在系統化。"
            }
            Self::StructuralFragility => "系統承壓能力下降，結構脆弱性在價格完全反應前已顯性化。",
            Self::MechanisticAmbiguity => "目前存在多個競爭解釋，世界狀態尚未完全收斂。",
            Self::ReflexiveCorrection => "人類校準已成為新的證據，正在修正系統原有解釋。",
            Self::EventCatalyst => "事件或盤前衝擊已成為主導力量，價格反應更像由催化劑觸發。",
            Self::LiquidityConstraint => "價格推進受制於流動性吸收與執行摩擦，而非單純缺乏方向。",
            Self::ReversionPressure => "價格伸展與結構支持失衡，均值回歸壓力正在累積。",
            Self::CrossMarketDislocation => "跨市場或相對價值關係發生失衡，收斂交易條件開始成形。",
            Self::SubstitutionFlow => "資金正在板塊間替代流動，這更像輪動而不是單向傳染。",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositeState {
    pub kind: CompositeStateKind,
    pub label: String,
    pub score: Decimal,
    pub summary: String,
    pub predicates: Vec<AtomicPredicateKind>,
}
