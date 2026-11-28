//! Delivery adapter lifecycle: a committed Delivery keeps the adapter that admitted it
//! until its handling ends, and a resolved claim never leaves adapters pinned behind.

#![cfg(unix)]

mod support;

use std::time::Duration;

use myrlet_protocol::paths::{ensure_private_dir, MyrletHome};
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

/// Poll `probe` until it is true or `limit` elapses.
async fn eventually(mut probe: impl FnMut() -> bool, limit: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        if probe() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    probe()
}

async fn wait_for_delivery(home: &MyrletHome, delivery_id: &str) {
    let record = home
        .worker_state_dir()
        .join(format!("deliveries/{delivery_id}.json"));
    let registered = eventually(|| record.exists(), Duration::from_secs(5)).await;
    assert!(registered, "the Delivery is registered on disk");
}

/// Pull reports until `predicate` matches one, or `limit` elapses.
async fn wait_for_report(
    hub: &mut support::StubHub,
    limit: Duration,
    mut predicate: impl FnMut(&Report) -> bool,
) -> Option<Report> {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, hub.next_report()).await {
            Ok(report) if predicate(&report) => return Some(report),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_delivery_releases_its_pinned_adapter() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    // The executor exits shortly after start, so the adapter reports end of handling.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-quick", "sleep 1.2")]),
    )
    .expect("write config");
    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let runtime =
        WorkerRuntime::new(home.clone(), config, support::credentials()).expect("runtime");
    // A second handle into the same runtime lets the test observe the adapter registry.
    let probe = runtime.clone();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_release"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    wait_for_delivery(&home, "delivery_task_release").await;

    // The adapter is pinned while the Delivery is live.
    assert!(
        eventually(|| probe.pinned_adapter_count() == 1, Duration::from_secs(5)).await,
        "a committed Delivery pins its adapter",
    );

    // When handling ends the exit handler posts one Finish; once accepted, the pinned
    // adapter reference is released so completed Deliveries cannot grow the registry.
    let finish = wait_for_report(&mut hub, Duration::from_secs(8), |report| {
        matches!(report, Report::Finish { .. })
    })
    .await;
    assert!(finish.is_some(), "the delivery finished");
    assert!(
        eventually(|| probe.pinned_adapter_count() == 0, Duration::from_secs(5)).await,
        "a finished Delivery releases its pinned adapter",
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resolved_claim_leaves_no_pending_adapter() {
    // Every claim attempt, whether it commits a Delivery, loses the race, or is refused,
    // resolves in `retry_pending_claim`. The adapters pinned for a request must be dropped
    // with it so the pending registry never accumulates stale references.
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-idle", "sleep 5")]),
    )
    .expect("write config");
    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let runtime =
        WorkerRuntime::new(home.clone(), config, support::credentials()).expect("runtime");
    let probe = runtime.clone();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    // One Wakeup claims the Slot; the pinned adapters travel with the committed Delivery.
    hub.send_frame(support::wakeup_frame("task_busy"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    wait_for_delivery(&home, "delivery_task_busy").await;
    assert!(
        eventually(|| probe.pinned_adapter_count() == 1, Duration::from_secs(5)).await,
        "the running Delivery pins its adapter",
    );

    // Every claim attempt resolved so far must have cleared its pending snapshot: the one
    // live Delivery holds a committed adapter, nothing is left waiting on a refusal.
    assert_eq!(
        probe.pending_adapter_count(),
        0,
        "a resolved claim clears its pending adapter snapshot",
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}
