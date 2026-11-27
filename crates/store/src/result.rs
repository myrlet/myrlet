//! Latest-value Markdown result per Delivery: the Delivery's Worker is its only writer.
//!
//! The Hub stores one value plus that value's SHA-256. A report with content replaces the
//! value, an identical digest is a no-op, and an explicit `text: null` clears it. There
//! is no revision counter, history, content CAS, or separate missing/clear state: the
//! absent value *is* the absence.

use rusqlite::{params, Connection, OptionalExtension};

use myrlet_protocol::{
    DeliveryId, DeliveryStatus, DeliveryView, HubEventEnvelope, ResultReceipt, ResultReportRequest,
    WorkerId,
};

use crate::deliveries::load_delivery;
use crate::error::StoreError;
use crate::events::insert_event;

const EVENT_RESULT: &str = "delivery.result";

/// Reconcile the Hub copy with the Worker's current content.
///
/// A live epoch is required at this write boundary, so a delayed old-epoch request can
/// never undo a newer committed value. The result never changes Task status.
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
    if worker.revoked || worker.connection_epoch != request.epoch {
        return Err(StoreError::StaleDelivery);
    }
    if !crate::claim::accepts_reports(delivery.state) {
        return Err(StoreError::InvalidTransition);
    }
    let stored = delivery_result(&tx, delivery_id)?;
    let wanted = request.digest();
    let current_digest = stored.as_ref().and_then(|(_, digest, _)| digest.clone());

    if wanted.as_deref() == current_digest.as_deref() {
        return Ok((
            ResultReceipt {
                delivery_id: delivery_id.clone(),
                task_id: delivery.task_id,
                recorded_at_ms: stored.map(|(_, _, at)| at),
                result_sha256: current_digest,
                cleared: false,
                duplicate: true,
            },
            None,
        ));
    }

    let cleared = wanted.is_none();
    tx.execute(
        "UPDATE deliveries SET result_markdown = ?1, result_sha256 = ?2, result_recorded_at_ms = ?3 \
         WHERE id = ?4",
        params![
            request.text,
            wanted,
            (!cleared).then_some(now_ms),
            delivery_id.as_str()
        ],
    )?;
    let event = insert_event(
        &tx,
        EVENT_RESULT,
        &serde_json::json!({
            "taskId": delivery.task_id.as_str(),
            "deliveryId": delivery_id.as_str(),
            "cleared": cleared,
            "recordedAtMs": now_ms,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((
        ResultReceipt {
            delivery_id: delivery_id.clone(),
            task_id: delivery.task_id,
            recorded_at_ms: (!cleared).then_some(now_ms),
            result_sha256: wanted,
            cleared,
            duplicate: false,
        },
        Some(event),
    ))
}

/// The stored text, its digest, and when it was recorded, when a value exists.
pub fn delivery_result(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<Option<(String, Option<String>, i64)>, StoreError> {
    let row = conn
        .query_row(
            "SELECT result_markdown, result_sha256, result_recorded_at_ms FROM deliveries \
             WHERE id = ?1 AND result_markdown IS NOT NULL",
            [delivery_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

/// Snapshot the current Hub result through the Task's Delivery for a Worker comparison.
pub fn delivery_result_view(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<Option<myrlet_protocol::DeliveryResultView>, StoreError> {
    let Some(row) = crate::deliveries::find_delivery(conn, delivery_id)? else {
        return Ok(None);
    };
    let stored = delivery_result(conn, delivery_id)?;
    let (text, digest, recorded) = match stored {
        Some((text, digest, recorded)) => (Some(text), digest, Some(recorded)),
        None => (None, None, None),
    };
    Ok(Some(myrlet_protocol::DeliveryResultView {
        delivery_id: row.id.clone(),
        task_id: row.task_id,
        text,
        recorded_at_ms: recorded,
        result_sha256: digest,
    }))
}

/// Read one Delivery only after verifying it belongs to the authenticated Worker.
pub fn worker_delivery(
    conn: &Connection,
    worker_id: &WorkerId,
    delivery_id: &DeliveryId,
) -> Result<DeliveryView, StoreError> {
    let row = load_delivery(conn, delivery_id)?;
    if &row.worker_id != worker_id {
        return Err(StoreError::Unauthorized);
    }
    crate::deliveries::delivery_view(conn, &row)
}

/// Whether a Delivery accepts result reconciliation in its current state.
pub fn accepts(state: DeliveryStatus) -> bool {
    crate::claim::accepts_reports(state)
}
