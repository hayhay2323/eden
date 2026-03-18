/// SurrealDB table and index definitions for Eden.
/// Called once at startup to ensure schema exists.
pub const SCHEMA: &str = r#"
-- Tick records: one per pipeline cycle
DEFINE TABLE tick_record SCHEMAFULL;
DEFINE FIELD tick_number ON tick_record TYPE int;
DEFINE FIELD timestamp ON tick_record TYPE datetime;
DEFINE FIELD signals ON tick_record TYPE object;
DEFINE INDEX idx_tick_number ON tick_record FIELDS tick_number UNIQUE;
DEFINE INDEX idx_timestamp ON tick_record FIELDS timestamp;

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
"#;
