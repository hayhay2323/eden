/// SurrealDB schema migration definitions for Eden.
///
/// Keep migrations append-only. New schema changes should add a new migration
/// instead of rewriting older migration steps.
pub const SCHEMA_VERSION_TABLE: &str = r#"
DEFINE TABLE IF NOT EXISTS schema_migration_state SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS version ON schema_migration_state TYPE int;
DEFINE FIELD IF NOT EXISTS name ON schema_migration_state TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON schema_migration_state TYPE string;
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaMigration {
    pub version: u32,
    pub name: &'static str,
    pub statements: &'static str,
}

const MIGRATION_001: &str = r#"
-- Tick records: one per pipeline cycle
DEFINE TABLE tick_record SCHEMAFULL;
DEFINE FIELD tick_number ON tick_record TYPE int;
DEFINE FIELD timestamp ON tick_record TYPE datetime;
DEFINE FIELD signals ON tick_record TYPE object;
DEFINE FIELD observations ON tick_record TYPE array;
DEFINE FIELD events ON tick_record TYPE array;
DEFINE FIELD derived_signals ON tick_record TYPE array;
DEFINE FIELD action_workflows ON tick_record TYPE array;
DEFINE FIELD polymarket_priors ON tick_record TYPE array;
DEFINE FIELD hypotheses ON tick_record TYPE array;
DEFINE FIELD propagation_paths ON tick_record TYPE array;
DEFINE FIELD tactical_setups ON tick_record TYPE array;
DEFINE FIELD hypothesis_tracks ON tick_record TYPE array;
DEFINE FIELD case_clusters ON tick_record TYPE array;
DEFINE FIELD world_state ON tick_record TYPE object;
DEFINE FIELD backward_reasoning ON tick_record TYPE object;
DEFINE INDEX idx_tick_number ON tick_record FIELDS tick_number UNIQUE;
DEFINE INDEX idx_timestamp ON tick_record FIELDS timestamp;

-- US tick records: one per US pipeline cycle
DEFINE TABLE us_tick_record SCHEMAFULL;
DEFINE FIELD tick_number ON us_tick_record TYPE int;
DEFINE FIELD timestamp ON us_tick_record TYPE datetime;
DEFINE FIELD signals ON us_tick_record TYPE object;
DEFINE FIELD cross_market_signals ON us_tick_record TYPE array;
DEFINE FIELD events ON us_tick_record TYPE array;
DEFINE FIELD derived_signals ON us_tick_record TYPE array;
DEFINE FIELD hypotheses ON us_tick_record TYPE array;
DEFINE FIELD tactical_setups ON us_tick_record TYPE array;
DEFINE FIELD market_regime ON us_tick_record TYPE string;
DEFINE INDEX idx_us_tick_number ON us_tick_record FIELDS tick_number UNIQUE;
DEFINE INDEX idx_us_tick_timestamp ON us_tick_record FIELDS timestamp;

-- Institution state: tracks institution behavior over time
DEFINE TABLE institution_state SCHEMAFULL;
DEFINE FIELD institution_id ON institution_state TYPE int;
DEFINE FIELD timestamp ON institution_state TYPE datetime;
DEFINE FIELD symbols ON institution_state TYPE array;
DEFINE FIELD ask_symbols ON institution_state TYPE array;
DEFINE FIELD bid_symbols ON institution_state TYPE array;
DEFINE FIELD seat_count ON institution_state TYPE int;
DEFINE INDEX idx_inst_time ON institution_state FIELDS institution_id, timestamp;

-- Daily summary: aggregated per symbol per day
DEFINE TABLE daily_summary SCHEMAFULL;
DEFINE FIELD symbol ON daily_summary TYPE string;
DEFINE FIELD date ON daily_summary TYPE string;
DEFINE FIELD tick_count ON daily_summary TYPE int;
DEFINE FIELD avg_composite ON daily_summary TYPE string;
DEFINE FIELD max_composite ON daily_summary TYPE string;
DEFINE FIELD min_composite ON daily_summary TYPE string;
DEFINE FIELD avg_inst_alignment ON daily_summary TYPE string;
DEFINE INDEX idx_sym_date ON daily_summary FIELDS symbol, date UNIQUE;

-- Action workflow: type-safe suggested -> confirm -> execute -> monitor -> review flow
DEFINE TABLE action_workflow SCHEMAFULL;
DEFINE FIELD workflow_id ON action_workflow TYPE string;
DEFINE FIELD title ON action_workflow TYPE string;
DEFINE FIELD payload ON action_workflow TYPE object;
DEFINE FIELD current_stage ON action_workflow TYPE string;
DEFINE FIELD recorded_at ON action_workflow TYPE datetime;
DEFINE FIELD actor ON action_workflow TYPE option<string>;
DEFINE FIELD owner ON action_workflow TYPE option<string>;
DEFINE FIELD reviewer ON action_workflow TYPE option<string>;
DEFINE FIELD queue_pin ON action_workflow TYPE option<string>;
DEFINE FIELD note ON action_workflow TYPE option<string>;
DEFINE INDEX idx_action_workflow_id ON action_workflow FIELDS workflow_id UNIQUE;

DEFINE TABLE action_workflow_event SCHEMAFULL;
DEFINE FIELD event_id ON action_workflow_event TYPE string;
DEFINE FIELD workflow_id ON action_workflow_event TYPE string;
DEFINE FIELD title ON action_workflow_event TYPE string;
DEFINE FIELD payload ON action_workflow_event TYPE object;
DEFINE FIELD from_stage ON action_workflow_event TYPE option<string>;
DEFINE FIELD to_stage ON action_workflow_event TYPE string;
DEFINE FIELD recorded_at ON action_workflow_event TYPE datetime;
DEFINE FIELD actor ON action_workflow_event TYPE option<string>;
DEFINE FIELD owner ON action_workflow_event TYPE option<string>;
DEFINE FIELD reviewer ON action_workflow_event TYPE option<string>;
DEFINE FIELD queue_pin ON action_workflow_event TYPE option<string>;
DEFINE FIELD note ON action_workflow_event TYPE option<string>;
DEFINE INDEX idx_action_workflow_event_id ON action_workflow_event FIELDS event_id UNIQUE;
DEFINE INDEX idx_action_workflow_event_time ON action_workflow_event FIELDS workflow_id, recorded_at;

-- Tactical setup: latest known ranked case view
DEFINE TABLE tactical_setup SCHEMAFULL;
DEFINE FIELD setup_id ON tactical_setup TYPE string;
DEFINE FIELD hypothesis_id ON tactical_setup TYPE string;
DEFINE FIELD runner_up_hypothesis_id ON tactical_setup TYPE option<string>;
DEFINE FIELD scope_key ON tactical_setup TYPE string;
DEFINE FIELD title ON tactical_setup TYPE string;
DEFINE FIELD action ON tactical_setup TYPE string;
DEFINE FIELD family_key ON tactical_setup TYPE option<string>;
DEFINE FIELD time_horizon ON tactical_setup TYPE string;
DEFINE FIELD confidence ON tactical_setup TYPE string;
DEFINE FIELD confidence_gap ON tactical_setup TYPE string;
DEFINE FIELD heuristic_edge ON tactical_setup TYPE string;
DEFINE FIELD workflow_id ON tactical_setup TYPE option<string>;
DEFINE FIELD entry_rationale ON tactical_setup TYPE string;
DEFINE FIELD risk_notes ON tactical_setup TYPE array;
DEFINE FIELD based_on ON tactical_setup TYPE array;
DEFINE FIELD blocked_by ON tactical_setup TYPE array;
DEFINE FIELD promoted_by ON tactical_setup TYPE array;
DEFINE FIELD falsified_by ON tactical_setup TYPE array;
DEFINE FIELD recorded_at ON tactical_setup TYPE datetime;
DEFINE INDEX idx_tactical_setup_id ON tactical_setup FIELDS setup_id UNIQUE;
DEFINE INDEX idx_tactical_setup_rank ON tactical_setup FIELDS action, recorded_at;

-- Reasoning assessment: durable semantics profile per case observation or workflow update
DEFINE TABLE case_reasoning_assessment SCHEMAFULL;
DEFINE FIELD assessment_id ON case_reasoning_assessment TYPE string;
DEFINE FIELD setup_id ON case_reasoning_assessment TYPE string;
DEFINE FIELD workflow_id ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD market ON case_reasoning_assessment TYPE string;
DEFINE FIELD symbol ON case_reasoning_assessment TYPE string;
DEFINE FIELD title ON case_reasoning_assessment TYPE string;
DEFINE FIELD family_label ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD sector ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD recommended_action ON case_reasoning_assessment TYPE string;
DEFINE FIELD workflow_state ON case_reasoning_assessment TYPE string;
DEFINE FIELD market_regime_bias ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD market_regime_confidence ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD market_breadth_delta ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD market_average_return ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD market_directional_consensus ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD source ON case_reasoning_assessment TYPE string;
DEFINE FIELD recorded_at ON case_reasoning_assessment TYPE datetime;
DEFINE FIELD owner ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD reviewer ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD actor ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD note ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD law_kinds ON case_reasoning_assessment TYPE array;
DEFINE FIELD predicate_kinds ON case_reasoning_assessment TYPE array;
DEFINE FIELD composite_state_kinds ON case_reasoning_assessment TYPE array;
DEFINE FIELD primary_mechanism_kind ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD primary_mechanism_score ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD competing_mechanism_kinds ON case_reasoning_assessment TYPE array;
DEFINE FIELD invalidation_rules ON case_reasoning_assessment TYPE array;
DEFINE FIELD reasoning_profile ON case_reasoning_assessment TYPE object;
DEFINE INDEX idx_case_reasoning_assessment_id ON case_reasoning_assessment FIELDS assessment_id UNIQUE;
DEFINE INDEX idx_case_reasoning_assessment_time ON case_reasoning_assessment FIELDS setup_id, recorded_at;
DEFINE INDEX idx_case_reasoning_assessment_mechanism ON case_reasoning_assessment FIELDS primary_mechanism_kind, recorded_at;

-- Case realized outcome: latest resolved per-case outcome snapshot
DEFINE TABLE case_realized_outcome SCHEMAFULL;
DEFINE FIELD setup_id ON case_realized_outcome TYPE string;
DEFINE FIELD workflow_id ON case_realized_outcome TYPE option<string>;
DEFINE FIELD market ON case_realized_outcome TYPE string;
DEFINE FIELD symbol ON case_realized_outcome TYPE option<string>;
DEFINE FIELD family ON case_realized_outcome TYPE string;
DEFINE FIELD session ON case_realized_outcome TYPE string;
DEFINE FIELD market_regime ON case_realized_outcome TYPE string;
DEFINE FIELD entry_tick ON case_realized_outcome TYPE int;
DEFINE FIELD entry_timestamp ON case_realized_outcome TYPE datetime;
DEFINE FIELD resolved_tick ON case_realized_outcome TYPE int;
DEFINE FIELD resolved_at ON case_realized_outcome TYPE datetime;
DEFINE FIELD direction ON case_realized_outcome TYPE int;
DEFINE FIELD return_pct ON case_realized_outcome TYPE string;
DEFINE FIELD net_return ON case_realized_outcome TYPE string;
DEFINE FIELD max_favorable_excursion ON case_realized_outcome TYPE string;
DEFINE FIELD max_adverse_excursion ON case_realized_outcome TYPE string;
DEFINE FIELD followed_through ON case_realized_outcome TYPE bool;
DEFINE FIELD invalidated ON case_realized_outcome TYPE bool;
DEFINE FIELD structure_retained ON case_realized_outcome TYPE bool;
DEFINE FIELD convergence_score ON case_realized_outcome TYPE string;
DEFINE INDEX idx_case_realized_outcome_id ON case_realized_outcome FIELDS setup_id UNIQUE;
DEFINE INDEX idx_case_realized_outcome_market ON case_realized_outcome FIELDS market, resolved_at;

-- Hypothesis track: latest cross-tick view of a tactical case
DEFINE TABLE hypothesis_track SCHEMAFULL;
DEFINE FIELD track_id ON hypothesis_track TYPE string;
DEFINE FIELD setup_id ON hypothesis_track TYPE string;
DEFINE FIELD hypothesis_id ON hypothesis_track TYPE string;
DEFINE FIELD runner_up_hypothesis_id ON hypothesis_track TYPE option<string>;
DEFINE FIELD scope_key ON hypothesis_track TYPE string;
DEFINE FIELD title ON hypothesis_track TYPE string;
DEFINE FIELD action ON hypothesis_track TYPE string;
DEFINE FIELD status ON hypothesis_track TYPE string;
DEFINE FIELD age_ticks ON hypothesis_track TYPE int;
DEFINE FIELD status_streak ON hypothesis_track TYPE int;
DEFINE FIELD confidence ON hypothesis_track TYPE string;
DEFINE FIELD previous_confidence ON hypothesis_track TYPE option<string>;
DEFINE FIELD confidence_change ON hypothesis_track TYPE string;
DEFINE FIELD confidence_gap ON hypothesis_track TYPE string;
DEFINE FIELD previous_confidence_gap ON hypothesis_track TYPE option<string>;
DEFINE FIELD confidence_gap_change ON hypothesis_track TYPE string;
DEFINE FIELD heuristic_edge ON hypothesis_track TYPE string;
DEFINE FIELD policy_reason ON hypothesis_track TYPE string;
DEFINE FIELD transition_reason ON hypothesis_track TYPE option<string>;
DEFINE FIELD first_seen_at ON hypothesis_track TYPE datetime;
DEFINE FIELD last_updated_at ON hypothesis_track TYPE datetime;
DEFINE FIELD invalidated_at ON hypothesis_track TYPE option<datetime>;
DEFINE INDEX idx_hypothesis_track_id ON hypothesis_track FIELDS track_id UNIQUE;
DEFINE INDEX idx_hypothesis_track_status ON hypothesis_track FIELDS status, last_updated_at;

-- Lineage evaluation snapshot: persisted leaderboard view for historical review
DEFINE TABLE lineage_snapshot SCHEMAFULL;
DEFINE FIELD snapshot_id ON lineage_snapshot TYPE string;
DEFINE FIELD tick_number ON lineage_snapshot TYPE int;
DEFINE FIELD recorded_at ON lineage_snapshot TYPE datetime;
DEFINE FIELD window_size ON lineage_snapshot TYPE int;
DEFINE FIELD stats ON lineage_snapshot TYPE object;
DEFINE INDEX idx_lineage_snapshot_id ON lineage_snapshot FIELDS snapshot_id UNIQUE;
DEFINE INDEX idx_lineage_snapshot_time ON lineage_snapshot FIELDS recorded_at, tick_number;

-- Flattened lineage metric rows for faster filtered queries and dashboards
DEFINE TABLE lineage_metric_row SCHEMAFULL;
DEFINE FIELD row_id ON lineage_metric_row TYPE string;
DEFINE FIELD snapshot_id ON lineage_metric_row TYPE string;
DEFINE FIELD tick_number ON lineage_metric_row TYPE int;
DEFINE FIELD recorded_at ON lineage_metric_row TYPE datetime;
DEFINE FIELD window_size ON lineage_metric_row TYPE int;
DEFINE FIELD bucket ON lineage_metric_row TYPE string;
DEFINE FIELD rank ON lineage_metric_row TYPE int;
DEFINE FIELD label ON lineage_metric_row TYPE string;
DEFINE FIELD family ON lineage_metric_row TYPE option<string>;
DEFINE FIELD session ON lineage_metric_row TYPE option<string>;
DEFINE FIELD market_regime ON lineage_metric_row TYPE option<string>;
DEFINE FIELD total ON lineage_metric_row TYPE int;
DEFINE FIELD resolved ON lineage_metric_row TYPE int;
DEFINE FIELD hits ON lineage_metric_row TYPE int;
DEFINE FIELD hit_rate ON lineage_metric_row TYPE string;
DEFINE FIELD mean_return ON lineage_metric_row TYPE string;
DEFINE FIELD mean_net_return ON lineage_metric_row TYPE string;
DEFINE FIELD follow_expectancy ON lineage_metric_row TYPE string;
DEFINE FIELD fade_expectancy ON lineage_metric_row TYPE string;
DEFINE FIELD wait_expectancy ON lineage_metric_row TYPE string;
DEFINE FIELD mean_mfe ON lineage_metric_row TYPE string;
DEFINE FIELD mean_mae ON lineage_metric_row TYPE string;
DEFINE FIELD follow_through_rate ON lineage_metric_row TYPE string;
DEFINE FIELD invalidation_rate ON lineage_metric_row TYPE string;
DEFINE FIELD structure_retention_rate ON lineage_metric_row TYPE string;
DEFINE FIELD mean_convergence_score ON lineage_metric_row TYPE string;
DEFINE FIELD mean_external_delta ON lineage_metric_row TYPE string;
DEFINE FIELD external_follow_through_rate ON lineage_metric_row TYPE string;
DEFINE INDEX idx_lineage_metric_row_id ON lineage_metric_row FIELDS row_id UNIQUE;
DEFINE INDEX idx_lineage_metric_row_lookup ON lineage_metric_row FIELDS bucket, label, family, session, market_regime, tick_number;

-- US lineage snapshots
DEFINE TABLE us_lineage_snapshot SCHEMAFULL;
DEFINE FIELD snapshot_id ON us_lineage_snapshot TYPE string;
DEFINE FIELD tick_number ON us_lineage_snapshot TYPE int;
DEFINE FIELD recorded_at ON us_lineage_snapshot TYPE datetime;
DEFINE FIELD window_size ON us_lineage_snapshot TYPE int;
DEFINE FIELD resolution_lag ON us_lineage_snapshot TYPE int;
DEFINE FIELD stats ON us_lineage_snapshot TYPE object;
DEFINE INDEX idx_us_lineage_snapshot_id ON us_lineage_snapshot FIELDS snapshot_id UNIQUE;
DEFINE INDEX idx_us_lineage_snapshot_time ON us_lineage_snapshot FIELDS recorded_at, tick_number;

-- Flattened US lineage rows
DEFINE TABLE us_lineage_metric_row SCHEMAFULL;
DEFINE FIELD row_id ON us_lineage_metric_row TYPE string;
DEFINE FIELD snapshot_id ON us_lineage_metric_row TYPE string;
DEFINE FIELD tick_number ON us_lineage_metric_row TYPE int;
DEFINE FIELD recorded_at ON us_lineage_metric_row TYPE datetime;
DEFINE FIELD window_size ON us_lineage_metric_row TYPE int;
DEFINE FIELD resolution_lag ON us_lineage_metric_row TYPE int;
DEFINE FIELD bucket ON us_lineage_metric_row TYPE string;
DEFINE FIELD rank ON us_lineage_metric_row TYPE int;
DEFINE FIELD template ON us_lineage_metric_row TYPE string;
DEFINE FIELD session ON us_lineage_metric_row TYPE option<string>;
DEFINE FIELD market_regime ON us_lineage_metric_row TYPE option<string>;
DEFINE FIELD total ON us_lineage_metric_row TYPE int;
DEFINE FIELD resolved ON us_lineage_metric_row TYPE int;
DEFINE FIELD hits ON us_lineage_metric_row TYPE int;
DEFINE FIELD hit_rate ON us_lineage_metric_row TYPE string;
DEFINE FIELD mean_return ON us_lineage_metric_row TYPE string;
DEFINE FIELD follow_expectancy ON us_lineage_metric_row TYPE string;
DEFINE FIELD fade_expectancy ON us_lineage_metric_row TYPE string;
DEFINE FIELD wait_expectancy ON us_lineage_metric_row TYPE string;
DEFINE INDEX idx_us_lineage_metric_row_id ON us_lineage_metric_row FIELDS row_id UNIQUE;
DEFINE INDEX idx_us_lineage_metric_row_lookup ON us_lineage_metric_row FIELDS bucket, template, session, market_regime, tick_number;
"#;

const MIGRATION_002: &str = r#"
DEFINE FIELD graph_edge_transitions ON tick_record TYPE array;
"#;

const MIGRATION_003: &str = r#"
DEFINE FIELD OVERWRITE timestamp ON tick_record TYPE string;
DEFINE FIELD OVERWRITE timestamp ON us_tick_record TYPE string;
DEFINE FIELD OVERWRITE timestamp ON institution_state TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON action_workflow TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON action_workflow_event TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON tactical_setup TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON case_reasoning_assessment TYPE string;
DEFINE FIELD OVERWRITE entry_timestamp ON case_realized_outcome TYPE string;
DEFINE FIELD OVERWRITE resolved_at ON case_realized_outcome TYPE string;
DEFINE FIELD OVERWRITE first_seen_at ON hypothesis_track TYPE string;
DEFINE FIELD OVERWRITE last_updated_at ON hypothesis_track TYPE string;
DEFINE FIELD OVERWRITE invalidated_at ON hypothesis_track TYPE option<string>;
DEFINE FIELD OVERWRITE recorded_at ON lineage_snapshot TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON lineage_metric_row TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON us_lineage_snapshot TYPE string;
DEFINE FIELD OVERWRITE recorded_at ON us_lineage_metric_row TYPE string;
"#;

const MIGRATION_004: &str = r#"
DEFINE TABLE macro_event_history SCHEMAFULL;
DEFINE FIELD record_id ON macro_event_history TYPE string;
DEFINE FIELD event_id ON macro_event_history TYPE string;
DEFINE FIELD tick_number ON macro_event_history TYPE int;
DEFINE FIELD market ON macro_event_history TYPE string;
DEFINE FIELD recorded_at ON macro_event_history TYPE string;
DEFINE FIELD event_type ON macro_event_history TYPE string;
DEFINE FIELD authority_level ON macro_event_history TYPE string;
DEFINE FIELD headline ON macro_event_history TYPE string;
DEFINE FIELD summary ON macro_event_history TYPE string;
DEFINE FIELD confidence ON macro_event_history TYPE string;
DEFINE FIELD confirmation_state ON macro_event_history TYPE string;
DEFINE FIELD primary_scope ON macro_event_history TYPE string;
DEFINE FIELD affected_markets ON macro_event_history TYPE array;
DEFINE FIELD affected_sectors ON macro_event_history TYPE array;
DEFINE FIELD affected_symbols ON macro_event_history TYPE array;
DEFINE FIELD preferred_expression ON macro_event_history TYPE string;
DEFINE FIELD requires_market_confirmation ON macro_event_history TYPE bool;
DEFINE FIELD decisive_factors ON macro_event_history TYPE array;
DEFINE FIELD supporting_notice_ids ON macro_event_history TYPE array;
DEFINE FIELD promotion_reasons ON macro_event_history TYPE array;
DEFINE INDEX idx_macro_event_history_id ON macro_event_history FIELDS record_id UNIQUE;
DEFINE INDEX idx_macro_event_history_market_tick ON macro_event_history FIELDS market, tick_number;
DEFINE INDEX idx_macro_event_history_event ON macro_event_history FIELDS event_id, tick_number;

DEFINE TABLE knowledge_link_history SCHEMAFULL;
DEFINE FIELD record_id ON knowledge_link_history TYPE string;
DEFINE FIELD link_id ON knowledge_link_history TYPE string;
DEFINE FIELD tick_number ON knowledge_link_history TYPE int;
DEFINE FIELD market ON knowledge_link_history TYPE string;
DEFINE FIELD recorded_at ON knowledge_link_history TYPE string;
DEFINE FIELD relation ON knowledge_link_history TYPE string;
DEFINE FIELD source_node_kind ON knowledge_link_history TYPE string;
DEFINE FIELD source_node_id ON knowledge_link_history TYPE string;
DEFINE FIELD source_label ON knowledge_link_history TYPE string;
DEFINE FIELD target_node_kind ON knowledge_link_history TYPE string;
DEFINE FIELD target_node_id ON knowledge_link_history TYPE string;
DEFINE FIELD target_label ON knowledge_link_history TYPE string;
DEFINE FIELD confidence ON knowledge_link_history TYPE string;
DEFINE FIELD rationale ON knowledge_link_history TYPE option<string>;
DEFINE INDEX idx_knowledge_link_history_id ON knowledge_link_history FIELDS record_id UNIQUE;
DEFINE INDEX idx_knowledge_link_history_market_tick ON knowledge_link_history FIELDS market, tick_number;
DEFINE INDEX idx_knowledge_link_history_source ON knowledge_link_history FIELDS source_node_id, tick_number;
DEFINE INDEX idx_knowledge_link_history_target ON knowledge_link_history FIELDS target_node_id, tick_number;
"#;

const MIGRATION_005: &str = r#"
DEFINE TABLE macro_event_state SCHEMAFULL;
DEFINE FIELD state_id ON macro_event_state TYPE string;
DEFINE FIELD event_id ON macro_event_state TYPE string;
DEFINE FIELD market ON macro_event_state TYPE string;
DEFINE FIELD latest_tick_number ON macro_event_state TYPE int;
DEFINE FIELD last_seen_at ON macro_event_state TYPE string;
DEFINE FIELD event_type ON macro_event_state TYPE string;
DEFINE FIELD authority_level ON macro_event_state TYPE string;
DEFINE FIELD headline ON macro_event_state TYPE string;
DEFINE FIELD summary ON macro_event_state TYPE string;
DEFINE FIELD confidence ON macro_event_state TYPE string;
DEFINE FIELD confirmation_state ON macro_event_state TYPE string;
DEFINE FIELD primary_scope ON macro_event_state TYPE string;
DEFINE FIELD affected_markets ON macro_event_state TYPE array;
DEFINE FIELD affected_sectors ON macro_event_state TYPE array;
DEFINE FIELD affected_symbols ON macro_event_state TYPE array;
DEFINE FIELD preferred_expression ON macro_event_state TYPE string;
DEFINE FIELD requires_market_confirmation ON macro_event_state TYPE bool;
DEFINE FIELD decisive_factors ON macro_event_state TYPE array;
DEFINE FIELD supporting_notice_ids ON macro_event_state TYPE array;
DEFINE FIELD promotion_reasons ON macro_event_state TYPE array;
DEFINE INDEX idx_macro_event_state_id ON macro_event_state FIELDS state_id UNIQUE;
DEFINE INDEX idx_macro_event_state_market_tick ON macro_event_state FIELDS market, latest_tick_number;

DEFINE TABLE knowledge_link_state SCHEMAFULL;
DEFINE FIELD state_id ON knowledge_link_state TYPE string;
DEFINE FIELD link_id ON knowledge_link_state TYPE string;
DEFINE FIELD market ON knowledge_link_state TYPE string;
DEFINE FIELD latest_tick_number ON knowledge_link_state TYPE int;
DEFINE FIELD last_seen_at ON knowledge_link_state TYPE string;
DEFINE FIELD relation ON knowledge_link_state TYPE string;
DEFINE FIELD source_node_kind ON knowledge_link_state TYPE string;
DEFINE FIELD source_node_id ON knowledge_link_state TYPE string;
DEFINE FIELD source_label ON knowledge_link_state TYPE string;
DEFINE FIELD target_node_kind ON knowledge_link_state TYPE string;
DEFINE FIELD target_node_id ON knowledge_link_state TYPE string;
DEFINE FIELD target_label ON knowledge_link_state TYPE string;
DEFINE FIELD confidence ON knowledge_link_state TYPE string;
DEFINE FIELD rationale ON knowledge_link_state TYPE option<string>;
DEFINE INDEX idx_knowledge_link_state_id ON knowledge_link_state FIELDS state_id UNIQUE;
DEFINE INDEX idx_knowledge_link_state_market_tick ON knowledge_link_state FIELDS market, latest_tick_number;
DEFINE INDEX idx_knowledge_link_state_source ON knowledge_link_state FIELDS source_node_id, latest_tick_number;
DEFINE INDEX idx_knowledge_link_state_target ON knowledge_link_state FIELDS target_node_id, latest_tick_number;
"#;

const MIGRATION_006: &str = r#"
DEFINE TABLE knowledge_node_history SCHEMAFULL;
DEFINE FIELD record_id ON knowledge_node_history TYPE string;
DEFINE FIELD node_id ON knowledge_node_history TYPE string;
DEFINE FIELD node_kind ON knowledge_node_history TYPE string;
DEFINE FIELD label ON knowledge_node_history TYPE string;
DEFINE FIELD market ON knowledge_node_history TYPE string;
DEFINE FIELD tick_number ON knowledge_node_history TYPE int;
DEFINE FIELD recorded_at ON knowledge_node_history TYPE string;
DEFINE FIELD attributes ON knowledge_node_history TYPE object;
DEFINE INDEX idx_knowledge_node_history_id ON knowledge_node_history FIELDS record_id UNIQUE;
DEFINE INDEX idx_knowledge_node_history_node ON knowledge_node_history FIELDS node_id, tick_number;
DEFINE INDEX idx_knowledge_node_history_market_tick ON knowledge_node_history FIELDS market, tick_number;

DEFINE TABLE knowledge_node_state SCHEMAFULL;
DEFINE FIELD state_id ON knowledge_node_state TYPE string;
DEFINE FIELD node_id ON knowledge_node_state TYPE string;
DEFINE FIELD node_kind ON knowledge_node_state TYPE string;
DEFINE FIELD label ON knowledge_node_state TYPE string;
DEFINE FIELD market ON knowledge_node_state TYPE string;
DEFINE FIELD latest_tick_number ON knowledge_node_state TYPE int;
DEFINE FIELD last_seen_at ON knowledge_node_state TYPE string;
DEFINE FIELD attributes ON knowledge_node_state TYPE object;
DEFINE INDEX idx_knowledge_node_state_id ON knowledge_node_state FIELDS state_id UNIQUE;
DEFINE INDEX idx_knowledge_node_state_node ON knowledge_node_state FIELDS node_id UNIQUE;
DEFINE INDEX idx_knowledge_node_state_market_tick ON knowledge_node_state FIELDS market, latest_tick_number;
"#;

const MIGRATION_007: &str = r#"
DEFINE FIELD attributes ON knowledge_link_history TYPE object;
DEFINE FIELD attributes ON knowledge_link_state TYPE object;
"#;

/// Backfill any pre-migration-003 records that still have native datetime values
/// in fields that were changed to string. SurrealDB's `string::concat` on a
/// datetime value produces its RFC-3339 representation, so we cast via
/// `<string>field` which is a no-op on values already stored as strings.
const MIGRATION_008: &str = r#"
UPDATE tick_record SET timestamp = <string>timestamp WHERE type::is::datetime(timestamp);
UPDATE us_tick_record SET timestamp = <string>timestamp WHERE type::is::datetime(timestamp);
UPDATE institution_state SET timestamp = <string>timestamp WHERE type::is::datetime(timestamp);
UPDATE action_workflow SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE action_workflow_event SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE tactical_setup SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE case_reasoning_assessment SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE case_realized_outcome SET entry_timestamp = <string>entry_timestamp WHERE type::is::datetime(entry_timestamp);
UPDATE case_realized_outcome SET resolved_at = <string>resolved_at WHERE type::is::datetime(resolved_at);
UPDATE hypothesis_track SET first_seen_at = <string>first_seen_at WHERE type::is::datetime(first_seen_at);
UPDATE hypothesis_track SET last_updated_at = <string>last_updated_at WHERE type::is::datetime(last_updated_at);
UPDATE hypothesis_track SET invalidated_at = <string>invalidated_at WHERE type::is::datetime(invalidated_at);
UPDATE lineage_snapshot SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE lineage_metric_row SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE us_lineage_snapshot SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
UPDATE us_lineage_metric_row SET recorded_at = <string>recorded_at WHERE type::is::datetime(recorded_at);
"#;

const MIGRATION_009: &str = r#"
-- Tick archive: full-fidelity market data snapshot per tick
DEFINE TABLE tick_archive SCHEMALESS;
DEFINE INDEX idx_tick_archive_tick ON tick_archive FIELDS tick_number UNIQUE;
"#;

const MIGRATION_010: &str = r#"
DEFINE TABLE knowledge_event_history SCHEMAFULL;
DEFINE FIELD record_id ON knowledge_event_history TYPE string;
DEFINE FIELD event_id ON knowledge_event_history TYPE string;
DEFINE FIELD tick_number ON knowledge_event_history TYPE int;
DEFINE FIELD market ON knowledge_event_history TYPE string;
DEFINE FIELD recorded_at ON knowledge_event_history TYPE string;
DEFINE FIELD kind ON knowledge_event_history TYPE string;
DEFINE FIELD subject_node_kind ON knowledge_event_history TYPE string;
DEFINE FIELD subject_node_id ON knowledge_event_history TYPE string;
DEFINE FIELD subject_label ON knowledge_event_history TYPE string;
DEFINE FIELD object_node_kind ON knowledge_event_history TYPE option<string>;
DEFINE FIELD object_node_id ON knowledge_event_history TYPE option<string>;
DEFINE FIELD object_label ON knowledge_event_history TYPE option<string>;
DEFINE FIELD confidence ON knowledge_event_history TYPE string;
DEFINE FIELD evidence ON knowledge_event_history TYPE array;
DEFINE FIELD attributes ON knowledge_event_history TYPE object;
DEFINE FIELD rationale ON knowledge_event_history TYPE option<string>;
DEFINE INDEX idx_knowledge_event_history_id ON knowledge_event_history FIELDS record_id UNIQUE;
DEFINE INDEX idx_knowledge_event_history_subject ON knowledge_event_history FIELDS subject_node_id, tick_number;
DEFINE INDEX idx_knowledge_event_history_object ON knowledge_event_history FIELDS object_node_id, tick_number;
DEFINE INDEX idx_knowledge_event_history_kind ON knowledge_event_history FIELDS kind, tick_number;

DEFINE TABLE knowledge_event_state SCHEMAFULL;
DEFINE FIELD state_id ON knowledge_event_state TYPE string;
DEFINE FIELD event_id ON knowledge_event_state TYPE string;
DEFINE FIELD market ON knowledge_event_state TYPE string;
DEFINE FIELD latest_tick_number ON knowledge_event_state TYPE int;
DEFINE FIELD last_seen_at ON knowledge_event_state TYPE string;
DEFINE FIELD kind ON knowledge_event_state TYPE string;
DEFINE FIELD subject_node_kind ON knowledge_event_state TYPE string;
DEFINE FIELD subject_node_id ON knowledge_event_state TYPE string;
DEFINE FIELD subject_label ON knowledge_event_state TYPE string;
DEFINE FIELD object_node_kind ON knowledge_event_state TYPE option<string>;
DEFINE FIELD object_node_id ON knowledge_event_state TYPE option<string>;
DEFINE FIELD object_label ON knowledge_event_state TYPE option<string>;
DEFINE FIELD confidence ON knowledge_event_state TYPE string;
DEFINE FIELD evidence ON knowledge_event_state TYPE array;
DEFINE FIELD attributes ON knowledge_event_state TYPE object;
DEFINE FIELD rationale ON knowledge_event_state TYPE option<string>;
DEFINE INDEX idx_knowledge_event_state_id ON knowledge_event_state FIELDS state_id UNIQUE;
DEFINE INDEX idx_knowledge_event_state_subject ON knowledge_event_state FIELDS subject_node_id, latest_tick_number;
DEFINE INDEX idx_knowledge_event_state_object ON knowledge_event_state FIELDS object_node_id, latest_tick_number;
DEFINE INDEX idx_knowledge_event_state_kind ON knowledge_event_state FIELDS kind, latest_tick_number;
"#;

const MIGRATION_011: &str = r#"
DEFINE FIELD OVERWRITE confidence ON macro_event_history TYPE decimal;
DEFINE FIELD OVERWRITE confidence ON knowledge_link_history TYPE decimal;
DEFINE FIELD OVERWRITE confidence ON macro_event_state TYPE decimal;
DEFINE FIELD OVERWRITE confidence ON knowledge_link_state TYPE decimal;
DEFINE FIELD OVERWRITE confidence ON knowledge_event_history TYPE decimal;
DEFINE FIELD OVERWRITE confidence ON knowledge_event_state TYPE decimal;

UPDATE macro_event_history SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE knowledge_link_history SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE macro_event_state SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE knowledge_link_state SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE knowledge_event_history SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE knowledge_event_state SET confidence = <decimal>confidence WHERE type::is::string(confidence);
"#;

const MIGRATION_012: &str = r#"
DEFINE FIELD OVERWRITE confidence ON tactical_setup TYPE decimal;
DEFINE FIELD OVERWRITE confidence_gap ON tactical_setup TYPE decimal;
DEFINE FIELD OVERWRITE heuristic_edge ON tactical_setup TYPE decimal;
DEFINE FIELD OVERWRITE convergence_score ON tactical_setup TYPE option<decimal>;

DEFINE FIELD OVERWRITE confidence ON hypothesis_track TYPE decimal;
DEFINE FIELD OVERWRITE previous_confidence ON hypothesis_track TYPE option<decimal>;
DEFINE FIELD OVERWRITE confidence_change ON hypothesis_track TYPE decimal;
DEFINE FIELD OVERWRITE confidence_gap ON hypothesis_track TYPE decimal;
DEFINE FIELD OVERWRITE previous_confidence_gap ON hypothesis_track TYPE option<decimal>;
DEFINE FIELD OVERWRITE confidence_gap_change ON hypothesis_track TYPE decimal;
DEFINE FIELD OVERWRITE heuristic_edge ON hypothesis_track TYPE decimal;

DEFINE FIELD OVERWRITE market_regime_confidence ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD OVERWRITE market_breadth_delta ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD OVERWRITE market_average_return ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD OVERWRITE market_directional_consensus ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD OVERWRITE primary_mechanism_score ON case_reasoning_assessment TYPE option<decimal>;

UPDATE tactical_setup SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE tactical_setup SET confidence_gap = <decimal>confidence_gap WHERE type::is::string(confidence_gap);
UPDATE tactical_setup SET heuristic_edge = <decimal>heuristic_edge WHERE type::is::string(heuristic_edge);
UPDATE tactical_setup SET convergence_score = <decimal>convergence_score WHERE type::is::string(convergence_score);

UPDATE hypothesis_track SET confidence = <decimal>confidence WHERE type::is::string(confidence);
UPDATE hypothesis_track SET previous_confidence = <decimal>previous_confidence WHERE type::is::string(previous_confidence);
UPDATE hypothesis_track SET confidence_change = <decimal>confidence_change WHERE type::is::string(confidence_change);
UPDATE hypothesis_track SET confidence_gap = <decimal>confidence_gap WHERE type::is::string(confidence_gap);
UPDATE hypothesis_track SET previous_confidence_gap = <decimal>previous_confidence_gap WHERE type::is::string(previous_confidence_gap);
UPDATE hypothesis_track SET confidence_gap_change = <decimal>confidence_gap_change WHERE type::is::string(confidence_gap_change);
UPDATE hypothesis_track SET heuristic_edge = <decimal>heuristic_edge WHERE type::is::string(heuristic_edge);

UPDATE case_reasoning_assessment SET market_regime_confidence = <decimal>market_regime_confidence WHERE type::is::string(market_regime_confidence);
UPDATE case_reasoning_assessment SET market_breadth_delta = <decimal>market_breadth_delta WHERE type::is::string(market_breadth_delta);
UPDATE case_reasoning_assessment SET market_average_return = <decimal>market_average_return WHERE type::is::string(market_average_return);
UPDATE case_reasoning_assessment SET market_directional_consensus = <decimal>market_directional_consensus WHERE type::is::string(market_directional_consensus);
UPDATE case_reasoning_assessment SET primary_mechanism_score = <decimal>primary_mechanism_score WHERE type::is::string(primary_mechanism_score);
"#;

const MIGRATION_013: &str = r#"
DEFINE FIELD OVERWRITE return_pct ON case_realized_outcome TYPE decimal;
DEFINE FIELD OVERWRITE net_return ON case_realized_outcome TYPE decimal;
DEFINE FIELD OVERWRITE max_favorable_excursion ON case_realized_outcome TYPE decimal;
DEFINE FIELD OVERWRITE max_adverse_excursion ON case_realized_outcome TYPE decimal;
DEFINE FIELD OVERWRITE convergence_score ON case_realized_outcome TYPE decimal;

DEFINE FIELD OVERWRITE hit_rate ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_return ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_net_return ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE follow_expectancy ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE fade_expectancy ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE wait_expectancy ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_mfe ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_mae ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE follow_through_rate ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE invalidation_rate ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE structure_retention_rate ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_convergence_score ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_external_delta ON lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE external_follow_through_rate ON lineage_metric_row TYPE decimal;

DEFINE FIELD OVERWRITE hit_rate ON us_lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE mean_return ON us_lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE follow_expectancy ON us_lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE fade_expectancy ON us_lineage_metric_row TYPE decimal;
DEFINE FIELD OVERWRITE wait_expectancy ON us_lineage_metric_row TYPE decimal;

UPDATE case_realized_outcome SET return_pct = <decimal>return_pct WHERE type::is::string(return_pct);
UPDATE case_realized_outcome SET net_return = <decimal>net_return WHERE type::is::string(net_return);
UPDATE case_realized_outcome SET max_favorable_excursion = <decimal>max_favorable_excursion WHERE type::is::string(max_favorable_excursion);
UPDATE case_realized_outcome SET max_adverse_excursion = <decimal>max_adverse_excursion WHERE type::is::string(max_adverse_excursion);
UPDATE case_realized_outcome SET convergence_score = <decimal>convergence_score WHERE type::is::string(convergence_score);

UPDATE lineage_metric_row SET hit_rate = <decimal>hit_rate WHERE type::is::string(hit_rate);
UPDATE lineage_metric_row SET mean_return = <decimal>mean_return WHERE type::is::string(mean_return);
UPDATE lineage_metric_row SET mean_net_return = <decimal>mean_net_return WHERE type::is::string(mean_net_return);
UPDATE lineage_metric_row SET follow_expectancy = <decimal>follow_expectancy WHERE type::is::string(follow_expectancy);
UPDATE lineage_metric_row SET fade_expectancy = <decimal>fade_expectancy WHERE type::is::string(fade_expectancy);
UPDATE lineage_metric_row SET wait_expectancy = <decimal>wait_expectancy WHERE type::is::string(wait_expectancy);
UPDATE lineage_metric_row SET mean_mfe = <decimal>mean_mfe WHERE type::is::string(mean_mfe);
UPDATE lineage_metric_row SET mean_mae = <decimal>mean_mae WHERE type::is::string(mean_mae);
UPDATE lineage_metric_row SET follow_through_rate = <decimal>follow_through_rate WHERE type::is::string(follow_through_rate);
UPDATE lineage_metric_row SET invalidation_rate = <decimal>invalidation_rate WHERE type::is::string(invalidation_rate);
UPDATE lineage_metric_row SET structure_retention_rate = <decimal>structure_retention_rate WHERE type::is::string(structure_retention_rate);
UPDATE lineage_metric_row SET mean_convergence_score = <decimal>mean_convergence_score WHERE type::is::string(mean_convergence_score);
UPDATE lineage_metric_row SET mean_external_delta = <decimal>mean_external_delta WHERE type::is::string(mean_external_delta);
UPDATE lineage_metric_row SET external_follow_through_rate = <decimal>external_follow_through_rate WHERE type::is::string(external_follow_through_rate);

UPDATE us_lineage_metric_row SET hit_rate = <decimal>hit_rate WHERE type::is::string(hit_rate);
UPDATE us_lineage_metric_row SET mean_return = <decimal>mean_return WHERE type::is::string(mean_return);
UPDATE us_lineage_metric_row SET follow_expectancy = <decimal>follow_expectancy WHERE type::is::string(follow_expectancy);
UPDATE us_lineage_metric_row SET fade_expectancy = <decimal>fade_expectancy WHERE type::is::string(fade_expectancy);
UPDATE us_lineage_metric_row SET wait_expectancy = <decimal>wait_expectancy WHERE type::is::string(wait_expectancy);
"#;

const MIGRATION_014: &str = r#"
DEFINE FIELD OVERWRITE execution_policy ON action_workflow TYPE string;
DEFINE FIELD OVERWRITE governance_reason_code ON action_workflow TYPE string;
DEFINE FIELD OVERWRITE execution_policy ON action_workflow_event TYPE string;
DEFINE FIELD OVERWRITE governance_reason_code ON action_workflow_event TYPE string;
"#;

const MIGRATION_015: &str = r#"
DEFINE FIELD IF NOT EXISTS family_label ON case_reasoning_assessment TYPE option<string>;
"#;

const MIGRATION_016: &str = r#"
-- Candidate mechanism: success patterns promoted to durable mechanisms
DEFINE TABLE IF NOT EXISTS candidate_mechanism SCHEMAFULL;
DEFINE FIELD mechanism_id ON candidate_mechanism TYPE string;
DEFINE FIELD market ON candidate_mechanism TYPE string;
DEFINE FIELD center_kind ON candidate_mechanism TYPE string;
DEFINE FIELD role ON candidate_mechanism TYPE string;
DEFINE FIELD channel_signature ON candidate_mechanism TYPE string;
DEFINE FIELD dominant_channels ON candidate_mechanism TYPE array;
DEFINE FIELD top_family ON candidate_mechanism TYPE string;
DEFINE FIELD samples ON candidate_mechanism TYPE int;
DEFINE FIELD mean_net_return ON candidate_mechanism TYPE decimal;
DEFINE FIELD mean_strength ON candidate_mechanism TYPE decimal;
DEFINE FIELD mean_coherence ON candidate_mechanism TYPE decimal;
DEFINE FIELD mean_channel_diversity ON candidate_mechanism TYPE decimal;
DEFINE FIELD mode ON candidate_mechanism TYPE string;
DEFINE FIELD promoted_at_tick ON candidate_mechanism TYPE int;
DEFINE FIELD last_seen_tick ON candidate_mechanism TYPE int;
DEFINE FIELD last_hit_tick ON candidate_mechanism TYPE option<int>;
DEFINE FIELD consecutive_misses ON candidate_mechanism TYPE int;
DEFINE FIELD post_promotion_hits ON candidate_mechanism TYPE int;
DEFINE FIELD post_promotion_misses ON candidate_mechanism TYPE int;
DEFINE FIELD post_promotion_net_return ON candidate_mechanism TYPE decimal;
DEFINE FIELD created_at ON candidate_mechanism TYPE string;
DEFINE FIELD updated_at ON candidate_mechanism TYPE string;
DEFINE INDEX idx_mechanism_id ON candidate_mechanism FIELDS mechanism_id UNIQUE;
DEFINE INDEX idx_mechanism_market ON candidate_mechanism FIELDS market;
DEFINE INDEX idx_mechanism_mode ON candidate_mechanism FIELDS mode;
"#;

const MIGRATION_017: &str = r#"
-- Causal schema: candidate mechanisms elevated to transferable causal structures
DEFINE TABLE IF NOT EXISTS causal_schema SCHEMAFULL;
DEFINE FIELD schema_id ON causal_schema TYPE string;
DEFINE FIELD mechanism_id ON causal_schema TYPE string;
DEFINE FIELD market ON causal_schema TYPE string;
DEFINE FIELD channel_chain ON causal_schema TYPE array;
DEFINE FIELD causal_narrative ON causal_schema TYPE string;
DEFINE FIELD regime_affinity ON causal_schema TYPE array;
DEFINE FIELD session_affinity ON causal_schema TYPE array;
DEFINE FIELD min_coherence ON causal_schema TYPE decimal;
DEFINE FIELD min_strength ON causal_schema TYPE decimal;
DEFINE FIELD min_convergence_score ON causal_schema TYPE decimal;
DEFINE FIELD preferred_contest_states ON causal_schema TYPE array;
DEFINE FIELD invalidation_rules ON causal_schema TYPE array;
DEFINE FIELD observed_symbols ON causal_schema TYPE array;
DEFINE FIELD observed_sectors ON causal_schema TYPE array;
DEFINE FIELD applicable_center_kinds ON causal_schema TYPE array;
DEFINE FIELD cross_symbol_validated ON causal_schema TYPE bool;
DEFINE FIELD cross_session_validated ON causal_schema TYPE bool;
DEFINE FIELD cross_regime_validated ON causal_schema TYPE bool;
DEFINE FIELD total_applications ON causal_schema TYPE int;
DEFINE FIELD successful_applications ON causal_schema TYPE int;
DEFINE FIELD failed_applications ON causal_schema TYPE int;
DEFINE FIELD mean_return_when_applied ON causal_schema TYPE decimal;
DEFINE FIELD mean_return_when_preconditions_met ON causal_schema TYPE decimal;
DEFINE FIELD mean_return_when_preconditions_violated ON causal_schema TYPE decimal;
DEFINE FIELD status ON causal_schema TYPE string;
DEFINE FIELD promoted_at_tick ON causal_schema TYPE int;
DEFINE FIELD last_applied_tick ON causal_schema TYPE int;
DEFINE FIELD created_at ON causal_schema TYPE string;
DEFINE FIELD updated_at ON causal_schema TYPE string;
DEFINE INDEX idx_schema_id ON causal_schema FIELDS schema_id UNIQUE;
DEFINE INDEX idx_schema_mechanism ON causal_schema FIELDS mechanism_id;
DEFINE INDEX idx_schema_market ON causal_schema FIELDS market;
DEFINE INDEX idx_schema_status ON causal_schema FIELDS status;
"#;

const MIGRATION_018: &str = r#"
DEFINE FIELD primary_lens ON case_realized_outcome TYPE option<string>;
"#;

const MIGRATION_019: &str = r#"
DEFINE TABLE IF NOT EXISTS terrain_shareholder SCHEMAFULL;
DEFINE FIELD symbol ON terrain_shareholder TYPE string;
DEFINE FIELD market ON terrain_shareholder TYPE string;
DEFINE FIELD holders ON terrain_shareholder TYPE array;
DEFINE FIELD fetched_at ON terrain_shareholder TYPE string;
DEFINE INDEX idx_terrain_shareholder_symbol ON terrain_shareholder FIELDS market, symbol UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_fund_holder SCHEMAFULL;
DEFINE FIELD symbol ON terrain_fund_holder TYPE string;
DEFINE FIELD market ON terrain_fund_holder TYPE string;
DEFINE FIELD funds ON terrain_fund_holder TYPE array;
DEFINE FIELD fetched_at ON terrain_fund_holder TYPE string;
DEFINE INDEX idx_terrain_fund_holder_symbol ON terrain_fund_holder FIELDS market, symbol UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_13f SCHEMAFULL;
DEFINE FIELD cik ON terrain_13f TYPE string;
DEFINE FIELD name ON terrain_13f TYPE option<string>;
DEFINE FIELD holdings ON terrain_13f TYPE array;
DEFINE FIELD period ON terrain_13f TYPE option<string>;
DEFINE FIELD fetched_at ON terrain_13f TYPE string;
DEFINE INDEX idx_terrain_13f_cik ON terrain_13f FIELDS cik UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_valuation_peers SCHEMAFULL;
DEFINE FIELD symbol ON terrain_valuation_peers TYPE string;
DEFINE FIELD market ON terrain_valuation_peers TYPE string;
DEFINE FIELD summary ON terrain_valuation_peers TYPE object;
DEFINE FIELD peers ON terrain_valuation_peers TYPE array;
DEFINE FIELD fetched_at ON terrain_valuation_peers TYPE string;
DEFINE INDEX idx_terrain_valuation_peers_symbol ON terrain_valuation_peers FIELDS market, symbol UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_ratings SCHEMAFULL;
DEFINE FIELD symbol ON terrain_ratings TYPE string;
DEFINE FIELD market ON terrain_ratings TYPE string;
DEFINE FIELD consensus ON terrain_ratings TYPE object;
DEFINE FIELD target_price ON terrain_ratings TYPE object;
DEFINE FIELD meta ON terrain_ratings TYPE object;
DEFINE FIELD fetched_at ON terrain_ratings TYPE string;
DEFINE INDEX idx_terrain_ratings_symbol ON terrain_ratings FIELDS market, symbol UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_calendar SCHEMAFULL;
DEFINE FIELD event_type ON terrain_calendar TYPE string;
DEFINE FIELD market ON terrain_calendar TYPE string;
DEFINE FIELD events ON terrain_calendar TYPE array;
DEFINE FIELD fetched_at ON terrain_calendar TYPE string;
DEFINE INDEX idx_terrain_calendar_key ON terrain_calendar FIELDS market, event_type UNIQUE;

DEFINE TABLE IF NOT EXISTS terrain_insider SCHEMAFULL;
DEFINE FIELD symbol ON terrain_insider TYPE string;
DEFINE FIELD trades ON terrain_insider TYPE array;
DEFINE FIELD fetched_at ON terrain_insider TYPE string;
DEFINE INDEX idx_terrain_insider_symbol ON terrain_insider FIELDS symbol UNIQUE;
"#;

const MIGRATION_020: &str = r#"
DEFINE TABLE IF NOT EXISTS edge_learning_ledger SCHEMAFULL;
DEFINE FIELD ledger_id ON edge_learning_ledger TYPE string;
DEFINE FIELD market ON edge_learning_ledger TYPE string;
DEFINE FIELD entries ON edge_learning_ledger TYPE array;
DEFINE FIELD updated_at ON edge_learning_ledger TYPE string;
DEFINE INDEX idx_edge_learning_ledger_id ON edge_learning_ledger FIELDS ledger_id UNIQUE;
DEFINE INDEX idx_edge_learning_ledger_market ON edge_learning_ledger FIELDS market UNIQUE;
"#;

const MIGRATION_021: &str = r#"
DEFINE FIELD IF NOT EXISTS case_signature ON tactical_setup TYPE option<object>;
DEFINE FIELD IF NOT EXISTS archetype_projections ON tactical_setup TYPE array;
DEFINE FIELD IF NOT EXISTS case_signature ON case_reasoning_assessment TYPE option<object>;
DEFINE FIELD IF NOT EXISTS archetype_projections ON case_reasoning_assessment TYPE array;
"#;

const MIGRATION_022: &str = r#"
DEFINE FIELD IF NOT EXISTS expectation_bindings ON tactical_setup TYPE array;
DEFINE FIELD IF NOT EXISTS expectation_violations ON tactical_setup TYPE array;
DEFINE FIELD IF NOT EXISTS expectation_bindings ON case_reasoning_assessment TYPE array;
DEFINE FIELD IF NOT EXISTS expectation_violations ON case_reasoning_assessment TYPE array;
"#;

const MIGRATION_023: &str = r#"
DEFINE TABLE IF NOT EXISTS discovered_archetype SCHEMAFULL;
DEFINE FIELD archetype_id ON discovered_archetype TYPE string;
DEFINE FIELD market ON discovered_archetype TYPE string;
DEFINE FIELD archetype_key ON discovered_archetype TYPE string;
DEFINE FIELD label ON discovered_archetype TYPE string;
DEFINE FIELD topology ON discovered_archetype TYPE option<string>;
DEFINE FIELD temporal_shape ON discovered_archetype TYPE option<string>;
DEFINE FIELD conflict_shape ON discovered_archetype TYPE option<string>;
DEFINE FIELD dominant_channels ON discovered_archetype TYPE array;
DEFINE FIELD expectation_violation_kinds ON discovered_archetype TYPE array;
DEFINE FIELD family_label ON discovered_archetype TYPE option<string>;
DEFINE FIELD samples ON discovered_archetype TYPE int;
DEFINE FIELD hits ON discovered_archetype TYPE int;
DEFINE FIELD hit_rate ON discovered_archetype TYPE decimal;
DEFINE FIELD mean_net_return ON discovered_archetype TYPE decimal;
DEFINE FIELD mean_affinity ON discovered_archetype TYPE decimal;
DEFINE FIELD updated_at ON discovered_archetype TYPE string;
DEFINE INDEX idx_discovered_archetype_id ON discovered_archetype FIELDS archetype_id UNIQUE;
DEFINE INDEX idx_discovered_archetype_market ON discovered_archetype FIELDS market;
DEFINE INDEX idx_discovered_archetype_key ON discovered_archetype FIELDS market, archetype_key UNIQUE;
"#;

const MIGRATION_024: &str = r#"
DEFINE FIELD IF NOT EXISTS inferred_intent ON tactical_setup TYPE option<object>;
DEFINE FIELD IF NOT EXISTS inferred_intent ON case_reasoning_assessment TYPE option<object>;
"#;

const MIGRATION_025: &str = r#"
DEFINE TABLE IF NOT EXISTS horizon_evaluation SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_horizon_evaluation_setup_id ON horizon_evaluation FIELDS setup_id;
"#;

const MIGRATION_026: &str = r#"
DEFINE TABLE IF NOT EXISTS case_resolution SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_case_resolution_setup_id ON case_resolution FIELDS setup_id;
"#;

// MIGRATION_027: primary_horizon fields added in Resolution System Wave 5 hotfix.
// TacticalSetupRecord.primary_horizon is HorizonBucket (non-optional, serde default Session).
// CaseReasoningAssessmentRecord.primary_horizon is Option<HorizonBucket>.
// Both tables are SCHEMAFULL, so the field must be declared before writes can land.
// HorizonBucket serializes as snake_case string (fast5m / mid30m / session / etc).
// Pre-existing tactical_setup records are backfilled to 'session' so they satisfy
// the strict `TYPE string` constraint. New writes always include the field.
const MIGRATION_027: &str = r#"
DEFINE FIELD IF NOT EXISTS primary_horizon ON tactical_setup TYPE string;
DEFINE FIELD IF NOT EXISTS primary_horizon ON case_reasoning_assessment TYPE option<string>;
UPDATE tactical_setup SET primary_horizon = 'session' WHERE primary_horizon = NONE;
"#;

// MIGRATION_028: tick_record.tick_number UNIQUE constraint prevents eden from
// ever restarting against an existing DB — every new run starts counting ticks
// from 1, which collides with the existing tick 1 and blocks the entire write
// transaction. tick_number is not meaningfully unique across runs anyway; the
// record_id `tick_{timestamp}_{tick_number}` carries the timestamp that
// disambiguates runs. Drop the UNIQUE, keep the index for range queries.
const MIGRATION_028: &str = r#"
REMOVE INDEX idx_tick_number ON tick_record;
DEFINE INDEX idx_tick_number ON tick_record FIELDS tick_number;
"#;

const MIGRATION_029: &str = r#"
DEFINE FIELD IF NOT EXISTS freshness_state ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS timing_state ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS timing_position_in_range ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD IF NOT EXISTS local_state ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS local_state_confidence ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD IF NOT EXISTS actionability_score ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD IF NOT EXISTS actionability_state ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS review_reason_code ON case_reasoning_assessment TYPE option<string>;
DEFINE FIELD IF NOT EXISTS state_persistence_ticks ON case_reasoning_assessment TYPE option<int>;
DEFINE FIELD IF NOT EXISTS direction_stability_rounds ON case_reasoning_assessment TYPE option<int>;
DEFINE FIELD IF NOT EXISTS state_reason_codes ON case_reasoning_assessment TYPE array;
"#;

const MIGRATION_030: &str = r#"
DEFINE FIELD IF NOT EXISTS actionability_score ON case_reasoning_assessment TYPE option<decimal>;
DEFINE FIELD IF NOT EXISTS review_reason_code ON case_reasoning_assessment TYPE option<string>;
"#;

const MIGRATION_031: &str = r#"
DEFINE TABLE IF NOT EXISTS symbol_perception_state SCHEMAFULL;
DEFINE FIELD state_id ON symbol_perception_state TYPE string;
DEFINE FIELD market ON symbol_perception_state TYPE string;
DEFINE FIELD symbol ON symbol_perception_state TYPE string;
DEFINE FIELD sector ON symbol_perception_state TYPE option<string>;
DEFINE FIELD label ON symbol_perception_state TYPE string;
DEFINE FIELD state_kind ON symbol_perception_state TYPE string;
DEFINE FIELD direction ON symbol_perception_state TYPE option<string>;
DEFINE FIELD age_ticks ON symbol_perception_state TYPE int;
DEFINE FIELD state_persistence_ticks ON symbol_perception_state TYPE int;
DEFINE FIELD direction_stability_rounds ON symbol_perception_state TYPE int;
DEFINE FIELD support_count ON symbol_perception_state TYPE int;
DEFINE FIELD contradict_count ON symbol_perception_state TYPE int;
DEFINE FIELD count_support_fraction ON symbol_perception_state TYPE decimal;
DEFINE FIELD weighted_support_fraction ON symbol_perception_state TYPE decimal;
DEFINE FIELD strength ON symbol_perception_state TYPE decimal;
DEFINE FIELD confidence ON symbol_perception_state TYPE decimal;
DEFINE FIELD support_weight ON symbol_perception_state TYPE decimal;
DEFINE FIELD contradict_weight ON symbol_perception_state TYPE decimal;
DEFINE FIELD trend ON symbol_perception_state TYPE string;
DEFINE FIELD supporting_evidence ON symbol_perception_state TYPE array;
DEFINE FIELD opposing_evidence ON symbol_perception_state TYPE array;
DEFINE FIELD missing_evidence ON symbol_perception_state TYPE array;
DEFINE FIELD conflict_age_ticks ON symbol_perception_state TYPE int;
DEFINE FIELD expectations ON symbol_perception_state TYPE array;
DEFINE FIELD active_setup_ids ON symbol_perception_state TYPE array;
DEFINE FIELD dominant_intent_kind ON symbol_perception_state TYPE option<string>;
DEFINE FIELD dominant_intent_state ON symbol_perception_state TYPE option<string>;
DEFINE FIELD cluster_key ON symbol_perception_state TYPE string;
DEFINE FIELD cluster_label ON symbol_perception_state TYPE string;
DEFINE FIELD last_transition_summary ON symbol_perception_state TYPE option<string>;
DEFINE FIELD updated_at ON symbol_perception_state TYPE string;
DEFINE INDEX idx_symbol_perception_state_id ON symbol_perception_state FIELDS state_id UNIQUE;
DEFINE INDEX idx_symbol_perception_state_market_symbol ON symbol_perception_state FIELDS market, symbol UNIQUE;
DEFINE INDEX idx_symbol_perception_state_market_kind ON symbol_perception_state FIELDS market, state_kind;
"#;

const MIGRATION_032: &str = r#"
DEFINE FIELD IF NOT EXISTS support_count ON symbol_perception_state TYPE int;
DEFINE FIELD IF NOT EXISTS contradict_count ON symbol_perception_state TYPE int;
DEFINE FIELD IF NOT EXISTS count_support_fraction ON symbol_perception_state TYPE decimal;
DEFINE FIELD IF NOT EXISTS weighted_support_fraction ON symbol_perception_state TYPE decimal;
DEFINE FIELD IF NOT EXISTS support_weight ON symbol_perception_state TYPE decimal;
DEFINE FIELD IF NOT EXISTS contradict_weight ON symbol_perception_state TYPE decimal;
DEFINE FIELD IF NOT EXISTS missing_evidence ON symbol_perception_state TYPE array;
DEFINE FIELD IF NOT EXISTS conflict_age_ticks ON symbol_perception_state TYPE int;
DEFINE FIELD IF NOT EXISTS expectations ON symbol_perception_state TYPE array;
"#;

// MIGRATION_033: fix knowledge_node_state UNIQUE index collision across markets.
//
// The original `idx_knowledge_node_state_node` keyed on `node_id` alone, which
// meant `market:market` (the synthetic market-level node) could only ever be
// written by ONE market — whichever ran first won, and the other market's
// runtime failed every tick with
//   "Database index `idx_knowledge_node_state_node` already contains
//    'market:market', with record `knowledge_node_state:`us:market:market``"
// Confirmed live on 2026-04-17 when HK runtime booted against a DB that
// already held US's `market:market` row. Drop the single-field index and
// replace with a composite `(market, node_id)` index so both markets can
// coexist.
const MIGRATION_033: &str = r#"
REMOVE INDEX idx_knowledge_node_state_node ON knowledge_node_state;
DEFINE INDEX idx_knowledge_node_state_market_node ON knowledge_node_state FIELDS market, node_id UNIQUE;
"#;

const MIGRATION_034: &str = r#"
DEFINE FIELD IF NOT EXISTS family_key ON tactical_setup TYPE option<string>;
"#;

// Periodic PressureBeliefField snapshots — Eden's cross-tick memory.
// Spec: docs/superpowers/specs/2026-04-19-belief-persistence-design.md
const MIGRATION_035: &str = r#"
DEFINE TABLE belief_snapshot SCHEMAFULL;
DEFINE FIELD market ON belief_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON belief_snapshot TYPE datetime;
DEFINE FIELD tick ON belief_snapshot TYPE int;
DEFINE FIELD gaussian ON belief_snapshot TYPE array;
DEFINE FIELD categorical ON belief_snapshot TYPE array;
DEFINE INDEX idx_belief_market_ts ON belief_snapshot FIELDS market, snapshot_ts;
"#;

// IntentBeliefField cross-session persistence — per-symbol
// CategoricalBelief<IntentKind> rolled out so sector_intent /
// intent_modulation / sector_alignment have warm priors on session
// start instead of starting from uniform noise.
const MIGRATION_036: &str = r#"
DEFINE TABLE intent_belief_snapshot SCHEMAFULL;
DEFINE FIELD market ON intent_belief_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON intent_belief_snapshot TYPE datetime;
DEFINE FIELD rows ON intent_belief_snapshot TYPE array;
DEFINE INDEX idx_intent_belief_market_ts ON intent_belief_snapshot FIELDS market, snapshot_ts;
"#;

// BrokerArchetypeBeliefField cross-session persistence — HK only.
// Broker archetype is the slowest-learning categorical in the stack
// (takes many ticks of bid/ask presence to dominate), so discarding
// it every night was a bigger loss than the intent snapshot.
const MIGRATION_037: &str = r#"
DEFINE TABLE broker_archetype_snapshot SCHEMAFULL;
DEFINE FIELD market ON broker_archetype_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON broker_archetype_snapshot TYPE datetime;
DEFINE FIELD rows ON broker_archetype_snapshot TYPE array;
DEFINE INDEX idx_broker_arch_market_ts ON broker_archetype_snapshot FIELDS market, snapshot_ts;
"#;

// Backfill legacy action_workflow / action_workflow_event payloads with
// `payload.market`, handled in Rust post-migration because the inference relies
// on existing payload/workflow identifiers rather than a simple schema rewrite.
const MIGRATION_038: &str = r#"
SELECT * FROM action_workflow LIMIT 0;
"#;

// action_workflow payloads carry arbitrary JSON objects. Under SCHEMAFULL, a
// plain `TYPE object` field was collapsing nested content to `{}` on
// round-trip. Mark these fields FLEXIBLE so workflow/state payloads persist
// intact, then let the post-migration hook re-run the market backfill against
// the now-writable payload field.
const MIGRATION_039: &str = r#"
DEFINE FIELD OVERWRITE payload ON action_workflow FLEXIBLE TYPE object;
DEFINE FIELD OVERWRITE payload ON action_workflow_event FLEXIBLE TYPE object;
"#;

// 2026-04-20 live bug: all three snapshot tables (belief /
// intent_belief / broker_archetype) were writing `snapshot_ts` as
// ISO-8601 strings (chrono DateTime<Utc> default serde) into fields
// declared TYPE datetime. SurrealDB 2.x rejects with
// "Found '2026-04-20T...Z' for field snapshot_ts, but expected a
// datetime". Switch field type to string — ISO-8601 is lexicographically
// sortable so existing queries (ORDER BY snapshot_ts / WHERE
// snapshot_ts BETWEEN) keep working.
const MIGRATION_040: &str = r#"
DEFINE FIELD OVERWRITE snapshot_ts ON belief_snapshot TYPE string;
DEFINE FIELD OVERWRITE snapshot_ts ON intent_belief_snapshot TYPE string;
DEFINE FIELD OVERWRITE snapshot_ts ON broker_archetype_snapshot TYPE string;
"#;

// 2026-04-23: Regime fingerprint as continuous embedding replacement
// for the discrete RegimeType enum (US) / LiveWorldSummary.regime
// string (HK). 5 universal feature dims + market-specific extension
// fields + deterministic bucket_key for use in conditioned learning.
// snapshot_ts is string (ISO-8601) for chrono compatibility per the
// MIGRATION_040 convention.
const MIGRATION_041: &str = r#"
DEFINE TABLE regime_fingerprint_snapshot SCHEMAFULL;
DEFINE FIELD market ON regime_fingerprint_snapshot TYPE string;
DEFINE FIELD snapshot_ts ON regime_fingerprint_snapshot TYPE string;
DEFINE FIELD tick ON regime_fingerprint_snapshot TYPE int;
DEFINE FIELD stress ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD synchrony ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD bull_bias ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD activity ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD turn_pressure ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD planner_utility ON regime_fingerprint_snapshot TYPE option<float>;
DEFINE FIELD regime_continuity ON regime_fingerprint_snapshot TYPE option<float>;
DEFINE FIELD dominant_driver ON regime_fingerprint_snapshot TYPE option<string>;
DEFINE FIELD legacy_label ON regime_fingerprint_snapshot TYPE string;
DEFINE FIELD legacy_confidence ON regime_fingerprint_snapshot TYPE float;
DEFINE FIELD bucket_key ON regime_fingerprint_snapshot TYPE string;
DEFINE INDEX idx_regime_fp_market_ts ON regime_fingerprint_snapshot FIELDS market, snapshot_ts;
DEFINE INDEX idx_regime_fp_bucket ON regime_fingerprint_snapshot FIELDS market, bucket_key;
"#;

// MIGRATION_042: drop polymarket_priors field after legacy Polymarket
// integration removal (2026-04-26). Existing rows had empty arrays only.
const MIGRATION_042: &str = r#"
REMOVE FIELD polymarket_priors ON TABLE tick_record;
"#;

// MIGRATION_043: tick_archive used to be keyed only by tick_number, which
// lets HK and US overwrite each other when they share data/eden.db. Add an
// explicit market field and replace the single-field unique index with a
// composite market/tick index.
const MIGRATION_043: &str = r#"
DEFINE FIELD IF NOT EXISTS market ON tick_archive TYPE string;
UPDATE tick_archive SET market = 'unknown' WHERE market = NONE;
REMOVE INDEX idx_tick_archive_tick ON tick_archive;
DEFINE INDEX idx_tick_archive_market_tick ON tick_archive FIELDS market, tick_number UNIQUE;
"#;

pub const LATEST_SCHEMA_VERSION: u32 = 43;

const MIGRATIONS: [SchemaMigration; 43] = [
    SchemaMigration {
        version: 1,
        name: "bootstrap_core_schema",
        statements: MIGRATION_001,
    },
    SchemaMigration {
        version: 2,
        name: "tick_record_graph_edge_transitions",
        statements: MIGRATION_002,
    },
    SchemaMigration {
        version: 3,
        name: "persist_time_fields_as_rfc3339_strings",
        statements: MIGRATION_003,
    },
    SchemaMigration {
        version: 4,
        name: "agent_macro_event_and_knowledge_link_history",
        statements: MIGRATION_004,
    },
    SchemaMigration {
        version: 5,
        name: "agent_macro_event_and_knowledge_link_state",
        statements: MIGRATION_005,
    },
    SchemaMigration {
        version: 6,
        name: "generic_knowledge_node_history_and_state",
        statements: MIGRATION_006,
    },
    SchemaMigration {
        version: 7,
        name: "knowledge_link_relation_attributes",
        statements: MIGRATION_007,
    },
    SchemaMigration {
        version: 8,
        name: "backfill_datetime_to_string",
        statements: MIGRATION_008,
    },
    SchemaMigration {
        version: 9,
        name: "tick_archive_table",
        statements: MIGRATION_009,
    },
    SchemaMigration {
        version: 10,
        name: "knowledge_event_history_and_state",
        statements: MIGRATION_010,
    },
    SchemaMigration {
        version: 11,
        name: "knowledge_graph_confidence_as_decimal",
        statements: MIGRATION_011,
    },
    SchemaMigration {
        version: 12,
        name: "workflow_and_assessment_numeric_fields_as_decimal",
        statements: MIGRATION_012,
    },
    SchemaMigration {
        version: 13,
        name: "lineage_and_outcome_numeric_fields_as_decimal",
        statements: MIGRATION_013,
    },
    SchemaMigration {
        version: 14,
        name: "workflow_execution_policy_fields",
        statements: MIGRATION_014,
    },
    SchemaMigration {
        version: 15,
        name: "case_reasoning_assessment_family_label",
        statements: MIGRATION_015,
    },
    SchemaMigration {
        version: 16,
        name: "candidate_mechanism_table",
        statements: MIGRATION_016,
    },
    SchemaMigration {
        version: 17,
        name: "causal_schema_table",
        statements: MIGRATION_017,
    },
    SchemaMigration {
        version: 18,
        name: "case_realized_outcome_primary_lens",
        statements: MIGRATION_018,
    },
    SchemaMigration {
        version: 19,
        name: "terrain_cache_tables",
        statements: MIGRATION_019,
    },
    SchemaMigration {
        version: 20,
        name: "edge_learning_ledger_table",
        statements: MIGRATION_020,
    },
    SchemaMigration {
        version: 21,
        name: "case_signature_and_archetype_fields",
        statements: MIGRATION_021,
    },
    SchemaMigration {
        version: 22,
        name: "expectation_binding_and_violation_fields",
        statements: MIGRATION_022,
    },
    SchemaMigration {
        version: 23,
        name: "discovered_archetype_table",
        statements: MIGRATION_023,
    },
    SchemaMigration {
        version: 24,
        name: "intent_hypothesis_fields",
        statements: MIGRATION_024,
    },
    SchemaMigration {
        version: 25,
        name: "horizon_evaluation_table",
        statements: MIGRATION_025,
    },
    SchemaMigration {
        version: 26,
        name: "case_resolution_table",
        statements: MIGRATION_026,
    },
    SchemaMigration {
        version: 27,
        name: "primary_horizon_fields_on_setup_and_assessment",
        statements: MIGRATION_027,
    },
    SchemaMigration {
        version: 28,
        name: "drop_tick_number_unique_constraint",
        statements: MIGRATION_028,
    },
    SchemaMigration {
        version: 29,
        name: "case_reasoning_assessment_perception_fields",
        statements: MIGRATION_029,
    },
    SchemaMigration {
        version: 30,
        name: "case_reasoning_assessment_review_reason_and_score",
        statements: MIGRATION_030,
    },
    SchemaMigration {
        version: 31,
        name: "symbol_perception_state_table",
        statements: MIGRATION_031,
    },
    SchemaMigration {
        version: 32,
        name: "symbol_perception_state_expectation_fields",
        statements: MIGRATION_032,
    },
    SchemaMigration {
        version: 33,
        name: "knowledge_node_state_market_node_unique",
        statements: MIGRATION_033,
    },
    SchemaMigration {
        version: 34,
        name: "tactical_setup_family_key_field",
        statements: MIGRATION_034,
    },
    SchemaMigration {
        version: 35,
        name: "belief_snapshot_table",
        statements: MIGRATION_035,
    },
    SchemaMigration {
        version: 36,
        name: "intent_belief_snapshot_table",
        statements: MIGRATION_036,
    },
    SchemaMigration {
        version: 37,
        name: "broker_archetype_snapshot_table",
        statements: MIGRATION_037,
    },
    SchemaMigration {
        version: 38,
        name: "action_workflow_payload_market_backfill",
        statements: MIGRATION_038,
    },
    SchemaMigration {
        version: 39,
        name: "action_workflow_payload_field_any",
        statements: MIGRATION_039,
    },
    SchemaMigration {
        version: 40,
        name: "snapshot_ts_as_string_for_chrono_compat",
        statements: MIGRATION_040,
    },
    SchemaMigration {
        version: 41,
        name: "regime_fingerprint_snapshot_table",
        statements: MIGRATION_041,
    },
    SchemaMigration {
        version: 42,
        name: "remove_polymarket_priors",
        statements: MIGRATION_042,
    },
    SchemaMigration {
        version: 43,
        name: "market_scoped_tick_archive",
        statements: MIGRATION_043,
    },
];

pub fn migrations() -> &'static [SchemaMigration] {
    &MIGRATIONS
}

pub fn pending_migrations(current_version: Option<u32>) -> &'static [SchemaMigration] {
    let start = match current_version {
        Some(version) => MIGRATIONS
            .iter()
            .position(|migration| migration.version > version)
            .unwrap_or(MIGRATIONS.len()),
        None => 0,
    };
    &MIGRATIONS[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_migrations_from_none_returns_all_steps() {
        let pending = pending_migrations(None);
        assert_eq!(pending.len(), MIGRATIONS.len());
        assert_eq!(pending[0].version, 1);
        assert_eq!(pending.last().unwrap().version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn pending_migrations_skip_applied_versions() {
        let pending = pending_migrations(Some(7));
        assert_eq!(pending[0].version, 8);
        assert_eq!(pending.last().unwrap().version, LATEST_SCHEMA_VERSION);

        assert!(pending_migrations(Some(LATEST_SCHEMA_VERSION)).is_empty());
    }
}
