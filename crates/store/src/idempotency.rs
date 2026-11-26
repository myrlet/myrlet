//! `Idempotency-Key` bookkeeping, kept inside the mutation's own transaction.

use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::StoreError;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// A previously committed response replayed for a repeated `Idempotency-Key`.
pub struct IdempotencyHit {
    pub status_code: u16,
    pub response_json: String,
}

/// Lowercase hex encoding of arbitrary bytes.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// SHA-256 verifier of a bearer token or pairing code; plaintext is never stored.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

/// Digest the canonical JSON of a request body so replays with different bodies conflict.
pub fn digest_request<T: Serialize>(request: &T) -> Result<String, StoreError> {
    Ok(sha256_hex(&serde_json::to_vec(request)?))
}

/// Return the stored response for `key`, or fail when the same key carried another request.
pub fn check_idempotency(
    tx: &Transaction<'_>,
    op: &str,
    key: &str,
    request_sha256: &str,
) -> Result<Option<IdempotencyHit>, StoreError> {
    let stored = tx
        .query_row(
            "SELECT request_sha256, status_code, response_json \
             FROM idempotency_keys WHERE op = ?1 AND key = ?2",
            params![op, key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((stored_sha256, status_code, response_json)) = stored else {
        return Ok(None);
    };
    if stored_sha256 != request_sha256 {
        return Err(StoreError::IdempotencyConflict);
    }
    Ok(Some(IdempotencyHit {
        status_code: u16::try_from(status_code).unwrap_or(200),
        response_json,
    }))
}

/// Persist the committed response of a mutating request under its `Idempotency-Key`.
pub fn record_idempotency(
    tx: &Transaction<'_>,
    op: &str,
    key: &str,
    request_sha256: &str,
    status_code: u16,
    response_json: &str,
    now_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO idempotency_keys (op, key, request_sha256, status_code, response_json, created_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            op,
            key,
            request_sha256,
            i64::from(status_code),
            response_json,
            now_ms
        ],
    )?;
    Ok(())
}
