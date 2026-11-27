//! Cross-process resilience: FIFO queue auto-dispatch and restart resync replay.

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
    let hub = PathBuf::from(env!("CARGO_BIN_EXE_hub"));
    let dir = hub.parent().expect("bin dir");
    let worker = dir.join(format!("worker{}", std::env::consts::EXE_SUFFIX));
    let cli = dir.join(format!("myrlet{}", std::env::consts::EXE_SUFFIX));
    assert!(
        worker.exists() && cli.exists(),
        "build the workspace binaries first"
    );
    (hub, worker, cli)
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
    listener.local_addr().expect("addr").port()
}

/// Read `key` from a CLI document, falling back to a nested `task` object.
fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.get(key).or_else(|| value.get("task")?.get(key))
}

fn status(value: &serde_json::Value) -> &str {
    field(value, "status")
        .and_then(|status| status.as_str())
        .unwrap_or("")
}

/// A `myrlet` CLI bound to one home directory and hub URL.
struct Cli<'a> {
    bin: &'a Path,
    home: &'a Path,
    hub_url: &'a str,
}

fn cli<'a>(bin: &'a Path, home: &'a Path, hub_url: &'a str) -> Cli<'a> {
    Cli { bin, home, hub_url }
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
}

/// Run a foreground `worker` subcommand, asserting a clean exit.
fn run_worker(worker: &Path, home: &Path, args: &[&str]) {
    let output = Command::new(worker)
        .args(args)
        .env("MYRLET_HOME", home)
        .stdout(Stdio::null())
        .output()
        .expect("run worker");
    assert!(
        output.status.success(),
        "worker {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Retry `probe` until it yields a value or `limit` elapses.
fn poll<T>(what: &str, limit: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(value) = probe() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {what}");
}

/// Send `SIGTERM` to a daemon and require a clean exit within 5s.
fn shutdown(child: &mut Child, name: &str) {
    let pid = child.id().to_string();
    let killed = Command::new("/bin/kill")
        .args(["-TERM", &pid])
        .status()
        .expect("kill");
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

/// Spawn a long-running daemon with detached output, guarded against leaks.
fn spawn_daemon(bin: &Path, envs: &[(&str, &str)]) -> ChildGuard {
    let mut command = Command::new(bin);
    command
        .envs(envs.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    ChildGuard(command.spawn().expect("spawn daemon"))
}

/// Poll the hub health endpoint until it answers `200 OK` (max 10s).
async fn wait_healthy(hub_url: &str) {
    let health = format!("{hub_url}/v1/health");
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if client
            .get(&health)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("hub never healthy at {health}");
}

/// Start a real hub on an ephemeral port and wait until it is healthy.
async fn start_hub(hub: &Path, home: &Path) -> (ChildGuard, String) {
    let port = free_port();
    let url = format!("http://127.0.0.1:{port}");
    let bind = format!("127.0.0.1:{port}");
    let env_home = home.to_str().expect("utf-8 home");
    let guard = spawn_daemon(
        hub,
        &[
            ("MYRLET_HOME", env_home),
            ("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN),
            ("MYRLET_HUB_BIND", bind.as_str()),
        ],
    );
    wait_healthy(&url).await;
    (guard, url)
}

/// Enroll a fresh Worker and pair the real binary with its one-shot code.
fn enroll_and_pair(cli: &Cli<'_>, worker: &Path, home: &Path, hub_url: &str) {
    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode").and_then(|code| code.as_str());
    run_worker(
        worker,
        home,
        &["pair", "--hub-url", hub_url, "--code", code.expect("code")],
    );
}

/// Submit one authorized Task pinned to `executor` and return its id.
fn create_task(cli: &Cli<'_>, key: &str, title: &str, capability: &str, executor: &str) -> String {
    let created = cli.run(&[
        "--json",
        "tasks",
        "submit",
        "--idempotency-key",
        key,
        "--title",
        title,
        "--prompt",
        "resilience",
        "--capability",
        capability,
        "--preferred-executor-id",
        executor,
    ]);
    assert_eq!(status(&created), "queued", "submit {key}: {created}");
    field(&created, "id")
        .and_then(|id| id.as_str())
        .expect("id")
        .to_owned()
}

fn show(cli: &Cli<'_>, id: &str) -> serde_json::Value {
    cli.run(&["--json", "tasks", "show", id])
}

fn finished(cli: &Cli<'_>, id: &str) -> Option<()> {
    (status(&show(cli, id)) == "finished").then_some(())
}

fn worker_json(cli: &Cli<'_>) -> serde_json::Value {
    cli.run(&["--json", "workers", "list"])["workers"][0].clone()
}

/// Epoch of the single Worker when it is online, else `None`.
fn online_epoch(cli: &Cli<'_>) -> Option<u64> {
    let worker = worker_json(cli);
    worker["online"]
        .as_bool()?
        .then_some(worker["connectionEpoch"].as_u64())?
}

fn worker_offline(cli: &Cli<'_>) -> bool {
    worker_json(cli)["online"].as_bool() == Some(false)
}

fn json_file_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count()
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

fn gpu_yaml(hub_url: &str) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: gpu-slot
    capabilities: [{name: gpu, state: available}]
    executors:
      - {id: exec-gpu, kind: process, program: /bin/sh, args: ['-c', 'p=$(sed -n ''s/.*to \(\/.*\.json\).*/\1/p'' | head -n1); printf ''{"text":"gpu queued ok"}'' > "$p"']}
"#;
    document.replace("HUB_URL", hub_url)
}

fn resync_yaml(hub_url: &str, counter: &Path) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: resync-slot
    capabilities: [{name: shell, state: available}]
    executors:
      - {id: exec-count, kind: process, program: /bin/sh, args: ['-c', 'echo run >> COUNTER']}
      - {id: exec-sleep, kind: process, program: /bin/sh, args: ['-c', 'sleep 30']}
"#;
    document
        .replace("HUB_URL", hub_url)
        .replace("COUNTER", &counter.display().to_string())
}

#[tokio::test]
async fn a_queued_task_waits_for_a_compatible_worker_to_claim_it() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    // Submission authorizes the Task; with no Worker online it simply stays queued.
    let task = create_task(&cli, "queue-create", "Queued GPU work", "gpu", "exec-gpu");
    assert_eq!(status(&show(&cli, &task)), "queued");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        status(&show(&cli, &task)),
        "queued",
        "a queued Task waits; no Worker means no claim"
    );
    assert!(
        show(&cli, &task)["currentDelivery"].is_null(),
        "no wakeup or queue admission creates a Delivery"
    );

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    std::fs::write(home.join("worker/worker.yaml"), gpu_yaml(&hub_url)).expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);

    let shown = poll("the queued task to finish", Duration::from_secs(15), || {
        let shown = show(&cli, &task);
        (status(&shown) == "finished").then_some(shown)
    });
    let delivery = field(&shown, "currentDelivery").expect("current delivery");
    assert_eq!(
        delivery["progress"]["text"], "gpu queued ok",
        "progress: {delivery}"
    );
    assert!(
        delivery["progress"]["firstReceivedAtMs"]
            .as_i64()
            .unwrap_or(0)
            > 0
    );

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}

#[tokio::test]
async fn worker_restart_resync_prevents_double_submit_and_replays_offline_cancel() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    myrlet_protocol::ensure_private_dir(&home).expect("create home");
    let counter = directory.path().join("counter.txt");
    let (mut hub_guard, hub_url) = start_hub(&hub_bin, &home).await;
    let cli = cli(&cli_bin, &home, &hub_url);
    let env_home = home.to_str().expect("utf-8 home");

    enroll_and_pair(&cli, &worker_bin, &home, &hub_url);
    let yaml = home.join("worker/worker.yaml");
    std::fs::write(&yaml, resync_yaml(&hub_url, &counter)).expect("write worker.yaml");

    let mut first = spawn_daemon(&worker_bin, &[("MYRLET_HOME", env_home)]);
    let epoch_one = poll("worker online", Duration::from_secs(10), || {
        online_epoch(&cli)
    });

    let counted = create_task(&cli, "resync-a", "Count one run", "shell", "exec-count");
    poll("Task A to finish", Duration::from_secs(15), || {
        finished(&cli, &counted)
    });
    assert_eq!(line_count(&counter), 1, "Task A must run exactly once");

    let sleeping = create_task(&cli, "resync-b", "Sleep", "shell", "exec-sleep");
    poll("Task B to be claimed", Duration::from_secs(15), || {
        (status(&show(&cli, &sleeping)) == "dispatched").then_some(())
    });
    let deliveries = home.join("worker/state/deliveries");
    poll("Task B registered", Duration::from_secs(10), || {
        (json_file_count(&deliveries) >= 2).then_some(())
    });

    first.0.kill().expect("kill worker instance 1");
    first.0.wait().expect("wait worker instance 1");
    poll("worker offline", Duration::from_secs(15), || {
        worker_offline(&cli).then_some(())
    });
    let cancel_args = [
        "--json",
        "tasks",
        "cancel",
        &sleeping,
        "--reason",
        "offline cancel",
    ];
    let cancelled = cli.run(&cancel_args);
    assert_eq!(
        status(&cancelled),
        "cancelled",
        "offline cancel: {cancelled}"
    );

    let mut second = spawn_daemon(
        &worker_bin,
        &[
            ("MYRLET_HOME", env_home),
            ("MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS", "1500"),
        ],
    );
    poll("worker online again", Duration::from_secs(15), || {
        online_epoch(&cli).filter(|epoch| *epoch > epoch_one)
    });
    poll("cancel acknowledged", Duration::from_secs(10), || {
        let shown = show(&cli, &sleeping);
        let delivery = field(&shown, "currentDelivery")?;
        (!delivery["cancelState"].is_null()).then_some(())
    });
    assert_eq!(line_count(&counter), 1, "Task A must not run twice");

    shutdown(&mut second.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}
