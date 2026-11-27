//! Bounded Task read models: detail, tree traversal, Snapshot page, and claimability.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use myrlet_protocol::{
    BlockingDependency, GlobalSnapshot, SnapshotTaskSummary, SubtreeCounts, TaskEligibility,
    TaskId, TaskListResponse, TaskRecord, TaskTreeDirection, TaskTreeResponse, WorkerId,
    SNAPSHOT_SCHEMA_VERSION,
};

use crate::deliveries;
use crate::error::StoreError;
use crate::events::latest_seq;
use crate::inventory;
use crate::tasks::{blocked_dependencies, build_record, list_task_rows, load_task, TaskRow};

/// Derived per-status counts for the subtree rooted at `task_id` (inclusive).
pub fn subtree_counts(conn: &Connection, task_id: &TaskId) -> Result<SubtreeCounts, StoreError> {
    let raw = conn.query_row(
        "WITH RECURSIVE subtree(id) AS ( \
             SELECT id FROM tasks WHERE id = ?1 \
             UNION ALL \
             SELECT t.id FROM tasks t JOIN subtree s ON t.parent_task_id = s.id \
         ) \
         SELECT COUNT(*), \
                COALESCE(SUM(status = 'held'), 0), \
                COALESCE(SUM(status = 'queued'), 0), \
                COALESCE(SUM(status = 'dispatched'), 0), \
                COALESCE(SUM(status = 'finished'), 0), \
                COALESCE(SUM(status = 'cancelled'), 0) \
         FROM tasks WHERE id IN (SELECT id FROM subtree)",
        params![task_id.as_str()],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )?;
    Ok(SubtreeCounts {
        total: u64::try_from(raw.0).unwrap_or(0),
        held: u64::try_from(raw.1).unwrap_or(0),
        queued: u64::try_from(raw.2).unwrap_or(0),
        dispatched: u64::try_from(raw.3).unwrap_or(0),
        finished: u64::try_from(raw.4).unwrap_or(0),
        cancelled: u64::try_from(raw.5).unwrap_or(0),
    })
}

fn dependency_ids(conn: &Connection, task_id: &TaskId) -> Result<Vec<TaskId>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT depends_on_task_id FROM task_deps WHERE task_id = ?1 \
         ORDER BY depends_on_task_id ASC",
    )?;
    let rows = stmt.query_map(params![task_id.as_str()], |row| row.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(TaskId::new(row?));
    }
    Ok(ids)
}

/// One authoritative Task record with its Delivery, blocking dependencies, latest result,
/// and subtree counts. Tree and Snapshot reads never attach the result text.
pub fn get_task(conn: &Connection, task_id: &TaskId) -> Result<TaskRecord, StoreError> {
    let row = load_task(conn, task_id)?;
    let mut record = build_record(conn, &row)?;
    if let Some(delivery) = &record.current_delivery {
        if let Some((text, _, recorded)) = crate::result::delivery_result(conn, &delivery.id)? {
            record.result_markdown = Some(text);
            record.result_recorded_at_ms = Some(recorded);
        }
    }
    Ok(record)
}

/// Bounded ancestor or descendant traversal; there is no view-fingerprint cursor.
pub fn get_task_tree(
    conn: &Connection,
    task_id: &TaskId,
    direction: TaskTreeDirection,
    max_depth: u8,
    limit: usize,
) -> Result<TaskTreeResponse, StoreError> {
    let start = load_task(conn, task_id)?;
    let mut ids: Vec<TaskId> = Vec::new();
    let mut truncated = false;
    match direction {
        TaskTreeDirection::Ancestors => {
            let mut current = Some(start.clone());
            let mut level = 0u8;
            while let Some(row) = current.take() {
                if ids.len() >= limit {
                    truncated = true;
                    break;
                }
                ids.push(row.id.clone());
                let Some(parent_id) = row.parent_task_id.clone() else {
                    break;
                };
                if level >= max_depth {
                    truncated = true;
                    break;
                }
                level += 1;
                current = Some(load_task(conn, &parent_id)?);
            }
        }
        TaskTreeDirection::Descendants => {
            let mut stmt = conn.prepare(
                "WITH RECURSIVE subtree(id, depth) AS ( \
                     SELECT id, 0 FROM tasks WHERE id = ?1 \
                     UNION ALL \
                     SELECT t.id, s.depth + 1 FROM tasks t JOIN subtree s ON t.parent_task_id = s.id \
                     WHERE s.depth <= ?2 \
                 ) \
                 SELECT id, depth FROM subtree ORDER BY depth ASC, id ASC LIMIT ?3",
            )?;
            let rows = stmt.query_map(
                params![
                    task_id.as_str(),
                    i64::from(max_depth),
                    i64::try_from(limit).unwrap_or(i64::MAX).saturating_add(1)
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?;
            let mut found = Vec::new();
            for row in rows {
                found.push(row?);
            }
            for (raw_id, depth) in found {
                if depth > i64::from(max_depth) || ids.len() >= limit {
                    truncated = true;
                    continue;
                }
                ids.push(TaskId::new(raw_id));
            }
        }
    }
    let mut tasks = Vec::with_capacity(ids.len());
    for id in &ids {
        tasks.push(build_record(conn, &load_task(conn, id)?)?);
    }
    Ok(TaskTreeResponse {
        root_task_id: start.root_task_id,
        tasks,
        truncated,
    })
}

fn summary(conn: &Connection, row: &TaskRow) -> Result<SnapshotTaskSummary, StoreError> {
    Ok(SnapshotTaskSummary {
        id: row.id.clone(),
        parent_task_id: row.parent_task_id.clone(),
        root_task_id: row.root_task_id.clone(),
        depth: row.depth,
        source_task_id: row.source_task_id.clone(),
        dependencies: dependency_ids(conn, &row.id)?,
        blocked_by: blocked_dependencies(conn, &row.id)?,
        category: row.category.clone(),
        title: row.title.clone(),
        status: row.status,
        version: row.version,
        created_at_ms: row.created_at_ms,
        updated_at_ms: row.updated_at_ms,
        finished_at_ms: row.finished_at_ms,
        current_delivery: deliveries::current_delivery(conn, &row.id)?,
        subtree: subtree_counts(conn, &row.id)?,
    })
}

/// Consistent cluster read: Workers plus a bounded page of newest Tasks and the event cursor.
pub fn list_snapshot(
    conn: &Connection,
    online_map: &HashMap<WorkerId, u64>,
    max_tasks: usize,
    now_ms: i64,
) -> Result<GlobalSnapshot, StoreError> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))?;
    let total_tasks = u64::try_from(total).unwrap_or(0);
    let rows = list_task_rows(conn, max_tasks)?;
    let mut tasks = Vec::with_capacity(rows.len());
    for row in &rows {
        tasks.push(summary(conn, row)?);
    }
    Ok(GlobalSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        generated_at_ms: now_ms,
        event_cursor: latest_seq(conn)?,
        tasks_truncated: total_tasks > tasks.len() as u64,
        total_tasks,
        workers: inventory::list_workers(conn, online_map)?,
        tasks,
    })
}

/// Bounded newest-first Task summaries for `GET /v1/tasks` (no Worker inventory).
pub fn list_tasks(conn: &Connection, limit: usize) -> Result<TaskListResponse, StoreError> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))?;
    let total_tasks = u64::try_from(total).unwrap_or(0);
    let rows = list_task_rows(conn, limit)?;
    let mut tasks = Vec::with_capacity(rows.len());
    for row in &rows {
        tasks.push(summary(conn, row)?);
    }
    Ok(TaskListResponse {
        truncated: total_tasks > tasks.len() as u64,
        total_tasks,
        event_cursor: latest_seq(conn)?,
        tasks,
    })
}

/// Whether one Task is claimable right now, and which prerequisites still block it.
pub fn eligibility(conn: &Connection, task_id: &TaskId) -> Result<TaskEligibility, StoreError> {
    let row = load_task(conn, task_id)?;
    let blocked_by: Vec<BlockingDependency> = blocked_dependencies(conn, task_id)?;
    let claimable = row.status == myrlet_protocol::TaskStatus::Queued && blocked_by.is_empty();
    Ok(TaskEligibility {
        task_id: task_id.clone(),
        claimable,
        blocked_by,
    })
}

/// Oldest `queued` Tasks this Worker's advertised Slots could run, bounded by `limit`.
pub fn claimable_for_worker(
    conn: &Connection,
    inventory: Option<&myrlet_protocol::WorkerInventory>,
    limit: usize,
) -> Result<Vec<TaskId>, StoreError> {
    let Some(inventory) = inventory else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT id, definition_json FROM tasks WHERE status = 'queued' \
         ORDER BY created_at_ms ASC, id ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut claimable = Vec::new();
    for row in rows {
        let (id, definition_json) = row?;
        let definition: myrlet_protocol::TaskDefinition = serde_json::from_str(&definition_json)?;
        let eligible = !deliveries::task_has_delivery(conn, &TaskId::new(id.clone()))?
            && inventory
                .slots
                .iter()
                .any(|slot| slot.accepts(&definition.routing, None));
        if eligible {
            claimable.push(TaskId::new(id));
            if claimable.len() >= limit {
                break;
            }
        }
    }
    Ok(claimable)
}

/// Oldest `queued` Tasks with their immutable definition, for wakeup scheduling.
pub fn list_queued_tasks(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<(TaskId, myrlet_protocol::TaskDefinition)>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, definition_json FROM tasks WHERE status = 'queued' \
         ORDER BY created_at_ms ASC, id ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut queued = Vec::new();
    for row in rows {
        let (id, definition_json) = row?;
        queued.push((
            TaskId::new(id),
            serde_json::from_str::<myrlet_protocol::TaskDefinition>(&definition_json)?,
        ));
    }
    Ok(queued)
}
