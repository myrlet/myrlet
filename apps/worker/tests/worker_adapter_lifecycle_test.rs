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

/// Pull frames until one of `kind` arrives, or `limit` elapses.
async fn wait_for_frame(
    hub: &mut support::StubHub,
    kind: &str,
    limit: Duration,
) -> Option<serde_json::Value> {
    let deadline = tokio::time::Instant::now() + limit;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, hub.next_frame()).await {
            Ok(frame) if frame["kind"] == kind => return Some(frame),
            Ok(_) => {}
            Err(_) => break,
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

/// A cancellation that ends handling releases the pinned adapter once its Finish is
/// accepted — no `CancelState` value keeps a live execution behind: `Confirmed` ends the
/// group, `Failed`/`Unsupported` are recorded and re-sent, `Uncertain` ends local handling
/// without proving termination. A duplicate cancel re-sends the recorded answer and never
/// calls the adapter again, so the reference is never needed for a second cancellation.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancellation_ack_releases_the_pinned_adapter_on_its_own() {
    // The Hub fails every `/finish`, so the only path that can release the pinned adapter
    // is the acknowledgement of the cancellation report. This proves the cancel-ACK path
    // independently of the Finish-ACK path.
    let mut hub = support::start_with_failing_finish(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-long", "sleep 30")]),
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
    hub.send_frame(support::wakeup_frame("task_cancel"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    wait_for_delivery(&home, "delivery_task_cancel").await;
    assert!(
        eventually(|| probe.pinned_adapter_count() == 1, Duration::from_secs(5)).await,
        "the live Delivery pins its adapter",
    );
    let record_path = home
        .worker_state_dir()
        .join("deliveries/delivery_task_cancel.json");
    let read_record = || {
        std::fs::read_to_string(&record_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    };

    // The Hub asks to cancel; the Worker reports the outcome. Finish is refused by the
    // stub, so nothing here can be acknowledged through the Finish path.
    hub.send_frame(hub.cancel_frame("task_cancel", "delivery_task_cancel"));
    let cancelled = wait_for_frame(&mut hub, "task.cancelled", Duration::from_secs(20))
        .await
        .expect("the Worker reports the cancellation answer");
    assert_eq!(cancelled["payload"]["state"], "confirmed", "{cancelled}");

    // The cancellation report is recorded and the adapter is still pinned: it is only
    // released once the Hub acknowledges the report.
    let recorded = read_record().expect("record on disk");
    assert!(
        recorded["cancelReport"].is_array(),
        "cancel report is persisted: {recorded}"
    );
    assert_eq!(recorded["cancelAcked"], false);
    assert_eq!(
        probe.pinned_adapter_count(),
        1,
        "an unacknowledged cancellation keeps the adapter",
    );

    // A duplicate request re-sends the recorded answer and does not cancel again.
    hub.send_frame(hub.cancel_frame("task_cancel", "delivery_task_cancel"));
    let repeat = wait_for_frame(&mut hub, "task.cancelled", Duration::from_secs(10))
        .await
        .expect("the duplicate cancel is answered from the record");
    assert_eq!(repeat["payload"]["state"], "confirmed", "{repeat}");
    // No second Finish was produced: handling already ended on the first cancellation.
    assert!(
        tokio::time::timeout(Duration::from_millis(400), hub.next_report())
            .await
            .map(|report| !matches!(report, Report::Finish { .. }))
            .unwrap_or(true),
        "a duplicate cancellation never re-runs the adapter or re-finishes",
    );

    // Acknowledge the cancellation report: the adapter is released through this path,
    // with Finish still failing the whole time.
    hub.send_frame(support::cancelled_ack("delivery_task_cancel", true));
    assert!(
        eventually(
            || read_record().is_some_and(|r| r["cancelAcked"] == true),
            Duration::from_secs(5),
        )
        .await,
        "the cancellation acknowledgement is persisted",
    );
    assert!(
        eventually(|| probe.pinned_adapter_count() == 0, Duration::from_secs(5)).await,
        "an acknowledged cancellation releases the pinned adapter",
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_claim_drops_its_pending_adapter() {
    let mut hub = support::start_with_refused_claim(None).await;
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

    hub.send_frame(support::wakeup_frame("task_refused"));
    let claim = hub.next_report().await;
    assert!(
        matches!(&claim, Report::Claim { task_id, .. } if task_id == "task_refused"),
        "the claim is attempted: {claim:?}",
    );

    // The Hub refused; no Delivery registered, and nothing stays pinned.
    let registered = home
        .worker_state_dir()
        .join("deliveries/delivery_task_refused.json");
    assert!(
        eventually(
            || probe.pending_adapter_count() == 0 && probe.pinned_adapter_count() == 0,
            Duration::from_secs(5),
        )
        .await,
        "a refused claim leaves no adapter pinned",
    );
    assert!(
        !registered.exists(),
        "a refused claim registers no Delivery"
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}
