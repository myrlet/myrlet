//! All-in-one deployment regression: the operator enables the Hub's own automatic
//! approval and configuration-apply policies, pairs one Worker, and it schedules work with
//! no per-Worker discovery or CLI approval step. This mirrors `docker/myrlet-start.sh`.

use std::time::Duration;

use myrlet_protocol::ensure_private_dir;

use crate::fixtures::{create_task, finished, line_count, show, status};
use crate::harness::{
    binaries, cli, field, online_epoch, poll, run_worker, shutdown, spawn_daemon, start_hub,
    worker_json,
};

#[tokio::test]
async fn automatic_policies_schedule_a_fresh_worker_without_manual_approval() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    ensure_private_dir(&home).expect("create home");
    let counter = directory.path().join("runs.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    // First boot: enable both automatic policies, then enroll and pair one Worker.
    let policy = cli.run(&[
        "--json",
        "policy",
        "set",
        "--auto-approve",
        "true",
        "--auto-apply",
        "true",
    ]);
    assert_eq!(policy["requireWorkerApproval"], false);
    assert_eq!(policy["requireConfigurationApply"], false);

    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode")
        .and_then(|code| code.as_str())
        .expect("enrollment code");
    run_worker(
        &worker_bin,
        &home,
        &["pair", "--hub-url", &hub_url, "--code", code],
    );

    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: local
    capabilities: [{name: shell, state: available}]
    executors:
      - {id: exec-shell, kind: process, program: /bin/sh, args: ['-c', 'echo run >> COUNTER']}
"#;
    std::fs::write(
        home.join("worker/worker.yaml"),
        document
            .replace("HUB_URL", &hub_url)
            .replace("COUNTER", &counter.display().to_string()),
    )
    .expect("write worker.yaml");

    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });

    // No `workers approve` or `workers config apply` was ever run.
    let worker = worker_json(&cli);
    assert_eq!(worker["approval"], "approved", "{worker}");
    assert_eq!(
        worker["reportedRevision"], worker["appliedRevision"],
        "the first report is applied automatically: {worker}"
    );

    let task = create_task(
        &cli,
        "auto-local",
        "Automatic local run",
        "shell",
        "exec-shell",
    );
    let shown = poll("the queued task to finish", Duration::from_secs(20), || {
        finished(&cli, &task).map(|()| show(&cli, &task))
    });
    assert_eq!(status(&shown), "finished");
    assert_eq!(line_count(&counter), 1, "the Task ran exactly once");

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}
