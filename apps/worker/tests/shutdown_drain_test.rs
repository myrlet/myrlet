//! Shutdown drain: a bounded graceful stop must cancel and reap the process groups the
//! Worker owns, never fabricate a Finish, and never let `main`'s guard truncate cleanup.
//!
//! This suite runs in its own test binary because it sets `MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS`
//! for the process; `shutdown_timeout_ms` reads the environment on each call.

#![cfg(unix)]

// The shared stub Hub carries the whole Worker-facing surface; this binary drives only
// the claim path, so the remaining helpers are legitimately unused here.
mod support;

use std::time::{Duration, Instant};

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

/// Poll for the leader PID the harness script wrote.
fn poll_leader(path: &std::path::Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the harness leader never started");
}

fn process_alive(pid: i32) -> bool {
    // SAFETY: `kill(pid, 0)` only probes for the process; it accesses no memory.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Kill the owned process group leader if the test fails before it is observed gone.
struct KillOnDrop(i32);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if process_alive(self.0) {
            // SAFETY: `kill` takes two integers and has no memory-safety preconditions; a
            // negative pid targets the group the harness leads.
            unsafe { libc::kill(-self.0, libc::SIGKILL) };
            // SAFETY: as above; this also covers a leader that left its own group.
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }
}

/// Shutdown with a small operator budget reaps the owned group and returns cleanly,
/// without posting a Finish for an execution that was merely cancelled.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_reaps_owned_groups_within_a_small_budget() {
    std::env::set_var("MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS", "4000");
    let mut hub = support::start(None).await;
    let (_directory, home) = temp_home();
    let pid_file = home.worker_dir().join("leader.pid");
    // The leader ignores TERM, so only the escalation to KILL can free the owned group.
    let script = format!(
        "trap '' TERM; echo $$ > {}; while :; do sleep 0.05; done",
        pid_file.display()
    );
    std::fs::write(
        home.worker_config(),
        support::process_yaml(&hub.url, &[("exec-long", script.as_str())]),
    )
    .expect("write config");
    save_credentials(&home.worker_credentials(), &support::credentials()).expect("credentials");

    let config = WorkerConfig::load(&home.worker_config()).expect("config");
    assert_eq!(config.shutdown_timeout_ms(), 4_000);
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
    let leader = poll_leader(&pid_file);
    let _guard = KillOnDrop(leader);

    let started = Instant::now();
    shutdown.send(true).expect("signal shutdown");
    let result = tokio::time::timeout(Duration::from_secs(8), worker)
        .await
        .expect("shutdown respects its drain bound")
        .expect("worker task");
    std::env::remove_var("MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS");
    assert!(result.is_ok(), "clean shutdown: {result:?}");
    assert!(
        !process_alive(leader),
        "the owned process group was reaped within the drain"
    );
    assert!(
        !hub.saw_finish(),
        "shutdown must never fabricate a Finish for the cancelled execution"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "the drain did not wait out the outer guard"
    );
}
