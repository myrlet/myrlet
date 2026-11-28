//! Worker connection epochs, reported/applied scheduling inventory, and Snapshot projections.

use std::collections::HashMap;

use rusqlite::{params, Connection, Transaction};

use myrlet_protocol::{
    HubEventEnvelope, SnapshotWorker, WorkerDetailView, WorkerId, WorkerInventory,
    WorkerSchedulingConfig, WorkerSnapshot,
};

use crate::error::StoreError;
use crate::events::insert_event;
use crate::settings::load_settings;
use crate::workers::{list_worker_rows, load_worker, WorkerRow};

/// A changed report makes the Worker's reported snapshot visible to other clients.
pub const EVENT_WORKER_INVENTORY_REPORTED: &str = "worker.inventory_reported";
/// The applied scheduling snapshot changed, so Slot matching is different from now on.
pub const EVENT_WORKER_CONFIG_APPLIED: &str = "worker.config_applied";

/// What one accepted report changed, and the events committed with it.
struct ReportOutcome {
    events: Vec<HubEventEnvelope>,
}

fn report_event(row: &WorkerRow, revision: u64) -> serde_json::Value {
    serde_json::json!({ "workerId": row.worker_id.as_str(), "revision": revision })
}

/// Store one accepted scheduling report and decide whether it takes effect now.
///
/// The Hub assigns its own increasing revision when the reviewable content changes and
/// keeps the revision for a repeated report. With manual Apply the previous snapshot
/// stays in force; with Apply disabled a **changed** report is applied in the same
/// transaction. A repeated report never applies an existing pending configuration just
/// because the policy was turned off afterwards.
fn record_report(
    tx: &Transaction<'_>,
    row: &WorkerRow,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<ReportOutcome, StoreError> {
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

    let mut events = Vec::new();
    if changed {
        events.push(insert_event(
            tx,
            EVENT_WORKER_INVENTORY_REPORTED,
            &report_event(row, reported_revision),
            now_ms,
        )?);
    }
    let applied_changed = !settings.require_configuration_apply
        && changed
        && row.applied_revision != Some(reported_revision);
    if applied_changed {
        tx.execute(
            "UPDATE workers SET applied_revision = ?1, applied_config_json = ?2 \
             WHERE worker_id = ?3",
            params![
                reported_revision as i64,
                config_json,
                row.worker_id.as_str()
            ],
        )?;
        events.push(insert_event(
            tx,
            EVENT_WORKER_CONFIG_APPLIED,
            &report_event(row, reported_revision),
            now_ms,
        )?);
    }
    Ok(ReportOutcome { events })
}

/// Open a newer WSS connection: fence older epochs, then record the reported inventory.
///
/// Returns the new epoch and any events committed with the report.
pub fn open_worker_connection(
    conn: &mut Connection,
    worker_id: &WorkerId,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<(u64, Vec<HubEventEnvelope>), StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::Unauthorized);
    }
    tx.execute(
        "UPDATE workers SET connection_epoch = connection_epoch + 1 WHERE worker_id = ?1",
        params![worker_id.as_str()],
    )?;
    let outcome = record_report(&tx, &worker, inventory, now_ms)?;
    let epoch = load_worker(&tx, worker_id)?.connection_epoch;
    tx.commit()?;
    Ok((epoch, outcome.events))
}

/// Persist a newer inventory report; a stale epoch is ignored and reports `(false, [])`.
pub fn update_worker_inventory(
    conn: &mut Connection,
    worker_id: &WorkerId,
    epoch: u64,
    inventory: &WorkerInventory,
    now_ms: i64,
) -> Result<(bool, Vec<HubEventEnvelope>), StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.connection_epoch != epoch {
        return Ok((false, Vec::new())); // a stale epoch can change neither snapshot
    }
    let outcome = record_report(&tx, &worker, inventory, now_ms)?;
    tx.commit()?;
    Ok((true, outcome.events))
}

/// Apply the latest pending report at the revision the Admin reviewed.
///
/// Returns the applied revision and the event committed with the change, if any.
pub fn apply_worker_config(
    conn: &mut Connection,
    worker_id: &WorkerId,
    revision: u64,
    now_ms: i64,
) -> Result<(u64, Option<HubEventEnvelope>), StoreError> {
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
    let mut event = None;
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
        event = Some(insert_event(
            &tx,
            EVENT_WORKER_CONFIG_APPLIED,
            &report_event(&worker, reported),
            now_ms,
        )?);
    }
    tx.commit()?;
    Ok((reported, event))
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

/// Build the Hub's authoritative view of one Worker from its stored row.
///
/// `configuration_pending` is derived here: a report is pending while one exists and has
/// not yet been applied. The applied configuration body and both revisions are exposed only
/// through the administrative detail view, never the Global Snapshot.
fn into_snapshot(
    row: WorkerRow,
    active: u64,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<WorkerSnapshot, StoreError> {
    let online = !row.revoked && online_map.get(&row.worker_id) == Some(&row.connection_epoch);
    let inventory = parse_json::<WorkerInventory>(&row.inventory_json)?;
    let configuration_pending = row
        .reported_revision
        .is_some_and(|reported| row.applied_revision != Some(reported));
    let summary = SnapshotWorker {
        approval: row.status(),
        worker_id: row.worker_id,
        online,
        revoked: row.revoked,
        permission_tier: row.permission_tier,
        connection_epoch: row.connection_epoch,
        last_seen_at_ms: row.last_seen_at_ms,
        active_deliveries: active,
        host: inventory.map(|inventory| inventory.host),
        configuration_pending,
    };
    Ok(WorkerSnapshot {
        summary,
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
    Ok(list_worker_snapshots(conn, online_map)?
        .into_iter()
        .map(|snapshot| snapshot.summary)
        .collect())
}

/// Every enrolled Worker as the Hub's authoritative view, for internal scheduling.
pub fn list_worker_snapshots(
    conn: &Connection,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<Vec<WorkerSnapshot>, StoreError> {
    let mut workers = Vec::new();
    for row in list_worker_rows(conn)? {
        let active = active_deliveries(conn, &row.worker_id)?;
        workers.push(into_snapshot(row, active, online_map)?);
    }
    Ok(workers)
}

/// The Global-Snapshot projection of one Worker (no administrative fields).
pub fn get_worker(
    conn: &Connection,
    worker_id: &WorkerId,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<SnapshotWorker, StoreError> {
    Ok(get_worker_snapshot(conn, worker_id, online_map)?.summary)
}

/// The Hub's authoritative view of one Worker.
pub fn get_worker_snapshot(
    conn: &Connection,
    worker_id: &WorkerId,
    online_map: &HashMap<WorkerId, u64>,
) -> Result<WorkerSnapshot, StoreError> {
    let worker = load_worker(conn, worker_id)?;
    let active = active_deliveries(conn, worker_id)?;
    into_snapshot(worker, active, online_map)
}

/// One Worker's administrative detail: authoritative revisions, both the applied and the
/// reported configuration, and the claimable Task IDs its applied Slots could run.
pub fn worker_detail_view(
    conn: &Connection,
    worker_id: &WorkerId,
    online_map: &HashMap<WorkerId, u64>,
    claimable_limit: usize,
) -> Result<WorkerDetailView, StoreError> {
    let snapshot = get_worker_snapshot(conn, worker_id, online_map)?;
    // A detail read does not require the Worker to be online; it previews what its applied
    // Slots could claim once it is.
    let claimable_task_ids = if snapshot.is_schedulable() {
        crate::reads::claimable_for_worker(conn, snapshot.applied_config.as_ref(), claimable_limit)?
    } else {
        Vec::new()
    };
    Ok(WorkerDetailView {
        reported_config: parse_json::<WorkerSchedulingConfig>(
            &load_worker(conn, worker_id)?.reported_config_json,
        )?,
        worker: snapshot.summary,
        reported_revision: snapshot.reported_revision,
        applied_revision: snapshot.applied_revision,
        applied_config: snapshot.applied_config,
        claimable_task_ids,
    })
}

/// One Worker's latest report, which may still be pending Apply.
pub fn reported_config(
    conn: &Connection,
    worker_id: &WorkerId,
) -> Result<Option<WorkerSchedulingConfig>, StoreError> {
    let row = load_worker(conn, worker_id)?;
    parse_json::<WorkerSchedulingConfig>(&row.reported_config_json)
}
