//! Real SSE lifecycle: an established global stream must stop exposing events as soon as
//! the Worker credential behind it loses global-read authorization.

mod common;

use std::time::Duration;

use common::{open_events, start, try_next_sse_event};
use serde_json::json;

/// How long a demoted/revoked stream may keep delivering events before it must end.
const REVOCATION_BOUND: Duration = Duration::from_secs(5);
/// How long to wait for a committed event that the stream must still deliver.
const DELIVERY_BOUND: Duration = Duration::from_secs(5);

/// Submit one Task as Admin, producing a global `task.created` invalidation.
async fn publish_event(hub: &common::Hub, key: &str) -> serde_json::Value {
    let definition = json!({ "title": key, "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some(key), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    created.1
}

/// Read SSE frames until one names `task_id` (skipping replayed history and other kinds).
async fn wait_for_task(
    stream: &mut reqwest::Response,
    task_id: &str,
    timeout: Duration,
) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let event = try_next_sse_event(stream, remaining)
            .await
            .unwrap_or_else(|| panic!("stream ended before an event for {task_id}"));
        if event["payload"]["taskId"] == task_id {
            return event;
        }
    }
}

/// An owner-tier Worker with a live global SSE stream, proven by one delivered event.
async fn owner_stream(hub: &common::Hub) -> (String, String, reqwest::Response) {
    let (worker_id, credential) = hub.pair().await;
    hub.set_tier(&worker_id, "owner").await;
    let mut stream = open_events(hub, &credential, None).await;
    let task = publish_event(hub, "stream-open").await;
    let task_id = task["task"]["id"].as_str().unwrap().to_owned();
    wait_for_task(&mut stream, &task_id, DELIVERY_BOUND).await;
    (worker_id, credential, stream)
}

/// Assert the stream terminates promptly and delivers nothing after it ends.
///
/// The revocation event itself may still be in flight when the credential changes; the
/// stream is allowed to deliver frames until it observes the invalidation, then must stop.
async fn assert_stream_revoked(stream: &mut reqwest::Response) {
    // Drain any in-flight frames and require the stream to end within the bound.
    let drain = async { while try_next_sse_event(stream, REVOCATION_BOUND).await.is_some() {} };
    tokio::time::timeout(REVOCATION_BOUND, drain)
        .await
        .expect("a revoked stream must terminate");
    // A terminated stream never resumes with a later event.
    assert!(
        try_next_sse_event(stream, Duration::from_millis(200))
            .await
            .is_none(),
        "a terminated stream cannot resume"
    );
}

#[tokio::test]
async fn demoting_an_owner_terminates_its_global_stream() {
    let hub = start(10_000).await;
    let (worker_id, _credential, mut stream) = owner_stream(&hub).await;

    hub.set_tier(&worker_id, "worker").await;
    publish_event(&hub, "after-demotion").await;

    assert_stream_revoked(&mut stream).await;
}

#[tokio::test]
async fn revoking_a_worker_terminates_its_global_stream() {
    let hub = start(10_000).await;
    let (worker_id, _credential, mut stream) = owner_stream(&hub).await;

    let path = format!("/v1/workers/{worker_id}/revoke");
    assert_eq!(hub.post_admin(&path, None, None).await.0, 204);
    publish_event(&hub, "after-revocation").await;

    assert_stream_revoked(&mut stream).await;
}

#[tokio::test]
async fn rotating_a_worker_terminates_its_global_stream() {
    let hub = start(10_000).await;
    let (worker_id, _credential, mut stream) = owner_stream(&hub).await;

    let path = format!("/v1/workers/{worker_id}/rotate");
    let rotated = hub.post_admin(&path, None, None).await;
    assert_eq!(rotated.0, 200, "rotation failed: {rotated:?}");
    publish_event(&hub, "after-rotation").await;

    // The stream was opened with the previous credential, which rotation fenced.
    assert_stream_revoked(&mut stream).await;
}

#[tokio::test]
async fn an_admin_stream_keeps_receiving_events() {
    let hub = start(10_000).await;
    let mut stream = open_events(&hub, common::ADMIN, None).await;

    for key in ["admin-1", "admin-2"] {
        let task = publish_event(&hub, key).await;
        let task_id = task["task"]["id"].as_str().unwrap().to_owned();
        wait_for_task(&mut stream, &task_id, DELIVERY_BOUND).await;
    }
}

#[tokio::test]
async fn a_worker_tier_credential_cannot_open_a_global_stream() {
    let hub = start(10_000).await;
    let (_, credential) = hub.pair().await;
    // A new pairing is `worker` tier: global event reads are forbidden.
    let denied = hub
        .request("GET", "/v1/events", Some(&credential), None, None)
        .await;
    assert_eq!(denied.0, 403, "{}", denied.1);
    assert_eq!(denied.1["requiredTier"], "owner");
    assert_eq!(denied.1["currentTier"], "worker");
}
