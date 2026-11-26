//! Cross-process end-to-end test driving the real `hub`, `worker`, and `myrlet` binaries.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ADMIN_TOKEN: &str = "e2e-secret";

/// RAII wrapper so a panicking test never leaks a daemon process.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn binaries() -> (PathBuf, PathBuf, PathBuf) {
    let hub_bin = PathBuf::from(env!("CARGO_BIN_EXE_hub"));
    let bin_dir = hub_bin.parent().expect("bin dir");
    let worker_bin = bin_dir.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    let cli_bin = bin_dir.join(format!("myrlet{}", std::env::consts::EXE_SUFFIX));
    for binary in [&worker_bin, &cli_bin] {
        assert!(
            binary.exists(),
            "missing {}: run cargo build --workspace --bins first",
            binary.display()
        );
    }
    (hub_bin, worker_bin, cli_bin)
}

/// Bind an ephemeral localhost port, then release it for the hub to claim.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local addr").port()
}

/// Read `key` from a CLI document, falling back to a nested `task` object.
fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.get(key).or_else(|| value.get("task")?.get(key))
}

fn status(value: &serde_json::Value) -> &str {
    field(value, "status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<missing>")
}

/// A `myrlet` CLI bound to one home directory and hub URL.
struct Cli<'a> {
    bin: &'a Path,
    home: &'a Path,
    hub_url: &'a str,
}

impl Cli<'_> {
    /// Run a subcommand, asserting a clean exit, and parse its stdout JSON.
    fn run(&self, args: &[&str]) -> serde_json::Value {
        let output = Command::new(self.bin)
            .args(args)
            .env("MYRLET_HOME", self.home)
            .env("MYRLET_HUB_URL", self.hub_url)
            .env("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN)
            .stdin(Stdio::null())
            .output()
            .expect("run myrlet cli");
        assert!(
            output.status.success(),
            "myrlet {args:?} exited with {}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("cli stdout is json")
    }

    /// Create a task pinned to one executor and assert it is `held`.
    fn create(&self, key: &str, title: &str, prompt: &str, executor: &str) -> String {
        let created = self.run(&[
            "--json",
            "tasks",
            "create",
            "--idempotency-key",
            key,
            "--title",
            title,
            "--prompt",
            prompt,
            "--capability",
            "shell",
            "--preferred-executor-id",
            executor,
        ]);
        assert_eq!(status(&created), "held", "create {key}: {created}");
        field(&created, "id")
            .and_then(serde_json::Value::as_str)
            .expect("task id")
            .to_owned()
    }

    /// Dispatch a task with `--match` and assert it left the held state.
    fn dispatch(&self, id: &str, extra: &[&str]) {
        let mut args = vec!["--json", "tasks", "dispatch", id];
        args.extend_from_slice(extra);
        let dispatched = self.run(&args);
        assert_eq!(
            status(&dispatched),
            "dispatched",
            "dispatch {id}: {dispatched}"
        );
    }
}

/// Run a foreground `worker` subcommand, asserting a clean exit.
fn run_worker(worker_bin: &Path, home: &Path, args: &[&str]) {
    let output = Command::new(worker_bin)
        .args(args)
        .env("MYRLET_HOME", home)
        .stdout(Stdio::null())
        .output()
        .expect("run worker");
    assert!(
        output.status.success(),
        "worker {args:?} exited with {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Retry `probe` for up to 10s, panicking with `what` when it never succeeds.
fn poll<T>(what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(value) = probe() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

/// Send `SIGTERM` to a daemon and require a clean exit within 5s.
fn shutdown(child: &mut Child, name: &str) {
    let pid = child.id().to_string();
    let killed = Command::new("/bin/kill")
        .arg("-TERM")
        .arg(&pid)
        .status()
        .expect("run /bin/kill");
    assert!(killed.success(), "kill -TERM {pid} failed");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(exit) = child.try_wait().expect("try_wait") {
            assert!(exit.success(), "{name} exited uncleanly: {exit}");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{name} did not exit within 5s");
}

fn worker_yaml(hub_url: &str) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: e2e-slot
    capabilities:
      - name: shell
        state: available
    executors:
      - id: exec-progress
        kind: process
        program: /bin/sh
        args: ['-c', 'p=$(sed -n ''s/.*to \(\/.*\.json\).*/\1/p'' | head -n1); printf ''{"text":"e2e progress ok"}'' > "$p"']
      - id: exec-slow
        kind: process
        program: /bin/sh
        args: ['-c', 'sleep 30']
"#;
    document.replace("HUB_URL", hub_url)
}

/// Spawn a long-running daemon with detached output, guarded against leaks.
fn spawn_daemon(bin: &Path, envs: &[(&str, &str)]) -> ChildGuard {
    let mut command = Command::new(bin);
    for (key, value) in envs {
        command.env(key, value);
    }
    let child = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {}: {error}", bin.display()));
    ChildGuard(child)
}

/// Poll the hub health endpoint until it answers `200 OK` (max 10s).
async fn wait_healthy(hub_url: &str) {
    let health = format!("{hub_url}/v1/health");
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(response) = client.get(&health).send().await {
            if response.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("hub never healthy at {health}");
}

#[tokio::test]
async fn cross_process_hub_worker_cli_end_to_end() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");

    let port = free_port();
    let hub_url = format!("http://127.0.0.1:{port}");
    let bind = format!("127.0.0.1:{port}");
    let mut hub_guard = spawn_daemon(
        &hub_bin,
        &[
            ("MYRLET_HOME", home.to_str().expect("utf-8 home")),
            ("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN),
            ("MYRLET_HUB_BIND", &bind),
        ],
    );
    wait_healthy(&hub_url).await;
    let cli = Cli {
        bin: &cli_bin,
        home: &home,
        hub_url: &hub_url,
    };

    // 1. Enroll a worker and grab its one-shot code.
    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode").and_then(serde_json::Value::as_str);
    let code = code.expect("enrollment code").to_owned();

    // 2. Pair the worker binary, then 3./4. write its config and validate it offline.
    run_worker(
        &worker_bin,
        &home,
        &["pair", "--hub-url", &hub_url, "--code", &code],
    );
    assert!(
        home.join("worker/credentials.json").exists(),
        "pair wrote no credentials"
    );
    std::fs::write(home.join("worker/worker.yaml"), worker_yaml(&hub_url))
        .expect("write worker.yaml");
    run_worker(&worker_bin, &home, &["check-config"]);

    // 5. Start the daemon and wait until the hub reports it online.
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", home.to_str().unwrap())]);
    poll("the worker to come online", || {
        let listed = cli.run(&["--json", "workers", "list"]);
        (listed["workers"][0]["online"] == serde_json::json!(true)).then_some(())
    });

    // 6. Create -> matched dispatch -> progress -> finish.
    let first = cli.create("e2e-1", "E2E progress", "Write progress", "exec-progress");
    let dispatch = [
        "--expected-version",
        "1",
        "--idempotency-key",
        "e2e-d1",
        "--match",
    ];
    cli.dispatch(&first, &dispatch);
    let shown = poll("the progress task to finish", || {
        let shown = cli.run(&["--json", "tasks", "show", &first]);
        (status(&shown) == "finished").then_some(shown)
    });
    let finished = field(&shown, "currentDelivery").expect("current delivery");
    assert_eq!(finished["state"], "finished", "delivery: {finished}");
    assert_eq!(
        finished["progress"]["text"], "e2e progress ok",
        "progress: {finished}"
    );
    assert!(
        finished["progress"]["firstReceivedAtMs"].as_i64().unwrap() > 0,
        "progress timestamp: {finished}"
    );

    // 7. Create -> dispatch -> cancel.
    let second = cli.create("e2e-2", "E2E cancel", "Sleep", "exec-slow");
    cli.dispatch(
        &second,
        &[
            "--expected-version",
            "1",
            "--idempotency-key",
            "e2e-d2",
            "--match",
        ],
    );
    let cancel = ["--json", "tasks", "cancel", &second, "--reason", "stop e2e"];
    let cancelled = cli.run(&cancel);
    assert_eq!(status(&cancelled), "cancelled", "cancel: {cancelled}");
    let shown = cli.run(&["--json", "tasks", "show", &second]);
    assert_eq!(status(&shown), "cancelled", "show 2: {shown}");

    // 8. Graceful shutdown of the real daemons.
    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}
