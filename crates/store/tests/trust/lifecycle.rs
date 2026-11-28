//! Delivery lifecycle: a committed claim survives a later configuration change, reports
//! and applies serialize without regressing the applied revision.

use std::collections::HashMap;

use myrlet_protocol::{ProgressReportRequest, ResultReportRequest};
use myrlet_store::{HubStore, StoreError};

use super::helpers::{approve_and_apply, claim, connect, definition, inventory, pair, worker, T0};

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
            Ok((applied, _event)) => assert_eq!(applied, reported),
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
