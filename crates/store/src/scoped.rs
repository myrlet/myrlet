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

/// Direct related-Task IDs in stable order: parent, children, dependencies, dependents.
fn related_ids(conn: &Connection, task_id: &TaskId) -> Result<Vec<TaskId>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT parent_task_id FROM tasks WHERE id = ?1 AND parent_task_id IS NOT NULL \
         UNION SELECT id FROM tasks WHERE parent_task_id = ?1 \
         UNION SELECT depends_on_task_id FROM task_deps WHERE task_id = ?1 \
         UNION SELECT task_id FROM task_deps WHERE depends_on_task_id = ?1 \
         ORDER BY 1 ASC",
    )?;
    let rows = stmt.query_map(params![task_id.as_str()], |row| row.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for row in rows {
        let id = TaskId::new(row?);
        if &id != task_id {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Whether `task_id` is a direct relative of any Task assigned to this Worker.
///
/// Visibility is anchored on the Tasks the Worker actually holds a Delivery for, never on
/// the requested Task's own neighborhood, so a Worker cannot walk the graph outward.
fn is_related_to_assignment(
    conn: &Connection,
    worker_id: &WorkerId,
    task_id: &TaskId,
) -> Result<bool, StoreError> {
    let mut stmt = conn.prepare("SELECT task_id FROM deliveries WHERE worker_id = ?1")?;
    let rows = stmt.query_map(params![worker_id.as_str()], |row| row.get::<_, String>(0))?;
    for row in rows {
        let assigned = TaskId::new(row?);
        if related_ids(conn, &assigned)?.contains(task_id) {
            return Ok(true);
        }
    }
    Ok(false)
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

fn related_summaries(
    conn: &Connection,
    task_id: &TaskId,
) -> Result<Vec<RelatedTaskSummary>, StoreError> {
    related_ids(conn, task_id)?
        .iter()
        .map(|id| related_summary(conn, id))
        .collect()
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
        return Ok(Some(TaskReadResponse {
            task: Some(reads::get_task(conn, task_id)?),
            summary: None,
            related: related_summaries(conn, task_id)?,
        }));
    }
    if !is_related_to_assignment(conn, worker_id, task_id)? {
        return Ok(None);
    }
    Ok(Some(TaskReadResponse {
        task: None,
        summary: Some(related_summary(conn, task_id)?),
        related: Vec::new(),
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
    Ok(Some(TaskTreeResponse {
        root_task_id: row.root_task_id.clone(),
        tasks: vec![build_record(conn, &row)?],
        related: related_summaries(conn, task_id)?,
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
        || is_related_to_assignment(conn, worker_id, task_id)?;
    if !visible {
        return Ok(None);
    }
    reads::eligibility(conn, task_id).map(Some)
}
