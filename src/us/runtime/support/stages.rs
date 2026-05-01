#[cfg(feature = "persistence")]
use super::*;

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
struct UsResolvedSetupContext {
    entry_tick: u64,
    entry_timestamp: time::OffsetDateTime,
    /// Hypothesis identifier — used as ledger bucket now that
    /// `TacticalSetup.family_key` is gone. Pass 2 will replace with a
    /// graph-derived structural key (regime fingerprint / hub topology).
    family: String,
}

#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
fn resolved_setup_context_by_id(
    tick_history: &crate::us::temporal::buffer::UsTickHistory,
) -> std::collections::HashMap<String, UsResolvedSetupContext> {
    let mut contexts = std::collections::HashMap::new();
    for record in tick_history.latest_n(tick_history.len()) {
        for setup in &record.tactical_setups {
            contexts
                .entry(setup.setup_id.clone())
                .or_insert_with(|| UsResolvedSetupContext {
                    entry_tick: record.tick_number,
                    entry_timestamp: record.timestamp,
                    family: setup.hypothesis_id.clone(),
                });
        }
    }
    contexts
}

#[cfg(feature = "persistence")]
pub(crate) async fn maybe_refresh_us_learning_feedback(
    store: &EdenStore,
    object_store: &Arc<ObjectStore>,
    tick: u64,
    refresh_interval: u64,
    cached_feedback: &mut Option<ReasoningLearningFeedback>,
) {
    if cached_feedback.is_some() && tick % refresh_interval != 0 {
        return;
    }

    let Ok(assessments) = store
        .recent_case_reasoning_assessments_by_market("us", 240)
        .await
    else {
        return;
    };
    if assessments.is_empty() {
        return;
    }

    let rows = store
        .recent_ranked_us_lineage_metric_rows(12, 5)
        .await
        .unwrap_or_default();
    let outcome_ctx = derive_outcome_learning_context_from_us_rows(&rows);
    let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
    object_store
        .knowledge
        .write()
        .unwrap()
        .apply_calibration(&feedback);
    *cached_feedback = Some(feedback);
}

#[cfg(feature = "persistence")]
pub(crate) async fn run_us_projection_stage<S: AnalystService>(
    runtime: &mut PreparedRuntimeContext,
    analyst_service: &S,
    tick: u64,
    now: time::OffsetDateTime,
    tick_started_at: std::time::Instant,
    received_push: bool,
    received_update: bool,
    live_push_count: u64,
    cached_feedback: &mut Option<ReasoningLearningFeedback>,
    reasoning: &UsReasoningSnapshot,
    artifact_projection: &crate::core::projection::ProjectionBundle,
    object_store: &Arc<ObjectStore>,
    tick_history: &crate::us::temporal::buffer::UsTickHistory,
    eden_ledger: &mut crate::persistence::case_realized_outcome::EdenLedgerAccumulator,
    intent_belief_field: &mut crate::pipeline::intent_belief::IntentBeliefField,
    outcome_credited_setup_ids: &mut std::collections::HashSet<String>,
    stage_timer: &mut crate::core::runtime::TickStageTimer,
) {
    if let Some(ref store) = runtime.store {
        maybe_refresh_us_learning_feedback(
            store,
            object_store,
            tick,
            US_LEARNING_FEEDBACK_REFRESH_INTERVAL,
            cached_feedback,
        )
        .await;
    }
    stage_timer.mark("S21b1_learning_feedback");

    let live_snapshot = &artifact_projection.live_snapshot;
    let agent_snapshot = &artifact_projection.agent_snapshot;
    let agent_recommendations = &artifact_projection.agent_recommendations;
    let case_list_for_graph =
        build_case_list_with_feedback(live_snapshot, cached_feedback.as_ref());

    // Compute realized outcomes from US tick history (mirrors HK persistence.rs:170-206)
    let topology_outcomes = crate::us::temporal::lineage::compute_us_resolved_topology_outcomes(
        tick_history,
        super::SIGNAL_RESOLUTION_LAG,
    );
    let setup_contexts = resolved_setup_context_by_id(tick_history);
    let realized_outcomes: Vec<crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord> =
        topology_outcomes
            .iter()
            .filter_map(|outcome| {
                let context = setup_contexts.get(&outcome.setup_id)?;
                Some(
                    crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord::from_us_topology_outcome(
                        outcome,
                        context.entry_tick,
                        context.entry_timestamp,
                        now,
                        context.family.as_str(),
                    ),
                )
            })
            .collect();
    eden_ledger.record_batch(&realized_outcomes);

    // Backward pass: outcome → focal IntentBelief (symmetric with HK).
    if !realized_outcomes.is_empty() {
        let summary = crate::pipeline::outcome_feedback::apply_outcome_batch(
            &realized_outcomes,
            intent_belief_field,
            outcome_credited_setup_ids,
        );
        if summary.applied() > 0 {
            eprintln!("{}", summary.summary_line("us"));
        }
    }
    stage_timer.mark("S21b2_outcomes_compute");

    runtime
        .publish_projection_with_followups_from_inputs(
            MarketId::Us,
            crate::cases::CaseMarket::Us,
            artifact_projection,
            Vec::new(),
            analyst_service,
            tick,
            live_push_count,
            tick_started_at,
            received_push,
            received_update,
            &case_list_for_graph.cases,
            now,
            "runtime",
            &agent_snapshot.knowledge_links,
            &agent_recommendations.knowledge_links,
            &agent_snapshot.macro_events,
            &agent_recommendations.decisions,
            &reasoning.hypotheses,
            &reasoning.tactical_setups,
            agent_snapshot.world_state.as_ref(),
            agent_snapshot.backward_reasoning.as_ref(),
            &live_snapshot.active_position_nodes,
            (!realized_outcomes.is_empty()).then_some(realized_outcomes),
        )
        .await;
    stage_timer.mark("S21b3_publish_followups");

    if let Some(ref store) = runtime.store {
        crate::core::runtime::settle_live_horizons_us(
            store,
            tick_history,
            &reasoning.tactical_setups,
            &reasoning.hypotheses,
            now,
        )
        .await;
    }
    stage_timer.mark("S21b4_settle_horizons");

    let symbol_state_records = live_snapshot
        .symbol_states
        .iter()
        .map(|state| {
            crate::persistence::symbol_perception_state::SymbolPerceptionStateRecord::from_state(
                live_snapshot.market,
                now,
                state,
            )
        })
        .collect::<Vec<_>>();
    if !symbol_state_records.is_empty() {
        runtime
            .persist_symbol_perception_states(crate::cases::CaseMarket::Us, symbol_state_records)
            .await;
    }
    stage_timer.mark("S21b5_persist_perception_states");
}

#[cfg(feature = "persistence")]
pub(crate) async fn run_us_persistence_stage(
    runtime: &PreparedRuntimeContext,
    tick: u64,
    now: time::OffsetDateTime,
    live: &UsLiveState,
    rest: &UsRestSnapshot,
    tick_record: &UsTickRecord,
    trades_this_tick: &HashMap<Symbol, Vec<longport::quote::Trade>>,
) {
    runtime.persist_us_tick(tick_record.clone()).await;

    let raw_snapshot = RawSnapshot {
        timestamp: now,
        brokers: HashMap::new(),
        calc_indexes: rest.calc_indexes.clone(),
        candlesticks: live.candlesticks.clone(),
        capital_flows: rest.capital_flows.clone(),
        capital_distributions: HashMap::new(),
        depths: HashMap::new(),
        intraday_lines: rest.intraday_lines.clone(),
        market_temperature: None,
        option_surfaces: rest.option_surfaces.clone(),
        quotes: live.quotes.clone(),
        trades: trades_this_tick.clone(),
    };
    let archive = crate::ontology::microstructure::TickArchive::from_raw_for_market(
        "us",
        tick,
        &raw_snapshot,
    );
    runtime.persist_market_tick_archive(archive).await;
}

#[cfg(feature = "persistence")]
pub(crate) async fn maybe_persist_us_lineage_stage(
    runtime: &PreparedRuntimeContext,
    tick: u64,
    now: time::OffsetDateTime,
    tick_history_len: usize,
    lineage_stats: &UsLineageStats,
) {
    if tick % 30 != 0 || tick_history_len <= 1 {
        return;
    }
    runtime
        .persist_us_lineage_stats(
            tick,
            now,
            tick_history_len,
            SIGNAL_RESOLUTION_LAG,
            lineage_stats,
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::Symbol;
    use crate::ontology::reasoning::{
        default_case_horizon, DecisionLineage, ReasoningScope, TacticalSetup,
    };
    use crate::us::graph::decision::UsMarketRegimeBias;
    use crate::us::temporal::buffer::UsTickHistory;
    use crate::us::temporal::record::UsTickRecord;
    use rust_decimal_macros::dec;

    fn make_setup(setup_id: &str, symbol: &str, family: &str) -> TacticalSetup {
        TacticalSetup {
            setup_id: setup_id.into(),
            hypothesis_id: format!("hyp:{setup_id}"),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                time::OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol(symbol.into())),
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
            risk_notes: vec![format!("family={family}")],
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    fn make_tick(
        tick_number: u64,
        seconds: i64,
        tactical_setups: Vec<TacticalSetup>,
    ) -> UsTickRecord {
        UsTickRecord {
            tick_number,
            timestamp: time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(seconds),
            signals: std::collections::HashMap::new(),
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups,
            market_regime: UsMarketRegimeBias::Neutral,
        }
    }

    #[test]
    fn resolved_setup_context_uses_entry_tick_and_hypothesis_from_history() {
        let mut history = UsTickHistory::new(8);
        history.push(make_tick(
            10,
            10,
            vec![make_setup("setup:1", "AAPL.US", "flow")],
        ));
        history.push(make_tick(11, 11, vec![]));
        history.push(make_tick(
            12,
            12,
            vec![
                make_setup("setup:1", "AAPL.US", "stress"),
                make_setup("setup:2", "MSFT.US", "rotation"),
            ],
        ));

        let contexts = resolved_setup_context_by_id(&history);

        let setup_one = contexts.get("setup:1").unwrap();
        assert_eq!(setup_one.entry_tick, 10);
        assert_eq!(setup_one.entry_timestamp.unix_timestamp(), 10);
        // ledger bucket now keyed by hypothesis_id (not legacy family_key)
        assert_eq!(setup_one.family, "hyp:setup:1");

        let setup_two = contexts.get("setup:2").unwrap();
        assert_eq!(setup_two.entry_tick, 12);
        assert_eq!(setup_two.entry_timestamp.unix_timestamp(), 12);
        assert_eq!(setup_two.family, "hyp:setup:2");
    }
}
