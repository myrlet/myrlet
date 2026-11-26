//! End-to-end store tests over an in-memory Hub database.

use std::collections::HashMap;

use myrlet_protocol::{
    CancelTaskRequest, DeliveryStatus, DeliveryView, DispatchTaskRequest, FinishReportRequest,
    FinishTaskRequest, PairWorkerRequest, ProgressReportRequest, TaskBundleImportRequest,
    TaskDefinition, TaskId, TaskRecord, TaskStatus, TaskTreeDirection, WorkerId, WorkerInventory,
};
use myrlet_store::{DeliveryTarget, HubStore, StoreError};

const T0: i64 = 1_764_070_000_000;
const WORKER: &str = "wkr_000000000000";

fn worker() -> WorkerId {
    WorkerId::from(WORKER)
}

fn definition(title: &str) -> TaskDefinition {
    serde_json::from_value(serde_json::json!({ "title": title, "prompt": "do the thing" })).unwrap()
}

fn child(title: &str, parent: &TaskId) -> TaskDefinition {
    let mut definition = definition(title);
    definition.parent_task_id = Some(parent.clone());
    definition
}

fn dispatch_request(version: u64) -> DispatchTaskRequest {
    serde_json::from_value(serde_json::json!({
        "expectedTaskVersion": version,
        "selection": { "mode": "targeted", "workerId": WORKER, "slotId": "slot-a" }
    }))
    .unwrap()
}

fn cancel_request(version: u64) -> CancelTaskRequest {
    serde_json::from_value(serde_json::json!({ "expectedTaskVersion": version })).unwrap()
}

fn finish_request(version: u64) -> FinishTaskRequest {
    serde_json::from_value(serde_json::json!({ "expectedTaskVersion": version })).unwrap()
}

fn progress(epoch: u64, observed_at_ms: i64, text: &str) -> ProgressReportRequest {
    serde_json::from_value(serde_json::json!({
        "epoch": epoch, "observedAtMs": observed_at_ms, "text": text
    }))
    .unwrap()
}

fn finish(epoch: u64) -> FinishReportRequest {
    serde_json::from_value(serde_json::json!({ "epoch": epoch })).unwrap()
}

fn inventory() -> WorkerInventory {
    serde_json::from_value(serde_json::json!({
        "revision": 1,
        "observedAtMs": T0,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{ "slotId": "slot-a", "enabled": true }]
    }))
    .unwrap()
}

fn target(epoch: u64) -> DeliveryTarget {
    DeliveryTarget {
        worker_id: worker(),
        slot_id: "slot-a".into(),
        executor_id: None,
        connection_epoch: epoch,
    }
}

async fn enrolled(store: &HubStore) -> u64 {
    let issued = store
        .create_enrollment_code(Some(600), None, T0)
        .await
        .unwrap();
    let request = pair_request(issued.enrollment_code);
    let (paired, _) = store
        .redeem_pairing_code("hub_1".into(), request, T0)
        .await
        .unwrap();
    store
        .open_worker_connection(paired.worker_id, inventory(), T0)
        .await
        .unwrap()
}

fn pair_request(enrollment_code: String) -> PairWorkerRequest {
    let body = serde_json::json!({ "enrollmentCode": enrollment_code, "workerId": WORKER });
    serde_json::from_value(body).unwrap()
}

async fn create(store: &HubStore, key: &str, definition: TaskDefinition) -> TaskRecord {
    let (record, _, _) = store.create_task(key.into(), definition, T0).await.unwrap();
    record
}

async fn dispatched(
    store: &HubStore,
    epoch: u64,
    definition: TaskDefinition,
    key: &str,
) -> (TaskRecord, DeliveryView) {
    let record = create(store, key, definition).await;
    let (queued, _, _) = store
        .enqueue_task(
            format!("{key}-queue"),
            record.id.clone(),
            dispatch_request(record.version),
            T0,
        )
        .await
        .unwrap();
    let (response, _, _) = store
        .create_delivery(
            Some(format!("{key}-dispatch")),
            queued.task_id.clone(),
            queued.version,
            target(epoch),
            T0,
        )
        .await
        .unwrap();
    (
        store.get_task(queued.task_id).await.unwrap(),
        response.delivery.unwrap(),
    )
}

#[tokio::test]
async fn five_state_lifecycle_with_version_fencing() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let created = store
        .create_task("create".into(), definition("root"), T0)
        .await
        .unwrap();
    let (root, event, replay) = created;
    assert_eq!(
        (root.status, root.version, replay),
        (TaskStatus::Held, 1, false)
    );
    assert_eq!(event.unwrap().kind, "task.created");
    let stale = store
        .cancel_task("c0".into(), root.id.clone(), cancel_request(9), T0)
        .await;
    assert!(matches!(stale, Err(StoreError::VersionConflict)));
    let queued = store
        .enqueue_task("q".into(), root.id.clone(), dispatch_request(1), T0)
        .await
        .unwrap()
        .0;
    assert_eq!(queued.status, TaskStatus::Queued);
    let request = Some("d".to_string());
    let offered = store
        .create_delivery(request, root.id.clone(), queued.version, target(epoch), T0)
        .await
        .unwrap()
        .0;
    let delivery = offered.delivery.clone().unwrap();
    let states = (offered.status, delivery.state);
    assert_eq!(states, (TaskStatus::Dispatched, DeliveryStatus::Offered));
    let first = store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch, 1000, "go"),
            T0,
        )
        .await
        .unwrap()
        .0;
    assert!(!first.duplicate);
    let finished = store
        .record_finish(worker(), delivery.id.clone(), finish(epoch), T0)
        .await
        .unwrap()
        .0;
    assert!(finished.changed);
    assert_eq!(finished.task_status, TaskStatus::Finished);
    let record = store.get_task(root.id).await.unwrap();
    assert_eq!((record.status, record.version), (TaskStatus::Finished, 4));
    assert!(record.finished_at_ms.is_some());
    assert_eq!(
        record.current_delivery.unwrap().state,
        DeliveryStatus::Finished
    );
}

#[tokio::test]
async fn five_level_tree_sibling_dependencies_and_cancel() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let root = create(&store, "root", definition("level1")).await;
    let mut deepest = root.clone();
    for level in 2..=5u8 {
        let title = format!("level{level}");
        deepest = create(&store, &title, child(&title, &deepest.id)).await;
        assert_eq!(deepest.depth, level);
        assert_eq!(deepest.root_task_id, root.id);
    }
    let too_deep = store
        .create_task("l6".into(), child("l6", &deepest.id), T0)
        .await;
    assert!(matches!(too_deep, Err(StoreError::InvalidInput(_))));
    let first = create(&store, "a", child("a", &root.id)).await;
    let mut second_definition = child("b", &root.id);
    second_definition.dependencies = vec![first.id.clone()];
    let second = create(&store, "b", second_definition).await;
    assert!(!store
        .are_dependencies_satisfied(second.id.clone())
        .await
        .unwrap());
    let blocked = store
        .create_delivery(None, second.id.clone(), second.version, target(epoch), T0)
        .await;
    assert!(matches!(blocked, Err(StoreError::InvalidInput(_))));
    let finish = finish_request(first.version);
    store
        .finish_coordination_task("bf".into(), first.id.clone(), finish, T0)
        .await
        .unwrap();
    assert!(store
        .are_dependencies_satisfied(second.id.clone())
        .await
        .unwrap());
    let cancel = cancel_request(second.version);
    let cancelled = store
        .cancel_task("bc".into(), second.id.clone(), cancel, T0)
        .await
        .unwrap();
    assert_eq!(cancelled.0.status, TaskStatus::Cancelled);
    assert!(cancelled.1.is_none());
    let tree = store
        .get_task_tree(root.id.clone(), TaskTreeDirection::Descendants, 5, 50)
        .await
        .unwrap();
    assert_eq!((tree.tasks.len(), tree.truncated), (7, false));
    let bounded = store
        .get_task_tree(root.id.clone(), TaskTreeDirection::Descendants, 1, 50)
        .await
        .unwrap();
    assert!(bounded.truncated);
    let ancestors = store
        .get_task_tree(deepest.id, TaskTreeDirection::Ancestors, 5, 50)
        .await
        .unwrap();
    assert_eq!(ancestors.tasks.len(), 5);
}

fn bundle_request() -> TaskBundleImportRequest {
    serde_json::from_value(serde_json::json!({
        "tasks": [
            { "clientRef": "root", "title": "root", "prompt": "p" },
            { "clientRef": "a", "title": "a", "prompt": "p", "parentTaskId": "root" },
            { "clientRef": "b", "title": "b", "prompt": "p", "parentTaskId": "root",
              "dependencies": ["a"] }
        ]
    }))
    .unwrap()
}

#[tokio::test]
async fn bundle_import_is_atomic_and_resolves_local_refs() {
    let store = HubStore::open_in_memory().unwrap();
    let (response, events, replay) = store
        .import_bundle("bundle".into(), bundle_request(), T0)
        .await
        .unwrap();
    assert_eq!((response.tasks.len(), events.len(), replay), (3, 3, false));
    let root = response.client_ref_to_task_id["root"].clone();
    let first = response.client_ref_to_task_id["a"].clone();
    let record = store
        .get_task(response.client_ref_to_task_id["b"].clone())
        .await
        .unwrap();
    assert_eq!((record.depth, record.task.dependencies), (2, vec![first]));
    assert_eq!(record.task.parent_task_id, Some(root));
    let (replayed, events, replay) = store
        .import_bundle("bundle".into(), bundle_request(), T0)
        .await
        .unwrap();
    assert!(replay && events.is_empty());
    assert_eq!(replayed.client_ref_to_task_id["b"], record.id);
    let bad: TaskBundleImportRequest = serde_json::from_value(serde_json::json!({
        "tasks": [{ "title": "orphan", "prompt": "p", "parentTaskId": "task_missing" }]
    }))
    .unwrap();
    let failed = store.import_bundle("bundle-bad".into(), bad, T0).await;
    assert!(matches!(failed, Err(StoreError::NotFound)));
    let snapshot = store.list_snapshot(HashMap::new(), 100, T0).await.unwrap();
    assert_eq!((snapshot.total_tasks, snapshot.tasks.len() as u64), (3, 3));
}

#[tokio::test]
async fn pairing_rotation_and_revocation_fence_credentials() {
    let store = HubStore::open_in_memory().unwrap();
    let issued = store
        .create_enrollment_code(Some(60), None, T0)
        .await
        .unwrap();
    assert_eq!(issued.expires_at_ms, T0 + 60_000);
    let request = pair_request(issued.enrollment_code);
    let (paired, event) = store
        .redeem_pairing_code("hub_1".into(), request.clone(), T0)
        .await
        .unwrap();
    assert_eq!(event.kind, "worker.paired");
    assert_eq!(
        (paired.worker_id.clone(), paired.credential.len()),
        (worker(), 64)
    );
    let authenticated = store
        .authenticate_worker(paired.credential.clone())
        .await
        .unwrap();
    assert_eq!(authenticated, worker());
    let reuse = store.redeem_pairing_code("hub_1".into(), request, T0).await;
    assert!(matches!(reuse, Err(StoreError::Unauthorized)));
    let rotated = store
        .rotate_worker_credential("hub_1".into(), worker(), T0)
        .await
        .unwrap()
        .0;
    let old = store.authenticate_worker(paired.credential).await;
    assert!(matches!(old, Err(StoreError::Unauthorized)));
    assert_eq!(
        store
            .authenticate_worker(rotated.credential.clone())
            .await
            .unwrap(),
        worker()
    );
    assert_eq!(
        store.revoke_worker(worker(), T0).await.unwrap().kind,
        "worker.revoked"
    );
    let revoked = store.authenticate_worker(rotated.credential).await;
    assert!(matches!(revoked, Err(StoreError::Unauthorized)));
    let workers = store.list_workers(HashMap::new()).await.unwrap();
    assert!(workers[0].revoked && !workers[0].online);
}

#[tokio::test]
async fn idempotency_replay_and_conflicting_reuse() {
    let store = HubStore::open_in_memory().unwrap();
    let first = store
        .create_task("same-key".into(), definition("one"), T0)
        .await
        .unwrap()
        .0;
    let (replayed, event, replay) = store
        .create_task("same-key".into(), definition("one"), T0)
        .await
        .unwrap();
    assert!(replay && event.is_none());
    assert_eq!(replayed.id, first.id);
    let conflict = store
        .create_task("same-key".into(), definition("two"), T0)
        .await;
    assert!(matches!(conflict, Err(StoreError::IdempotencyConflict)));
    assert_eq!(
        store.list_events_after(0, 100, 10_000).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn progress_promotion_monotonicity_and_stale_epoch() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (record, delivery) = dispatched(&store, epoch, definition("p"), "p").await;
    let request = progress(epoch, 100, "one");
    let first = store
        .record_progress(worker(), delivery.id.clone(), request, T0)
        .await
        .unwrap()
        .0;
    assert_eq!((first.first_accepted_at_ms, first.duplicate), (T0, false));
    let view = store
        .get_task(record.id.clone())
        .await
        .unwrap()
        .current_delivery
        .unwrap();
    assert_eq!(view.state, DeliveryStatus::Accepted);
    let later = progress(epoch, 200, "two");
    let second = store
        .record_progress(worker(), delivery.id.clone(), later.clone(), T0 + 10)
        .await
        .unwrap()
        .0;
    assert_eq!(
        (second.accepted_at_ms, second.first_accepted_at_ms),
        (T0 + 10, T0)
    );
    let backwards = progress(epoch, 150, "old");
    let stale = store
        .record_progress(worker(), delivery.id.clone(), backwards, T0 + 20)
        .await;
    assert!(matches!(stale, Err(StoreError::StaleDelivery)));
    let duplicate = store
        .record_progress(worker(), delivery.id.clone(), later, T0 + 30)
        .await
        .unwrap();
    assert!(duplicate.0.duplicate && duplicate.1.is_none());
    assert_eq!(duplicate.0.first_accepted_at_ms, T0);
    let future_epoch = store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch + 1, 300, "x"),
            T0,
        )
        .await;
    assert!(matches!(future_epoch, Err(StoreError::StaleDelivery)));
    assert_eq!(
        store.get_task(record.id).await.unwrap().status,
        TaskStatus::Dispatched
    );
}

#[tokio::test]
async fn finish_without_progress_and_cancel_is_never_overwritten() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (_, delivery) = dispatched(&store, epoch, definition("n"), "n").await;
    let stale = store
        .record_finish(worker(), delivery.id.clone(), finish(epoch + 1), T0)
        .await;
    assert!(matches!(stale, Err(StoreError::StaleDelivery)));
    let receipt = store
        .record_finish(worker(), delivery.id.clone(), finish(epoch), T0)
        .await
        .unwrap()
        .0;
    assert!(receipt.changed);
    assert_eq!(receipt.task_status, TaskStatus::Finished);
    let replay = store
        .record_finish(worker(), delivery.id.clone(), finish(epoch), T0)
        .await
        .unwrap()
        .0;
    assert!(!replay.changed);

    let (cancelled, cancelled_delivery) = dispatched(&store, epoch, definition("c"), "c").await;
    let cancel = cancel_request(cancelled.version);
    let outcome = store
        .cancel_task("cc".into(), cancelled.id.clone(), cancel, T0)
        .await
        .unwrap();
    assert_eq!(outcome.0.status, TaskStatus::Cancelled);
    assert_eq!(outcome.1.unwrap().state, DeliveryStatus::Cancelled);
    let late = store
        .record_finish(worker(), cancelled_delivery.id, finish(epoch), T0)
        .await
        .unwrap()
        .0;
    assert!(!late.changed);
    assert_eq!(late.task_status, TaskStatus::Cancelled);
    assert_eq!(
        store.get_task(cancelled.id).await.unwrap().status,
        TaskStatus::Cancelled
    );
}

#[tokio::test]
async fn event_backfill_and_cursor_expiry() {
    let store = HubStore::open_in_memory().unwrap();
    for index in 0..4 {
        create(&store, &format!("e{index}"), definition("e")).await;
    }
    let latest = store.latest_seq().await.unwrap();
    assert_eq!(latest, 4);
    let backfill = store.list_events_after(0, 100, 10_000).await.unwrap();
    assert_eq!(backfill.len(), 4);
    assert!(backfill.iter().all(|event| event.kind == "task.created"));
    assert!(store
        .list_events_after(latest, 100, 10_000)
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        store.list_events_after(0, 100, 3).await,
        Err(StoreError::CursorExpired)
    ));
    assert_eq!(
        store
            .list_events_after(latest - 3, 100, 3)
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn resync_rebinds_epoch_and_flags_unacked_cancelled_deliveries() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch1 = enrolled(&store).await;
    let (active_task, active_delivery) =
        dispatched(&store, epoch1, definition("live"), "live").await;
    let (cancelled_task, cancelled_delivery) =
        dispatched(&store, epoch1, definition("off"), "off").await;
    store
        .cancel_task(
            "cancel-off".into(),
            cancelled_task.id.clone(),
            cancel_request(cancelled_task.version),
            T0,
        )
        .await
        .unwrap();
    let cross = store
        .cancel_task(
            "cancel-off".into(),
            active_task.id.clone(),
            cancel_request(active_task.version),
            T0,
        )
        .await;
    assert!(matches!(cross, Err(StoreError::IdempotencyConflict)));

    let epoch2 = store
        .open_worker_connection(worker(), inventory(), T0 + 10)
        .await
        .unwrap();
    let needs_cancel = store
        .rebind_deliveries_on_resync(
            worker(),
            epoch2,
            vec![active_delivery.id.clone(), cancelled_delivery.id.clone()],
            T0 + 20,
        )
        .await
        .unwrap();
    assert_eq!(needs_cancel, vec![cancelled_delivery.id]);
    let receipt = store
        .record_progress(
            worker(),
            active_delivery.id,
            progress(epoch2, 500, "resumed"),
            T0 + 30,
        )
        .await
        .unwrap()
        .0;
    assert!(!receipt.duplicate);
}
