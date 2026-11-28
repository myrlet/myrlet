//! Worker HTTP reporting: Harness-authored Progress, Result, and outcome-free Finish.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{
    DeliveryId, FinishReportRequest, ProgressReportRequest, ResultReportRequest,
};

use crate::auth;
use crate::error::ApiError;
use crate::http::{read_bounded_json, run_bounded};
use crate::state::{now_ms, HubState};
use crate::wakeup::spawn_wakeup;

/// `POST /v1/worker/deliveries/{delivery_id}/progress`: latest-value Progress only.
pub async fn report_progress(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(delivery_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let worker = auth::require_worker(&state, &headers).await?;
    let worker_id = worker.worker_id;
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
    let worker = auth::require_worker(&state, &headers).await?;
    let worker_id = worker.worker_id;
    run_bounded(state.clone(), async move {
        let request: FinishReportRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let recorded = state
            .store
            .record_finish(worker_id, DeliveryId::new(delivery_id), request, now_ms())
            .await?;
        let (receipt, event) = recorded;
        state.publish_event(event);
        spawn_wakeup(&state);
        Ok(Json(receipt).into_response())
    })
    .await
}

/// `POST /v1/worker/deliveries/{delivery_id}/result`: reconcile the latest Markdown.
pub async fn report_result(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(delivery_id): Path<String>,
    body: Body,
) -> Result<Response, ApiError> {
    let worker = auth::require_worker(&state, &headers).await?;
    let worker_id = worker.worker_id;
    run_bounded(state.clone(), async move {
        let request: ResultReportRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        if state.online_map().get(&worker_id) != Some(&request.epoch) {
            return Err(myrlet_store::StoreError::StaleDelivery.into());
        }
        let recorded = state
            .store
            .record_result(worker_id, DeliveryId::new(delivery_id), request, now_ms())
            .await?;
        let (receipt, event) = recorded;
        state.publish_event(event);
        Ok(Json(receipt).into_response())
    })
    .await
}

/// `GET /v1/worker/deliveries/{delivery_id}/result`: current Hub copy for a comparison.
///
/// Only the Delivery's own Worker may read it back, and reading never writes.
pub async fn read_result(
    State(state): State<HubState>,
    headers: HeaderMap,
    Path(delivery_id): Path<String>,
) -> Result<Response, ApiError> {
    let worker = auth::require_worker(&state, &headers).await?;
    let worker_id = worker.worker_id;
    let view = state
        .store
        .worker_delivery_result(worker_id, DeliveryId::new(delivery_id))
        .await?;
    Ok(Json(view).into_response())
}
