use super::*;
use crate::ontology::objects::Symbol;
use crate::us::action::workflow::{UsActionStage, UsActionWorkflow};
use crate::us::graph::decision::{UsOrderDirection, UsSignalRecord};
use rust_decimal_macros::dec;

#[test]
fn us_market_hours_respect_dst_windows() {
    let july = time::OffsetDateTime::parse(
        "2024-07-08T13:30:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let january = time::OffsetDateTime::parse(
        "2024-01-16T14:30:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(is_us_regular_market_hours(july));
    assert!(is_us_regular_market_hours(january));

    // Pre-market was extended to 04:00 EDT → 09:00 EST = 09:00 UTC in winter.
    // 08:30 UTC is before market opens.
    let pre_open_winter = time::OffsetDateTime::parse(
        "2024-01-16T08:30:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(!is_us_regular_market_hours(pre_open_winter));
}

#[test]
fn signal_records_are_pruned_with_cap() {
    let mut records = (0..(US_SIGNAL_RECORD_CAP + 20))
        .map(|index| UsSignalRecord {
            symbol: Symbol(format!("A{index}.US")),
            tick_emitted: index as u64,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(10)),
            resolved: true,
            price_at_resolution: Some(dec!(11)),
            hit: Some(true),
            realized_return: Some(dec!(0.1)),
        })
        .collect::<Vec<_>>();

    prune_us_signal_records(&mut records, 10_000);
    assert!(records.len() <= US_SIGNAL_RECORD_CAP);
}

#[test]
fn workflows_are_pruned_with_hard_cap() {
    let mut workflows = (0..(US_WORKFLOW_CAP + 8))
        .map(|index| UsActionWorkflow {
            workflow_id: format!("wf:{index}"),
            symbol: Symbol(format!("A{index}.US")),
            stage: UsActionStage::Reviewed,
            setup_id: format!("setup:{index}"),
            entry_tick: index as u64,
            stage_entered_tick: index as u64,
            entry_price: None,
            confidence_at_entry: dec!(0.5),
            current_confidence: dec!(0.5),
            pnl: None,
            degradation: None,
            notes: vec![],
        })
        .collect::<Vec<_>>();

    prune_us_workflows(&mut workflows);
    assert!(workflows.len() <= US_WORKFLOW_CAP);
}

#[test]
fn build_quotes_skips_missing_prev_close() {
    let raw = HashMap::from([
        (
            Symbol("AAPL.US".into()),
            SecurityQuote {
                symbol: "AAPL.US".into(),
                last_done: dec!(180),
                prev_close: Decimal::ZERO,
                open: dec!(179),
                high: dec!(181),
                low: dec!(178),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 100,
                turnover: dec!(18_000),
                trade_status: TradeStatus::Normal,
                pre_market_quote: None,
                post_market_quote: None,
                overnight_quote: None,
            },
        ),
        (
            Symbol("MSFT.US".into()),
            SecurityQuote {
                symbol: "MSFT.US".into(),
                last_done: dec!(410),
                prev_close: dec!(400),
                open: dec!(402),
                high: dec!(412),
                low: dec!(399),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 100,
                turnover: dec!(41_000),
                trade_status: TradeStatus::Normal,
                pre_market_quote: None,
                post_market_quote: None,
                overnight_quote: None,
            },
        ),
    ]);

    let quotes = build_quotes(&raw);
    assert_eq!(quotes.len(), 1);
    assert_eq!(quotes[0].symbol, Symbol("MSFT.US".into()));
}

#[test]
fn rest_update_backfills_missing_prev_close() {
    let mut live = UsLiveState::new();
    live.quotes.insert(
        Symbol("AAPL.US".into()),
        SecurityQuote {
            symbol: "AAPL.US".into(),
            last_done: dec!(180),
            prev_close: Decimal::ZERO,
            open: dec!(179),
            high: dec!(181),
            low: dec!(178),
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            volume: 100,
            turnover: dec!(18_000),
            trade_status: TradeStatus::Normal,
            pre_market_quote: None,
            post_market_quote: None,
            overnight_quote: None,
        },
    );
    let mut rest = UsRestSnapshot::empty();
    let mut tick_state = UsTickState {
        live: &mut live,
        rest: &mut rest,
    };

    tick_state.apply_update(UsRestSnapshot {
        quotes: HashMap::from([(
            Symbol("AAPL.US".into()),
            SecurityQuote {
                symbol: "AAPL.US".into(),
                last_done: dec!(181),
                prev_close: dec!(175),
                open: dec!(176),
                high: dec!(182),
                low: dec!(174),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 120,
                turnover: dec!(21_000),
                trade_status: TradeStatus::Normal,
                pre_market_quote: None,
                post_market_quote: None,
                overnight_quote: None,
            },
        )]),
        calc_indexes: HashMap::new(),
        capital_flows: HashMap::new(),
        intraday_lines: HashMap::new(),
        option_surfaces: Vec::new(),
    });

    assert_eq!(
        tick_state.live.quotes[&Symbol("AAPL.US".into())].prev_close,
        dec!(175)
    );
}

#[test]
fn rest_update_refreshes_existing_quote_fields() {
    let mut live = UsLiveState::new();
    live.quotes.insert(
        Symbol("AAPL.US".into()),
        SecurityQuote {
            symbol: "AAPL.US".into(),
            last_done: dec!(180),
            prev_close: dec!(175),
            open: dec!(176),
            high: dec!(181),
            low: dec!(174),
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            volume: 100,
            turnover: dec!(18_000),
            trade_status: TradeStatus::Normal,
            pre_market_quote: None,
            post_market_quote: None,
            overnight_quote: None,
        },
    );
    let mut rest = UsRestSnapshot::empty();
    let mut tick_state = UsTickState {
        live: &mut live,
        rest: &mut rest,
    };

    tick_state.apply_update(UsRestSnapshot {
        quotes: HashMap::from([(
            Symbol("AAPL.US".into()),
            SecurityQuote {
                symbol: "AAPL.US".into(),
                last_done: dec!(182),
                prev_close: dec!(175),
                open: dec!(177),
                high: dec!(183),
                low: dec!(174),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 150,
                turnover: dec!(27_300),
                trade_status: TradeStatus::Normal,
                pre_market_quote: None,
                post_market_quote: None,
                overnight_quote: None,
            },
        )]),
        calc_indexes: HashMap::new(),
        capital_flows: HashMap::new(),
        intraday_lines: HashMap::new(),
        option_surfaces: Vec::new(),
    });

    let quote = &tick_state.live.quotes[&Symbol("AAPL.US".into())];
    assert_eq!(quote.last_done, dec!(182));
    assert_eq!(quote.open, dec!(177));
    assert_eq!(quote.high, dec!(183));
    assert_eq!(quote.volume, 150);
    assert_eq!(quote.turnover, dec!(27_300));
}
