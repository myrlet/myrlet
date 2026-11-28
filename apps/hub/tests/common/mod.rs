//! Shared loopback Hub harness for the integration tests.
//!
//! Each test binary compiles this module separately, so helpers a given binary does not
//! use are expected rather than dead.
#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use myrlet_hub::{build_router, HubConfig, HubState};
use myrlet_protocol::{MyrletHome, WORKER_PROTOCOL_VERSION};
use myrlet_store::HubStore;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

/// Operator-provisioned Admin token used by every test Hub.
pub const ADMIN: &str = "hub-test-admin-token";

/// Worker WSS client socket.
pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// One Hub listening on an ephemeral loopback port.
pub struct Hub {
    pub base: String,
    pub connect_url: String,
    state: Option<HubState>,
}

fn config(retention_window: usize, ui_enabled: bool) -> HubConfig {
    let home = tempfile::tempdir().unwrap();
    HubConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        admin_token: ADMIN.to_string(),
        home: MyrletHome::resolve(Some(home.path())).unwrap(),
        max_body_bytes: 2 * 1024 * 1024,
        max_bundle_bytes: 1024 * 1024,
        request_timeout_ms: 5_000,
        max_concurrent_requests: 64,
        shutdown_timeout_ms: 1_000,
        event_retention_window: retention_window,
        snapshot_max_tasks: 100,
        heartbeat_interval_ms: 1_000,
        lease_ttl_ms: 30_000,
        ui_enabled,
    }
}

/// Start one Hub over an in-memory store and a real listener.
pub async fn start(retention_window: usize) -> Hub {
    start_with_ui(retention_window, true).await
}

/// Start one Hub with the embedded Web UI explicitly enabled or disabled.
pub async fn start_with_ui(retention_window: usize, ui_enabled: bool) -> Hub {
    start_with_config(retention_window, |config| config.ui_enabled = ui_enabled).await
}

/// Start one Hub after the caller adjusts the configuration (for example its Admin token).
pub async fn start_with_config(
    retention_window: usize,
    adjust: impl FnOnce(&mut HubConfig),
) -> Hub {
    let mut config = config(retention_window, true);
    adjust(&mut config);
    let state = HubState::new(
        config,
        Arc::new(HubStore::open_in_memory().unwrap()),
        "hub_test".to_string(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    myrlet_hub::wakeup::spawn_wakeup_loop(&state);
    let served = state.clone();
    tokio::spawn(async move { axum::serve(listener, build_router(served)).await.unwrap() });
    Hub {
        base: format!("http://{address}"),
        connect_url: format!("ws://{address}/v1/workers/connect"),
        state: Some(state),
    }
}

impl Hub {
    /// Send one raw request and decode whatever JSON body comes back.
    pub async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        key: Option<&str>,
        body: Option<&str>,
    ) -> (u16, Value) {
        let method = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut request = reqwest::Client::new().request(method, format!("{}{path}", self.base));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    pub async fn get_admin(&self, path: &str) -> (u16, Value) {
        self.request("GET", path, Some(ADMIN), None, None).await
    }

    pub async fn post_admin(
        &self,
        path: &str,
        key: Option<&str>,
        body: Option<&str>,
    ) -> (u16, Value) {
        self.request("POST", path, Some(ADMIN), key, body).await
    }

    /// Send one `POST` under an explicit Admin token (for collision tests).
    pub async fn post_admin_with(
        &self,
        token: &str,
        path: &str,
        key: Option<&str>,
        body: Option<&str>,
    ) -> (u16, Value) {
        self.request("POST", path, Some(token), key, body).await
    }

    /// The shared Hub state, for tests that need direct store access.
    pub fn state(&self) -> &HubState {
        self.state.as_ref().expect("Hub state is captured")
    }

    /// Attempt the WSS upgrade with a raw token, returning the handshake result.
    pub async fn connect_with(
        &self,
        token: &str,
    ) -> Result<Socket, Box<tokio_tungstenite::tungstenite::Error>> {
        let mut request = self.connect_url.clone().into_client_request().unwrap();
        let bearer = HeaderValue::from_str(&format!("Bearer {token}")).unwrap();
        request.headers_mut().insert("authorization", bearer);
        tokio_tungstenite::connect_async(request)
            .await
            .map(|(socket, _)| socket)
            .map_err(Box::new)
    }

    pub async fn post_worker(&self, path: &str, credential: &str, body: &str) -> (u16, Value) {
        self.request("POST", path, Some(credential), None, Some(body))
            .await
    }

    pub async fn patch_admin(&self, path: &str, body: Option<&str>) -> (u16, Value) {
        self.request("PATCH", path, Some(ADMIN), None, body).await
    }

    /// Promote a Worker to the `owner` tier as Admin.
    pub async fn set_tier(&self, worker_id: &str, tier: &str) {
        let body = json!({ "permissionTier": tier }).to_string();
        let (status, response) = self
            .patch_admin(
                &format!("/v1/workers/{worker_id}/permission-tier"),
                Some(&body),
            )
            .await;
        assert_eq!(status, 200, "set tier: {response}");
        assert_eq!(response["permissionTier"], tier);
    }

    /// Issue an enrollment code and redeem it in one step.
    pub async fn pair(&self) -> (String, String) {
        self.pair_as(None).await
    }

    /// Pair using a custom Admin token (for tests whose Hub does not use `ADMIN`).
    pub async fn pair_with_admin(&self, admin_token: &str) -> (String, String) {
        let issued = self
            .request(
                "POST",
                "/v1/enrollment-codes",
                Some(admin_token),
                None,
                Some("{}"),
            )
            .await;
        assert_eq!(issued.0, 200, "{}", issued.1);
        let body = json!({ "enrollmentCode": issued.1["enrollmentCode"] }).to_string();
        let paired = self
            .request("POST", "/v1/pair", None, None, Some(&body))
            .await;
        assert!(paired.0 == 200, "pairing failed");
        let worker_id = paired.1["workerId"].as_str().unwrap().to_string();
        let credential = paired.1["credential"].as_str().unwrap().to_string();
        (worker_id, credential)
    }

    /// Issue an enrollment code that suggests `worker_id` and redeem it, so a revoked
    /// Worker ID can be re-paired explicitly.
    pub async fn pair_as(&self, worker_id: Option<&str>) -> (String, String) {
        let request = match worker_id {
            Some(worker_id) => json!({ "suggestedWorkerId": worker_id }).to_string(),
            None => "{}".to_string(),
        };
        let issued = self
            .post_admin("/v1/enrollment-codes", None, Some(&request))
            .await;
        assert_eq!(issued.0, 200, "{}", issued.1);
        let body = json!({ "enrollmentCode": issued.1["enrollmentCode"] }).to_string();
        let paired = self
            .request("POST", "/v1/pair", None, None, Some(&body))
            .await;
        assert!(paired.0 == 200, "pairing failed");
        let body = &paired.1;
        let worker_id = body["workerId"].as_str().unwrap().to_string();
        let credential = body["credential"].as_str().unwrap().to_string();
        (worker_id, credential)
    }

    /// Connect a Worker over WSS and complete the `worker.hello` handshake.
    pub async fn connect(&self, worker_id: &str, credential: &str) -> (Socket, u64) {
        let mut request = self.connect_url.clone().into_client_request().unwrap();
        let bearer = HeaderValue::from_str(&format!("Bearer {credential}")).unwrap();
        request.headers_mut().insert("authorization", bearer);
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        let hello = json!({
            "v": WORKER_PROTOCOL_VERSION,
            "kind": "worker.hello",
            "messageId": "msg_hello",
            "workerId": worker_id,
            "sentAtMs": 1,
            "payload": { "inventory": inventory() },
        });
        socket.send(Message::Text(hello.to_string())).await.unwrap();
        let welcome = read_frame(&mut socket).await;
        assert_eq!(welcome["kind"], "hub.welcome");
        (
            socket,
            welcome["payload"]["connectionEpoch"].as_u64().unwrap(),
        )
    }

    /// Approve the Worker and apply its latest reported inventory, clearing both
    /// default-on trust gates so ordinary claim tests can proceed.
    pub async fn clear_trust_gates(&self, worker_id: &str) {
        self.clear_trust_gates_with(ADMIN, worker_id).await;
    }

    /// Clear both trust gates using an explicit Admin token.
    pub async fn clear_trust_gates_with(&self, admin_token: &str, worker_id: &str) {
        assert_eq!(
            self.request(
                "POST",
                &format!("/v1/workers/{worker_id}/approve"),
                Some(admin_token),
                None,
                None,
            )
            .await
            .0,
            204
        );
        let detail = self
            .request(
                "GET",
                &format!("/v1/workers/{worker_id}"),
                Some(admin_token),
                None,
                None,
            )
            .await;
        assert_eq!(detail.0, 200, "{}", detail.1);
        let revision = detail.1["worker"]["reportedRevision"]
            .as_u64()
            .expect("the connected Worker reported an inventory");
        let body = json!({ "revision": revision }).to_string();
        let applied = self
            .request(
                "POST",
                &format!("/v1/workers/{worker_id}/config/apply"),
                Some(admin_token),
                None,
                Some(&body),
            )
            .await;
        assert_eq!(applied.0, 200, "{}", applied.1);
    }

    /// Pair, connect, and clear both trust gates in one step.
    pub async fn enrolled(&self) -> (String, String, Socket, u64) {
        let (worker_id, credential) = self.pair().await;
        let (socket, epoch) = self.connect(&worker_id, &credential).await;
        self.clear_trust_gates(&worker_id).await;
        (worker_id, credential, socket, epoch)
    }

    /// Submit one Task, consume the wakeup, and claim it for `slot-a`.
    ///
    /// Returns the Task ID and the committed Delivery ID.
    pub async fn claim_one(
        &self,
        socket: &mut Socket,
        credential: &str,
        epoch: u64,
        key: &str,
    ) -> (String, String) {
        let definition = json!({ "title": "Verify", "prompt": "run the suite" }).to_string();
        let created = self
            .post_admin("/v1/tasks", Some(key), Some(&definition))
            .await;
        assert_eq!(created.0, 201, "{}", created.1);
        let task_id = created.1["task"]["id"].as_str().unwrap().to_string();
        assert_eq!(created.1["task"]["status"], "queued");

        let wakeup = read_frame(socket).await;
        assert_eq!(wakeup["kind"], "hub.wakeup");
        assert_eq!(wakeup["payload"]["taskId"], task_id);

        let claim = json!({
            "taskId": task_id,
            "slotId": "slot-a",
            "connectionEpoch": epoch,
        })
        .to_string();
        let claimed = self
            .post_worker("/v1/workers/claim", credential, &claim)
            .await;
        assert_eq!(claimed.0, 200, "{}", claimed.1);
        let delivery_id = claimed.1["delivery"]["id"].as_str().unwrap().to_string();
        (task_id, delivery_id)
    }
}

fn inventory() -> Value {
    json!({
        "revision": 1,
        "observedAtMs": 1,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{
            "slotId": "slot-a",
            "enabled": true,
            "advisoryCapacity": 2,
            "roles": ["general"],
            "capabilities": [{ "name": "shell", "state": "available" }],
            "executors": [{ "id": "exec-claude", "kind": "claude" }],
        }],
    })
}

/// Await the next `task.*`/`hub.*` frame as JSON.
pub async fn read_frame(socket: &mut Socket) -> Value {
    loop {
        let next = tokio::time::timeout(Duration::from_secs(5), socket.next()).await;
        let message = next.expect("timeout").expect("closed").expect("failed");
        match message {
            Message::Text(text) => return serde_json::from_str(text.as_str()).unwrap(),
            Message::Close(_) => panic!("socket closed before the expected frame"),
            _ => continue,
        }
    }
}

/// Wait for the Hub to close the session.
pub async fn is_closed(socket: &mut Socket) -> bool {
    let closed = async {
        while let Some(message) = socket.next().await {
            if message.is_err() || matches!(message, Ok(Message::Close(_))) {
                return true;
            }
        }
        true
    };
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .unwrap_or(false)
}

/// Read one SSE frame's JSON `data` line.
pub async fn next_sse_event(response: &mut reqwest::Response) -> Value {
    let mut buffer = String::new();
    loop {
        while let Some(index) = buffer.find("\n\n") {
            let frame: String = buffer.drain(..index + 2).collect();
            if let Some(data) = frame.lines().find_map(|line| line.strip_prefix("data: ")) {
                return serde_json::from_str(data).unwrap();
            }
        }
        let chunk = response.chunk().await.unwrap().expect("event stream ended");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
    }
}

/// Open the global SSE stream as `token` and assert it was authorized.
pub async fn open_events(hub: &Hub, token: &str, cursor: Option<u64>) -> reqwest::Response {
    let path = match cursor {
        Some(seq) => format!("/v1/events?cursor={seq}"),
        None => "/v1/events".to_string(),
    };
    let response = reqwest::Client::new()
        .get(format!("{}{path}", hub.base))
        .bearer_auth(token)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "SSE open rejected");
    response
}

/// Await the next SSE data frame, or `None` once the stream ends.
pub async fn try_next_sse_event(
    response: &mut reqwest::Response,
    timeout: Duration,
) -> Option<Value> {
    let read = async {
        let mut buffer = String::new();
        loop {
            while let Some(index) = buffer.find("\n\n") {
                let frame: String = buffer.drain(..index + 2).collect();
                if let Some(data) = frame.lines().find_map(|line| line.strip_prefix("data: ")) {
                    return Some(serde_json::from_str(data).unwrap());
                }
            }
            match response.chunk().await {
                Ok(Some(chunk)) => buffer.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(None) | Err(_) => return None,
            }
        }
    };
    tokio::time::timeout(timeout, read).await.unwrap_or(None)
}

/// Drain an SSE stream to its end, returning whether it terminated within `timeout`.
pub async fn sse_ends(response: &mut reqwest::Response, timeout: Duration) -> bool {
    let drain = async { while (response.chunk().await).is_ok_and(|chunk| chunk.is_some()) {} };
    tokio::time::timeout(timeout, drain).await.is_ok()
}
