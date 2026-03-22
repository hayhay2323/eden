use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::states::CompositeStateKind;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MechanismFactorSource {
    State,
    Derived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MechanismCandidateKind {
    MechanicalExecutionSignature,
    FragilityBuildUp,
    ContagionOnset,
    NarrativeFailure,
    LiquidityTrap,
    EventDrivenDislocation,
    MeanReversionSnapback,
    ArbitrageConvergence,
    CapitalRotation,
}

impl MechanismCandidateKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MechanicalExecutionSignature => "Mechanical Execution Signature",
            Self::FragilityBuildUp => "Fragility Build-up",
            Self::ContagionOnset => "Contagion Onset",
            Self::NarrativeFailure => "Narrative Failure",
            Self::LiquidityTrap => "Liquidity Trap",
            Self::EventDrivenDislocation => "Event-driven Dislocation",
            Self::MeanReversionSnapback => "Mean Reversion Snapback",
            Self::ArbitrageConvergence => "Arbitrage Convergence",
            Self::CapitalRotation => "Capital Rotation",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::MechanicalExecutionSignature => {
                "結構更像機械化執行或穩定模板在推動，而不是自由裁量型噪音。"
            }
            Self::FragilityBuildUp => "結構脆弱性正在累積，價格未必立刻失控，但承壓能力已經下降。",
            Self::ContagionOnset => {
                "局部事件正在跨範圍傳播，世界狀態更接近傳染初期而非孤立 shock。"
            }
            Self::NarrativeFailure => "原本主導的解釋框架正在失效，反證與人類校準都在增強。",
            Self::LiquidityTrap => {
                "價格停滯更像流動性卡住而非沒有力量，成交與推進之間存在吸收摩擦。"
            }
            Self::EventDrivenDislocation => {
                "事件或盤前衝擊正在主導價格偏離，case 更接近事件驅動而非穩態延續。"
            }
            Self::MeanReversionSnapback => {
                "價格伸展已超過結構支持，系統更像在累積回歸均值的修正壓力。"
            }
            Self::ArbitrageConvergence => {
                "跨市場或相對價值的失衡正在收斂，邏輯更接近套利回補而非單邊敘事。"
            }
            Self::CapitalRotation => {
                "資金正在從一組板塊切往另一組板塊，這更像替代流而不是傳染擴散。"
            }
        }
    }

    pub fn invalidation(self) -> &'static [&'static str] {
        match self {
            Self::MechanicalExecutionSignature => &[
                "若主導 source 快速分散，機械執行解釋需下調。",
                "若敘事與價格明顯失耦，應重新評估是否仍屬執行模板。",
            ],
            Self::FragilityBuildUp => &[
                "若 stress 回落且 structural degradation 緩解，脆弱性累積解釋需降級。",
                "若 coupling 恢復且反證消退，視為 fragility 暫時解除。",
            ],
            Self::ContagionOnset => &[
                "若傳播鏈快速終止或回到單點 scope，傳染初期解釋需撤回。",
                "若 cross-market linkage 消失，應下調 contagion 機率。",
            ],
            Self::NarrativeFailure => &[
                "若主要敘事重新穩定且反證減弱，narrative failure 應回收。",
                "若 human review 反向確認原敘事成立，需重算主導機制。",
            ],
            Self::LiquidityTrap => &[
                "若價格開始順暢穿透而非被吸收，liquidity trap 解釋需降級。",
                "若資金壓力與價格重新同向放大，應轉回 directional execution 類解釋。",
            ],
            Self::EventDrivenDislocation => &[
                "若事件影響快速消退且 gap 被完全吸收，event-driven 解釋需撤回。",
                "若後續沒有新的事件或盤前異常接續，應下調 dislocation 權重。",
            ],
            Self::MeanReversionSnapback => &[
                "若極端伸展獲得新的流量支持，均值回歸假說需降級。",
                "若價格繼續沿原方向擴張且背離消失，snapback 解釋應撤回。",
            ],
            Self::ArbitrageConvergence => &[
                "若跨市場失衡持續擴大而非收斂，套利收斂解釋需撤回。",
                "若 linkage 斷裂且相對價值不再回補，應改判為 local narrative 主導。",
            ],
            Self::CapitalRotation => &[
                "若板塊間的相對資金差快速收斂或反轉，capital rotation 解釋需降級。",
                "若當前板塊不再是資金流入/流出替代對象，應回到 local execution 或 event 解釋。",
            ],
        }
    }

    pub fn human_checks(self) -> &'static [&'static str] {
        match self {
            Self::MechanicalExecutionSignature => &[
                "確認 source 是否持續集中在同一驅動與節律。",
                "檢查價格與結構是否仍同向而非情緒性脫鉤。",
            ],
            Self::FragilityBuildUp => &[
                "確認壓力是否累積在多個 scope，而不只是單一 symbol。",
                "確認失耦是否已開始影響主要敘事與風險筆記。",
            ],
            Self::ContagionOnset => &[
                "確認傳播是否已跨越 sector 或跨市場邊界。",
                "檢查是否出現更多同步活化節點。",
            ],
            Self::NarrativeFailure => &[
                "確認目前反證是暫時噪音還是解釋框架真的失效。",
                "核對 reviewer / actor 的校準是否指出同一類 mismatch。",
            ],
            Self::LiquidityTrap => &[
                "確認是否存在持續的資金壓力，但價格推進明顯被吸收。",
                "檢查成交、價差或隊列是否顯示執行摩擦而非缺乏需求。",
            ],
            Self::EventDrivenDislocation => &[
                "確認事件摘要或盤前異常是否與當前 symbol 直接相關。",
                "檢查事件後的價格反應是延續還是已被市場吸收。",
            ],
            Self::MeanReversionSnapback => &[
                "確認伸展是否缺少新的資金支持，而不是趨勢剛起步。",
                "檢查 valuation / anomaly 是否已超出歷史可持續範圍。",
            ],
            Self::ArbitrageConvergence => &[
                "確認 cross-market divergence 是否在回補，而不是結構性脫鉤。",
                "檢查相對價值鏈接是否仍有效且可交易。",
            ],
            Self::CapitalRotation => &[
                "確認當前板塊是資金承接方還是流出方，而不是單點異動。",
                "檢查是否存在另一個明顯的對手板塊承接相反資金流。",
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismFactor {
    pub key: String,
    pub label: String,
    pub source: MechanismFactorSource,
    pub activation: Decimal,
    pub base_weight: Decimal,
    #[serde(default)]
    pub learned_weight_delta: Decimal,
    pub effective_weight: Decimal,
    pub contribution: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismCounterfactual {
    pub factor_key: String,
    pub factor_label: String,
    pub scenario: String,
    pub adjusted_score: Decimal,
    pub score_delta: Decimal,
    pub remains_viable: bool,
    pub remains_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MechanismCandidate {
    pub kind: MechanismCandidateKind,
    pub label: String,
    pub score: Decimal,
    pub summary: String,
    pub supporting_states: Vec<CompositeStateKind>,
    pub invalidation: Vec<String>,
    pub human_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factors: Vec<MechanismFactor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counterfactuals: Vec<MechanismCounterfactual>,
}
