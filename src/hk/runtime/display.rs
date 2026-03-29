use super::*;

#[path = "display/console.rs"]
mod console;
#[path = "display/microstructure.rs"]
mod microstructure;

pub(super) fn display_hk_bootstrap_preview(
    readiness: &ReadinessReport,
    workflow_snapshots: &[ActionWorkflowSnapshot],
    propagation_paths: &[eden::PropagationPath],
) {
    console::display_hk_bootstrap_preview(readiness, workflow_snapshots, propagation_paths);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn display_hk_reasoning_console(
    pct: Decimal,
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    graph_insights: &GraphInsights,
    observation_snapshot: &ObservationSnapshot,
    event_snapshot: &EventSnapshot,
    derived_signal_snapshot: &DerivedSignalSnapshot,
    workflow_snapshots: &[ActionWorkflowSnapshot],
    reasoning_snapshot: &ReasoningSnapshot,
    world_snapshots: &WorldSnapshots,
    decision: &DecisionSnapshot,
    ready_convergence_scores: &HashMap<Symbol, crate::graph::convergence::ConvergenceScore>,
    actionable_order_suggestions: &[crate::graph::decision::OrderSuggestion],
    lineage_stats: &crate::temporal::lineage::LineageStats,
    causal_timelines: &HashMap<String, CausalTimeline>,
) {
    console::display_hk_reasoning_console(
        pct,
        store,
        graph_insights,
        observation_snapshot,
        event_snapshot,
        derived_signal_snapshot,
        workflow_snapshots,
        reasoning_snapshot,
        world_snapshots,
        decision,
        ready_convergence_scores,
        actionable_order_suggestions,
        lineage_stats,
        causal_timelines,
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn display_hk_market_microstructure(
    pct: Decimal,
    tick: u64,
    bootstrap_mode: bool,
    history_len: usize,
    dynamics: &HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
    polymarket_dynamics: &[eden::temporal::analysis::PolymarketDynamics],
    actionable_order_suggestions: &[crate::graph::decision::OrderSuggestion],
    new_set: &HashSet<&Symbol>,
    scorecard: &mut SignalScorecard,
    links: &LinkSnapshot,
    readiness: &ReadinessReport,
    graph_insights: &GraphInsights,
    aged_degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    trade_symbols: Vec<(Symbol, usize, i64)>,
    live: &LiveState,
    tracker: &PositionTracker,
    newly_entered: &[Symbol],
) {
    microstructure::display_hk_market_microstructure(
        pct,
        tick,
        bootstrap_mode,
        history_len,
        dynamics,
        polymarket_dynamics,
        actionable_order_suggestions,
        new_set,
        scorecard,
        links,
        readiness,
        graph_insights,
        aged_degradations,
        trade_symbols,
        live,
        tracker,
        newly_entered,
    );
}

pub(super) fn display_hk_temporal_debug(
    tick: u64,
    decision: &DecisionSnapshot,
    graph_node_delta: &crate::graph::temporal::GraphNodeTemporalDelta,
    broker_delta: &crate::graph::temporal::BrokerTemporalDelta,
) {
    microstructure::display_hk_temporal_debug(tick, decision, graph_node_delta, broker_delta);
}
