//! Enrolled Worker fleet administration: credential rotation, revocation, and approval.

use rusqlite::{params, Connection};

use myrlet_protocol::{
    HubEventEnvelope, RotateWorkerCredentialResponse, WorkerApprovalStatus, WorkerId,
};

use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::sha256_hex;
use crate::workers::{
    load_worker, random_hex, CREDENTIAL_BYTES, EVENT_WORKER_APPROVED, EVENT_WORKER_REVOKED,
    EVENT_WORKER_ROTATED,
};

/// Rotate a Worker credential; the previous verifier and the live WSS epoch are both fenced.
/// Approval and both scheduling snapshots survive rotation.
pub fn rotate_worker_credential(
    conn: &mut Connection,
    hub_id: &str,
    worker_id: &WorkerId,
    now_ms: i64,
) -> Result<(RotateWorkerCredentialResponse, HubEventEnvelope), StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::Unauthorized);
    }
    let credential = random_hex(CREDENTIAL_BYTES);
    tx.execute(
        "UPDATE workers SET token_sha256 = ?1, rotated_at_ms = ?2, connection_epoch = connection_epoch + 1 \
         WHERE worker_id = ?3",
        params![sha256_hex(credential.as_bytes()), now_ms, worker_id.as_str()],
    )?;
    let event = insert_event(
        &tx,
        EVENT_WORKER_ROTATED,
        &serde_json::json!({ "workerId": worker_id.as_str() }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((
        RotateWorkerCredentialResponse {
            hub_id: hub_id.to_string(),
            worker_id: worker_id.clone(),
            credential,
        },
        event,
    ))
}

/// Revoke a known Worker (pending or approved) and fence its live WSS session.
pub fn revoke_worker(
    conn: &mut Connection,
    worker_id: &WorkerId,
    now_ms: i64,
) -> Result<HubEventEnvelope, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE workers SET revoked = 1, connection_epoch = connection_epoch + 1 WHERE worker_id = ?1",
        params![worker_id.as_str()],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound);
    }
    let event = insert_event(
        &tx,
        EVENT_WORKER_REVOKED,
        &serde_json::json!({ "workerId": worker_id.as_str() }),
        now_ms,
    )?;
    tx.commit()?;
    Ok(event)
}

/// Approve a pending Worker; repeating approval for an approved Worker succeeds.
///
/// A revoked Worker cannot be re-admitted without a new Pair.
pub fn approve_worker(
    conn: &mut Connection,
    worker_id: &WorkerId,
    now_ms: i64,
) -> Result<HubEventEnvelope, StoreError> {
    let tx = conn.transaction()?;
    let worker = load_worker(&tx, worker_id)?;
    if worker.revoked {
        return Err(StoreError::InvalidTransition);
    }
    tx.execute(
        "UPDATE workers SET approval = 'approved' WHERE worker_id = ?1",
        params![worker_id.as_str()],
    )?;
    let event = insert_event(
        &tx,
        EVENT_WORKER_APPROVED,
        &serde_json::json!({
            "workerId": worker_id.as_str(),
            "approval": "approved",
            "changed": worker.approval != WorkerApprovalStatus::Approved,
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok(event)
}
