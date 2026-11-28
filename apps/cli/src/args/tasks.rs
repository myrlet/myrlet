//! Parser for the `tasks` subcommands.

use myrlet_protocol::{
    ExecutorId, TaskDefinition, TaskExecution, TaskId, TaskRouting, TaskTreeDirection, WorkerId,
};

use crate::CliError;

use super::{usage, Command, RawArgs, Scanner};

pub(super) fn parse(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("list") => {
            let mut raw = RawArgs::parse(scanner)?;
            let limit = raw.number::<usize>("--limit")?;
            raw.finish("tasks list")?;
            Ok(Command::TasksList { limit })
        }
        Some("show") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("tasks show <id>")?;
            raw.finish("tasks show")?;
            Ok(Command::TasksShow { id })
        }
        Some("tree") => parse_tree(scanner),
        Some("submit") => parse_submit(scanner),
        Some("import") => parse_import(scanner),
        Some("update") => parse_update(scanner),
        Some("cancel") => parse_cancel(scanner),
        Some("finish") => parse_finish(scanner),
        Some(other) => Err(usage(format!("unknown tasks command {other:?}"))),
        None => Err(usage("tasks requires a command")),
    }
}

fn parse_tree(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let id = raw.positional("tasks tree <id>")?;
    let direction = match raw.single("--direction").as_deref() {
        None | Some("descendants") => TaskTreeDirection::Descendants,
        Some("ancestors") => TaskTreeDirection::Ancestors,
        Some(other) => return Err(usage(format!("invalid --direction {other:?}"))),
    };
    let max_depth = raw.number::<u8>("--max-depth")?;
    let limit = raw.number::<usize>("--limit")?;
    raw.finish("tasks tree")?;
    Ok(Command::TasksTree {
        id,
        direction,
        max_depth,
        limit,
    })
}

/// `tasks submit --file <def.json>` or inline `--title/--prompt` flags.
fn parse_submit(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let idempotency_key = raw.single("--idempotency-key");
    let file = raw.single("--file");
    let title = raw.single("--title");
    let prompt = raw.single("--prompt");
    let (file, definition) = match (file, title, prompt) {
        (Some(file), None, None) => (Some(file), None),
        (None, Some(title), Some(prompt)) => (
            None,
            Some(Box::new(TaskDefinition {
                client_ref: None,
                parent_task_id: raw.single("--parent-task-id").map(TaskId::new),
                source_task_id: raw.single("--source-task-id").map(TaskId::new),
                dependencies: raw
                    .list("--depends-on")
                    .into_iter()
                    .map(TaskId::new)
                    .collect(),
                category: raw
                    .single("--category")
                    .unwrap_or_else(|| "general".to_string()),
                tags: raw.list("--tag"),
                title,
                prompt,
                input: None,
                expected_output: None,
                external_references: Vec::new(),
                routing: TaskRouting {
                    required_capabilities: raw.list("--capability"),
                    preferred_worker_id: raw.single("--preferred-worker-id").map(WorkerId::new),
                    preferred_role: raw.single("--preferred-role"),
                    preferred_executor_id: raw
                        .single("--preferred-executor-id")
                        .map(ExecutorId::new),
                },
                execution: TaskExecution {
                    preferred_model: raw.single("--preferred-model"),
                    timeout_seconds: raw.number::<u64>("--timeout-seconds")?,
                    parameters: serde_json::Map::new(),
                },
                metadata: serde_json::Map::new(),
            })),
        ),
        (Some(_), ..) => return Err(usage("--file cannot be combined with --title/--prompt")),
        (None, None, None) => {
            return Err(usage(
                "tasks submit requires --file or --title and --prompt",
            ))
        }
        _ => return Err(usage("tasks submit requires both --title and --prompt")),
    };
    raw.finish("tasks submit")?;
    Ok(Command::TasksSubmit {
        file,
        idempotency_key,
        definition,
    })
}

fn parse_import(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let file = raw.required("--file")?;
    let idempotency_key = raw.single("--idempotency-key");
    raw.finish("tasks import")?;
    Ok(Command::TasksImport {
        file,
        idempotency_key,
    })
}

fn parse_update(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let id = raw.positional("tasks update <id>")?;
    let expected_version = raw
        .number::<u64>("--expected-version")?
        .ok_or_else(|| usage("tasks update requires --expected-version"))?;
    let idempotency_key = raw.single("--idempotency-key");
    let dependencies = if raw.values.contains_key("--depends-on") {
        Some(raw.list("--depends-on"))
    } else {
        None
    };
    raw.finish("tasks update")?;
    Ok(Command::TasksUpdate {
        id,
        expected_version,
        idempotency_key,
        dependencies,
    })
}

fn parse_cancel(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let id = raw.positional("tasks cancel <id>")?;
    let expected_version = raw.number::<u64>("--expected-version")?;
    let idempotency_key = raw.single("--idempotency-key");
    let reason = raw.single("--reason");
    raw.finish("tasks cancel")?;
    Ok(Command::TasksCancel {
        id,
        expected_version,
        idempotency_key,
        reason,
    })
}

fn parse_finish(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let id = raw.positional("tasks finish <id>")?;
    let expected_version = raw.number::<u64>("--expected-version")?;
    let idempotency_key = raw.single("--idempotency-key");
    raw.finish("tasks finish")?;
    Ok(Command::TasksFinish {
        id,
        expected_version,
        idempotency_key,
    })
}
