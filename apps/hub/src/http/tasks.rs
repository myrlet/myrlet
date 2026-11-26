//! Task control-plane handlers: create, bundle import, reads, dispatch, cancel, and finish.

use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{
    CancelTaskRequest, DeliveryView, DispatchTaskRequest, FinishTaskRequest,
    TaskBundleImportRequest, TaskCancelPayload, TaskDefinition, TaskId, TaskStatus,
    TaskTreeDirection, WorkerWireKind,
};
use myrlet_store::StoreError;

use crate::auth;
use crate::error::ApiError;
use crate::http::{authorize_command, read_bounded_json, run_bounded};
use crate::matcher;
use crate::queue::{send_offer_or_release, spawn_drain};
use crate::state::{now_ms, HubState};
use crate::ws::build_hub_frame;

const MAX_TREE_DEPTH: u8 = 5;
const DEFAULT_TREE_LIMIT: usize = 200;
const MAX_TREE_LIMIT: usize = 500;

/// `POST /v1/tasks`: create one `held` Task.
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
        Ok((StatusCode::CREATED, Json(record)).into_response())
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
        Ok((StatusCode::CREATED, Json(response)).into_response())
    })
    .await
}

pub async fn get_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    let record = state.store.get_task(TaskId::new(task_id)).await?;
    Ok(Json(record).into_response())
}

pub async fn get_task_tree(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
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

/// `POST /v1/tasks/{id}/dispatch`: targeted or match dispatch, optionally queueing.
pub async fn dispatch_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let key = authorize_command(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: DispatchTaskRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let task_id = TaskId::new(task_id);
        let record = state.store.get_task(task_id.clone()).await?;
        if record.version != request.expected_task_version {
            return Err(ApiError::from(StoreError::VersionConflict));
        }
        if !matches!(record.status, TaskStatus::Held | TaskStatus::Queued) {
            return Err(ApiError::from(StoreError::InvalidTransition));
        }
        let ready = state
            .store
            .are_dependencies_satisfied(task_id.clone())
            .await?;
        let target = if ready {
            let workers = state.store.list_workers(state.online_map()).await?;
            matcher::resolve(&workers, &record.task.routing, &request.selection)
        } else {
            None
        };
        if let Some(target) = target {
            let created =
                state
                    .store
                    .create_delivery(Some(key), task_id, record.version, target, now_ms());
            let (response, event, _) = created.await?;
            state.publish_event(event);
            send_offer_or_release(&state, response.delivery.clone(), &record.task, false).await;
            return Ok(Json(response).into_response());
        }
        if request.queue {
            let queued = state
                .store
                .enqueue_task(key, task_id, request, now_ms())
                .await?;
            let (response, event, _) = queued;
            state.publish_event(event);
            return Ok((StatusCode::ACCEPTED, Json(response)).into_response());
        }
        let conflict = ApiError::from(StoreError::InvalidTransition);
        Err(if ready {
            ApiError::service_unavailable()
        } else {
            conflict
        })
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
        spawn_drain(&state);
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
        spawn_drain(&state);
        Ok(Json(record).into_response())
    })
    .await
}

/// Best-effort `task.cancel` for the Delivery of a just-cancelled Task.
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
