//! Worker approval, applied scheduling inventory, and the Security & Trust policies.

use std::collections::HashMap;

use myrlet_protocol::{
    ClaimTaskRequest, PairWorkerRequest, ProgressReportRequest, ResultReportRequest,
    SecurityTrustSettings, TaskDefinition, TaskStatus, UpdateSecurityTrustRequest,
    WorkerApprovalStatus, WorkerId,
};
use myrlet_store::{HubStore, StoreError};

const T0: i64 = 1_764_070_000_000;
const WORKER: &str = "wkr_000000000000";
const SLOT: &str = "slot-a";

fn worker() -> WorkerId {
    WorkerId::from(WORKER)
}

fn inventory(revision: u64, capability: &str, executor: &str) -> myrlet_protocol::WorkerInventory {
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

fn definition(capability: &str) -> TaskDefinition {
    serde_json::from_value(serde_json::json!({
        "title": "trust",
        "prompt": "p",
        "routing": { "requiredCapabilities": [capability] },
    }))
    .unwrap()
}

fn claim(task_id: &str, epoch: u64) -> ClaimTaskRequest {
    ClaimTaskRequest {
        task_id: task_id.into(),
        slot_id: SLOT.into(),
        connection_epoch: epoch,
        executor_id: None,
    }
}

async fn pair(store: &HubStore) -> (WorkerId, String) {
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

async fn set_policies(store: &HubStore, approval: bool, apply: bool) {
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

async fn connect(store: &HubStore, inventory: myrlet_protocol::WorkerInventory) -> u64 {
    store
        .open_worker_connection(worker(), inventory, T0)
        .await
        .unwrap()
}

async fn approve_and_apply(store: &HubStore) {
    store.approve_worker(worker(), T0).await.unwrap();
    let reported = store
        .get_worker(worker(), HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .expect("a report exists");
    store
        .apply_worker_config(worker(), reported, T0)
        .await
        .unwrap();
}

#[tokio::test]
async fn policies_default_on_and_a_pending_worker_cannot_claim() {
    let store = HubStore::open_in_memory().unwrap();
    assert_eq!(
        store.load_settings().await.unwrap(),
        SecurityTrustSettings {
            require_worker_approval: true,
            require_configuration_apply: true,
        }
    );
    let (worker_id, credential) = pair(&store).await;
    let snapshot = store.list_workers(HashMap::new()).await.unwrap();
    assert_eq!(snapshot[0].approval, WorkerApprovalStatus::Pending);
    assert!(snapshot[0].reported_revision.is_none());
    assert!(snapshot[0].applied_revision.is_none());

    // A pending Worker may still connect, heartbeat, and read; it just cannot claim.
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    assert_eq!(
        store.authenticate_worker(credential).await.unwrap(),
        worker_id
    );
    store.touch_worker_seen(worker(), epoch, T0).await.unwrap();
    let (record, _, _) = store
        .create_task("pending".into(), definition("shell"), T0)
        .await
        .unwrap();
    let refused = store
        .claim_task(worker(), claim(record.id.as_str(), epoch), T0)
        .await;
    assert!(matches!(refused, Err(StoreError::InvalidTransition)));
    assert_eq!(
        store.get_task(record.id).await.unwrap().status,
        TaskStatus::Queued
    );
}

#[tokio::test]
async fn approval_alone_still_waits_for_the_applied_revision() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    let (record, _, _) = store
        .create_task("waiting".into(), definition("shell"), T0)
        .await
        .unwrap();
    store.approve_worker(worker(), T0).await.unwrap();
    let still_gated = store
        .claim_task(worker(), claim(record.id.as_str(), epoch), T0)
        .await;
    assert!(matches!(still_gated, Err(StoreError::InvalidTransition)));

    approve_and_apply(&store).await;
    let (claimed, _) = store
        .claim_task(worker(), claim(record.id.as_str(), epoch), T0)
        .await
        .unwrap();
    assert_eq!(claimed.task.status, TaskStatus::Dispatched);
}

#[tokio::test]
async fn apply_teaches_the_hub_the_new_inventory_only_after_review() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    approve_and_apply(&store).await;
    let gpu = store
        .create_task("gpu".into(), definition("gpu"), T0)
        .await
        .unwrap()
        .0;

    // A changed report gets a new Hub revision and does not take effect by itself.
    store
        .update_worker_inventory(worker(), epoch, inventory(2, "gpu", "exec-b"), T0)
        .await
        .unwrap();
    let detail = store
        .worker_detail(worker(), HashMap::new(), 50)
        .await
        .unwrap();
    assert_eq!(detail.worker.reported_revision, Some(2));
    assert_eq!(detail.worker.applied_revision, Some(1));
    assert_eq!(
        detail.reported_config.as_ref().unwrap().slots[0].executors[0]
            .id
            .as_str(),
        "exec-b"
    );
    assert_eq!(
        detail.worker.applied_config.as_ref().unwrap().slots[0].executors[0]
            .id
            .as_str(),
        "exec-a"
    );
    assert!(detail.claimable_task_ids.is_empty());
    // Approval holds, but the newer report is still pending, so the Worker's applied
    // revision no longer matches and no new Delivery may be created.
    let refused = store
        .claim_task(worker(), claim(gpu.id.as_str(), epoch), T0)
        .await;
    assert!(matches!(refused, Err(StoreError::InvalidTransition)));

    // Applying the reviewed revision makes the Slot claimable.
    assert_eq!(store.apply_worker_config(worker(), 2, T0).await.unwrap(), 2);
    let (claimed, _) = store
        .claim_task(worker(), claim(gpu.id.as_str(), epoch), T0)
        .await
        .unwrap();
    assert_eq!(claimed.delivery.executor_id.unwrap().as_str(), "exec-b");
}

#[tokio::test]
async fn apply_errors_leave_the_applied_snapshot_untouched() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    approve_and_apply(&store).await;
    store
        .update_worker_inventory(worker(), epoch, inventory(2, "gpu", "exec-b"), T0)
        .await
        .unwrap();

    let mismatch = store.apply_worker_config(worker(), 1, T0).await;
    assert!(matches!(mismatch, Err(StoreError::ConfigRevisionMismatch)));
    let applied = store
        .get_worker(worker(), HashMap::new())
        .await
        .unwrap()
        .applied_revision;
    assert_eq!(applied, Some(1));

    store.revoke_worker(worker(), T0).await.unwrap();
    let revoked = store.apply_worker_config(worker(), 2, T0).await;
    assert!(matches!(revoked, Err(StoreError::InvalidTransition)));
}

#[tokio::test]
async fn a_repeated_report_keeps_the_revision_and_any_pending_decision() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    store
        .update_worker_inventory(worker(), epoch, inventory(2, "shell", "exec-a"), T0)
        .await
        .unwrap();
    let first = store
        .get_worker(worker(), HashMap::new())
        .await
        .unwrap()
        .reported_revision;
    // A reconnect with identical content (different Worker-local revision) preserves it.
    let epoch2 = connect(&store, inventory(9, "shell", "exec-a")).await;
    assert!(epoch2 > epoch);
    let second = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(second.reported_revision, first);
    assert_eq!(second.applied_revision, None);

    let stale = store
        .update_worker_inventory(worker(), epoch, inventory(3, "gpu", "exec-z"), T0)
        .await
        .unwrap();
    assert!(!stale, "a stale epoch cannot change either snapshot");
    assert_eq!(
        store
            .get_worker(worker(), HashMap::new())
            .await
            .unwrap()
            .reported_revision,
        first
    );
}

#[tokio::test]
async fn disabling_apply_admits_a_report_in_the_same_transaction() {
    let store = HubStore::open_in_memory().unwrap();
    set_policies(&store, false, false).await;
    pair(&store).await;
    let snapshot = store.list_workers(HashMap::new()).await.unwrap();
    assert_eq!(snapshot[0].approval, WorkerApprovalStatus::Approved);
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    let snapshot = store.list_workers(HashMap::new()).await.unwrap();
    assert_eq!(snapshot[0].reported_revision, Some(1));
    assert_eq!(snapshot[0].applied_revision, Some(1));
    let (record, _, _) = store
        .create_task("open".into(), definition("shell"), T0)
        .await
        .unwrap();
    assert_eq!(
        store
            .claim_task(worker(), claim(record.id.as_str(), epoch), T0)
            .await
            .map(|(response, _)| response.task.status)
            .unwrap(),
        TaskStatus::Dispatched
    );
}

#[tokio::test]
async fn toggles_never_act_retroactively() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    connect(&store, inventory(1, "shell", "exec-a")).await;
    approve_and_apply(&store).await;
    // Turning both policies off changes no existing decision or snapshot.
    set_policies(&store, false, false).await;
    let row = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(row.approval, WorkerApprovalStatus::Approved);
    assert_eq!(row.applied_revision, Some(1));
    // Turning approval back on does not retroactively un-approve it either.
    set_policies(&store, true, true).await;
    let row = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(row.approval, WorkerApprovalStatus::Approved);
}

#[tokio::test]
async fn a_pending_worker_can_be_revoked_and_a_repaired_id_starts_fresh() {
    let store = HubStore::open_in_memory().unwrap();
    let (_id, credential) = pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    store.revoke_worker(worker(), T0).await.unwrap();
    assert_eq!(
        store
            .get_worker(worker(), HashMap::new())
            .await
            .unwrap()
            .approval,
        WorkerApprovalStatus::Revoked
    );
    let denied = store.authenticate_worker(credential).await;
    assert!(matches!(denied, Err(StoreError::Unauthorized)));
    let approve_revoked = store.approve_worker(worker(), T0).await;
    assert!(matches!(
        approve_revoked,
        Err(StoreError::InvalidTransition)
    ));

    // Re-pairing the revoked ID starts a fresh decision and clears both snapshots.
    let (repaired, credential) = pair(&store).await;
    assert_eq!(repaired, worker());
    let worker_row = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(worker_row.approval, WorkerApprovalStatus::Pending);
    assert!(worker_row.applied_revision.is_none());
    assert!(worker_row.reported_revision.is_none());
    assert_eq!(
        store.authenticate_worker(credential).await.unwrap(),
        worker()
    );

    // The retained high-water mark means a pre-repair revision cannot be applied.
    let next_epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    assert!(next_epoch > epoch);
    let repaired_revision = store
        .get_worker(worker(), HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .unwrap();
    assert!(
        repaired_revision > 1,
        "revisions keep increasing across re-pair"
    );
    let stale_apply = store.apply_worker_config(worker(), 1, T0).await;
    assert!(matches!(
        stale_apply,
        Err(StoreError::ConfigRevisionMismatch)
    ));
}

#[tokio::test]
async fn rotation_preserves_approval_and_both_snapshots() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    connect(&store, inventory(1, "shell", "exec-a")).await;
    approve_and_apply(&store).await;
    store
        .rotate_worker_credential("hub_1".into(), worker(), T0)
        .await
        .unwrap();
    let row = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(row.approval, WorkerApprovalStatus::Approved);
    assert_eq!(row.reported_revision, Some(1));
    assert_eq!(row.applied_revision, Some(1));
}

#[tokio::test]
async fn an_in_flight_claim_survives_a_later_config_change() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    approve_and_apply(&store).await;

    // The Worker claims, then immediately reports a scheduling change before the Admin
    // applies it. The committed Delivery keeps its Slot, Executor, and reporting path.
    let (first, _, _) = store
        .create_task("committed".into(), definition("shell"), T0)
        .await
        .unwrap();
    let (committed, _) = store
        .claim_task(worker(), claim(first.id.as_str(), epoch), T0)
        .await
        .unwrap();
    store
        .update_worker_inventory(worker(), epoch, inventory(2, "gpu", "exec-b"), T0)
        .await
        .unwrap();

    let progress: ProgressReportRequest = serde_json::from_value(serde_json::json!({
        "epoch": epoch, "observedAtMs": 1, "text": "still reporting"
    }))
    .unwrap();
    let receipt = store
        .record_progress(worker(), committed.delivery.id.clone(), progress, T0)
        .await
        .unwrap()
        .0;
    assert!(!receipt.duplicate);
    let result: ResultReportRequest = serde_json::from_value(serde_json::json!({
        "epoch": epoch, "text": "# Findings"
    }))
    .unwrap();
    store
        .record_result(worker(), committed.delivery.id.clone(), result, T0)
        .await
        .unwrap();

    // Applying the new revision does not disturb the committed Delivery either.
    store.apply_worker_config(worker(), 2, T0).await.unwrap();
    let view = store
        .get_task(first.id)
        .await
        .unwrap()
        .current_delivery
        .unwrap();
    assert_eq!(view.id, committed.delivery.id);
    assert_eq!(view.executor_id.unwrap().as_str(), "exec-a");
    assert_eq!(view.progress.unwrap().text, "still reporting");
}

#[tokio::test]
async fn concurrent_reports_and_applies_serialize_without_regressions() {
    let store = std::sync::Arc::new(HubStore::open_in_memory().unwrap());
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    store.approve_worker(worker(), T0).await.unwrap();
    let reported = store
        .get_worker(worker(), HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .unwrap();

    let apply = |revision: u64| {
        let store = std::sync::Arc::clone(&store);
        tokio::spawn(async move { store.apply_worker_config(worker(), revision, T0).await })
    };
    let report = {
        let store = std::sync::Arc::clone(&store);
        tokio::spawn(async move {
            store
                .update_worker_inventory(worker(), epoch, inventory(2, "gpu", "exec-b"), T0)
                .await
        })
    };
    let (first, second, report) = tokio::join!(apply(reported), apply(reported), report);
    report.unwrap().unwrap();
    // An Apply is only ever accepted for the revision it was shown. If the concurrent
    // report landed first, both Applies must refuse rather than apply a newer snapshot
    // that no Admin reviewed.
    let outcomes = [first.unwrap(), second.unwrap()];
    let row = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(row.reported_revision, Some(2));
    for outcome in outcomes {
        match outcome {
            Ok(applied) => assert_eq!(applied, reported),
            Err(StoreError::ConfigRevisionMismatch) => {}
            Err(other) => panic!("unexpected apply error: {other}"),
        }
    }
    // Applied never advances past the revision the Admin actually saw.
    assert!(row
        .applied_revision
        .is_none_or(|applied| applied <= reported));
    if row.applied_revision == Some(reported) {
        assert_eq!(
            row.applied_config.unwrap().slots[0].executors[0]
                .id
                .as_str(),
            "exec-a"
        );
    }
}

#[tokio::test]
async fn application_requires_a_pending_report() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    store.approve_worker(worker(), T0).await.unwrap();
    let none = store.apply_worker_config(worker(), 1, T0).await;
    assert!(matches!(none, Err(StoreError::NoPendingConfig)));
}
