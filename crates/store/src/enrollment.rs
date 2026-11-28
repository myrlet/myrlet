//! Worker enrollment: one-time pairing codes, credential redemption, and constant-time
//! bearer verification.

use rusqlite::{params, Connection, OptionalExtension};
use subtle::ConstantTimeEq;

use myrlet_protocol::{
    CreateEnrollmentCodeResponse, HubEventEnvelope, PairWorkerRequest, PairWorkerResponse,
    WorkerApprovalStatus, WorkerId,
};

use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::sha256_hex;
use crate::settings::load_settings;
use crate::workers::{
    approval_str, random_hex, CREDENTIAL_BYTES, DEFAULT_TTL_SECS, ENROLLMENT_CODE_BYTES,
    EVENT_WORKER_PAIRED, MAX_TTL_SECS,
};

/// Issue a short-lived one-time pairing code; only its SHA-256 verifier is stored.
pub fn create_enrollment_code(
    conn: &Connection,
    ttl_secs: Option<u64>,
    suggested_worker_id: Option<&WorkerId>,
    now_ms: i64,
) -> Result<CreateEnrollmentCodeResponse, StoreError> {
    if suggested_worker_id.is_some_and(|worker_id| !worker_id.is_canonical()) {
        return Err(StoreError::invalid("suggestedWorkerId is not canonical"));
    }
    let ttl = ttl_secs.unwrap_or(DEFAULT_TTL_SECS).clamp(1, MAX_TTL_SECS);
    let code = random_hex(ENROLLMENT_CODE_BYTES);
    let expires_at_ms = now_ms + (ttl as i64) * 1_000;
    conn.execute(
        "INSERT INTO enrollment_codes \
         (code_sha256, suggested_worker_id, expires_at_ms, consumed_at_ms, created_at_ms) \
         VALUES (?1, ?2, ?3, NULL, ?4)",
        params![
            sha256_hex(code.as_bytes()),
            suggested_worker_id.map(WorkerId::as_str),
            expires_at_ms,
            now_ms
        ],
    )?;
    Ok(CreateEnrollmentCodeResponse {
        enrollment_code: code,
        expires_at_ms,
        suggested_worker_id: suggested_worker_id.cloned(),
    })
}

/// Redeem a pairing code in one step: bind a Worker and hand back its only plaintext credential.
///
/// A fresh pairing (or a re-pair after revocation) starts a new approval decision and
/// clears both scheduling snapshots; the revision high-water mark is retained.
pub fn redeem_pairing_code(
    conn: &mut Connection,
    hub_id: &str,
    request: &PairWorkerRequest,
    now_ms: i64,
) -> Result<(PairWorkerResponse, HubEventEnvelope), StoreError> {
    let code_sha256 = sha256_hex(request.enrollment_code.as_bytes());
    let tx = conn.transaction()?;
    let matched = {
        let mut stmt = tx.prepare(
            "SELECT code_sha256, suggested_worker_id, expires_at_ms \
             FROM enrollment_codes WHERE consumed_at_ms IS NULL",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut found = None;
        for row in rows {
            let (stored_hash, suggested, expires_at_ms) = row?;
            let equal: bool = stored_hash.as_bytes().ct_eq(code_sha256.as_bytes()).into();
            if equal {
                found = Some((stored_hash, suggested, expires_at_ms));
            }
        }
        found
    };
    let Some((matched_hash, suggested, expires_at_ms)) = matched else {
        return Err(StoreError::Unauthorized);
    };
    if expires_at_ms <= now_ms {
        return Err(StoreError::Unauthorized);
    }
    let suggested = suggested.map(WorkerId::new);
    let worker_id = match &request.worker_id {
        Some(worker_id) => {
            if !worker_id.is_canonical() {
                return Err(StoreError::invalid("workerId is not canonical"));
            }
            worker_id.clone()
        }
        None => suggested.clone().unwrap_or_else(WorkerId::generate),
    };
    let enrolled = tx
        .query_row(
            "SELECT revoked FROM workers WHERE worker_id = ?1",
            params![worker_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if enrolled == Some(0) {
        return Err(StoreError::invalid("worker is already enrolled"));
    }
    let approval = if load_settings(&tx)?.require_worker_approval {
        WorkerApprovalStatus::Pending
    } else {
        WorkerApprovalStatus::Approved
    };
    let credential = random_hex(CREDENTIAL_BYTES);
    tx.execute(
        "INSERT INTO workers \
         (worker_id, token_sha256, revoked, approval, connection_epoch, inventory_revision, \
          inventory_json, reported_revision, reported_config_json, applied_revision, \
          applied_config_json, revision_high_water, last_seen_at_ms, created_at_ms, rotated_at_ms) \
         VALUES (?1, ?2, 0, ?3, 0, 0, NULL, NULL, NULL, NULL, NULL, 0, NULL, ?4, NULL) \
         ON CONFLICT(worker_id) DO UPDATE SET \
         token_sha256 = excluded.token_sha256, revoked = 0, approval = excluded.approval, \
         inventory_json = NULL, reported_revision = NULL, reported_config_json = NULL, \
         applied_revision = NULL, applied_config_json = NULL, rotated_at_ms = ?4",
        params![
            worker_id.as_str(),
            sha256_hex(credential.as_bytes()),
            approval_str(approval),
            now_ms
        ],
    )?;
    tx.execute(
        "UPDATE enrollment_codes SET consumed_at_ms = ?1 WHERE code_sha256 = ?2",
        params![now_ms, matched_hash],
    )?;
    let event = insert_event(
        &tx,
        EVENT_WORKER_PAIRED,
        &serde_json::json!({
            "workerId": worker_id.as_str(),
            "approval": match approval {
                WorkerApprovalStatus::Pending => "pending",
                _ => "approved",
            },
        }),
        now_ms,
    )?;
    tx.commit()?;
    Ok((
        PairWorkerResponse {
            hub_id: hub_id.to_string(),
            worker_id,
            credential,
        },
        event,
    ))
}

/// Constant-time bearer verification against every stored SHA-256 verifier.
pub fn authenticate_worker(conn: &Connection, bearer_token: &str) -> Result<WorkerId, StoreError> {
    if bearer_token.is_empty() {
        return Err(StoreError::Unauthorized);
    }
    let presented = sha256_hex(bearer_token.as_bytes());
    let mut stmt = conn.prepare("SELECT worker_id, token_sha256, revoked FROM workers")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut authenticated = None;
    for row in rows {
        let (worker_id, stored, revoked) = row?;
        let matches: bool = stored.as_bytes().ct_eq(presented.as_bytes()).into();
        if matches && revoked == 0 {
            authenticated = Some(WorkerId::new(worker_id));
        }
    }
    authenticated.ok_or(StoreError::Unauthorized)
}
