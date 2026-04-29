//! Contract registry for Eden's sub-KG ontology.
//!
//! This is not an inference path. It is a backend contract surface for
//! observability, validation, graph export, and query tooling. Every
//! `NodeId` must have an explicit value domain, producer, and consumer
//! declaration so the typed ontology remains inspectable as Eden grows.

use serde::Serialize;

use crate::pipeline::symbol_sub_kg::{NodeId, NodeKind};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum ValueDomain {
    Identifier,
    Label,
    Boolean,
    Count,
    Days,
    Price,
    Decimal,
    NonNegativeDecimal,
    SignedUnit,
    UnitInterval,
    Probability,
    Ratio,
    Percent,
    Rate,
    Volatility,
    Score,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NodeContract {
    pub node_id: NodeId,
    pub node_kind: NodeKind,
    pub value_domain: ValueDomain,
    pub producer: &'static str,
    pub consumers: &'static [&'static str],
    pub default_semantics: &'static str,
}

const OBSERVABILITY: &[&str] = &[
    "symbol_sub_kg::snapshot_to_ndjson",
    "visual_graph_frame",
    "graph_query_backend",
];
const BP_PRIOR: &[&str] = &[
    "loopy_bp::observe_from_subkg",
    "symbol_sub_kg::snapshot_to_ndjson",
    "visual_graph_frame",
];
const STRUCTURAL_DETECTORS: &[&str] = &[
    "cluster_sync",
    "consistency_gauge",
    "structural_contrast",
    "structural_persistence",
    "visual_graph_frame",
];
const BOOK_CONSUMERS: &[&str] = &[
    "symbol_sub_kg::book_edges",
    "cluster_sync",
    "structural_expectation",
    "visual_graph_frame",
];

fn c(
    node_id: NodeId,
    node_kind: NodeKind,
    value_domain: ValueDomain,
    producer: &'static str,
    consumers: &'static [&'static str],
    default_semantics: &'static str,
) -> NodeContract {
    NodeContract {
        node_id,
        node_kind,
        value_domain,
        producer,
        consumers,
        default_semantics,
    }
}

pub fn contract_for(node_id: &NodeId) -> NodeContract {
    match node_id {
        NodeId::Symbol => c(
            node_id.clone(),
            NodeKind::SymbolRoot,
            ValueDomain::Identifier,
            "SymbolSubKG::new_empty",
            OBSERVABILITY,
            "stable per-symbol root node",
        ),
        NodeId::LastPrice => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::Price,
            "quotes",
            STRUCTURAL_DETECTORS,
            "unset until quote update",
        ),
        NodeId::MidPrice => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::Price,
            "order_book",
            STRUCTURAL_DETECTORS,
            "unset until bid/ask are both present",
        ),
        NodeId::PrevClose => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::Price,
            "quotes",
            STRUCTURAL_DETECTORS,
            "unset until quote update",
        ),
        NodeId::Spread => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::NonNegativeDecimal,
            "order_book",
            STRUCTURAL_DETECTORS,
            "unset until bid/ask are both present",
        ),
        NodeId::DayHigh => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::Price,
            "quotes",
            OBSERVABILITY,
            "unset until quote update",
        ),
        NodeId::DayLow => c(
            node_id.clone(),
            NodeKind::Price,
            ValueDomain::Price,
            "quotes",
            OBSERVABILITY,
            "unset until quote update",
        ),
        NodeId::Volume => c(
            node_id.clone(),
            NodeKind::Volume,
            ValueDomain::NonNegativeDecimal,
            "quotes",
            STRUCTURAL_DETECTORS,
            "unset until quote update",
        ),
        NodeId::Turnover => c(
            node_id.clone(),
            NodeKind::Volume,
            ValueDomain::NonNegativeDecimal,
            "quotes",
            STRUCTURAL_DETECTORS,
            "unset until quote update",
        ),
        NodeId::VolRatio => c(
            node_id.clone(),
            NodeKind::Volume,
            ValueDomain::Ratio,
            "dimensions",
            STRUCTURAL_DETECTORS,
            "unset until dimension update",
        ),
        NodeId::FiveMinChgRate => c(
            node_id.clone(),
            NodeKind::Volume,
            ValueDomain::Percent,
            "dimensions",
            STRUCTURAL_DETECTORS,
            "unset until dimension update",
        ),
        NodeId::Time5min => c(
            node_id.clone(),
            NodeKind::Time,
            ValueDomain::Label,
            "runtime_clock",
            OBSERVABILITY,
            "label-only time bucket",
        ),
        NodeId::Time30min => c(
            node_id.clone(),
            NodeKind::Time,
            ValueDomain::Label,
            "runtime_clock",
            OBSERVABILITY,
            "label-only time bucket",
        ),
        NodeId::StateClassification => c(
            node_id.clone(),
            NodeKind::State,
            ValueDomain::Label,
            "state_engine",
            STRUCTURAL_DETECTORS,
            "label-only until state update",
        ),
        NodeId::SectorRef => c(
            node_id.clone(),
            NodeKind::SectorRef,
            ValueDomain::Identifier,
            "ontology_store",
            STRUCTURAL_DETECTORS,
            "label-only sector reference",
        ),

        NodeId::PressureOrderBook => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed order-book pressure",
        ),
        NodeId::PressureCapitalFlow => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed capital-flow pressure",
        ),
        NodeId::PressureInstitutional => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed institutional pressure",
        ),
        NodeId::PressureMomentum => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed momentum pressure",
        ),
        NodeId::PressureVolume => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed volume pressure",
        ),
        NodeId::PressureStructure => c(
            node_id.clone(),
            NodeKind::Pressure,
            ValueDomain::SignedUnit,
            "dimensions",
            BP_PRIOR,
            "zero means no observed structural pressure",
        ),

        NodeId::IntentAccumulation => c(
            node_id.clone(),
            NodeKind::Intent,
            ValueDomain::Probability,
            "intent_belief",
            BP_PRIOR,
            "posterior mass for accumulation intent",
        ),
        NodeId::IntentDistribution => c(
            node_id.clone(),
            NodeKind::Intent,
            ValueDomain::Probability,
            "intent_belief",
            BP_PRIOR,
            "posterior mass for distribution intent",
        ),
        NodeId::IntentRotation => c(
            node_id.clone(),
            NodeKind::Intent,
            ValueDomain::Probability,
            "intent_belief",
            BP_PRIOR,
            "posterior mass for rotation intent",
        ),
        NodeId::IntentVolatility => c(
            node_id.clone(),
            NodeKind::Intent,
            ValueDomain::Probability,
            "intent_belief",
            BP_PRIOR,
            "posterior mass for volatility intent",
        ),
        NodeId::IntentUnknown => c(
            node_id.clone(),
            NodeKind::Intent,
            ValueDomain::Probability,
            "intent_belief",
            BP_PRIOR,
            "posterior mass for unknown intent",
        ),

        NodeId::CallWarrantCount => c(
            node_id.clone(),
            NodeKind::Warrant,
            ValueDomain::Count,
            "hk_warrant_surface",
            STRUCTURAL_DETECTORS,
            "zero means no observed call warrants",
        ),
        NodeId::PutWarrantCount => c(
            node_id.clone(),
            NodeKind::Warrant,
            ValueDomain::Count,
            "hk_warrant_surface",
            STRUCTURAL_DETECTORS,
            "zero means no observed put warrants",
        ),
        NodeId::WarrantIvGap => c(
            node_id.clone(),
            NodeKind::Warrant,
            ValueDomain::Volatility,
            "hk_warrant_surface",
            STRUCTURAL_DETECTORS,
            "zero means no observed IV gap",
        ),
        NodeId::CallWarrantShare => c(
            node_id.clone(),
            NodeKind::Warrant,
            ValueDomain::UnitInterval,
            "hk_warrant_surface",
            STRUCTURAL_DETECTORS,
            "unset or zero when no warrant pool exists",
        ),
        NodeId::PutWarrantShare => c(
            node_id.clone(),
            NodeKind::Warrant,
            ValueDomain::UnitInterval,
            "hk_warrant_surface",
            STRUCTURAL_DETECTORS,
            "unset or zero when no warrant pool exists",
        ),

        NodeId::CapitalFlowCum => c(
            node_id.clone(),
            NodeKind::CapitalFlow,
            ValueDomain::Decimal,
            "capital_flow",
            STRUCTURAL_DETECTORS,
            "zero means no cumulative flow observed",
        ),
        NodeId::CapitalFlowAccelLast30m => c(
            node_id.clone(),
            NodeKind::CapitalFlow,
            ValueDomain::Decimal,
            "capital_flow",
            STRUCTURAL_DETECTORS,
            "zero means no acceleration observed",
        ),
        NodeId::SessionPhase => c(
            node_id.clone(),
            NodeKind::Session,
            ValueDomain::Label,
            "runtime_clock",
            OBSERVABILITY,
            "label-only session phase",
        ),
        NodeId::NextEarningsDays => c(
            node_id.clone(),
            NodeKind::Earnings,
            ValueDomain::Days,
            "calendar",
            STRUCTURAL_DETECTORS,
            "unset when no upcoming event is known",
        ),
        NodeId::InEarningsWindow => c(
            node_id.clone(),
            NodeKind::Earnings,
            ValueDomain::Boolean,
            "calendar",
            STRUCTURAL_DETECTORS,
            "one when symbol is inside event window",
        ),
        NodeId::CrossMarketBridge => c(
            node_id.clone(),
            NodeKind::CrossMarket,
            ValueDomain::Identifier,
            "bridge_registry",
            STRUCTURAL_DETECTORS,
            "label-only counterpart reference",
        ),
        NodeId::IndexMembership => c(
            node_id.clone(),
            NodeKind::Index,
            ValueDomain::Identifier,
            "ontology_store",
            STRUCTURAL_DETECTORS,
            "label-only index membership",
        ),
        NodeId::OvernightSpx => c(
            node_id.clone(),
            NodeKind::Macro,
            ValueDomain::Percent,
            "market_context",
            STRUCTURAL_DETECTORS,
            "zero means no overnight context observed",
        ),
        NodeId::SectorIndexLevel => c(
            node_id.clone(),
            NodeKind::Macro,
            ValueDomain::Decimal,
            "sector_index",
            STRUCTURAL_DETECTORS,
            "unset until sector index update",
        ),

        NodeId::TradeTapeBuyMinusSell30s => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::SignedUnit,
            "raw_trade_tape",
            STRUCTURAL_DETECTORS,
            "zero means balanced or unobserved trade tape",
        ),
        NodeId::TradeTapeAccelLast1m => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::Rate,
            "raw_trade_tape",
            STRUCTURAL_DETECTORS,
            "zero means no trade-rate acceleration",
        ),
        NodeId::DepthAsymmetryTop3 => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::UnitInterval,
            "raw_depth",
            STRUCTURAL_DETECTORS,
            "0.5 means balanced top-three depth",
        ),
        NodeId::QueueStabilityBid1 => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::Count,
            "raw_depth",
            STRUCTURAL_DETECTORS,
            "zero means bid1 has no stability streak",
        ),
        NodeId::QueueStabilityAsk1 => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::Count,
            "raw_depth",
            STRUCTURAL_DETECTORS,
            "zero means ask1 has no stability streak",
        ),
        NodeId::Vwap => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::Price,
            "trade_tape",
            STRUCTURAL_DETECTORS,
            "unset until VWAP is available",
        ),
        NodeId::VwapDeviationPct => c(
            node_id.clone(),
            NodeKind::Microstructure,
            ValueDomain::Percent,
            "trade_tape",
            STRUCTURAL_DETECTORS,
            "zero means at VWAP or unobserved",
        ),

        NodeId::InsiderHoldingPct => c(
            node_id.clone(),
            NodeKind::Holder,
            ValueDomain::Percent,
            "terrain",
            STRUCTURAL_DETECTORS,
            "zero means unknown or no insider holding observed",
        ),
        NodeId::InstitutionalHolderCount => c(
            node_id.clone(),
            NodeKind::Holder,
            ValueDomain::Count,
            "terrain",
            STRUCTURAL_DETECTORS,
            "zero means no institutional holders observed",
        ),
        NodeId::SouthboundFlowToday => c(
            node_id.clone(),
            NodeKind::Holder,
            ValueDomain::Decimal,
            "terrain",
            STRUCTURAL_DETECTORS,
            "zero means no southbound flow observed",
        ),
        NodeId::EtfHoldingPct => c(
            node_id.clone(),
            NodeKind::Holder,
            ValueDomain::Percent,
            "terrain",
            STRUCTURAL_DETECTORS,
            "zero means no ETF holding observed",
        ),

        NodeId::BigTradeCountLast1h => c(
            node_id.clone(),
            NodeKind::Event,
            ValueDomain::Count,
            "event_context",
            STRUCTURAL_DETECTORS,
            "zero means no large trade observed",
        ),
        NodeId::HasHaltedToday => c(
            node_id.clone(),
            NodeKind::Event,
            ValueDomain::Boolean,
            "event_context",
            STRUCTURAL_DETECTORS,
            "one means halted during current session",
        ),
        NodeId::VolumeSpikeFresh => c(
            node_id.clone(),
            NodeKind::Event,
            ValueDomain::Boolean,
            "event_context",
            STRUCTURAL_DETECTORS,
            "one means fresh volume-spike event",
        ),

        NodeId::LeaderLaggardScore => c(
            node_id.clone(),
            NodeKind::Role,
            ValueDomain::Score,
            "sector_roles",
            STRUCTURAL_DETECTORS,
            "zero means neutral peer role",
        ),
        NodeId::SectorRelativeStrength => c(
            node_id.clone(),
            NodeKind::Role,
            ValueDomain::Percent,
            "sector_roles",
            STRUCTURAL_DETECTORS,
            "zero means sector-average relative strength",
        ),
        NodeId::ShortInterestPct => c(
            node_id.clone(),
            NodeKind::ShortInterest,
            ValueDomain::Percent,
            "short_interest",
            STRUCTURAL_DETECTORS,
            "zero means no short-interest observation",
        ),
        NodeId::DaysToCover => c(
            node_id.clone(),
            NodeKind::ShortInterest,
            ValueDomain::Days,
            "short_interest",
            STRUCTURAL_DETECTORS,
            "zero means no days-to-cover observation",
        ),
        NodeId::HkdUsdRate => c(
            node_id.clone(),
            NodeKind::Fx,
            ValueDomain::Ratio,
            "fx_context",
            OBSERVABILITY,
            "unset until FX update",
        ),
        NodeId::UsdCnyRate => c(
            node_id.clone(),
            NodeKind::Fx,
            ValueDomain::Ratio,
            "fx_context",
            OBSERVABILITY,
            "unset until FX update",
        ),
        NodeId::TickRule => c(
            node_id.clone(),
            NodeKind::TickRule,
            ValueDomain::Label,
            "trade_tape",
            STRUCTURAL_DETECTORS,
            "label-only tick rule",
        ),
        NodeId::SpreadVelocity => c(
            node_id.clone(),
            NodeKind::BookQuality,
            ValueDomain::SignedUnit,
            "raw_depth",
            STRUCTURAL_DETECTORS,
            "zero means no spread velocity",
        ),
        NodeId::BookChurnRate => c(
            node_id.clone(),
            NodeKind::BookQuality,
            ValueDomain::Rate,
            "raw_depth",
            STRUCTURAL_DETECTORS,
            "zero means no book churn observed",
        ),
        NodeId::TradeSizeAvg30s => c(
            node_id.clone(),
            NodeKind::BookQuality,
            ValueDomain::NonNegativeDecimal,
            "raw_trade_tape",
            STRUCTURAL_DETECTORS,
            "zero means no average trade size observed",
        ),
        NodeId::LargestTradeLast5m => c(
            node_id.clone(),
            NodeKind::BookQuality,
            ValueDomain::NonNegativeDecimal,
            "raw_trade_tape",
            STRUCTURAL_DETECTORS,
            "zero means no large trade observed",
        ),
        NodeId::AnalystRatingMean => c(
            node_id.clone(),
            NodeKind::Sentiment,
            ValueDomain::Score,
            "external_sentiment",
            OBSERVABILITY,
            "unset until analyst rating is observed",
        ),
        NodeId::NewsFlowDensity1h => c(
            node_id.clone(),
            NodeKind::Sentiment,
            ValueDomain::Rate,
            "external_sentiment",
            OBSERVABILITY,
            "zero means no news flow observed",
        ),

        NodeId::OptionAtmCallIv => c(
            node_id.clone(),
            NodeKind::Option,
            ValueDomain::Volatility,
            "option_surface",
            STRUCTURAL_DETECTORS,
            "unset until option surface is available",
        ),
        NodeId::OptionAtmPutIv => c(
            node_id.clone(),
            NodeKind::Option,
            ValueDomain::Volatility,
            "option_surface",
            STRUCTURAL_DETECTORS,
            "unset until option surface is available",
        ),
        NodeId::OptionPutCallSkew => c(
            node_id.clone(),
            NodeKind::Option,
            ValueDomain::Volatility,
            "option_surface",
            STRUCTURAL_DETECTORS,
            "zero means no put/call skew observed",
        ),
        NodeId::OptionPutCallOiRatio => c(
            node_id.clone(),
            NodeKind::Option,
            ValueDomain::Ratio,
            "option_surface",
            STRUCTURAL_DETECTORS,
            "unset until OI ratio is available",
        ),
        NodeId::OptionTotalOi => c(
            node_id.clone(),
            NodeKind::Option,
            ValueDomain::Count,
            "option_surface",
            STRUCTURAL_DETECTORS,
            "zero means no option OI observed",
        ),

        NodeId::OutcomeMemory => c(
            node_id.clone(),
            NodeKind::Memory,
            ValueDomain::SignedUnit,
            "decision_ledger",
            BP_PRIOR,
            "zero means no resolved outcome memory",
        ),
        NodeId::EngramAlignment => c(
            node_id.clone(),
            NodeKind::Memory,
            ValueDomain::SignedUnit,
            "regime_analog_index",
            BP_PRIOR,
            "zero means no current regime analog bias",
        ),
        NodeId::WlAnalogConfidence => c(
            node_id.clone(),
            NodeKind::Memory,
            ValueDomain::UnitInterval,
            "symbol_wl_analog_index",
            BP_PRIOR,
            "zero means no prior WL signature visits",
        ),
        NodeId::BeliefEntropy => c(
            node_id.clone(),
            NodeKind::Belief,
            ValueDomain::UnitInterval,
            "belief_field",
            BP_PRIOR,
            "one means max uncertainty in categorical belief",
        ),
        NodeId::BeliefSampleCount => c(
            node_id.clone(),
            NodeKind::Belief,
            ValueDomain::UnitInterval,
            "belief_field",
            BP_PRIOR,
            "zero means no accumulated belief samples",
        ),
        NodeId::ForecastAccuracy => c(
            node_id.clone(),
            NodeKind::Causal,
            ValueDomain::UnitInterval,
            "active_probe",
            BP_PRIOR,
            "0.5 means neutral forecast accuracy prior",
        ),
        NodeId::SectorIntentBull => c(
            node_id.clone(),
            NodeKind::Sector,
            ValueDomain::Probability,
            "sector_intent",
            BP_PRIOR,
            "sector posterior mass copied to member symbol",
        ),
        NodeId::SectorIntentBear => c(
            node_id.clone(),
            NodeKind::Sector,
            ValueDomain::Probability,
            "sector_intent",
            BP_PRIOR,
            "sector posterior mass copied to member symbol",
        ),
        NodeId::KlSurpriseMagnitude => c(
            node_id.clone(),
            NodeKind::Surprise,
            ValueDomain::UnitInterval,
            "kl_surprise",
            BP_PRIOR,
            "tanh(|max_z|/2) over per-channel KL z-scores",
        ),
        NodeId::KlSurpriseDirection => c(
            node_id.clone(),
            NodeKind::Surprise,
            ValueDomain::SignedUnit,
            "kl_surprise",
            BP_PRIOR,
            "sign of dominant channel mean shift",
        ),

        NodeId::BidLevel(_) => c(
            node_id.clone(),
            NodeKind::BidDepth,
            ValueDomain::Price,
            "order_book",
            BOOK_CONSUMERS,
            "unset until bid depth update",
        ),
        NodeId::AskLevel(_) => c(
            node_id.clone(),
            NodeKind::AskDepth,
            ValueDomain::Price,
            "order_book",
            BOOK_CONSUMERS,
            "unset until ask depth update",
        ),
        NodeId::Broker(_) => c(
            node_id.clone(),
            NodeKind::Broker,
            ValueDomain::Boolean,
            "broker_queue",
            STRUCTURAL_DETECTORS,
            "one when broker is active on current book",
        ),
        NodeId::FundHolder(_) => c(
            node_id.clone(),
            NodeKind::FundHolder,
            ValueDomain::Boolean,
            "terrain",
            STRUCTURAL_DETECTORS,
            "one when holder is observed for symbol",
        ),
    }
}

pub fn fixed_node_contracts() -> Vec<NodeContract> {
    fixed_node_ids()
        .iter()
        .map(contract_for)
        .collect::<Vec<_>>()
}

pub fn fixed_node_ids() -> Vec<NodeId> {
    let mut ids = vec![
        NodeId::Symbol,
        NodeId::LastPrice,
        NodeId::MidPrice,
        NodeId::PrevClose,
        NodeId::Spread,
        NodeId::DayHigh,
        NodeId::DayLow,
        NodeId::Volume,
        NodeId::Turnover,
        NodeId::VolRatio,
        NodeId::FiveMinChgRate,
        NodeId::Time5min,
        NodeId::Time30min,
        NodeId::StateClassification,
        NodeId::SectorRef,
        NodeId::PressureOrderBook,
        NodeId::PressureCapitalFlow,
        NodeId::PressureInstitutional,
        NodeId::PressureMomentum,
        NodeId::PressureVolume,
        NodeId::PressureStructure,
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
        NodeId::IntentRotation,
        NodeId::IntentVolatility,
        NodeId::IntentUnknown,
        NodeId::CallWarrantCount,
        NodeId::PutWarrantCount,
        NodeId::WarrantIvGap,
        NodeId::CallWarrantShare,
        NodeId::PutWarrantShare,
        NodeId::CapitalFlowCum,
        NodeId::CapitalFlowAccelLast30m,
        NodeId::SessionPhase,
        NodeId::NextEarningsDays,
        NodeId::InEarningsWindow,
        NodeId::CrossMarketBridge,
        NodeId::IndexMembership,
        NodeId::OvernightSpx,
        NodeId::SectorIndexLevel,
        NodeId::TradeTapeBuyMinusSell30s,
        NodeId::TradeTapeAccelLast1m,
        NodeId::DepthAsymmetryTop3,
        NodeId::QueueStabilityBid1,
        NodeId::QueueStabilityAsk1,
        NodeId::Vwap,
        NodeId::VwapDeviationPct,
        NodeId::InsiderHoldingPct,
        NodeId::InstitutionalHolderCount,
        NodeId::SouthboundFlowToday,
        NodeId::EtfHoldingPct,
        NodeId::BigTradeCountLast1h,
        NodeId::HasHaltedToday,
        NodeId::VolumeSpikeFresh,
        NodeId::LeaderLaggardScore,
        NodeId::SectorRelativeStrength,
        NodeId::ShortInterestPct,
        NodeId::DaysToCover,
        NodeId::HkdUsdRate,
        NodeId::UsdCnyRate,
        NodeId::TickRule,
        NodeId::SpreadVelocity,
        NodeId::BookChurnRate,
        NodeId::TradeSizeAvg30s,
        NodeId::LargestTradeLast5m,
        NodeId::AnalystRatingMean,
        NodeId::NewsFlowDensity1h,
        NodeId::OptionAtmCallIv,
        NodeId::OptionAtmPutIv,
        NodeId::OptionPutCallSkew,
        NodeId::OptionPutCallOiRatio,
        NodeId::OptionTotalOi,
        NodeId::OutcomeMemory,
        NodeId::EngramAlignment,
        NodeId::WlAnalogConfidence,
        NodeId::BeliefEntropy,
        NodeId::BeliefSampleCount,
        NodeId::ForecastAccuracy,
        NodeId::SectorIntentBull,
        NodeId::SectorIntentBear,
        NodeId::KlSurpriseMagnitude,
        NodeId::KlSurpriseDirection,
    ];
    for level in 1..=10 {
        ids.push(NodeId::BidLevel(level));
        ids.push(NodeId::AskLevel(level));
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::symbol_sub_kg::SymbolSubKG;
    use chrono::Utc;

    #[test]
    fn fixed_contracts_match_subkg_template_nodes() {
        let kg = SymbolSubKG::new_empty("TEST.US".to_string(), Utc::now());
        let contracts = fixed_node_contracts();

        assert_eq!(contracts.len(), kg.nodes.len());
        for contract in contracts {
            let activation = kg
                .nodes
                .get(&contract.node_id)
                .unwrap_or_else(|| panic!("missing fixed node {:?}", contract.node_id));
            assert_eq!(
                activation.kind, contract.node_kind,
                "contract kind mismatch for {:?}",
                contract.node_id
            );
        }
    }

    #[test]
    fn dynamic_node_contracts_are_generic_by_family() {
        assert_eq!(
            contract_for(&NodeId::Broker("B001".to_string())).node_kind,
            NodeKind::Broker
        );
        assert_eq!(
            contract_for(&NodeId::BidLevel(7)).node_kind,
            NodeKind::BidDepth
        );
        assert_eq!(
            contract_for(&NodeId::AskLevel(4)).node_kind,
            NodeKind::AskDepth
        );
        assert_eq!(
            contract_for(&NodeId::FundHolder("fund".to_string())).node_kind,
            NodeKind::FundHolder
        );
    }

    #[test]
    fn v2_bp_node_contracts_point_to_bp_prior_consumer() {
        for id in [
            NodeId::OutcomeMemory,
            NodeId::EngramAlignment,
            NodeId::WlAnalogConfidence,
            NodeId::BeliefEntropy,
            NodeId::BeliefSampleCount,
            NodeId::ForecastAccuracy,
            NodeId::SectorIntentBull,
            NodeId::SectorIntentBear,
            NodeId::KlSurpriseMagnitude,
            NodeId::KlSurpriseDirection,
        ] {
            let contract = contract_for(&id);
            assert!(
                contract.consumers.contains(&"loopy_bp::observe_from_subkg"),
                "{id:?} must feed BP through sub-KG prior observation"
            );
        }
    }
}
