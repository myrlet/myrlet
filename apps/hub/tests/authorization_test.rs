//! Three-tier authorization boundaries: worker assignment visibility, owner coordination,
//! admin administration, and Worker-only execution channels.

mod common;

use common::{read_frame, start};
use serde_json::json;

fn claim_body(task_id: &str, epoch: u64) -> String {
    json!({ "taskId": task_id, "slotId": "slot-a", "connectionEpoch": epoch }).to_string()
}

async fn queued_task(hub: &common::Hub, key: &str, title: &str) -> String {
    let definition = json!({ "title": title, "prompt": "p" }).to_string();
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
async fn a_new_pairing_defaults_to_the_worker_tier() {
    let hub = start(10_000).await;
    let (worker_id, _credential) = hub.pair().await;
    let detail = hub.get_admin(&format!("/v1/workers/{worker_id}")).await;
    assert_eq!(detail.0, 200, "{}", detail.1);
    assert_eq!(detail.1["worker"]["permissionTier"], "worker");
}

#[tokio::test]
async fn only_admin_changes_a_permission_tier() {
    let hub = start(10_000).await;
    let (worker_id, worker_credential) = hub.pair().await;
    let path = format!("/v1/workers/{worker_id}/permission-tier");

    // Missing credentials are 401; a Worker credential is 403 with the tier it needs.
    assert_eq!(
        hub.request(
            "PATCH",
            &path,
            None,
            None,
            Some(r#"{"permissionTier":"owner"}"#)
        )
        .await
        .0,
        401
    );
    let denied = hub
        .request(
            "PATCH",
            &path,
            Some(&worker_credential),
            None,
            Some(r#"{"permissionTier":"owner"}"#),
        )
        .await;
    assert_eq!(denied.0, 403, "{}", denied.1);
    assert_eq!(denied.1["error"], "Forbidden");
    assert_eq!(denied.1["requiredTier"], "admin");
    assert_eq!(denied.1["currentTier"], "worker");
    assert_eq!(
        hub.get_admin(&format!("/v1/workers/{worker_id}")).await.1["worker"]["permissionTier"],
        "worker"
    );

    // Admin may assign `worker` or `owner`, but never the `admin` tier itself.
    hub.set_tier(&worker_id, "owner").await;
    assert_eq!(
        hub.get_admin(&format!("/v1/workers/{worker_id}")).await.1["worker"]["permissionTier"],
        "owner"
    );
    let rejected = hub
        .patch_admin(&path, Some(r#"{"permissionTier":"admin"}"#))
        .await;
    assert_eq!(rejected.0, 400, "{}", rejected.1);
    assert_eq!(rejected.1["error"], "BadRequest");
}

#[tokio::test]
async fn credential_rotation_preserves_the_tier_and_repairing_resets_it() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    hub.set_tier(&worker_id, "owner").await;

    let rotated = hub
        .post_admin(&format!("/v1/workers/{worker_id}/rotate"), None, None)
        .await;
    assert_eq!(rotated.0, 200, "rotation failed: {rotated:?}");
    assert_eq!(
        hub.get_admin(&format!("/v1/workers/{worker_id}")).await.1["worker"]["permissionTier"],
        "owner"
    );

    // Revocation then re-pairing starts a fresh trust decision at the `worker` tier.
    assert_eq!(
        hub.post_admin(&format!("/v1/workers/{worker_id}/revoke"), None, None)
            .await
            .0,
        204
    );
    assert_eq!(
        hub.request("GET", "/v1/workers", Some(&credential), None, None)
            .await
            .0,
        401,
        "a revoked credential is no longer valid"
    );
    let (repaired, _credential) = hub.pair_as(Some(&worker_id)).await;
    assert_eq!(repaired, worker_id);
    assert_eq!(
        hub.get_admin(&format!("/v1/workers/{worker_id}")).await.1["worker"]["permissionTier"],
        "worker"
    );
}

#[tokio::test]
async fn a_worker_reads_only_its_assigned_task_and_one_hop_relatives() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;

    // One root groups three siblings: the assigned Task, its dependency, and a stranger.
    // An unrelated root stays completely outside the tree.
    let root = queued_task(&hub, "root", "Root").await;
    let sibling = |title: &str, dependencies: &[&str]| {
        json!({
            "title": title,
            "prompt": "p",
            "parentTaskId": root,
            "dependencies": dependencies,
        })
        .to_string()
    };
    let dependency = {
        let created = hub
            .post_admin(
                "/v1/tasks",
                Some("dependency"),
                Some(&sibling("Dependency", &[])),
            )
            .await;
        created.1["task"]["id"].as_str().unwrap().to_owned()
    };
    let assigned = {
        let body = sibling("Assigned", &[&dependency]);
        let created = hub
            .post_admin("/v1/tasks", Some("assigned"), Some(&body))
            .await;
        created.1["task"]["id"].as_str().unwrap().to_owned()
    };
    let stranger = {
        let created = hub
            .post_admin(
                "/v1/tasks",
                Some("stranger"),
                Some(&sibling("Stranger", &[])),
            )
            .await;
        created.1["task"]["id"].as_str().unwrap().to_owned()
    };
    let unrelated = queued_task(&hub, "unrelated", "Unrelated").await;

    // Before any Delivery the Worker holds nothing, so every Task is invisible.
    for candidate in [&assigned, &dependency, &stranger, &root, &unrelated] {
        let hidden = hub
            .request(
                "GET",
                &format!("/v1/tasks/{candidate}"),
                Some(&credential),
                None,
                None,
            )
            .await;
        assert_eq!(hidden.0, 404, "{candidate}: {}", hidden.1);
        assert_eq!(hidden.1["error"], "NotFound");
    }

    // Committing a Delivery to this Worker establishes assignment of exactly one Task.
    let (mut socket, epoch) = hub.connect(&worker_id, &credential).await;
    hub.clear_trust_gates(&worker_id).await;
    let _ = read_frame(&mut socket).await; // the wakeup hint for a queued Task
    let claim = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&dependency, epoch),
        )
        .await;
    assert_eq!(claim.0, 200, "{}", claim.1);

    // The assigned Task reads in full and lists its one-hop relatives.
    let owned = hub
        .request(
            "GET",
            &format!("/v1/tasks/{dependency}"),
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(owned.0, 200, "{}", owned.1);
    assert_eq!(owned.1["task"]["id"], dependency);
    let related_ids: Vec<&str> = owned.1["related"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["id"].as_str().unwrap())
        .collect();
    // The claimed Task's direct parent and every Task that declares it as a dependency
    // are visible; siblings of the dependents, other subtree members, and unrelated
    // Tasks are not.
    assert_eq!(related_ids.len(), 2, "{related_ids:?}");
    assert!(related_ids.iter().any(|id| id == &root));
    assert!(related_ids.iter().any(|id| id == &assigned));
    assert!(!related_ids.iter().any(|id| id == &stranger));
    assert!(!related_ids.iter().any(|id| id == &unrelated));

    // A related summary never carries prompt, Delivery, Progress, or Result.
    let related_task = owned.1["related"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == assigned.as_str())
        .expect("dependent summary");
    assert!(related_task["task"].is_null());
    assert!(related_task["currentDelivery"].is_null());
    assert!(related_task["resultMarkdown"].is_null());
    assert!(related_task["prompt"].is_null());

    // A directly related Task is a limited summary, never a full record.
    let related = hub
        .request(
            "GET",
            &format!("/v1/tasks/{assigned}"),
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(related.0, 200, "{}", related.1);
    assert!(related.1["task"].is_null());
    assert_eq!(related.1["summary"]["id"], assigned);
    assert!(related.1["summary"]["title"].is_string());

    // Unrelated Tasks and the global listing stay invisible. A sibling that is not a
    // dependency or dependent is outside the one-hop boundary even though it shares a
    // parent with the assigned Task.
    for path in [
        format!("/v1/tasks/{unrelated}"),
        format!("/v1/tasks/{stranger}"),
        "/v1/tasks".to_string(),
        "/v1/workers".to_string(),
        "/v1/snapshot".to_string(),
    ] {
        let (status, body) = hub
            .request("GET", &path, Some(&credential), None, None)
            .await;
        assert!(status == 403 || status == 404, "{path}: {body}");
    }
    // The direct parent is a one-hop relative, but only as a summary.
    let parent = hub
        .request(
            "GET",
            &format!("/v1/tasks/{root}"),
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(parent.0, 200, "{}", parent.1);
    assert!(parent.1["task"].is_null());
    assert_eq!(parent.1["summary"]["id"], root);

    // The assigned Task's tree anchors to one hop instead of expanding the subtree.
    let tree = hub
        .request(
            "GET",
            &format!("/v1/tasks/{dependency}/tree"),
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(tree.0, 200, "{}", tree.1);
    assert_eq!(tree.1["tasks"][0]["id"], dependency);
    for related in tree.1["related"].as_array().unwrap() {
        assert!(related["prompt"].is_null());
        assert!(related["currentDelivery"].is_null());
    }
    // An anchor outside the Worker's assignment is concealed.
    let foreign_tree = hub
        .request(
            "GET",
            &format!("/v1/tasks/{unrelated}/tree"),
            Some(&credential),
            None,
            None,
        )
        .await;
    assert_eq!(foreign_tree.0, 404, "{}", foreign_tree.1);
}

#[tokio::test]
async fn worker_tier_cannot_coordinate_tasks_or_read_worker_state() {
    let hub = start(10_000).await;
    let (worker_id, credential) = hub.pair().await;
    let task_id = queued_task(&hub, "coordinate", "Coordinate").await;
    let version = hub.get_admin(&format!("/v1/tasks/{task_id}")).await.1["task"]["version"]
        .as_u64()
        .unwrap();
    let cancel = json!({ "expectedTaskVersion": version }).to_string();
    let denied = hub
        .request(
            "POST",
            &format!("/v1/tasks/{task_id}/cancel"),
            Some(&credential),
            Some("worker-cancel"),
            Some(&cancel),
        )
        .await;
    assert_eq!(denied.0, 403, "{}", denied.1);
    assert_eq!(denied.1["requiredTier"], "owner");
    assert_eq!(
        hub.get_admin(&format!("/v1/tasks/{task_id}")).await.1["task"]["status"],
        "queued"
    );
    for method in ["rotate", "revoke", "approve"] {
        let denied = hub
            .request(
                "POST",
                &format!("/v1/workers/{worker_id}/{method}"),
                Some(&credential),
                None,
                None,
            )
            .await;
        assert_eq!(denied.0, 403, "{method}: {}", denied.1);
        assert_eq!(denied.1["requiredTier"], "admin");
    }
}

#[tokio::test]
async fn admin_cannot_impersonate_worker_execution_operations() {
    let hub = start(10_000).await;
    let (worker_id, credential, mut socket, epoch) = hub.enrolled().await;
    let definition = json!({ "title": "Claim", "prompt": "p" }).to_string();
    let created = hub
        .post_admin("/v1/tasks", Some("claim"), Some(&definition))
        .await;
    let task_id = created.1["task"]["id"].as_str().unwrap().to_owned();
    let _ = read_frame(&mut socket).await;

    // Claim, WSS connect, and every report route reject the Admin principal.
    assert_eq!(
        hub.post_admin(
            "/v1/workers/claim",
            None,
            Some(&claim_body(&task_id, epoch))
        )
        .await
        .0,
        401
    );
    assert_eq!(
        hub.get_admin(&format!("/v1/workers/connect?workerId={worker_id}"))
            .await
            .0,
        401
    );
    for route in ["progress", "result", "finish"] {
        let path = format!("/v1/worker/deliveries/delivery_x/{route}");
        assert_eq!(
            hub.post_admin(&path, None, Some("{}")).await.0,
            401,
            "{route}"
        );
    }
    // The Worker itself still owns these operations.
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
async fn insufficient_credentials_are_distinguished_from_invalid_ones() {
    let hub = start(10_000).await;
    // Missing and unknown credentials are 401 on both a global read and a command.
    assert_eq!(
        hub.request("GET", "/v1/snapshot", None, None, None).await.0,
        401
    );
    assert_eq!(
        hub.request("GET", "/v1/snapshot", Some("not-a-token"), None, None)
            .await
            .0,
        401
    );
    let (_, credential) = hub.pair().await;
    // A valid credential with the wrong tier is 403, not 401.
    let forbidden = hub
        .request("GET", "/v1/snapshot", Some(&credential), None, None)
        .await;
    assert_eq!(forbidden.0, 403, "{}", forbidden.1);
    assert_eq!(forbidden.1["requiredTier"], "owner");
    assert_eq!(forbidden.1["currentTier"], "worker");
}
