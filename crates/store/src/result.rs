//! Optional per-Delivery Markdown result: write-once text, independent of Finish.

use rusqlite::{params, Connection, OptionalExtension};

use myrlet_protocol::{
    DeliveryId, DeliveryStatus, HubEventEnvelope, ResultReceipt, ResultReportRequest, WorkerId,
};

use crate::deliveries::load_delivery;
use crate::error::StoreError;
use crate::events::insert_event;

const EVENT_RESULT: &str = "delivery.result";

/// Store the final Harness-written result once; identical replays are idempotent.
///
/// A live epoch is still required after Finish, so a late sync following reconnect is
/// accepted. The result never changes Task status and cancellation never removes it.
pub fn record_result(
    conn: &mut Connection,
    worker_id: &WorkerId,
    delivery_id: &DeliveryId,
    request: &ResultReportRequest,
    now_ms: i64,
) -> Result<(ResultReceipt, Option<HubEventEnvelope>), StoreError> {
    request
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let tx = conn.transaction()?;
    let delivery = load_delivery(&tx, delivery_id)?;
    if &delivery.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    let worker = crate::workers::load_worker(&tx, worker_id)?;
    let terminal = matches!(
        delivery.state,
        DeliveryStatus::Finished | DeliveryStatus::Cancelled
    );
    if worker.revoked
        || worker.connection_epoch != request.epoch
        || (!terminal && delivery.connection_epoch != request.epoch)
    {
        return Err(StoreError::StaleDelivery);
    }
    if !matches!(
        delivery.state,
        DeliveryStatus::Accepted | DeliveryStatus::Finished | DeliveryStatus::Cancelled
    ) {
        return Err(StoreError::StaleDelivery);
    }
    if let Some((stored, recorded_at_ms)) = delivery_result(&tx, delivery_id)? {
        if stored == request.text {
            return Ok((
                ResultReceipt {
                    delivery_id: delivery_id.clone(),
                    task_id: delivery.task_id,
                    recorded_at_ms,
                    duplicate: true,
                },
                None,
            ));
        }
        return Err(StoreError::InvalidTransition);
    }
    tx.execute(
        "UPDATE deliveries SET result_markdown = ?1, result_recorded_at_ms = ?2 WHERE id = ?3",
        params![request.text, now_ms, delivery_id.as_str()],
    )?;
    let event = insert_event(
        &tx,
        EVENT_RESULT,
        &serde_json::json!({
            "taskId": delivery.task_id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "recordedAtMs": now_ms,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((
        ResultReceipt {
            delivery_id: delivery_id.clone(),
            task_id: delivery.task_id,
            recorded_at_ms: now_ms,
            duplicate: false,
        },
        Some(event),
    ))
}

/// Read the immutable text from the Delivery selected by the Task detail.
pub fn delivery_result(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<Option<(String, i64)>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT result_markdown, result_recorded_at_ms FROM deliveries \
             WHERE id = ?1 AND result_markdown IS NOT NULL",
            [delivery_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}
