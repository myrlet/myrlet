//! Hub-wide Security & Trust policy reads and Admin writes.

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{SecurityTrustSettings, UpdateSecurityTrustRequest};

use crate::auth;
use crate::error::ApiError;
use crate::http::{read_bounded_json, require_reader, run_bounded};
use crate::state::{now_ms, HubState};

/// `GET /v1/settings/security-trust`: the two stored policy booleans.
///
/// Either principal may read them; only Admin may change them.
pub async fn read_settings(
    State(state): State<HubState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_reader(&state, &headers).await?;
    let settings: SecurityTrustSettings = state.store.load_settings().await?;
    Ok(Json(settings).into_response())
}

/// `PATCH /v1/settings/security-trust`: update one or both policies.
pub async fn update_settings(
    State(state): State<HubState>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    auth::require_admin(&state, &headers)?;
    run_bounded(state.clone(), async move {
        let request: UpdateSecurityTrustRequest =
            read_bounded_json(body, state.config.max_body_bytes).await?;
        let (settings, event) = state.store.update_settings(request, now_ms()).await?;
        state.publish_event(Some(event));
        Ok(Json(settings).into_response())
    })
    .await
}
