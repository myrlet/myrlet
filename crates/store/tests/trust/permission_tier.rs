//! Server-controlled Worker permission tier: pairing defaults, Admin-only changes,
//! rotation/re-pair semantics, and the in-place schema step.

use std::collections::HashMap;

use myrlet_protocol::PermissionTier;
use myrlet_store::{HubStore, StoreError};

use super::helpers::{pair, T0};

#[tokio::test]
async fn permission_tier_lifecycle_is_server_controlled() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, credential) = pair(&store).await;

    // A new pairing defaults to the least-privileged tier.
    let authenticated = store.authenticate_worker(credential.clone()).await.unwrap();
    assert_eq!(authenticated.worker_id, worker_id);
    assert_eq!(authenticated.permission_tier, PermissionTier::Worker);

    // Only an explicit tier change elevates the same credential.
    let (promoted, event) = store
        .set_worker_permission_tier(worker_id.clone(), PermissionTier::Owner, T0)
        .await
        .unwrap();
    assert_eq!(event.kind, "worker.permission_tier");
    assert_eq!(promoted.permission_tier, PermissionTier::Owner);
    assert_eq!(
        store
            .authenticate_worker(credential.clone())
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Owner
    );

    // Rotation preserves the tier and fences the old credential.
    let rotated = store
        .rotate_worker_credential("hub_1".into(), worker_id.clone(), T0)
        .await
        .unwrap()
        .0;
    assert!(matches!(
        store.authenticate_worker(credential).await,
        Err(StoreError::Unauthorized)
    ));
    assert_eq!(
        store
            .authenticate_worker(rotated.credential.clone())
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Owner
    );
    assert_eq!(
        store
            .get_worker(worker_id.clone(), HashMap::new())
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Owner
    );

    // Demotion is also explicit and effective immediately.
    store
        .set_worker_permission_tier(worker_id.clone(), PermissionTier::Worker, T0)
        .await
        .unwrap();
    assert_eq!(
        store
            .authenticate_worker(rotated.credential)
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Worker
    );
}

#[tokio::test]
async fn repairing_a_revoked_worker_resets_the_tier() {
    let store = HubStore::open_in_memory().unwrap();
    let (worker_id, _credential) = pair(&store).await;
    store
        .set_worker_permission_tier(worker_id.clone(), PermissionTier::Owner, T0)
        .await
        .unwrap();
    store.revoke_worker(worker_id.clone(), T0).await.unwrap();

    // A revoked Worker cannot have its tier changed, mirroring its other trust operations.
    let denied = store
        .set_worker_permission_tier(worker_id.clone(), PermissionTier::Owner, T0)
        .await;
    assert!(matches!(denied, Err(StoreError::InvalidTransition)));

    let (repaired, credential) = pair(&store).await;
    assert_eq!(repaired, worker_id);
    // Re-pairing starts a fresh trust decision, including the least-privileged tier.
    assert_eq!(
        store
            .get_worker(worker_id.clone(), HashMap::new())
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Worker
    );
    assert_eq!(
        store
            .authenticate_worker(credential)
            .await
            .unwrap()
            .permission_tier,
        PermissionTier::Worker
    );
}

#[test]
fn an_existing_database_gains_the_worker_tier_column_safely() {
    // Simulate a pre-three-tier database: a workers table without `permission_tier`.
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE workers ( \
                 worker_id TEXT PRIMARY KEY NOT NULL, \
                 token_sha256 TEXT NOT NULL, \
                 revoked INTEGER NOT NULL DEFAULT 0, \
                 approval TEXT NOT NULL DEFAULT 'pending', \
                 connection_epoch INTEGER NOT NULL DEFAULT 0, \
                 inventory_json TEXT, reported_revision INTEGER, \
                 reported_config_json TEXT, applied_revision INTEGER, \
                 applied_config_json TEXT, \
                 revision_high_water INTEGER NOT NULL DEFAULT 0, \
                 last_seen_at_ms INTEGER, created_at_ms INTEGER NOT NULL, \
                 rotated_at_ms INTEGER); \
             INSERT INTO workers (worker_id, token_sha256, approval, created_at_ms) \
             VALUES ('wkr_000000000000', 'deadbeef', 'approved', 1);",
        )
        .unwrap();
    myrlet_store::schema::initialize(&connection).unwrap();
    let tier: String = connection
        .query_row(
            "SELECT permission_tier FROM workers WHERE worker_id = 'wkr_000000000000'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tier, "worker", "existing rows receive the safe default");
}
