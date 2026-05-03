use super::*;
use crate::core::market_snapshot::{CanonicalMarketStatus, CanonicalQuote};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{default_case_horizon, DecisionLineage, TacticalSetup};
use crate::us::action::workflow::{UsActionStage, UsActionWorkflow};
use crate::us::graph::decision::{UsOrderDirection, UsSignalRecord};
use crate::us::pipeline::signals::UsObservationSnapshot;
use longport::quote::{PrePostQuote, SecurityQuote, TradeStatus};
use rust_decimal::Decimal;
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

    let pre_market_dst = time::OffsetDateTime::parse(
        "2024-07-08T12:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(is_us_regular_market_hours(pre_market_dst));
    assert!(!is_us_cash_session_hours(pre_market_dst));
}

#[test]
fn signal_records_are_pruned_with_cap() {
    let mut records = (0..(US_SIGNAL_RECORD_CAP + 20))
        .map(|index| UsSignalRecord {
            setup_id: format!("setup:A{index}.US"),
            symbol: Symbol(format!("A{index}.US")),
            tick_emitted: index as u64,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(10)),
            resolved: true,
            price_at_resolution: Some(dec!(11)),
            hit: Some(true),
            realized_return: Some(dec!(0.1)),
            is_actionable_tier: false,
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
fn oscillation_symbols_keep_disappeared_names_until_quiet_reset() {
    let mut tracker = crate::pipeline::oscillation::OscillationTracker::with_window(20);
    for present in [true, false] {
        tracker.observe("COIN.US", present);
    }
    let current = HashSet::new();
    let observed = us_oscillation_observation_symbols(&tracker, &current);
    assert!(observed.contains(&"COIN.US".to_string()));
}

#[test]
fn dropping_inactive_symbols_clears_stale_flip_state() {
    let mut velocity = crate::pipeline::signal_velocity::SignalVelocityTracker::new();
    velocity.observe("MSTR.US", dec!(0.80), 1);
    velocity.observe("MSTR.US", dec!(0.90), 2);
    let mut flips = crate::pipeline::direction_flip::DirectionFlipTracker::new();
    flips.observe("MSTR.US", crate::pipeline::direction_flip::Direction::Long);
    let previous = HashSet::from(["MSTR.US".to_string()]);
    let current = HashSet::new();

    us_drop_inactive_symbol_trackers(&mut velocity, &mut flips, &previous, &current);

    assert!(velocity.velocity("MSTR.US").is_none());
    assert!(flips.last_direction("MSTR.US").is_none());
}

fn simple_setup(symbol: &str) -> TacticalSetup {
    TacticalSetup {
        setup_id: format!("setup:{symbol}:enter"),
        hypothesis_id: format!("hyp:{symbol}"),
        runner_up_hypothesis_id: None,
        provenance: crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            time::OffsetDateTime::UNIX_EPOCH,
        ),
        lineage: DecisionLineage::default(),
        scope: crate::ontology::ReasoningScope::Symbol(Symbol(symbol.into())),
        title: format!("Long {symbol}"),
        action: "enter".into(),
        direction: None,
        horizon: default_case_horizon(),
        confidence: dec!(0.8),
        confidence_gap: dec!(0.1),
        heuristic_edge: dec!(0.2),
        convergence_score: Some(dec!(0.55)),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "test".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    }
}

#[test]
fn workflow_does_not_confirm_without_price() {
    let mut workflow = UsActionWorkflow::from_setup(&simple_setup("NVDA.US"), 12, None);

    let advance = advance_us_workflow_with_price(&mut workflow, None, 12);

    assert_eq!(advance, UsWorkflowAdvance::default());
    assert_eq!(workflow.stage, UsActionStage::Suggested);
    assert!(workflow.entry_price.is_none());
}

#[test]
fn workflow_advances_from_confirmed_when_price_arrives() {
    let mut workflow = UsActionWorkflow::from_setup(&simple_setup("AMD.US"), 10, None);
    workflow.confirm(10).unwrap();

    let advance = advance_us_workflow_with_price(&mut workflow, Some(dec!(101.5)), 11);

    assert_eq!(
        advance,
        UsWorkflowAdvance {
            confirmed: false,
            executed: true,
            monitoring: true
        }
    );
    assert_eq!(workflow.stage, UsActionStage::Monitoring);
    assert_eq!(workflow.entry_price, Some(dec!(101.5)));
}

#[cfg(feature = "persistence")]
#[test]
fn restore_persisted_workflows_rehydrates_open_position_without_duplicates() {
    let setup = TacticalSetup {
        setup_id: "setup:AAPL.US:enter".into(),
        hypothesis_id: "hyp:AAPL.US".into(),
        runner_up_hypothesis_id: None,
        provenance: crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            time::OffsetDateTime::UNIX_EPOCH,
        ),
        lineage: DecisionLineage::default(),
        scope: crate::ontology::ReasoningScope::Symbol(Symbol("AAPL.US".into())),
        title: "Long AAPL.US".into(),
        action: "enter".into(),
        direction: None,
        horizon: default_case_horizon(),
        confidence: dec!(0.8),
        confidence_gap: dec!(0.1),
        heuristic_edge: dec!(0.2),
        convergence_score: Some(dec!(0.55)),
        convergence_detail: None,
        workflow_id: Some("workflow:setup:AAPL.US:enter".into()),
        entry_rationale: "test".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: "workflow:setup:AAPL.US:enter".into(),
        title: "Position AAPL.US".into(),
        payload: serde_json::json!({
            "entry_tick": 12,
            "stage_entered_tick": 18,
            "entry_price": "101",
            "confidence_at_entry": "0.66",
            "current_confidence": "0.55",
            "notes": ["restored"]
        }),
        current_stage: crate::action::workflow::ActionStage::Monitor,
        execution_policy: crate::action::workflow::ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: time::OffsetDateTime::UNIX_EPOCH,
        actor: Some("tracker".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("restored".into()),
    };
    let dim_snapshot = UsDimensionSnapshot {
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::from([(
            Symbol("AAPL.US".into()),
            crate::us::pipeline::dimensions::UsSymbolDimensions {
                capital_flow_direction: dec!(0.2),
                price_momentum: dec!(0.3),
                volume_profile: dec!(0.1),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                multi_horizon_momentum: Decimal::ZERO,
            },
        )]),
    };
    let mut workflows = Vec::new();
    let mut tracker = UsPositionTracker::new();
    let records = HashMap::from([(record.workflow_id.clone(), record)]);

    restore_persisted_us_workflows(
        &mut workflows,
        &mut tracker,
        &[setup.clone()],
        &records,
        &dim_snapshot,
    );
    restore_persisted_us_workflows(
        &mut workflows,
        &mut tracker,
        &[setup],
        &records,
        &dim_snapshot,
    );

    assert_eq!(workflows.len(), 1);
    assert!(tracker.is_active(&Symbol("AAPL.US".into())));
}

#[cfg(feature = "persistence")]
#[test]
fn restore_persisted_reviewed_workflow_removes_open_position() {
    let setup = TacticalSetup {
        setup_id: "setup:MSFT.US:enter".into(),
        hypothesis_id: "hyp:MSFT.US".into(),
        runner_up_hypothesis_id: None,
        provenance: crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            time::OffsetDateTime::UNIX_EPOCH,
        ),
        lineage: DecisionLineage::default(),
        scope: crate::ontology::ReasoningScope::Symbol(Symbol("MSFT.US".into())),
        title: "Long MSFT.US".into(),
        action: "enter".into(),
        direction: None,
        horizon: default_case_horizon(),
        confidence: dec!(0.8),
        confidence_gap: dec!(0.1),
        heuristic_edge: dec!(0.2),
        convergence_score: Some(dec!(0.55)),
        convergence_detail: None,
        workflow_id: Some("workflow:setup:MSFT.US:enter".into()),
        entry_rationale: "test".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: "workflow:setup:MSFT.US:enter".into(),
        title: "Position MSFT.US".into(),
        payload: serde_json::json!({
            "entry_tick": 12,
            "stage_entered_tick": 18,
            "entry_price": "101",
            "confidence_at_entry": "0.66",
            "current_confidence": "0.55",
            "notes": ["closed"]
        }),
        current_stage: crate::action::workflow::ActionStage::Review,
        execution_policy: crate::action::workflow::ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::TerminalReviewStage,
        recorded_at: time::OffsetDateTime::UNIX_EPOCH,
        actor: Some("operator".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("closed".into()),
    };
    let dim_snapshot = UsDimensionSnapshot {
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::from([(
            Symbol("MSFT.US".into()),
            crate::us::pipeline::dimensions::UsSymbolDimensions {
                capital_flow_direction: dec!(0.2),
                price_momentum: dec!(0.3),
                volume_profile: dec!(0.1),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                multi_horizon_momentum: Decimal::ZERO,
            },
        )]),
    };
    let mut workflows = vec![UsActionWorkflow {
        workflow_id: "workflow:setup:MSFT.US:enter".into(),
        symbol: Symbol("MSFT.US".into()),
        stage: UsActionStage::Monitoring,
        setup_id: "setup:MSFT.US:enter".into(),
        entry_tick: 12,
        stage_entered_tick: 18,
        entry_price: Some(dec!(101)),
        confidence_at_entry: dec!(0.66),
        current_confidence: dec!(0.55),
        pnl: None,
        degradation: None,
        notes: vec![],
    }];
    let mut tracker = UsPositionTracker::new();
    tracker.enter(UsStructuralFingerprint::capture(
        Symbol("MSFT.US".into()),
        12,
        Some(dec!(101)),
        dim_snapshot
            .dimensions
            .get(&Symbol("MSFT.US".into()))
            .unwrap(),
    ));
    let records = HashMap::from([(record.workflow_id.clone(), record)]);

    restore_persisted_us_workflows(
        &mut workflows,
        &mut tracker,
        &[setup],
        &records,
        &dim_snapshot,
    );

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].stage, UsActionStage::Reviewed);
    assert!(!tracker.is_active(&Symbol("MSFT.US".into())));
}

#[cfg(feature = "persistence")]
#[test]
fn restore_persisted_workflows_bootstraps_without_current_setup_seed() {
    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: "workflow:setup:NVDA.US:enter".into(),
        title: "Position NVDA.US".into(),
        payload: serde_json::json!({
            "setup_id": "setup:NVDA.US:enter",
            "symbol": "NVDA.US",
            "entry_tick": 12,
            "stage_entered_tick": 18,
            "entry_price": "910",
            "confidence_at_entry": "0.77",
            "current_confidence": "0.72",
            "notes": ["restored"]
        }),
        current_stage: crate::action::workflow::ActionStage::Monitor,
        execution_policy: crate::action::workflow::ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: time::OffsetDateTime::UNIX_EPOCH,
        actor: Some("tracker".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("restored".into()),
    };
    let dim_snapshot = UsDimensionSnapshot {
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::from([(
            Symbol("NVDA.US".into()),
            crate::us::pipeline::dimensions::UsSymbolDimensions {
                capital_flow_direction: dec!(0.2),
                price_momentum: dec!(0.3),
                volume_profile: dec!(0.1),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: Decimal::ZERO,
                multi_horizon_momentum: Decimal::ZERO,
            },
        )]),
    };
    let records = HashMap::from([(record.workflow_id.clone(), record)]);
    let mut workflows = Vec::new();
    let mut tracker = UsPositionTracker::new();

    restore_persisted_us_workflows(&mut workflows, &mut tracker, &[], &records, &dim_snapshot);
    restore_persisted_us_workflows(&mut workflows, &mut tracker, &[], &records, &dim_snapshot);

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].workflow_id, "workflow:setup:NVDA.US:enter");
    assert_eq!(workflows[0].setup_id, "setup:NVDA.US:enter");
    assert_eq!(workflows[0].symbol, Symbol("NVDA.US".into()));
    assert_eq!(workflows[0].stage, UsActionStage::Monitoring);
    assert_eq!(workflows[0].entry_tick, 12);
    assert!(tracker.is_active(&Symbol("NVDA.US".into())));
}

#[test]
fn scorecard_records_emit_from_tactical_setups_once_per_setup_id() {
    let symbol = Symbol("AAPL.US".into());
    let setup = TacticalSetup {
        setup_id: "setup:AAPL.US:enter".into(),
        hypothesis_id: "hyp:AAPL.US".into(),
        runner_up_hypothesis_id: None,
        provenance: crate::ontology::ProvenanceMetadata::new(
            crate::ontology::ProvenanceSource::Computed,
            time::OffsetDateTime::UNIX_EPOCH,
        ),
        lineage: DecisionLineage::default(),
        scope: crate::ontology::ReasoningScope::Symbol(symbol.clone()),
        title: "Long AAPL.US".into(),
        action: "enter".into(),
        direction: None,
        horizon: default_case_horizon(),
        confidence: dec!(0.8),
        confidence_gap: dec!(0.1),
        heuristic_edge: dec!(0.2),
        convergence_score: Some(dec!(0.55)),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: "test".into(),
        causal_narrative: None,
        risk_notes: vec![],
        review_reason_code: None,
        policy_verdict: None,
    };
    let quotes = HashMap::from([(
        symbol.clone(),
        CanonicalQuote {
            symbol: symbol.clone(),
            last_done: dec!(180),
            prev_close: dec!(179),
            open: dec!(179),
            high: dec!(181),
            low: dec!(178),
            volume: 100,
            turnover: dec!(18000),
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            market_status: CanonicalMarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
    )]);

    let mut records = Vec::new();
    emit_setup_scorecard_records(&mut records, 42, std::slice::from_ref(&setup), &quotes);
    emit_setup_scorecard_records(&mut records, 43, std::slice::from_ref(&setup), &quotes);

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].setup_id, "setup:AAPL.US:enter");
    assert_eq!(records[0].symbol, symbol);
    assert_eq!(records[0].direction, UsOrderDirection::Buy);
    assert_eq!(records[0].price_at_emission, Some(dec!(180)));
    assert!(records[0].is_actionable_tier);
}

#[tokio::test]
async fn rest_update_backfills_missing_prev_close() {
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
        pressure_event_bus: None,
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
fn observation_snapshot_from_canonical_skips_zero_prev_close_quotes() {
    use crate::core::market::MarketId;
    use crate::core::market_snapshot::{
        CanonicalMarketSnapshot, CanonicalMarketStatus, CanonicalQuote,
    };

    let quotes = HashMap::from([
        (
            Symbol("AAPL.US".into()),
            CanonicalQuote {
                symbol: Symbol("AAPL.US".into()),
                last_done: dec!(180),
                prev_close: Decimal::ZERO,
                open: dec!(179),
                high: dec!(181),
                low: dec!(178),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 100,
                turnover: dec!(18_000),
                market_status: CanonicalMarketStatus::Normal,
                pre_market: None,
                post_market: None,
            },
        ),
        (
            Symbol("MSFT.US".into()),
            CanonicalQuote {
                symbol: Symbol("MSFT.US".into()),
                last_done: dec!(410),
                prev_close: dec!(400),
                open: dec!(402),
                high: dec!(412),
                low: dec!(399),
                timestamp: time::OffsetDateTime::UNIX_EPOCH,
                volume: 100,
                turnover: dec!(41_000),
                market_status: CanonicalMarketStatus::Normal,
                pre_market: None,
                post_market: None,
            },
        ),
    ]);

    let snapshot = CanonicalMarketSnapshot {
        timestamp: time::OffsetDateTime::UNIX_EPOCH,
        market: MarketId::Us,
        quotes,
        trades: HashMap::new(),
        candlesticks: HashMap::new(),
        order_books: HashMap::new(),
        broker_queues: HashMap::new(),
        calc_indexes: HashMap::new(),
        capital_flow_series: HashMap::new(),
        capital_distributions: HashMap::new(),
        intraday: HashMap::new(),
        option_surfaces: Vec::new(),
        market_temperature: None,
    };

    let built = UsObservationSnapshot::from_canonical_market(&snapshot);
    let quote_symbols = built
        .observations
        .iter()
        .filter_map(|obs| match &obs.value {
            crate::us::pipeline::signals::UsObservationRecord::Quote { symbol, .. } => {
                Some(symbol.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(quote_symbols, vec![Symbol("MSFT.US".into())]);
}

#[tokio::test]
async fn rest_update_refreshes_existing_quote_fields() {
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
        pressure_event_bus: None,
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

#[test]
fn merge_rest_quote_preserves_extended_session_quotes_when_missing() {
    let existing = SecurityQuote {
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
        pre_market_quote: Some(PrePostQuote {
            last_done: dec!(181),
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            volume: 10,
            turnover: dec!(1810),
            high: dec!(181),
            low: dec!(180),
            prev_close: dec!(175),
        }),
        post_market_quote: Some(PrePostQuote {
            last_done: dec!(179),
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            volume: 8,
            turnover: dec!(1432),
            high: dec!(180),
            low: dec!(179),
            prev_close: dec!(175),
        }),
        overnight_quote: None,
    };
    let incoming = SecurityQuote {
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
    };

    let merged = merge_rest_quote(Some(&existing), incoming);
    assert!(merged.pre_market_quote.is_some());
    assert!(merged.post_market_quote.is_some());
}
