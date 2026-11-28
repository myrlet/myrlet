//! `worker.yaml` hot reload: activation, claim pausing, adapter pinning, and a claim
//! whose reply crosses a reload.

#![cfg(unix)]

mod support;

use std::time::Duration;

use myrlet_protocol::paths::{ensure_private_dir, MyrletHome};
use myrlet_worker_daemon::credentials::save_credentials;
use myrlet_worker_daemon::runtime::WorkerRuntime;
use myrlet_worker_daemon::WorkerConfig;
use support::Report;
use tokio::sync::watch;

fn temp_home() -> (tempfile::TempDir, MyrletHome) {
    let directory = tempfile::tempdir().expect("temp dir");
    let home = MyrletHome::resolve(Some(directory.path())).expect("MYRLET_HOME");
    ensure_private_dir(&home.worker_dir()).expect("worker dir");
    (directory, home)
}

/// Read frames until one of `kind` arrives, skipping heartbeats that share the socket.
async fn next_frame_of(hub: &mut support::StubHub, kind: &str) -> serde_json::Value {
    loop {
        let frame = hub.next_frame().await;
        assert_ne!(frame["kind"], "worker.bye", "socket closed before {kind}");
        if frame["kind"] == kind {
            return frame;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_valid_worker_yaml_reload_pauses_claims_until_the_hub_acknowledges() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-quiet", "sleep 5")]),
    )
    .unwrap();
    save_credentials(&home.worker_credentials(), &support::credentials()).unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home.clone(), config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    let hello = hub.next_frame().await;
    assert_eq!(hello["kind"], "worker.hello");
    assert_eq!(hello["payload"]["inventory"]["revision"], 1);
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    // A valid scheduling change is activated locally and reported as `worker.inventory`.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-new", "sleep 5")]),
    )
    .unwrap();
    let inventory = next_frame_of(&mut hub, "worker.inventory").await;
    let revision = inventory["payload"]["revision"]
        .as_u64()
        .expect("inventory revision");
    assert_eq!(revision, 2, "a scheduling change bumps the local revision");

    // Until the Hub acknowledges it, a hinted Task is not claimed.
    hub.send_frame(support::wakeup_frame("task_paused"));
    let paused = tokio::time::timeout(Duration::from_millis(500), hub.next_report()).await;
    assert!(paused.is_err(), "claims stay paused before acknowledgement");

    hub.send_frame(support::inventory_ack(revision, true));
    hub.send_frame(support::wakeup_frame("task_resumed"));
    let claim = hub.next_report().await;
    assert!(
        matches!(&claim, Report::Claim { task_id, .. } if task_id == "task_resumed"),
        "claims resume after the ack: {claim:?}"
    );

    shutdown.send(true).unwrap();
    let mut worker = worker;
    if tokio::time::timeout(Duration::from_secs(1), &mut worker)
        .await
        .is_err()
    {
        worker.abort();
        let _ = worker.await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_invalid_worker_yaml_edit_keeps_the_last_valid_configuration() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-keep", "sleep 5")]),
    )
    .unwrap();
    save_credentials(&home.worker_credentials(), &support::credentials()).unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home.clone(), config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    // An invalid edit is diagnosed locally; no inventory frame is emitted and the last
    // valid configuration keeps scheduling.
    std::fs::write(home.worker_config(), "hubUrl: ftp://nope\n").unwrap();
    hub.send_frame(support::wakeup_frame("task_invalid"));
    let claim = hub.next_report().await;
    assert!(
        matches!(&claim, Report::Claim { task_id, .. } if task_id == "task_invalid"),
        "the previous valid configuration still claims: {claim:?}"
    );

    shutdown.send(true).unwrap();
    let mut worker = worker;
    if tokio::time::timeout(Duration::from_secs(1), &mut worker)
        .await
        .is_err()
    {
        worker.abort();
        let _ = worker.await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_committed_delivery_keeps_its_adapter_across_a_config_reload() {
    let mut hub = support::start(None).await;
    let (directory, home) = temp_home();
    let counter = directory.path().join("runs.txt");
    let script = format!("echo run >> {}; sleep 5", counter.display());
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-old", script.as_str())]),
    )
    .unwrap();
    save_credentials(&home.worker_credentials(), &support::credentials()).unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home.clone(), config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_lifecycle"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    let registered = home
        .worker_state_dir()
        .join("deliveries/delivery_task_lifecycle.json");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !registered.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(registered.exists(), "the Delivery is registered");

    // Reload to a configuration that no longer declares the Delivery's Executor.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-new", "sleep 5")]),
    )
    .unwrap();
    let inventory = next_frame_of(&mut hub, "worker.inventory").await;
    let revision = inventory["payload"]["revision"]
        .as_u64()
        .expect("inventory revision");
    hub.send_frame(support::inventory_ack(revision, true));

    // Cancellation still reaches the pinned adapter, so the running execution stops.
    hub.send_frame(support::hub_frame(
        "task.cancel",
        serde_json::json!({
            "taskId": "task_lifecycle",
            "deliveryId": "delivery_task_lifecycle",
            "reason": "operator",
        }),
    ));
    let cancelled = next_frame_of(&mut hub, "task.cancelled").await;
    assert_eq!(
        cancelled["payload"]["deliveryId"],
        "delivery_task_lifecycle"
    );
    assert_eq!(
        cancelled["payload"]["state"], "confirmed",
        "the pinned adapter cancelled the running process: {cancelled}"
    );

    shutdown.send(true).unwrap();
    let mut worker = worker;
    if tokio::time::timeout(Duration::from_secs(1), &mut worker)
        .await
        .is_err()
    {
        worker.abort();
        let _ = worker.await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_claim_response_across_a_reload_uses_the_claimed_adapter() {
    let mut hub = support::start_with_held_claim_for(None, "exec-old").await;
    let (directory, home) = temp_home();
    let old_runs = directory.path().join("old-runs.txt");
    let new_runs = directory.path().join("new-runs.txt");
    let old_script = format!("echo old >> {}; sleep 5", old_runs.display());
    let new_script = format!("echo new >> {}; sleep 5", new_runs.display());
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-old", &old_script)]),
    )
    .unwrap();
    save_credentials(&home.worker_credentials(), &support::credentials()).unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home.clone(), config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    hub.send_frame(support::wakeup_frame("task_held"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));

    // The Hub committed the claim but its response is delayed. Replace the Slot and
    // Executor before admitting it; the Delivery must still start the claimed adapter.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-new", &new_script)]),
    )
    .unwrap();
    // The session is awaiting the claim response, but the independent watcher still
    // activates the replacement after its one-second poll.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    hub.release_claim();

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !old_runs.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        old_runs.exists(),
        "the claimed adapter must start the Delivery"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !new_runs.exists(),
        "a delayed claim must not be re-dispatched through the reloaded adapter"
    );

    shutdown.send(true).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_inventory_ack_cannot_resume_claims_for_a_newer_report() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-one", "sleep 5")]),
    )
    .unwrap();
    save_credentials(&home.worker_credentials(), &support::credentials()).unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home.clone(), config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    // Two scheduling changes in a row: revision 2 is reported, then revision 3 before the
    // Hub answers the first report.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-two", "sleep 5")]),
    )
    .unwrap();
    let two = next_frame_of(&mut hub, "worker.inventory").await;
    assert_eq!(two["payload"]["revision"], 2);
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-three", "sleep 5")]),
    )
    .unwrap();
    let three = next_frame_of(&mut hub, "worker.inventory").await;
    assert_eq!(three["payload"]["revision"], 3);

    // The late acknowledgement for revision 2 is stale: claims stay paused.
    hub.send_frame(support::inventory_ack(2, true));
    hub.send_frame(support::wakeup_frame("task_after_stale_ack"));
    let paused = tokio::time::timeout(Duration::from_millis(600), hub.next_report()).await;
    assert!(
        paused.is_err(),
        "an older ack must not resume claims for a newer pending revision"
    );

    // Only the acknowledgement for the current revision resumes them.
    hub.send_frame(support::inventory_ack(3, true));
    hub.send_frame(support::wakeup_frame("task_after_current_ack"));
    let claim = hub.next_report().await;
    assert!(
        matches!(&claim, Report::Claim { task_id, .. } if task_id == "task_after_current_ack"),
        "claims resume after the current ack: {claim:?}"
    );

    shutdown.send(true).unwrap();
    let mut worker = worker;
    if tokio::time::timeout(Duration::from_secs(1), &mut worker)
        .await
        .is_err()
    {
        worker.abort();
        let _ = worker.await;
    }
}
