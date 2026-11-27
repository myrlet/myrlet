# Developer Quickstart

Local source-development setup. For deployment, see [Deployment](../deployment/README.md).

## Prerequisites

- Git and stable Rust (`rustfmt`, `clippy`).
- A free local port (default Hub port: `4140`).
- No LLM key is required for workspace checks or Process Executor tests.

## 1. Verify Workspace

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

## 2. Start Local Hub

Use an absolute `MYRLET_HOME` outside the Git checkout (startup rejects roots inside a repository):

```bash
export MYRLET_HOME="$HOME/.local/share/myrlet"
export MYRLET_ADMIN_TOKEN='replace-with-local-admin-token'

cargo run -p myrlet-hub            # runs the `hub` binary
```

Check liveness:

```bash
curl --fail-with-body http://127.0.0.1:4140/v1/health
```

## 3. Enroll and Start Worker

In a second terminal (export the same `MYRLET_HOME` and `MYRLET_ADMIN_TOKEN`):

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'

# `-p myrlet-cli` runs the `myrlet` binary; `-p myrlet-worker-daemon` runs `worker`.
code="$(cargo run --quiet -p myrlet-cli -- --json workers enroll | jq -er '.enrollmentCode')"
cargo run -p myrlet-worker-daemon -- pair --hub-url "$MYRLET_HUB_URL" --code "$code"
```

Create `$MYRLET_HOME/worker/worker.yaml`, validate it, and run the Worker:

```bash
cargo run -p myrlet-worker-daemon -- check-config
cargo run -p myrlet-worker-daemon
```

## 4. Submit a Task

In a third terminal:

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'
export MYRLET_ADMIN_TOKEN='replace-with-local-admin-token'

cargo run -p myrlet-cli -- tasks submit \
  --idempotency-key local-task-1 \
  --title 'Verify local Worker' \
  --prompt 'Run the configured Executor'

cargo run -p myrlet-cli -- events watch
```

Submission authorizes the Task: with no earlier prerequisites it becomes `queued`, the Hub wakes an eligible Worker, and that Worker claims it for a free Slot. A Task whose prerequisites are not all `finished` instead stays `held` and lists its `blockedBy`.

A Harness may optionally write the final Markdown result to `$MYRLET_HOME/worker/results/<delivery-id>.md` (owner-only, up to 64 KiB). The Worker reconciles the latest content to the Hub; `tasks show <task-id>` and the Web Task Detail display it even if the Worker later goes offline. The Harness asks the Worker to sync and finish by running `myrlet done <delivery-id>` before exit.

## Next Steps

- [Architecture](../architecture/README.md) · [Owner Agent integration](agent-integration.md)
- [E2E testing](e2e.md) · [CI](ci.md)
- [Deployment](../deployment/README.md) — pre-built container images (`ghcr.io/myrlet/{hub,worker,myrlet}`) and the multi-stage `Dockerfile` targets.
