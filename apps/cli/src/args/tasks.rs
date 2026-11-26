//! Parser for the `tasks` subcommands.

use myrlet_protocol::{
    DispatchSelection, ExecutorId, SlotId, TaskDefinition, TaskExecution, TaskId, TaskRouting,
    TaskTreeDirection, WorkerId,
};

use crate::CliError;

use super::{usage, Command, CreateSource, RawArgs, Scanner};

pub(super) fn parse(scanner: &mut Scanner) -> Result<Command, CliError> {
    match scanner.take_token().as_deref() {
        Some("list") => {
            RawArgs::parse(scanner)?.finish("tasks list")?;
            Ok(Command::TasksList)
        }
        Some("show") => {
            let mut raw = RawArgs::parse(scanner)?;
            let id = raw.positional("tasks show <id>")?;
            raw.finish("tasks show")?;
            Ok(Command::TasksShow { id })
        }
        Some("tree") => parse_tree(scanner),
        Some("create") => parse_create(scanner),
        Some("dispatch") => parse_dispatch(scanner),
        Some("submit") => parse_submit(scanner),
        Some("bundle-import") => parse_bundle(scanner),
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

fn parse_create(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let idempotency_key = raw.required("--idempotency-key")?;
    let file = raw.single("--file");
    let title = raw.single("--title");
    let prompt = raw.single("--prompt");
    let source = match (file, title, prompt) {
        (Some(file), None, None) => CreateSource::File(file),
        (None, Some(title), Some(prompt)) => CreateSource::Definition(Box::new(TaskDefinition {
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
                preferred_executor_id: raw.single("--preferred-executor-id").map(ExecutorId::new),
            },
            execution: TaskExecution {
                preferred_model: raw.single("--preferred-model"),
                timeout_seconds: raw.number::<u64>("--timeout-seconds")?,
                parameters: serde_json::Map::new(),
            },
            metadata: serde_json::Map::new(),
        })),
        (Some(_), ..) => return Err(usage("--file cannot be combined with --title/--prompt")),
        (None, None, None) => {
            return Err(usage(
                "tasks create requires --file or --title and --prompt",
            ))
        }
        _ => return Err(usage("tasks create requires both --title and --prompt")),
    };
    raw.finish("tasks create")?;
    Ok(Command::TasksCreate {
        idempotency_key,
        source,
    })
}

fn parse_dispatch(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let id = raw.positional("tasks dispatch <id>")?;
    let expected_version = raw
        .number::<u64>("--expected-version")?
        .ok_or_else(|| usage("tasks dispatch requires --expected-version"))?;
    let idempotency_key = raw.required("--idempotency-key")?;
    let queue = raw.boolean("--queue");
    let fallback_allowed = raw.boolean("--fallback-allowed");
    let worker_id = raw.single("--worker-id");
    let slot_id = raw.single("--slot-id");
    let executor_id = raw.single("--executor-id");
    let _ = raw.number::<u64>("--connection-epoch")?;
    let selection = if raw.boolean("--match") {
        if worker_id.is_some() || slot_id.is_some() {
            return Err(usage(
                "--match cannot be combined with --worker-id/--slot-id",
            ));
        }
        DispatchSelection::Match { fallback_allowed }
    } else {
        if fallback_allowed {
            return Err(usage("--fallback-allowed requires --match"));
        }
        let worker_id = worker_id.ok_or_else(|| usage("targeted dispatch requires --worker-id"))?;
        let slot_id = slot_id.ok_or_else(|| usage("targeted dispatch requires --slot-id"))?;
        DispatchSelection::Targeted {
            worker_id: WorkerId::new(worker_id),
            slot_id: SlotId::new(slot_id),
            executor_id: executor_id.map(ExecutorId::new),
        }
    };
    raw.finish("tasks dispatch")?;
    Ok(Command::TasksDispatch {
        id,
        expected_version,
        idempotency_key,
        selection,
        queue,
    })
}

fn parse_submit(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let file = raw.required("--file")?;
    let idempotency_key = raw.required("--idempotency-key")?;
    let dispatch_idempotency_key = raw.single("--dispatch-idempotency-key");
    let fallback_allowed = raw.boolean("--fallback-allowed");
    let queue = raw.boolean("--queue");
    raw.finish("tasks submit")?;
    Ok(Command::TasksSubmit {
        file,
        idempotency_key,
        dispatch_idempotency_key,
        fallback_allowed,
        queue,
    })
}

fn parse_bundle(scanner: &mut Scanner) -> Result<Command, CliError> {
    let mut raw = RawArgs::parse(scanner)?;
    let file = raw.required("--file")?;
    let idempotency_key = raw.required("--idempotency-key")?;
    raw.finish("tasks bundle-import")?;
    Ok(Command::TasksBundleImport {
        file,
        idempotency_key,
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
