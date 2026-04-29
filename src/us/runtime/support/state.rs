use super::*;
use crate::core::market::MarketId;
use crate::core::market_snapshot::CanonicalMarketSnapshot;
use crate::ontology::links::OptionSurfaceObservation;
use crate::pipeline::raw_events::{RawEventSource, RawEventStore};
use longport::quote::{PushEvent, PushEventDetail, SecurityCalcIndex, SecurityQuote, Trade};

pub(crate) struct UsLiveState {
    pub(crate) quotes: HashMap<Symbol, SecurityQuote>,
    pub(crate) trades: HashMap<Symbol, Vec<Trade>>,
    pub(crate) candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    pub(crate) raw_events: RawEventStore,
    pub(crate) push_count: u64,
    pub(crate) dirty: bool,
}

impl UsLiveState {
    pub(crate) fn new() -> Self {
        Self {
            quotes: HashMap::new(),
            trades: HashMap::new(),
            candlesticks: HashMap::new(),
            raw_events: RawEventStore::default(),
            push_count: 0,
            dirty: false,
        }
    }

    fn apply(&mut self, event: PushEvent) {
        let symbol = Symbol(event.symbol);
        self.push_count += 1;
        self.dirty = true;
        match event.detail {
            PushEventDetail::Quote(quote) => {
                let existing = self.quotes.get(&symbol);
                let merged = SecurityQuote {
                    symbol: symbol.0.clone(),
                    last_done: quote.last_done,
                    prev_close: existing.map(|q| q.prev_close).unwrap_or(Decimal::ZERO),
                    open: quote.open,
                    high: quote.high,
                    low: quote.low,
                    timestamp: quote.timestamp,
                    volume: quote.volume,
                    turnover: quote.turnover,
                    trade_status: quote.trade_status,
                    pre_market_quote: existing.and_then(|q| q.pre_market_quote.clone()),
                    post_market_quote: existing.and_then(|q| q.post_market_quote.clone()),
                    overnight_quote: existing.and_then(|q| q.overnight_quote.clone()),
                };
                self.raw_events
                    .record_quote(symbol.clone(), merged.clone(), RawEventSource::Push);
                self.quotes.insert(symbol, merged);
            }
            PushEventDetail::Trade(push_trades) => {
                self.raw_events.record_trades(
                    symbol.clone(),
                    &push_trades.trades,
                    time::OffsetDateTime::now_utc(),
                    RawEventSource::Push,
                );
                let entry = self.trades.entry(symbol).or_default();
                entry.extend(push_trades.trades);
                if entry.len() > TRADE_BUFFER_CAP_PER_SYMBOL {
                    entry.drain(..entry.len() - TRADE_BUFFER_CAP_PER_SYMBOL);
                }
            }
            PushEventDetail::Candlestick(candle) => {
                self.raw_events.record_candlestick(
                    symbol.clone(),
                    candle.candlestick.clone(),
                    RawEventSource::Push,
                );
                let entry = self.candlesticks.entry(symbol).or_default();
                entry.push(candle.candlestick);
                if entry.len() > 60 {
                    entry.drain(..entry.len() - 60);
                }
            }
            _ => {}
        }
    }

    fn apply_batch(&mut self, events: Vec<PushEvent>) {
        for event in events {
            self.apply(event);
        }
    }

    pub(crate) fn record_rest_snapshot(
        &mut self,
        update: &UsRestSnapshot,
        ingested_at: time::OffsetDateTime,
    ) {
        self.raw_events.record_calc_index_snapshot(
            &update.calc_indexes,
            ingested_at,
            RawEventSource::Rest,
        );
        self.raw_events.record_capital_flow_snapshot(
            &update.capital_flows,
            ingested_at,
            RawEventSource::Rest,
        );
        self.raw_events.record_intraday_snapshot(
            &update.intraday_lines,
            ingested_at,
            RawEventSource::Rest,
        );
        self.raw_events.record_option_surface_snapshot(
            &update.option_surfaces,
            ingested_at,
            RawEventSource::Rest,
        );
        for (symbol, quote) in &update.quotes {
            self.raw_events
                .record_quote(symbol.clone(), quote.clone(), RawEventSource::Rest);
        }
    }

    pub(crate) fn to_canonical_snapshot(
        &self,
        rest: &UsRestSnapshot,
        timestamp: time::OffsetDateTime,
    ) -> CanonicalMarketSnapshot {
        let mut quotes = self.quotes.clone();
        for (symbol, quote) in &rest.quotes {
            let merged = merge_rest_quote(quotes.get(symbol), quote.clone());
            quotes.insert(symbol.clone(), merged);
        }

        crate::ontology::snapshot::RawSnapshot {
            timestamp,
            brokers: HashMap::new(),
            calc_indexes: rest.calc_indexes.clone(),
            candlesticks: self.candlesticks.clone(),
            capital_flows: rest.capital_flows.clone(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            intraday_lines: rest.intraday_lines.clone(),
            market_temperature: None,
            option_surfaces: rest.option_surfaces.clone(),
            quotes,
            trades: self.trades.clone(),
        }
        .to_canonical_snapshot(MarketId::Us, &rest.intraday_lines)
        .with_option_surfaces(&rest.option_surfaces)
    }
}

pub(crate) struct UsRestSnapshot {
    pub(crate) quotes: HashMap<Symbol, SecurityQuote>,
    pub(crate) calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    pub(crate) capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
    pub(crate) intraday_lines: HashMap<Symbol, Vec<longport::quote::IntradayLine>>,
    pub(crate) option_surfaces: Vec<OptionSurfaceObservation>,
}

impl UsRestSnapshot {
    pub(crate) fn empty() -> Self {
        Self {
            quotes: HashMap::new(),
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            intraday_lines: HashMap::new(),
            option_surfaces: Vec::new(),
        }
    }
}

pub(crate) struct UsTickState<'a> {
    pub(crate) live: &'a mut UsLiveState,
    pub(crate) rest: &'a mut UsRestSnapshot,
    /// Optional sub-tick pressure-event bus. When `Some`, every push
    /// event is demuxed into per-channel `PressureEvent`s and published
    /// to the bus before being applied to live state — this is the
    /// hook that lets pressure channels recompute between ticks.
    pub(crate) pressure_event_bus:
        Option<std::sync::Arc<crate::pipeline::pressure_events::EventBusHandle>>,
}

pub(crate) fn merge_rest_quote(
    existing: Option<&SecurityQuote>,
    quote: SecurityQuote,
) -> SecurityQuote {
    let mut merged = quote;
    if let Some(existing) = existing {
        if merged.prev_close == Decimal::ZERO {
            merged.prev_close = existing.prev_close;
        }
        if merged.last_done == Decimal::ZERO {
            merged.last_done = existing.last_done;
        }
        if merged.open == Decimal::ZERO {
            merged.open = existing.open;
        }
        if merged.high == Decimal::ZERO {
            merged.high = existing.high;
        }
        if merged.low == Decimal::ZERO {
            merged.low = existing.low;
        }
        if merged.volume == 0 {
            merged.volume = existing.volume;
        }
        if merged.turnover == Decimal::ZERO {
            merged.turnover = existing.turnover;
        }
        if merged.pre_market_quote.is_none() {
            merged.pre_market_quote = existing.pre_market_quote.clone();
        }
        if merged.post_market_quote.is_none() {
            merged.post_market_quote = existing.post_market_quote.clone();
        }
        if merged.overnight_quote.is_none() {
            merged.overnight_quote = existing.overnight_quote.clone();
        }
    }
    merged
}

impl TickState<Vec<PushEvent>, UsRestSnapshot> for UsTickState<'_> {
    fn apply_push(&mut self, events: Vec<PushEvent>) {
        if let Some(bus) = self.pressure_event_bus.as_ref() {
            for evt in &events {
                for pe in crate::pipeline::pressure_events::demux_push_event(evt) {
                    bus.publish(pe);
                }
            }
        }
        self.live.apply_batch(events);
    }

    fn apply_update(&mut self, update: UsRestSnapshot) {
        let ingested_at = time::OffsetDateTime::now_utc();
        self.live.record_rest_snapshot(&update, ingested_at);
        let UsRestSnapshot {
            quotes,
            calc_indexes,
            capital_flows,
            intraday_lines,
            option_surfaces,
        } = update;
        for (symbol, quote) in quotes {
            self.live
                .raw_events
                .record_quote(symbol.clone(), quote.clone(), RawEventSource::Rest);
            let merged = merge_rest_quote(self.live.quotes.get(&symbol), quote);
            self.live.quotes.insert(symbol, merged);
        }
        self.rest.quotes = HashMap::new();
        self.rest.calc_indexes = calc_indexes;
        self.rest.capital_flows = capital_flows;
        self.rest.intraday_lines = intraday_lines;
        self.rest.option_surfaces = option_surfaces;
        self.live.dirty = true;
    }

    fn is_dirty(&self) -> bool {
        self.live.dirty
    }

    fn clear_dirty(&mut self) {
        self.live.dirty = false;
    }
}

pub(crate) fn prune_us_signal_records(records: &mut Vec<UsSignalRecord>, current_tick: u64) {
    records.retain(|record| {
        !record.resolved
            || current_tick.saturating_sub(record.tick_emitted) <= US_SIGNAL_RECORD_RETENTION_TICKS
    });

    if records.len() <= US_SIGNAL_RECORD_CAP {
        return;
    }

    let unresolved_count = records.iter().filter(|record| !record.resolved).count();
    let resolved_count = records.len().saturating_sub(unresolved_count);
    let resolved_keep_cap = US_SIGNAL_RECORD_CAP.saturating_sub(unresolved_count);

    if resolved_count <= resolved_keep_cap {
        return;
    }

    let mut resolved_to_drop = resolved_count - resolved_keep_cap;
    records.retain(|record| {
        if record.resolved && resolved_to_drop > 0 {
            resolved_to_drop -= 1;
            false
        } else {
            true
        }
    });
}

pub(crate) fn prune_us_workflows(workflows: &mut Vec<UsActionWorkflow>) {
    if workflows.len() <= US_WORKFLOW_CAP {
        return;
    }

    fn prune_rank(stage: UsActionStage) -> usize {
        match stage {
            UsActionStage::Reviewed => 0,
            UsActionStage::Suggested => 1,
            UsActionStage::Confirmed => 2,
            UsActionStage::Executed => 3,
            UsActionStage::Monitoring => 4,
        }
    }

    while workflows.len() > US_WORKFLOW_CAP {
        let Some((index, _)) = workflows
            .iter()
            .enumerate()
            .min_by_key(|(_, workflow)| (prune_rank(workflow.stage), workflow.entry_tick))
        else {
            break;
        };
        workflows.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::us::graph::decision::UsOrderDirection;
    use rust_decimal::Decimal;

    fn record(tick_emitted: u64, resolved: bool) -> UsSignalRecord {
        UsSignalRecord {
            setup_id: format!("setup:TEST.US:{tick_emitted}"),
            symbol: Symbol("TEST.US".into()),
            tick_emitted,
            direction: UsOrderDirection::Buy,
            composite_at_emission: Decimal::ZERO,
            price_at_emission: None,
            resolved,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: false,
        }
    }

    fn workflow(id: usize, stage: UsActionStage) -> UsActionWorkflow {
        UsActionWorkflow {
            workflow_id: format!("wf:{id}"),
            symbol: Symbol(format!("S{id}.US")),
            stage,
            setup_id: format!("setup:{id}"),
            entry_tick: id as u64,
            stage_entered_tick: id as u64,
            entry_price: None,
            confidence_at_entry: Decimal::ZERO,
            current_confidence: Decimal::ZERO,
            pnl: None,
            degradation: None,
            notes: vec![],
        }
    }

    #[test]
    fn prune_keeps_unresolved_signals_even_when_over_capacity() {
        let mut records = (0..(US_SIGNAL_RECORD_CAP + 25))
            .map(|tick| record(tick as u64, false))
            .collect::<Vec<_>>();

        prune_us_signal_records(&mut records, US_SIGNAL_RECORD_RETENTION_TICKS + 1);

        assert_eq!(records.len(), US_SIGNAL_RECORD_CAP + 25);
        assert!(records.iter().all(|record| !record.resolved));
    }

    #[test]
    fn prune_discards_oldest_resolved_records_first() {
        let mut records = (0..US_SIGNAL_RECORD_CAP)
            .map(|tick| record(tick as u64, false))
            .collect::<Vec<_>>();
        records.extend((0..10).map(|tick| record((tick + 10_000) as u64, true)));

        prune_us_signal_records(&mut records, 10_001);

        assert_eq!(
            records.iter().filter(|record| !record.resolved).count(),
            US_SIGNAL_RECORD_CAP
        );
        assert_eq!(records.iter().filter(|record| record.resolved).count(), 0);
    }

    #[test]
    fn prune_us_workflows_prefers_reviewed_and_suggested_before_monitoring() {
        let mut workflows = Vec::new();
        workflows.push(workflow(0, UsActionStage::Monitoring));
        workflows.push(workflow(1, UsActionStage::Reviewed));
        workflows.push(workflow(2, UsActionStage::Suggested));
        workflows
            .extend((3..(US_WORKFLOW_CAP + 2)).map(|id| workflow(id, UsActionStage::Monitoring)));

        prune_us_workflows(&mut workflows);

        assert_eq!(workflows.len(), US_WORKFLOW_CAP);
        assert!(workflows
            .iter()
            .all(|wf| wf.stage == UsActionStage::Monitoring));
    }

    #[test]
    fn prune_us_workflows_drops_confirmed_before_monitoring() {
        let mut workflows = Vec::new();
        workflows.push(workflow(0, UsActionStage::Monitoring));
        workflows.push(workflow(1, UsActionStage::Confirmed));
        workflows
            .extend((2..(US_WORKFLOW_CAP + 1)).map(|id| workflow(id, UsActionStage::Monitoring)));

        prune_us_workflows(&mut workflows);

        assert_eq!(workflows.len(), US_WORKFLOW_CAP);
        assert_eq!(
            workflows
                .iter()
                .filter(|wf| wf.stage == UsActionStage::Confirmed)
                .count(),
            0
        );
        assert_eq!(
            workflows
                .iter()
                .filter(|wf| wf.stage == UsActionStage::Monitoring)
                .count(),
            US_WORKFLOW_CAP
        );
    }
}
