use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A persisted candidate mechanism — a success pattern that has been promoted
/// from ephemeral tick-history observation to a durable, named mechanism
/// that can participate in hypothesis generation.
///
/// Lifecycle: shadow → assist → live
///   shadow: scored but does not influence live decisions
///   assist: influences attention + confidence boost only
///   live:   participates in hypothesis template generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateMechanismRecord {
    pub mechanism_id: String,
    pub market: String,

    // --- identity (from success pattern fingerprint) ---
    pub center_kind: String,
    pub role: String,
    pub channel_signature: String,
    pub dominant_channels: Vec<String>,
    pub top_family: String,

    // --- aggregated statistics ---
    pub samples: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_net_return: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_strength: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_coherence: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub mean_channel_diversity: Decimal,

    // --- lifecycle ---
    /// "shadow" | "assist" | "live"
    pub mode: String,
    pub promoted_at_tick: u64,
    pub last_seen_tick: u64,
    pub last_hit_tick: Option<u64>,
    pub consecutive_misses: u64,

    // --- performance since promotion ---
    pub post_promotion_hits: u64,
    pub post_promotion_misses: u64,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub post_promotion_net_return: Decimal,

    pub created_at: String,
    pub updated_at: String,
}

impl CandidateMechanismRecord {
    pub fn mechanism_key(
        market: &str,
        center_kind: &str,
        role: &str,
        channel_signature: &str,
    ) -> String {
        format!(
            "mech:{}:{}:{}:{}",
            market, center_kind, role, channel_signature
        )
    }

    pub fn record_id(&self) -> &str {
        &self.mechanism_id
    }

    pub fn hit_rate(&self) -> Decimal {
        let total = self.post_promotion_hits + self.post_promotion_misses;
        if total == 0 {
            Decimal::ZERO
        } else {
            Decimal::from(self.post_promotion_hits) / Decimal::from(total)
        }
    }

    /// Should this mechanism be decayed (demoted or removed)?
    pub fn should_decay(&self, current_tick: u64) -> bool {
        let ticks_since_seen = current_tick.saturating_sub(self.last_seen_tick);
        // Stale: not seen for 200 ticks
        if ticks_since_seen > 200 {
            return true;
        }
        // Failing: 10+ post-promotion evaluations, hit rate below 20%
        let total = self.post_promotion_hits + self.post_promotion_misses;
        if total >= 10 && self.hit_rate() < Decimal::new(20, 2) {
            return true;
        }
        // Consecutive misses
        if self.consecutive_misses >= 8 {
            return true;
        }
        false
    }

    /// Should this mechanism be promoted from shadow → assist?
    pub fn should_promote_to_assist(&self) -> bool {
        if self.mode != "shadow" {
            return false;
        }
        let total = self.post_promotion_hits + self.post_promotion_misses;
        // Need at least 5 evaluations with >40% hit rate and positive net return
        total >= 5
            && self.hit_rate() >= Decimal::new(40, 2)
            && self.post_promotion_net_return > Decimal::ZERO
    }

    /// Should this mechanism be promoted from assist → live?
    pub fn should_promote_to_live(&self) -> bool {
        if self.mode != "assist" {
            return false;
        }
        let total = self.post_promotion_hits + self.post_promotion_misses;
        // Need at least 12 evaluations with >35% hit rate and positive net return
        total >= 12
            && self.hit_rate() >= Decimal::new(35, 2)
            && self.post_promotion_net_return > Decimal::ZERO
    }

    /// Demote one level: live → assist, assist → shadow.
    /// Returns None if already shadow (should be removed).
    pub fn demoted_mode(&self) -> Option<&'static str> {
        match self.mode.as_str() {
            "live" => Some("assist"),
            "assist" => Some("shadow"),
            _ => None,
        }
    }
}
