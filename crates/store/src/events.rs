//! Append-only Hub event log: one row written inside every committed state transition.

use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

use myrlet_protocol::HubEventEnvelope;

use crate::error::StoreError;

/// Append one event inside the caller's transaction and return its committed envelope.
pub fn insert_event(
    tx: &Transaction<'_>,
    kind: &str,
    payload: &Value,
    now_ms: i64,
) -> Result<HubEventEnvelope, StoreError> {
    tx.execute(
        "INSERT INTO hub_events (kind, payload_json, created_at_ms) VALUES (?1, ?2, ?3)",
        params![kind, payload.to_string(), now_ms],
    )?;
    Ok(HubEventEnvelope {
        seq: u64::try_from(tx.last_insert_rowid())
            .map_err(|_| StoreError::invalid("event sequence is negative"))?,
        kind: kind.to_string(),
        created_at_ms: now_ms,
        payload: payload.clone(),
    })
}

/// Newest committed event sequence, or `0` when the log is empty.
pub fn latest_seq(conn: &Connection) -> Result<u64, StoreError> {
    let seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM hub_events", [], |row| {
        row.get(0)
    })?;
    u64::try_from(seq).map_err(|_| StoreError::invalid("event sequence is negative"))
}

/// Backfill events after `after_seq`, failing closed when the cursor predates retention.
///
/// The newest `retention_window` events are retained; an older cursor cannot be replayed
/// and the caller must reload the Snapshot instead.
pub fn list_events_after(
    conn: &Connection,
    after_seq: u64,
    limit: usize,
    retention_window: usize,
) -> Result<Vec<HubEventEnvelope>, StoreError> {
    let latest = latest_seq(conn)?;
    if after_seq < latest.saturating_sub(retention_window as u64) {
        return Err(StoreError::CursorExpired);
    }
    let after = i64::try_from(after_seq).map_err(|_| StoreError::invalid("cursor too large"))?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = conn.prepare(
        "SELECT seq, kind, payload_json, created_at_ms FROM hub_events \
         WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after, limit], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut events = Vec::new();
    for row in rows {
        let (seq, kind, payload_json, created_at_ms) = row?;
        events.push(HubEventEnvelope {
            seq: u64::try_from(seq).map_err(|_| StoreError::invalid("sequence is negative"))?,
            kind,
            created_at_ms,
            payload: serde_json::from_str(&payload_json)?,
        });
    }
    Ok(events)
}
