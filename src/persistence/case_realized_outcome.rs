use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::temporal::lineage::CaseRealizedOutcome;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRealizedOutcomeRecord {
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub market: String,
    pub symbol: Option<String>,
    pub primary_lens: Option<String>,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub entry_tick: u64,
    #[serde(with = "rfc3339")]
    pub entry_timestamp: OffsetDateTime,
    pub resolved_tick: u64,
    #[serde(with = "rfc3339")]
    pub resolved_at: OffsetDateTime,
    pub direction: i8,
    pub return_pct: Decimal,
    pub net_return: Decimal,
    pub max_favorable_excursion: Decimal,
    pub max_adverse_excursion: Decimal,
    pub followed_through: bool,
    pub invalidated: bool,
    pub structure_retained: bool,
    pub convergence_score: Decimal,
}

impl CaseRealizedOutcomeRecord {
    pub fn from_outcome(
        outcome: &CaseRealizedOutcome,
        market: &str,
        primary_lens: Option<String>,
    ) -> Self {
        Self {
            setup_id: outcome.setup_id.clone(),
            workflow_id: outcome.workflow_id.clone(),
            market: market.to_string(),
            symbol: outcome.symbol.clone(),
            primary_lens,
            family: outcome.family.clone(),
            session: outcome.session.clone(),
            market_regime: outcome.market_regime.clone(),
            entry_tick: outcome.entry_tick,
            entry_timestamp: outcome.entry_timestamp,
            resolved_tick: outcome.resolved_tick,
            resolved_at: outcome.resolved_at,
            direction: outcome.direction,
            return_pct: outcome.return_pct,
            net_return: outcome.net_return,
            max_favorable_excursion: outcome.max_favorable_excursion,
            max_adverse_excursion: outcome.max_adverse_excursion,
            followed_through: outcome.followed_through,
            invalidated: outcome.invalidated,
            structure_retained: outcome.structure_retained,
            convergence_score: outcome.convergence_score,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.setup_id
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn realized_outcome_record_preserves_key_fields() {
        let outcome = CaseRealizedOutcome {
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            symbol: Some("700.HK".into()),
            entry_tick: 10,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 25,
            resolved_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(30),
            family: "Flow".into(),
            session: "am".into(),
            market_regime: "risk_on".into(),
            direction: 1,
            return_pct: dec!(0.04),
            net_return: dec!(0.035),
            max_favorable_excursion: dec!(0.06),
            max_adverse_excursion: dec!(-0.02),
            followed_through: true,
            invalidated: false,
            structure_retained: true,
            convergence_score: dec!(0.5),
        };

        let record = CaseRealizedOutcomeRecord::from_outcome(
            &outcome,
            "hk",
            Some("iceberg".into()),
        );
        assert_eq!(record.market, "hk");
        assert_eq!(record.setup_id, "setup:1");
        assert_eq!(record.primary_lens.as_deref(), Some("iceberg"));
        assert_eq!(record.net_return, dec!(0.035));
        assert!(record.followed_through);
    }
}
