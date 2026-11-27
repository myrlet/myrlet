//! Executes parsed commands against the Hub client and renders output.

use std::fs;
use std::io::{self, Write};

use myrlet_client::{Client, ClientError, NativeTransport};
use myrlet_protocol::{
    CancelTaskRequest, FinishTaskRequest, LocalCommand, LocalReply, TaskBundleImportRequest,
    TaskDefinition, TaskId, UpdateTaskRequest,
};

use crate::args::Command;
use crate::output;
use crate::CliError;

type NativeClient = Client<NativeTransport>;

/// Environment access shared by every command.
pub struct Env<'a> {
    pub hub_url: &'a str,
    pub admin_token: Option<&'a str>,
    pub worker_socket: Option<&'a str>,
}

/// The credential a command needs: execution-local commands use only the Worker socket.
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
    let token = env
        .admin_token
        .ok_or_else(|| CliError::Usage("MYRLET_ADMIN_TOKEN is not set".to_string()))?;
    let client =
        Client::new(env.hub_url, token).map_err(|error| CliError::Runtime(error.to_string()))?;
    match command {
        Command::Help => Ok(()),
        Command::TasksList { limit } => {
            let page = client.list_tasks(limit).await.map_err(runtime)?;
            render(output::task_list(stdout, &page, json))
        }
        Command::TasksShow { id } => {
            let record = client.get_task(&TaskId::new(id)).await.map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
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
                .get_worker(&myrlet_protocol::WorkerId::new(id))
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
                suggested_worker_id: suggested_worker_id.map(myrlet_protocol::WorkerId::new),
            };
            let response = client
                .create_enrollment_code(&request)
                .await
                .map_err(runtime)?;
            render(output::enrollment(stdout, &response, json))
        }
        Command::WorkersRotate { id } => {
            let response = client
                .rotate_worker(&myrlet_protocol::WorkerId::new(id))
                .await
                .map_err(runtime)?;
            render(output::rotate(stdout, &response, json))
        }
        Command::WorkersRevoke { id } => {
            let worker_id = myrlet_protocol::WorkerId::new(id);
            client.revoke_worker(&worker_id).await.map_err(runtime)?;
            render(output::revoked(stdout, &worker_id, json))
        }
        Command::EventsWatch { cursor, limit } => watch(&client, cursor, limit, json, stdout).await,
        Command::Done { .. }
        | Command::ResultCheck { .. }
        | Command::ResultSync { .. }
        | Command::ResultStatus { .. } => unreachable!("local commands are handled above"),
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

async fn current_version(
    client: &NativeClient,
    task_id: &TaskId,
    expected_version: Option<u64>,
) -> Result<u64, CliError> {
    match expected_version {
        Some(version) => Ok(version),
        None => Ok(client.get_task(task_id).await.map_err(runtime)?.version),
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
