use crate::external::polymarket::{
    fetch_polymarket_snapshot, load_polymarket_configs, PolymarketMarketConfig, PolymarketSnapshot,
};
use crate::cli::commands::OperatorCommandDescriptor;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::{row_matches_filters, snapshot_records_from_rows};
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;
use crate::core::runtime_tasks::{
    RuntimeTaskCreateRequest, RuntimeTaskFilter, RuntimeTaskKind, RuntimeTaskRecord,
    RuntimeTaskStatus, RuntimeTaskStatusUpdateRequest,
};
#[cfg(feature = "persistence")]
use crate::temporal::buffer::TickHistory;
#[cfg(feature = "persistence")]
use crate::temporal::causality::{
    compute_causal_timelines, CausalFlipEvent, CausalTimeline, CausalTimelinePoint,
};
use crate::temporal::lineage::{LineageAlignmentFilter, LineageFilters, LineageSortKey};
use rust_decimal::Decimal;

type AppError = Box<dyn std::error::Error + Send + Sync>;

#[path = "cli/commands.rs"]
pub mod commands;
#[path = "cli/parser.rs"]
mod parser;
#[path = "cli/query.rs"]
mod query;
#[path = "cli/render.rs"]
mod render;

pub use parser::{parse_cli_command, CliCommand, LineageViewOptions};
pub use query::run_cli_query;
