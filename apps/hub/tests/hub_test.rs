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
    // A Worker credential may read the fleet but may never manage it.
    let listed = hub
        .request("GET", "/v1/workers", Some(&credential), None, None)
        .await;
    assert_eq!(listed.0, 200);
    let rotate = format!(
        "/v1/workers/{}/rotate",
        listed.1["workers"][0]["workerId"].as_str().unwrap()
    );
    let denied = hub
        .request("POST", &rotate, Some(&credential), None, None)
        .await;
    assert_eq!(denied.0, 401, "a Worker read credential must not rotate");
    let progress = "/v1/worker/deliveries/delivery_x/progress";
    assert_eq!(hub.post_admin(progress, None, Some("{}")).await.0, 401);
    assert_eq!(hub.get_admin("/v1/workers/connect").await.0, 401);
}

#[tokio::test]
async fn workers_can_read_tasks_trees_workers_and_events_without_the_admin_token() {
    let hub = start(10_000).await;
    let (worker_id, credential, _socket, _epoch) = hub.enrolled().await;
    let definition = json!({ "title": "Readable", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("read-1"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    let task_id = created.1["task"]["id"].as_str().unwrap().to_string();

    for path in [
        "/v1/tasks".to_string(),
        format!("/v1/tasks/{task_id}"),
        format!("/v1/tasks/{task_id}/tree"),
        format!("/v1/tasks/{task_id}/eligibility"),
        "/v1/workers".to_string(),
        format!("/v1/workers/{worker_id}"),
    ] {
        let (status, _) = hub
            .request("GET", &path, Some(&credential), None, None)
            .await;
        assert_eq!(status, 200, "a Worker credential may read {path}");
    }
    // A read credential cannot perform management writes.
    let cancel = json!({ "expectedTaskVersion": created.1["task"]["version"] }).to_string();
    let denied = hub
        .request(
            "POST",
            &format!("/v1/tasks/{task_id}/cancel"),
            Some(&credential),
            Some("read-cancel"),
            Some(&cancel),
        )
        .await;
    assert_eq!(denied.0, 401);
}

#[tokio::test]
async fn submit_authorizes_ready_work_and_holds_blocked_work() {
    let hub = start(10_000).await;
    let root = json!({ "title": "Root", "prompt": "p" }).to_string();
    let created = hub.post_admin("/v1/tasks", Some("root"), Some(&root)).await;
    assert_eq!(created.0, 201, "{}", created.1);
    assert_eq!(created.1["task"]["status"], "queued");
    assert_eq!(created.1["eligible"], true);
    let root_id = created.1["task"]["id"].as_str().unwrap().to_string();

    let first = json!({ "title": "First", "prompt": "p", "parentTaskId": root_id }).to_string();
    let first = hub
        .post_admin("/v1/tasks", Some("first"), Some(&first))
        .await;
    assert_eq!(first.1["task"]["status"], "queued");
    let first_id = first.1["task"]["id"].as_str().unwrap().to_string();

    let blocked = json!({
        "title": "Blocked",
        "prompt": "p",
        "parentTaskId": root_id,
        "dependencies": [first_id],
    })
    .to_string();
    let blocked = hub
        .post_admin("/v1/tasks", Some("blocked"), Some(&blocked))
        .await;
    assert_eq!(blocked.0, 201, "{}", blocked.1);
    assert_eq!(blocked.1["task"]["status"], "held");
    assert_eq!(blocked.1["eligible"], false);
    assert_eq!(blocked.1["task"]["blockedBy"][0]["taskId"], first_id);
}

#[tokio::test]
async fn waking_a_worker_creates_no_delivery_and_a_claim_commits_one() {
    let hub = start(10_000).await;
    let (_worker_id, credential, mut socket, epoch) = hub.enrolled().await;

    let definition = json!({ "title": "Wake", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("wake"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    let task_id = created.1["task"]["id"].as_str().unwrap().to_string();

    // The Hub wakes the eligible Worker, but the hint reserves nothing.
    let wakeup = read_frame(&mut socket).await;
    assert_eq!(wakeup["kind"], "hub.wakeup");
    assert_eq!(wakeup["payload"]["taskId"], task_id);
    let shown = hub.get_admin(&format!("/v1/tasks/{task_id}")).await;
    assert_eq!(shown.1["status"], "queued");
    assert!(
        shown.1["currentDelivery"].is_null(),
        "a wakeup is only a hint"
    );

    // The Worker claims by Task ID and Slot ID; the Hub commits the sole Delivery.
    let claim = json!({
        "taskId": task_id,
        "slotId": "slot-a",
        "connectionEpoch": epoch,
    })
    .to_string();
    let claimed = hub
        .post_worker("/v1/workers/claim", &credential, &claim)
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);
    assert_eq!(claimed.1["task"]["status"], "dispatched");
    assert_eq!(claimed.1["delivery"]["state"], "accepted");
    assert_eq!(claimed.1["delivery"]["slotId"], "slot-a");

    // The winning Worker can replay its claim without creating another Delivery.
    let repeated = hub
        .post_worker("/v1/workers/claim", &credential, &claim)
        .await;
    assert_eq!(repeated.0, 200, "{}", repeated.1);
    assert_eq!(repeated.1["delivery"]["id"], claimed.1["delivery"]["id"]);
    let shown = hub.get_admin(&format!("/v1/tasks/{task_id}")).await;
    assert_eq!(
        shown.1["currentDelivery"]["id"],
        claimed.1["delivery"]["id"]
    );
}

#[tokio::test]
async fn a_stale_epoch_cannot_claim() {
    let hub = start(10_000).await;
    let (_worker_id, credential, _socket, epoch) = hub.enrolled().await;
    let definition = json!({ "title": "Epoch", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("epoch"), Some(&definition))
        .await;
    let task_id = created.1["task"]["id"].as_str().unwrap().to_string();
    let claim = json!({
        "taskId": task_id,
        "slotId": "slot-a",
        "connectionEpoch": epoch + 1,
    })
    .to_string();
    let stale = hub
        .post_worker("/v1/workers/claim", &credential, &claim)
        .await;
    assert_eq!(stale.0, 409, "{}", stale.1);
    assert_eq!(stale.1["error"], "StaleDelivery");
}

#[tokio::test]
async fn claim_hold_finish_and_progress_drive_the_task_to_finished() {
    let hub = start(10_000).await;
    let (_worker_id, credential, mut socket, epoch) = hub.enrolled().await;
    let (task_id, delivery_id) = hub.claim_one(&mut socket, &credential, epoch, "flow").await;

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
async fn cancellation_fences_the_worker_and_survives_a_late_finish() {
    let hub = start(10_000).await;
    let (_worker_id, credential, mut socket, epoch) = hub.enrolled().await;
    let (task_id, delivery_id) = hub
        .claim_one(&mut socket, &credential, epoch, "cancel")
        .await;

    let task_path = format!("/v1/tasks/{task_id}");
    let detail = hub.get_admin(&task_path).await;
    let cancel = json!({
        "expectedTaskVersion": detail.1["version"],
        "reason": "operator",
    });
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
async fn result_reconciles_latest_content_and_clears_when_absent() {
    let hub = start(10_000).await;
    let (_worker_id, credential, mut socket, epoch) = hub.enrolled().await;
    let (task_id, delivery_id) = hub
        .claim_one(&mut socket, &credential, epoch, "result")
        .await;
    let path = format!("/v1/worker/deliveries/{delivery_id}/result");

    assert_eq!(hub.post_admin(&path, None, Some("{}")).await.0, 401);
    let task_path = format!("/v1/tasks/{task_id}");
    assert!(hub.get_admin(&task_path).await.1["resultMarkdown"].is_null());

    let report = json!({ "epoch": epoch, "text": "# Findings\n\n- one" });
    let recorded = hub
        .post_worker(&path, &credential, &report.to_string())
        .await;
    assert_eq!(recorded.0, 200, "{}", recorded.1);
    assert_eq!(recorded.1["duplicate"], false);
    assert_eq!(recorded.1["cleared"], false);
    assert_eq!(recorded.1["resultSha256"].as_str().unwrap().len(), 64);

    // Equal content needs no write; different content replaces it.
    let replay = hub
        .post_worker(&path, &credential, &report.to_string())
        .await;
    assert_eq!(replay.1["duplicate"], true);
    let changed = json!({ "epoch": epoch, "text": "# Different" });
    let replaced = hub
        .post_worker(&path, &credential, &changed.to_string())
        .await;
    assert_eq!(replaced.0, 200, "{}", replaced.1);
    assert_eq!(
        hub.get_admin(&task_path).await.1["resultMarkdown"],
        "# Different"
    );

    // The Worker can read its own copy back for comparison without writing.
    let read_back = hub
        .request("GET", &path, Some(&credential), None, None)
        .await;
    assert_eq!(read_back.0, 200, "{}", read_back.1);
    assert_eq!(read_back.1["text"], "# Different");

    // Finish is independent, and an explicit absence clears the stored value.
    let finish = json!({ "epoch": epoch }).to_string();
    let finish_path = format!("/v1/worker/deliveries/{delivery_id}/finish");
    assert_eq!(
        hub.post_worker(&finish_path, &credential, &finish).await.0,
        200
    );
    let cleared = json!({ "epoch": epoch, "text": null });
    let cleared = hub
        .post_worker(&path, &credential, &cleared.to_string())
        .await;
    assert_eq!(cleared.0, 200, "{}", cleared.1);
    assert_eq!(cleared.1["cleared"], true);
    assert!(hub.get_admin(&task_path).await.1["resultMarkdown"].is_null());
    let repeat = hub
        .post_worker(
            &path,
            &credential,
            &json!({ "epoch": epoch, "text": null }).to_string(),
        )
        .await;
    assert_eq!(repeat.1["duplicate"], true, "repeating a clear is harmless");
}

#[tokio::test]
async fn a_delayed_old_epoch_result_cannot_overwrite_a_newer_value() {
    let hub = start(10_000).await;
    let (worker_id, credential, mut socket, epoch1) = hub.enrolled().await;
    let (task_id, delivery_id) = hub
        .claim_one(&mut socket, &credential, epoch1, "late")
        .await;
    let path = format!("/v1/worker/deliveries/{delivery_id}/result");
    let v1 = json!({ "epoch": epoch1, "text": "# v1" });
    assert_eq!(
        hub.post_worker(&path, &credential, &v1.to_string()).await.0,
        200
    );

    drop(socket);
    let (_socket, epoch2) = hub.connect(&worker_id, &credential).await;
    assert!(epoch2 > epoch1);
    let v2 = json!({ "epoch": epoch2, "text": "# v2" });
    assert_eq!(
        hub.post_worker(&path, &credential, &v2.to_string()).await.0,
        200
    );
    // The stale request arrives after the reconnect and is refused at the boundary.
    let stale = json!({ "epoch": epoch1, "text": "# v1 delayed" });
    let refused = hub
        .post_worker(&path, &credential, &stale.to_string())
        .await;
    assert_eq!(refused.0, 409, "{}", refused.1);
    let task_path = format!("/v1/tasks/{task_id}");
    assert_eq!(hub.get_admin(&task_path).await.1["resultMarkdown"], "# v2");
}

#[tokio::test]
async fn a_finished_prerequisite_releases_only_ready_dependents() {
    let hub = start(10_000).await;

    let root = json!({ "title": "Root", "prompt": "p" }).to_string();
    let root = hub.post_admin("/v1/tasks", Some("r"), Some(&root)).await;
    let root_id = root.1["task"]["id"].as_str().unwrap().to_string();
    let first = json!({ "title": "First", "prompt": "p", "parentTaskId": root_id }).to_string();
    let first = hub.post_admin("/v1/tasks", Some("f"), Some(&first)).await;
    let first_id = first.1["task"]["id"].as_str().unwrap().to_string();
    let second = json!({
        "title": "Second",
        "prompt": "p",
        "parentTaskId": root_id,
        "dependencies": [first_id],
    })
    .to_string();
    let second = hub.post_admin("/v1/tasks", Some("s"), Some(&second)).await;
    let second_id = second.1["task"]["id"].as_str().unwrap().to_string();
    assert_eq!(second.1["task"]["status"], "held");

    // Finish the coordination-only prerequisite; its dependent becomes eligible.
    let finish = json!({ "expectedTaskVersion": first.1["task"]["version"] }).to_string();
    let closed = hub
        .post_admin(
            &format!("/v1/tasks/{first_id}/finish"),
            Some("close-first"),
            Some(&finish),
        )
        .await;
    assert_eq!(closed.0, 200, "{}", closed.1);
    let released = hub.get_admin(&format!("/v1/tasks/{second_id}")).await;
    assert_eq!(released.1["status"], "queued");
    assert_eq!(released.1["blockedBy"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn dispatch_idempotency_replays_after_version_bump_and_rejects_cross_payload() {
    let hub = start(10_000).await;

    let definition = json!({ "title": "Idempotent", "prompt": "replay me" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("idem-create"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    let replay = hub
        .post_admin("/v1/tasks", Some("idem-create"), Some(&definition))
        .await;
    assert_eq!(replay.0, 201, "{}", replay.1);
    assert_eq!(replay.1["task"]["id"], created.1["task"]["id"]);
    let changed = json!({ "title": "Different", "prompt": "replay me" }).to_string();
    let conflict = hub
        .post_admin("/v1/tasks", Some("idem-create"), Some(&changed))
        .await;
    assert_eq!(conflict.0, 409, "{}", conflict.1);
    assert_eq!(conflict.1["error"], "IdempotencyConflict");
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
    let (worker_id, credential, mut socket, _) = hub.enrolled().await;

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
async fn snapshot_and_task_list_stay_bounded_and_camel_case() {
    let hub = start(10_000).await;
    let definition = json!({ "title": "t", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("list-1"), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);

    let snapshot = hub.get_admin("/v1/snapshot").await;
    assert_eq!(snapshot.1["schemaVersion"], SNAPSHOT_SCHEMA_VERSION);
    assert!(snapshot.1["tasks"][0]["blockedBy"].is_array());
    let listed = hub.get_admin("/v1/tasks?limit=1").await;
    assert_eq!(listed.0, 200, "{}", listed.1);
    assert_eq!(listed.1["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(listed.1["totalTasks"], 1);
    assert_eq!(listed.1["truncated"], false);
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

#[tokio::test]
async fn same_worker_recovers_claim_with_discarded_response() {
    let hub = start(10_000).await;
    let (worker, cred, mut socket, epoch) = hub.enrolled().await;
    let task = hub
        .post_admin(
            "/v1/tasks",
            Some("lost"),
            Some(r#"{"title":"test","prompt":"test"}"#),
        )
        .await;
    let id = task.1["task"]["id"].as_str().unwrap();
    assert_eq!(read_frame(&mut socket).await["kind"], "hub.wakeup");
    let claim = |epoch| json!({"taskId":id,"slotId":"slot-a","connectionEpoch":epoch}).to_string();
    // The test transport discards the first response. The simulated Worker never learns the ID.
    assert_eq!(
        hub.post_worker("/v1/workers/claim", &cred, &claim(epoch))
            .await
            .0,
        200
    );
    let committed = hub.get_admin(&format!("/v1/tasks/{id}")).await.1;
    assert_eq!(committed["status"], "dispatched");
    let (_next, new_epoch) = hub.connect(&worker, &cred).await;
    assert!(new_epoch > epoch);
    let (code, replay) = hub
        .post_worker("/v1/workers/claim", &cred, &claim(new_epoch))
        .await;
    assert_eq!(
        code, 200,
        "committed {}, replay returned {replay}",
        committed["currentDelivery"]["id"]
    );
    assert_eq!(replay["delivery"]["id"], committed["currentDelivery"]["id"]);
    assert_eq!(replay["delivery"]["connectionEpoch"], new_epoch);
    let wrong_slot = json!({"taskId":id,"slotId":"slot-b","connectionEpoch":new_epoch}).to_string();
    assert_eq!(
        hub.post_worker("/v1/workers/claim", &cred, &wrong_slot)
            .await
            .0,
        409
    );
}

#[tokio::test]
async fn wakeup_rotates_past_two_silent_workers() {
    let hub = start(10_000).await;
    let mut paired = [hub.pair().await, hub.pair().await, hub.pair().await];
    paired.sort();
    let mut connected = Vec::new();
    for (worker_id, credential) in &paired {
        let (socket, epoch) = hub.connect(worker_id, credential).await;
        hub.clear_trust_gates(worker_id).await;
        connected.push((socket, epoch));
    }
    let (mut third, epoch) = connected.pop().expect("third worker");
    let (mut second, _) = connected.pop().expect("second worker");
    let (mut first, _) = connected.pop().expect("first worker");
    let workers = paired;
    let submitted = hub
        .post_admin(
            "/v1/tasks",
            Some("rotate"),
            Some(r#"{"title":"Rotate","prompt":"test"}"#),
        )
        .await;
    assert_eq!(submitted.0, 201);
    let id = submitted.1["task"]["id"].as_str().unwrap();
    for socket in [&mut first, &mut second, &mut third] {
        let wake = read_frame(socket).await;
        assert_eq!(wake["kind"], "hub.wakeup");
        assert_eq!(wake["payload"]["taskId"], id);
    }
    let state = hub.get_admin(&format!("/v1/tasks/{id}")).await.1;
    assert_eq!(state["status"], "queued");
    assert!(state["currentDelivery"].is_null());
    let claim = json!({"taskId":id,"slotId":"slot-a","connectionEpoch":epoch}).to_string();
    let claimed = hub
        .post_worker("/v1/workers/claim", &workers[2].1, &claim)
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);
    assert_eq!(claimed.1["delivery"]["workerId"], workers[2].0);
}
