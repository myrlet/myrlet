//! In-process Hub stub: Worker HTTP Progress/Finish endpoints plus one WSS control socket.
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{SinkExt, StreamExt};
use myrlet_protocol::{
    ClaimTaskRequest, ClaimTaskResponse, DeliveryId, DeliveryStatus, DeliveryView, FinishReceipt,
    FinishReportRequest, ProgressReceipt, ProgressReportRequest, ResultReceipt,
    ResultReportRequest, TaskId, TaskStatus, WorkerId, WORKER_PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

use myrlet_worker_daemon::credentials::WorkerCredentials;

pub const WORKER_ID: &str = "wkr_000000000000";
pub const CREDENTIAL: &str = "test-worker-credential";
pub const SLOT_ID: &str = "slot-test";
pub const EPOCH: u64 = 3;

/// One HTTP report the Worker posted to the stub Hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Progress {
        delivery_id: String,
        epoch: u64,
        text: String,
    },
    Finish {
        delivery_id: String,
        epoch: u64,
    },
    Result {
        delivery_id: String,
        epoch: u64,
        text: Option<String>,
    },
    /// One `/v1/workers/claim` attempt, whether it won or lost.
    Claim {
        task_id: String,
        connection_epoch: u64,
    },
    /// One `GET .../result` comparison read.
    ReadResult {
        delivery_id: String,
    },
}

#[derive(Clone)]
struct HubState {
    reports: UnboundedSender<Report>,
    frames: UnboundedSender<Value>,
    outbound: Arc<Mutex<UnboundedReceiver<Value>>>,
    authorized: UnboundedSender<bool>,
    stale_epoch: Option<u64>,
    result_gate: Option<(Arc<tokio::sync::Notify>, StatusCode)>,
    /// Truncate the first claim acknowledgement after committing the Delivery.
    lose_first_claim_reply: Arc<Mutex<bool>>,
    /// The committed Delivery, replayed to the owning Worker on a repeated claim.
    committed: Arc<Mutex<Option<DeliveryView>>>,
}

/// A running stub Hub with channels for observing and driving one Worker.
pub struct StubHub {
    pub url: String,
    reports: UnboundedReceiver<Report>,
    frames: UnboundedReceiver<Value>,
    outbound: UnboundedSender<Value>,
    authorized: UnboundedReceiver<bool>,
}

impl StubHub {
    /// The Worker's WSS upgrade presented the expected bearer credential.
    pub async fn expect_authorized(&mut self) {
        let authorized = tokio::time::timeout(Duration::from_secs(5), self.authorized.recv())
            .await
            .expect("worker never upgraded the WSS connection")
            .expect("authorization channel closed");
        assert!(
            authorized,
            "worker WSS upgrade missed the bearer credential"
        );
    }

    /// Next frame the Worker sent.
    pub async fn next_frame(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(5), self.frames.recv())
            .await
            .expect("worker sent no frame")
            .expect("frame channel closed")
    }

    /// Push one Hub frame to the Worker.
    pub fn send_frame(&self, frame: Value) {
        self.outbound.send(frame).expect("worker socket closed");
    }

    /// Next HTTP report the Worker posted.
    pub async fn next_report(&mut self) -> Report {
        tokio::time::timeout(Duration::from_secs(15), self.reports.recv())
            .await
            .expect("worker posted no report")
            .expect("report channel closed")
    }
}

/// Start the stub Hub; `stale_epoch` makes Progress with that epoch return `409`.
pub async fn start(stale_epoch: Option<u64>) -> StubHub {
    start_with_result_gate(stale_epoch, None).await
}

pub async fn start_with_result_gate(
    stale_epoch: Option<u64>,
    result_gate: Option<Arc<tokio::sync::Notify>>,
) -> StubHub {
    launch(
        stale_epoch,
        result_gate.map(|gate| (gate, StatusCode::OK)),
        false,
    )
    .await
}

/// Hold Result A until the test releases a gateway timeout while WSS stays connected.
pub async fn start_with_result_timeout(gate: Arc<tokio::sync::Notify>) -> StubHub {
    launch(None, Some((gate, StatusCode::GATEWAY_TIMEOUT)), false).await
}

/// Start a stub Hub whose first claim commits and then truncates its acknowledgement,
/// so the Worker observes a transport loss after the Delivery already exists.
pub async fn start_with_lost_claim_reply(stale_epoch: Option<u64>) -> StubHub {
    launch(stale_epoch, None, true).await
}

async fn launch(
    stale_epoch: Option<u64>,
    result_gate: Option<(Arc<tokio::sync::Notify>, StatusCode)>,
    lose_first_claim_reply: bool,
) -> StubHub {
    let (reports_tx, reports) = mpsc::unbounded_channel();
    let (frames_tx, frames) = mpsc::unbounded_channel();
    let (outbound_tx, outbound) = mpsc::unbounded_channel();
    let (authorized_tx, authorized) = mpsc::unbounded_channel();
    let state = HubState {
        reports: reports_tx,
        frames: frames_tx,
        outbound: Arc::new(Mutex::new(outbound)),
        authorized: authorized_tx,
        stale_epoch,
        result_gate,
        lose_first_claim_reply: Arc::new(Mutex::new(lose_first_claim_reply)),
        committed: Arc::default(),
    };
    let router = Router::new()
        .route("/v1/workers/connect", get(ws_handler))
        .route("/v1/workers/claim", post(claim))
        .route(
            "/v1/worker/deliveries/:delivery_id/progress",
            post(progress),
        )
        .route("/v1/worker/deliveries/:delivery_id/finish", post(finish))
        .route(
            "/v1/worker/deliveries/:delivery_id/result",
            get(read_result).post(result),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub hub");
    let address = listener.local_addr().expect("stub address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    StubHub {
        url: format!("http://{address}"),
        reports,
        frames,
        outbound: outbound_tx,
        authorized,
    }
}

fn bearer_ok(headers: &HeaderMap) -> bool {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {CREDENTIAL}"))
}

fn unauthorized() -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Unauthorized", "message": "invalid credential" })),
    )
}

async fn ws_handler(
    upgrade: WebSocketUpgrade,
    State(state): State<HubState>,
    headers: HeaderMap,
) -> Response {
    let _ = state.authorized.send(bearer_ok(&headers));
    upgrade.on_upgrade(move |socket| pump(socket, state))
}

async fn pump(socket: WebSocket, state: HubState) {
    let (mut sink, mut source) = socket.split();
    let mut outbound = state.outbound.lock().await;
    loop {
        tokio::select! {
            incoming = source.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        let _ = state.frames.send(value);
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            },
            outgoing = outbound.recv() => {
                let Some(value) = outgoing else { break };
                let Ok(text) = serde_json::to_string(&value) else { break };
                if sink.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn progress(
    State(state): State<HubState>,
    Path(delivery_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ProgressReportRequest>,
) -> (StatusCode, Json<Value>) {
    if !bearer_ok(&headers) {
        return unauthorized();
    }
    if state.stale_epoch == Some(body.epoch) {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "StaleDelivery", "message": "stale epoch" })),
        );
    }
    let receipt = ProgressReceipt {
        delivery_id: DeliveryId::new(delivery_id.clone()),
        accepted_at_ms: 2,
        first_accepted_at_ms: 1,
        duplicate: false,
    };
    let _ = state.reports.send(Report::Progress {
        delivery_id,
        epoch: body.epoch,
        text: body.text,
    });
    (
        StatusCode::OK,
        Json(serde_json::to_value(receipt).expect("progress receipt json")),
    )
}

async fn finish(
    State(state): State<HubState>,
    Path(delivery_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FinishReportRequest>,
) -> (StatusCode, Json<Value>) {
    if !bearer_ok(&headers) {
        return unauthorized();
    }
    let receipt = FinishReceipt {
        delivery_id: DeliveryId::new(delivery_id.clone()),
        task_id: TaskId::new("task_stub"),
        task_status: TaskStatus::Finished,
        changed: true,
    };
    let _ = state.reports.send(Report::Finish {
        delivery_id,
        epoch: body.epoch,
    });
    (
        StatusCode::OK,
        Json(serde_json::to_value(receipt).expect("finish receipt json")),
    )
}

async fn result(
    State(state): State<HubState>,
    Path(delivery_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ResultReportRequest>,
) -> (StatusCode, Json<Value>) {
    if !bearer_ok(&headers) {
        return unauthorized();
    }
    let receipt = ResultReceipt {
        delivery_id: DeliveryId::new(delivery_id.clone()),
        task_id: TaskId::new("task_stub"),
        recorded_at_ms: body.text.as_ref().map(|_| 3),
        result_sha256: body.text.as_deref().map(myrlet_protocol::result_sha256),
        cleared: body.text.is_none(),
        duplicate: false,
    };
    let hold = body.text.as_deref() == Some("A");
    let _ = state.reports.send(Report::Result {
        delivery_id,
        epoch: body.epoch,
        text: body.text,
    });
    if hold {
        if let Some((gate, status)) = &state.result_gate {
            gate.notified().await;
            if !status.is_success() {
                return (*status, Json(json!({ "error": "RequestTimeout" })));
            }
        }
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(receipt).expect("result receipt json")),
    )
}

/// `POST /v1/workers/claim`: commit one Delivery for the named Task.
async fn claim(
    State(state): State<HubState>,
    headers: HeaderMap,
    Json(body): Json<ClaimTaskRequest>,
) -> (StatusCode, Json<Value>) {
    if !bearer_ok(&headers) {
        return unauthorized();
    }
    let _ = state.reports.send(Report::Claim {
        task_id: body.task_id.to_string(),
        connection_epoch: body.connection_epoch,
    });
    if body.connection_epoch != EPOCH && body.connection_epoch != EPOCH + 1 {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "StaleDelivery", "message": "stale epoch" })),
        );
    }
    let losing = std::mem::take(&mut *state.lose_first_claim_reply.lock().await);
    // Replaying the same claim after a lost reply returns the committed Delivery.
    if let Some(mut committed) = state.committed.lock().await.clone() {
        if committed.task_id == body.task_id && committed.slot_id == body.slot_id {
            committed.connection_epoch = body.connection_epoch;
            let response = ClaimTaskResponse {
                task: stub_task(&body.task_id),
                delivery: committed,
            };
            return (
                StatusCode::OK,
                Json(serde_json::to_value(response).expect("claim response json")),
            );
        }
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "AlreadyClaimed", "message": "already committed" })),
        );
    }
    let delivery_id = format!("delivery_{}", body.task_id);
    let delivery = DeliveryView {
        id: DeliveryId::new(delivery_id.clone()),
        task_id: body.task_id.clone(),
        worker_id: WorkerId::new(WORKER_ID),
        slot_id: body.slot_id.clone(),
        executor_id: body.executor_id.clone(),
        connection_epoch: body.connection_epoch,
        state: DeliveryStatus::Accepted,
        cancel_state: None,
        cancel_reason: None,
        created_at_ms: 1,
        updated_at_ms: 1,
        progress: None,
    };
    let response = ClaimTaskResponse {
        task: stub_task(&body.task_id),
        delivery,
    };
    if losing {
        *state.committed.lock().await = Some(response.delivery.clone());
        // Committed at the Hub, but the acknowledgement is lost in transit: the
        // Worker cannot tell whether a Delivery now exists.
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "InternalError", "message": "reply lost" })),
        );
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).expect("claim response json")),
    )
}

/// `GET /v1/worker/deliveries/{id}/result`: the current Hub copy for a comparison.
async fn read_result(
    State(state): State<HubState>,
    Path(delivery_id): Path<String>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if !bearer_ok(&headers) {
        return unauthorized();
    }
    let _ = state.reports.send(Report::ReadResult {
        delivery_id: delivery_id.clone(),
    });
    let view = myrlet_protocol::DeliveryResultView {
        delivery_id: DeliveryId::new(delivery_id),
        task_id: TaskId::new("task_stub"),
        text: None,
        recorded_at_ms: None,
        result_sha256: None,
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(view).expect("result view json")),
    )
}

fn stub_task(task_id: &TaskId) -> myrlet_protocol::TaskRecord {
    let definition: myrlet_protocol::TaskDefinition = serde_json::from_value(json!({
        "title": "Stub task",
        "prompt": "Write progress, then stop.",
    }))
    .expect("task definition");
    myrlet_protocol::TaskRecord {
        id: task_id.clone(),
        root_task_id: task_id.clone(),
        depth: 1,
        status: TaskStatus::Dispatched,
        version: 2,
        created_at_ms: 1,
        updated_at_ms: 1,
        finished_at_ms: None,
        task: definition,
        current_delivery: None,
        blocked_by: Vec::new(),
        result_markdown: None,
        result_recorded_at_ms: None,
        subtree: Default::default(),
    }
}

/// One Hub control frame with the canonical envelope.
pub fn hub_frame(kind: &str, payload: Value) -> Value {
    json!({
        "v": WORKER_PROTOCOL_VERSION,
        "kind": kind,
        "messageId": format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "workerId": WORKER_ID,
        "connectionEpoch": EPOCH,
        "sentAtMs": 1,
        "payload": payload,
    })
}

/// `hub.welcome` for epoch [`EPOCH`].
pub fn welcome_frame() -> Value {
    hub_frame(
        "hub.welcome",
        json!({
            "hubId": "hub_stub",
            "workerId": WORKER_ID,
            "connectionId": "connection_stub",
            "connectionEpoch": EPOCH,
            "heartbeatIntervalMs": 5_000,
            "leaseTtlMs": 20_000,
        }),
    )
}

/// `hub.wakeup`: one queued Task the Worker may claim.
pub fn wakeup_frame(task_id: &str) -> Value {
    hub_frame(
        "hub.wakeup",
        json!({ "taskId": task_id, "eligibleSlotIds": [SLOT_ID] }),
    )
}

/// Credential document matching [`WORKER_ID`] and [`CREDENTIAL`].
pub fn credentials() -> WorkerCredentials {
    WorkerCredentials {
        hub_id: "hub_stub".to_owned(),
        worker_id: WorkerId::new(WORKER_ID),
        credential: CREDENTIAL.to_owned(),
    }
}

/// `worker.yaml` with one Enabled Slot and plain `Process` Executors.
pub fn process_yaml(hub_url: &str, executors: &[(&str, &str)]) -> String {
    let mut yaml = format!("hubUrl: {hub_url}\nslots:\n  - slotId: {SLOT_ID}\n    executors:\n");
    for (id, script) in executors {
        let script = format!("'{}'", script.replace('\'', "''"));
        yaml.push_str(&format!(
            "      - kind: process\n        id: {id}\n        program: /bin/sh\n        args: [\"-c\", {script}]\n"
        ));
    }
    yaml
}
