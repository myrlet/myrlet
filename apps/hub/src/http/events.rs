//! Snapshot read model and the resumable SSE invalidation stream.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::HubEventEnvelope;
use tokio::sync::broadcast::error::RecvError;

use crate::auth;
use crate::error::ApiError;
use crate::state::{now_ms, HubState};

/// Events replayed per SSE resume; the store fails closed beyond its retention window.
const BACKFILL_LIMIT: usize = 10_000;
/// Period of SSE keepalive comments.
const KEEPALIVE_SECS: u64 = 15;

/// `GET /v1/snapshot`: consistent bounded read model of the whole Hub.
pub async fn snapshot(
    State(state): State<HubState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    auth::require_global_reader(&state, &headers).await?;
    let snapshot = state
        .store
        .list_snapshot(
            state.online_map(),
            state.config.snapshot_max_tasks,
            now_ms(),
        )
        .await?;
    Ok(Json(snapshot).into_response())
}

/// `GET /v1/events`: replay, then stream live invalidation hints deduplicated by `seq`.
pub async fn events(
    State(state): State<HubState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    auth::require_global_reader(&state, &headers).await?;
    let cursor = resume_cursor(&headers, query.as_deref())?;
    // Subscribe before reading the log so no committed event can fall between the two.
    let mut receiver = state.events_tx.subscribe();
    let backfill = state
        .store
        .list_events_after(cursor, BACKFILL_LIMIT, state.config.event_retention_window)
        .await?;

    let stream = async_stream::stream! {
        let mut last_sent = cursor;
        for event in backfill {
            if event.seq > last_sent {
                last_sent = event.seq;
                yield Ok::<Event, Infallible>(sse_event(&event));
            }
        }
        loop {
            match receiver.recv().await {
                Ok(event) if event.seq > last_sent => {
                    last_sent = event.seq;
                    yield Ok::<Event, Infallible>(sse_event(&event));
                }
                Ok(_) => continue,
                Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => break,
            }
        }
    };
    let keepalive = KeepAlive::new().interval(Duration::from_secs(KEEPALIVE_SECS));
    Ok(Sse::new(stream).keep_alive(keepalive).into_response())
}

fn sse_event(event: &HubEventEnvelope) -> Event {
    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    Event::default()
        .id(event.seq.to_string())
        .event(event.kind.clone())
        .data(data)
}

/// Read the resume cursor: `Last-Event-ID` wins over `?cursor=<seq>`.
fn resume_cursor(headers: &HeaderMap, query: Option<&str>) -> Result<u64, ApiError> {
    let header = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok());
    if let Some(raw) = header {
        return raw
            .trim()
            .parse::<u64>()
            .map_err(|_| ApiError::bad_request("Last-Event-ID must be an event sequence"));
    }
    for pair in query.unwrap_or_default().split('&') {
        if let Some(raw) = pair.strip_prefix("cursor=") {
            return raw
                .parse::<u64>()
                .map_err(|_| ApiError::bad_request("cursor must be an event sequence"));
        }
    }
    Ok(0)
}
