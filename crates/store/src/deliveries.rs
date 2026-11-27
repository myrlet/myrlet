//! Delivery rows and wire views shared by claims, reporting, and Task reads.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use myrlet_protocol::{
    CancelState, DeliveryId, DeliveryStatus, DeliveryView, ExecutorId, SlotId, TaskId, WorkerId,
};

use crate::error::StoreError;
use crate::progress;

/// Canonical wire token for a Delivery state.
pub fn state_str(state: DeliveryStatus) -> &'static str {
    match state {
        DeliveryStatus::Accepted => "accepted",
        DeliveryStatus::Finished => "finished",
        DeliveryStatus::Cancelled => "cancelled",
    }
}

fn parse_state(raw: &str) -> Result<DeliveryStatus, StoreError> {
    match raw {
        "accepted" => Ok(DeliveryStatus::Accepted),
        "finished" => Ok(DeliveryStatus::Finished),
        "cancelled" => Ok(DeliveryStatus::Cancelled),
        other => Err(StoreError::invalid(format!(
            "unknown delivery state {other}"
        ))),
    }
}

pub fn cancel_state_str(state: CancelState) -> &'static str {
    match state {
        CancelState::Confirmed => "confirmed",
        CancelState::Failed => "failed",
        CancelState::Uncertain => "uncertain",
        CancelState::Unsupported => "unsupported",
    }
}

fn parse_cancel_state(raw: &str) -> Result<CancelState, StoreError> {
    match raw {
        "confirmed" => Ok(CancelState::Confirmed),
        "failed" => Ok(CancelState::Failed),
        "uncertain" => Ok(CancelState::Uncertain),
        "unsupported" => Ok(CancelState::Unsupported),
        other => Err(StoreError::invalid(format!("unknown cancel state {other}"))),
    }
}

/// Stored Delivery row without the latest Progress view.
#[derive(Debug, Clone)]
pub struct DeliveryRow {
    pub id: DeliveryId,
    pub task_id: TaskId,
    pub worker_id: WorkerId,
    pub slot_id: SlotId,
    pub executor_id: Option<ExecutorId>,
    pub connection_epoch: u64,
    pub state: DeliveryStatus,
    pub cancel_state: Option<CancelState>,
    pub cancel_reason: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

const DELIVERY_COLUMNS: &str = "id, task_id, worker_id, slot_id, executor_id, connection_epoch, \
     state, cancel_state, cancel_reason, created_at_ms, updated_at_ms";

type RawDelivery = (
    String,
    String,
    String,
    String,
    Option<String>,
    i64,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

fn map_delivery(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawDelivery> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn to_delivery_row(raw: RawDelivery) -> Result<DeliveryRow, StoreError> {
    Ok(DeliveryRow {
        id: DeliveryId::new(raw.0),
        task_id: TaskId::new(raw.1),
        worker_id: WorkerId::new(raw.2),
        slot_id: SlotId::new(raw.3),
        executor_id: raw.4.map(ExecutorId::new),
        connection_epoch: u64::try_from(raw.5)
            .map_err(|_| StoreError::invalid("stored connection epoch is negative"))?,
        state: parse_state(&raw.6)?,
        cancel_state: raw
            .7
            .map(|stored| parse_cancel_state(&stored))
            .transpose()?,
        cancel_reason: raw.8,
        created_at_ms: raw.9,
        updated_at_ms: raw.10,
    })
}

pub fn find_delivery(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<Option<DeliveryRow>, StoreError> {
    let sql = format!("SELECT {DELIVERY_COLUMNS} FROM deliveries WHERE id = ?1");
    let raw = conn
        .query_row(&sql, params![delivery_id.as_str()], map_delivery)
        .optional()?;
    raw.map(to_delivery_row).transpose()
}

pub fn load_delivery(
    conn: &Connection,
    delivery_id: &DeliveryId,
) -> Result<DeliveryRow, StoreError> {
    find_delivery(conn, delivery_id)?.ok_or(StoreError::NotFound)
}

/// Project a stored Delivery into the wire view, including its latest accepted Progress.
pub fn delivery_view(conn: &Connection, row: &DeliveryRow) -> Result<DeliveryView, StoreError> {
    Ok(DeliveryView {
        id: row.id.clone(),
        task_id: row.task_id.clone(),
        worker_id: row.worker_id.clone(),
        slot_id: row.slot_id.clone(),
        executor_id: row.executor_id.clone(),
        connection_epoch: row.connection_epoch,
        state: row.state,
        cancel_state: row.cancel_state,
        cancel_reason: row.cancel_reason.clone(),
        created_at_ms: row.created_at_ms,
        updated_at_ms: row.updated_at_ms,
        progress: progress::progress_view(conn, &row.id)?,
    })
}

/// The Task's one Delivery, whatever its state.
pub fn current_delivery(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<Option<DeliveryView>, StoreError> {
    let sql = format!("SELECT {DELIVERY_COLUMNS} FROM deliveries WHERE task_id = ?1");
    let raw = conn
        .query_row(&sql, params![task_id.as_str()], map_delivery)
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    Ok(Some(delivery_view(conn, &to_delivery_row(raw)?)?))
}

/// The Task's Delivery while it is still live (`accepted`).
pub fn active_delivery(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<Option<DeliveryRow>, StoreError> {
    let sql = format!(
        "SELECT {DELIVERY_COLUMNS} FROM deliveries \
         WHERE task_id = ?1 AND state = 'accepted'"
    );
    let raw = conn
        .query_row(&sql, params![task_id.as_str()], map_delivery)
        .optional()?;
    raw.map(to_delivery_row).transpose()
}

pub fn task_has_delivery(conn: &Connection, task_id: &TaskId) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM deliveries WHERE task_id = ?1",
        params![task_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Mark the live Delivery cancelled so the Hub can send `task.cancel` for it.
pub fn cancel_active_delivery(
    tx: &Transaction<'_>,
    task_id: &TaskId,
    reason: Option<&str>,
    now_ms: i64,
) -> Result<Option<DeliveryView>, StoreError> {
    let Some(row) = active_delivery(tx, task_id)? else {
        return Ok(None);
    };
    tx.execute(
        "UPDATE deliveries SET state = 'cancelled', cancel_reason = ?1, updated_at_ms = ?2 \
         WHERE id = ?3",
        params![reason, now_ms, row.id.as_str()],
    )?;
    let updated = load_delivery(tx, &row.id)?;
    Ok(Some(delivery_view(tx, &updated)?))
}
