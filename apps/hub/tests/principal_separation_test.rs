//! The Admin principal and a Worker principal stay distinct even when their raw bearer
//! values are identical, so Worker-originated execution paths never accept Admin.

mod common;

use common::{read_frame, start_with_config};
use myrlet_protocol::WorkerId;
use serde_json::json;

const COLLIDING_TOKEN: &str = "one-shared-bearer-value";

fn claim_body(task_id: &str, epoch: u64) -> String {
    json!({ "taskId": task_id, "slotId": "slot-a", "connectionEpoch": epoch }).to_string()
}

#[tokio::test]
async fn a_colliding_admin_token_never_authenticates_as_a_worker() {
    // Configure the Hub Admin token and a stored Worker credential to the same value.
    let hub = start_with_config(10_000, |config| {
        config.admin_token = COLLIDING_TOKEN.to_string();
    })
    .await;
    let state = hub.state();
    state
        .store
        .pair_with_credential(
            WorkerId::new("wkr_000000000000"),
            COLLIDING_TOKEN.to_string(),
            myrlet_hub::state::now_ms(),
        )
        .await
        .unwrap();

    // Admin operations still work with that value.
    let snapshot = hub
        .request("GET", "/v1/snapshot", Some(COLLIDING_TOKEN), None, None)
        .await;
    assert_eq!(snapshot.0, 200, "{}", snapshot.1);
    let task = hub
        .post_admin_with(
            COLLIDING_TOKEN,
            "/v1/tasks",
            Some("collision"),
            Some(r#"{"title":"T","prompt":"p"}"#),
        )
        .await;
    assert_eq!(task.0, 201, "{}", task.1);
    let task_id = task.1["task"]["id"].as_str().unwrap().to_owned();

    // Worker-only HTTP endpoints reject the Admin principal.
    assert_eq!(
        hub.request(
            "POST",
            "/v1/workers/claim",
            Some(COLLIDING_TOKEN),
            None,
            Some(&claim_body(&task_id, 1)),
        )
        .await
        .0,
        401
    );
    for route in ["progress", "result", "finish"] {
        let path = format!("/v1/worker/deliveries/delivery_x/{route}");
        assert_eq!(
            hub.post_worker(&path, COLLIDING_TOKEN, "{}").await.0,
            401,
            "{route} must reject the Admin principal"
        );
    }

    // WSS rejects the Admin principal before the handshake completes.
    assert!(hub.connect_with(COLLIDING_TOKEN).await.is_err());

    // A genuine Worker credential still authenticates as a Worker.
    let (worker_id, credential) = hub.pair_with_admin(COLLIDING_TOKEN).await;
    let (mut socket, epoch) = hub.connect(&worker_id, &credential).await;
    hub.clear_trust_gates_with(COLLIDING_TOKEN, &worker_id)
        .await;
    let _ = read_frame(&mut socket).await;
    let claimed = hub
        .post_worker(
            "/v1/workers/claim",
            &credential,
            &claim_body(&task_id, epoch),
        )
        .await;
    assert_eq!(claimed.0, 200, "{}", claimed.1);
}
