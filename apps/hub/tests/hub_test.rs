//! End-to-end Hub flows over an in-memory store and a real loopback listener.

mod common;

use std::time::Duration;

use common::{is_closed, next_sse_event, read_frame, start, start_with_ui, ADMIN};
use myrlet_protocol::SNAPSHOT_SCHEMA_VERSION;
use serde_json::json;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

#[tokio::test]
async fn authentication_precedes_body_parsing_and_separates_principals() {
    let hub = start(10_000).await;
    let anonymous = hub
        .request("POST", "/v1/tasks", None, Some("k1"), Some("{nope"))
        .await;
    assert_eq!(anonymous.0, 401);
    let wrong = hub
        .request("POST", "/v1/tasks", Some("bad"), Some("k1"), Some("{nope"))
        .await;
    assert_eq!(wrong.0, 401);
    let malformed = hub.post_admin("/v1/tasks", Some("k1"), Some("{nope")).await;
    assert_eq!(malformed.0, 400);
    assert_eq!(malformed.1["error"], "BadRequest");

    let (_, credential) = hub.pair().await;
    let listed = hub
        .request("GET", "/v1/workers", Some(&credential), None, None)
        .await;
    assert_eq!(listed.0, 401);
    let progress = "/v1/worker/deliveries/delivery_x/progress";
    assert_eq!(hub.post_admin(progress, None, Some("{}")).await.0, 401);
    assert_eq!(hub.get_admin("/v1/workers/connect").await.0, 401);
}

#[tokio::test]
async fn pairing_wss_handshake_and_reported_inventory() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (_socket, epoch) = hub.connect(&worker_id, &credential).await;
    assert!(epoch >= 1);

    let listed = hub.get_admin("/v1/workers").await;
    assert_eq!(listed.0, 200);
    let worker = &listed.1["workers"][0];
    assert_eq!(worker["workerId"], worker_id);
    assert_eq!(worker["online"], true);
    assert_eq!(worker["inventory"]["slots"][0]["slotId"], "slot-a");
    assert_eq!(
        worker["inventory"]["slots"][0]["executors"][0]["id"],
        "exec-claude"
    );

    let snapshot = hub.get_admin("/v1/snapshot").await;
    assert_eq!(snapshot.1["schemaVersion"], SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snapshot.1["workers"][0]["online"], true);
}

#[tokio::test]
async fn match_dispatch_progress_and_finish_drive_the_task_to_finished() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (mut socket, epoch) = hub.connect(&worker_id, &credential).await;
    let (task_id, delivery_id) = hub.dispatch_one(&mut socket, "flow").await;

    let progress = json!({ "epoch": epoch, "observedAtMs": 1_000, "text": "started" });
    let path = format!("/v1/worker/deliveries/{delivery_id}/progress");
    let reported = hub
        .post_worker(&path, &credential, &progress.to_string())
        .await;
    assert_eq!(reported.0, 200, "{}", reported.1);
    assert_eq!(reported.1["duplicate"], false);

    let task_path = format!("/v1/tasks/{task_id}");
    let detail = hub.get_admin(&task_path).await;
    assert_eq!(detail.1["currentDelivery"]["state"], "accepted");
    assert_eq!(detail.1["currentDelivery"]["progress"]["text"], "started");

    let finish = json!({ "epoch": epoch }).to_string();
    let path = format!("/v1/worker/deliveries/{delivery_id}/finish");
    let finished = hub.post_worker(&path, &credential, &finish).await;
    assert_eq!(finished.0, 200, "{}", finished.1);
    assert_eq!(finished.1["taskStatus"], "finished");
    assert_eq!(finished.1["changed"], true);
}

#[tokio::test]
async fn dispatch_idempotency_replays_after_version_bump_and_rejects_cross_payload() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (_socket, _epoch) = hub.connect(&worker_id, &credential).await;

    let definition = json!({ "title": "Idempotent", "prompt": "replay me" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("idem-create"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    let task_id = created.1["id"].as_str().unwrap().to_string();
    let path = format!("/v1/tasks/{task_id}/dispatch");
    let dispatch = json!({
        "expectedTaskVersion": created.1["version"],
        "selection": { "mode": "match" },
    })
    .to_string();

    let first = hub
        .post_admin(&path, Some("k-disp-1"), Some(&dispatch))
        .await;
    assert_eq!(first.0, 200, "{}", first.1);
    assert_eq!(first.1["status"], "dispatched");
    let delivery_id = first.1["delivery"]["id"].as_str().unwrap().to_string();

    let replay = hub
        .post_admin(&path, Some("k-disp-1"), Some(&dispatch))
        .await;
    assert_eq!(replay.0, 200, "{}", replay.1);
    assert_eq!(replay.1["delivery"]["id"], delivery_id);
    assert_eq!(replay.1["version"], first.1["version"]);

    let changed = json!({
        "expectedTaskVersion": created.1["version"],
        "selection": { "mode": "match" },
        "queue": true,
    })
    .to_string();
    let conflict = hub
        .post_admin(&path, Some("k-disp-1"), Some(&changed))
        .await;
    assert_eq!(conflict.0, 409, "{}", conflict.1);
    assert_eq!(conflict.1["error"], "IdempotencyConflict");

    let queued_task = json!({ "title": "Queued", "prompt": "wait" }).to_string();
    let queued = hub
        .post_admin("/v1/tasks", Some("idem-create-queue"), Some(&queued_task))
        .await;
    assert_eq!(queued.0, 201, "{}", queued.1);
    let queued_id = queued.1["id"].as_str().unwrap().to_string();
    let queued_path = format!("/v1/tasks/{queued_id}/dispatch");
    let queue_dispatch = json!({
        "expectedTaskVersion": queued.1["version"],
        "selection": {
            "mode": "targeted",
            "workerId": "wkr_ZZZZZZZZZZZZ",
            "slotId": "slot-a",
        },
        "queue": true,
    })
    .to_string();
    let accepted = hub
        .post_admin(&queued_path, Some("k-disp-queue"), Some(&queue_dispatch))
        .await;
    assert_eq!(accepted.0, 202, "{}", accepted.1);
    assert_eq!(accepted.1["status"], "queued");
    assert_eq!(accepted.1["delivery"], serde_json::Value::Null);
    let queued_replay = hub
        .post_admin(&queued_path, Some("k-disp-queue"), Some(&queue_dispatch))
        .await;
    assert_eq!(queued_replay.0, 202, "{}", queued_replay.1);
    assert_eq!(queued_replay.1["version"], accepted.1["version"]);
    assert_eq!(queued_replay.1["delivery"], serde_json::Value::Null);
}

#[tokio::test]
async fn cancellation_fences_the_worker_and_survives_a_late_finish() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (mut socket, epoch) = hub.connect(&worker_id, &credential).await;
    let (task_id, delivery_id) = hub.dispatch_one(&mut socket, "cancel").await;

    let task_path = format!("/v1/tasks/{task_id}");
    let detail = hub.get_admin(&task_path).await;
    let cancel = json!({ "expectedTaskVersion": detail.1["version"], "reason": "operator" });
    let path = format!("/v1/tasks/{task_id}/cancel");
    let cancelled = hub
        .post_admin(&path, Some("cancel-key"), Some(&cancel.to_string()))
        .await;
    assert_eq!(cancelled.0, 200, "{}", cancelled.1);
    assert_eq!(cancelled.1["status"], "cancelled");

    let command = read_frame(&mut socket).await;
    assert_eq!(command["kind"], "task.cancel");
    assert_eq!(command["payload"]["deliveryId"], delivery_id);

    let finish = json!({ "epoch": epoch }).to_string();
    let path = format!("/v1/worker/deliveries/{delivery_id}/finish");
    let late = hub.post_worker(&path, &credential, &finish).await;
    assert_eq!(late.1["taskStatus"], "cancelled");
    assert_eq!(late.1["changed"], false);
    assert_eq!(hub.get_admin(&task_path).await.1["status"], "cancelled");
}

#[tokio::test]
async fn result_is_write_once_bounded_and_survives_late_sync_after_finish() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (mut socket, epoch) = hub.connect(&worker_id, &credential).await;
    let (task_id, delivery_id) = hub.dispatch_one(&mut socket, "result").await;
    let path = format!("/v1/worker/deliveries/{delivery_id}/result");

    // A Worker credential is required, and the task detail has no result yet.
    assert_eq!(hub.post_admin(&path, None, Some("{}")).await.0, 401);
    let task_path = format!("/v1/tasks/{task_id}");
    let detail = hub.get_admin(&task_path).await;
    assert!(detail.1["resultMarkdown"].is_null());

    let progress = json!({ "epoch": epoch, "observedAtMs": 1_000, "text": "started" });
    let progress_path = format!("/v1/worker/deliveries/{delivery_id}/progress");
    assert_eq!(
        hub.post_worker(&progress_path, &credential, &progress.to_string())
            .await
            .0,
        200
    );

    let report = json!({ "epoch": epoch, "text": "# Findings\n\n- one" });
    let recorded = hub
        .post_worker(&path, &credential, &report.to_string())
        .await;
    assert_eq!(recorded.0, 200, "{}", recorded.1);
    assert_eq!(recorded.1["duplicate"], false);

    // Identical replay is idempotent; different content is refused (write-once).
    let replay = hub
        .post_worker(&path, &credential, &report.to_string())
        .await;
    assert_eq!(replay.0, 200, "{}", replay.1);
    assert_eq!(replay.1["duplicate"], true);
    let changed = json!({ "epoch": epoch, "text": "# Different" });
    let refused = hub
        .post_worker(&path, &credential, &changed.to_string())
        .await;
    assert_eq!(refused.0, 409, "{}", refused.1);

    // Oversized and blank text are rejected without touching the stored result.
    let oversized = json!({ "epoch": epoch, "text": "x".repeat(64 * 1024 + 1) });
    let too_large = hub
        .post_worker(&path, &credential, &oversized.to_string())
        .await;
    assert_eq!(too_large.0, 400, "{}", too_large.1);

    // Finish is independent, and a late sync after finish still lands in Task detail.
    let finish = json!({ "epoch": epoch }).to_string();
    let finish_path = format!("/v1/worker/deliveries/{delivery_id}/finish");
    assert_eq!(
        hub.post_worker(&finish_path, &credential, &finish).await.0,
        200
    );
    let (task_id, delivery_id) = hub.dispatch_one(&mut socket, "late").await;
    let finish_path = format!("/v1/worker/deliveries/{delivery_id}/finish");
    assert_eq!(
        hub.post_worker(&finish_path, &credential, &finish).await.0,
        200
    );
    let late_path = format!("/v1/worker/deliveries/{delivery_id}/result");
    drop(socket);
    let (_reconnected, new_epoch) = hub.connect(&worker_id, &credential).await;
    assert!(new_epoch > epoch);
    let late = json!({ "epoch": new_epoch, "text": "late result" });
    let accepted = hub
        .post_worker(&late_path, &credential, &late.to_string())
        .await;
    assert_eq!(accepted.0, 200, "{}", accepted.1);
    let late_task = format!("/v1/tasks/{task_id}");
    let shown = hub.get_admin(&late_task).await;
    assert_eq!(shown.1["status"], "finished");
    assert_eq!(shown.1["resultMarkdown"], "late result");

    // A stale epoch never records a result.
    let stale = json!({ "epoch": epoch, "text": "stale" });
    let stale_path = format!("/v1/worker/deliveries/{delivery_id}/result");
    let refused = hub
        .post_worker(&stale_path, &credential, &stale.to_string())
        .await;
    assert_eq!(refused.0, 409, "{}", refused.1);
    assert_eq!(
        hub.get_admin(&late_task).await.1["resultMarkdown"],
        "late result"
    );
}

#[tokio::test]
async fn event_stream_backfills_streams_and_fails_closed_when_expired() {
    let hub = start(10_000).await;
    let definition = json!({ "title": "t", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("sse-1"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);

    let mut stream = reqwest::Client::new()
        .get(format!("{}/v1/events", hub.base))
        .bearer_auth(ADMIN)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status().as_u16(), 200);
    let backfilled = tokio::time::timeout(Duration::from_secs(5), next_sse_event(&mut stream));
    let backfilled = backfilled.await.unwrap();
    assert_eq!(backfilled["kind"], "task.created");

    let live = hub.post_admin("/v1/tasks", Some("sse-2"), Some(&definition));
    assert_eq!(live.await.0, 201);
    let live = tokio::time::timeout(Duration::from_secs(5), next_sse_event(&mut stream));
    let live = live.await.unwrap();
    assert!(live["seq"].as_u64().unwrap() > backfilled["seq"].as_u64().unwrap());

    let tiny = start(1).await;
    for index in 0..3 {
        let key = format!("tiny-{index}");
        let created = tiny.post_admin("/v1/tasks", Some(&key), Some(&definition));
        assert_eq!(created.await.0, 201);
    }
    let expired = tiny.get_admin("/v1/events?cursor=0").await;
    assert_eq!(expired.0, 410);
    assert_eq!(expired.1["error"], "CursorExpired");
}

#[tokio::test]
async fn rotation_and_revocation_fence_the_live_worker_session() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let (mut socket, _) = hub.connect(&worker_id, &credential).await;

    let path = format!("/v1/workers/{worker_id}/rotate");
    let rotated = hub.post_admin(&path, None, None).await;
    assert_eq!(rotated.0, 200, "{}", rotated.1);
    let rotated_credential = rotated.1["credential"].as_str().unwrap().to_string();
    assert_ne!(rotated_credential, credential);
    assert!(is_closed(&mut socket).await);

    let mut request = hub.connect_url.clone().into_client_request().unwrap();
    let stale = HeaderValue::from_str(&format!("Bearer {credential}")).unwrap();
    request.headers_mut().insert("authorization", stale);
    assert!(tokio_tungstenite::connect_async(request).await.is_err());

    let (mut socket, _) = hub.connect(&worker_id, &rotated_credential).await;
    let path = format!("/v1/workers/{worker_id}/revoke");
    assert_eq!(hub.post_admin(&path, None, None).await.0, 204);
    assert!(is_closed(&mut socket).await);

    let listed = hub.get_admin("/v1/workers").await;
    assert_eq!(listed.1["workers"][0]["revoked"], true);
    assert_eq!(listed.1["workers"][0]["online"], false);
}

#[tokio::test]
async fn embedded_ui_never_shadows_the_api_and_respects_the_toggle() {
    let hub = start(10_000).await;
    let unknown = hub.get_admin("/v1/does-not-exist").await;
    assert_eq!(unknown.0, 404, "{}", unknown.1);
    assert_eq!(unknown.1["error"], "NotFound");

    let disabled = start_with_ui(10_000, false).await;
    for path in ["/", "/tasks/task_1"] {
        let (status, body) = disabled.request("GET", path, None, None, None).await;
        assert_eq!(status, 404, "{path}: {body}");
        assert_eq!(body["error"], "NotFound", "{path}");
    }

    let enabled = start_with_ui(10_000, true).await;
    let response = reqwest::Client::new()
        .get(format!("{}/", enabled.base))
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    assert!(status == 200 || status == 404, "unexpected status {status}");
    if status == 200 {
        let headers = response.headers();
        let content_type = headers["content-type"].to_str().unwrap();
        assert!(content_type.contains("text/html"), "{content_type}");
        assert_eq!(headers["x-frame-options"], "DENY");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert!(headers.contains_key("content-security-policy"));
        let unknown = enabled.get_admin("/v1/unknown").await;
        assert_eq!(unknown.0, 404, "{}", unknown.1);
        assert_eq!(unknown.1["error"], "NotFound");
    }
}
