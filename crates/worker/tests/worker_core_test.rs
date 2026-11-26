//! Worker core contracts: idempotent local registration, inventory revision reuse,
//! bounded Progress observation, and process lifecycle through the Adapter trait.

use std::path::{Path, PathBuf};
use std::time::Duration;

use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{DeliveryId, ExecutorId, SlotId, TaskId};
use myrlet_protocol::progress::MAX_PROGRESS_FILE_BYTES;
use myrlet_protocol::task::TaskDefinition;
use myrlet_worker::adapter::ExecutorAdapter as _;
use myrlet_worker::{
    build_startup_prompt, read_progress_if_changed, HandlingEnded, HarnessCliAdapter, HarnessKind,
    LocalDeliveryRecord, LocalRegistry, ProcessAdapter, SubmitRequest,
};
use tokio::sync::mpsc;

/// `worker/` must exist as an owner-only directory before `LocalRegistry::open` runs,
/// exactly as the daemon prepares it from `MYRLET_HOME`.
fn state_dir(root: &Path) -> PathBuf {
    let worker_dir = root.join("worker");
    myrlet_protocol::paths::ensure_private_dir(&worker_dir).unwrap();
    worker_dir.join("state")
}

fn record(delivery: &str) -> LocalDeliveryRecord {
    LocalDeliveryRecord::new(
        DeliveryId::new(delivery),
        TaskId::new("task_01"),
        SlotId::new("slot-1"),
        PathBuf::from("/tmp/progress.json"),
    )
}

fn submit_request(progress_path: &Path) -> SubmitRequest {
    let task: TaskDefinition =
        serde_json::from_str(r#"{"title":"Build","prompt":"Run the test suite."}"#).unwrap();
    SubmitRequest {
        task_id: TaskId::new("task_01"),
        delivery_id: DeliveryId::new("delivery_01"),
        task,
        progress_path: progress_path.to_path_buf(),
    }
}

#[test]
fn registry_registration_is_idempotent_and_durable() {
    let temp = tempfile::tempdir().unwrap();
    let registry = LocalRegistry::open(state_dir(temp.path())).unwrap();
    let first = record("delivery_01");

    assert!(registry.register_if_absent(&first).unwrap());
    assert!(
        !registry.register_if_absent(&first).unwrap(),
        "replayed offers must not re-register"
    );
    let missing = DeliveryId::new("delivery_missing");
    assert!(registry
        .get(&DeliveryId::new("delivery_01"))
        .unwrap()
        .is_some());
    assert!(registry.get(&missing).unwrap().is_none());

    let mut saved = first.clone();
    saved.finish_pending = true;
    saved.executor_id = Some(ExecutorId::new("exec.coding"));
    registry.save(&saved).unwrap();

    // A second registry instance sees the same durable state (restart path).
    let reopened = LocalRegistry::open(state_dir(temp.path())).unwrap();
    let loaded = reopened.load_all().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0], saved);

    assert!(reopened.register_if_absent(&record("delivery_00")).unwrap());
    let ids: Vec<_> = reopened
        .load_all()
        .unwrap()
        .into_iter()
        .map(|e| e.delivery_id)
        .collect();
    let expected = vec![
        DeliveryId::new("delivery_00"),
        DeliveryId::new("delivery_01"),
    ];
    assert_eq!(ids, expected);
}

#[test]
fn registry_inventory_revision_is_persisted_and_bumped() {
    let temp = tempfile::tempdir().unwrap();
    let registry = LocalRegistry::open(state_dir(temp.path())).unwrap();

    assert_eq!(
        registry.load_or_bump_inventory_revision("cfg-a").unwrap(),
        1
    );
    assert_eq!(
        registry.load_or_bump_inventory_revision("cfg-a").unwrap(),
        1
    );
    assert_eq!(
        registry.load_or_bump_inventory_revision("cfg-b").unwrap(),
        2
    );
    let reopened = LocalRegistry::open(state_dir(temp.path())).unwrap();
    assert_eq!(
        reopened.load_or_bump_inventory_revision("cfg-b").unwrap(),
        2
    );
    assert_eq!(
        reopened.load_or_bump_inventory_revision("cfg-c").unwrap(),
        3
    );
}

#[test]
fn progress_watch_reports_only_valid_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("delivery_01.json");
    let mut hash = None;

    // Never creates the file, and a missing file is not an error.
    assert!(read_progress_if_changed(&path, &mut hash, 0).is_none());
    assert!(
        !path.exists(),
        "the Worker must never create the Progress file"
    );

    std::fs::write(&path, b"{\"text\":").unwrap();
    assert!(
        read_progress_if_changed(&path, &mut hash, 0).is_none(),
        "partial JSON is ignored"
    );

    std::fs::write(&path, br#"{"text":"starting"}"#).unwrap();
    let first = read_progress_if_changed(&path, &mut hash, 0).unwrap();
    assert_eq!(first.text, "starting");
    assert!(first.observed_at_ms > 0);
    assert!(
        read_progress_if_changed(&path, &mut hash, 0).is_none(),
        "duplicate suppressed"
    );

    std::fs::write(&path, br#"{"text":"halfway","unknown":true}"#).unwrap();
    let second = read_progress_if_changed(&path, &mut hash, first.observed_at_ms).unwrap();
    assert_eq!(second.text, "halfway");
    assert!(
        second.observed_at_ms > first.observed_at_ms,
        "timestamps stay monotonic"
    );

    let oversized = vec![b'x'; MAX_PROGRESS_FILE_BYTES + 1];
    std::fs::write(&path, oversized).unwrap();
    assert!(read_progress_if_changed(&path, &mut hash, second.observed_at_ms).is_none());
    assert!(
        path.exists(),
        "observing Progress never rewrites the Harness file"
    );
}

#[test]
fn startup_prompt_carries_only_identification_and_progress_path() {
    let prompt = build_startup_prompt(&submit_request(Path::new(
        "/var/lib/myrlet/worker/progress/delivery_01.json",
    )));
    assert!(prompt.contains("Task ID: task_01"));
    assert!(prompt.contains("Delivery ID: delivery_01"));
    assert!(prompt.contains("/var/lib/myrlet/worker/progress/delivery_01.json"));
    assert!(prompt.contains("Run the test suite."));
    assert!(prompt.contains("{\"text\":\"...\"}"));
    let forbidden = [
        "--mcp",
        "CLAUDE.md",
        ".mcp.json",
        "MYRLET_",
        "hook",
        "plugin",
    ];
    for needle in forbidden {
        assert!(!prompt.contains(needle), "prompt must not inject {needle}");
    }
}

#[test]
fn harness_adapters_report_the_kind_of_their_cli_family() {
    for (kind, label) in [
        (HarnessKind::Claude, "claude"),
        (HarnessKind::Codex, "codex"),
        (HarnessKind::Pi, "pi"),
    ] {
        let adapter = HarnessCliAdapter::new(ExecutorId::new("exec.harness"), kind);
        assert_eq!(adapter.kind(), label);
        assert_eq!(
            kind.program(),
            label,
            "CLI name matches its inventory label"
        );
    }
}

#[tokio::test]
async fn process_adapter_reports_end_of_handling_and_confirms_cancel() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.process"),
        "/bin/sh",
        vec!["-c".to_owned(), "sleep 30".to_owned()],
        None,
    );
    let (on_exit, mut exited) = mpsc::unbounded_channel();

    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();
    assert_eq!(adapter.id().as_str(), "exec.process");
    assert_eq!(adapter.kind(), "process");

    let state = adapter.cancel(&execution_ref).await;
    assert_eq!(state, CancelState::Confirmed);
    let ended = exited
        .recv()
        .await
        .expect("end of handling must be reported");
    let expected = HandlingEnded {
        delivery_id: DeliveryId::new("delivery_01"),
        execution_ref: execution_ref.clone(),
    };
    assert_eq!(ended, expected);

    // A second cancel for an already-reaped execution proves nothing about side effects.
    assert_eq!(adapter.cancel(&execution_ref).await, CancelState::Uncertain);
}

#[tokio::test]
async fn process_adapter_reports_handling_end_without_cancel() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.quick"),
        "/bin/sh",
        vec!["-c".to_owned(), "exit 3".to_owned()],
        Some(5),
    );
    let (on_exit, mut exited) = mpsc::unbounded_channel();
    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();

    let ended = tokio::time::timeout(Duration::from_secs(10), exited.recv())
        .await
        .unwrap()
        .expect("end of handling must be reported");
    let expected = HandlingEnded {
        delivery_id: DeliveryId::new("delivery_01"),
        execution_ref,
    };
    assert_eq!(ended, expected);
}
