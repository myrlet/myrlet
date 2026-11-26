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
python3 scripts/check-doc-links.py
```

## 2. Start Local Hub

Use an absolute `MYRLET_HOME` outside the Git checkout (startup rejects roots inside a repository):

```bash
export MYRLET_HOME="$HOME/.local/share/myrlet"
export MYRLET_ADMIN_TOKEN='replace-with-local-admin-token'

cargo run -p myrlet-hub-daemon
```

Check liveness:

```bash
curl --fail-with-body http://127.0.0.1:4140/v1/health
```

## 3. Enroll and Start Worker

In a second terminal (export the same `MYRLET_HOME` and `MYRLET_ADMIN_TOKEN`):

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'
export WORKER_ID='worker-local-dev'

code="$(curl --fail --silent --show-error \
  --request POST "$MYRLET_HUB_URL/v1/enrollment-codes" \
  --header "Authorization: Bearer $MYRLET_ADMIN_TOKEN" \
  --header 'Content-Type: application/json' \
  --data "{\"ttlSeconds\":300,\"suggestedWorkerId\":\"$WORKER_ID\"}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["enrollmentCode"])')"

curl --fail-with-body --silent --show-error \
  --request POST "$MYRLET_HUB_URL/v1/pair" \
  --header 'Content-Type: application/json' \
  --data "{\"enrollmentCode\":\"$code\",\"workerId\":\"$WORKER_ID\"}" \
  | cargo run --quiet -p myrlet-worker-daemon -- credential-import
```

Create `$MYRLET_HOME/worker/worker.yaml`, validate it, and run the Worker:

```bash
cargo run -p myrlet-worker-daemon -- check-config
export MYRLET_HUB_WSS_URL='ws://127.0.0.1:4140/v1/workers/connect'
cargo run -p myrlet-worker-daemon
```

## 4. Create and Dispatch a Task

In a third terminal:

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'
export MYRLET_ADMIN_TOKEN='replace-with-local-admin-token'

cargo run -p myrlet-cli -- tasks create \
  --idempotency-key local-task-1 \
  --title 'Verify local Worker' \
  --prompt 'Run the configured Executor'

cargo run -p myrlet-cli -- tasks dispatch <task-id> \
  --expected-version 1 \
  --idempotency-key local-dispatch-1 \
  --match

cargo run -p myrlet-cli -- events watch
```

## Next Steps

- [Architecture](../architecture/README.md) · [Owner Agent integration](agent-integration.md)
- [E2E testing](e2e.md) · [CI](ci.md)
