//! `Client` behavior verified against an in-process axum Hub stub.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use myrlet_client::{Client, ClientError};
use myrlet_protocol::{TaskDefinition, TaskId, SNAPSHOT_SCHEMA_VERSION};
use serde_json::{json, Value};

async fn spawn(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

fn snapshot(schema_version: &str) -> Value {
    json!({
        "schemaVersion": schema_version,
        "generatedAtMs": 1_764_070_000_500i64,
        "eventCursor": 41,
        "tasksTruncated": false,
        "totalTasks": 0,
        "workers": [],
        "tasks": []
    })
}

fn task_record() -> Value {
    json!({
        "id": "task_01",
        "rootTaskId": "task_01",
        "depth": 1,
        "status": "held",
        "version": 1,
        "createdAtMs": 10,
        "updatedAtMs": 10,
        "finishedAtMs": null,
        "task": { "title": "Verify the build", "prompt": "Run the test suite." },
        "currentDelivery": null,
        "subtree": { "total": 1, "held": 1, "queued": 0, "dispatched": 0, "finished": 0, "cancelled": 0 }
    })
}

#[tokio::test]
async fn health_and_snapshot_match_the_contract() {
    let app = Router::new()
        .route(
            "/v1/health",
            get(|| async { Json(json!({ "status": "ok", "version": "0.1.0" })) }),
        )
        .route(
            "/v1/snapshot",
            get(|| async { Json(snapshot(SNAPSHOT_SCHEMA_VERSION)) }),
        );
    let client = Client::new(spawn(app).await, "admin-token").unwrap();

    let health = client.health().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.version, "0.1.0");

    let snapshot = client.snapshot().await.unwrap();
    assert_eq!(snapshot.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snapshot.event_cursor, 41);
}

#[tokio::test]
async fn snapshot_with_unknown_schema_version_fails_closed() {
    let app = Router::new().route(
        "/v1/snapshot",
        get(|| async { Json(snapshot("myrlet.snapshot/v2")) }),
    );
    let client = Client::new(spawn(app).await, "admin-token").unwrap();
    assert!(matches!(
        client.snapshot().await,
        Err(ClientError::Decode(_))
    ));
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(String, String)>>>);

async fn create_task(State(captured): State<Captured>, headers: HeaderMap) -> impl IntoResponse {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    captured
        .0
        .lock()
        .unwrap()
        .push((header("authorization"), header("idempotency-key")));
    Json(json!({ "task": task_record(), "eligible": true }))
}

#[tokio::test]
async fn create_task_sends_bearer_and_idempotency_key() {
    let captured = Captured::default();
    let app = Router::new()
        .route("/v1/tasks", post(create_task))
        .with_state(captured.clone());
    let client = Client::new(spawn(app).await, "admin-token").unwrap();
    let def: TaskDefinition =
        serde_json::from_str(r#"{"title":"Verify the build","prompt":"Run the test suite."}"#)
            .unwrap();

    let record = client.create_task("key-1", &def).await.unwrap();
    assert_eq!(record.id, TaskId::new("task_01"));
    assert_eq!(
        *captured.0.lock().unwrap(),
        vec![("Bearer admin-token".to_string(), "key-1".to_string())]
    );
}

#[tokio::test]
async fn hub_errors_map_to_typed_client_errors() {
    let app = Router::new()
        .route(
            "/v1/snapshot",
            get(|| async {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Unauthorized", "message": "invalid token" })),
                )
            }),
        )
        .route(
            "/v1/tasks/:id",
            get(|| async {
                (
                    StatusCode::GONE,
                    Json(json!({ "error": "CursorExpired", "message": "cursor expired" })),
                )
            }),
        );
    let client = Client::new(spawn(app).await, "stale-token").unwrap();

    let error = client.snapshot().await.unwrap_err();
    assert_eq!(error, ClientError::Unauthorized);
    assert!(!error.is_retryable());

    let error = client.get_task(&TaskId::new("task_01")).await.unwrap_err();
    assert_eq!(error, ClientError::CursorExpired);
}

#[test]
fn debug_output_redacts_the_bearer() {
    let client = Client::new("http://127.0.0.1:1", "super-secret-token").unwrap();
    let rendered = format!("{client:?}");
    assert!(!rendered.contains("super-secret-token"), "{rendered}");
    assert!(rendered.contains("redacted"), "{rendered}");
}

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Option<String>>>);

async fn events(State(seen): State<Seen>, headers: HeaderMap) -> Response {
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    *seen.0.lock().unwrap() = last_event_id;

    let data = r#"{"seq":43,"kind":"task.status_changed","createdAtMs":123,"payload":{"title":"héllo ✓"}}"#;
    let body = format!("id: 43\nevent: task.status_changed\ndata: {data}\n\n").into_bytes();
    // Split inside a multi-byte UTF-8 character so chunks never align to char boundaries.
    let split = body.iter().position(|byte| *byte == 0xc3).unwrap() + 1;
    let chunks = vec![
        body[..split].to_vec(),
        body[split..split + 3].to_vec(),
        body[split + 3..].to_vec(),
    ];
    let stream = futures::stream::iter(chunks).then(|chunk| async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok::<Vec<u8>, std::io::Error>(chunk)
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}

#[tokio::test]
async fn open_events_streams_fragmented_utf8_with_last_event_id() {
    let seen = Seen::default();
    let app = Router::new()
        .route("/v1/events", get(events))
        .with_state(seen.clone());
    let client = Client::new(spawn(app).await, "admin-token").unwrap();

    let mut stream = client.open_events(Some(42)).await.unwrap();
    let event = stream.next().await.unwrap().unwrap();
    assert_eq!(event.seq, 43);
    assert_eq!(event.kind, "task.status_changed");
    assert_eq!(event.payload["title"], "héllo ✓");
    assert!(stream.next().await.is_none());
    assert_eq!(seen.0.lock().unwrap().as_deref(), Some("42"));
}

#[test]
fn invalid_base_urls_are_rejected() {
    assert!(matches!(
        Client::new("not-a-url", "token"),
        Err(ClientError::InvalidUrl(_))
    ));
    assert!(matches!(
        Client::new("ftp://hub.example", "token"),
        Err(ClientError::InvalidUrl(_))
    ));
}

#[tokio::test]
async fn path_traversal_ids_are_rejected_client_side() {
    let client = Client::new("http://127.0.0.1:1", "token").unwrap();
    let bad = client.get_task(&TaskId::new("../workers")).await;
    assert!(matches!(bad, Err(ClientError::InvalidUrl(_))));
}
