//! One live WSS session: wakeup handling, claims, cancellation relaying, local Slot
//! availability, and the execution-local control socket.

#![cfg(unix)]

mod support;

use std::time::Duration;

use myrlet_protocol::paths::{ensure_private_dir, MyrletHome};
use myrlet_worker_daemon::credentials::{load_credentials, save_credentials};
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
async fn runtime_session_claims_wakes_and_reports_progress_finish_and_result() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    let written = home.worker_progress_dir().join("delivery_task_one.json");
    let result_file = home.worker_results_dir().join("delivery_task_one.md");
    let script = format!(
        "umask 077; printf '{{\"text\":\"step one\"}}' > {}; printf '# Result\\n\\nFindings.' > {}; sleep 1.5",
        written.display(),
        result_file.display()
    );
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-progress", script.as_str())]),
    )
    .expect("write config");
    save_credentials(&home.worker_credentials(), &support::credentials()).expect("credentials");

    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let credentials = load_credentials(&home.worker_credentials()).expect("credentials");
    let runtime = WorkerRuntime::new(home, config, credentials).expect("runtime");
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(shutdown_rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));

    let mut reports = Vec::new();
    while reports.len() < 4 {
        reports.push(hub.next_report().await);
    }
    let claims = reports
        .iter()
        .filter(|report| matches!(report, Report::Claim { .. }))
        .count();
    assert_eq!(
        claims, 1,
        "one wakeup produced one claim attempt: {reports:?}"
    );
    let relayed = reports.iter().any(|report| {
        matches!(
            report,
            Report::Progress { delivery_id, text, .. }
                if delivery_id == "delivery_task_one" && text == "step one"
        )
    });
    assert!(relayed, "relayed progress: {reports:?}");
    let result = reports.iter().any(|report| {
        matches!(
            report,
            Report::Result { delivery_id, text, .. }
                if delivery_id == "delivery_task_one"
                    && text.as_deref() == Some("# Result\n\nFindings.")
        )
    });
    assert!(result, "relayed result: {reports:?}");
    let finishes = reports
        .iter()
        .filter(|report| matches!(report, Report::Finish { .. }))
        .count();
    assert_eq!(finishes, 1, "the delivery finished: {reports:?}");
    assert!(written.exists(), "the harness-written Progress file exists");
    assert!(
        result_file.exists(),
        "the harness-written result file exists"
    );

    shutdown.send(true).expect("signal shutdown");
    let result = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("worker exits after shutdown")
        .expect("worker task");
    assert!(result.is_ok(), "clean shutdown");
}

/// A cancel whose cleanup outlives the heartbeat interval must keep the Hub lease alive
/// while it runs, then report the confirmed answer on that same connection.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_cancel_keeps_heartbeating_until_the_group_is_gone() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    // The leader ignores TERM, so cleanup falls through to the SIGKILL phase.
    std::fs::write(
        home.worker_config(),
        support::process_yaml(
            &hub.url,
            &[(
                "exec-stubborn",
                "trap '' TERM; while :; do sleep 0.05; done",
            )],
        ),
    )
    .expect("write config");
    save_credentials(&home.worker_credentials(), &support::credentials()).expect("credentials");

    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let credentials = load_credentials(&home.worker_credentials()).expect("credentials");
    let runtime = WorkerRuntime::new(home, config, credentials).expect("runtime");
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(shutdown_rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    // Heartbeat every 100 ms with a short lease: a missing heartbeat during cleanup would
    // expire the lease before the cancel answer could be sent.
    hub.send_frame(support::welcome_frame_with(100, 1_000));
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    tokio::time::sleep(Duration::from_millis(300)).await;

    hub.send_frame(hub.cancel_frame("task_one", "delivery_task_one"));
    let mut heartbeats = 0;
    let answer = loop {
        let frame = hub.next_frame().await;
        match frame["kind"].as_str() {
            Some("worker.heartbeat") => heartbeats += 1,
            Some("task.cancelled") => break frame,
            Some(_) => {}
            None => panic!("frame without a kind: {frame}"),
        }
    };
    assert_eq!(
        answer["payload"]["state"], "confirmed",
        "the group was killed and the answer reported: {answer}"
    );
    assert!(
        heartbeats >= 1,
        "the lease must be refreshed during cleanup, saw {heartbeats} heartbeats"
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wakeup_with_no_free_slot_claims_nothing() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-slow", "sleep 3")]),
    )
    .expect("write config");
    save_credentials(&home.worker_credentials(), &support::credentials()).expect("credentials");

    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let credentials = load_credentials(&home.worker_credentials()).expect("credentials");
    let runtime = WorkerRuntime::new(home, config, credentials).expect("runtime");
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(shutdown_rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    // The one Slot is now busy, so a second hint is declined locally, not at the Hub.
    hub.send_frame(support::wakeup_frame("task_two"));
    tokio::time::sleep(Duration::from_millis(500)).await;

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn local_control_channel_answers_done_and_result_commands_by_delivery_id() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    let result_file = home.worker_results_dir().join("delivery_task_one.md");
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-quiet", "sleep 5")]),
    )
    .expect("write config");
    save_credentials(&home.worker_credentials(), &support::credentials()).expect("credentials");
    let socket_path = home.worker_control_socket();

    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    let credentials = load_credentials(&home.worker_credentials()).expect("credentials");
    let runtime = WorkerRuntime::new(home, config, credentials).expect("runtime");
    let (shutdown, shutdown_rx) = watch::channel(false);
    let worker = tokio::spawn(async move { runtime.run(shutdown_rx).await });

    hub.expect_authorized().await;
    assert_eq!(hub.next_frame().await["kind"], "worker.hello");
    hub.send_frame(support::welcome_frame());
    assert_eq!(hub.next_frame().await["kind"], "worker.resync");
    hub.send_frame(support::wakeup_frame("task_one"));
    assert!(matches!(hub.next_report().await, Report::Claim { .. }));
    // Give the runtime a moment to register and submit the claimed Delivery.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // A command for a Delivery this Worker never registered is rejected locally.
    let unknown = myrlet_protocol::send(
        &socket_path,
        &myrlet_protocol::LocalCommand::Done {
            delivery_id: "delivery_missing".into(),
        },
    )
    .expect("control socket reachable");
    assert!(!unknown.local_receipt);
    assert!(unknown.outcome.contains("no locally recorded Delivery"));

    // A real Delivery accepts `done`: local receipt plus a Hub-confirmed result sync.
    let done = myrlet_protocol::send(
        &socket_path,
        &myrlet_protocol::LocalCommand::Done {
            delivery_id: "delivery_task_one".into(),
        },
    )
    .expect("control socket reachable");
    assert!(done.local_receipt);
    assert_eq!(done.hub_confirmed, Some(true));
    assert!(matches!(hub.next_report().await, Report::Result { .. }));

    // `result check` is advisory and never blocks.
    let check = myrlet_protocol::send(
        &socket_path,
        &myrlet_protocol::LocalCommand::ResultCheck {
            delivery_id: "delivery_task_one".into(),
        },
    )
    .expect("control socket reachable");
    assert!(check.local_receipt);
    assert!(check.local_sha256.is_none(), "no result file yet");

    // `result status` compares both sides without writing.
    let status = myrlet_protocol::send(
        &socket_path,
        &myrlet_protocol::LocalCommand::ResultStatus {
            delivery_id: "delivery_task_one".into(),
        },
    )
    .expect("control socket reachable");
    assert!(status.local_receipt);
    assert!(!matches!(
        status.outcome.as_str(),
        "local_receipt" | "unknown"
    ));
    assert!(matches!(hub.next_report().await, Report::ReadResult { .. }));
    assert!(
        !result_file.exists(),
        "the Worker never writes the result file"
    );

    shutdown.send(true).expect("signal shutdown");
    let _ = tokio::time::timeout(Duration::from_secs(10), worker).await;
}
