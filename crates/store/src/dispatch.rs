//! Dispatch commands: FIFO queue admission, offer creation, and pre-send release.

use rusqlite::{params, Connection};

use myrlet_protocol::{
    DeliveryId, DeliveryStatus, DispatchTaskRequest, DispatchTaskResponse, ExecutorId,
    HubEventEnvelope, SlotId, TaskId, TaskStatus, WorkerId,
};

use crate::deliveries::{delivery_view, load_delivery};
use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::{check_idempotency, digest_request, record_idempotency};
use crate::tasks::{
    are_dependencies_satisfied, force_task_state, load_task, status_str, transition_task,
    EVENT_TASK_DISPATCHED, EVENT_TASK_QUEUED,
};
use crate::workers::load_worker;
use crate::DispatchOutcome;

const OP_ENQUEUE: &str = "tasks.enqueue";
const OP_DISPATCH: &str = "tasks.dispatch";
pub const EVENT_DELIVERY_RELEASED: &str = "delivery.released";

/// Resolved destination of one Delivery; the epoch is fenced against the Worker row.
#[derive(Debug, Clone)]
pub struct DeliveryTarget {
    pub worker_id: WorkerId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    pub connection_epoch: u64,
}

/// Authorize a `held` Task into the FIFO queue without reserving any capacity.
pub fn enqueue_task(
    conn: &mut Connection,
    idempotency_key: &str,
    task_id: &TaskId,
    request: &DispatchTaskRequest,
    now_ms: i64,
) -> Result<DispatchOutcome, StoreError> {
    let digest = digest_request(&serde_json::json!({
        "taskId": task_id.as_str(),
        "request": request,
    }))?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_ENQUEUE, idempotency_key, &digest)? {
        let response: DispatchTaskResponse = serde_json::from_str(&hit.response_json)?;
        return Ok((response, None, true));
    }
    let row = load_task(&tx, task_id)?;
    if row.version != request.expected_task_version {
        return Err(StoreError::VersionConflict);
    }
    if row.status == TaskStatus::Queued {
        let response = DispatchTaskResponse {
            task_id: task_id.clone(),
            status: TaskStatus::Queued,
            version: row.version,
            delivery: None,
        };
        return Ok((response, None, false));
    }
    if row.status != TaskStatus::Held {
        return Err(StoreError::InvalidTransition);
    }
    let version = transition_task(&tx, &row, TaskStatus::Queued, now_ms)?;
    tx.execute(
        "UPDATE tasks SET queued_at_ms = ?1, queue_request_json = ?2 WHERE id = ?3",
        params![now_ms, serde_json::to_string(request)?, task_id.as_str()],
    )?;
    let event = insert_event(
        &tx,
        EVENT_TASK_QUEUED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "status": status_str(TaskStatus::Queued),
            "version": version,
        }),
        now_ms,
    )?;
    let response = DispatchTaskResponse {
        task_id: task_id.clone(),
        status: TaskStatus::Queued,
        version,
        delivery: None,
    };
    record_idempotency(
        &tx,
        OP_ENQUEUE,
        idempotency_key,
        &digest,
        202,
        &serde_json::to_string(&response)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((response, Some(event), false))
}

/// Commit one `offered` Delivery and move the Task to `dispatched`.
pub fn create_delivery(
    conn: &mut Connection,
    idempotency_key: Option<&str>,
    task_id: &TaskId,
    expected_version: u64,
    target: &DeliveryTarget,
    now_ms: i64,
) -> Result<DispatchOutcome, StoreError> {
    let mut digest = None;
    if idempotency_key.is_some() {
        let intent = serde_json::json!({
            "taskId": task_id.as_str(),
            "expectedTaskVersion": expected_version,
            "workerId": target.worker_id.as_str(),
            "slotId": target.slot_id.as_str(),
            "executorId": target.executor_id.as_ref().map(ExecutorId::as_str),
            "connectionEpoch": target.connection_epoch,
        });
        digest = Some(digest_request(&intent)?);
    }
    let tx = conn.transaction()?;
    let replay = match (idempotency_key, digest.as_ref()) {
        (Some(key), Some(digest)) => check_idempotency(&tx, OP_DISPATCH, key, digest)?,
        _ => None,
    };
    if let Some(hit) = replay {
        let response: DispatchTaskResponse = serde_json::from_str(&hit.response_json)?;
        return Ok((response, None, true));
    }
    let row = load_task(&tx, task_id)?;
    if row.version != expected_version {
        return Err(StoreError::VersionConflict);
    }
    if !matches!(row.status, TaskStatus::Held | TaskStatus::Queued) {
        return Err(StoreError::InvalidTransition);
    }
    if !are_dependencies_satisfied(&tx, task_id)? {
        return Err(StoreError::invalid("dependencies are not satisfied"));
    }
    let worker = load_worker(&tx, &target.worker_id)?;
    if worker.revoked {
        return Err(StoreError::Unauthorized);
    }
    if worker.connection_epoch != target.connection_epoch {
        return Err(StoreError::StaleDelivery);
    }
    let delivery_id = DeliveryId::new(format!("delivery_{}", uuid::Uuid::new_v4().simple()));
    tx.execute(
        "INSERT INTO deliveries (id, task_id, worker_id, slot_id, executor_id, connection_epoch, \
         state, rejection_code, cancel_state, cancel_reason, created_at_ms, updated_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'offered', NULL, NULL, NULL, ?7, ?7)",
        params![
            delivery_id.as_str(),
            task_id.as_str(),
            target.worker_id.as_str(),
            target.slot_id.as_str(),
            target.executor_id.as_ref().map(ExecutorId::as_str),
            target.connection_epoch as i64,
            now_ms
        ],
    )?;
    let version = transition_task(&tx, &row, TaskStatus::Dispatched, now_ms)?;
    let delivery = delivery_view(&tx, &load_delivery(&tx, &delivery_id)?)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_DISPATCHED,
        &serde_json::json!({
            "taskId": task_id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "workerId": target.worker_id.as_str(),
            "slotId": target.slot_id.as_str(),
            "status": status_str(TaskStatus::Dispatched),
            "version": version,
        }),
        now_ms,
    )?;
    let response = DispatchTaskResponse {
        task_id: task_id.clone(),
        status: TaskStatus::Dispatched,
        version,
        delivery: Some(delivery),
    };
    if let (Some(key), Some(digest)) = (idempotency_key, digest.as_ref()) {
        record_idempotency(
            &tx,
            OP_DISPATCH,
            key,
            digest,
            200,
            &serde_json::to_string(&response)?,
            now_ms,
        )?;
    }
    tx.commit()?;
    Ok((response, Some(event), false))
}

/// Roll back an offer that never reached the socket; the Task waits again.
pub fn release_delivery(
    conn: &mut Connection,
    delivery_id: &DeliveryId,
    requeue: bool,
    now_ms: i64,
) -> Result<Option<HubEventEnvelope>, StoreError> {
    let tx = conn.transaction()?;
    let row = load_delivery(&tx, delivery_id)?;
    if row.state != DeliveryStatus::Offered {
        return Ok(None);
    }
    tx.execute(
        "UPDATE deliveries SET state = 'released', updated_at_ms = ?1 WHERE id = ?2",
        params![now_ms, delivery_id.as_str()],
    )?;
    let task = load_task(&tx, &row.task_id)?;
    let mut task_status = task.status;
    if !task.status.is_terminal() {
        let next = if requeue {
            TaskStatus::Queued
        } else {
            TaskStatus::Held
        };
        force_task_state(&tx, &task, next, now_ms)?;
        if requeue {
            tx.execute(
                "UPDATE tasks SET queued_at_ms = ?1 WHERE id = ?2",
                params![now_ms, task.id.as_str()],
            )?;
        }
        task_status = next;
    }
    let event = insert_event(
        &tx,
        EVENT_DELIVERY_RELEASED,
        &serde_json::json!({
            "deliveryId": delivery_id.as_str(),
            "taskId": row.task_id.as_str(),
            "taskStatus": status_str(task_status),
            "requeued": requeue,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok(Some(event))
}
