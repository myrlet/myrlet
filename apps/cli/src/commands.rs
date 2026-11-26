//! Executes parsed commands against the Hub client and renders output.

use std::fs;
use std::io::{self, Write};

use myrlet_client::{Client, ClientError, NativeTransport};
use myrlet_protocol::{
    CancelTaskRequest, CreateEnrollmentCodeRequest, DispatchSelection, DispatchTaskRequest,
    FinishTaskRequest, TaskBundleImportRequest, TaskDefinition, TaskId, WorkerId,
};

use crate::args::{Command, CreateSource};
use crate::output;
use crate::CliError;

type NativeClient = Client<NativeTransport>;

/// Execute one command, writing structured output to `stdout`.
pub async fn execute<W: Write>(
    client: &NativeClient,
    command: Command,
    json: bool,
    stdout: &mut W,
) -> Result<(), CliError> {
    match command {
        Command::Help => Ok(()),
        Command::TasksList => {
            let snapshot = client.snapshot().await.map_err(runtime)?;
            render(output::tasks_list(stdout, &snapshot, json))
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
        Command::TasksCreate {
            idempotency_key,
            source,
        } => {
            let definition = match source {
                CreateSource::File(path) => read_json(&path)?,
                CreateSource::Definition(definition) => *definition,
            };
            let record = client
                .create_task(&idempotency_key, &definition)
                .await
                .map_err(runtime)?;
            render(output::task_record(stdout, &record, json))
        }
        Command::TasksDispatch {
            id,
            expected_version,
            idempotency_key,
            selection,
            queue,
        } => {
            let request = DispatchTaskRequest {
                expected_task_version: expected_version,
                selection,
                queue,
            };
            let response = client
                .dispatch_task(&idempotency_key, &TaskId::new(id), &request)
                .await
                .map_err(runtime)?;
            render(output::dispatch(stdout, &response, json))
        }
        Command::TasksSubmit {
            file,
            idempotency_key,
            dispatch_idempotency_key,
            fallback_allowed,
            queue,
        } => {
            let definition: TaskDefinition = read_json(&file)?;
            let created = client
                .create_task(&idempotency_key, &definition)
                .await
                .map_err(runtime)?;
            let dispatch_key =
                dispatch_idempotency_key.unwrap_or_else(|| format!("{idempotency_key}-dispatch"));
            let request = DispatchTaskRequest {
                expected_task_version: created.version,
                selection: DispatchSelection::Match { fallback_allowed },
                queue,
            };
            let response = client
                .dispatch_task(&dispatch_key, &created.id, &request)
                .await
                .map_err(runtime)?;
            render(output::dispatch(stdout, &response, json))
        }
        Command::TasksBundleImport {
            file,
            idempotency_key,
        } => {
            let request: TaskBundleImportRequest = read_json(&file)?;
            let response = client
                .import_bundle(&idempotency_key, &request)
                .await
                .map_err(runtime)?;
            render(output::bundle_import(stdout, &response, json))
        }
        Command::TasksCancel {
            id,
            expected_version,
            idempotency_key,
            reason,
        } => {
            let task_id = TaskId::new(id);
            let expected_task_version = current_version(client, &task_id, expected_version).await?;
            let key = idempotency_key
                .unwrap_or_else(|| format!("cancel-{}", uuid::Uuid::new_v4().simple()));
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
            let expected_task_version = current_version(client, &task_id, expected_version).await?;
            let key = idempotency_key
                .unwrap_or_else(|| format!("finish-{}", uuid::Uuid::new_v4().simple()));
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
            let worker = client
                .get_worker(&WorkerId::new(id))
                .await
                .map_err(runtime)?;
            render(output::worker(stdout, &worker, json))
        }
        Command::WorkersEnroll {
            ttl_seconds,
            suggested_worker_id,
        } => {
            let request = CreateEnrollmentCodeRequest {
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
        Command::EventsWatch { cursor, limit } => watch(client, cursor, limit, json, stdout).await,
    }
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
