//! Public health/pairing routes, Worker claims, and the Admin fleet routes.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{
    ClaimTaskRequest, CreateEnrollmentCodeRequest, HealthResponse, PairWorkerRequest, WorkerDetail,
    WorkerId, WorkerListResponse,
};

use crate::auth;
use crate::error::ApiError;
use crate::http::{read_bounded_json, require_reader, run_bounded};
use crate::state::{now_ms, HubState};
use crate::wakeup::spawn_wakeup;

/// Claimable Tasks considered per Worker detail read.
const CLAIMABLE_LIMIT: usize = 50;

/// `GET /health`, `GET /v1/health`: unauthenticated liveness probe.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// `POST /v1/pair`: redeem a one-time pairing code (unauthenticated but bounded).
pub async fn pair_worker(State(state): State<HubState>, body: Body) -> Result<Response, ApiError> {
    run_bounded(state.clone(), async move {
        let request: PairWorkerRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let (paired, event) = state
            .store
            .redeem_pairing_code(state.hub_id.clone(), request, now_ms())
            .await?;
        state.publish_event(Some(event));
        Ok(Json(paired).into_response())
    })
    .await
}

/// `POST /v1/enrollment-codes`: issue a short-lived one-time pairing code.
pub async fn create_enrollment_code(
    State(state): State<HubState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: CreateEnrollmentCodeRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let issued = state
            .store
            .create_enrollment_code(request.ttl_seconds, request.suggested_worker_id, now_ms())
            .await?;
        Ok(Json(issued).into_response())
    })
    .await
}

/// `GET /v1/workers`: enrolled Workers with connectivity and reported inventory.
///
/// Owners and Workers may both read the fleet; neither read grants management authority.
pub async fn list_workers(
    State(state): State<HubState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let workers = state.store.list_workers(state.online_map()).await?;
    Ok(Json(WorkerListResponse { workers }).into_response())
}

/// `GET /v1/workers/{id}`: one Worker's detail, inventory, and claimable Task IDs.
pub async fn get_worker(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let detail: WorkerDetail = state
        .store
        .worker_detail(
            WorkerId::new(worker_id),
            state.online_map(),
            CLAIMABLE_LIMIT,
        )
        .await?;
    Ok(Json(detail).into_response())
}

/// `POST /v1/workers/claim`: explicitly claim one queued Task for one local Slot.
///
/// The Worker credential identifies the claimant; the Hub rechecks readiness, Slot
/// eligibility, and delivery uniqueness atomically, so racing claims yield one winner.
pub async fn claim_task(
    State(state): State<HubState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let worker_id = auth::require_worker(&state, &headers).await?;
    run_bounded(state.clone(), async move {
        let request: ClaimTaskRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let (response, event) = state.store.claim_task(worker_id, request, now_ms()).await?;
        state.publish_event(event);
        Ok(Json(response).into_response())
    })
    .await
}

/// `POST /v1/workers/{id}/rotate`: rotate the credential and fence the live session.
pub async fn rotate_worker(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let worker_id = WorkerId::new(worker_id);
        let (rotated, event) = state
            .store
            .rotate_worker_credential(state.hub_id.clone(), worker_id.clone(), now_ms())
            .await?;
        state.publish_event(Some(event));
        state.disconnect_worker(&worker_id);
        spawn_wakeup(&state);
        Ok(Json(rotated).into_response())
    })
    .await
}

/// `POST /v1/workers/{id}/revoke`: revoke the credential and disconnect the Worker.
pub async fn revoke_worker(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let worker_id = WorkerId::new(worker_id);
        let event = state
            .store
            .revoke_worker(worker_id.clone(), now_ms())
            .await?;
        state.publish_event(Some(event));
        state.disconnect_worker(&worker_id);
        spawn_wakeup(&state);
        Ok(StatusCode::NO_CONTENT.into_response())
    })
    .await
}
