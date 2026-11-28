//! Worker approval, configuration Apply, and the Security & Trust settings API.

mod common;

use common::{read_frame, start, ADMIN};
use serde_json::json;

/// Pair one Worker and complete the `worker.hello` handshake without clearing any gate.
async fn pending_worker(hub: &common::Hub) -> (String, String, common::Socket, u64) {
    let (worker_id, credential) = hub.pair().await;
    let (socket, epoch) = hub.connect(&worker_id, &credential).await;
    (worker_id, credential, socket, epoch)
}

fn claim_body(task_id: &str, epoch: u64) -> String {
    json!({ "taskId": task_id, "slotId": "slot-a", "connectionEpoch": epoch }).to_string()
}

async fn queued_task(hub: &common::Hub, key: &str) -> String {
    let definition = json!({ "title": "Trust", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some(key), Some(&definition))
        .await;
    assert_eq!(created.0, 201, "{}", created.1);
    created.1["task"]["id"]
        .as_str()
        .expect("task id")
        .to_owned()
}

#[tokio::test]
async fn policy_defaults_are_on_and_readable_by_admin_only() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let admin = hub.get_admin("/v1/settings/security-trust").await;
    assert_eq!(admin.0, 200, "{}", admin.1);
    assert_eq!(admin.1["requireWorkerApproval"], true);
    assert_eq!(admin.1["requireConfigurationApply"], true);

    // Even an owner-tier Worker cannot read or change system security configuration.
    hub.set_tier(&worker_id, "owner").await;
    let worker_read = hub
        .request(
            "GET",
            "/v1/settings/security-trust",
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(worker_read.0, 403, "{}", worker_read.1);
    assert_eq!(worker_read.1["requiredTier"], "admin");
    let denied = hub
        .request(
            "PATCH",
            "/v1/settings/security-trust",
            Some(&credential),
            None,
            Some(r#"{"requireWorkerApproval":false}"#),
        )
        .await;
    assert_eq!(denied.0, 403);
    assert_eq!(
        hub.get_admin("/v1/settings/security-trust").await.1["requireWorkerApproval"],
        true
    );
}

#[tokio::test]
async fn an_admin_updates_one_policy_and_the_other_is_preserved() {
    let hub = start(10_000).await;
    let patched = hub
        .request(
            "PATCH",
            "/v1/settings/security-trust",
            Some(ADMIN),
            None,
            Some(r#"{"requireConfigurationApply":false}"#),
        )
        .await;
    assert_eq!(patched.0, 200, "{}", patched.1);
    assert_eq!(patched.1["requireWorkerApproval"], true);
    assert_eq!(patched.1["requireConfigurationApply"], false);
    let read = hub.get_admin("/v1/settings/security-trust").await;
    assert_eq!(read.1, patched.1);
}

#[tokio::test]
async fn a_pending_worker_cannot_claim_and_approval_is_idempotent() {
    let hub = start(10_000).await;
    let (worker_id, credential, _socket, epoch) = pending_worker(&hub).await;
    let listed = hub.get_admin("/v1/workers").await;
    assert_eq!(listed.1["workers"][0]["approval"], "pending");
    // The Snapshot carries no configuration revisions, only whether one is pending.
    assert_eq!(listed.1["workers"][0]["configurationPending"], true);
    assert!(listed.1["workers"][0].get("reportedRevision").is_none());
    assert!(listed.1["workers"][0].get("appliedRevision").is_none());

    // A pending Worker is never woken; the hint gate is the Hub's, not a local check.
    let task_id = queued_task(&hub, "pending").await;
    let refused = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&task_id, epoch),
        )
        .await;
    assert_eq!(refused.0, 409, "{}", refused.1);
    assert_eq!(refused.1["error"], "InvalidTransition");
    assert_eq!(
        hub.get_admin(&format!("/v1/tasks/{task_id}")).await.1["task"]["status"],
        "queued"
    );

    let approve = format!("/v1/workers/{worker_id}/approve");
    assert_eq!(hub.post_admin(&approve, None, None).await.0, 204);
    // Repeating approval for an approved Worker succeeds.
    assert_eq!(hub.post_admin(&approve, None, None).await.0, 204);
    let detail = hub.get_admin(&format!("/v1/workers/{worker_id}")).await;
    assert_eq!(detail.1["worker"]["approval"], "approved");
    assert_eq!(detail.1["reportedRevision"], 1);
    assert!(
        detail.1["appliedRevision"].is_null(),
        "approval alone never applies a report"
    );
    assert_eq!(detail.1["reportedConfig"]["slots"][0]["slotId"], "slot-a");
    assert!(
        detail.1["claimableTaskIds"].as_array().unwrap().is_empty(),
        "Apply is still pending, so nothing is claimable"
    );

    let applied = hub
        .post_admin(
            &format!("/v1/workers/{worker_id}/config/apply"),
            None,
            Some(r#"{"revision":1}"#),
        )
        .await;
    assert_eq!(applied.0, 200, "{}", applied.1);
    let detail = hub.get_admin(&format!("/v1/workers/{worker_id}")).await;
    assert_eq!(detail.1["appliedRevision"], 1);
    assert!(
        detail.1["claimableTaskIds"]
            .as_array()
            .unwrap()
            .contains(&json!(task_id)),
        "an approved, applied Worker can see what it could claim: {}",
        detail.1
    );

    let claimed = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&task_id, epoch),
        )
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);
}

#[tokio::test]
async fn apply_requires_the_reviewed_revision_and_leaves_applied_unchanged_on_error() {
    let hub = start(10_000).await;
    let (worker_id, credential, mut socket, epoch) = pending_worker(&hub).await;
    let revoke = format!("/v1/workers/{worker_id}/revoke");
    let apply = format!("/v1/workers/{worker_id}/config/apply");

    // With no report yet, Apply refuses with a specific code.
    let fresh_hub = start(10_000).await;
    let (fresh_id, _fresh_credential) = fresh_hub.pair().await;
    let no_report = fresh_hub
        .post_admin(
            &format!("/v1/workers/{fresh_id}/config/apply"),
            None,
            Some(r#"{"revision":1}"#),
        )
        .await;
    assert_eq!(no_report.0, 409, "{}", no_report.1);
    assert_eq!(no_report.1["error"], "NoPendingConfig");

    // A pending report with a different revision asks the operator to re-read it.
    let mismatch = hub
        .post_admin(&apply, None, Some(r#"{"revision":9}"#))
        .await;
    assert_eq!(mismatch.0, 409, "{}", mismatch.1);
    assert_eq!(mismatch.1["error"], "ConfigRevisionMismatch");

    // Approval and Apply may occur in either order: apply first, then approve.
    let applied = hub
        .post_admin(&apply, None, Some(r#"{"revision":1}"#))
        .await;
    assert_eq!(applied.0, 200, "{}", applied.1);
    assert_eq!(applied.1["appliedRevision"], 1);
    let detail = hub.get_admin(&format!("/v1/workers/{worker_id}")).await;
    assert_eq!(detail.1["worker"]["approval"], "pending");
    assert_eq!(detail.1["appliedRevision"], 1);
    assert!(
        detail.1["claimableTaskIds"].as_array().unwrap().is_empty(),
        "approval is still pending"
    );
    assert_eq!(
        hub.post_admin(&format!("/v1/workers/{worker_id}/approve"), None, None)
            .await
            .0,
        204
    );
    let task_id = queued_task(&hub, "apply-first").await;
    // Clearing the last gate triggers a wakeup pass; the hint creates nothing by itself.
    let wake = read_frame(&mut socket).await;
    assert_eq!(wake["kind"], "hub.wakeup");
    assert_eq!(wake["payload"]["taskId"], task_id);
    let claimed = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&task_id, epoch),
        )
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);

    // A revoked Worker cannot be approved or applied; both report InvalidTransition.
    assert_eq!(hub.post_admin(&revoke, None, None).await.0, 204);
    let reapprove = hub
        .post_admin(&format!("/v1/workers/{worker_id}/approve"), None, None)
        .await;
    assert_eq!(reapprove.0, 409, "{}", reapprove.1);
    assert_eq!(reapprove.1["error"], "InvalidTransition");
    let revoked_apply = hub
        .post_admin(&apply, None, Some(r#"{"revision":1}"#))
        .await;
    assert_eq!(revoked_apply.0, 409, "{}", revoked_apply.1);
    assert_eq!(revoked_apply.1["error"], "InvalidTransition");
}

#[tokio::test]
async fn a_new_report_waits_for_review_and_then_clears_the_claim_gate() {
    let hub = start(10_000).await;
    let (worker_id, credential, mut socket, _epoch) = pending_worker(&hub).await;
    hub.clear_trust_gates(&worker_id).await;

    // Report a changed inventory over the live socket; the Hub keeps it pending.
    let inventory = json!({
        "revision": 2,
        "observedAtMs": 5,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{
            "slotId": "slot-a",
            "enabled": true,
            "advisoryCapacity": 2,
            "roles": ["general"],
            "capabilities": [{ "name": "gpu", "state": "available" }],
            "executors": [{ "id": "exec-claude", "kind": "claude" }],
        }],
    });
    let frame = json!({
        "v": myrlet_protocol::WORKER_PROTOCOL_VERSION,
        "kind": "worker.inventory",
        "messageId": "msg_inventory",
        "workerId": worker_id,
        "connectionEpoch": hub_connection_epoch(&hub, &worker_id).await,
        "sentAtMs": 2,
        "payload": inventory.clone(),
    });
    socket_send(&mut socket, &frame).await;
    let ack = read_frame(&mut socket).await;
    assert_eq!(ack["kind"], "worker.inventory_ack");
    assert_eq!(ack["payload"]["accepted"], true);

    let detail = hub.get_admin(&format!("/v1/workers/{worker_id}")).await;
    assert_eq!(detail.1["reportedRevision"], 2);
    assert_eq!(detail.1["appliedRevision"], 1);
    assert_eq!(
        detail.1["reportedConfig"]["slots"][0]["capabilities"][0]["name"],
        "gpu"
    );

    let gpu = hub
        .post_admin(
            "/v1/tasks",
            Some("gpu"),
            Some(r#"{"title":"GPU","prompt":"p","routing":{"requiredCapabilities":["gpu"]}}"#),
        )
        .await;
    assert_eq!(gpu.0, 201, "{}", gpu.1);
    let gpu_id = gpu.1["task"]["id"].as_str().unwrap().to_owned();
    let epoch = hub_connection_epoch(&hub, &worker_id).await;
    let gated = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&gpu_id, epoch),
        )
        .await;
    assert_eq!(gated.0, 409, "{}", gated.1);

    let applied = hub
        .post_admin(
            &format!("/v1/workers/{worker_id}/config/apply"),
            None,
            Some(r#"{"revision":2}"#),
        )
        .await;
    assert_eq!(applied.0, 200, "{}", applied.1);
    let claimed = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&gpu_id, epoch),
        )
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);
}

async fn hub_connection_epoch(hub: &common::Hub, worker_id: &str) -> u64 {
    hub.get_admin(&format!("/v1/workers/{worker_id}")).await.1["worker"]["connectionEpoch"]
        .as_u64()
        .expect("connection epoch")
}

async fn socket_send(socket: &mut common::Socket, frame: &serde_json::Value) {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    socket
        .send(Message::Text(frame.to_string()))
        .await
        .expect("send frame");
}

/// Applying a reviewed revision is visible to a second connected client through the
/// durable event stream, not only by polling the Snapshot again.
#[tokio::test]
async fn an_apply_is_observable_by_another_connected_client() {
    let hub = start(10_000).await;
    let (worker_id, _credential, mut socket, _epoch) = pending_worker(&hub).await;

    // Subscribe before the Apply so no committed event can fall between the two.
    let mut stream = reqwest::Client::new()
        .get(format!("{}/v1/events", hub.base))
        .header("Authorization", format!("Bearer {ADMIN}"))
        .send()
        .await
        .expect("open the event stream");
    assert_eq!(stream.status(), 200);

    let applied = hub
        .post_admin(
            &format!("/v1/workers/{worker_id}/config/apply"),
            None,
            Some(r#"{"revision":1}"#),
        )
        .await;
    assert_eq!(applied.0, 200, "{}", applied.1);

    // The stream replays from the start; the committed Apply is the newest event.
    let event = loop {
        let event = common::next_sse_event(&mut stream).await;
        if event["kind"] == "worker.config_applied" {
            break event;
        }
    };
    assert_eq!(event["payload"]["workerId"], worker_id);
    assert_eq!(event["payload"]["workerId"], worker_id);
    assert_eq!(event["payload"]["revision"], 1);

    // The Worker's own `worker.inventory_ack` is unaffected by the extra subscriber.
    socket_send(
        &mut socket,
        &json!({
            "v": myrlet_protocol::WORKER_PROTOCOL_VERSION,
            "kind": "worker.inventory",
            "messageId": "msg_repeat",
            "workerId": worker_id,
            "sentAtMs": 3,
            "payload": {
                "revision": 1,
                "observedAtMs": 2,
                "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
                "slots": [{
                    "slotId": "slot-a",
                    "enabled": true,
                    "advisoryCapacity": 2,
                    "roles": ["general"],
                    "capabilities": [{ "name": "shell", "state": "available" }],
                    "executors": [{ "id": "exec-claude", "kind": "claude" }],
                }],
            },
        }),
    )
    .await;
    let ack = read_frame(&mut socket).await;
    assert_eq!(ack["kind"], "worker.inventory_ack");
    assert_eq!(ack["payload"]["accepted"], true);
}
