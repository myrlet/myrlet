//! Configuration apply: a changed report waits for review, applying at the reviewed
//! revision teaches the Hub the new inventory, and errors leave the applied snapshot alone.

use std::collections::HashMap;

use myrlet_store::{HubStore, StoreError};

use super::helpers::{approve_and_apply, claim, connect, definition, inventory, pair, worker, T0};

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
    assert_eq!(detail.reported_revision, Some(2));
    assert_eq!(detail.applied_revision, Some(1));
    assert_eq!(
        detail.reported_config.as_ref().unwrap().slots[0].executors[0]
            .id
            .as_str(),
        "exec-b"
    );
    assert_eq!(
        detail.applied_config.as_ref().unwrap().slots[0].executors[0]
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
    assert_eq!(
        store.apply_worker_config(worker(), 2, T0).await.unwrap().0,
        2
    );
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
        .get_worker_snapshot(worker(), HashMap::new())
        .await
        .unwrap()
        .applied_revision;
    assert_eq!(applied, Some(1));

    store.revoke_worker(worker(), T0).await.unwrap();
    let revoked = store.apply_worker_config(worker(), 2, T0).await;
    assert!(matches!(revoked, Err(StoreError::InvalidTransition)));
}

#[tokio::test]
async fn application_requires_a_pending_report() {
    let store = HubStore::open_in_memory().unwrap();
    pair(&store).await;
    store.approve_worker(worker(), T0).await.unwrap();
    let none = store.apply_worker_config(worker(), 1, T0).await;
    assert!(matches!(none, Err(StoreError::NoPendingConfig)));
}
