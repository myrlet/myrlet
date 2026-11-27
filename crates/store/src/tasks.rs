//! Durable Task rows: the shared row model, insert/transition helpers, and derived records.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use myrlet_protocol::{
    BlockingDependency, TaskDefinition, TaskId, TaskRecord, TaskStatus, TaskValidationError,
};

use crate::deliveries;
use crate::error::StoreError;
use crate::reads;

pub const MAX_TREE_DEPTH: u8 = 5;

pub const EVENT_TASK_CREATED: &str = "task.created";
pub const EVENT_TASK_QUEUED: &str = "task.queued";
pub const EVENT_TASK_UPDATED: &str = "task.updated";
pub const EVENT_TASK_DISPATCHED: &str = "task.dispatched";
pub const EVENT_TASK_FINISHED: &str = "task.finished";
pub const EVENT_TASK_CANCELLED: &str = "task.cancelled";

const TASK_COLUMNS: &str =
    "id, parent_task_id, root_task_id, depth, source_task_id, status, version, \
     category, title, definition_json, created_at_ms, updated_at_ms, finished_at_ms";

/// Canonical wire token for a Task status.
pub fn status_str(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Held => "held",
        TaskStatus::Queued => "queued",
        TaskStatus::Dispatched => "dispatched",
        TaskStatus::Finished => "finished",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn parse_status(raw: &str) -> Result<TaskStatus, StoreError> {
    match raw {
        "held" => Ok(TaskStatus::Held),
        "queued" => Ok(TaskStatus::Queued),
        "dispatched" => Ok(TaskStatus::Dispatched),
        "finished" => Ok(TaskStatus::Finished),
        "cancelled" => Ok(TaskStatus::Cancelled),
        other => Err(StoreError::invalid(format!("unknown task status {other}"))),
    }
}

pub fn generate_task_id() -> TaskId {
    TaskId::new(format!("task_{}", uuid::Uuid::new_v4().simple()))
}

/// Stored Task row before Delivery and subtree enrichment.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: TaskId,
    pub parent_task_id: Option<TaskId>,
    pub root_task_id: TaskId,
    pub depth: u8,
    pub source_task_id: Option<TaskId>,
    pub status: TaskStatus,
    pub version: u64,
    pub category: String,
    pub title: String,
    pub definition_json: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub finished_at_ms: Option<i64>,
}

impl TaskRow {
    pub fn definition(&self) -> Result<TaskDefinition, StoreError> {
        Ok(serde_json::from_str(&self.definition_json)?)
    }
}

type RawTask = (
    String,
    Option<String>,
    String,
    i64,
    Option<String>,
    String,
    i64,
    String,
    String,
    String,
    i64,
    i64,
    Option<i64>,
);

fn map_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawTask> {
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
        row.get(11)?,
        row.get(12)?,
    ))
}

fn to_task_row(raw: RawTask) -> Result<TaskRow, StoreError> {
    Ok(TaskRow {
        id: TaskId::new(raw.0),
        parent_task_id: raw.1.map(TaskId::new),
        root_task_id: TaskId::new(raw.2),
        depth: u8::try_from(raw.3)
            .map_err(|_| StoreError::invalid("stored depth is out of range"))?,
        source_task_id: raw.4.map(TaskId::new),
        status: parse_status(&raw.5)?,
        version: u64::try_from(raw.6)
            .map_err(|_| StoreError::invalid("stored version is negative"))?,
        category: raw.7,
        title: raw.8,
        definition_json: raw.9,
        created_at_ms: raw.10,
        updated_at_ms: raw.11,
        finished_at_ms: raw.12,
    })
}

pub fn load_task(conn: &Connection, task_id: &TaskId) -> Result<TaskRow, StoreError> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1");
    let raw = conn
        .query_row(&sql, params![task_id.as_str()], map_task)
        .optional()?
        .ok_or(StoreError::NotFound)?;
    to_task_row(raw)
}

/// Newest Tasks first; the Snapshot page bound is applied by the caller.
pub fn list_task_rows(conn: &Connection, limit: usize) -> Result<Vec<TaskRow>, StoreError> {
    let sql =
        format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY created_at_ms DESC, id DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], map_task)?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(to_task_row(row?)?);
    }
    Ok(tasks)
}

/// Insert one Task with the given submission status, plus its sibling dependency rows.
pub fn insert_task(
    tx: &Transaction<'_>,
    id: &TaskId,
    def: &TaskDefinition,
    root_task_id: &TaskId,
    depth: u8,
    status: TaskStatus,
    now_ms: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO tasks (id, parent_task_id, root_task_id, depth, source_task_id, status, version, \
         category, title, definition_json, created_at_ms, updated_at_ms, finished_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?9, ?10, ?10, NULL)",
        params![
            id.as_str(),
            def.parent_task_id.as_ref().map(TaskId::as_str),
            root_task_id.as_str(),
            i64::from(depth),
            def.source_task_id.as_ref().map(TaskId::as_str),
            status_str(status),
            def.category,
            def.title,
            serde_json::to_string(def)?,
            now_ms
        ],
    )?;
    for dependency in &def.dependencies {
        tx.execute(
            "INSERT INTO task_deps (task_id, depends_on_task_id) VALUES (?1, ?2)",
            params![id.as_str(), dependency.as_str()],
        )?;
    }
    Ok(())
}

fn write_task_state(
    tx: &Transaction<'_>,
    row: &TaskRow,
    next: TaskStatus,
    now_ms: i64,
) -> Result<u64, StoreError> {
    let version = row.version + 1;
    let finished_at_ms = if next == TaskStatus::Finished {
        Some(now_ms)
    } else {
        row.finished_at_ms
    };
    tx.execute(
        "UPDATE tasks SET status = ?1, version = ?2, updated_at_ms = ?3, finished_at_ms = ?4 \
         WHERE id = ?5",
        params![
            status_str(next),
            i64::try_from(version).unwrap_or(i64::MAX),
            now_ms,
            finished_at_ms,
            row.id.as_str()
        ],
    )?;
    Ok(version)
}

/// Apply one legal Task transition, bumping `version` for optimistic concurrency.
pub fn transition_task(
    tx: &Transaction<'_>,
    row: &TaskRow,
    next: TaskStatus,
    now_ms: i64,
) -> Result<u64, StoreError> {
    if !row.status.can_transition_to(next) {
        return Err(StoreError::InvalidTransition);
    }
    write_task_state(tx, row, next, now_ms)
}

/// Apply a contract-implied state outside the generic transition table: closing a
/// coordination-only Task an Owner finishes directly.
pub fn force_task_state(
    tx: &Transaction<'_>,
    row: &TaskRow,
    next: TaskStatus,
    now_ms: i64,
) -> Result<u64, StoreError> {
    write_task_state(tx, row, next, now_ms)
}

/// Build the authoritative Task record: definition, current Delivery, blocking
/// dependencies, and subtree counts.
///
/// The latest result is attached only by the single-Task read in [`crate::reads`].
pub fn build_record(conn: &Connection, row: &TaskRow) -> Result<TaskRecord, StoreError> {
    Ok(TaskRecord {
        id: row.id.clone(),
        root_task_id: row.root_task_id.clone(),
        depth: row.depth,
        status: row.status,
        version: row.version,
        created_at_ms: row.created_at_ms,
        updated_at_ms: row.updated_at_ms,
        finished_at_ms: row.finished_at_ms,
        task: row.definition()?,
        current_delivery: deliveries::current_delivery(conn, &row.id)?,
        blocked_by: blocked_dependencies(conn, &row.id)?,
        result_markdown: None,
        result_recorded_at_ms: None,
        subtree: reads::subtree_counts(conn, &row.id)?,
    })
}

/// Declared dependencies whose status is not `finished`, with that status.
pub fn blocked_dependencies(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<Vec<BlockingDependency>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.status FROM task_deps d JOIN tasks t ON t.id = d.depends_on_task_id \
         WHERE d.task_id = ?1 ORDER BY t.id ASC",
    )?;
    let rows = stmt.query_map(params![task_id.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut blocking = Vec::new();
    for row in rows {
        let (id, status) = row?;
        let status = parse_status(&status)?;
        if status != TaskStatus::Finished {
            blocking.push(BlockingDependency {
                task_id: TaskId::new(id),
                status,
            });
        }
    }
    Ok(blocking)
}

/// Deterministic submission gate: every declared dependency is already `finished`.
pub fn are_dependencies_satisfied(conn: &Connection, task_id: &TaskId) -> Result<bool, StoreError> {
    Ok(blocked_dependencies(conn, task_id)?.is_empty())
}

/// Resolve the status a Task should have given its dependencies and current claim.
///
/// `finished` and `cancelled` are terminal and never recomputed.
pub fn recompute_readiness(
    tx: &Transaction<'_>,
    row: &TaskRow,
    now_ms: i64,
) -> Result<TaskRow, StoreError> {
    if row.status.is_terminal() || row.status == TaskStatus::Dispatched {
        return Ok(row.clone());
    }
    let satisfied = are_dependencies_satisfied(tx, &row.id)?;
    let target = TaskDefinition::submitted_status(&row.definition()?, satisfied);
    if target == row.status {
        return Ok(row.clone());
    }
    write_task_state(tx, row, target, now_ms)?;
    load_task(tx, &row.id)
}

/// Validate one client-supplied definition, keeping the message stable for clients.
pub fn validate_definition(def: &TaskDefinition) -> Result<(), StoreError> {
    def.validate()
        .map_err(|source: TaskValidationError| StoreError::invalid(source.to_string()))
}
