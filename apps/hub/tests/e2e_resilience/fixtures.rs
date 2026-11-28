//! Task, Worker, and `worker.yaml` fixtures shared by the cross-process scenarios.

use std::path::Path;

use crate::harness::{field, worker_json, Cli};

pub(crate) fn status(value: &serde_json::Value) -> &str {
    field(value, "status")
        .and_then(|status| status.as_str())
        .unwrap_or("")
}

/// Submit one authorized Task pinned to `executor` and return its id.
pub(crate) fn create_task(
    cli: &Cli<'_>,
    key: &str,
    title: &str,
    capability: &str,
    executor: &str,
) -> String {
    submit_task(cli, key, title, "resilience", capability, executor, None)
}

pub(crate) fn show(cli: &Cli<'_>, id: &str) -> serde_json::Value {
    cli.run(&["--json", "tasks", "show", id])
}

/// The Task record once it has finished, else `None`.
pub(crate) fn finished(cli: &Cli<'_>, id: &str) -> Option<()> {
    (status(&show(cli, id)) == "finished").then_some(())
}

/// Submit one authorized Task with an explicit execution timeout.
pub(crate) fn create_task_with_timeout(
    cli: &Cli<'_>,
    key: &str,
    title: &str,
    executor: &str,
    timeout_seconds: u64,
) -> String {
    submit_task(
        cli,
        key,
        title,
        "long duration",
        "shell",
        executor,
        Some(timeout_seconds),
    )
}

pub(crate) fn submit_task(
    cli: &Cli<'_>,
    key: &str,
    title: &str,
    prompt: &str,
    capability: &str,
    executor: &str,
    timeout_seconds: Option<u64>,
) -> String {
    let timeout = timeout_seconds.map(|seconds| seconds.to_string());
    let mut args = vec![
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
        capability,
        "--preferred-executor-id",
        executor,
    ];
    if let Some(timeout) = timeout.as_deref() {
        args.extend(["--timeout-seconds", timeout]);
    }
    let created = cli.run(&args);
    assert_eq!(status(&created), "queued", "submit {key}: {created}");
    field(&created, "id")
        .and_then(|id| id.as_str())
        .expect("id")
        .to_owned()
}

pub(crate) fn clear_trust_gates(cli: &Cli<'_>) {
    let worker_id = worker_json(cli)["workerId"]
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

pub(crate) fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

pub(crate) fn gpu_yaml(hub_url: &str) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: gpu-slot
    capabilities: [{name: gpu, state: available}]
    executors:
      - {id: exec-gpu, kind: process, program: /bin/sh, args: ['-c', 'p=$(sed -n ''s/.*to \(\/.*\.json\).*/\1/p'' | head -n1); printf ''{"text":"gpu queued ok"}'' > "$p"']}
"#;
    document.replace("HUB_URL", hub_url)
}

pub(crate) fn resync_yaml(hub_url: &str, counter: &Path) -> String {
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

pub(crate) fn silent_long_yaml(hub_url: &str, launches: &Path) -> String {
    let document = r#"hubUrl: 'HUB_URL'
slots:
  - slotId: long-slot
    capabilities: [{name: shell, state: available}]
    executors:
      - id: exec-long
        kind: process
        program: /bin/sh
        args:
          - -c
          - |
            umask 077
            echo launch >> LAUNCH_COUNTER
            result=""
            while IFS= read -r line; do
              case "$line" in
                'Optionally write the final Markdown result '*) result="${line##* to }" ;;
              esac
            done
            sleep 2
            printf '# Long task result\n\nNo progress was ever written.\n' > "$result"
"#;
    document
        .replace("HUB_URL", hub_url)
        .replace("LAUNCH_COUNTER", &launches.display().to_string())
}
