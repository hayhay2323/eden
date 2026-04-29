use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatorCommandCategory {
    Discovery,
    RuntimeTask,
    Query,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperatorCommandAvailability {
    Always,
    PersistenceOnly,
}

impl OperatorCommandAvailability {
    pub fn is_enabled(self) -> bool {
        match self {
            OperatorCommandAvailability::Always => true,
            OperatorCommandAvailability::PersistenceOnly => cfg!(feature = "persistence"),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperatorCommandDescriptor {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub category: OperatorCommandCategory,
    pub availability: OperatorCommandAvailability,
    pub supports_json: bool,
}

pub fn builtin_operator_commands() -> Vec<OperatorCommandDescriptor> {
    vec![
        OperatorCommandDescriptor {
            name: "operator.commands",
            summary: "List the built-in operator-facing command registry",
            usage: "eden operator commands [--json]",
            category: OperatorCommandCategory::Discovery,
            availability: OperatorCommandAvailability::Always,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "tasks.list",
            summary: "List runtime tasks from the shared registry",
            usage: "eden tasks [--status <status>] [--kind <kind>] [--market <market>] [--owner <owner>] [--json]",
            category: OperatorCommandCategory::RuntimeTask,
            availability: OperatorCommandAvailability::Always,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "tasks.create",
            summary: "Create a runtime task entry for operator or workflow work",
            usage: "eden tasks create <kind> --label <value> [--market <market>] [--owner <owner>] [--detail <value>] [--json]",
            category: OperatorCommandCategory::RuntimeTask,
            availability: OperatorCommandAvailability::Always,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "tasks.status",
            summary: "Update the status of a runtime task entry",
            usage: "eden tasks status <task_id> <status> [--detail <value>] [--error <value>] [--json]",
            category: OperatorCommandCategory::RuntimeTask,
            availability: OperatorCommandAvailability::Always,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "causal.timeline",
            summary: "Inspect a recent causal timeline for a scope key",
            usage: "eden causal timeline <leaf_scope_key> [limit]",
            category: OperatorCommandCategory::Query,
            availability: OperatorCommandAvailability::PersistenceOnly,
            supports_json: false,
        },
        OperatorCommandDescriptor {
            name: "causal.flips",
            summary: "Inspect recent causal flip events",
            usage: "eden causal flips [limit]",
            category: OperatorCommandCategory::Query,
            availability: OperatorCommandAvailability::PersistenceOnly,
            supports_json: false,
        },
        OperatorCommandDescriptor {
            name: "lineage",
            summary: "Summarize recent lineage evaluation",
            usage: "eden lineage [limit] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--json]",
            category: OperatorCommandCategory::Query,
            availability: OperatorCommandAvailability::PersistenceOnly,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "lineage.history",
            summary: "Inspect lineage history snapshots",
            usage: "eden lineage history [snapshots] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]",
            category: OperatorCommandCategory::Query,
            availability: OperatorCommandAvailability::PersistenceOnly,
            supports_json: true,
        },
        OperatorCommandDescriptor {
            name: "lineage.rows",
            summary: "Inspect raw ranked lineage rows",
            usage: "eden lineage rows [rows] [--label <value>] [--bucket <value>] [--family <value>] [--session <value>] [--regime <value>] [--top <n>] [--sort net|conv|external] [--alignment all|confirm|contradict] [--latest-only] [--json]",
            category: OperatorCommandCategory::Query,
            availability: OperatorCommandAvailability::PersistenceOnly,
            supports_json: true,
        },
    ]
}

pub fn available_operator_commands() -> Vec<OperatorCommandDescriptor> {
    builtin_operator_commands()
        .into_iter()
        .filter(|command| command.availability.is_enabled())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_registry_keeps_only_enabled_commands() {
        let commands = available_operator_commands();
        assert!(commands.iter().any(|command| command.name == "tasks.list"));
        if !cfg!(feature = "persistence") {
            assert!(!commands.iter().any(|command| command.name == "lineage"));
        }
    }
}
