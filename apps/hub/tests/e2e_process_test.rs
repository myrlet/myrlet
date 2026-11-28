//! Cross-process end-to-end test driving the real `hub`, `worker`, and `myrlet` binaries.

use std::path::{Path, PathBuf};

use serde_json::json;
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

/// Read `key` from a CLI document, falling back to the Task record nested under `task`.
///
/// A `tasks show` response is `{ task, summary, related }`; the Task record itself also
/// nests its immutable definition under `task`.
fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value
        .get(key)
        .or_else(|| value.get("task")?.get(key))
        .or_else(|| value.get("task")?.get("task")?.get(key))
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

    /// Submit a Task pinned to one executor and assert it is authorized (`queued`).
    fn submit(&self, key: &str, title: &str, prompt: &str, executor: &str) -> String {
        let submitted = self.run(&[
            "--json",
            "tasks",
            "submit",
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
        assert_eq!(status(&submitted), "queued", "submit {key}: {submitted}");
        field(&submitted, "id")
            .and_then(serde_json::Value::as_str)
            .expect("task id")
            .to_owned()
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
        args:
          - -c
          - |
            umask 077
            while IFS= read -r line; do
              case "$line" in
                'Write optional JSON progress '*) printf '{"text":"e2e progress ok"}' > "${line##* to }" ;;
                'Optionally write the final Markdown result '*) printf '# Findings\n' > "${line##* to }" ;;
              esac
            done
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

/// Approve the single enrolled Worker and apply the revision it reported, clearing both
/// default-on trust gates before any Task can be claimed.
fn clear_trust_gates(cli: &Cli<'_>) {
    let worker_id = cli.run(&["--json", "workers", "list"])["workers"][0]["workerId"]
        .as_str()
        .expect("worker id")
        .to_owned();
    cli.run(&["--json", "workers", "approve", &worker_id]);
    let detail = cli.run(&["--json", "workers", "show", &worker_id]);
    let revision = detail["worker"]["reportedRevision"]
        .as_u64()
        .expect("the online Worker reported an inventory");
    cli.run(&[
        "--json",
        "workers",
        "config",
        "apply",
        &worker_id,
        "--revision",
        &revision.to_string(),
    ]);
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
    clear_trust_gates(&cli);

    // 6. Submit authorizes work; the Worker claims it after a wakeup and runs it.
    let first = cli.submit("e2e-1", "E2E progress", "Write progress", "exec-progress");
    let shown = poll("the submitted task to finish", || {
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

    // 7. Submit -> claim -> cancel while the Harness sleeps.
    let second = cli.submit("e2e-2", "E2E cancel", "Sleep", "exec-slow");
    poll("the second task to be claimed", || {
        (status(&cli.run(&["--json", "tasks", "show", &second])) == "dispatched").then_some(())
    });
    let cancel = ["--json", "tasks", "cancel", &second, "--reason", "stop e2e"];
    let cancelled = cli.run(&cancel);
    assert_eq!(status(&cancelled), "cancelled", "cancel: {cancelled}");
    let shown = cli.run(&["--json", "tasks", "show", &second]);
    assert_eq!(status(&shown), "cancelled", "show 2: {shown}");

    // Once the Hub has the result, reading it requires no connection to the producer.
    poll("Markdown result to sync", || {
        (field(
            &cli.run(&["--json", "tasks", "show", &first]),
            "resultMarkdown",
        ) == Some(&json!("# Findings\n")))
        .then_some(())
    });
    shutdown(&mut worker_guard.0, "worker");
    assert_eq!(
        field(
            &cli.run(&["--json", "tasks", "show", &first]),
            "resultMarkdown"
        ),
        Some(&json!("# Findings\n"))
    );
    shutdown(&mut hub_guard.0, "hub");
}

/// A Harness-style worker that writes a Result and calls `myrlet done` before exiting.
fn done_yaml(hub_url: &str, cli: &Path) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: e2e-slot
    capabilities:
      - name: shell
        state: available
    executors:
      - id: exec-done
        kind: process
        program: /bin/sh
        args:
          - -c
          - |
            umask 077
            delivery=""
            result=""
            while IFS= read -r line; do
              case "$line" in
                'Delivery ID: '*) delivery="${line#Delivery ID: }" ;;
                'Optionally write the final Markdown result '*) result="${line##* to }" ;;
              esac
            done
            printf '# Harness result\n\n- done via CLI\n' > "$result"
            "$MYRLET_CLI" done "$delivery" > /dev/null 2>&1 || printf 'done command failed\n' >> "$result"
"#;
    document
        .replace("HUB_URL", hub_url)
        .replace("$MYRLET_CLI", &cli.display().to_string())
}

#[tokio::test]
async fn a_harness_can_call_done_and_clear_its_result_after_finish() {
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
    let enrolled = cli.run(&["--json", "workers", "enroll"]);
    let code = field(&enrolled, "enrollmentCode")
        .and_then(|code| code.as_str())
        .expect("enrollment code");
    run_worker(
        &worker_bin,
        &home,
        &["pair", "--hub-url", &hub_url, "--code", code],
    );
    std::fs::write(
        home.join("worker/worker.yaml"),
        done_yaml(&hub_url, &cli_bin),
    )
    .expect("write worker.yaml");
    let mut worker_guard = spawn_daemon(&worker_bin, &[("MYRLET_HOME", home.to_str().unwrap())]);
    poll("the worker to come online", || {
        let listed = cli.run(&["--json", "workers", "list"]);
        (listed["workers"][0]["online"] == serde_json::json!(true)).then_some(())
    });
    clear_trust_gates(&cli);

    let task = cli.submit("done-1", "Done via CLI", "Write the result", "exec-done");
    // `done` syncs the Result independently of Finish, so the Result can arrive while
    // the Task is still `dispatched`; wait for the Worker-observed exit separately.
    poll("the harness result to reach the Hub", || {
        let shown = cli.run(&["--json", "tasks", "show", &task]);
        (field(&shown, "resultMarkdown") == Some(&json!("# Harness result\n\n- done via CLI\n")))
            .then_some(())
    });
    let shown = poll("worker exit to finish the Task", || {
        let shown = cli.run(&["--json", "tasks", "show", &task]);
        (status(&shown) == "finished").then_some(shown)
    });

    // The Harness deleted its file; a later sync after Finish clears the Hub copy.
    let delivery_id = field(&shown, "currentDelivery")
        .and_then(|delivery| delivery["id"].as_str())
        .expect("delivery");
    let result_path = home
        .join("worker/results")
        .join(format!("{delivery_id}.md"));
    assert!(result_path.exists(), "harness wrote its result file");
    std::fs::remove_file(&result_path).expect("delete result file");
    let cleared = cli.run(&["result", "sync", delivery_id, "--json"]);
    assert!(
        cleared["hubConfirmed"].as_bool() == Some(true),
        "sync confirmed: {cleared}"
    );
    poll("the Hub result to clear", || {
        (field(
            &cli.run(&["--json", "tasks", "show", &task]),
            "resultMarkdown",
        ) == Some(&serde_json::Value::Null))
        .then_some(())
    });

    shutdown(&mut worker_guard.0, "worker");
    shutdown(&mut hub_guard.0, "hub");
}
