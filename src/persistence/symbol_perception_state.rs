use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::live_snapshot::LiveMarket;
use crate::pipeline::state_engine::{
    PersistentStateEvidence, PersistentStateExpectation, PersistentStateKind, PersistentStateTrend,
    PersistentSymbolState,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolPerceptionStateRecord {
    pub state_id: String,
    pub market: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub label: String,
    pub state_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub age_ticks: u64,
    pub state_persistence_ticks: u16,
    pub direction_stability_rounds: u16,
    pub support_count: usize,
    pub contradict_count: usize,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub count_support_fraction: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub weighted_support_fraction: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub strength: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub support_weight: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub contradict_weight: Decimal,
    pub trend: String,
    // SurrealDB schema (MIGRATION_031) declares these as `TYPE array`,
    // which treats NONE as an error. `skip_serializing_if = "Vec::is_empty"`
    // caused empty vectors to be omitted from the JSON body, producing the
    // "Found NONE for field `expectations`, expected a array" error that
    // spammed every tick of the 2026-04-17 overnight run. Always serialize
    // an explicit array (possibly empty).
    #[serde(default)]
    pub supporting_evidence: Vec<PersistentStateEvidence>,
    #[serde(default)]
    pub opposing_evidence: Vec<PersistentStateEvidence>,
    #[serde(default)]
    pub missing_evidence: Vec<PersistentStateEvidence>,
    pub conflict_age_ticks: u64,
    #[serde(default)]
    pub expectations: Vec<PersistentStateExpectation>,
    #[serde(default)]
    pub active_setup_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_intent_state: Option<String>,
    pub cluster_key: String,
    pub cluster_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
    #[serde(with = "rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl SymbolPerceptionStateRecord {
    pub fn from_state(
        market: LiveMarket,
        updated_at: OffsetDateTime,
        state: &PersistentSymbolState,
    ) -> Self {
        Self {
            state_id: state.state_id.clone(),
            market: market_slug(market).into(),
            symbol: state.symbol.clone(),
            sector: state.sector.clone(),
            label: state.label.clone(),
            state_kind: state.state_kind.as_str().into(),
            direction: state.direction.clone(),
            age_ticks: state.age_ticks,
            state_persistence_ticks: state.state_persistence_ticks,
            direction_stability_rounds: state.direction_stability_rounds,
            support_count: state.support_count,
            contradict_count: state.contradict_count,
            count_support_fraction: state.count_support_fraction,
            weighted_support_fraction: state.weighted_support_fraction,
            strength: state.strength,
            confidence: state.confidence,
            support_weight: state.support_weight,
            contradict_weight: state.contradict_weight,
            trend: state.trend.as_str().into(),
            supporting_evidence: state.supporting_evidence.clone(),
            opposing_evidence: state.opposing_evidence.clone(),
            missing_evidence: state.missing_evidence.clone(),
            conflict_age_ticks: state.conflict_age_ticks,
            expectations: state.expectations.clone(),
            active_setup_ids: state.active_setup_ids.clone(),
            dominant_intent_kind: state.dominant_intent_kind.clone(),
            dominant_intent_state: state.dominant_intent_state.clone(),
            cluster_key: state.cluster_key.clone(),
            cluster_label: state.cluster_label.clone(),
            last_transition_summary: state.last_transition_summary.clone(),
            updated_at,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.state_id
    }

    pub fn to_state(&self) -> PersistentSymbolState {
        PersistentSymbolState {
            state_id: self.state_id.clone(),
            symbol: self.symbol.clone(),
            sector: self.sector.clone(),
            label: self.label.clone(),
            state_kind: PersistentStateKind::from_str(&self.state_kind),
            direction: self.direction.clone(),
            age_ticks: self.age_ticks,
            state_persistence_ticks: self.state_persistence_ticks,
            direction_stability_rounds: self.direction_stability_rounds,
            support_count: self.support_count,
            contradict_count: self.contradict_count,
            count_support_fraction: self.count_support_fraction,
            weighted_support_fraction: self.weighted_support_fraction,
            strength: self.strength,
            confidence: self.confidence,
            support_weight: self.support_weight,
            contradict_weight: self.contradict_weight,
            trend: PersistentStateTrend::from_str(&self.trend),
            supporting_evidence: self.supporting_evidence.clone(),
            opposing_evidence: self.opposing_evidence.clone(),
            missing_evidence: self.missing_evidence.clone(),
            conflict_age_ticks: self.conflict_age_ticks,
            expectations: self.expectations.clone(),
            active_setup_ids: self.active_setup_ids.clone(),
            dominant_intent_kind: self.dominant_intent_kind.clone(),
            dominant_intent_state: self.dominant_intent_state.clone(),
            cluster_key: self.cluster_key.clone(),
            cluster_label: self.cluster_label.clone(),
            last_transition_summary: self.last_transition_summary.clone(),
        }
    }
}

fn market_slug(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::pipeline::state_engine::{PersistentStateKind, PersistentStateTrend};

    #[test]
    fn record_preserves_symbol_state_shape() {
        let state = PersistentSymbolState {
            state_id: "hk:symbol:700.HK".into(),
            symbol: "700.HK".into(),
            sector: Some("Internet".into()),
            label: "Long 700.HK".into(),
            state_kind: PersistentStateKind::Continuation,
            direction: Some("buy".into()),
            age_ticks: 8,
            state_persistence_ticks: 4,
            direction_stability_rounds: 5,
            support_count: 4,
            contradict_count: 1,
            count_support_fraction: dec!(0.80),
            weighted_support_fraction: dec!(0.86),
            support_weight: dec!(1.85),
            contradict_weight: dec!(0.30),
            strength: dec!(0.82),
            confidence: dec!(0.88),
            trend: PersistentStateTrend::Strengthening,
            supporting_evidence: vec![PersistentStateEvidence {
                code: "supermajority_raw_support".into(),
                summary: "aligned".into(),
                weight: dec!(0.28),
            }],
            opposing_evidence: vec![],
            missing_evidence: vec![],
            conflict_age_ticks: 0,
            expectations: vec![],
            active_setup_ids: vec!["setup:700.HK".into()],
            dominant_intent_kind: Some("accumulation".into()),
            dominant_intent_state: Some("active".into()),
            cluster_key: "sector:Internet".into(),
            cluster_label: "Internet".into(),
            last_transition_summary: Some("flow remains dominant".into()),
        };

        let record = SymbolPerceptionStateRecord::from_state(
            LiveMarket::Hk,
            OffsetDateTime::UNIX_EPOCH,
            &state,
        );
        assert_eq!(record.market, "hk");
        assert_eq!(record.state_kind, "continuation");
        assert_eq!(record.trend, "strengthening");
        assert_eq!(record.symbol, "700.HK");
    }
}
