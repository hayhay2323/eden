use super::*;

#[derive(Debug)]
pub enum CliCommand {
    Live,
    UsLive,
    CausalTimeline {
        leaf_scope_key: String,
        limit: usize,
    },
    CausalFlips {
        limit: usize,
    },
    Lineage {
        limit: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
    LineageHistory {
        snapshots: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
    LineageRows {
        rows: usize,
        filters: LineageFilters,
        view: LineageViewOptions,
    },
    TasksList {
        json: bool,
        filters: RuntimeTaskFilter,
    },
    TasksCreate {
        json: bool,
        input: RuntimeTaskCreateRequest,
    },
    TasksUpdateStatus {
        json: bool,
        task_id: String,
        update: RuntimeTaskStatusUpdateRequest,
    },
    OperatorCommands {
        json: bool,
    },
}

const CLI_USAGE: &str =
    "usage: eden us\n       eden causal timeline <leaf_scope_key> [limit]\n       eden causal flips [limit]\n       eden lineage [limit] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--json]\n       eden lineage history [snapshots] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]\n       eden lineage rows [rows] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]\n       eden tasks [--status <value>] [--kind <value>] [--market <value>] [--owner <value>] [--json]\n       eden tasks create <kind> --label <value> [--market <value>] [--owner <value>] [--detail <value>] [--json]\n       eden tasks status <task_id> <status> [--detail <value>] [--error <value>] [--json]\n       eden operator commands [--json]";

#[derive(Debug, Clone, Copy, Default)]
pub struct LineageViewOptions {
    pub top: usize,
    pub latest_only: bool,
    pub json: bool,
    pub sort_by: LineageSortKey,
    pub alignment: LineageAlignmentFilter,
}

pub fn parse_cli_command(args: &[String]) -> Result<CliCommand, String> {
    const DEFAULT_LIMIT: usize = 120;

    if args.len() <= 1 {
        return Ok(CliCommand::Live);
    }

    match args.get(1).map(|value| value.as_str()) {
        Some("us") => Ok(CliCommand::UsLive),
        Some("causal") => match args.get(2).map(|value| value.as_str()) {
            Some("timeline") => {
                let leaf_scope_key = args.get(3).cloned().ok_or_else(|| {
                    "usage: eden causal timeline <leaf_scope_key> [limit]".to_string()
                })?;
                let limit = parse_optional_limit(args.get(4), DEFAULT_LIMIT)?;
                Ok(CliCommand::CausalTimeline {
                    leaf_scope_key,
                    limit,
                })
            }
            Some("flips") => {
                let limit = parse_optional_limit(args.get(3), DEFAULT_LIMIT)?;
                Ok(CliCommand::CausalFlips { limit })
            }
            _ => Err(CLI_USAGE.into()),
        },
        Some("lineage") => parse_lineage_cli_command(&args[2..], DEFAULT_LIMIT),
        Some("tasks") => parse_tasks_cli_command(&args[2..]),
        Some("operator") => parse_operator_cli_command(&args[2..]),
        _ => Err(CLI_USAGE.into()),
    }
}

fn parse_tasks_cli_command(args: &[String]) -> Result<CliCommand, String> {
    match args.first().map(|value| value.as_str()) {
        Some("create") => parse_tasks_create_command(&args[1..]),
        Some("status") => parse_tasks_status_command(&args[1..]),
        Some("list") => parse_tasks_list_command(&args[1..]),
        _ => parse_tasks_list_command(args),
    }
}

fn parse_tasks_list_command(args: &[String]) -> Result<CliCommand, String> {
    let mut json = false;
    let mut filters = RuntimeTaskFilter::default();
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json = true;
                index += 1;
            }
            "--status" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "missing value for --status".to_string())?;
                filters.status = Some(value.parse::<RuntimeTaskStatus>()?);
                index += 2;
            }
            "--kind" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "missing value for --kind".to_string())?;
                filters.kind = Some(value.parse::<RuntimeTaskKind>()?);
                index += 2;
            }
            "--market" => {
                filters.market = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --market".to_string())?
                        .clone(),
                );
                index += 2;
            }
            "--owner" => {
                filters.owner = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --owner".to_string())?
                        .clone(),
                );
                index += 2;
            }
            other => return Err(format!("unknown tasks flag: {other}")),
        }
    }

    Ok(CliCommand::TasksList { json, filters })
}

fn parse_tasks_create_command(args: &[String]) -> Result<CliCommand, String> {
    let kind = args
        .first()
        .ok_or_else(|| {
            "usage: eden tasks create <kind> --label <value> [--market <value>] [--owner <value>] [--detail <value>] [--json]"
                .to_string()
        })?
        .parse::<RuntimeTaskKind>()?;
    let mut json = false;
    let mut label: Option<String> = None;
    let mut market: Option<String> = None;
    let mut owner: Option<String> = None;
    let mut detail: Option<String> = None;
    let mut index = 1usize;

    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json = true;
                index += 1;
            }
            "--label" => {
                label = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --label".to_string())?
                        .clone(),
                );
                index += 2;
            }
            "--market" => {
                market = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --market".to_string())?
                        .clone(),
                );
                index += 2;
            }
            "--owner" => {
                owner = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --owner".to_string())?
                        .clone(),
                );
                index += 2;
            }
            "--detail" => {
                detail = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --detail".to_string())?
                        .clone(),
                );
                index += 2;
            }
            other => return Err(format!("unknown tasks create flag: {other}")),
        }
    }

    let label = label.ok_or_else(|| "missing required --label for tasks create".to_string())?;
    Ok(CliCommand::TasksCreate {
        json,
        input: RuntimeTaskCreateRequest {
            label,
            kind,
            market,
            owner,
            detail,
            metadata: None,
        },
    })
}

fn parse_tasks_status_command(args: &[String]) -> Result<CliCommand, String> {
    let task_id = args.first().cloned().ok_or_else(|| {
        "usage: eden tasks status <task_id> <status> [--detail <value>] [--error <value>] [--json]"
            .to_string()
    })?;
    let status = args
        .get(1)
        .ok_or_else(|| {
            "usage: eden tasks status <task_id> <status> [--detail <value>] [--error <value>] [--json]"
                .to_string()
        })?
        .parse::<RuntimeTaskStatus>()?;
    let mut json = false;
    let mut detail: Option<String> = None;
    let mut error: Option<String> = None;
    let mut index = 2usize;

    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                json = true;
                index += 1;
            }
            "--detail" => {
                detail = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --detail".to_string())?
                        .clone(),
                );
                index += 2;
            }
            "--error" => {
                error = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "missing value for --error".to_string())?
                        .clone(),
                );
                index += 2;
            }
            other => return Err(format!("unknown tasks status flag: {other}")),
        }
    }

    Ok(CliCommand::TasksUpdateStatus {
        json,
        task_id,
        update: RuntimeTaskStatusUpdateRequest {
            status,
            detail,
            error,
            metadata: None,
        },
    })
}

fn parse_operator_cli_command(args: &[String]) -> Result<CliCommand, String> {
    if matches!(
        args.first().map(|value| value.as_str()),
        Some("commands") | None
    ) {
        let rest = if matches!(args.first().map(|value| value.as_str()), Some("commands")) {
            &args[1..]
        } else {
            args
        };
        let mut json = false;
        let mut index = 0usize;
        while index < rest.len() {
            match rest[index].as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                other => return Err(format!("unknown operator flag: {other}")),
            }
        }
        return Ok(CliCommand::OperatorCommands { json });
    }
    Err(CLI_USAGE.into())
}

fn parse_lineage_cli_command(args: &[String], default_limit: usize) -> Result<CliCommand, String> {
    if matches!(args.first().map(|value| value.as_str()), Some("rows")) {
        let (rows, filters, view) = parse_lineage_arguments(&args[1..], default_limit)?;
        return Ok(CliCommand::LineageRows {
            rows,
            filters,
            view,
        });
    }
    if matches!(args.first().map(|value| value.as_str()), Some("history")) {
        let (snapshots, filters, view) = parse_lineage_arguments(&args[1..], default_limit)?;
        return Ok(CliCommand::LineageHistory {
            snapshots,
            filters,
            view,
        });
    }

    let (limit, filters, view) = parse_lineage_arguments(args, default_limit)?;
    if view.latest_only {
        return Err("--latest-only is only valid for `eden lineage history`".into());
    }
    Ok(CliCommand::Lineage {
        limit,
        filters,
        view,
    })
}

fn parse_lineage_arguments(
    args: &[String],
    default_limit: usize,
) -> Result<(usize, LineageFilters, LineageViewOptions), String> {
    let mut index = 0usize;
    let mut limit = default_limit;
    let mut filters = LineageFilters::default();
    let mut view = LineageViewOptions {
        top: 5,
        latest_only: false,
        json: false,
        sort_by: LineageSortKey::NetReturn,
        alignment: LineageAlignmentFilter::All,
    };

    if let Some(value) = args.get(index) {
        if !value.starts_with("--") {
            limit = parse_optional_limit(Some(value), default_limit)?;
            index += 1;
        }
    }

    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--latest-only" => {
                view.latest_only = true;
                index += 1;
                continue;
            }
            "--json" => {
                view.json = true;
                index += 1;
                continue;
            }
            "--label" | "--bucket" | "--family" | "--session" | "--regime" | "--top" | "--sort"
            | "--alignment" => {}
            _ => return Err(format!("unknown lineage flag: {}", flag)),
        }

        let value = args.get(index + 1).ok_or_else(|| match flag {
            "--label" => "missing value for --label".to_string(),
            "--bucket" => "missing value for --bucket".to_string(),
            "--family" => "missing value for --family".to_string(),
            "--session" => "missing value for --session".to_string(),
            "--regime" => "missing value for --regime".to_string(),
            "--top" => "missing value for --top".to_string(),
            "--sort" => "missing value for --sort".to_string(),
            "--alignment" => "missing value for --alignment".to_string(),
            _ => format!("unknown lineage flag: {}", flag),
        })?;

        match flag {
            "--label" => filters.label = Some(value.clone()),
            "--bucket" => filters.bucket = Some(value.clone()),
            "--family" => filters.family = Some(value.clone()),
            "--session" => filters.session = Some(value.clone()),
            "--regime" => filters.market_regime = Some(value.clone()),
            "--top" => {
                view.top = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid top value: {}", value))?;
                if view.top == 0 {
                    return Err("--top must be greater than 0".into());
                }
            }
            "--sort" => {
                view.sort_by = match value.as_str() {
                    "net" | "net_return" => LineageSortKey::NetReturn,
                    "follow" | "follow_expectancy" => LineageSortKey::FollowExpectancy,
                    "fade" | "fade_expectancy" => LineageSortKey::FadeExpectancy,
                    "wait" | "wait_expectancy" => {
                        return Err("wait_expectancy sort is temporarily unsupported because the metric is not yet meaningfully populated".into())
                    }
                    "conv" | "convergence" => LineageSortKey::ConvergenceScore,
                    "external" | "ext" => LineageSortKey::ExternalDelta,
                    _ => return Err(format!("invalid sort value: {}", value)),
                };
            }
            "--alignment" => {
                view.alignment = match value.as_str() {
                    "all" => LineageAlignmentFilter::All,
                    "confirm" => LineageAlignmentFilter::Confirm,
                    "contradict" => LineageAlignmentFilter::Contradict,
                    _ => return Err(format!("invalid alignment value: {}", value)),
                };
            }
            _ => return Err(format!("unknown lineage flag: {}", flag)),
        }
        index += 2;
    }

    Ok((limit, filters, view))
}

fn parse_optional_limit(arg: Option<&String>, default: usize) -> Result<usize, String> {
    match arg {
        None => Ok(default),
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| format!("invalid limit: {}", value))
            .and_then(|limit| {
                if limit == 0 {
                    Err("limit must be greater than 0".into())
                } else {
                    Ok(limit)
                }
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tasks_create_command() {
        let args = vec![
            "eden".to_string(),
            "tasks".to_string(),
            "create".to_string(),
            "operator".to_string(),
            "--label".to_string(),
            "Review alerts".to_string(),
            "--market".to_string(),
            "hk".to_string(),
        ];
        let command = parse_cli_command(&args).expect("parse");
        match command {
            CliCommand::TasksCreate { input, .. } => {
                assert_eq!(input.kind, RuntimeTaskKind::Operator);
                assert_eq!(input.label, "Review alerts");
                assert_eq!(input.market.as_deref(), Some("hk"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_operator_commands_command() {
        let args = vec![
            "eden".to_string(),
            "operator".to_string(),
            "commands".to_string(),
            "--json".to_string(),
        ];
        let command = parse_cli_command(&args).expect("parse");
        match command {
            CliCommand::OperatorCommands { json } => assert!(json),
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
