use std::collections::{BTreeSet, HashMap, HashSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{OffsetDateTime, UtcOffset};

use crate::cases::CaseMarket;
use crate::core::market::{ArtifactKind, MarketId, MarketRegistry};
use crate::external::world_monitor::{load_world_monitor_events, WorldMonitorEventRecord};
use crate::live_snapshot::{
    LiveClusterState, LiveCrossMarketSignal, LiveEvent, LiveMarket, LiveMarketRegime, LivePressure,
    LiveSnapshot, LiveStressSnapshot, LiveWorldSummary,
};
use crate::ontology::links::{InstitutionActivity, LinkSnapshot, OrderBookObservation};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    Hypothesis, HypothesisTrack, HypothesisTrackStatus, InvestigationSelection, ReasoningScope,
    TacticalSetup,
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

mod alerts;
mod artifacts;
pub(crate) mod attention;
pub(crate) mod builders;
pub mod codex;
mod context;
mod investigations;
mod io;
mod judgments;
mod lens;
mod macro_events;
// option_inference deleted — T25 rule-based narrative assembly
mod recommendations;
pub(crate) mod shared;
// symbol_inference deleted — T22 rule-based narrative assembly
pub mod tool_registry;
mod tools;
mod views;
use alerts::{
    alert_outcome_from_resolution, alert_resolution_action, compute_alert_slice_stats,
    compute_alert_stats, resolve_alert_resolution, top_noisy_slices, top_positive_slices,
    top_resolved_alerts,
};
use artifacts::{backfill_alert_resolution_from_legacy_outcome, sync_alert_views};
pub use artifacts::{
    build_recommendation_journal_record, load_agent_snapshot_path, load_briefing_path,
    load_eod_review_path, load_recommendation_journal_path, load_recommendations_path,
    load_scoreboard_path, load_session_path, load_watchlist_path, update_recommendation_journal,
};
use attention::{
    build_hk_notices, build_sector_flows, build_us_notices, build_wake_state,
    collect_active_structures,
};
use builders::alpha_horizon_label;
pub use builders::{build_hk_agent_snapshot, build_us_agent_snapshot};
use context::{
    best_hk_context_prior, best_us_context_prior, clamp_unit_interval, current_hk_context_priors,
    current_us_context_priors,
};
pub use investigations::build_investigations;
pub use io::{
    load_briefing, load_eod_review, load_recommendations, load_scoreboard, load_session,
    load_snapshot, load_watchlist,
};
pub use judgments::build_judgments;
pub(crate) use lens::{default_lens_engine, LensBundle, LensContext};
pub(crate) use macro_events::knowledge_link_matches_filters;
use macro_events::{
    build_decision_knowledge_links, build_macro_event_candidates,
    build_macro_event_knowledge_links, build_world_monitor_macro_event_candidates,
    merge_macro_event_candidates, promote_macro_events,
};
pub use perception::{build_perception_report, AgentPerceptionReport};
pub use recommendations::build_recommendations;
use recommendations::{
    agent_bias_for_symbol, best_counterfactual_action, counterfactual_regret,
    decision_alert_record, realized_return_for_action, recommendation_resolution_status,
    resolve_market_recommendation_outcome, resolve_recommendation_outcome,
    resolve_sector_recommendation_outcome, sector_reference_value, symbol_mark_price,
    symbol_status,
};
use shared::{
    decimal_mean, decimal_sign, dedupe_strings, extract_symbols, previous_agent_symbol_map,
    push_unique, render_hk_transition_summary, render_track_state, scope_symbol,
    sort_symbol_states, symbol_priority,
};
#[cfg(test)]
pub(crate) use tools::compat_query_allowlist;
pub(crate) use tools::sort_suggested_tool_calls;
pub use tools::{execute_tool, tool_catalog};
pub use views::{
    build_alert_scoreboard, build_briefing, build_eod_review, build_session, build_watchlist,
};
use views::{
    decision_matches_filters, market_scope_symbol, recommendation_decisions,
    sync_recommendation_views,
};
pub mod llm;
pub mod perception;
mod types;
pub use types::*;

#[cfg(test)]
mod tests;
