//! Worker daemon behaviour: configuration validation, credential handling, mandatory TLS
//! verification, HTTP reporting, and one end-to-end WSS session against a stub Hub.

#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use myrlet_protocol::paths::{ensure_private_dir, write_private_file_atomic, MyrletHome};
use myrlet_protocol::{DeliveryId, ProgressReportRequest};
use myrlet_worker_daemon::credentials::{load_credentials, save_credentials};
use myrlet_worker_daemon::reporting::{HubReporter, ReportError};
use myrlet_worker_daemon::runtime::WorkerRuntime;
use myrlet_worker_daemon::tls::{build_http_client, build_tls_config, hub_ws_url};
use myrlet_worker_daemon::WorkerConfig;
use support::{Report, CREDENTIAL};
use tokio::sync::watch;

const VALID_YAML: &str = r#"
hubUrl: "http://127.0.0.1:4140"
slots:
  - slotId: "slot-a"
    capabilities:
      - name: "coding"
        state: "available"
    executors:
      - kind: claude
        id: "exec-claude"
      - kind: codex
        id: "exec-codex"
      - kind: pi
        id: "exec-pi"
      - kind: process
        id: "exec-process"
        program: "/bin/sh"
        args: ["-c", "true"]
        timeoutSeconds: 30
"#;

const DUPLICATE_SLOT_YAML: &str = r#"
hubUrl: "http://127.0.0.1:4140"
slots:
  - slotId: "slot-a"
  - slotId: "slot-a"
"#;

const BAD_HUB_YAML: &str = r#"
hubUrl: "ftp://hub.example"
slots: []
"#;

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

#[test]
fn worker_yaml_is_parsed_and_validated() {
    let (_directory, home) = temp_home();
    let path = home.worker_config();

    std::fs::write(&path, VALID_YAML).expect("write config");
    let config = WorkerConfig::load(&path).expect("valid configuration");
    assert_eq!(config.slots.len(), 1);
    assert_eq!(config.build_adapters().len(), 4);
    assert_eq!(config.inventory_fingerprint().len(), 64);
    let inventory = config.build_inventory(7, 1);
    assert_eq!(inventory.revision, 7);
    assert_eq!(inventory.slots[0].executors.len(), 4);

    std::fs::write(&path, DUPLICATE_SLOT_YAML).expect("write config");
    assert!(
        WorkerConfig::load(&path).is_err(),
        "duplicate slots must fail"
    );
    std::fs::write(&path, BAD_HUB_YAML).expect("write config");
    assert!(
        WorkerConfig::load(&path).is_err(),
        "non-http hub URL must fail"
    );
}

#[test]
fn credentials_are_private_and_fail_closed() {
    let (_directory, home) = temp_home();
    let path = home.worker_credentials();
    let credentials = support::credentials();

    save_credentials(&path, &credentials).expect("save credentials");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "credentials must be owner-only");
    assert_eq!(load_credentials(&path).expect("load"), credentials);
    let debug = format!("{credentials:?}");
    assert!(debug.contains("<redacted>"), "Debug must redact: {debug}");
    assert!(!debug.contains(CREDENTIAL), "Debug leaked the credential");

    std::fs::remove_file(&path).expect("remove credentials");
    assert!(load_credentials(&path).is_err(), "missing must fail closed");
    write_private_file_atomic(&path, b"{not json").expect("write corrupt");
    assert!(load_credentials(&path).is_err(), "corrupt must fail closed");
    save_credentials(&path, &credentials).expect("save again");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(
        load_credentials(&path).is_err(),
        "permissive must fail closed"
    );
}

#[test]
fn tls_verification_is_always_on() {
    assert!(build_tls_config(None).is_ok());
    assert!(build_http_client(None).is_ok());

    let (_directory, home) = temp_home();
    let missing = home.worker_dir().join("absent-ca.pem");
    assert!(build_tls_config(Some(&missing)).is_err());
    assert!(build_http_client(Some(&missing)).is_err());
    let invalid = home.worker_dir().join("invalid-ca.pem");
    let pem = b"-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n";
    std::fs::write(&invalid, pem).expect("write CA bundle");
    assert!(build_tls_config(Some(&invalid)).is_err());
    assert!(build_http_client(Some(&invalid)).is_err());

    assert_eq!(
        hub_ws_url("http://127.0.0.1:4140").expect("ws url"),
        "ws://127.0.0.1:4140/v1/workers/connect"
    );
    assert_eq!(
        hub_ws_url("https://hub.example:4140/").expect("wss url"),
        "wss://hub.example:4140/v1/workers/connect"
    );
    assert!(hub_ws_url("ftp://hub.example").is_err());
}

#[tokio::test]
async fn reporter_posts_progress_and_dedupes_finish() {
    let mut hub = support::start(None).await;
    let http = build_http_client(None).expect("http client");
    let reporter = HubReporter::new(hub.url.clone(), CREDENTIAL, http.clone());
    let delivery_id = DeliveryId::new("delivery_report");
    let progress = ProgressReportRequest {
        epoch: support::EPOCH,
        observed_at_ms: 10,
        text: "step one".to_owned(),
    };

    let receipt = reporter
        .post_progress(&delivery_id, &progress)
        .await
        .expect("progress accepted");
    assert_eq!(receipt.delivery_id, delivery_id);
    assert_eq!(
        hub.next_report().await,
        Report::Progress {
            delivery_id: "delivery_report".to_owned(),
            epoch: support::EPOCH,
            text: "step one".to_owned(),
        }
    );

    let stale_hub = support::start(Some(support::EPOCH)).await;
    let stale = HubReporter::new(stale_hub.url.clone(), CREDENTIAL, http.clone())
        .post_progress(&delivery_id, &progress)
        .await;
    assert!(matches!(stale, Err(ReportError::StaleDelivery(_))));

    let denied = HubReporter::new(hub.url.clone(), "wrong-credential", http)
        .post_progress(&delivery_id, &progress)
        .await;
    assert!(matches!(denied, Err(ReportError::Unauthorized)));

    let (first, second) = tokio::join!(
        reporter.post_finish(&delivery_id, support::EPOCH),
        reporter.post_finish(&delivery_id, support::EPOCH)
    );
    let results = [first, second];
    let posted = results
        .iter()
        .filter(|result| matches!(result, Ok(Some(_))))
        .count();
    let deduped = results
        .iter()
        .filter(|result| matches!(result, Ok(None)))
        .count();
    assert_eq!((posted, deduped), (1, 1), "finish must post once");
    assert_eq!(
        hub.next_report().await,
        Report::Finish {
            delivery_id: "delivery_report".to_owned(),
            epoch: support::EPOCH,
        }
    );
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
