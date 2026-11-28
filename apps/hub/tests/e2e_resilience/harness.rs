//! Shared cross-process harness: real binaries, spawned daemons, a loopback Hub, and a
//! `myrlet` CLI bound to one disposable home.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) const ADMIN_TOKEN: &str = "e2e-secret";

/// RAII wrapper so a panicking test never leaks a daemon process.
pub(crate) struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn binaries() -> (PathBuf, PathBuf, PathBuf) {
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

pub(crate) fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
    listener.local_addr().expect("addr").port()
}

/// Read `key` from a CLI document, falling back to a nested `task` object.
pub(crate) fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value.get(key).or_else(|| value.get("task")?.get(key))
}

/// A `myrlet` CLI bound to one home directory and hub URL.
pub(crate) struct Cli<'a> {
    pub(crate) bin: &'a Path,
    pub(crate) home: &'a Path,
    pub(crate) hub_url: &'a str,
}

pub(crate) fn cli<'a>(bin: &'a Path, home: &'a Path, hub_url: &'a str) -> Cli<'a> {
    Cli { bin, home, hub_url }
}

impl Cli<'_> {
    /// Run a subcommand, asserting a clean exit, and parse its stdout JSON.
    pub(crate) fn run(&self, args: &[&str]) -> serde_json::Value {
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
pub(crate) fn run_worker(worker: &Path, home: &Path, args: &[&str]) {
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
pub(crate) fn poll<T>(what: &str, limit: Duration, mut probe: impl FnMut() -> Option<T>) -> T {
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
pub(crate) fn shutdown(child: &mut Child, name: &str) {
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
pub(crate) fn spawn_daemon(bin: &Path, envs: &[(&str, &str)]) -> ChildGuard {
    let mut command = Command::new(bin);
    command
        .envs(envs.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    ChildGuard(command.spawn().expect("spawn daemon"))
}

/// Poll the hub health endpoint until it answers `200 OK` (max 10s).
pub(crate) async fn wait_healthy(hub_url: &str) {
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
pub(crate) async fn start_hub(hub: &Path, home: &Path) -> (ChildGuard, String) {
    let port = free_port();
    let url = format!("http://127.0.0.1:{port}");
    let guard = start_hub_on_port(hub, home, port).await;
    (guard, url)
}

/// Start a real hub on an already-chosen port so a restart can reuse the same URL.
pub(crate) async fn start_hub_on_port(hub: &Path, home: &Path, port: u16) -> ChildGuard {
    let bind = format!("127.0.0.1:{port}");
    let url = format!("http://127.0.0.1:{port}");
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
    guard
}

/// Enroll a fresh Worker and pair the real binary with its one-shot code.
pub(crate) fn enroll_and_pair(cli: &Cli<'_>, worker: &Path, home: &Path, hub_url: &str) {
    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode").and_then(|code| code.as_str());
    run_worker(
        worker,
        home,
        &["pair", "--hub-url", hub_url, "--code", code.expect("code")],
    );
}

pub(crate) fn worker_json(cli: &Cli<'_>) -> serde_json::Value {
    cli.run(&["--json", "workers", "list"])["workers"][0].clone()
}

/// Epoch of the single Worker when it is online, else `None`.
pub(crate) fn online_epoch(cli: &Cli<'_>) -> Option<u64> {
    let worker = worker_json(cli);
    worker["online"]
        .as_bool()?
        .then_some(worker["connectionEpoch"].as_u64())?
}

pub(crate) fn worker_offline(cli: &Cli<'_>) -> bool {
    worker_json(cli)["online"].as_bool() == Some(false)
}

/// Approve the single enrolled Worker and apply the revision it reported, clearing both
/// default-on trust gates before any Task can be claimed.
pub(crate) fn json_file_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count()
}
