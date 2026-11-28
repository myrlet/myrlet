//! Hub HTTP router, ingress bounds, and the auth-before-body JSON reader.

use std::future::Future;
use std::time::Duration;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::routing::{get, patch, post};
use axum::Router;
use futures::StreamExt;
use serde::de::DeserializeOwned;

use crate::error::ApiError;
use crate::state::HubState;

pub mod events;
pub mod reports;
pub mod settings;
pub mod tasks;
pub mod ui;
pub mod workers;

/// Upper bound of an accepted `Idempotency-Key` header value.
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 200;

/// Build the complete V1 Hub router from the shared state.
pub fn build_router(state: HubState) -> Router {
    Router::new()
        .route("/health", get(workers::health))
        .route("/v1/health", get(workers::health))
        .route("/v1/pair", post(workers::pair_worker))
        .route("/v1/snapshot", get(events::snapshot))
        .route("/v1/events", get(events::events))
        .route("/v1/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route(
            "/v1/tasks/:id",
            get(tasks::get_task).post(tasks::update_task),
        )
        .route("/v1/tasks/:id/tree", get(tasks::get_task_tree))
        .route("/v1/tasks/:id/cancel", post(tasks::cancel_task))
        .route("/v1/tasks/:id/finish", post(tasks::finish_task))
        .route("/v1/tasks/:id/eligibility", get(tasks::task_eligibility))
        .route("/v1/task-bundles/import", post(tasks::import_bundle))
        .route(
            "/v1/enrollment-codes",
            post(workers::create_enrollment_code),
        )
        .route("/v1/workers", get(workers::list_workers))
        .route("/v1/workers/connect", get(crate::ws::connect_worker))
        .route("/v1/workers/claim", post(workers::claim_task))
        .route("/v1/workers/:id", get(workers::get_worker))
        .route("/v1/workers/:id/rotate", post(workers::rotate_worker))
        .route("/v1/workers/:id/revoke", post(workers::revoke_worker))
        .route("/v1/workers/:id/approve", post(workers::approve_worker))
        .route(
            "/v1/workers/:id/permission-tier",
            patch(workers::update_permission_tier),
        )
        .route(
            "/v1/workers/:id/config/apply",
            post(workers::apply_worker_config),
        )
        .route(
            "/v1/settings/security-trust",
            get(settings::read_settings).patch(settings::update_settings),
        )
        .route(
            "/v1/worker/deliveries/:delivery_id/progress",
            post(reports::report_progress),
        )
        .route(
            "/v1/worker/deliveries/:delivery_id/finish",
            post(reports::report_finish),
        )
        .route(
            "/v1/worker/deliveries/:delivery_id/result",
            get(reports::read_result).post(reports::report_result),
        )
        .fallback(ui::serve_ui)
        .with_state(state)
}

/// Authenticate a Task command and read its required `Idempotency-Key`.
///
/// Task coordination is available to the Admin principal and to an `owner`-tier Worker
/// credential; a `worker`-tier credential receives `403`.
pub(crate) async fn authorize_command(
    state: &HubState,
    headers: &HeaderMap,
) -> Result<String, ApiError> {
    crate::auth::require_global_reader(state, headers).await?;
    idempotency_key(headers)
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    if value.is_empty() || value.len() > MAX_IDEMPOTENCY_KEY_BYTES {
        return Err(ApiError::bad_request(
            "a non-empty Idempotency-Key header is required",
        ));
    }
    Ok(value)
}

/// Buffer and decode one bounded JSON body. Callers authenticate **before** calling this.
pub(crate) async fn read_bounded_json<T: DeserializeOwned>(
    body: Body,
    max_bytes: usize,
) -> Result<T, ApiError> {
    let mut stream = body.into_data_stream();
    let mut buffer = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| ApiError::bad_request("the request body could not be read"))?;
        if buffer.len() + chunk.len() > max_bytes {
            return Err(ApiError::payload_too_large());
        }
        buffer.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&buffer)
        .map_err(|_| ApiError::bad_request("the request body is not valid JSON"))
}

/// Run one bounded command under a concurrency permit and the request timeout.
pub(crate) async fn run_bounded<T>(
    state: HubState,
    command: impl Future<Output = Result<T, ApiError>>,
) -> Result<T, ApiError> {
    if state.is_shutting_down() {
        return Err(ApiError::service_unavailable());
    }
    let permit = state
        .command_permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::service_unavailable())?;
    let timeout = Duration::from_millis(state.config.request_timeout_ms.max(1));
    let outcome = tokio::time::timeout(timeout, command).await;
    drop(permit);
    outcome.map_err(|_| ApiError::request_timeout())?
}

/// Global read caller: the Admin principal or an `owner`-tier Worker.
///
/// A `worker`-tier credential receives `403`; invalid or missing credentials receive
/// `401`. Global reads never grant management authority.
pub(crate) use crate::auth::require_global_reader;
