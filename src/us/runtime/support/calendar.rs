use super::*;

pub(crate) fn us_market_phase(now: time::OffsetDateTime) -> &'static str {
    if is_us_cash_session_hours(now) {
        "cash_session"
    } else if is_us_regular_market_hours(now) {
        "pre_market"
    } else {
        "closed"
    }
}

pub(crate) fn is_us_regular_market_hours(now: time::OffsetDateTime) -> bool {
    let (open_hour, open_minute, close_hour, close_minute) = us_market_hours_utc(now);
    let utc_total_min = now.hour() as u32 * 60 + now.minute() as u32;
    let open_total_min = open_hour * 60 + open_minute;
    let close_total_min = close_hour * 60 + close_minute;
    utc_total_min >= open_total_min && utc_total_min < close_total_min
}

pub(crate) fn is_us_cash_session_hours(now: time::OffsetDateTime) -> bool {
    let (open_hour, open_minute, close_hour, close_minute) = us_cash_session_hours_utc(now);
    let utc_total_min = now.hour() as u32 * 60 + now.minute() as u32;
    let open_total_min = open_hour * 60 + open_minute;
    let close_total_min = close_hour * 60 + close_minute;
    utc_total_min >= open_total_min && utc_total_min < close_total_min
}

pub(crate) fn us_market_hours_utc(now: time::OffsetDateTime) -> (u32, u32, u32, u32) {
    // Include pre-market (04:00 EDT) through regular close (16:00 EDT).
    // Pre-market data is valuable for gap analysis and institutional positioning.
    if is_us_eastern_dst(now) {
        (8, 0, 20, 0) // 04:00-16:00 EDT = 08:00-20:00 UTC
    } else {
        (9, 0, 21, 0) // 04:00-16:00 EST = 09:00-21:00 UTC
    }
}

pub(crate) fn us_cash_session_hours_utc(now: time::OffsetDateTime) -> (u32, u32, u32, u32) {
    if is_us_eastern_dst(now) {
        (13, 30, 20, 0) // 09:30-16:00 EDT = 13:30-20:00 UTC
    } else {
        (14, 30, 21, 0) // 09:30-16:00 EST = 14:30-21:00 UTC
    }
}

pub(crate) fn is_us_eastern_dst(now: time::OffsetDateTime) -> bool {
    let datetime = Utc
        .timestamp_opt(now.unix_timestamp(), 0)
        .single()
        .unwrap_or_else(Utc::now);
    let year = datetime.year();
    let dst_start = us_dst_boundary_utc(year, 3, 2, 7);
    let dst_end = us_dst_boundary_utc(year, 11, 1, 6);
    datetime >= dst_start && datetime < dst_end
}

pub(crate) fn us_dst_boundary_utc(
    year: i32,
    month: u32,
    sunday_ordinal: u8,
    utc_hour: u32,
) -> chrono::DateTime<Utc> {
    let day = nth_weekday_of_month(year, month, Weekday::Sun, sunday_ordinal);
    Utc.with_ymd_and_hms(year, month, day, utc_hour, 0, 0)
        .single()
        .expect("valid DST boundary")
}

pub(crate) fn nth_weekday_of_month(year: i32, month: u32, weekday: Weekday, ordinal: u8) -> u32 {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let shift = (7 + weekday.num_days_from_monday() as i64
        - first.weekday().num_days_from_monday() as i64)
        % 7;
    1 + shift as u32 + 7 * u32::from(ordinal.saturating_sub(1))
}
