//! Recovery semantics: a lost claim acknowledgement is replayed without a second
//! submission, and an ambiguous Result write is retried at the new epoch.

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

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_claim_acknowledgement_is_recovered_without_a_second_submission() {
    let mut hub = support::start_with_lost_claim_reply(None).await;
    let (directory, home) = temp_home();
    let counter = directory.path().join("runs.txt");
    let script = format!("echo run >> {}", counter.display());
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-count", script.as_str())]),
    )
    .expect("write config");
    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let runtime =
        WorkerRuntime::new(home.clone(), config, support::credentials()).expect("runtime");
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_lost"));

    // The stub Hub commits the Delivery but answers with a server error, so the Worker
    // must treat the claim outcome as unknown and replay the same claim.
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    hub.expect_authorized().await;
    // The stub may still have the last heartbeat from the former session queued.
    while hub.next_frame().await["kind"] != "worker.hello" {}
    let mut welcome = support::welcome_frame();
    welcome["connectionEpoch"] = serde_json::json!(support::EPOCH + 1);
    welcome["payload"]["connectionEpoch"] = serde_json::json!(support::EPOCH + 1);
    hub.send_frame(welcome);
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    let replay = hub.next_report().await;
    assert!(
        matches!(&replay, Report::Claim { task_id, connection_epoch } if task_id == "task_lost" && *connection_epoch == support::EPOCH + 1),
        "the lost claim must be replayed: {replay:?}"
    );
    // The replayed reply is the committed Delivery, and the Handler runs exactly once.
    let registered = home
        .worker_state_dir()
        .join("deliveries/delivery_task_lost.json");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !registered.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(registered.exists(), "the recovered Delivery is registered");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let runs = std::fs::read_to_string(&counter).unwrap_or_default();
    assert_eq!(runs.lines().count(), 1, "no duplicate execution: {runs:?}");
    let mut claims = vec![replay];
    while let Ok(report) = tokio::time::timeout(Duration::from_millis(100), hub.next_report()).await
    {
        claims.push(report);
    }
    assert_eq!(
        claims
            .iter()
            .filter(|report| matches!(report, Report::Claim { .. }))
            .count(),
        1,
        "no further claim retries once acknowledged: {claims:?}"
    );
    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ambiguous_timed_out_result_write_retries_at_the_new_epoch() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut hub = support::start_with_result_timeout(gate.clone()).await;
    let (_directory, home) = temp_home();
    let path = home.worker_results_dir().join("delivery_task_one.md");
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-quiet", "sleep 5")]),
    )
    .unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home, config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });
    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));

    // Seed the acknowledged digest with B before an ambiguous write of A.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap()
        .write_all(b"B")
        .unwrap();
    assert!(
        matches!(hub.next_report().await, Report::Result{epoch:support::EPOCH,text:Some(t),..} if t=="B")
    );
    std::fs::write(&path, b"A").unwrap();
    assert!(
        matches!(hub.next_report().await, Report::Result{epoch:support::EPOCH,text:Some(t),..} if t=="A")
    );

    // Revert to the previously confirmed bytes while A is still outstanding.
    // Release a 504 without touching WSS. The Worker must initiate reconnect.
    std::fs::write(&path, b"B").unwrap();
    gate.notify_one();
    hub.expect_authorized().await;
    while hub.next_frame().await["kind"] != "worker.hello" {}
    let mut welcome = support::welcome_frame();
    welcome["connectionEpoch"] = serde_json::json!(support::EPOCH + 1);
    welcome["payload"]["connectionEpoch"] = serde_json::json!(support::EPOCH + 1);
    hub.send_frame(welcome);
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");

    // The reconciler rereads the local file even though B matches the old digest.
    let retry = hub.next_report().await;
    assert!(
        matches!(
            &retry,
            Report::Result { delivery_id, epoch, text: Some(text) }
                if delivery_id == "delivery_task_one"
                    && text == "B"
                    && *epoch == support::EPOCH + 1
        ),
        "ambiguous write must be retried at EPOCH+1: {retry:?}"
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
async fn result_sync_waits_for_background_write_and_reads_latest_content() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let gate = std::sync::Arc::new(tokio::sync::Notify::new());
    let mut hub = support::start_with_result_gate(None, Some(gate.clone())).await;
    let (_directory, home) = temp_home();
    let path = home.worker_results_dir().join("delivery_task_one.md");
    let socket = home.worker_control_socket();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-quiet", "sleep 5")]),
    )
    .unwrap();
    let config = WorkerConfig::load(&home.worker_config()).unwrap();
    let runtime = WorkerRuntime::new(home, config, support::credentials()).unwrap();
    let (shutdown, rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(rx).await });
    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap()
        .write_all(b"A")
        .unwrap();
    assert!(matches!(hub.next_report().await, Report::Result{text:Some(t),..} if t=="A"));
    // The fake Hub is still handling A. Overlap is directly observable if it receives B now.
    std::fs::write(&path, b"B").unwrap();
    let manual = tokio::task::spawn_blocking(move || {
        myrlet_protocol::send(
            &socket,
            &myrlet_protocol::LocalCommand::ResultSync {
                delivery_id: "delivery_task_one".into(),
            },
        )
        .unwrap()
    });
    let early = tokio::time::timeout(Duration::from_secs(1), hub.next_report()).await;
    gate.notify_one();
    let reply = tokio::time::timeout(Duration::from_secs(3), manual)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.hub_confirmed, Some(true));
    if early.is_err() {
        assert!(matches!(hub.next_report().await, Report::Result{text:Some(t),..} if t=="B"));
    }
    shutdown.send(true).unwrap();
    let mut worker = worker;
    if tokio::time::timeout(Duration::from_secs(1), &mut worker)
        .await
        .is_err()
    {
        worker.abort();
        let _ = worker.await;
    }
    assert!(
        early.is_err(),
        "received a second request before releasing A: {early:?}"
    );
}
