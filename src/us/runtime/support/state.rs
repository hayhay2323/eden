use super::*;

// US candlestick extraction uses the same 8% saturation point as HK: beyond that
// the range is clearly "expanded" already and more width should not add more weight.
pub(crate) fn candle_range_normalizer() -> Decimal {
    Decimal::new(8, 2)
}

pub(crate) struct UsLiveState {
    pub(crate) quotes: HashMap<Symbol, SecurityQuote>,
    pub(crate) trades: HashMap<Symbol, Vec<Trade>>,
    pub(crate) candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    pub(crate) push_count: u64,
    pub(crate) dirty: bool,
}

impl UsLiveState {
    pub(crate) fn new() -> Self {
        Self {
            quotes: HashMap::new(),
            trades: HashMap::new(),
            candlesticks: HashMap::new(),
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
                self.quotes.insert(
                    symbol.clone(),
                    SecurityQuote {
                        symbol: symbol.0,
                        last_done: quote.last_done,
                        prev_close: existing.map(|q| q.prev_close).unwrap_or(Decimal::ZERO),
                        open: quote.open,
                        high: quote.high,
                        low: quote.low,
                        timestamp: quote.timestamp,
                        volume: quote.volume,
                        turnover: quote.turnover,
                        trade_status: quote.trade_status,
                        pre_market_quote: None,
                        post_market_quote: None,
                        overnight_quote: None,
                    },
                );
            }
            PushEventDetail::Trade(push_trades) => {
                let entry = self.trades.entry(symbol).or_default();
                entry.extend(push_trades.trades);
                if entry.len() > TRADE_BUFFER_CAP_PER_SYMBOL {
                    entry.drain(..entry.len() - TRADE_BUFFER_CAP_PER_SYMBOL);
                }
            }
            PushEventDetail::Candlestick(candle) => {
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
    }
    merged
}

impl TickState<Vec<PushEvent>, UsRestSnapshot> for UsTickState<'_> {
    fn apply_push(&mut self, events: Vec<PushEvent>) {
        self.live.apply_batch(events);
    }

    fn apply_update(&mut self, update: UsRestSnapshot) {
        let UsRestSnapshot {
            quotes,
            calc_indexes,
            capital_flows,
            intraday_lines,
            option_surfaces,
        } = update;
        for (symbol, quote) in quotes {
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

    let mut overflow = workflows.len() - US_WORKFLOW_CAP;
    let mut index = 0usize;
    while overflow > 0 && index < workflows.len() {
        if matches!(workflows[index].stage, UsActionStage::Reviewed) {
            workflows.remove(index);
            overflow -= 1;
        } else {
            index += 1;
        }
    }

    if workflows.len() > US_WORKFLOW_CAP {
        workflows.drain(..workflows.len() - US_WORKFLOW_CAP);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::us::graph::decision::UsOrderDirection;
    use rust_decimal::Decimal;

    fn record(tick_emitted: u64, resolved: bool) -> UsSignalRecord {
        UsSignalRecord {
            symbol: Symbol("TEST.US".into()),
            tick_emitted,
            direction: UsOrderDirection::Buy,
            composite_at_emission: Decimal::ZERO,
            price_at_emission: None,
            resolved,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
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
}
