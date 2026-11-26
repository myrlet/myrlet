//! Worker WSS reports: pre-registration refusal, cancellation answers, and resync rebinding.

use rusqlite::{params, Connection};

use myrlet_protocol::{
    DeliveryId, DeliveryStatus, HubEventEnvelope, TaskCancelledPayload, TaskRejectedPayload,
    TaskStatus, WorkerId,
};

use crate::deliveries::{cancel_state_str, find_delivery, rejection_str};
use crate::error::StoreError;
use crate::events::insert_event;
use crate::progress;
use crate::tasks::{force_task_state, load_task, status_str};

pub const EVENT_TASK_REJECTED: &str = "task.rejected";
pub const EVENT_CANCEL_REPORTED: &str = "delivery.cancel_reported";

/// Record a pre-registration refusal: only an `offered` Delivery without Progress qualifies.
pub fn record_rejection(
    conn: &mut Connection,
    worker_id: &WorkerId,
    epoch: u64,
    payload: &TaskRejectedPayload,
    now_ms: i64,
) -> Result<(bool, Option<HubEventEnvelope>), StoreError> {
    let tx = conn.transaction()?;
    let Some(delivery) = find_delivery(&tx, &payload.delivery_id)? else {
        return Ok((false, None));
    };
    if &delivery.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    if delivery.connection_epoch != epoch || delivery.task_id != payload.task_id {
        return Ok((false, None));
    }
    if delivery.state != DeliveryStatus::Offered || progress::has_progress(&tx, &delivery.id)? {
        return Ok((false, None));
    }
    tx.execute(
        "UPDATE deliveries SET state = 'rejected', rejection_code = ?1, updated_at_ms = ?2 \
         WHERE id = ?3",
        params![rejection_str(payload.code), now_ms, delivery.id.as_str()],
    )?;
    let task = load_task(&tx, &delivery.task_id)?;
    let mut task_status = task.status;
    if task.status.can_transition_to(TaskStatus::Held) {
        force_task_state(&tx, &task, TaskStatus::Held, now_ms)?;
        task_status = TaskStatus::Held;
    }
    let event = insert_event(
        &tx,
        EVENT_TASK_REJECTED,
        &serde_json::json!({
            "taskId": delivery.task_id.as_str(),
            "deliveryId": delivery.id.as_str(),
            "code": rejection_str(payload.code),
            "taskStatus": status_str(task_status),
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((true, Some(event)))
}

/// Record the Harness cancellation answer relayed by the Worker; never a termination claim.
pub fn record_cancellation_report(
    conn: &mut Connection,
    worker_id: &WorkerId,
    epoch: u64,
    payload: &TaskCancelledPayload,
    now_ms: i64,
) -> Result<(bool, Option<HubEventEnvelope>), StoreError> {
    let tx = conn.transaction()?;
    let Some(delivery) = find_delivery(&tx, &payload.delivery_id)? else {
        return Ok((false, None));
    };
    if &delivery.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    if delivery.connection_epoch != epoch || delivery.task_id != payload.task_id {
        return Ok((false, None));
    }
    if delivery.cancel_state == Some(payload.state) {
        return Ok((false, None));
    }
    tx.execute(
        "UPDATE deliveries SET cancel_state = ?1, cancel_reason = COALESCE(?2, cancel_reason), \
         updated_at_ms = ?3 WHERE id = ?4",
        params![
            cancel_state_str(payload.state),
            payload.reason,
            now_ms,
            delivery.id.as_str()
        ],
    )?;
    let event = insert_event(
        &tx,
        EVENT_CANCEL_REPORTED,
        &serde_json::json!({
            "taskId": delivery.task_id.as_str(),
            "deliveryId": delivery.id.as_str(),
            "state": cancel_state_str(payload.state),
            "reason": payload.reason,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((true, Some(event)))
}

/// Rebind retained Deliveries to the new epoch and return cancelled Deliveries that still need
/// `task.cancel` replayed to the Worker.
pub fn rebind_deliveries_on_resync(
    conn: &mut Connection,
    worker_id: &WorkerId,
    new_epoch: u64,
    active_delivery_ids: &[DeliveryId],
    now_ms: i64,
) -> Result<Vec<DeliveryId>, StoreError> {
    let tx = conn.transaction()?;
    let candidates: Vec<(DeliveryId, String, Option<String>)> = {
        let mut stmt = tx.prepare(
            "SELECT id, state, cancel_state FROM deliveries \
             WHERE worker_id = ?1 AND state IN ('offered', 'accepted', 'cancelled') \
             ORDER BY created_at_ms ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![worker_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, state, cancel_state) = row?;
            items.push((DeliveryId::new(id), state, cancel_state));
        }
        items
    };
    let mut needs_cancel = Vec::new();
    for (id, state, cancel_state) in &candidates {
        let is_active = state != "cancelled";
        let unacked_cancel = state == "cancelled" && cancel_state.is_none();
        let retained_by_worker = active_delivery_ids.contains(id);
        if is_active || unacked_cancel || retained_by_worker {
            tx.execute(
                "UPDATE deliveries SET connection_epoch = ?1, updated_at_ms = ?2 WHERE id = ?3",
                params![new_epoch as i64, now_ms, id.as_str()],
            )?;
        }
        if unacked_cancel {
            needs_cancel.push(id.clone());
        }
    }
    tx.commit()?;
    Ok(needs_cancel)
}
