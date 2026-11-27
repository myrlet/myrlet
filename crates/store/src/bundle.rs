//! Atomic Task Bundle import: up to 100 `held` Tasks in one SQLite transaction.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use myrlet_protocol::{TaskBundleImportRequest, TaskBundleImportResponse, TaskId, TaskStatus};

use crate::error::StoreError;
use crate::events::insert_event;
use crate::idempotency::{check_idempotency, digest_request, record_idempotency};
use crate::tasks::{
    build_record, generate_task_id, insert_task, load_task, status_str, EVENT_TASK_CREATED,
    EVENT_TASK_QUEUED, MAX_TREE_DEPTH,
};
use crate::BundleOutcome;

const OP_BUNDLE: &str = "task-bundles.import";

/// Create every Task of a bundle as `held`; local references resolve inside one transaction.
pub fn import_bundle(
    conn: &mut Connection,
    idempotency_key: &str,
    request: &TaskBundleImportRequest,
    now_ms: i64,
) -> Result<BundleOutcome, StoreError> {
    request
        .validate()
        .map_err(|source| StoreError::invalid(source.to_string()))?;
    let digest = digest_request(request)?;
    let tx = conn.transaction()?;
    if let Some(hit) = check_idempotency(&tx, OP_BUNDLE, idempotency_key, &digest)? {
        let response: TaskBundleImportResponse = serde_json::from_str(&hit.response_json)?;
        return Ok((response, Vec::new(), true));
    }

    // Hub owns every ID; bundle-local `clientRef` strings resolve to them.
    let mut ref_to_id: BTreeMap<String, TaskId> = BTreeMap::new();
    let mut ids = Vec::with_capacity(request.tasks.len());
    for task in &request.tasks {
        let id = generate_task_id();
        if let Some(client_ref) = &task.client_ref {
            ref_to_id.insert(client_ref.clone(), id.clone());
        }
        ids.push(id);
    }
    let resolve = |reference: &TaskId| -> TaskId {
        ref_to_id
            .get(reference.as_str())
            .cloned()
            .unwrap_or_else(|| reference.clone())
    };
    let mut resolved = Vec::with_capacity(ids.len());
    for task in &request.tasks {
        let mut def = task.clone();
        def.parent_task_id = task.parent_task_id.as_ref().map(resolve);
        def.dependencies = task.dependencies.iter().map(resolve).collect();
        resolved.push(def);
    }
    let internal: HashMap<&TaskId, usize> = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();

    // Depth/root come from in-bundle parents or already stored Tasks; a cycle never resolves.
    let mut raw_depths: Vec<Option<u8>> = vec![None; ids.len()];
    let mut raw_roots: Vec<Option<TaskId>> = vec![None; ids.len()];
    for _ in 0..=ids.len() {
        let mut progressed = false;
        for index in 0..ids.len() {
            if raw_depths[index].is_some() {
                continue;
            }
            match &resolved[index].parent_task_id {
                None => {
                    raw_depths[index] = Some(1);
                    raw_roots[index] = Some(ids[index].clone());
                    progressed = true;
                }
                Some(parent_id) => match internal.get(parent_id) {
                    Some(&parent_index) => {
                        if let Some(parent_depth) = raw_depths[parent_index] {
                            raw_depths[index] = Some(parent_depth + 1);
                            raw_roots[index] = raw_roots[parent_index].clone();
                            progressed = true;
                        }
                    }
                    None => {
                        let parent = load_task(&tx, parent_id)?;
                        raw_depths[index] = Some(parent.depth + 1);
                        raw_roots[index] = Some(parent.root_task_id);
                        progressed = true;
                    }
                },
            }
        }
        if !progressed || raw_depths.iter().all(Option::is_some) {
            break;
        }
    }

    let mut placement = Vec::with_capacity(ids.len());
    for (index, depth) in raw_depths.into_iter().enumerate() {
        let Some(depth) = depth else {
            return Err(StoreError::invalid("bundle parent references form a cycle"));
        };
        if depth > MAX_TREE_DEPTH {
            return Err(StoreError::invalid("task tree cannot exceed 5 levels"));
        }
        let root = raw_roots[index]
            .clone()
            .ok_or_else(|| StoreError::invalid("bundle parent references form a cycle"))?;
        placement.push((depth, root));
    }

    // External dependencies must exist and stay siblings of the resolved Task.
    for def in &resolved {
        for dependency in &def.dependencies {
            if internal.contains_key(dependency) {
                continue;
            }
            let dep = load_task(&tx, dependency)?;
            if dep.parent_task_id != def.parent_task_id {
                return Err(StoreError::invalid(
                    "dependencies must share the same parent task",
                ));
            }
        }
    }

    let mut events = Vec::with_capacity(ids.len());
    let mut records = Vec::with_capacity(ids.len());
    let mut eligible_task_ids = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let (depth, root) = &placement[index];
        insert_task(
            &tx,
            id,
            &resolved[index],
            root,
            *depth,
            TaskStatus::Held,
            now_ms,
        )?;
        // Submission authorizes every Task in the bundle; readiness is computed after all
        // rows exist so in-bundle prerequisite references resolve against real statuses.
        let row =
            crate::tasks::recompute_readiness(&tx, &crate::tasks::load_task(&tx, id)?, now_ms)?;
        if row.status == TaskStatus::Queued {
            eligible_task_ids.push(id.clone());
        }
        events.push(insert_event(
            &tx,
            EVENT_TASK_CREATED,
            &serde_json::json!({
                "taskId": id.as_str(),
                "parentTaskId": resolved[index].parent_task_id.as_ref().map(TaskId::as_str),
                "rootTaskId": root.as_str(),
                "status": status_str(row.status),
                "version": 1,
            }),
            now_ms,
        )?);
        if row.status == TaskStatus::Queued {
            events.push(insert_event(
                &tx,
                EVENT_TASK_QUEUED,
                &serde_json::json!({
                    "taskId": id.as_str(),
                    "status": status_str(TaskStatus::Queued),
                    "version": row.version,
                }),
                now_ms,
            )?);
        }
        records.push(build_record(&tx, &crate::tasks::load_task(&tx, id)?)?);
    }

    let response = TaskBundleImportResponse {
        client_ref_to_task_id: ref_to_id,
        tasks: records,
        eligible_task_ids,
    };
    record_idempotency(
        &tx,
        OP_BUNDLE,
        idempotency_key,
        &digest,
        201,
        &serde_json::to_string(&response)?,
        now_ms,
    )?;
    tx.commit()?;
    Ok((response, events, false))
}
