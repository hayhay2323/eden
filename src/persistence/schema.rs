/// SurrealDB table and index definitions for Eden.
/// Called once at startup to ensure schema exists.
pub const SCHEMA: &str = r#"
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
DEFINE INDEX idx_us_lineage_metric_row_id ON us_lineage_metric_row FIELDS row_id UNIQUE;
DEFINE INDEX idx_us_lineage_metric_row_lookup ON us_lineage_metric_row FIELDS bucket, template, session, market_regime, tick_number;
"#;
