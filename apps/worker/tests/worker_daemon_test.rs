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
async fn runtime_session_reports_progress_and_finish() {
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    let written = home.worker_progress_dir().join("delivery_progress.json");
    let untouched = home.worker_progress_dir().join("delivery_silent.json");
    let script = format!(
        "printf '{{\"text\":\"step one\"}}' > {}; sleep 1.5",
        written.display()
    );
    let executors: [(&str, &str); 2] = [
        ("exec-progress", script.as_str()),
        ("exec-silent", "sleep 1.5"),
    ];
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &executors),
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
    hub.send_frame(support::offer_frame("delivery_progress", "exec-progress"));
    hub.send_frame(support::offer_frame("delivery_silent", "exec-silent"));

    let mut reports = Vec::new();
    while reports.len() < 3 {
        reports.push(hub.next_report().await);
    }
    let relayed = reports.iter().any(|report| {
        matches!(
            report,
            Report::Progress { delivery_id, text, .. }
                if delivery_id == "delivery_progress" && text == "step one"
        )
    });
    assert!(relayed, "relayed progress: {reports:?}");
    let finishes = reports
        .iter()
        .filter(|report| matches!(report, Report::Finish { .. }))
        .count();
    assert_eq!(finishes, 2, "both deliveries finished: {reports:?}");
    assert!(written.exists(), "the harness-written Progress file exists");
    assert!(
        !untouched.exists(),
        "the worker must never create the Progress file"
    );

    shutdown.send(true).expect("signal shutdown");
    let result = tokio::time::timeout(Duration::from_secs(10), worker)
        .await
        .expect("worker exits after shutdown")
        .expect("worker task");
    assert!(result.is_ok(), "clean shutdown");
}
