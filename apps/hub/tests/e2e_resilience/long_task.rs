//! Long-task scenarios: a silent Task still finishes with its own Result, a configured
//! >2h timeout is honoured, and a transport outage replays Result and Finish exactly once.

use std::time::Duration;

use crate::fixtures::{
    clear_trust_gates, create_task, create_task_with_timeout, finished, line_count, show,
    silent_long_yaml, status,
};
use crate::harness::{
    binaries, cli, enroll_and_pair, field, free_port, online_epoch, poll, record, shutdown,
    spawn_daemon, start_hub, start_hub_on_port,
};

/// A silent long task that writes no Progress still finishes, and its own Result reaches
/// the Hub through the real Worker/Hub entry points.
#[tokio::test]
async fn a_silent_long_task_without_progress_finishes_with_its_own_result() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let launches = directory.path().join("launches.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    std::fs::write(
        home.join("worker/worker.yaml"),
        silent_long_yaml(&hub_url, &launches),
    )
    .expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let task = create_task(
        &cli,
        "long-silent",
        "Silent long task",
        "shell",
        "exec-long",
    );
    let shown = poll("the silent task to finish", Duration::from_secs(30), || {
        finished(&cli, &task).map(|()| show(&cli, &task))
    });
    let delivery = record(&shown)
        .and_then(|task| task.get("currentDelivery"))
        .expect("current delivery");
    assert_eq!(delivery["state"], "finished", "delivery: {delivery}");
    assert!(
        delivery["progress"].is_null(),
        "no Progress was ever written: {delivery}"
    );
    assert_eq!(
        field(&shown, "resultMarkdown"),
        Some(&serde_json::json!(
            "# Long task result\n\nNo progress was ever written.\n"
        ))
    );
    assert_eq!(
        line_count(&launches),
        1,
        "the long task launched exactly once"
    );

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

/// A Task configured with a >2h execution timeout must behave exactly like any other long
/// task for its whole logical lifetime: it stays `dispatched` after claiming, writes no
/// Progress, then naturally writes its Result and finishes. This is a fast integration
/// exercise of the configured-bound path, not a real two-hour wall-clock run.
#[tokio::test]
async fn a_task_configured_for_more_than_two_hours_is_not_timed_out_early() {
    const TWO_HOURS_10_MINUTES: u64 = 7_800;

    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let launches = directory.path().join("launches.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    std::fs::write(
        home.join("worker/worker.yaml"),
        silent_long_yaml(&hub_url, &launches),
    )
    .expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let task = create_task_with_timeout(
        &cli,
        "long-2h",
        "Two-hour long task",
        "exec-long",
        TWO_HOURS_10_MINUTES,
    );
    // Claimed and running: the Task must not be finished by any timeout sweep.
    poll(
        "the long task to be claimed",
        Duration::from_secs(15),
        || (status(&show(&cli, &task)) == "dispatched").then_some(()),
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        status(&show(&cli, &task)),
        "dispatched",
        "a >2h timeout must not be shortened by any intermediate fence"
    );

    let shown = poll("the long task to finish", Duration::from_secs(30), || {
        finished(&cli, &task).map(|()| show(&cli, &task))
    });
    let delivery = record(&shown)
        .and_then(|task| task.get("currentDelivery"))
        .expect("current delivery");
    assert!(delivery["progress"].is_null(), "no Progress was written");
    assert_eq!(
        field(&shown, "resultMarkdown"),
        Some(&serde_json::json!(
            "# Long task result\n\nNo progress was ever written.\n"
        ))
    );
    assert_eq!(line_count(&launches), 1, "exactly one launch");

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

/// Opt-in real-duration exercise of the same path. Ignored by default; run explicitly:
///
/// ```text
/// MYRLET_LONG_TASK_SECONDS=7500 cargo test -p myrlet-hub \
///     --test e2e_resilience_test a_real_duration_long_task -- --ignored --nocapture
/// ```
///
/// This is the only test that consumes real wall-clock time; the default suite proves the
/// lifecycle with short scripts instead.
#[tokio::test]
#[ignore = "real wall-clock duration; opt in with MYRLET_LONG_TASK_SECONDS"]
async fn a_real_duration_long_task_runs_and_finishes_once() {
    let seconds: u64 = std::env::var("MYRLET_LONG_TASK_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .expect("set MYRLET_LONG_TASK_SECONDS to the real duration to exercise");

    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let launches = directory.path().join("launches.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    // The YAML sleep is the real duration; the Task timeout stays comfortably above it.
    let yaml =
        silent_long_yaml(&hub_url, &launches).replace("sleep 2", &format!("sleep {seconds}"));
    std::fs::write(home.join("worker/worker.yaml"), yaml).expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let timeout = seconds + 600;
    let task = create_task_with_timeout(&cli, "real-long", "Real long task", "exec-long", timeout);
    let started = std::time::Instant::now();
    let shown = poll(
        "the real long task to finish",
        Duration::from_secs(seconds + 120),
        || finished(&cli, &task).map(|()| show(&cli, &task)),
    );
    assert!(
        started.elapsed() >= Duration::from_secs(seconds),
        "the harness really ran for its full duration"
    );
    assert_eq!(
        field(&shown, "resultMarkdown"),
        Some(&serde_json::json!(
            "# Long task result\n\nNo progress was ever written.\n"
        ))
    );
    assert!(record(&shown)
        .and_then(|task| task.get("currentDelivery"))
        .expect("delivery")["progress"]
        .is_null());
    assert_eq!(line_count(&launches), 1);

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

/// The Hub is lost while a silent long task runs and exits. On reconnect the new epoch
/// replays both the Result and the Finish, and the task is never launched a second time.
#[tokio::test]
async fn transport_disconnect_across_process_exit_replays_result_and_finish_once() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let launches = directory.path().join("launches.txt");
    let port = free_port();
    let hub_url = format!("http://127.0.0.1:{port}");
    let mut hub_guard = start_hub_on_port(&hub_bin, &home, port).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    std::fs::write(
        home.join("worker/worker.yaml"),
        silent_long_yaml(&hub_url, &launches),
    )
    .expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    let first_epoch = poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });
    clear_trust_gates(&cli);

    let task = create_task(
        &cli,
        "long-outage",
        "Long task across outage",
        "shell",
        "exec-long",
    );
    poll("the task to be claimed", Duration::from_secs(15), || {
        (status(&show(&cli, &task)) == "dispatched").then_some(())
    });
    poll("the task to launch once", Duration::from_secs(10), || {
        (line_count(&launches) == 1).then_some(())
    });

    // Take the Hub away while the Harness is still running, and keep it away past the
    // Harness' own exit so the Finish and Result can only arrive after a reconnect.
    hub_guard.0.kill().expect("stop the hub");
    hub_guard.0.wait().expect("wait for the stopped hub");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        line_count(&launches),
        1,
        "the transport loss must not relaunch the task"
    );

    hub_guard = start_hub_on_port(&hub_bin, &home, port).await;
    let second_epoch = poll("worker online again", Duration::from_secs(20), || {
        online_epoch(&cli).filter(|epoch| *epoch > first_epoch)
    });
    assert!(second_epoch > first_epoch);
    let shown = poll(
        "the replayed result and finish",
        Duration::from_secs(20),
        || finished(&cli, &task).map(|()| show(&cli, &task)),
    );
    assert_eq!(
        field(&shown, "resultMarkdown"),
        Some(&serde_json::json!(
            "# Long task result\n\nNo progress was ever written.\n"
        )),
        "the new epoch replayed the Result: {shown}"
    );
    assert_eq!(
        line_count(&launches),
        1,
        "exactly one launch across the disconnect"
    );

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}
