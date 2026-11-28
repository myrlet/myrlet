//! All-in-one deployment regression: execute the **actual** `docker/myrlet-start.sh`
//! entrypoint against freshly built binaries, and prove the local Worker is approved and
//! applied by its own identity on a fresh install and again on a restart — without the
//! script ever touching the Hub's Security & Trust policies or another Worker.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use myrlet_protocol::ensure_private_dir;

use crate::harness::{binaries, ADMIN_TOKEN};

/// The entrypoint shipped in the image, located relative to this crate.
fn entrypoint() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker/myrlet-start.sh")
        .canonicalize()
        .expect("docker/myrlet-start.sh exists")
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind port");
    listener.local_addr().expect("addr").port()
}

/// One running `myrlet-start` instance and the Hub it supervises.
struct Entrypoint {
    child: Option<Child>,
    hub_url: String,
}

impl Entrypoint {
    fn start(hub: &Path, worker: &Path, cli: &Path, home: &Path, bind: &str) -> Self {
        let port = bind.rsplit(':').next().expect("port");
        let hub_url = format!("http://127.0.0.1:{port}");
        let child = Command::new("/bin/bash")
            .arg(entrypoint())
            .env("MYRLET_HOME", home)
            .env("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN)
            .env("MYRLET_HUB_BIND", bind)
            .env("MYRLET_HUB_BIN", hub)
            .env("MYRLET_WORKER_BIN", worker)
            .env("MYRLET_CLI_BIN", cli)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn myrlet-start");
        Self {
            child: Some(child),
            hub_url,
        }
    }

    /// One `myrlet` CLI call against this instance's Hub, parsed as JSON.
    ///
    /// Returns `None` while the Hub is still starting (a transport failure), so a poll can
    /// retry; a non-transport failure panics with the CLI's message.
    fn cli(&self, cli: &Path, home: &Path, args: &[&str]) -> Option<serde_json::Value> {
        let output = Command::new(cli)
            .args(args)
            .env("MYRLET_HOME", home)
            .env("MYRLET_HUB_URL", &self.hub_url)
            .env("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN)
            .stdin(Stdio::null())
            .output()
            .expect("run myrlet cli");
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("transport failure") || stderr.contains("connect") {
                return None; // the Hub is not listening yet
            }
            panic!("myrlet {args:?} failed: {stderr}");
        }
        serde_json::from_slice(&output.stdout).ok()
    }

    /// Poll until `probe` returns a value, feeding it a CLI runner that returns `None`
    /// while the Hub is still starting.
    fn poll<T>(
        &self,
        cli: &Path,
        home: &Path,
        what: &str,
        limit: Duration,
        mut probe: impl FnMut(&dyn Fn(&[&str]) -> Option<serde_json::Value>) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            let run = |args: &[&str]| self.cli(cli, home, args);
            if let Some(value) = probe(&run) {
                return value;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("timed out waiting for {what}");
    }

    /// Stop the entrypoint with SIGTERM; it forwards to both daemons and exits cleanly.
    fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if child.try_wait().expect("try_wait").is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("myrlet-start did not exit within 10s");
    }
}

/// Enroll and pair one extra Worker against the entrypoint's Hub, returning its id.
///
/// Uses the Admin CLI to issue a code and the Worker binary to pair into its own home, so
/// it never shares the entrypoint's identity.
fn enroll_extra_worker(entry: &Entrypoint, cli: &Path, worker: &Path, other_home: &Path) -> String {
    let issued = {
        let output = Command::new(cli)
            .args(["--json", "workers", "enroll"])
            .env("MYRLET_HOME", other_home)
            .env("MYRLET_HUB_URL", &entry.hub_url)
            .env("MYRLET_ADMIN_TOKEN", ADMIN_TOKEN)
            .stdin(Stdio::null())
            .output()
            .expect("run myrlet workers enroll");
        assert!(
            output.status.success(),
            "workers enroll failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("enroll json")
    };
    let code = issued["enrollmentCode"].as_str().expect("enrollment code");
    let output = Command::new(worker)
        .args(["pair", "--hub-url", &entry.hub_url, "--code", code])
        .env("MYRLET_HOME", other_home)
        .output()
        .expect("run worker pair");
    assert!(
        output.status.success(),
        "worker pair failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let creds = std::fs::read_to_string(other_home.join("worker/credentials.json"))
        .expect("other worker credentials");
    let parsed: serde_json::Value = serde_json::from_str(&creds).expect("credentials json");
    parsed["workerId"].as_str().expect("workerId").to_owned()
}

impl Drop for Entrypoint {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[tokio::test]
async fn the_entrypoint_converges_its_local_worker_on_fresh_boot_and_restart() {
    let (hub_bin, worker_bin, cli_bin) = binaries();
    let directory = tempfile::tempdir().expect("temp dir");
    let home = directory.path().join("home");
    ensure_private_dir(&home).expect("create home");
    let bind = format!("127.0.0.1:{}", free_port());

    // --- Fresh install -------------------------------------------------------
    let mut entry = Entrypoint::start(&hub_bin, &worker_bin, &cli_bin, &home, &bind);

    // The entrypoint pairs, approves, and applies its local Worker on its own.
    let (worker, local_id) = entry.poll(
        &cli_bin,
        &home,
        "the local worker approved and applied",
        Duration::from_secs(60),
        |run| {
            let list = run(&["--json", "workers", "list"])?;
            let worker = list["workers"].as_array()?.first()?.clone();
            let online = worker["online"].as_bool()?;
            let approved = worker["approval"] == "approved";
            let applied = worker["configurationPending"] == false;
            let id = worker["workerId"].as_str()?.to_owned();
            (online && approved && applied).then_some((worker, id))
        },
    );
    assert!(local_id.starts_with("wkr_"), "a workerId: {worker}");
    let detail = entry
        .cli(&cli_bin, &home, &["--json", "workers", "show", &local_id])
        .expect("workers show");
    assert_eq!(
        detail["appliedRevision"], detail["reportedRevision"],
        "the local worker's configuration is applied: {detail}"
    );

    // The Hub's policies are untouched: both still on.
    let policies = entry
        .cli(&cli_bin, &home, &["--json", "policy", "show"])
        .expect("policy show");
    assert_eq!(policies["requireWorkerApproval"], true);
    assert_eq!(policies["requireConfigurationApply"], true);

    // The identity lives in the local credential file the entrypoint wrote.
    let creds = std::fs::read_to_string(home.join("worker/credentials.json"))
        .expect("credentials written by the entrypoint");
    assert!(
        creds.contains(&local_id),
        "credentials name the local worker"
    );

    // A separately enrolled Worker joins the same Hub (its own home). It must stay subject
    // to the approval policy: enrolled via the CLI here, it never runs through the
    // entrypoint and must remain pending.
    let other_home = directory.path().join("other-home");
    ensure_private_dir(&other_home).expect("create other home");
    let other_id = enroll_extra_worker(&entry, &cli_bin, &worker_bin, &other_home);
    assert_ne!(other_id, local_id);
    let other = entry
        .cli(&cli_bin, &home, &["--json", "workers", "show", &other_id])
        .expect("other workers show");
    assert_eq!(other["worker"]["approval"], "pending", "other: {other}");

    entry.stop();

    // --- Restart on the same persistent data directory -----------------------
    let mut restarted = Entrypoint::start(&hub_bin, &worker_bin, &cli_bin, &home, &bind);
    // Identity is recovered from the credential file, not re-paired; convergence is a
    // no-op that must still succeed and must not enrol a second Worker.
    let (after, restarted_id) = restarted.poll(
        &cli_bin,
        &home,
        "the same local worker approved after restart",
        Duration::from_secs(60),
        |run| {
            let list = run(&["--json", "workers", "list"])?;
            let workers = list["workers"].as_array()?;
            // The restart reuses the paired identity and enrolls no third Worker.
            if workers.len() != 2 {
                return None;
            }
            let worker = workers
                .iter()
                .find(|w| w["workerId"].as_str() == Some(local_id.as_str()))?
                .clone();
            let online = worker["online"].as_bool()?;
            let approved = worker["approval"] == "approved";
            let id = worker["workerId"].as_str()?.to_owned();
            (online && approved).then_some((worker, id))
        },
    );
    assert_eq!(restarted_id, local_id, "restart reuses the paired identity");
    assert_eq!(
        after["configurationPending"], false,
        "the restart converges the existing configuration without changing it: {after}"
    );
    // The separately enrolled Worker is untouched by the restart: still pending, not applied.
    let other = restarted
        .cli(&cli_bin, &home, &["--json", "workers", "show", &other_id])
        .expect("other workers show");
    assert_eq!(
        other["worker"]["approval"], "pending",
        "the restart never approves another Worker: {other}"
    );
    assert!(
        other["appliedRevision"].is_null(),
        "the other Worker is not applied: {other}"
    );
    let policies = restarted
        .cli(&cli_bin, &home, &["--json", "policy", "show"])
        .expect("policy show");
    assert_eq!(policies["requireWorkerApproval"], true);
    assert_eq!(policies["requireConfigurationApply"], true);

    restarted.stop();
}
