//! Worker connection epochs, reported/applied scheduling inventory, and Snapshot projections.

use std::collections::HashMap;

use rusqlite::{params, Connection, Transaction};

use myrlet_protocol::{SnapshotWorker, WorkerId, WorkerInventory, WorkerSchedulingConfig};

use crate::error::StoreError;
use crate::settings::load_settings;
use crate::workers::{list_worker_rows, load_worker, WorkerRow};

/// Store one accepted scheduling report and decide whether it takes effect now.
///
/// The Hub assigns its own increasing revision when the reviewable content changes and
/// keeps the revision for a repeated report. With manual Apply the previous snapshot
/// stays in force; with Apply disabled the report is applied in the same transaction.
/// Returns whether Applied changed.
fn record_report(
    tx: &Transaction<'_>,
    row: &WorkerRow,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<bool, StoreError> {
    inventory
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let settings = load_settings(tx)?;
    let config = WorkerSchedulingConfig::from_inventory(inventory);
    let config_json = serde_json::to_string(&config)?;
    let inventory_json = serde_json::to_string(inventory)?;

    let changed = row.reported_config_json.as_deref() != Some(config_json.as_str());
    let reported_revision = if changed {
        row.revision_high_water + 1
    } else {
        row.reported_revision.unwrap_or(row.revision_high_water)
    };
    let high_water = row.revision_high_water.max(reported_revision);

    tx.execute(
        "UPDATE workers SET inventory_revision = ?1, inventory_json = ?2, \
         reported_revision = ?3, reported_config_json = ?4, revision_high_water = ?5, \
         last_seen_at_ms = ?6 WHERE worker_id = ?7",
        params![
            inventory.revision as i64,
            inventory_json,
            reported_revision as i64,
            config_json,
            high_water as i64,
            now_ms,
            row.worker_id.as_str()
        ],
    )?;
    if !settings.require_configuration_apply && row.applied_revision != Some(reported_revision) {
        tx.execute(
            "UPDATE workers SET applied_revision = ?1, applied_config_json = ?2 \
             WHERE worker_id = ?3",
            params![
                reported_revision as i64,
                config_json,
                row.worker_id.as_str()
            ],
        )?;
        return Ok(true);
    }
    Ok(false)
}

/// Open a newer WSS connection: fence older epochs, then record the reported inventory.
pub fn open_worker_connection(
    conn: &mut Connection,
    worker_id: &WorkerId,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<u64, StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::Unauthorized);
    }
    tx.execute(
        "UPDATE workers SET connection_epoch = connection_epoch + 1 WHERE worker_id = ?1",
        params![worker_id.as_str()],
    )?;
    record_report(&tx, &worker, inventory, now_ms)?;
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
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.connection_epoch != epoch {
        return Ok(false); // a stale epoch can change neither snapshot
    }
    record_report(&tx, &worker, inventory, now_ms)?;
    tx.commit()?;
    Ok(true)
}

/// Apply the latest pending report at the revision the Admin reviewed.
pub fn apply_worker_config(
    conn: &mut Connection,
    worker_id: &WorkerId,
    revision: u64,
    _now_ms: i64,
) -> Result<u64, StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::InvalidTransition);
    }
    let Some(reported) = worker.reported_revision else {
        return Err(StoreError::NoPendingConfig);
    };
    if reported != revision {
        return Err(StoreError::ConfigRevisionMismatch);
    }
    if worker.applied_revision != Some(reported) {
        tx.execute(
            "UPDATE workers SET applied_revision = ?1, applied_config_json = ?2 \
             WHERE worker_id = ?3",
            params![
                reported as i64,
                worker.reported_config_json,
                worker_id.as_str()
            ],
        )?;
    }
    tx.commit()?;
    Ok(reported)
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
        "SELECT COUNT(*) FROM deliveries WHERE worker_id = ?1 AND state = 'accepted'",
        params![worker_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(count).unwrap_or(0))
}

fn parse_json<T: serde::de::DeserializeOwned>(
    value: &Option<String>,
) -> Result<Option<T>, StoreError> {
    match value {
        Some(raw) => Ok(Some(serde_json::from_str(raw)?)),
        None => Ok(None),
    }
}

fn into_snapshot(
    row: WorkerRow,
    active: u64,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<SnapshotWorker, StoreError> {
    let online = !row.revoked && online_map.get(&row.worker_id) == Some(&row.connection_epoch);
    let inventory = parse_json::<WorkerInventory>(&row.inventory_json)?;
    Ok(SnapshotWorker {
        approval: row.status(),
        worker_id: row.worker_id,
        online,
        revoked: row.revoked,
        connection_epoch: row.connection_epoch,
        last_seen_at_ms: row.last_seen_at_ms,
        active_deliveries: active,
        host: inventory.map(|inventory| inventory.host),
        reported_revision: row.reported_revision,
        applied_revision: row.applied_revision,
        applied_config: parse_json::<WorkerSchedulingConfig>(&row.applied_config_json)?,
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

/// One Worker's latest report, which may still be pending Apply.
pub fn reported_config(
    conn: &Connection,
    worker_id: &WorkerId,
) -> Result<Option<WorkerSchedulingConfig>, StoreError> {
    let row = load_worker(conn, worker_id)?;
    parse_json::<WorkerSchedulingConfig>(&row.reported_config_json)
}
