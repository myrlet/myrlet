//! Worker HTTP reporting: Harness-authored Progress and the outcome-free Finish report.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{DeliveryId, FinishReportRequest, ProgressReportRequest};

use crate::auth;
use crate::error::ApiError;
use crate::http::{read_bounded_json, run_bounded};
use crate::queue::spawn_drain;
use crate::state::{now_ms, HubState};

/// `POST /v1/worker/deliveries/{delivery_id}/progress`: latest-state Progress only.
pub async fn report_progress(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(delivery_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let worker_id = auth::require_worker(&state, &headers).await?;
    run_bounded(state.clone(), async move {
        let request: ProgressReportRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let recorded = state
            .store
            .record_progress(worker_id, DeliveryId::new(delivery_id), request, now_ms())
            .await?;
        let (receipt, event) = recorded;
        state.publish_event(event);
        Ok(Json(receipt).into_response())
    })
    .await
}

/// `POST /v1/worker/deliveries/{delivery_id}/finish`: report that handling ended.
pub async fn report_finish(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(delivery_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let worker_id = auth::require_worker(&state, &headers).await?;
    run_bounded(state.clone(), async move {
        let request: FinishReportRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let recorded = state
            .store
            .record_finish(worker_id, DeliveryId::new(delivery_id), request, now_ms())
            .await?;
        let (receipt, event) = recorded;
        state.publish_event(event);
        spawn_drain(&state);
        Ok(Json(receipt).into_response())
    })
    .await
}
