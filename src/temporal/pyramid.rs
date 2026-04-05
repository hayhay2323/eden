use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::live_snapshot::LiveTemporalBar;
use crate::ontology::objects::Symbol;
use crate::temporal::buffer::TickHistory;
use crate::temporal::record::TickRecord;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::record::UsTickRecord;

const LIVE_HORIZONS_MINUTES: [i64; 2] = [5, 30];

#[derive(Clone, Copy)]
struct TemporalPoint {
    mark_price: Option<Decimal>,
    composite: Decimal,
    capital_flow: Decimal,
    volume: i64,
}

trait TemporalRecordView {
    fn timestamp(&self) -> OffsetDateTime;
    fn point_for_symbol(&self, symbol: &str) -> Option<TemporalPoint>;
    fn event_count(&self) -> usize;
}

impl TemporalRecordView for TickRecord {
    fn timestamp(&self) -> OffsetDateTime {
        self.timestamp
    }

    fn point_for_symbol(&self, symbol: &str) -> Option<TemporalPoint> {
        let signal = self.signals.get(&Symbol(symbol.to_string()))?;
        Some(TemporalPoint {
            mark_price: signal.mark_price,
            composite: signal.composite,
            capital_flow: signal.capital_flow_direction,
            volume: signal.trade_volume,
        })
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl TemporalRecordView for UsTickRecord {
    fn timestamp(&self) -> OffsetDateTime {
        self.timestamp
    }

    fn point_for_symbol(&self, symbol: &str) -> Option<TemporalPoint> {
        let signal = self.signals.get(&Symbol(symbol.to_string()))?;
        Some(TemporalPoint {
            mark_price: signal.mark_price,
            composite: signal.composite,
            capital_flow: signal.capital_flow_direction,
            volume: 0,
        })
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }
}

pub fn build_hk_live_temporal_bars(
    history: &TickHistory,
    symbols: &[String],
) -> Vec<LiveTemporalBar> {
    let records = history.latest_n(history.len());
    build_live_temporal_bars(records, symbols)
}

pub fn build_us_live_temporal_bars(
    history: &UsTickHistory,
    symbols: &[String],
) -> Vec<LiveTemporalBar> {
    let records = history.latest_n(history.len());
    build_live_temporal_bars(records, symbols)
}

fn build_live_temporal_bars<T>(records: Vec<&T>, symbols: &[String]) -> Vec<LiveTemporalBar>
where
    T: TemporalRecordView,
{
    let Some(latest) = records.last().copied() else {
        return Vec::new();
    };
    let latest_ts = latest.timestamp();
    let mut items = Vec::new();

    for symbol in symbols.iter().take(12) {
        for horizon in LIVE_HORIZONS_MINUTES {
            let bucket_start = floor_to_horizon(latest_ts, horizon);
            let bucket_end = bucket_start + time::Duration::minutes(horizon);
            let points = records
                .iter()
                .copied()
                .filter(|record| {
                    let timestamp = record.timestamp();
                    timestamp >= bucket_start && timestamp < bucket_end
                })
                .filter_map(|record| record.point_for_symbol(symbol).map(|point| (record, point)))
                .collect::<Vec<_>>();
            if points.is_empty() {
                continue;
            }

            let open = points.first().and_then(|(_, point)| point.mark_price);
            let close = points.last().and_then(|(_, point)| point.mark_price);
            let high = points
                .iter()
                .filter_map(|(_, point)| point.mark_price)
                .max();
            let low = points
                .iter()
                .filter_map(|(_, point)| point.mark_price)
                .min();
            let composite_open = points
                .first()
                .map(|(_, point)| point.composite)
                .unwrap_or_default();
            let composite_close = points
                .last()
                .map(|(_, point)| point.composite)
                .unwrap_or_default();
            let composite_high = points
                .iter()
                .map(|(_, point)| point.composite)
                .max()
                .unwrap_or_default();
            let composite_low = points
                .iter()
                .map(|(_, point)| point.composite)
                .min()
                .unwrap_or_default();
            let composite_sum: Decimal = points.iter().map(|(_, point)| point.composite).sum();
            let capital_flow_sum: Decimal =
                points.iter().map(|(_, point)| point.capital_flow).sum();
            let capital_flow_delta = points
                .last()
                .map(|(_, point)| point.capital_flow)
                .unwrap_or_default()
                - points
                    .first()
                    .map(|(_, point)| point.capital_flow)
                    .unwrap_or_default();
            let volume_total: i64 = points.iter().map(|(_, point)| point.volume).sum();
            let event_count: usize = points.iter().map(|(record, _)| record.event_count()).sum();
            let close_sign = composite_close.signum();
            let signal_persistence = points
                .iter()
                .rev()
                .take_while(|(_, point)| point.composite.signum() == close_sign)
                .count() as u64;

            items.push(LiveTemporalBar {
                horizon: format!("{}m", horizon),
                symbol: symbol.clone(),
                bucket_started_at: bucket_start
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                open,
                high,
                low,
                close,
                composite_open,
                composite_high,
                composite_low,
                composite_close,
                composite_mean: composite_sum / Decimal::from(points.len() as i64),
                capital_flow_sum,
                capital_flow_delta,
                volume_total,
                event_count,
                signal_persistence,
            });
        }
    }

    items
}

fn floor_to_horizon(timestamp: OffsetDateTime, horizon_minutes: i64) -> OffsetDateTime {
    let bucket_size = horizon_minutes * 60;
    let unix = timestamp.unix_timestamp();
    let floored = unix - unix.rem_euclid(bucket_size);
    OffsetDateTime::from_unix_timestamp(floored).unwrap_or(timestamp)
}
