use rust_decimal::Decimal;
use time::{OffsetDateTime, UtcOffset, Weekday};

pub fn is_hk_regular_market_hours(timestamp: OffsetDateTime) -> bool {
    let hkt = timestamp.to_offset(UtcOffset::from_hms(8, 0, 0).expect("valid HKT offset"));
    if matches!(hkt.weekday(), Weekday::Saturday | Weekday::Sunday) {
        return false;
    }
    let minutes = i32::from(hkt.hour()) * 60 + i32::from(hkt.minute());
    (570..720).contains(&minutes) || (780..960).contains(&minutes)
}

pub fn freshness_score_from_age_secs(age_secs: i64, half_life_secs: i64) -> Decimal {
    if age_secs <= 0 {
        return Decimal::ONE;
    }
    if half_life_secs <= 0 {
        return Decimal::ZERO;
    }
    let age = Decimal::from(age_secs);
    let half_life = Decimal::from(half_life_secs);
    let ratio = age / half_life;
    (Decimal::ONE / (Decimal::ONE + ratio)).round_dp(4)
}

pub fn event_half_life_secs(kind: &str) -> i64 {
    match kind {
        "GapOpen" | "PreMarketDislocation" | "pre_market_gap" => 30 * 60,
        "VolumeSpike" | "CapitalFlowReversal" | "SectorMomentumShift" => 15 * 60,
        _ => 60 * 60,
    }
}
