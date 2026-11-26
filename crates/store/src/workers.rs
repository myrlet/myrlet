//! Worker enrollment, constant-time credential verification, and inventory persistence.

use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use subtle::ConstantTimeEq;

use myrlet_protocol::{
    CreateEnrollmentCodeResponse, HubEventEnvelope, PairWorkerRequest, PairWorkerResponse,
    RotateWorkerCredentialResponse, WorkerId,
};

use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::{hex, sha256_hex};

const CREDENTIAL_BYTES: usize = 32;
const ENROLLMENT_CODE_BYTES: usize = 16;
const DEFAULT_TTL_SECS: u64 = 900;
const MAX_TTL_SECS: u64 = 86_400;

pub const EVENT_WORKER_PAIRED: &str = "worker.paired";
pub const EVENT_WORKER_ROTATED: &str = "worker.rotated";
pub const EVENT_WORKER_REVOKED: &str = "worker.revoked";

/// Stored Worker row without the derived Snapshot fields.
#[derive(Debug, Clone)]
pub struct WorkerRow {
    pub worker_id: WorkerId,
    pub revoked: bool,
    pub connection_epoch: u64,
    pub inventory_json: Option<String>,
    pub last_seen_at_ms: Option<i64>,
}

type RawWorker = (String, i64, i64, Option<String>, Option<i64>);

fn map_worker(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawWorker> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn to_worker_row(raw: RawWorker) -> Result<WorkerRow, StoreError> {
    Ok(WorkerRow {
        worker_id: WorkerId::new(raw.0),
        revoked: raw.1 != 0,
        connection_epoch: u64::try_from(raw.2)
            .map_err(|_| StoreError::invalid("stored connection epoch is negative"))?,
        inventory_json: raw.3,
        last_seen_at_ms: raw.4,
    })
}

const WORKER_COLUMNS: &str =
    "worker_id, revoked, connection_epoch, inventory_json, last_seen_at_ms";

pub fn load_worker(conn: &Connection, worker_id: &WorkerId) -> Result<WorkerRow, StoreError> {
    let sql = format!("SELECT {WORKER_COLUMNS} FROM workers WHERE worker_id = ?1");
    let raw = conn
        .query_row(&sql, params![worker_id.as_str()], map_worker)
        .optional()?
        .ok_or(StoreError::NotFound)?;
    to_worker_row(raw)
}

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex(&bytes)
}

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
    let credential = random_hex(CREDENTIAL_BYTES);
    tx.execute(
        "INSERT INTO workers \
         (worker_id, token_sha256, revoked, connection_epoch, inventory_revision, inventory_json, last_seen_at_ms, created_at_ms, rotated_at_ms) \
         VALUES (?1, ?2, 0, 0, 0, NULL, NULL, ?3, NULL) \
         ON CONFLICT(worker_id) DO UPDATE SET \
         token_sha256 = excluded.token_sha256, revoked = 0, rotated_at_ms = ?3",
        params![worker_id.as_str(), sha256_hex(credential.as_bytes()), now_ms],
    )?;
    tx.execute(
        "UPDATE enrollment_codes SET consumed_at_ms = ?1 WHERE code_sha256 = ?2",
        params![now_ms, matched_hash],
    )?;
    let event = insert_event(
        &tx,
        EVENT_WORKER_PAIRED,
        &serde_json::json!({ "workerId": worker_id.as_str() }),
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

/// Rotate a Worker credential; the previous verifier and the live WSS epoch are both fenced.
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

/// Revoke a Worker and fence its live WSS session.
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

/// Every enrolled Worker row, ordered by `workerId`.
pub fn list_worker_rows(conn: &Connection) -> Result<Vec<WorkerRow>, StoreError> {
    let sql = format!("SELECT {WORKER_COLUMNS} FROM workers ORDER BY worker_id ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_worker)?;
    let mut workers = Vec::new();
    for row in rows {
        workers.push(to_worker_row(row?)?);
    }
    Ok(workers)
}
