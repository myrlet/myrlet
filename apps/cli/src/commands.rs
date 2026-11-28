//! Executes parsed commands against the Hub client and renders output.

use std::fs;
use std::io::{self, Write};

use myrlet_client::{Client, ClientError, NativeTransport};
use myrlet_protocol::{
    load_worker_credentials, CancelTaskRequest, FinishTaskRequest, LocalCommand, LocalReply,
    PermissionTier, TaskBundleImportRequest, TaskDefinition, TaskId, UpdateTaskRequest, WorkerId,
};

use crate::args::Command;
use crate::output;
use crate::CliError;

type NativeClient = Client<NativeTransport>;

/// Environment access shared by every command.
pub struct Env<'a> {
    pub hub_url: &'a str,
    pub admin_token: Option<&'a str>,
    pub home: Option<&'a str>,
    pub worker_socket: Option<&'a str>,
}

/// Whether a command is execution-local (Worker control socket only, no Hub credential).
fn local_only(command: &Command) -> Option<&str> {
    match command {
        Command::Done { delivery_id }
        | Command::ResultCheck { delivery_id }
        | Command::ResultSync { delivery_id }
        | Command::ResultStatus { delivery_id } => Some(delivery_id),
        _ => None,
    }
}

/// Execute one command, writing structured output to `stdout`.
pub async fn execute<W: Write>(
    env: &Env<'_>,
    command: Command,
    json: bool,
    stdout: &mut W,
) -> Result<(), CliError> {
    if local_only(&command).is_some() {
        return run_local(env, command, json, stdout);
    }
    let client = authorize(env, &command)?;
    match command {
        Command::Help => Ok(()),
        Command::TasksList { limit } => {
            let page = client.list_tasks(limit).await.map_err(runtime)?;
            render(output::task_list(stdout, &page, json))
        }
        Command::TasksShow { id } => {
            let read = client.get_task(&TaskId::new(id)).await.map_err(runtime)?;
            render(output::task_read(stdout, &read, json))
        }
        Command::TasksTree {
            id,
            direction,
            max_depth,
            limit,
        } => {
            let tree = client
                .get_task_tree(&TaskId::new(id), direction, max_depth, limit)
                .await
                .map_err(runtime)?;
            render(output::task_tree(stdout, &tree, json))
        }
        Command::TasksSubmit {
            file,
            idempotency_key,
            definition,
        } => {
            let definition = match (file, definition) {
                (Some(path), _) => read_json(&path)?,
                (None, Some(definition)) => *definition,
                (None, None) => {
                    return Err(CliError::Usage(
                        "tasks submit requires --file or --title and --prompt".to_string(),
                    ))
                }
            };
            let key = idempotency_key.unwrap_or_else(new_key);
            let record = client
                .create_task(&key, &definition)
                .await
                .map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
        }
        Command::TasksImport {
            file,
            idempotency_key,
        } => {
            let request: TaskBundleImportRequest = read_json(&file)?;
            let key = idempotency_key.unwrap_or_else(new_key);
            let response = client
                .import_bundle(&key, &request)
                .await
                .map_err(runtime)?;
            render(output::bundle_import(stdout, &response, json))
        }
        Command::TasksUpdate {
            id,
            expected_version,
            idempotency_key,
            dependencies,
        } => {
            let task_id = TaskId::new(id);
            let request = UpdateTaskRequest {
                expected_task_version: expected_version,
                update: myrlet_protocol::PendingTaskUpdate {
                    dependencies: dependencies
                        .map(|ids| ids.into_iter().map(TaskId::new).collect()),
                    routing: None,
                },
            };
            let key = idempotency_key.unwrap_or_else(new_key);
            let record = client
                .update_task(&key, &task_id, &request)
                .await
                .map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
        }
        Command::TasksCancel {
            id,
            expected_version,
            idempotency_key,
            reason,
        } => {
            let task_id = TaskId::new(id);
            let expected_task_version =
                current_version(&client, &task_id, expected_version).await?;
            let key = idempotency_key.unwrap_or_else(new_key);
            let request = CancelTaskRequest {
                expected_task_version,
                reason,
            };
            let record = client
                .cancel_task(&key, &task_id, &request)
                .await
                .map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
        }
        Command::TasksFinish {
            id,
            expected_version,
            idempotency_key,
        } => {
            let task_id = TaskId::new(id);
            let expected_task_version =
                current_version(&client, &task_id, expected_version).await?;
            let key = idempotency_key.unwrap_or_else(new_key);
            let request = FinishTaskRequest {
                expected_task_version,
            };
            let record = client
                .finish_task(&key, &task_id, &request)
                .await
                .map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
        }
        Command::WorkersList => {
            let list = client.list_workers().await.map_err(runtime)?;
            render(output::worker_list(stdout, &list, json))
        }
        Command::WorkersShow { id } => {
            let detail = client
                .get_worker(&WorkerId::new(id))
                .await
                .map_err(runtime)?;
            render(output::worker_detail(stdout, &detail, json))
        }
        Command::WorkersEnroll {
            ttl_seconds,
            suggested_worker_id,
        } => {
            let request = myrlet_protocol::CreateEnrollmentCodeRequest {
                ttl_seconds,
                suggested_worker_id: suggested_worker_id.map(WorkerId::new),
            };
            let response = client
                .create_enrollment_code(&request)
                .await
                .map_err(runtime)?;
            render(output::enrollment(stdout, &response, json))
        }
        Command::WorkersRotate { id } => {
            let response = client
                .rotate_worker(&WorkerId::new(id))
                .await
                .map_err(runtime)?;
            render(output::rotate(stdout, &response, json))
        }
        Command::WorkersRevoke { id } => {
            let worker_id = WorkerId::new(id);
            client.revoke_worker(&worker_id).await.map_err(runtime)?;
            render(output::revoked(stdout, &worker_id, json))
        }
        Command::WorkersApprove { id } => {
            let worker_id = WorkerId::new(id);
            client.approve_worker(&worker_id).await.map_err(runtime)?;
            render(output::approved(stdout, &worker_id, json))
        }
        Command::WorkersConfigShow { id } => {
            let detail = client
                .get_worker(&WorkerId::new(id))
                .await
                .map_err(runtime)?;
            render(output::worker_config(stdout, &detail, json))
        }
        Command::WorkersConfigApply { id, revision } => {
            let worker_id = WorkerId::new(id);
            let response = client
                .apply_worker_config(&worker_id, revision)
                .await
                .map_err(runtime)?;
            render(output::config_applied(stdout, &worker_id, &response, json))
        }
        Command::PolicyShow => {
            let settings = client.security_trust().await.map_err(runtime)?;
            render(output::policy(stdout, &settings, json))
        }
        Command::PolicySet {
            auto_approve,
            auto_apply,
        } => {
            let request = myrlet_protocol::UpdateSecurityTrustRequest {
                require_worker_approval: auto_approve.map(|enabled| !enabled),
                require_configuration_apply: auto_apply.map(|enabled| !enabled),
            };
            let stored = client
                .update_security_trust(&request)
                .await
                .map_err(runtime)?;
            render(output::policy(stdout, &stored, json))
        }
        Command::WorkersPermissionSet { id, tier } => {
            let worker_id = WorkerId::new(id);
            let response = client
                .set_worker_permission_tier(&worker_id, tier)
                .await
                .map_err(runtime)?;
            render(output::permission_tier(stdout, &response, json))
        }
        Command::EventsWatch { cursor, limit } => watch(&client, cursor, limit, json, stdout).await,
        Command::Done { .. }
        | Command::ResultCheck { .. }
        | Command::ResultSync { .. }
        | Command::ResultStatus { .. } => unreachable!("local commands are handled above"),
    }
}

/// Select the credential a command presents.
///
/// Admin commands use `MYRLET_ADMIN_TOKEN` and never fall back to a Worker credential.
/// Worker and Owner commands use the Hub-issued Worker credential from `MYRLET_HOME`,
/// unless an Admin token is present, in which case the Admin credential is preferred and
/// gains the full administration surface. The CLI never inspects or selects a permission
/// tier; the Hub decides.
fn authorize(env: &Env<'_>, command: &Command) -> Result<NativeClient, CliError> {
    let bearer = if command.requires_admin() {
        env.admin_token.map(str::to_string).ok_or_else(|| {
            CliError::Usage(format!(
                "{command_label} requires MYRLET_ADMIN_TOKEN (Admin credential)",
                command_label = command.label()
            ))
        })?
    } else if let Some(token) = env.admin_token {
        token.to_string()
    } else {
        worker_credential(env)?
    };
    Client::new(env.hub_url, bearer).map_err(|error| CliError::Runtime(error.to_string()))
}

/// Read the Worker's own Hub-issued credential from `MYRLET_HOME`, failing closed.
fn worker_credential(env: &Env<'_>) -> Result<String, CliError> {
    let home = env.home.ok_or_else(|| {
        CliError::Usage(
            "a Worker or Owner command needs MYRLET_HOME (for worker/credentials.json) or \
             MYRLET_ADMIN_TOKEN"
                .to_string(),
        )
    })?;
    let path = std::path::Path::new(home)
        .join("worker")
        .join("credentials.json");
    load_worker_credentials(&path)
        .map(|credentials| credentials.credential)
        .map_err(|error| CliError::Runtime(error.to_string()))
}

impl Command {
    /// Whether this command is Admin-only (Worker and system administration).
    fn requires_admin(&self) -> bool {
        matches!(
            self,
            Command::WorkersEnroll { .. }
                | Command::WorkersRotate { .. }
                | Command::WorkersRevoke { .. }
                | Command::WorkersApprove { .. }
                | Command::WorkersConfigShow { .. }
                | Command::WorkersConfigApply { .. }
                | Command::WorkersPermissionSet { .. }
                // Security & Trust policy writes are Admin system configuration.
                | Command::PolicySet { .. }
        )
    }

    /// Short command name used in credential error messages.
    fn label(&self) -> &'static str {
        match self {
            Command::TasksList { .. } => "tasks list",
            Command::TasksShow { .. } => "tasks show",
            Command::TasksTree { .. } => "tasks tree",
            Command::TasksSubmit { .. } => "tasks submit",
            Command::TasksImport { .. } => "tasks import",
            Command::TasksUpdate { .. } => "tasks update",
            Command::TasksCancel { .. } => "tasks cancel",
            Command::TasksFinish { .. } => "tasks finish",
            Command::WorkersList => "workers list",
            Command::WorkersShow { .. } => "workers show",
            Command::WorkersEnroll { .. } => "workers enroll",
            Command::WorkersRotate { .. } => "workers rotate",
            Command::WorkersRevoke { .. } => "workers revoke",
            Command::WorkersApprove { .. } => "workers approve",
            Command::WorkersConfigShow { .. } => "workers config show",
            Command::WorkersConfigApply { .. } => "workers config apply",
            Command::WorkersPermissionSet { .. } => "workers permission set",
            Command::PolicyShow => "policy show",
            Command::PolicySet { .. } => "policy set",
            Command::EventsWatch { .. } => "events watch",
            Command::Done { .. } => "done",
            Command::ResultCheck { .. } => "result check",
            Command::ResultSync { .. } => "result sync",
            Command::ResultStatus { .. } => "result status",
            Command::Help => "help",
        }
    }
}

/// Execute one execution-local command through the Worker's private control socket.
fn run_local<W: Write>(
    env: &Env<'_>,
    command: Command,
    json: bool,
    stdout: &mut W,
) -> Result<(), CliError> {
    let socket = env.worker_socket.ok_or_else(|| {
        CliError::Usage(
            "MYRLET_HOME is required for execution-local commands; the Worker control socket is \
             under $MYRLET_HOME/worker"
                .to_string(),
        )
    })?;
    let request = to_local(command);
    let reply = send_local(socket, &request)?;
    render(output::local_reply(stdout, &reply, json))
}

/// Resolve the control-socket path from `MYRLET_HOME`, outside any Hub HTTP call.
pub fn control_socket(home: &str) -> std::path::PathBuf {
    std::path::Path::new(home)
        .join("worker")
        .join("control.sock")
}

fn to_local(command: Command) -> LocalCommand {
    let delivery_id = local_only(&command)
        .expect("only local commands reach the control socket")
        .to_string();
    match command {
        Command::Done { .. } => LocalCommand::Done { delivery_id },
        Command::ResultCheck { .. } => LocalCommand::ResultCheck { delivery_id },
        Command::ResultSync { .. } => LocalCommand::ResultSync { delivery_id },
        Command::ResultStatus { .. } => LocalCommand::ResultStatus { delivery_id },
        _ => unreachable!("only local commands reach the control socket"),
    }
}

fn send_local(socket: &str, request: &LocalCommand) -> Result<LocalReply, CliError> {
    myrlet_protocol::send(std::path::Path::new(socket), request)
        .map_err(|error| CliError::Runtime(error.to_string()))
}

fn new_key() -> String {
    format!("myrlet-{}", uuid::Uuid::new_v4().simple())
}

/// Resolve the Task version a version-fenced command should send.
///
/// A full Task record supplies its version; a Worker reading a directly related Task only
/// receives a limited summary, which also carries the version. Either way the CLI reports
/// what the Hub sent and lets the Hub decide whether the write is authorized.
async fn current_version(
    client: &NativeClient,
    task_id: &TaskId,
    expected_version: Option<u64>,
) -> Result<u64, CliError> {
    match expected_version {
        Some(version) => Ok(version),
        None => {
            let read = client.get_task(task_id).await.map_err(runtime)?;
            if let Some(record) = read.task {
                return Ok(record.version);
            }
            if let Some(summary) = read.summary {
                return Ok(summary.version);
            }
            Err(CliError::Runtime(
                "this Task has no readable record".to_string(),
            ))
        }
    }
}

async fn watch<W: Write>(
    client: &NativeClient,
    cursor: Option<u64>,
    limit: Option<usize>,
    json: bool,
    stdout: &mut W,
) -> Result<(), CliError> {
    let mut snapshot = client.snapshot().await.map_err(runtime)?;
    render(output::snapshot_line(stdout, &snapshot, json))?;
    if limit == Some(0) {
        return Ok(());
    }
    let mut cursor = cursor.unwrap_or(snapshot.event_cursor);
    let mut emitted = 0usize;
    loop {
        let mut stream = match client.open_events(Some(cursor)).await {
            Ok(stream) => stream,
            Err(ClientError::CursorExpired) => {
                snapshot = client.snapshot().await.map_err(runtime)?;
                cursor = snapshot.event_cursor;
                render(output::snapshot_line(stdout, &snapshot, json))?;
                continue;
            }
            Err(error) => return Err(runtime(error)),
        };
        let mut expired = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => {
                    render(output::event_line(stdout, &event, json))?;
                    emitted += 1;
                    if limit.is_some_and(|max| emitted >= max) {
                        return Ok(());
                    }
                }
                Err(ClientError::CursorExpired) => {
                    expired = true;
                    break;
                }
                Err(error) => return Err(runtime(error)),
            }
        }
        if !expired {
            return Ok(());
        }
        snapshot = client.snapshot().await.map_err(runtime)?;
        cursor = snapshot.event_cursor;
        render(output::snapshot_line(stdout, &snapshot, json))?;
    }
}

fn runtime(error: ClientError) -> CliError {
    CliError::Runtime(error.to_string())
}

fn render(result: io::Result<()>) -> Result<(), CliError> {
    result.map_err(|error| CliError::Runtime(error.to_string()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, CliError> {
    let text = fs::read_to_string(path)
        .map_err(|error| CliError::Runtime(format!("failed to read {path}: {error}")))?;
    serde_json::from_str(&text)
        .map_err(|error| CliError::Runtime(format!("failed to decode {path}: {error}")))
}

/// Read a `TaskDefinition` file without going through a Hub call.
pub fn read_definition(path: &str) -> Result<TaskDefinition, CliError> {
    read_json(path)
}

/// The permission tier label a CLI argument names.
pub fn parse_permission_tier(raw: &str) -> Result<PermissionTier, CliError> {
    PermissionTier::parse(raw)
        .ok_or_else(|| CliError::Usage(format!("--tier must be worker or owner, got {raw:?}")))
}
