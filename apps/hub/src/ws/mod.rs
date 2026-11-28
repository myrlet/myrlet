//! Worker WSS upgrade, `worker.hello` handshake, lease loop, and Hub frame encoding.

use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use myrlet_protocol::{
    HubWelcomePayload, WorkerHelloPayload, WorkerId, WorkerWireEnvelope, WorkerWireKind,
    MAX_WS_FRAME_BYTES, WORKER_PROTOCOL_VERSION,
};
use serde::Serialize;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::auth::require_worker;
use crate::error::ApiError;
use crate::state::{now_ms, HubState, LiveWorker, CONNECTION_QUEUE};
use crate::wakeup::spawn_wakeup;

mod frames;

const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;

/// Read the next inbound frame, if the peer is still connected.
pub(super) async fn next_message(socket: &mut WebSocket) -> Option<Result<Message, axum::Error>> {
    futures::StreamExt::next(socket).await
}

/// Write one outbound frame.
pub(super) async fn send_message(
    socket: &mut WebSocket,
    message: Message,
) -> Result<(), axum::Error> {
    futures::SinkExt::send(socket, message).await
}

/// `GET /v1/workers/connect`: reject non-Worker credentials before the upgrade runs.
///
/// The Admin principal is not accepted here: WSS is a Worker-only execution channel, and
/// a missing or Admin credential receives `401`.
pub async fn connect_worker(
    State(state): State<HubState>,
    headers: HeaderMap,
    upgrade: Option<WebSocketUpgrade>,
) -> Response {
    let worker = match require_worker(&state, &headers).await {
        Ok(worker) => worker,
        Err(error) => return error.into_response(),
    };
    let Some(upgrade) = upgrade else {
        return ApiError::bad_request("a WebSocket upgrade request is required").into_response();
    };
    upgrade.on_upgrade(move |socket| run(state, worker.worker_id, socket))
}

/// Encode one Hub frame envelope for a Worker session.
pub fn build_hub_frame<T: Serialize>(
    kind: WorkerWireKind,
    worker_id: &WorkerId,
    connection_id: Option<&str>,
    connection_epoch: u64,
    sent_at_ms: i64,
    payload: T,
) -> Option<String> {
    let envelope = WorkerWireEnvelope {
        v: WORKER_PROTOCOL_VERSION.to_string(),
        kind,
        message_id: format!("msg_{}", Uuid::new_v4().simple()),
        worker_id: worker_id.clone(),
        slot_id: None,
        connection_id: connection_id.map(str::to_owned),
        connection_epoch: Some(connection_epoch),
        sent_at_ms,
        reply_to: None,
        payload,
    };
    match serde_json::to_string(&envelope) {
        Ok(frame) => Some(frame),
        Err(error) => {
            tracing::error!(%error, "could not encode a hub frame");
            None
        }
    }
}

async fn run(state: HubState, worker_id: WorkerId, mut socket: WebSocket) {
    let Some(hello) = read_hello(&mut socket, &worker_id).await else {
        close(&mut socket, 1008, "expected a valid worker.hello").await;
        return;
    };
    let (epoch, events) = match state
        .store
        .open_worker_connection(worker_id.clone(), hello.inventory, now_ms())
        .await
    {
        Ok(outcome) => outcome,
        Err(error) => {
            tracing::warn!(%error, %worker_id, "worker connection refused");
            close(&mut socket, 1011, "connection refused").await;
            return;
        }
    };
    for event in events {
        state.publish_event(Some(event));
    }
    let connection_id = format!("conn_{}", Uuid::new_v4().simple());
    let (tx, mut rx) = mpsc::channel(CONNECTION_QUEUE);
    state.register_connection(
        worker_id.clone(),
        LiveWorker {
            connection_id: connection_id.clone(),
            epoch,
            tx,
        },
    );
    let welcome = HubWelcomePayload {
        hub_id: state.hub_id.clone(),
        worker_id: worker_id.clone(),
        connection_id: connection_id.clone(),
        connection_epoch: epoch,
        heartbeat_interval_ms: state.config.heartbeat_interval_ms,
        lease_ttl_ms: state.config.lease_ttl_ms,
    };
    let sent = send_frame(
        &mut socket,
        WorkerWireKind::HubWelcome,
        &worker_id,
        &connection_id,
        epoch,
        welcome,
    )
    .await;
    if sent {
        spawn_wakeup(&state);
        lease_loop(
            &state,
            &worker_id,
            &connection_id,
            epoch,
            &mut socket,
            &mut rx,
        )
        .await;
    }
    state.unregister_connection(&worker_id, epoch);
}

async fn lease_loop(
    state: &HubState,
    worker_id: &WorkerId,
    connection_id: &str,
    epoch: u64,
    socket: &mut WebSocket,
    rx: &mut mpsc::Receiver<String>,
) {
    let lease = Duration::from_millis(state.config.lease_ttl_ms.max(1));
    let mut deadline = tokio::time::Instant::now() + lease;
    loop {
        tokio::select! {
            incoming = next_message(socket) => {
                let Some(Ok(message)) = incoming else { break };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    _ => continue,
                };
                if text.len() > MAX_WS_FRAME_BYTES {
                    break;
                }
                match frames::handle_frame(state, socket, worker_id, connection_id, epoch, &text)
                    .await
                {
                    frames::FrameOutcome::Accepted => {
                        deadline = tokio::time::Instant::now() + lease;
                    }
                    frames::FrameOutcome::Ignored => {}
                    frames::FrameOutcome::Close => break,
                }
            }
            outgoing = rx.recv() => {
                let Some(frame) = outgoing else { break };
                if send_message(socket, Message::Text(frame)).await.is_err() {
                    break;
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                tracing::info!(%worker_id, "worker lease expired");
                break;
            }
        }
    }
    close(socket, 1000, "connection closed").await;
}

async fn read_hello(socket: &mut WebSocket, expected: &WorkerId) -> Option<WorkerHelloPayload> {
    let handshake = async {
        loop {
            let message = next_message(socket).await?.ok()?;
            let text = match message {
                Message::Text(text) => text,
                _ => continue,
            };
            if text.len() > MAX_WS_FRAME_BYTES {
                return None;
            }
            let frame: WorkerWireEnvelope<WorkerHelloPayload> = serde_json::from_str(&text).ok()?;
            if frame.kind != WorkerWireKind::WorkerHello
                || &frame.worker_id != expected
                || frame.v != WORKER_PROTOCOL_VERSION
            {
                return None;
            }
            frame.payload.inventory.validate().ok()?;
            return Some(frame.payload);
        }
    };
    let timeout = Duration::from_millis(HANDSHAKE_TIMEOUT_MS);
    tokio::time::timeout(timeout, handshake)
        .await
        .ok()
        .flatten()
}

pub(super) async fn send_frame<T: Serialize>(
    socket: &mut WebSocket,
    kind: WorkerWireKind,
    worker_id: &WorkerId,
    connection_id: &str,
    epoch: u64,
    payload: T,
) -> bool {
    let frame = build_hub_frame(
        kind,
        worker_id,
        Some(connection_id),
        epoch,
        now_ms(),
        payload,
    );
    match frame {
        Some(frame) => send_message(socket, Message::Text(frame)).await.is_ok(),
        None => false,
    }
}

async fn close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    let _ = send_message(socket, Message::Close(Some(frame))).await;
}
