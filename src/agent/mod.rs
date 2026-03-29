use std::collections::{BTreeSet, HashMap, HashSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{OffsetDateTime, UtcOffset};

use crate::cases::CaseMarket;
use crate::core::market::{ArtifactKind, MarketId, MarketRegistry};
use crate::external::world_monitor::{load_world_monitor_events, WorldMonitorEventRecord};
use crate::live_snapshot::{
    LiveCrossMarketSignal, LiveEvent, LiveMarket, LiveMarketRegime, LivePressure, LiveSnapshot,
    LiveStressSnapshot,
};
use crate::ontology::links::{InstitutionActivity, LinkSnapshot, OrderBookObservation};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    Hypothesis, HypothesisTrack, HypothesisTrackStatus, ReasoningScope, TacticalSetup,
};
use crate::ontology::store::ObjectStore;
use crate::ontology::world::{
    BackwardInvestigation, BackwardReasoningSnapshot, WorldStateSnapshot,
};
use crate::ontology::{
    decision_knowledge_node_ref, macro_event_knowledge_node_ref, market_knowledge_node_ref,
    sector_knowledge_node_ref, symbol_knowledge_node_ref, ActionNode, AgentEventImpact,
    AgentKnowledgeLink, AgentKnowledgeNodeRef, AgentMacroEvent, AgentMacroEventCandidate,
    KnowledgeLinkAttributes, KnowledgeRelation,
};
use crate::temporal::buffer::TickHistory;
use crate::temporal::lineage::FamilyContextLineageOutcome;
use crate::temporal::record::SymbolSignals;
use crate::us::pipeline::reasoning::UsReasoningSnapshot;
use crate::us::pipeline::world::UsBackwardSnapshot;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::lineage::UsLineageStats;
use crate::us::temporal::record::UsSymbolSignals;

mod artifacts;
mod alerts;
mod attention;
mod builders;
mod context;
mod io;
mod lens;
mod macro_events;
mod recommendations;
mod shared;
mod tools;
mod views;
pub use artifacts::{
    build_recommendation_journal_record, load_agent_snapshot_path, load_briefing_path,
    load_eod_review_path, load_recommendation_journal_path, load_recommendations_path,
    load_scoreboard_path, load_session_path, load_watchlist_path, update_recommendation_journal,
};
pub use io::{
    load_briefing, load_eod_review, load_recommendations, load_scoreboard, load_session,
    load_snapshot, load_watchlist,
};
use artifacts::{backfill_alert_resolution_from_legacy_outcome, sync_alert_views};
use alerts::{
    alert_outcome_from_resolution, alert_resolution_action, compute_alert_slice_stats,
    compute_alert_stats, resolve_alert_resolution, top_noisy_slices, top_positive_slices,
    top_resolved_alerts,
};
use attention::{
    build_hk_notices, build_sector_flows, build_us_notices, build_wake_state,
    collect_active_structures,
};
use context::{
    best_hk_context_prior, best_us_context_prior, clamp_unit_interval,
    current_hk_context_priors, current_us_context_priors, hk_context_regime,
};
use macro_events::{
    build_decision_knowledge_links, build_macro_event_candidates,
    build_macro_event_knowledge_links, build_world_monitor_macro_event_candidates,
    knowledge_link_matches_filters, merge_macro_event_candidates, promote_macro_events,
};
pub use recommendations::build_recommendations;
pub use tools::{execute_tool, tool_catalog};
use builders::alpha_horizon_label;
pub use builders::{build_hk_agent_snapshot, build_us_agent_snapshot};
pub(crate) use lens::{default_lens_engine, LensBundle, LensContext};
use shared::{
    decimal_mean, decimal_sign, dedupe_strings, extract_symbols, previous_agent_symbol_map,
    push_unique, render_hk_transition_summary, render_track_state, scope_symbol,
    sort_symbol_states, symbol_priority,
};
use recommendations::{
    agent_bias_for_symbol, best_counterfactual_action, counterfactual_regret,
    decision_alert_record, realized_return_for_action, recommendation_resolution_status,
    resolve_market_recommendation_outcome, resolve_recommendation_outcome,
    resolve_sector_recommendation_outcome, sector_reference_value, symbol_mark_price,
    symbol_status,
};
pub use views::{
    build_alert_scoreboard, build_briefing, build_eod_review, build_session, build_watchlist,
};
use views::{
    decision_matches_filters, market_scope_symbol, recommendation_decisions,
    sync_recommendation_views,
};
mod types;
pub use types::*;

#[cfg(test)]
mod tests;
