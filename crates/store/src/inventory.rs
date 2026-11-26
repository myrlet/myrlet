//! Worker connection epochs, reported inventory, and Snapshot projections.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use myrlet_protocol::{SnapshotWorker, WorkerId, WorkerInventory};

use crate::error::StoreError;
use crate::workers::{list_worker_rows, load_worker, WorkerRow};

/// Open a newer WSS connection: bump the monotonic epoch and persist the reported inventory.
pub fn open_worker_connection(
    conn: &mut Connection,
    worker_id: &WorkerId,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<u64, StoreError> {
    inventory
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let tx = conn.transaction()?;
    if load_worker(&tx, worker_id)?.revoked {
        return Err(StoreError::Unauthorized);
    }
    tx.execute(
        "UPDATE workers SET connection_epoch = connection_epoch + 1, inventory_revision = ?1, \
         inventory_json = ?2, last_seen_at_ms = ?3 WHERE worker_id = ?4",
        params![
            inventory.revision as i64,
            serde_json::to_string(inventory)?,
            now_ms,
            worker_id.as_str()
        ],
    )?;
    let epoch = load_worker(&tx, worker_id)?.connection_epoch;
    tx.commit()?;
    Ok(epoch)
}

/// Persist a newer inventory report; a stale epoch is ignored and reports `false`.
pub fn update_worker_inventory(
    conn: &mut Connection,
    worker_id: &WorkerId,
    epoch: u64,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<bool, StoreError> {
    inventory
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let tx = conn.transaction()?;
    if load_worker(&tx, worker_id)?.connection_epoch != epoch {
        return Ok(false);
    }
    tx.execute(
        "UPDATE workers SET inventory_revision = ?1, inventory_json = ?2, last_seen_at_ms = ?3 \
         WHERE worker_id = ?4",
        params![
            inventory.revision as i64,
            serde_json::to_string(inventory)?,
            now_ms,
            worker_id.as_str()
        ],
    )?;
    tx.commit()?;
    Ok(true)
}

/// Refresh liveness for the current connection epoch; older epochs are ignored.
pub fn touch_worker_seen(
    conn: &Connection,
    worker_id: &WorkerId,
    epoch: u64,
    now_ms: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE workers SET last_seen_at_ms = ?1 WHERE worker_id = ?2 AND connection_epoch = ?3",
        params![now_ms, worker_id.as_str(), epoch as i64],
    )?;
    Ok(())
}

fn active_deliveries(conn: &Connection, worker_id: &WorkerId) -> Result<u64, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM deliveries WHERE worker_id = ?1 AND state IN ('offered', 'accepted')",
        params![worker_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(count).unwrap_or(0))
}

fn into_snapshot(
    row: WorkerRow,
    active: u64,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<SnapshotWorker, StoreError> {
    let online = !row.revoked && online_map.get(&row.worker_id) == Some(&row.connection_epoch);
    let inventory = match &row.inventory_json {
        Some(json) => Some(serde_json::from_str::<WorkerInventory>(json)?),
        None => None,
    };
    Ok(SnapshotWorker {
        worker_id: row.worker_id,
        online,
        revoked: row.revoked,
        connection_epoch: row.connection_epoch,
        last_seen_at_ms: row.last_seen_at_ms,
        active_deliveries: active,
        inventory,
    })
}

/// Every enrolled Worker with connectivity derived from the live connection map.
pub fn list_workers(
    conn: &Connection,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<Vec<SnapshotWorker>, StoreError> {
    let mut workers = Vec::new();
    for row in list_worker_rows(conn)? {
        let active = active_deliveries(conn, &row.worker_id)?;
        workers.push(into_snapshot(row, active, online_map)?);
    }
    Ok(workers)
}

/// One enrolled Worker with the same derived fields as [`list_workers`].
pub fn get_worker(
    conn: &Connection,
    worker_id: &WorkerId,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<SnapshotWorker, StoreError> {
    let worker = load_worker(conn, worker_id)?;
    let active = active_deliveries(conn, worker_id)?;
    into_snapshot(worker, active, online_map)
}
