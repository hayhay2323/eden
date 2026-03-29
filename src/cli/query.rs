use super::render::print_polymarket_snapshot;
#[cfg(feature = "persistence")]
use super::render::{
    print_causal_flips, print_causal_timeline, print_lineage_history, print_lineage_report,
    print_lineage_rows, select_lineage_rows,
};
use super::*;

#[cfg(feature = "persistence")]
async fn open_query_store() -> Result<EdenStore, AppError> {
    let eden_db_path = std::env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".into());
    EdenStore::open(&eden_db_path).await
}

#[cfg(feature = "persistence")]
pub async fn run_cli_query(command: CliCommand) -> Result<(), AppError> {
    let store = open_query_store().await?;

    match command {
        CliCommand::Live => Ok(()),
        CliCommand::UsLive => Ok(()),
        CliCommand::Polymarket { json } => {
            let configs = load_polymarket_configs()
                .map_err(|error| -> AppError { Box::new(std::io::Error::other(error)) })?;
            if configs.is_empty() {
                println!("No Polymarket markets configured. Set POLYMARKET_MARKETS first.");
                return Ok(());
            }
            let snapshot = fetch_polymarket_snapshot(&configs)
                .await
                .map_err(|error| -> AppError { Box::new(std::io::Error::other(error)) })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "configs": configs,
                        "snapshot": snapshot,
                    }))?
                );
            } else {
                print_polymarket_snapshot(&configs, &snapshot);
            }
            Ok(())
        }
        CliCommand::CausalTimeline {
            leaf_scope_key,
            limit,
        } => {
            let Some(timeline) = store.recent_causal_timeline(&leaf_scope_key, limit).await? else {
                println!("No causal timeline found for {}", leaf_scope_key);
                return Ok(());
            };
            print_causal_timeline(&timeline);
            Ok(())
        }
        CliCommand::CausalFlips { limit } => {
            let records = store.recent_tick_window(limit).await?;
            let mut history = TickHistory::new(records.len().max(1));
            for record in records {
                history.push(record);
            }
            let timelines = compute_causal_timelines(&history);
            print_causal_flips(timelines.values().collect());
            Ok(())
        }
        CliCommand::Lineage {
            limit,
            filters,
            view,
        } => {
            let stats = store.recent_lineage_stats(limit).await?;
            let stats = stats
                .filtered(&filters)
                .aligned(view.alignment)
                .sorted_by(view.sort_by)
                .truncated(view.top);
            if view.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "window_size": limit,
                        "filters": filters,
                        "top": view.top,
                        "sort_by": view.sort_by,
                        "alignment": view.alignment,
                        "stats": stats,
                    }))?
                );
            } else {
                print_lineage_report(&stats, limit, &filters, view.top);
            }
            Ok(())
        }
        CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        } => {
            let rows = store
                .recent_ranked_lineage_metric_rows(snapshots, view.top)
                .await?;
            let rows = select_lineage_rows(
                &rows,
                &filters,
                snapshots.saturating_mul(view.top.max(1)),
                view.latest_only,
                view.sort_by,
                view.alignment,
            );
            let records = snapshot_records_from_rows(&rows, &filters, view.latest_only);
            if view.json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else {
                print_lineage_history(&records, &filters, view.top);
            }
            Ok(())
        }
        CliCommand::LineageRows {
            rows,
            filters,
            view,
        } => {
            let ranked_rows = store
                .recent_ranked_lineage_metric_rows(rows.max(1), view.top)
                .await?;
            let rows = select_lineage_rows(
                &ranked_rows,
                &filters,
                rows,
                view.latest_only,
                view.sort_by,
                view.alignment,
            );
            if view.json {
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print_lineage_rows(&rows);
            }
            Ok(())
        }
    }
}

#[cfg(not(feature = "persistence"))]
pub async fn run_cli_query(command: CliCommand) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        CliCommand::Live => Ok(()),
        CliCommand::UsLive => Ok(()),
        CliCommand::Polymarket { json } => {
            let configs = load_polymarket_configs()
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if configs.is_empty() {
                println!("No Polymarket markets configured. Set POLYMARKET_MARKETS first.");
                return Ok(());
            }
            let snapshot = fetch_polymarket_snapshot(&configs)
                .await
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "configs": configs,
                        "snapshot": snapshot,
                    }))?
                );
            } else {
                print_polymarket_snapshot(&configs, &snapshot);
            }
            Ok(())
        }
        CliCommand::CausalTimeline {
            leaf_scope_key,
            limit,
        } => {
            let _ = (leaf_scope_key, limit);
            Err("causal query commands require building with --features persistence".into())
        }
        CliCommand::CausalFlips { limit } => {
            let _ = limit;
            Err("causal query commands require building with --features persistence".into())
        }
        CliCommand::Lineage {
            limit,
            filters,
            view,
        } => {
            let _ = (limit, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
        CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        } => {
            let _ = (snapshots, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
        CliCommand::LineageRows {
            rows,
            filters,
            view,
        } => {
            let _ = (rows, filters, view);
            Err("lineage query commands require building with --features persistence".into())
        }
    }
}
