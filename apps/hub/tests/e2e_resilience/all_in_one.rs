//! All-in-one deployment regression: the local Worker is approved and applied by its own
//! Hub-assigned identity, while the Hub's Security & Trust policies stay untouched and any
//! other Worker remains subject to them. Mirrors `docker/myrlet-start.sh`.

use std::path::Path;
use std::time::Duration;

use myrlet_protocol::ensure_private_dir;

use crate::fixtures::{create_task, finished, line_count};
use crate::harness::{binaries, cli, field, poll, shutdown, spawn_daemon, start_hub, Cli};

/// Run a foreground `worker` subcommand and return its combined output text.
fn run_worker_capture(worker: &Path, home: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new(worker)
        .args(args)
        .env("MYRLET_HOME", home)
        .output()
        .expect("run worker");
    assert!(
        output.status.success(),
        "worker {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Parse `worker: paired as <id>` from the pairing command's stdout.
fn paired_id(paired: &str) -> String {
    paired
        .trim()
        .rsplit_once("paired as ")
        .map(|(_, id)| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .expect("pairing prints the assigned workerId")
}

/// Enroll and pair one Worker under its own home, returning its assigned identity.
fn pair_worker(cli: &Cli<'_>, worker_bin: &Path, home: &Path, hub_url: &str) -> String {
    ensure_private_dir(home).expect("create worker home");
    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode")
        .and_then(|code| code.as_str())
        .expect("enrollment code");
    let paired = run_worker_capture(
        worker_bin,
        home,
        &["pair", "--hub-url", hub_url, "--code", code],
    );
    paired_id(&paired)
}

/// Read one Worker's detail once and converge it exactly as the entrypoint does.
///
/// Approval and the applied revision come from the same `workers show`; each step runs only
/// while the Hub still requires it, so repeating the call changes nothing.
fn converge_one(cli: &Cli<'_>, worker_id: &str) -> u32 {
    let shown = cli.run(&["--json", "workers", "show", worker_id]);
    let mut mutations = 0;
    if shown["worker"]["approval"].as_str() != Some("approved") {
        cli.run(&["--json", "workers", "approve", worker_id]);
        mutations += 1;
    }
    let revision = shown["worker"]["reportedRevision"]
        .as_u64()
        .expect("the online Worker reported an inventory");
    if shown["worker"]["appliedRevision"].as_u64() != Some(revision) {
        cli.run(&[
            "--json",
            "workers",
            "config",
            "apply",
            worker_id,
            "--revision",
            &revision.to_string(),
        ]);
        mutations += 1;
    }
    mutations
}

fn shell_worker_yaml(hub_url: &str, counter: &Path) -> String {
    format!(
        "hubUrl: '{hub_url}'\nslots:\n  - slotId: local\n    capabilities: [{{name: shell, state: available}}]\n    executors:\n      - {{id: exec-shell, kind: process, program: /bin/sh, args: ['-c', 'echo run >> {counter}']}}\n",
        counter = counter.display(),
    )
}

/// Approve a Worker and read its reported revision, mirroring the entrypoint's `workers show`.
fn reported_revision(cli: &Cli<'_>, worker_id: &str) -> Option<u64> {
    cli.run(&["--json", "workers", "show", worker_id])["worker"]["reportedRevision"].as_u64()
}

#[tokio::test]
async fn a_local_worker_is_converged_without_changing_the_hub_policies() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    ensure_private_dir(&home).expect("create home");
    let counter = directory.path().join("runs.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);

    // The Hub starts with both policies on and the entrypoint never changes them.
    let policies_before = cli.run(&["--json", "policy", "show"]);
    assert_eq!(policies_before["requireWorkerApproval"], true);
    assert_eq!(policies_before["requireConfigurationApply"], true);

    // Pair the local Worker and remember the identity its pairing reported.
    let local_id = pair_worker(&cli, &worker_bin, &home, &hub_url);
    assert!(local_id.starts_with("wkr_"), "a workerId: {local_id}");
    std::fs::write(
        home.join("worker/worker.yaml"),
        shell_worker_yaml(&hub_url, &counter),
    )
    .expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", home.to_str().unwrap())]);
    poll(
        "local worker online and reporting",
        Duration::from_secs(10),
        || reported_revision(&cli, &local_id),
    );

    // Approve and apply only this Worker; both are required by the still-on policies.
    assert_eq!(converge_one(&cli, &local_id), 2, "approve then apply");
    // A second call converges nothing, so a container restart is a no-op.
    assert_eq!(
        converge_one(&cli, &local_id),
        0,
        "repeat startup is idempotent"
    );

    let task = create_task(&cli, "local-run", "Local run", "shell", "exec-shell");
    poll("the queued task to finish", Duration::from_secs(20), || {
        finished(&cli, &task)
    });
    assert_eq!(line_count(&counter), 1, "the Task ran exactly once");

    // The Hub's policies are untouched by the whole exchange.
    let policies_after = cli.run(&["--json", "policy", "show"]);
    assert_eq!(
        policies_after, policies_before,
        "startup leaves policies alone"
    );

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

#[tokio::test]
async fn another_workers_configuration_is_never_applied_and_approval_is_not_forced() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    let remote_home = directory.path().join("remote-home");
    ensure_private_dir(&home).expect("create home");
    ensure_private_dir(&remote_home).expect("create remote home");
    let local_counter = directory.path().join("local.txt");
    let remote_counter = directory.path().join("remote.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);

    // Two distinct Workers; only the first belongs to this "container".
    let local_id = pair_worker(&cli, &worker_bin, &home, &hub_url);
    let remote_id = pair_worker(&cli, &worker_bin, &remote_home, &hub_url);
    assert_ne!(local_id, remote_id, "each pairing is a distinct Worker");
    std::fs::write(
        home.join("worker/worker.yaml"),
        shell_worker_yaml(&hub_url, &local_counter),
    )
    .expect("write local worker.yaml");
    std::fs::write(
        remote_home.join("worker/worker.yaml"),
        shell_worker_yaml(&hub_url, &remote_counter),
    )
    .expect("write remote worker.yaml");
    let mut local_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", home.to_str().unwrap())]);
    let mut remote_guard = spawn_daemon(
        &worker_bin,
        &[("MYRLET_HOME", remote_home.to_str().unwrap())],
    );

    poll(
        "both workers online and reporting",
        Duration::from_secs(15),
        || {
            let local = reported_revision(&cli, &local_id);
            let remote = reported_revision(&cli, &remote_id);
            (local.is_some() && remote.is_some()).then_some(())
        },
    );

    // Startup converges only the local Worker.
    assert_eq!(converge_one(&cli, &local_id), 2);

    // The remote Worker keeps the Hub's approval policy: still pending, never applied.
    let remote = cli.run(&["--json", "workers", "show", &remote_id]);
    assert_eq!(
        remote["worker"]["approval"], "pending",
        "an unrelated Worker is not approved: {remote}"
    );
    assert_eq!(
        remote["worker"]["appliedRevision"],
        serde_json::Value::Null,
        "an unrelated Worker's configuration is not applied: {remote}"
    );

    // The policies are untouched and still gate the remote Worker.
    let policies = cli.run(&["--json", "policy", "show"]);
    assert_eq!(policies["requireWorkerApproval"], true);
    assert_eq!(policies["requireConfigurationApply"], true);

    // The local Worker remains approved and applied; the remote one stays pending.
    let local = cli.run(&["--json", "workers", "show", &local_id]);
    assert_eq!(local["worker"]["approval"], "approved");
    assert_eq!(
        local["worker"]["appliedRevision"], local["worker"]["reportedRevision"],
        "the local Worker's configuration is applied"
    );

    shutdown(&mut remote_guard.0, "remote worker");
    shutdown(&mut local_guard.0, "local worker");
    shutdown(&mut hub_guard.0, "hub");
}
