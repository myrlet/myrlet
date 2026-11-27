//! Progress and Finish reporting: latest-state Progress and outcome-free finish.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use myrlet_protocol::{
    DeliveryId, DeliveryProgressView, DeliveryStatus, FinishReceipt, FinishReportRequest,
    HubEventEnvelope, ProgressReceipt, ProgressReportRequest, TaskId, TaskStatus, WorkerId,
};

use crate::deliveries::{self, DeliveryRow};
use crate::error::StoreError;
use crate::events::insert_event;
use crate::tasks::{self, status_str, EVENT_TASK_FINISHED};

const EVENT_PROGRESS: &str = "delivery.progress";

pub fn has_progress(conn: &Connection, delivery_id: &DeliveryId) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM delivery_progress WHERE delivery_id = ?1",
        params![delivery_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Latest accepted Progress for a Delivery; Hub keeps no Progress history.
pub fn progress_view(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<Option<DeliveryProgressView>, StoreError> {
    let row = conn
        .query_row(
            "SELECT task_id, text, observed_at_ms, first_received_at_ms, received_at_ms \
             FROM delivery_progress WHERE delivery_id = ?1",
            params![delivery_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((task_id, text, observed_at_ms, first_received_at_ms, received_at_ms)) = row else {
        return Ok(None);
    };
    Ok(Some(DeliveryProgressView {
        delivery_id: delivery_id.clone(),
        task_id: TaskId::new(task_id),
        text,
        observed_at_ms,
        received_at_ms,
        first_received_at_ms,
    }))
}

/// Touch the Delivery: first accepted Progress keeps its audit timestamp current.
fn mark_reported(
    tx: &Transaction<'_>,
    delivery: &DeliveryRow,
    now_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE deliveries SET updated_at_ms = ?1 WHERE id = ?2",
        params![now_ms, delivery.id.as_str()],
    )?;
    Ok(())
}

/// Store one bounded latest-value Progress; never changes Task status, even after cancel.
pub fn record_progress(
    conn: &mut Connection,
    worker_id: &WorkerId,
    delivery_id: &DeliveryId,
    request: &ProgressReportRequest,
    now_ms: i64,
) -> Result<(ProgressReceipt, Option<HubEventEnvelope>), StoreError> {
    request
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let tx = conn.transaction()?;
    let delivery = deliveries::load_delivery(&tx, delivery_id)?;
    if &delivery.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    if delivery.connection_epoch != request.epoch {
        return Err(StoreError::StaleDelivery);
    }
    if !crate::claim::accepts_reports(delivery.state) {
        return Err(StoreError::StaleDelivery);
    }
    let existing = tx
        .query_row(
            "SELECT observed_at_ms, first_received_at_ms, received_at_ms \
             FROM delivery_progress WHERE delivery_id = ?1",
            params![delivery_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;

    let receipt = match existing {
        None => {
            tx.execute(
                "INSERT INTO delivery_progress (delivery_id, task_id, text, observed_at_ms, \
                 first_received_at_ms, received_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    delivery_id.as_str(),
                    delivery.task_id.as_str(),
                    request.text,
                    request.observed_at_ms,
                    now_ms
                ],
            )?;
            mark_reported(&tx, &delivery, now_ms)?;
            ProgressReceipt {
                delivery_id: delivery_id.clone(),
                accepted_at_ms: now_ms,
                first_accepted_at_ms: now_ms,
                duplicate: false,
            }
        }
        Some((observed_at_ms, first_received_at_ms, received_at_ms)) => {
            if request.observed_at_ms < observed_at_ms {
                return Err(StoreError::StaleDelivery);
            }
            if request.observed_at_ms == observed_at_ms {
                return Ok((
                    ProgressReceipt {
                        delivery_id: delivery_id.clone(),
                        accepted_at_ms: received_at_ms,
                        first_accepted_at_ms: first_received_at_ms,
                        duplicate: true,
                    },
                    None,
                ));
            }
            tx.execute(
                "UPDATE delivery_progress SET text = ?1, observed_at_ms = ?2, received_at_ms = ?3 \
                 WHERE delivery_id = ?4",
                params![
                    request.text,
                    request.observed_at_ms,
                    now_ms,
                    delivery_id.as_str()
                ],
            )?;
            mark_reported(&tx, &delivery, now_ms)?;
            ProgressReceipt {
                delivery_id: delivery_id.clone(),
                accepted_at_ms: now_ms,
                first_accepted_at_ms: first_received_at_ms,
                duplicate: false,
            }
        }
    };

    let event = insert_event(
        &tx,
        EVENT_PROGRESS,
        &serde_json::json!({
            "taskId": delivery.task_id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "observedAtMs": request.observed_at_ms,
            "receivedAtMs": now_ms,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((receipt, Some(event)))
}

/// Record the end of Worker handling; `cancelled` is never overwritten and replay is a no-op.
pub fn record_finish(
    conn: &mut Connection,
    worker_id: &WorkerId,
    delivery_id: &DeliveryId,
    request: &FinishReportRequest,
    now_ms: i64,
) -> Result<(FinishReceipt, Option<HubEventEnvelope>), StoreError> {
    let tx = conn.transaction()?;
    let delivery = deliveries::load_delivery(&tx, delivery_id)?;
    if &delivery.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    let worker = crate::workers::load_worker(&tx, worker_id)?;
    if worker.revoked || worker.connection_epoch != request.epoch {
        return Err(StoreError::StaleDelivery);
    }
    let task = tasks::load_task(&tx, &delivery.task_id)?;
    if task.status == TaskStatus::Cancelled {
        return Ok((
            FinishReceipt {
                delivery_id: delivery_id.clone(),
                task_id: task.id,
                task_status: TaskStatus::Cancelled,
                changed: false,
            },
            None,
        ));
    }
    if delivery.state == DeliveryStatus::Finished {
        return Ok((
            FinishReceipt {
                delivery_id: delivery_id.clone(),
                task_id: task.id,
                task_status: task.status,
                changed: false,
            },
            None,
        ));
    }
    if !matches!(delivery.state, DeliveryStatus::Accepted) {
        return Err(StoreError::StaleDelivery);
    }
    tx.execute(
        "UPDATE deliveries SET state = 'finished', updated_at_ms = ?1 WHERE id = ?2",
        params![now_ms, delivery_id.as_str()],
    )?;
    let version = tasks::transition_task(&tx, &task, TaskStatus::Finished, now_ms)?;
    let event = insert_event(
        &tx,
        EVENT_TASK_FINISHED,
        &serde_json::json!({
            "taskId": task.id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "status": status_str(TaskStatus::Finished),
            "version": version,
        }),
        now_ms,
    )?;
    crate::commands::promote_siblings(&tx, &task, now_ms)?;
    tx.commit()?;
    Ok((
        FinishReceipt {
            delivery_id: delivery_id.clone(),
            task_id: task.id,
            task_status: TaskStatus::Finished,
            changed: true,
        },
        Some(event),
    ))
}
