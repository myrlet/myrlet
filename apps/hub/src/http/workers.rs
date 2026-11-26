//! Public health/pairing routes plus the Admin Worker-enrollment and fleet routes.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{
    CreateEnrollmentCodeRequest, HealthResponse, PairWorkerRequest, WorkerId, WorkerListResponse,
};

use crate::auth;
use crate::error::ApiError;
use crate::http::{read_bounded_json, run_bounded};
use crate::state::{now_ms, HubState};

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
pub async fn list_workers(
    State(state): State<HubState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    let workers = state.store.list_workers(state.online_map()).await?;
    Ok(Json(WorkerListResponse { workers }).into_response())
}

/// `GET /v1/workers/{id}`: one Worker's detail and inventory.
pub async fn get_worker(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(worker_id): Path<String>,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    let worker_id = WorkerId::new(worker_id);
    let worker = state
        .store
        .get_worker(worker_id, state.online_map())
        .await?;
    Ok(Json(worker).into_response())
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
        Ok(StatusCode::NO_CONTENT.into_response())
    })
    .await
}
