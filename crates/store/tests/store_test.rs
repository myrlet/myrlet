//! End-to-end store tests over an in-memory Hub database.

use std::collections::HashMap;

use myrlet_protocol::{
    CancelTaskRequest, ClaimTaskRequest, DeliveryStatus, FinishReportRequest, FinishTaskRequest,
    PairWorkerRequest, ProgressReportRequest, ResultReportRequest, TaskBundleImportRequest,
    TaskDefinition, TaskId, TaskRecord, TaskStatus, TaskTreeDirection, UpdateTaskRequest, WorkerId,
    WorkerInventory,
};
use myrlet_store::{HubStore, StoreError};

const T0: i64 = 1_764_070_000_000;
const WORKER: &str = "wkr_000000000000";
const SLOT: &str = "slot-a";

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

fn cancel_request(version: u64) -> CancelTaskRequest {
    serde_json::from_value(serde_json::json!({ "expectedTaskVersion": version })).unwrap()
}

fn finish_request(version: u64) -> FinishTaskRequest {
    serde_json::from_value(serde_json::json!({ "expectedTaskVersion": version })).unwrap()
}

fn update_request(version: u64, dependencies: Vec<TaskId>) -> UpdateTaskRequest {
    serde_json::from_value(serde_json::json!({
        "expectedTaskVersion": version,
        "dependencies": dependencies,
    }))
    .unwrap()
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

fn result(epoch: u64, text: Option<&str>) -> ResultReportRequest {
    serde_json::from_value(serde_json::json!({ "epoch": epoch, "text": text })).unwrap()
}

fn inventory() -> WorkerInventory {
    serde_json::from_value(serde_json::json!({
        "revision": 1,
        "observedAtMs": T0,
        "host": { "os": "linux", "arch": "x86_64", "deploymentMode": "native" },
        "slots": [{
            "slotId": SLOT,
            "enabled": true,
            "capabilities": [{ "name": "shell", "state": "available" }],
            "executors": [{ "id": "exec-claude", "kind": "claude" }],
        }]
    }))
    .unwrap()
}

/// Pair a Worker, connect it, and clear both default-on approval gates so ordinary
/// claim tests start from an approved Worker whose reported inventory is applied.
async fn enrolled(store: &HubStore) -> u64 {
    let (worker_id, _credential) = paired(store).await;
    let epoch = store
        .open_worker_connection(worker_id.clone(), inventory(), T0)
        .await
        .unwrap();
    store.approve_worker(worker_id.clone(), T0).await.unwrap();
    let reported = store
        .get_worker(worker_id, HashMap::new())
        .await
        .unwrap()
        .reported_revision
        .expect("a report exists");
    store
        .apply_worker_config(worker(), reported, T0)
        .await
        .unwrap();
    epoch
}

/// Pair a Worker under the current policies without opening a connection or approving it.
async fn paired(store: &HubStore) -> (WorkerId, String) {
    let issued = store
        .create_enrollment_code(Some(600), None, T0)
        .await
        .unwrap();
    let request = pair_request(issued.enrollment_code);
    let (paired, _) = store
        .redeem_pairing_code("hub_1".into(), request, T0)
        .await
        .unwrap();
    (paired.worker_id, paired.credential)
}

fn pair_request(enrollment_code: String) -> PairWorkerRequest {
    pair_request_for(enrollment_code, WORKER)
}

fn pair_request_for(enrollment_code: String, worker_id: &str) -> PairWorkerRequest {
    let body = serde_json::json!({ "enrollmentCode": enrollment_code, "workerId": worker_id });
    serde_json::from_value(body).unwrap()
}

async fn submit(store: &HubStore, key: &str, definition: TaskDefinition) -> TaskRecord {
    let (record, _, _) = store.create_task(key.into(), definition, T0).await.unwrap();
    record
}

fn claim(task_id: &TaskId, epoch: u64) -> ClaimTaskRequest {
    ClaimTaskRequest {
        task_id: task_id.clone(),
        slot_id: SLOT.into(),
        connection_epoch: epoch,
        executor_id: None,
    }
}

/// Submit one Task and claim it, returning the record and its committed Delivery.
async fn claimed(
    store: &HubStore,
    epoch: u64,
    definition: TaskDefinition,
    key: &str,
) -> (TaskRecord, myrlet_protocol::DeliveryView) {
    let record = submit(store, key, definition).await;
    let (response, _) = store
        .claim_task(worker(), claim(&record.id, epoch), T0)
        .await
        .unwrap();
    (response.task, response.delivery)
}

#[tokio::test]
async fn submission_authorizes_ready_tasks_and_holds_blocked_ones() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("root")).await;
    assert_eq!(root.status, TaskStatus::Queued);

    let first = submit(&store, "a", child("a", &root.id)).await;
    assert_eq!(first.status, TaskStatus::Queued, "no prerequisites yet");

    let mut blocked_definition = child("b", &root.id);
    blocked_definition.dependencies = vec![first.id.clone()];
    let blocked = submit(&store, "b", blocked_definition).await;
    assert_eq!(blocked.status, TaskStatus::Held);
    assert_eq!(blocked.blocked_by.len(), 1);
    assert_eq!(blocked.blocked_by[0].task_id, first.id);
    assert_eq!(blocked.blocked_by[0].status, TaskStatus::Queued);
}

#[tokio::test]
async fn finishing_a_prerequisite_promotes_only_its_ready_dependents() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("root")).await;
    let first = submit(&store, "a", child("a", &root.id)).await;
    let mut second_definition = child("b", &root.id);
    second_definition.dependencies = vec![first.id.clone()];
    let second = submit(&store, "b", second_definition).await;
    assert_eq!(second.status, TaskStatus::Held);

    store
        .finish_coordination_task(
            "f1".into(),
            first.id.clone(),
            finish_request(first.version),
            T0,
        )
        .await
        .unwrap();
    let promoted = store.get_task(second.id.clone()).await.unwrap();
    assert_eq!(promoted.status, TaskStatus::Queued);
    assert!(promoted.blocked_by.is_empty());
}

#[tokio::test]
async fn cancelled_prerequisite_keeps_dependents_held_until_the_owner_edits() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("root")).await;
    let prerequisite = submit(&store, "a", child("a", &root.id)).await;
    let mut dependent_definition = child("b", &root.id);
    dependent_definition.dependencies = vec![prerequisite.id.clone()];
    let dependent = submit(&store, "b", dependent_definition).await;
    assert_eq!(dependent.status, TaskStatus::Held);

    store
        .cancel_task(
            "c1".into(),
            prerequisite.id.clone(),
            cancel_request(prerequisite.version),
            T0,
        )
        .await
        .unwrap();
    // Cancellation never edits the graph or cascades on its own.
    let still_held = store.get_task(dependent.id.clone()).await.unwrap();
    assert_eq!(still_held.status, TaskStatus::Held);
    assert_eq!(still_held.blocked_by[0].status, TaskStatus::Cancelled);

    // The Owner replaces the cancelled prerequisite with a finished sibling.
    let replacement = submit(&store, "c", child("c", &root.id)).await;
    store
        .finish_coordination_task(
            "f2".into(),
            replacement.id.clone(),
            finish_request(replacement.version),
            T0,
        )
        .await
        .unwrap();
    store
        .update_task(
            "u1".into(),
            dependent.id.clone(),
            update_request(dependent.version, vec![replacement.id.clone()]),
            T0,
        )
        .await
        .unwrap();
    let eligible = store.get_task(dependent.id).await.unwrap();
    assert_eq!(eligible.status, TaskStatus::Queued);
    assert!(eligible.task.dependencies.contains(&replacement.id));
}

#[tokio::test]
async fn update_rejects_cycles_self_reference_and_claimed_tasks() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let root = submit(&store, "root", definition("root")).await;
    let first = submit(&store, "a", child("a", &root.id)).await;
    let second = submit(&store, "b", child("b", &root.id)).await;
    let self_reference = store
        .update_task(
            "self".into(),
            first.id.clone(),
            update_request(first.version, vec![first.id.clone()]),
            T0,
        )
        .await;
    assert!(matches!(self_reference, Err(StoreError::InvalidInput(_))));
    store
        .update_task(
            "u1".into(),
            second.id.clone(),
            update_request(second.version, vec![first.id.clone()]),
            T0,
        )
        .await
        .unwrap();
    let cycle = store
        .update_task(
            "u2".into(),
            first.id.clone(),
            update_request(first.version, vec![second.id.clone()]),
            T0,
        )
        .await;
    assert!(matches!(cycle, Err(StoreError::InvalidInput(_))));
    // A claimed Task refuses edits, so the claim's execution context stands.
    let (_, delivery) = claimed(&store, epoch, definition("claimed"), "claimed").await;
    let current = store.get_task(delivery.task_id.clone()).await.unwrap();
    let claimed_edit = store
        .update_task(
            "u3".into(),
            delivery.task_id.clone(),
            update_request(current.version, Vec::new()),
            T0,
        )
        .await;
    assert!(matches!(claimed_edit, Err(StoreError::InvalidTransition)));
}

#[tokio::test]
async fn claim_commits_one_delivery_and_racing_claims_yield_one_winner() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let record = submit(&store, "c", definition("one winner")).await;
    let (first, event) = store
        .claim_task(worker(), claim(&record.id, epoch), T0)
        .await
        .unwrap();
    assert_eq!(first.task.status, TaskStatus::Dispatched);
    assert_eq!(first.delivery.state, DeliveryStatus::Accepted);
    assert_eq!(event.unwrap().kind, "task.dispatched");

    // The winning Worker replaying its own claim recovers the same Delivery: a lost
    // acknowledgement never produces a replacement event or a second Delivery.
    let (recovered, replayed_event) = store
        .claim_task(worker(), claim(&record.id, epoch), T0)
        .await
        .unwrap();
    assert!(replayed_event.is_none(), "a replay commits nothing new");
    assert_eq!(recovered.delivery.id, first.delivery.id);

    // A different Worker still loses the race even at a live epoch.
    let other = WorkerId::new("wkr_111111111111");
    store
        .redeem_pairing_code(
            "hub_1".into(),
            pair_request_for(
                store
                    .create_enrollment_code(Some(600), Some(other.clone()), T0)
                    .await
                    .unwrap()
                    .enrollment_code,
                other.as_str(),
            ),
            T0,
        )
        .await
        .unwrap();
    let other_epoch = store
        .open_worker_connection(other.clone(), inventory(), T0)
        .await
        .unwrap();
    let second = store
        .claim_task(other, claim(&record.id, other_epoch), T0)
        .await;
    assert!(matches!(second, Err(StoreError::AlreadyClaimed)));
    let detail = store.get_task(record.id).await.unwrap();
    assert_eq!(detail.status, TaskStatus::Dispatched);
    assert_eq!(
        detail.current_delivery.unwrap().id,
        first.delivery.id,
        "one Task has exactly one Delivery"
    );
}

#[tokio::test]
async fn claim_requires_the_live_epoch_and_an_eligible_slot() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let record = submit(&store, "e", definition("epoch")).await;
    let stale = store
        .claim_task(worker(), claim(&record.id, epoch + 1), T0)
        .await;
    assert!(matches!(stale, Err(StoreError::StaleDelivery)));

    let task: TaskDefinition = serde_json::from_value(serde_json::json!({
        "title": "needs capability",
        "prompt": "p",
        "routing": { "requiredCapabilities": ["gpu"] },
    }))
    .unwrap();
    let demanding = submit(&store, "cap", task).await;
    let unmet = store
        .claim_task(worker(), claim(&demanding.id, epoch), T0)
        .await;
    assert!(matches!(unmet, Err(StoreError::InvalidInput(_))));
    assert_eq!(
        store.get_task(demanding.id).await.unwrap().status,
        TaskStatus::Queued,
        "an unmet hard constraint leaves the Task queued"
    );
}

#[tokio::test]
async fn held_tasks_and_cancelled_tasks_cannot_be_claimed() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let root = submit(&store, "root", definition("root")).await;
    let prerequisite = submit(&store, "a", child("a", &root.id)).await;
    let mut blocked_definition = child("b", &root.id);
    blocked_definition.dependencies = vec![prerequisite.id.clone()];
    let blocked = submit(&store, "b", blocked_definition).await;
    let held = store
        .claim_task(worker(), claim(&blocked.id, epoch), T0)
        .await;
    assert!(matches!(held, Err(StoreError::InvalidTransition)));

    store
        .cancel_task(
            "c".into(),
            prerequisite.id.clone(),
            cancel_request(prerequisite.version),
            T0,
        )
        .await
        .unwrap();
    let cancelled = store
        .claim_task(worker(), claim(&prerequisite.id, epoch), T0)
        .await;
    assert!(matches!(cancelled, Err(StoreError::InvalidTransition)));
}

#[tokio::test]
async fn five_level_tree_depth_and_cancel_without_cascade() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("level1")).await;
    let mut deepest = root.clone();
    for level in 2..=5u8 {
        let title = format!("level{level}");
        deepest = submit(&store, &title, child(&title, &deepest.id)).await;
        assert_eq!(deepest.depth, level);
        assert_eq!(deepest.root_task_id, root.id);
        assert_eq!(deepest.status, TaskStatus::Queued);
    }
    let too_deep = store
        .create_task("l6".into(), child("l6", &deepest.id), T0)
        .await;
    assert!(matches!(too_deep, Err(StoreError::InvalidInput(_))));
    let tree = store
        .get_task_tree(root.id.clone(), TaskTreeDirection::Descendants, 5, 50)
        .await
        .unwrap();
    assert_eq!((tree.tasks.len(), tree.truncated), (5, false));
    let ancestors = store
        .get_task_tree(deepest.id.clone(), TaskTreeDirection::Ancestors, 5, 50)
        .await
        .unwrap();
    assert_eq!(ancestors.tasks.len(), 5);
    let cancelled = store
        .cancel_task(
            "c".into(),
            root.id.clone(),
            cancel_request(root.version),
            T0,
        )
        .await
        .unwrap();
    assert_eq!(cancelled.0.status, TaskStatus::Cancelled);
    let subtree = store
        .get_task_tree(root.id, TaskTreeDirection::Descendants, 5, 50)
        .await
        .unwrap();
    assert!(subtree
        .tasks
        .iter()
        .skip(1)
        .all(|task| task.status == TaskStatus::Queued));
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
async fn bundle_import_is_atomic_and_authorizes_within_one_transaction() {
    let store = HubStore::open_in_memory().unwrap();
    let (response, events, replay) = store
        .import_bundle("bundle".into(), bundle_request(), T0)
        .await
        .unwrap();
    assert!(events.len() >= 3 && !replay);
    let root = response.client_ref_to_task_id["root"].clone();
    let first = response.client_ref_to_task_id["a"].clone();
    let second = response.client_ref_to_task_id["b"].clone();
    assert_eq!(
        response.eligible_task_ids,
        vec![root.clone(), first.clone()],
        "b waits on a"
    );
    let blocked = store.get_task(second.clone()).await.unwrap();
    assert_eq!((blocked.depth, blocked.status), (2, TaskStatus::Held));
    assert_eq!(blocked.task.dependencies, vec![first]);
    let (replayed, events, replay) = store
        .import_bundle("bundle".into(), bundle_request(), T0)
        .await
        .unwrap();
    assert!(replay && events.is_empty());
    assert_eq!(replayed.client_ref_to_task_id["b"], blocked.id);
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
async fn submission_is_idempotent_and_conflicting_reuse_is_refused() {
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
}

#[tokio::test]
async fn progress_is_monotonic_epoch_fenced_and_only_the_owner_writes() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (record, delivery) = claimed(&store, epoch, definition("p"), "p").await;
    let first = store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch, 100, "one"),
            T0,
        )
        .await
        .unwrap()
        .0;
    assert_eq!((first.first_accepted_at_ms, first.duplicate), (T0, false));
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
    let future_epoch = store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch + 1, 300, "x"),
            T0,
        )
        .await;
    assert!(matches!(future_epoch, Err(StoreError::StaleDelivery)));
    let foreign = store
        .record_progress(
            WorkerId::new("wkr_ZZZZZZZZZZZZ"),
            delivery.id.clone(),
            progress(epoch, 400, "x"),
            T0,
        )
        .await;
    assert!(matches!(foreign, Err(StoreError::Unauthorized)));
    assert_eq!(
        store.get_task(record.id).await.unwrap().status,
        TaskStatus::Dispatched,
        "Progress is never a verdict"
    );
}

#[tokio::test]
async fn finish_without_progress_and_cancel_is_never_overwritten() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (_, delivery) = claimed(&store, epoch, definition("n"), "n").await;
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

    let (cancelled, cancelled_delivery) = claimed(&store, epoch, definition("c"), "c").await;
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
async fn finish_releases_ready_siblings_in_the_same_commit() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("root")).await;
    let first = submit(&store, "a", child("a", &root.id)).await;
    let mut second_definition = child("b", &root.id);
    second_definition.dependencies = vec![first.id.clone()];
    let second = submit(&store, "b", second_definition).await;
    let epoch = enrolled(&store).await;
    let (_, first_delivery) = store
        .claim_task(worker(), claim(&first.id, epoch), T0)
        .await
        .map(|(response, _)| (response.task, response.delivery))
        .unwrap();
    store
        .record_finish(worker(), first_delivery.id, finish(epoch), T0)
        .await
        .unwrap();
    assert_eq!(
        store.get_task(second.id).await.unwrap().status,
        TaskStatus::Queued
    );
}

#[tokio::test]
async fn result_reconciles_latest_content_and_clears_on_explicit_absence() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (record, delivery) = claimed(&store, epoch, definition("r"), "r").await;
    let (receipt, event) = store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# Findings")),
            T0 + 1,
        )
        .await
        .unwrap();
    assert!(!receipt.duplicate && !receipt.cleared);
    assert_eq!(event.unwrap().kind, "delivery.result");

    let replay = store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# Findings")),
            T0 + 2,
        )
        .await
        .unwrap()
        .0;
    assert!(replay.duplicate, "equal content is a no-op");

    // Different content replaces the value; there is no write-once rule.
    let changed = store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# Other")),
            T0 + 3,
        )
        .await
        .unwrap()
        .0;
    assert!(!changed.duplicate && !changed.cleared);
    let detail = store.get_task(record.id.clone()).await.unwrap();
    assert_eq!(detail.result_markdown.as_deref(), Some("# Other"));
    assert_eq!(detail.result_recorded_at_ms, Some(T0 + 3));
    assert_eq!(
        detail.status,
        TaskStatus::Dispatched,
        "result is not a verdict"
    );

    // Finish does not erase it, and sync keeps working afterwards.
    store
        .record_finish(worker(), delivery.id.clone(), finish(epoch), T0 + 4)
        .await
        .unwrap();
    let after_finish = store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# After finish")),
            T0 + 5,
        )
        .await
        .unwrap()
        .0;
    assert!(!after_finish.cleared);
    assert_eq!(
        store
            .get_task(record.id.clone())
            .await
            .unwrap()
            .result_markdown
            .as_deref(),
        Some("# After finish")
    );

    // A confirmed absence clears the Hub copy, and repeating it is harmless.
    let cleared = store
        .record_result(worker(), delivery.id.clone(), result(epoch, None), T0 + 6)
        .await
        .unwrap()
        .0;
    assert!(cleared.cleared && !cleared.duplicate);
    assert!(store
        .get_task(record.id.clone())
        .await
        .unwrap()
        .result_markdown
        .is_none());
    let repeat = store
        .record_result(worker(), delivery.id.clone(), result(epoch, None), T0 + 7)
        .await
        .unwrap()
        .0;
    assert!(repeat.duplicate && !repeat.cleared);
    assert!(store
        .get_task(record.id)
        .await
        .unwrap()
        .result_markdown
        .is_none());
}

#[tokio::test]
async fn delayed_old_epoch_result_never_undoes_a_newer_value() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch1 = enrolled(&store).await;
    let (record, delivery) = claimed(&store, epoch1, definition("late"), "late").await;
    store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch1, Some("# v1")),
            T0 + 1,
        )
        .await
        .unwrap();

    // Reconnect fences the old epoch; the newer value commits under the new epoch.
    let epoch2 = store
        .open_worker_connection(worker(), inventory(), T0 + 10)
        .await
        .unwrap();
    assert!(epoch2 > epoch1);
    store
        .rebind_deliveries_on_resync(worker(), epoch2, vec![delivery.id.clone()], T0 + 11)
        .await
        .unwrap();
    store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch2, Some("# v2")),
            T0 + 12,
        )
        .await
        .unwrap();

    // The delayed request arrives carrying the old epoch and is refused at the boundary.
    let delayed = store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch1, Some("# v1 delayed")),
            T0 + 13,
        )
        .await;
    assert!(matches!(delayed, Err(StoreError::StaleDelivery)));
    assert_eq!(
        store
            .get_task(record.id)
            .await
            .unwrap()
            .result_markdown
            .as_deref(),
        Some("# v2")
    );
}

#[tokio::test]
async fn resync_rebinds_epoch_and_flags_unacked_cancelled_deliveries() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch1 = enrolled(&store).await;
    let (active_task, active_delivery) = claimed(&store, epoch1, definition("live"), "live").await;
    let (cancelled_task, cancelled_delivery) =
        claimed(&store, epoch1, definition("off"), "off").await;
    store
        .cancel_task(
            "cancel-off".into(),
            cancelled_task.id.clone(),
            cancel_request(cancelled_task.version),
            T0,
        )
        .await
        .unwrap();

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
    assert_eq!(
        store.get_task(active_task.id).await.unwrap().status,
        TaskStatus::Dispatched
    );
}

#[tokio::test]
async fn event_backfill_and_cursor_expiry() {
    let store = HubStore::open_in_memory().unwrap();
    for index in 0..4 {
        submit(&store, &format!("e{index}"), definition("e")).await;
    }
    let latest = store.latest_seq().await.unwrap();
    // Each ready Task emits `task.created` plus `task.queued`; a held one only `created`.
    assert_eq!(latest, 8);
    let backfill = store.list_events_after(0, 100, 10_000).await.unwrap();
    assert!(backfill.len() >= 4);
    assert!(store
        .list_events_after(latest, 100, 10_000)
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        store.list_events_after(0, 100, 3).await,
        Err(StoreError::CursorExpired)
    ));
}

#[tokio::test]
async fn reads_expose_blockers_and_claimability() {
    let store = HubStore::open_in_memory().unwrap();
    let root = submit(&store, "root", definition("root")).await;
    let first = submit(&store, "a", child("a", &root.id)).await;
    let mut blocked_definition = child("b", &root.id);
    blocked_definition.dependencies = vec![first.id.clone()];
    let blocked = submit(&store, "b", blocked_definition).await;

    let ready = store.task_eligibility(first.id.clone()).await.unwrap();
    assert!(ready.claimable);
    let waiting = store.task_eligibility(blocked.id.clone()).await.unwrap();
    assert!(!waiting.claimable);
    assert_eq!(waiting.blocked_by[0].task_id, first.id);

    let page = store.list_tasks(100).await.unwrap();
    assert_eq!(page.tasks.len(), 3);
    assert!(page.tasks.iter().any(|task| !task.blocked_by.is_empty()));
}

#[tokio::test]
async fn progress_and_result_are_independent_latest_values() {
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (record, delivery) = claimed(&store, epoch, definition("both"), "both").await;

    store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch, 100, "working"),
            T0,
        )
        .await
        .unwrap();
    store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# Draft")),
            T0 + 1,
        )
        .await
        .unwrap();
    // Updating one never rewrites the other, and neither changes Task status.
    store
        .record_progress(
            worker(),
            delivery.id.clone(),
            progress(epoch, 200, "later"),
            T0 + 2,
        )
        .await
        .unwrap();
    let detail = store.get_task(record.id.clone()).await.unwrap();
    assert_eq!(
        detail
            .current_delivery
            .as_ref()
            .unwrap()
            .progress
            .as_ref()
            .unwrap()
            .text,
        "later"
    );
    assert_eq!(detail.result_markdown.as_deref(), Some("# Draft"));
    assert_eq!(detail.status, TaskStatus::Dispatched);

    store
        .record_result(
            worker(),
            delivery.id.clone(),
            result(epoch, Some("# Revised")),
            T0 + 3,
        )
        .await
        .unwrap();
    let detail = store.get_task(record.id).await.unwrap();
    assert_eq!(
        detail.current_delivery.unwrap().progress.unwrap().text,
        "later",
        "a new Result leaves Progress untouched"
    );
    assert_eq!(detail.result_markdown.as_deref(), Some("# Revised"));
}

#[tokio::test]
async fn a_worker_refuses_a_delivery_id_it_never_registered_locally() {
    // The Hub can only report on Deliveries; a client asking about an unknown one gets a
    // `NotFound`, which is how the Worker's own control channel answers a stale ID.
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let (record, delivery) = claimed(&store, epoch, definition("known"), "known").await;
    let view = store
        .worker_delivery_result(worker(), delivery.id.clone())
        .await
        .unwrap();
    assert_eq!(view.delivery_id, delivery.id);
    assert!(view.text.is_none());
    assert_eq!(record.status, TaskStatus::Dispatched);
    let unknown = store
        .worker_delivery_result(
            worker(),
            myrlet_protocol::DeliveryId::new("delivery_missing"),
        )
        .await;
    assert!(matches!(unknown, Err(StoreError::NotFound)));
    let foreign = store
        .worker_delivery_result(WorkerId::new("wkr_ZZZZZZZZZZZZ"), delivery.id)
        .await;
    assert!(matches!(foreign, Err(StoreError::Unauthorized)));
}

#[tokio::test]
async fn an_owner_read_never_grants_management_authority() {
    // The store has no read-scoped mutation: every management call checks its own
    // precondition, so reading a record cannot cancel or rotate anything.
    let store = HubStore::open_in_memory().unwrap();
    let epoch = enrolled(&store).await;
    let record = submit(&store, "owned", definition("owned")).await;
    let read = store.get_task(record.id.clone()).await.unwrap();
    assert_eq!(read.status, TaskStatus::Queued);
    assert_eq!(read.version, record.version);
    // A claim needs the live epoch; merely reading the Task cannot authorize one.
    let stale = store
        .claim_task(worker(), claim(&record.id, epoch + 9), T0)
        .await;
    assert!(matches!(stale, Err(StoreError::StaleDelivery)));
    assert_eq!(
        store.get_task(record.id).await.unwrap().status,
        TaskStatus::Queued
    );
}
