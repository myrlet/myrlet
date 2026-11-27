//! Hub SQLite schema: eight tables, the query indexes, and no triggers.

use rusqlite::Connection;

/// Apply the durability pragmas and create the V1 schema on a fresh or existing file.
pub fn initialize(conn: &Connection) -> Result<(), rusqlite::Error> {
    for pragma in [
        "PRAGMA journal_mode = WAL",
        "PRAGMA synchronous = FULL",
        "PRAGMA foreign_keys = ON",
        "PRAGMA busy_timeout = 5000",
    ] {
        apply(conn, pragma)?;
    }
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

/// Execute one statement and drain any result rows, since pragmas differ in what they report.
fn apply(conn: &Connection, sql: &str) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while rows.next()?.is_some() {}
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id                 TEXT PRIMARY KEY NOT NULL,
    parent_task_id     TEXT,
    root_task_id       TEXT NOT NULL,
    depth              INTEGER NOT NULL,
    source_task_id     TEXT,
    status             TEXT NOT NULL,
    version            INTEGER NOT NULL,
    category           TEXT NOT NULL,
    title              TEXT NOT NULL,
    definition_json    TEXT NOT NULL,
    queue_request_json TEXT,
    queued_at_ms       INTEGER,
    created_at_ms      INTEGER NOT NULL,
    updated_at_ms      INTEGER NOT NULL,
    finished_at_ms     INTEGER
);

CREATE TABLE IF NOT EXISTS task_deps (
    task_id            TEXT NOT NULL,
    depends_on_task_id TEXT NOT NULL,
    PRIMARY KEY (task_id, depends_on_task_id)
);

CREATE TABLE IF NOT EXISTS workers (
    worker_id          TEXT PRIMARY KEY NOT NULL,
    token_sha256       TEXT NOT NULL,
    revoked            INTEGER NOT NULL DEFAULT 0,
    connection_epoch   INTEGER NOT NULL DEFAULT 0,
    inventory_revision INTEGER NOT NULL DEFAULT 0,
    inventory_json     TEXT,
    last_seen_at_ms    INTEGER,
    created_at_ms      INTEGER NOT NULL,
    rotated_at_ms      INTEGER
);

CREATE TABLE IF NOT EXISTS enrollment_codes (
    code_sha256         TEXT PRIMARY KEY NOT NULL,
    suggested_worker_id TEXT,
    expires_at_ms       INTEGER NOT NULL,
    consumed_at_ms      INTEGER,
    created_at_ms       INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS deliveries (
    id               TEXT PRIMARY KEY NOT NULL,
    task_id          TEXT NOT NULL,
    worker_id        TEXT NOT NULL,
    slot_id          TEXT NOT NULL,
    executor_id      TEXT,
    connection_epoch INTEGER NOT NULL,
    state            TEXT NOT NULL,
    rejection_code   TEXT,
    cancel_state     TEXT,
    cancel_reason    TEXT,
    created_at_ms    INTEGER NOT NULL,
    updated_at_ms    INTEGER NOT NULL,
    result_markdown  TEXT,
    result_recorded_at_ms INTEGER
);

CREATE TABLE IF NOT EXISTS delivery_progress (
    delivery_id         TEXT PRIMARY KEY NOT NULL,
    task_id             TEXT NOT NULL,
    text                TEXT NOT NULL,
    observed_at_ms      INTEGER NOT NULL,
    first_received_at_ms INTEGER NOT NULL,
    received_at_ms      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hub_events (
    seq           INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT NOT NULL,
    payload_json  TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    op            TEXT NOT NULL,
    key           TEXT NOT NULL,
    request_sha256 TEXT NOT NULL,
    status_code   INTEGER NOT NULL,
    response_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (op, key)
);

CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_root ON tasks(root_task_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status_queued ON tasks(status, queued_at_ms);
CREATE INDEX IF NOT EXISTS idx_deliveries_task ON deliveries(task_id, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_deliveries_worker_state ON deliveries(worker_id, state);
"#;
