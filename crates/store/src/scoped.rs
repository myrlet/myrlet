//! Worker-tier Task visibility: the assigned Task in full plus one-hop related summaries.
//!
//! A Task is assigned to a Worker only after a Delivery with that `workerId` is committed;
//! `preferredWorkerId` grants nothing. Related visibility is exactly one hop (direct
//! parent, direct children, declared dependencies, and direct dependents) and never
//! expands recursively. Related Tasks are projected as [`RelatedTaskSummary`] so prompts,
//! inputs, metadata, Delivery, Progress, and Result never leak.

use rusqlite::{params, Connection};

use myrlet_protocol::{
    RelatedTaskSummary, TaskEligibility, TaskId, TaskReadResponse, TaskTreeResponse, WorkerId,
};

use crate::error::StoreError;
use crate::reads;
use crate::tasks::{blocked_dependencies, build_record, load_task};

/// Maximum number of one-hop related Tasks returned with a Worker-scoped read.
///
/// It matches the bounded Task-tree read depth of the V1 contract: a page large enough for
/// realistic sibling sets while keeping a response under a heavy fan-out bounded.
pub const MAX_RELATED_SUMMARIES: usize = 200;

/// Whether the given Worker holds a Delivery for this Task.
pub fn worker_has_delivery_for(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM deliveries WHERE task_id = ?1 AND worker_id = ?2",
        params![task_id.as_str(), worker_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Whether `task_id` is a direct relative of a Task assigned to this Worker.
///
/// The check is one SQL `EXISTS`: it only looks at the Worker's committed Deliveries (any
/// state, never just the active one) and the requested Task's one-hop neighborhood. It
/// never walks outward from the request, so a Worker cannot follow the graph past its own
/// assignment.
fn related_to_assignment(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<bool, StoreError> {
    let visible: i64 = conn.query_row(
        "SELECT EXISTS ( \
             SELECT 1 FROM deliveries d JOIN tasks t ON t.id = d.task_id \
             WHERE d.worker_id = ?1 AND ( \
                 t.parent_task_id = ?2 \
                 OR t.id = (SELECT parent_task_id FROM tasks WHERE id = ?2) \
                 OR EXISTS (SELECT 1 FROM task_deps \
                            WHERE task_id = t.id AND depends_on_task_id = ?2) \
                 OR EXISTS (SELECT 1 FROM task_deps \
                            WHERE task_id = ?2 AND depends_on_task_id = t.id) \
             ) \
         )",
        params![worker_id.as_str(), task_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(visible != 0)
}

/// Bounded, deterministically ordered one-hop related-Task IDs for `task_id`.
///
/// Ordering is `(createdAtMs, id)` — the same sort as the global Task list — so repeated
/// reads return the same page. One extra row is fetched to report truncation without a
/// second `COUNT` query. The Task itself is always excluded.
fn related_ids(
    conn: &Connection,
    task_id: &TaskId,
    limit: usize,
) -> Result<(Vec<TaskId>, bool), StoreError> {
    let fetch = limit.saturating_add(1);
    let mut stmt = conn.prepare(
        "WITH related(id) AS ( \
             SELECT parent_task_id FROM tasks WHERE id = ?1 AND parent_task_id IS NOT NULL \
             UNION SELECT id FROM tasks WHERE parent_task_id = ?1 \
             UNION SELECT depends_on_task_id FROM task_deps WHERE task_id = ?1 \
             UNION SELECT task_id FROM task_deps WHERE depends_on_task_id = ?1 \
         ) \
         SELECT t.id FROM tasks t JOIN related r ON r.id = t.id \
         WHERE t.id != ?1 \
         ORDER BY t.created_at_ms ASC, t.id ASC \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(
        params![task_id.as_str(), i64::try_from(fetch).unwrap_or(i64::MAX)],
        |row| row.get::<_, String>(0),
    )?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(TaskId::new(row?));
    }
    let truncated = ids.len() > limit;
    ids.truncate(limit);
    Ok((ids, truncated))
}

/// The limited one-hop projection of one related Task.
fn related_summary(conn: &Connection, task_id: &TaskId) -> Result<RelatedTaskSummary, StoreError> {
    let row = load_task(conn, task_id)?;
    Ok(RelatedTaskSummary {
        id: row.id.clone(),
        parent_task_id: row.parent_task_id.clone(),
        depth: row.depth,
        dependencies: reads::dependency_ids(conn, &row.id)?,
        blocked_by: blocked_dependencies(conn, &row.id)?,
        title: row.title,
        status: row.status,
        version: row.version,
    })
}

/// Bounded related summaries plus their truncation flag.
fn related_summaries(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<(Vec<RelatedTaskSummary>, bool), StoreError> {
    let (ids, truncated) = related_ids(conn, task_id, MAX_RELATED_SUMMARIES)?;
    let mut summaries = Vec::with_capacity(ids.len());
    for id in &ids {
        summaries.push(related_summary(conn, id)?);
    }
    Ok((summaries, truncated))
}

/// Worker-tier `GET /v1/tasks/{id}`.
///
/// The assigned Task is returned in full with its one-hop relatives. A directly related
/// Task is returned as its limited summary, and its own relatives are never expanded.
/// A Task outside the boundary yields `None`, which the Hub reports as `404`.
pub fn task_read_for_worker(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<Option<TaskReadResponse>, StoreError> {
    if worker_has_delivery_for(conn, worker_id, task_id)? {
        let (related, related_truncated) = related_summaries(conn, task_id)?;
        return Ok(Some(TaskReadResponse {
            task: Some(reads::get_task(conn, task_id)?),
            summary: None,
            related,
            related_truncated,
        }));
    }
    if !related_to_assignment(conn, worker_id, task_id)? {
        return Ok(None);
    }
    Ok(Some(TaskReadResponse {
        task: None,
        summary: Some(related_summary(conn, task_id)?),
        related: Vec::new(),
        related_truncated: false,
    }))
}

/// Worker-tier `GET /v1/tasks/{id}/tree`: anchor to the assigned Task and return it plus
/// one-hop related summaries. A Task outside the boundary yields `None`.
pub fn task_tree_for_worker(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<Option<TaskTreeResponse>, StoreError> {
    if !worker_has_delivery_for(conn, worker_id, task_id)? {
        return Ok(None);
    }
    let row = load_task(conn, task_id)?;
    let (related, related_truncated) = related_summaries(conn, task_id)?;
    Ok(Some(TaskTreeResponse {
        root_task_id: row.root_task_id.clone(),
        tasks: vec![build_record(conn, &row)?],
        related,
        related_truncated,
        truncated: false,
    }))
}

/// Worker-tier `GET /v1/tasks/{id}/eligibility`: own Task or directly related Task only.
pub fn eligibility_for_worker(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<Option<TaskEligibility>, StoreError> {
    let visible = worker_has_delivery_for(conn, worker_id, task_id)?
        || related_to_assignment(conn, worker_id, task_id)?;
    if !visible {
        return Ok(None);
    }
    reads::eligibility(conn, task_id).map(Some)
}

/// `EXPLAIN QUERY PLAN` for the bounded related-page query, for index assertions.
#[cfg(any(test, feature = "test-support"))]
pub fn explain_related_lookup(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare(
        "EXPLAIN QUERY PLAN          WITH related(id) AS (              SELECT parent_task_id FROM tasks WHERE id = ?1 AND parent_task_id IS NOT NULL              UNION SELECT id FROM tasks WHERE parent_task_id = ?1              UNION SELECT depends_on_task_id FROM task_deps WHERE task_id = ?1              UNION SELECT task_id FROM task_deps WHERE depends_on_task_id = ?1          )          SELECT t.id FROM tasks t JOIN related r ON r.id = t.id          WHERE t.id != ?1 ORDER BY t.created_at_ms ASC, t.id ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![task_id.as_str(), 10i64], |row| {
        row.get::<_, String>(3)
    })?;
    let mut plan = Vec::new();
    for row in rows {
        plan.push(row?);
    }
    Ok(plan)
}

/// `EXPLAIN QUERY PLAN` for the assignment-authorization `EXISTS`, for index assertions.
#[cfg(any(test, feature = "test-support"))]
pub fn explain_assignment_lookup(
    conn: &Connection,
    worker_id: &WorkerId,
) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare(
        "EXPLAIN QUERY PLAN          SELECT EXISTS (              SELECT 1 FROM deliveries d JOIN tasks t ON t.id = d.task_id              WHERE d.worker_id = ?1 AND t.parent_task_id = ?2          )",
    )?;
    let rows = stmt.query_map(params![worker_id.as_str(), "task_x"], |row| {
        row.get::<_, String>(3)
    })?;
    let mut plan = Vec::new();
    for row in rows {
        plan.push(row?);
    }
    Ok(plan)
}
