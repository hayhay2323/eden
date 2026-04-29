use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::reasoning::HypothesisTrack;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypothesisTrackRecord {
    pub track_id: String,
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub scope_key: String,
    pub title: String,
    pub action: String,
    pub status: String,
    pub age_ticks: u64,
    pub status_streak: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub previous_confidence: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence_change: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence_gap: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision_option")]
    pub previous_confidence_gap: Option<Decimal>,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence_gap_change: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
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

impl HypothesisTrackRecord {
    pub fn from_track(track: &HypothesisTrack) -> Self {
        Self {
            track_id: track.track_id.clone(),
            setup_id: track.setup_id.clone(),
            hypothesis_id: track.hypothesis_id.clone(),
            runner_up_hypothesis_id: track.runner_up_hypothesis_id.clone(),
            scope_key: format!("{:?}", track.scope),
            title: track.title.clone(),
            action: track.action.clone(),
            status: track.status.to_string(),
            age_ticks: track.age_ticks,
            status_streak: track.status_streak,
            confidence: track.confidence,
            previous_confidence: track.previous_confidence,
            confidence_change: track.confidence_change,
            confidence_gap: track.confidence_gap,
            previous_confidence_gap: track.previous_confidence_gap,
            confidence_gap_change: track.confidence_gap_change,
            heuristic_edge: track.heuristic_edge,
            policy_reason: track.policy_reason.clone(),
            transition_reason: track.transition_reason.clone(),
            first_seen_at: track.first_seen_at,
            last_updated_at: track.last_updated_at,
            invalidated_at: track.invalidated_at,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.track_id
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::ontology::reasoning::{HypothesisTrackStatus, ReasoningScope};
    use crate::ontology::Symbol;

    #[test]
    fn hypothesis_track_record_preserves_status_and_deltas() {
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 4,
            status_streak: 2,
            confidence: dec!(0.71),
            previous_confidence: Some(dec!(0.65)),
            confidence_change: dec!(0.06),
            confidence_gap: dec!(0.19),
            previous_confidence_gap: Some(dec!(0.12)),
            confidence_gap_change: dec!(0.07),
            heuristic_edge: dec!(0.14),
            policy_reason: "gap widened while confidence improved".into(),
            transition_reason: Some("promoted from review to enter".into()),
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let record = HypothesisTrackRecord::from_track(&track);
        assert_eq!(record.status, "strengthening");
        assert_eq!(record.status_streak, 2);
        assert_eq!(record.confidence_change, dec!(0.06));
        assert_eq!(record.previous_confidence_gap, Some(dec!(0.12)));
        assert_eq!(
            record.transition_reason.as_deref(),
            Some("promoted from review to enter")
        );
    }
}
