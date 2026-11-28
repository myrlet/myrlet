//! Shared pairing, policy, and connection fixtures for the Worker trust suites.
//!
//! Each test binary compiles this module separately, so helpers one suite does not use
//! are expected rather than dead.

#![allow(dead_code)]

use std::collections::HashMap;

use myrlet_protocol::{
    ClaimTaskRequest, PairWorkerRequest, TaskDefinition, UpdateSecurityTrustRequest, WorkerId,
};
use myrlet_store::HubStore;

pub(super) const T0: i64 = 1_764_070_000_000;
pub(super) const WORKER: &str = "wkr_000000000000";
pub(super) const SLOT: &str = "slot-a";

pub(super) fn worker() -> WorkerId {
    WorkerId::from(WORKER)
}

pub(super) fn inventory(
    revision: u64,
    capability: &str,
    executor: &str,
) -> myrlet_protocol::WorkerInventory {
    serde_json::from_value(serde_json::json!({
        "revision": revision,
        "observedAtMs": T0,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{
            "slotId": SLOT,
            "enabled": true,
            "advisoryCapacity": 2,
            "capabilities": [{ "name": capability, "state": "available" }],
            "executors": [{ "id": executor, "kind": "claude" }],
        }]
    }))
    .unwrap()
}

pub(super) fn definition(capability: &str) -> TaskDefinition {
    serde_json::from_value(serde_json::json!({
        "title": "trust",
        "prompt": "p",
        "routing": { "requiredCapabilities": [capability] },
    }))
    .unwrap()
}

pub(super) fn claim(task_id: &str, epoch: u64) -> ClaimTaskRequest {
    ClaimTaskRequest {
        task_id: task_id.into(),
        slot_id: SLOT.into(),
        connection_epoch: epoch,
        executor_id: None,
    }
}

pub(super) async fn pair(store: &HubStore) -> (WorkerId, String) {
    let issued = store
        .create_enrollment_code(Some(600), None, T0)
        .await
        .unwrap();
    let request: PairWorkerRequest = serde_json::from_value(serde_json::json!({
        "enrollmentCode": issued.enrollment_code,
        "workerId": WORKER,
    }))
    .unwrap();
    let (paired, _) = store
        .redeem_pairing_code("hub_1".into(), request, T0)
        .await
        .unwrap();
    (paired.worker_id, paired.credential)
}

pub(super) async fn set_policies(store: &HubStore, approval: bool, apply: bool) {
    store
        .update_settings(
            UpdateSecurityTrustRequest {
                require_worker_approval: Some(approval),
                require_configuration_apply: Some(apply),
            },
            T0,
        )
        .await
        .unwrap();
}

pub(super) async fn connect(store: &HubStore, inventory: myrlet_protocol::WorkerInventory) -> u64 {
    store
        .open_worker_connection(worker(), inventory, T0)
        .await
        .unwrap()
        .0
}

pub(super) async fn approve_and_apply(store: &HubStore) {
    store.approve_worker(worker(), T0).await.unwrap();
    let reported = store
        .get_worker_snapshot(worker(), HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .expect("a report exists");
    store
        .apply_worker_config(worker(), reported, T0)
        .await
        .unwrap();
}
