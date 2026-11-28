//! Worker approval: a pending Worker cannot claim, approval alone is not enough, and a
//! revoked or re-paired identity starts a fresh decision.

use std::collections::HashMap;

use myrlet_protocol::{SecurityTrustSettings, TaskStatus, WorkerApprovalStatus};
use myrlet_store::{HubStore, StoreError};

use super::helpers::{approve_and_apply, claim, connect, definition, inventory, pair, worker, T0};

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
    // Before any report there is nothing to apply, so configuration is not pending.
    assert!(!snapshot[0].configuration_pending);

    // A pending Worker may still connect, heartbeat, and read; it just cannot claim.
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    // Its first report now awaits Apply.
    let snapshot = store.list_workers(HashMap::new()).await.unwrap();
    assert!(snapshot[0].configuration_pending);
    assert_eq!(
        store
            .authenticate_worker(credential)
            .await
            .unwrap()
            .worker_id,
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
async fn a_pending_worker_can_be_revoked_and_a_repaired_id_starts_fresh() {
    let store = HubStore::open_in_memory().unwrap();
    let (_id, credential) = pair(&store).await;
    let epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    store.revoke_worker(worker(), T0).await.unwrap();
    assert_eq!(
        store
            .get_worker_snapshot(worker(), HashMap::new())
            .await
            .unwrap()
            .summary
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
    let worker_row = store
        .get_worker_snapshot(worker(), HashMap::new())
        .await
        .unwrap();
    assert_eq!(worker_row.summary.approval, WorkerApprovalStatus::Pending);
    assert!(worker_row.applied_revision.is_none());
    assert!(worker_row.reported_revision.is_none());
    assert_eq!(
        store
            .authenticate_worker(credential)
            .await
            .unwrap()
            .worker_id,
        worker()
    );

    // The retained high-water mark means a pre-repair revision cannot be applied.
    let next_epoch = connect(&store, inventory(1, "shell", "exec-a")).await;
    assert!(next_epoch > epoch);
    let repaired_revision = store
        .get_worker_snapshot(worker(), HashMap::new())
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
    let row = store
        .get_worker_snapshot(worker(), HashMap::new())
        .await
        .unwrap();
    assert_eq!(row.summary.approval, WorkerApprovalStatus::Approved);
    assert_eq!(row.reported_revision, Some(1));
    assert_eq!(row.applied_revision, Some(1));
}
