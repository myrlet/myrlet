//! Task commands: creation, cancellation, and coordination-only finish.

use rusqlite::Connection;

use myrlet_protocol::{
    CancelTaskRequest, FinishTaskRequest, TaskDefinition, TaskId, TaskRecord, TaskStatus,
};

use crate::deliveries;
use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::{check_idempotency, digest_request, record_idempotency};
use crate::tasks::{
    build_record, force_task_state, generate_task_id, insert_task, load_task, status_str,
    transition_task, EVENT_TASK_CANCELLED, EVENT_TASK_CREATED, EVENT_TASK_FINISHED, MAX_TREE_DEPTH,
};
use crate::{CancellationOutcome, TaskMutation};

const OP_CREATE: &str = "tasks.create";
const OP_CANCEL: &str = "tasks.cancel";
const OP_FINISH: &str = "tasks.finish";

/// Create one `held` Task with parent/depth and sibling dependency validation.
pub fn create_task(
    conn: &mut Connection,
    idempotency_key: &str,
    def: &TaskDefinition,
    now_ms: i64,
) -> Result<TaskMutation, StoreError> {
    def.validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let digest = digest_request(def)?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_CREATE, idempotency_key, &digest)? {
        let record: TaskRecord = serde_json::from_str(&hit.response_json)?;
        return Ok((record, None, true));
    }
    let id = generate_task_id();
    let (root_task_id, depth) = match &def.parent_task_id {
        Some(parent_id) => {
            let parent = load_task(&tx, parent_id)?;
            if parent.depth >= MAX_TREE_DEPTH {
                return Err(StoreError::invalid("task tree cannot exceed 5 levels"));
            }
            (parent.root_task_id, parent.depth + 1)
        }
        None => (id.clone(), 1),
    };
    for dependency in &def.dependencies {
        let dep = load_task(&tx, dependency)?;
        if dep.parent_task_id != def.parent_task_id {
            return Err(StoreError::invalid(
                "dependencies must share the same parent task",
            ));
        }
    }
    insert_task(&tx, &id, def, &root_task_id, depth, now_ms)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_CREATED,
        &serde_json::json!({
            "taskId": id.as_str(),
            "parentTaskId": def.parent_task_id.as_ref().map(TaskId::as_str),
            "rootTaskId": root_task_id.as_str(),
            "status": status_str(TaskStatus::Held),
            "version": 1,
        }),
        now_ms,
    )?;
    let record = build_record(&tx, &load_task(&tx, &id)?)?;
    record_idempotency(
        &tx,
        OP_CREATE,
        idempotency_key,
        &digest,
        201,
        &serde_json::to_string(&record)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((record, Some(event), false))
}

/// Cancel any non-terminal Task and hand back the Delivery that needs `task.cancel`.
pub fn cancel_task(
    conn: &mut Connection,
    idempotency_key: &str,
    task_id: &TaskId,
    request: &CancelTaskRequest,
    now_ms: i64,
) -> Result<CancellationOutcome, StoreError> {
    let digest = digest_request(&serde_json::json!({
        "taskId": task_id.as_str(),
        "request": request,
    }))?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_CANCEL, idempotency_key, &digest)? {
        let stored: serde_json::Value = serde_json::from_str(&hit.response_json)?;
        let record: TaskRecord = serde_json::from_value(stored["task"].clone())?;
        let delivery = serde_json::from_value(stored["delivery"].clone())?;
        return Ok((record, delivery, None, true));
    }
    let row = load_task(&tx, task_id)?;
    if row.version != request.expected_task_version {
        return Err(StoreError::VersionConflict);
    }
    if row.status.is_terminal() {
        return Err(StoreError::InvalidTransition);
    }
    let version = transition_task(&tx, &row, TaskStatus::Cancelled, now_ms)?;
    let delivery =
        deliveries::cancel_active_delivery(&tx, task_id, request.reason.as_deref(), now_ms)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_CANCELLED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "status": status_str(TaskStatus::Cancelled),
            "version": version,
            "reason": request.reason,
        }),
        now_ms,
    )?;
    let record = build_record(&tx, &load_task(&tx, task_id)?)?;
    let stored = serde_json::json!({ "task": record.clone(), "delivery": delivery.clone() });
    record_idempotency(
        &tx,
        OP_CANCEL,
        idempotency_key,
        &digest,
        200,
        &stored.to_string(),
        now_ms,
    )?;
    tx.commit()?;
    Ok((record, delivery, Some(event), false))
}

/// Finish a coordination-only `held` Parent that never had a Delivery.
pub fn finish_coordination_task(
    conn: &mut Connection,
    idempotency_key: &str,
    task_id: &TaskId,
    request: &FinishTaskRequest,
    now_ms: i64,
) -> Result<TaskMutation, StoreError> {
    let digest = digest_request(&serde_json::json!({
        "taskId": task_id.as_str(),
        "request": request,
    }))?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_FINISH, idempotency_key, &digest)? {
        let record: TaskRecord = serde_json::from_str(&hit.response_json)?;
        return Ok((record, None, true));
    }
    let row = load_task(&tx, task_id)?;
    if row.version != request.expected_task_version {
        return Err(StoreError::VersionConflict);
    }
    if row.status != TaskStatus::Held || deliveries::task_has_delivery(&tx, task_id)? {
        return Err(StoreError::InvalidTransition);
    }
    let version = force_task_state(&tx, &row, TaskStatus::Finished, now_ms)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_FINISHED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "status": status_str(TaskStatus::Finished),
            "version": version,
            "coordination": true,
        }),
        now_ms,
    )?;
    let record = build_record(&tx, &load_task(&tx, task_id)?)?;
    record_idempotency(
        &tx,
        OP_FINISH,
        idempotency_key,
        &digest,
        200,
        &serde_json::to_string(&record)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((record, Some(event), false))
}
