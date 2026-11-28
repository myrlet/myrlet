//! Snapshot read model and the resumable SSE invalidation stream.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{RawQuery, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myrlet_protocol::{HubEventEnvelope, WorkerId};
use tokio::sync::broadcast::error::RecvError;

use crate::auth;
use crate::error::ApiError;
use crate::state::{now_ms, HubState};

/// Events replayed per SSE resume; the store fails closed beyond its retention window.
const BACKFILL_LIMIT: usize = 10_000;
/// Period of SSE keepalive comments.
const KEEPALIVE_SECS: u64 = 15;
/// Safety-net revalidation period for a Worker stream between trust-change signals.
const REVALIDATE_INTERVAL: Duration = Duration::from_secs(60);

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
///
/// A Worker principal's authority is tied to the credential it presented. The stream keeps
/// that credential, revalidates it lazily (when the Hub signals a trust change for this
/// Worker, and periodically as a safety net), and ends as soon as the credential no longer
/// authenticates or no longer carries a global-read tier. The Admin principal is permanent
/// for the life of its token and never needs revalidation.
pub async fn events(
    State(state): State<HubState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response, ApiError> {
    let caller = auth::require_global_reader(&state, &headers).await?;
    let cursor = resume_cursor(&headers, query.as_deref())?;
    // Subscribe before reading the log so no committed event can fall between the two.
    let mut receiver = state.events_tx.subscribe();
    let backfill = state
        .store
        .list_events_after(cursor, BACKFILL_LIMIT, state.config.event_retention_window)
        .await?;

    // Only a Worker credential needs revalidation; remember it and its identity while the
    // temporary headers are still available for this request.
    let revalidation = match &caller {
        auth::Caller::Admin => None,
        auth::Caller::Worker { worker_id, .. } => {
            let token = auth::extract_bearer(&headers).map(str::to_owned);
            token.map(|token| StreamAuth {
                worker_id: worker_id.clone(),
                token,
            })
        }
    };
    let mut auth_invalidations = state.auth_invalidations.subscribe();

    let stream = async_stream::stream! {
        let mut last_sent = cursor;
        let mut authorized = true;
        for event in backfill {
            if event.seq > last_sent {
                last_sent = event.seq;
                yield Ok::<Event, Infallible>(sse_event(&event));
            }
        }
        let mut recheck = tokio::time::interval(REVALIDATE_INTERVAL);
        recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        recheck.tick().await; // the first tick resolves immediately; start a fresh period
        loop {
            let observed_trust_change = tokio::select! {
                received = receiver.recv() => match received {
                    Ok(event) if event.seq > last_sent => {
                        last_sent = event.seq;
                        // A trust event on the normal stream is also a signal to recheck.
                        let trust_change = names_this_worker(&event, revalidation.as_ref());
                        yield Ok::<Event, Infallible>(sse_event(&event));
                        trust_change
                    }
                    Ok(_) => false,
                    Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => break,
                },
                // A trust change for this Worker invalidates the cached verdict.
                _ = wait_for_own_invalidation(&mut auth_invalidations, revalidation.as_ref()) => true,
                _ = recheck.tick() => false,
            };
            if !observed_trust_change {
                continue;
            }
            if let Some(check) = &revalidation {
                authorized = authorized && reauthorize(&state, check).await;
                if !authorized {
                    return;
                }
            }
        }
    };
    let keepalive = KeepAlive::new().interval(Duration::from_secs(KEEPALIVE_SECS));
    Ok(Sse::new(stream).keep_alive(keepalive).into_response())
}

/// One Worker stream's original credential, retained only for revalidation.
struct StreamAuth {
    worker_id: WorkerId,
    token: String,
}

/// Resolve when the invalidation channel names this stream's Worker.
///
/// A stream with no `StreamAuth` (an Admin) waits forever, so the branch never fires.
async fn wait_for_own_invalidation(
    invalidations: &mut tokio::sync::broadcast::Receiver<WorkerId>,
    auth: Option<&StreamAuth>,
) {
    match auth {
        None => std::future::pending().await,
        Some(auth) => loop {
            match invalidations.recv().await {
                Ok(worker_id) if worker_id == auth.worker_id => return,
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => std::future::pending().await,
            }
        },
    }
}

/// Whether `event` is a trust-state change for this stream's own Worker.
fn names_this_worker(event: &HubEventEnvelope, auth: Option<&StreamAuth>) -> bool {
    match auth {
        None => false,
        Some(auth) => {
            event.payload.get("workerId").and_then(|id| id.as_str())
                == Some(auth.worker_id.as_str())
                && matches!(
                    event.kind.as_str(),
                    "worker.permission_tier" | "worker.revoked" | "worker.rotated"
                )
        }
    }
}

/// Re-read the original credential and confirm it still grants global-read authority.
async fn reauthorize(state: &HubState, auth: &StreamAuth) -> bool {
    match state.store.reauthenticate_worker(auth.token.clone()).await {
        // The Worker must still authenticate *and* still be the same identity with a
        // global-read tier; a re-pair that reused the token cannot borrow another Worker.
        Ok(Some(worker)) => {
            worker.worker_id == auth.worker_id && worker.permission_tier.is_global()
        }
        Ok(None) | Err(_) => false,
    }
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
