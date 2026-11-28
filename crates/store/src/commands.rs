//! Task commands: submission, version-fenced pending edits, cancellation, and
//! coordination-only finish.

use rusqlite::{params, Connection, Transaction};

use myrlet_protocol::{
    CancelTaskRequest, FinishTaskRequest, HubEventEnvelope, SubmitTaskResponse, TaskDefinition,
    TaskId, TaskRecord, TaskStatus, UpdateTaskRequest,
};

use crate::deliveries;
use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::{check_idempotency, digest_request, record_idempotency};
use crate::tasks::{
    are_dependencies_satisfied, build_record, force_task_state, generate_task_id, insert_task,
    load_task, recompute_readiness, status_str, transition_task, validate_definition,
    EVENT_TASK_CANCELLED, EVENT_TASK_CREATED, EVENT_TASK_FINISHED, EVENT_TASK_QUEUED,
    EVENT_TASK_UPDATED, MAX_TREE_DEPTH,
};
use crate::{CancellationOutcome, TaskMutation};

const OP_CREATE: &str = "tasks.submit";
const OP_UPDATE: &str = "tasks.update";
const OP_CANCEL: &str = "tasks.cancel";
const OP_FINISH: &str = "tasks.finish";

/// Submit one Task: validate, resolve its tree position, and persist it as `queued`
/// when every prerequisite is already `finished`, otherwise as `held`.
pub fn create_task(
    conn: &mut Connection,
    idempotency_key: &str,
    def: &TaskDefinition,
    now_ms: i64,
) -> Result<TaskMutation, StoreError> {
    validate_definition(def)?;
    let digest = digest_request(def)?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_CREATE, idempotency_key, &digest)? {
        let response: SubmitTaskResponse = serde_json::from_str(&hit.response_json)?;
        return Ok((response.task, None, true));
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
    insert_task(
        &tx,
        &id,
        def,
        &root_task_id,
        depth,
        TaskStatus::Held,
        now_ms,
    )?;
    // Submission itself authorizes execution: the new rows decide `held` vs `queued`.
    let row = recompute_readiness(&tx, &load_task(&tx, &id)?, now_ms)?;
    let mut event = insert_event(
        &tx,
        EVENT_TASK_CREATED,
        &serde_json::json!({
            "taskId": id.as_str(),
            "parentTaskId": def.parent_task_id.as_ref().map(TaskId::as_str),
            "rootTaskId": root_task_id.as_str(),
            "status": status_str(row.status),
            "version": 1,
        }),
        now_ms,
    )?;
    if row.status == TaskStatus::Queued {
        event = queue_event(&tx, &id, row.version, now_ms)?;
    }
    let record = build_record(&tx, &load_task(&tx, &id)?)?;
    let response = SubmitTaskResponse {
        task: record,
        eligible: row.status == TaskStatus::Queued,
    };
    record_idempotency(
        &tx,
        OP_CREATE,
        idempotency_key,
        &digest,
        201,
        &serde_json::to_string(&response)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((response.task, Some(event), false))
}

/// Apply version-fenced dependency/routing edits to a Task that has no Delivery yet.
pub fn update_task(
    conn: &mut Connection,
    idempotency_key: &str,
    task_id: &TaskId,
    request: &UpdateTaskRequest,
    now_ms: i64,
) -> Result<TaskMutation, StoreError> {
    let digest = digest_request(&serde_json::json!({
        "taskId": task_id.as_str(),
        "request": request,
    }))?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_UPDATE, idempotency_key, &digest)? {
        let record: TaskRecord = serde_json::from_str(&hit.response_json)?;
        return Ok((record, None, true));
    }
    let row = load_task(&tx, task_id)?;
    if row.version != request.expected_task_version {
        return Err(StoreError::VersionConflict);
    }
    if row.status.is_terminal() || row.status == TaskStatus::Dispatched {
        return Err(StoreError::InvalidTransition);
    }
    if deliveries::task_has_delivery(&tx, task_id)? {
        // A claim won the race; the Owner sees that the Task is already claimed.
        return Err(StoreError::InvalidTransition);
    }
    let mut def = row.definition()?;
    if let Some(dependencies) = &request.update.dependencies {
        connect_dependencies(&tx, task_id, &def.parent_task_id, dependencies)?;
        def.dependencies = dependencies.clone();
    }
    if let Some(routing) = &request.update.routing {
        def.routing = routing.clone();
    }
    validate_definition(&def)?;
    tx.execute(
        "UPDATE tasks SET definition_json = ?1, version = version + 1, updated_at_ms = ?2 \
         WHERE id = ?3",
        params![serde_json::to_string(&def)?, now_ms, task_id.as_str()],
    )?;
    let row = recompute_readiness(&tx, &load_task(&tx, task_id)?, now_ms)?;
    let mut event = insert_event(
        &tx,
        EVENT_TASK_UPDATED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "status": status_str(row.status),
            "version": row.version,
        }),
        now_ms,
    )?;
    if row.status == TaskStatus::Queued && !request.update.dependencies.is_none() {
        event = queue_event(&tx, task_id, row.version, now_ms)?;
    }
    let record = build_record(&tx, &row)?;
    record_idempotency(
        &tx,
        OP_UPDATE,
        idempotency_key,
        &digest,
        200,
        &serde_json::to_string(&record)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((record, Some(event), false))
}

/// Replace dependency rows after sibling, self, and cycle checks, then write them.
fn connect_dependencies(
    tx: &Connection,
    task_id: &TaskId,
    parent_task_id: &Option<TaskId>,
    dependencies: &[TaskId],
) -> Result<(), StoreError> {
    for dependency in dependencies {
        if dependency == task_id {
            return Err(StoreError::invalid("a task cannot depend on itself"));
        }
        let dep = load_task(tx, dependency)?;
        if &dep.parent_task_id != parent_task_id {
            return Err(StoreError::invalid(
                "dependencies must share the same parent task",
            ));
        }
        if depends_on(tx, dependency, task_id)? {
            return Err(StoreError::invalid("dependencies must not form a cycle"));
        }
    }
    tx.execute(
        "DELETE FROM task_deps WHERE task_id = ?1",
        params![task_id.as_str()],
    )?;
    for dependency in dependencies {
        tx.execute(
            "INSERT INTO task_deps (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![task_id.as_str(), dependency.as_str()],
        )?;
    }
    Ok(())
}

/// Whether `from` transitively depends on `target`.
fn depends_on(conn: &Connection, from: &TaskId, target: &TaskId) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "WITH RECURSIVE reach(id) AS ( \
             SELECT ?1 \
             UNION \
             SELECT d.depends_on_task_id FROM task_deps d JOIN reach r ON d.task_id = r.id \
         ) \
         SELECT COUNT(*) FROM reach WHERE id = ?2",
        params![from.as_str(), target.as_str()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Close a coordination-only Task without routing through a generic transition.
fn force_finished(
    tx: &Transaction<'_>,
    row: &crate::tasks::TaskRow,
    now_ms: i64,
) -> Result<u64, StoreError> {
    force_task_state(tx, row, TaskStatus::Finished, now_ms)
}

/// Cancel any non-terminal Task and hand back the Delivery that needs `task.cancel`.
///
/// Dependents are never touched: a `cancelled` prerequisite keeps them `held`.
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

/// Finish a coordination-only Task that never had a Delivery, then promote its siblings.
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
    if row.status.is_terminal() || deliveries::task_has_delivery(&tx, task_id)? {
        return Err(StoreError::InvalidTransition);
    }
    // A coordination-only Task may still be waiting (`held`) or already authorized
    // (`queued`); either way it never had a Delivery, so the Owner closes it directly.
    let version = force_finished(&tx, &row, now_ms)?;
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
    promote_siblings(&tx, &row, now_ms)?;
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

/// Promote every waiting sibling of `row` whose dependencies all finished.
///
/// Only a `finished` Task releases its dependents; a `cancelled` prerequisite keeps them
/// `held` until the Owner edits dependencies or cancels them explicitly.
pub fn promote_siblings(
    tx: &Transaction<'_>,
    row: &crate::tasks::TaskRow,
    now_ms: i64,
) -> Result<Vec<TaskId>, StoreError> {
    let candidates: Vec<String> = match &row.parent_task_id {
        Some(parent) => {
            let mut stmt = tx.prepare(
                "SELECT id FROM tasks WHERE parent_task_id = ?1 AND status = 'held' \
                 ORDER BY created_at_ms ASC, id ASC",
            )?;
            let rows = stmt.query_map(params![parent.as_str()], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        }
        None => {
            let mut stmt = tx.prepare(
                "SELECT id FROM tasks WHERE parent_task_id IS NULL AND status = 'held' \
                 ORDER BY created_at_ms ASC, id ASC",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        }
    };
    let mut promoted = Vec::new();
    for raw_id in candidates {
        let task_id = TaskId::new(raw_id);
        if !are_dependencies_satisfied(tx, &task_id)? {
            continue;
        }
        let waiting = load_task(tx, &task_id)?;
        let version = transition_task(tx, &waiting, TaskStatus::Queued, now_ms)?;
        queue_event(tx, &task_id, version, now_ms)?;
        promoted.push(task_id);
    }
    Ok(promoted)
}

fn queue_event(
    tx: &Transaction<'_>,
    task_id: &TaskId,
    version: u64,
    now_ms: i64,
) -> Result<HubEventEnvelope, StoreError> {
    insert_event(
        tx,
        EVENT_TASK_QUEUED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "status": status_str(TaskStatus::Queued),
            "version": version,
        }),
        now_ms,
    )
}
