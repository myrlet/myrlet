//! The report revision rule and the Security & Trust policy matrix: all four
//! approval/apply combinations, the rule that a repeated report never applies a pending
//! decision, and the guarantee that toggles never act retroactively.

use std::collections::HashMap;

use myrlet_protocol::{TaskStatus, WorkerApprovalStatus};
use myrlet_store::HubStore;

use super::helpers::{
    approve_and_apply, claim, connect, definition, inventory, pair, set_policies, worker, T0,
    WORKER,
};

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

    let (stale, _events) = store
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

/// All four approval/apply combinations: the Pair decision follows only the approval
/// policy, and the first report takes effect immediately only when automation allows it.
#[tokio::test]
async fn every_automatic_policy_combination_applies_to_its_own_decision() {
    for approval_policy in [true, false] {
        for apply_policy in [true, false] {
            let store = HubStore::open_in_memory().unwrap();
            set_policies(&store, approval_policy, apply_policy).await;
            pair(&store).await;
            let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;

            let snapshot = &store.list_workers(HashMap::new()).await.unwrap()[0];
            let expected_approval = if approval_policy {
                WorkerApprovalStatus::Pending
            } else {
                WorkerApprovalStatus::Approved
            };
            assert_eq!(
                snapshot.approval, expected_approval,
                "approval policy {approval_policy}"
            );
            assert_eq!(snapshot.reported_revision, Some(1));

            if apply_policy {
                // Manual Apply: the first report stays pending even when approval was auto.
                assert_eq!(
                    snapshot.applied_revision, None,
                    "apply policy {apply_policy} keeps the report pending"
                );
                store.apply_worker_config(worker(), 1, T0).await.unwrap();
            } else {
                // Automatic Apply: the first report is applied in its own transaction.
                assert_eq!(
                    snapshot.applied_revision,
                    Some(1),
                    "apply policy {apply_policy} admits the report"
                );
            }
            if approval_policy {
                store.approve_worker(worker(), T0).await.unwrap();
            }

            let (record, _, _) = store
                .create_task("combination".into(), definition("shell"), T0)
                .await
                .unwrap();
            let claimed = store
                .claim_task(worker(), claim(record.id.as_str(), epoch), T0)
                .await
                .unwrap()
                .0;
            assert_eq!(claimed.task.status, TaskStatus::Dispatched);
        }
    }
}

/// Turning Apply off does not admit an already-pending report. Only a *changed* report —
/// the next one — is applied automatically, so no Admin decision is silently replaced.
#[tokio::test]
async fn a_repeated_report_never_retroactively_applies_a_pending_decision() {
    let store = HubStore::open_in_memory().unwrap();
    // Start with manual Apply so the first report becomes pending at revision 1.
    set_policies(&store, false, true).await;
    pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    let pending = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(pending.reported_revision, Some(1));
    assert_eq!(pending.applied_revision, None);

    // Operator turns Apply off and the Worker reconnects reporting the same schedule.
    set_policies(&store, false, false).await;
    let epoch2 = connect(&store, inventory(9, "shell", "exec-a")).await;
    assert!(epoch2 > epoch);
    let unchanged = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(unchanged.reported_revision, Some(1), "the revision is kept");
    assert_eq!(
        unchanged.applied_revision, None,
        "an unchanged report never applies a pending decision by itself"
    );

    // A genuinely changed report is admitted in its own transaction.
    store
        .update_worker_inventory(worker(), epoch2, inventory(10, "gpu", "exec-b"), T0)
        .await
        .unwrap();
    let changed = store.get_worker(worker(), HashMap::new()).await.unwrap();
    assert_eq!(changed.reported_revision, Some(2));
    assert_eq!(changed.applied_revision, Some(2));
}

/// A changed report and an Apply each commit an event, so an SSE client learns about the
/// new applied configuration without polling.
#[tokio::test]
async fn a_report_and_an_apply_each_commit_an_event() {
    let store = HubStore::open_in_memory().unwrap();
    set_policies(&store, false, true).await;
    pair(&store).await;
    let (_epoch, events) = store
        .open_worker_connection(worker(), inventory(1, "shell", "exec-a"), T0)
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "the first report is announced");
    assert_eq!(events[0].kind, "worker.inventory_reported");
    assert_eq!(events[0].payload["workerId"], WORKER);
    assert_eq!(events[0].payload["revision"], 1);

    let (_applied, event) = store.apply_worker_config(worker(), 1, T0).await.unwrap();
    let event = event.expect("an Apply that changes the snapshot emits an event");
    assert_eq!(event.kind, "worker.config_applied");
    assert_eq!(event.payload["revision"], 1);

    // Applying the same revision again changes nothing, so it emits nothing either.
    let (_, again) = store.apply_worker_config(worker(), 1, T0).await.unwrap();
    assert!(again.is_none());
}
