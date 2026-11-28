//! Worker core contracts: idempotent local registration, inventory revision reuse,
//! bounded Progress observation, and process lifecycle through the Adapter trait.

use std::path::{Path, PathBuf};
use std::time::Duration;

use myrlet_protocol::delivery::CancelState;
use myrlet_protocol::id::{DeliveryId, ExecutorId, SlotId, TaskId};
use myrlet_protocol::progress::MAX_PROGRESS_FILE_BYTES;
use myrlet_protocol::result::MAX_RESULT_BYTES;
use myrlet_protocol::task::TaskDefinition;
use myrlet_worker::adapter::ExecutorAdapter as _;
use myrlet_worker::{
    build_startup_prompt, read_progress_if_changed, read_result, HandlingEnded, HarnessCliAdapter,
    HarnessKind, LocalDeliveryRecord, LocalRegistry, LocalResult, ProcessAdapter, SubmitRequest,
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
        PathBuf::from("/tmp/result.md"),
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
        result_path: progress_path.with_extension("md"),
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
fn result_read_is_bounded_utf8_and_never_creates_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("results");
    myrlet_protocol::ensure_private_dir(&dir).unwrap();
    let path = dir.join("delivery_01.md");

    assert_eq!(
        read_result(&path),
        LocalResult::Absent,
        "missing output is not a result"
    );
    assert!(
        !path.exists(),
        "the Worker must never create the result file"
    );

    myrlet_protocol::paths::write_private_file_atomic(&path, b"   \n").unwrap();
    assert_eq!(
        read_result(&path),
        LocalResult::Empty,
        "blank output is no result"
    );

    myrlet_protocol::paths::write_private_file_atomic(&path, b"# Findings\n\n- one\n").unwrap();
    assert_eq!(
        read_result(&path),
        LocalResult::Content("# Findings\n\n- one\n".to_string())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let link = dir.join("linked.md");
        symlink(&path, &link).unwrap();
        assert_eq!(
            read_result(&link),
            LocalResult::Unreadable,
            "a symlink is never a result"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            read_result(&path),
            LocalResult::Unreadable,
            "a group-readable file is unsafe"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    myrlet_protocol::paths::write_private_file_atomic(&path, &[0xff, 0xfe]).unwrap();
    assert_eq!(
        read_result(&path),
        LocalResult::Unreadable,
        "invalid UTF-8 is unsafe, never absence"
    );

    let oversized = vec![b'x'; MAX_RESULT_BYTES + 1];
    myrlet_protocol::paths::write_private_file_atomic(&path, &oversized).unwrap();
    assert_eq!(
        read_result(&path),
        LocalResult::Unreadable,
        "oversized is unsafe, never absence"
    );
    assert!(
        path.exists(),
        "observing a result never removes the Harness file"
    );
}

#[test]
fn startup_prompt_carries_only_identification_progress_and_result_path() {
    let prompt = build_startup_prompt(&submit_request(Path::new(
        "/var/lib/myrlet/worker/progress/delivery_01.json",
    )));
    assert!(prompt.contains("Task ID: task_01"));
    assert!(prompt.contains("Delivery ID: delivery_01"));
    assert!(prompt.contains("/var/lib/myrlet/worker/progress/delivery_01.json"));
    assert!(prompt.contains("/var/lib/myrlet/worker/progress/delivery_01.md"));
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

/// A same-group descendant that ignores `SIGTERM` must not survive cancellation, and the
/// cancel may only be confirmed after the whole group is gone, not merely the leader.
#[cfg(unix)]
#[tokio::test]
async fn cancel_reaps_a_same_group_descendant_that_ignores_term() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("descendant.pid");
    let script = descendant_script(&pid_file);
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.descendant"),
        "/bin/sh",
        vec!["-c".to_owned(), script],
        None,
    );
    let (on_exit, _exited) = mpsc::unbounded_channel();
    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();

    let descendant = poll_pid(&pid_file).expect("the descendant must start");
    poll_ready(&pid_file.with_extension("pid.ready"));
    let _guard = KillOnDrop(descendant);

    let state = adapter.cancel(&execution_ref).await;
    assert_eq!(state, CancelState::Confirmed);
    assert!(
        !process_alive(descendant),
        "a same-group descendant that ignores TERM survived a confirmed cancel"
    );
}

/// The TERM-immune leader alone is still killed by the escalation, and only then is the
/// cancel confirmed.
#[cfg(unix)]
#[tokio::test]
async fn a_term_immune_leader_is_killed_and_then_confirmed() {
    let temp = tempfile::tempdir().unwrap();
    let ready = temp.path().join("ready");
    let script = format!(
        "trap '' TERM; : > {}; while :; do sleep 0.05; done",
        ready.display()
    );
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.stubborn"),
        "/bin/sh",
        vec!["-c".to_owned(), script],
        None,
    );
    let (on_exit, mut exited) = mpsc::unbounded_channel();
    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();
    poll_ready(&ready);

    assert_eq!(adapter.cancel(&execution_ref).await, CancelState::Confirmed);
    exited.recv().await.expect("handling must end");
}

/// A cancellation that killed the group still reaches end of handling exactly once.
#[cfg(unix)]
#[tokio::test]
async fn cancel_reaches_end_of_handling_after_killing_the_group() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("descendant.pid");
    let script = descendant_script(&pid_file);
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.ended"),
        "/bin/sh",
        vec!["-c".to_owned(), script],
        None,
    );
    let (on_exit, mut exited) = mpsc::unbounded_channel();
    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();
    let descendant = poll_pid(&pid_file).expect("the descendant must start");
    poll_ready(&pid_file.with_extension("pid.ready"));
    let _guard = KillOnDrop(descendant);

    assert_eq!(adapter.cancel(&execution_ref).await, CancelState::Confirmed);
    let ended = exited.recv().await.expect("handling must end");
    assert_eq!(ended.execution_ref, execution_ref);
    assert!(
        exited.try_recv().is_err(),
        "end of handling is reported once"
    );
}

/// A naturally exited long task that still writes its own Result is reported once, even
/// though it never wrote Progress.
#[cfg(unix)]
#[tokio::test]
async fn natural_exit_reports_handling_once_and_never_confirms_a_late_cancel() {
    let temp = tempfile::tempdir().unwrap();
    let results = temp.path().join("results");
    myrlet_protocol::ensure_private_dir(&results).unwrap();
    let result_path = results.join("delivery_01.md");
    let script = format!(
        "umask 077; printf '# Long result\\n' > '{}'",
        result_path.display()
    );
    let adapter = ProcessAdapter::new(
        ExecutorId::new("exec.long"),
        "/bin/sh",
        vec!["-c".to_owned(), script],
        None,
    );
    let (on_exit, mut exited) = mpsc::unbounded_channel();
    let execution_ref = adapter
        .submit(submit_request(&temp.path().join("p.json")), on_exit)
        .await
        .unwrap();

    let ended = exited.recv().await.expect("handling must end");
    assert_eq!(ended.execution_ref, execution_ref);
    assert!(
        exited.try_recv().is_err(),
        "end of handling is reported once"
    );
    assert_eq!(
        read_result(&result_path),
        LocalResult::Content("# Long result\n".into())
    );
    // The execution already ended: a later cancel cannot claim physical termination.
    assert_eq!(adapter.cancel(&execution_ref).await, CancelState::Uncertain);
}

/// A leader that exits on TERM while a descendant ignores it: the script keeps the
/// descendant in the same group so only a group-wide escalation can reap it. The file
/// written after installing both traps prevents racing the signal handlers.
#[cfg(unix)]
fn descendant_script(pid_file: &Path) -> String {
    format!(
        "trap 'exit 0' TERM; (trap '' TERM; exec sleep 30) & echo $! > {}; : > {}.ready; \
         while :; do sleep 0.05; done",
        pid_file.display(),
        pid_file.display()
    )
}

/// Kill a descendant if the test fails before/after observing it.
#[cfg(unix)]
struct KillOnDrop(i32);
#[cfg(unix)]
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if process_alive(self.0) {
            // SAFETY: `kill` takes two integers and has no memory-safety preconditions.
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }
}

/// Poll until the child script has installed its trap and started looping.
#[cfg(unix)]
fn poll_ready(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the harness script never became ready");
}

/// Poll for a PID written by the child script.
#[cfg(unix)]
fn poll_pid(path: &Path) -> Option<i32> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return Some(pid);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

/// Whether a process with `pid` currently exists.
#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    // SAFETY: `kill(pid, 0)` only probes for the process; it accesses no memory.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
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
