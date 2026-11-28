//! Task control-plane handlers: submission, reads, pending edits, cancel, and finish.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{
    CancelTaskRequest, DeliveryView, FinishTaskRequest, SubmitTaskResponse,
    TaskBundleImportRequest, TaskCancelPayload, TaskDefinition, TaskEligibility, TaskId,
    TaskStatus, TaskTreeDirection, UpdateTaskRequest, WorkerWireKind,
};

use crate::error::ApiError;
use crate::http::{authorize_command, read_bounded_json, require_reader, run_bounded};
use crate::state::{now_ms, HubState};
use crate::wakeup::spawn_wakeup;
use crate::ws::build_hub_frame;

const MAX_TREE_DEPTH: u8 = 5;
const DEFAULT_TREE_LIMIT: usize = 200;
const MAX_TREE_LIMIT: usize = 500;
const DEFAULT_TASK_LIST_LIMIT: usize = 500;
const MAX_TASK_LIST_LIMIT: usize = 2_000;

/// `POST /v1/tasks`: submit one Task, which authorizes it for execution.
pub async fn create_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let definition: TaskDefinition =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let (record, event, _) = state.store.create_task(key, definition, now_ms()).await?;
        state.publish_event(event);
        let eligible = record.status == TaskStatus::Queued;
        if eligible {
            spawn_wakeup(&state);
        }
        let response = SubmitTaskResponse {
            task: record,
            eligible,
        };
        Ok((StatusCode::CREATED, Json(response)).into_response())
    })
    .await
}

pub async fn import_bundle(
    State(state): State<HubState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: TaskBundleImportRequest =
            read_bounded_json(body, state.config.max_bundle_bytes).await?;
        let (response, events, _) = state.store.import_bundle(key, request, now_ms()).await?;
        for event in events {
            state.publish_event(Some(event));
        }
        if !response.eligible_task_ids.is_empty() {
            spawn_wakeup(&state);
        }
        Ok((StatusCode::CREATED, Json(response)).into_response())
    })
    .await
}

/// `GET /v1/tasks`: bounded newest-first Task summaries. Owners and Workers may read.
pub async fn list_tasks(
    State(state): State<HubState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let limit = match query.get("limit") {
        Some(raw) => bounded(raw, "limit")?.clamp(1, MAX_TASK_LIST_LIMIT),
        None => DEFAULT_TASK_LIST_LIMIT,
    };
    Ok(Json(state.store.list_tasks(limit).await?).into_response())
}

pub async fn get_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let record = state.store.get_task(TaskId::new(task_id)).await?;
    Ok(Json(record).into_response())
}

/// `GET /v1/tasks/{id}/eligibility`: whether this Task can be claimed, and why not.
pub async fn task_eligibility(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let eligibility: TaskEligibility = state.store.task_eligibility(TaskId::new(task_id)).await?;
    Ok(Json(eligibility).into_response())
}

pub async fn get_task_tree(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let mut direction = TaskTreeDirection::Descendants;
    let mut max_depth = MAX_TREE_DEPTH;
    let mut limit = DEFAULT_TREE_LIMIT;
    for (name, value) in &query {
        match name.as_str() {
            "direction" if value == "descendants" => direction = TaskTreeDirection::Descendants,
            "direction" if value == "ancestors" => direction = TaskTreeDirection::Ancestors,
            "direction" => return Err(ApiError::bad_request("unknown direction value")),
            "maxDepth" => {
                let depth = bounded(value, "maxDepth")?.clamp(1, usize::from(MAX_TREE_DEPTH));
                max_depth = u8::try_from(depth).unwrap_or(MAX_TREE_DEPTH);
            }
            "limit" => limit = bounded(value, "limit")?.clamp(1, MAX_TREE_LIMIT),
            _ => {}
        }
    }
    let tree = state
        .store
        .get_task_tree(TaskId::new(task_id), direction, max_depth, limit);
    Ok(Json(tree.await?).into_response())
}

fn bounded(raw: &str, name: &str) -> Result<usize, ApiError> {
    raw.parse()
        .map_err(|_| ApiError::bad_request(format!("{name} must be a positive integer")))
}

/// `POST /v1/tasks/{id}`: version-fenced edits to an unclaimed Task.
pub async fn update_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: UpdateTaskRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let updated = state
            .store
            .update_task(key, TaskId::new(task_id), request, now_ms())
            .await?;
        let (record, event, _) = updated;
        state.publish_event(event);
        if record.status == TaskStatus::Queued {
            spawn_wakeup(&state);
        }
        Ok(Json(record).into_response())
    })
    .await
}

pub async fn cancel_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: CancelTaskRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let cancelled = state
            .store
            .cancel_task(key, TaskId::new(task_id), request, now_ms());
        let (record, delivery, event, replay) = cancelled.await?;
        state.publish_event(event);
        if let (Some(delivery), false) = (delivery, replay) {
            send_cancel(&state, &delivery);
        }
        Ok(Json(record).into_response())
    })
    .await
}

pub async fn finish_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: FinishTaskRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let finished =
            state
                .store
                .finish_coordination_task(key, TaskId::new(task_id), request, now_ms());
        let (record, event, _) = finished.await?;
        state.publish_event(event);
        spawn_wakeup(&state);
        Ok(Json(record).into_response())
    })
    .await
}

/// Best-effort `task.cancel` for the live Delivery of a just-cancelled Task.
fn send_cancel(state: &HubState, delivery: &DeliveryView) {
    let payload = TaskCancelPayload {
        task_id: delivery.task_id.clone(),
        delivery_id: delivery.id.clone(),
        reason: delivery.cancel_reason.clone(),
    };
    let frame = build_hub_frame(
        WorkerWireKind::TaskCancel,
        &delivery.worker_id,
        None,
        delivery.connection_epoch,
        now_ms(),
        payload,
    );
    if let Some(frame) = frame {
        let _ = state.send_to_worker(&delivery.worker_id, delivery.connection_epoch, frame);
    }
}
