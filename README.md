<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/myrlet-mark-inverse.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/myrlet-mark.svg">
    <img src="assets/brand/myrlet-mark.svg" alt="Myrlet, the ant-inspired collaboration mark" width="168">
  </picture>
</p>

<h1 align="center">Myrlet</h1>

<p align="center">
  <strong>A self-hosted task broker for coding agents working across machines.</strong><br>
  Give your agents a way to delegate work to environments with the tools and permissions it needs.
</p>

<p align="center">
  <a href="https://github.com/myrlet/myrlet/actions"><img src="https://img.shields.io/badge/status-pre--release%200.1.0-f59e0b" alt="Pre-release 0.1.0"></a>
  <img src="https://img.shields.io/badge/written%20in-Rust-000000?logo=rust" alt="Written in Rust">
  <img src="https://img.shields.io/badge/storage-single%20SQLite-0f766e" alt="Single SQLite">
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-2563eb" alt="MIT or Apache-2.0"></a>
</p>

<p align="center">
  <a href="#quick-start">Quick Start</a> ·
  <a href="#why-myrlet">Why Myrlet</a> ·
  <a href="#how-it-works">How It Works</a> ·
  <a href="#4-or-run-from-containers">Docker</a> ·
  <a href="docs/architecture/README.md">Architecture</a> ·
  <a href="skills/myrlet/SKILL.md">Agent Skill</a>
</p>

---

## Why Myrlet

Coding agents can already decide when to split a task, ask for a second opinion, or run a test on another machine. Myrlet gives them a small CLI for acting on those decisions. An owner agent can inspect available workers, create a task, choose where to send it, and check what happened. As agents get better at making those choices, they can use the same commands in new ways.

### An independent review on another machine

Suppose Codex has prepared a patch on your Mac and decides that a change to authentication deserves a second review. It can use `myrlet` to send a Linux worker a task naming the repository, base and head commits, and the question it wants Claude Code to investigate. The operator gives that worker read access to the repository and a writable directory for progress. The reviewer uses its own tools and session. Codex can read the reported findings and decide whether to revise the patch.

The commit references matter. Each harness owns its workspace and conversation. The task supplies the context the next agent needs. A thin adapter starts or cancels the harness, which keeps its existing configuration, credentials, and skills. A harness can optionally write a small JSON progress file for the worker to relay.

### Give each worker the access its work requires

A Mac with Xcode, a Linux test runner, and a read-only review environment offer different capabilities. We recommend a separate Docker worker for each permission boundary where the toolchain supports containers. Run the harness inside that boundary. Mount only the files it needs, scope its network access, and supply only its own credentials. You can provision those credentials with an external secrets manager such as Infisical.

Slots advertise where work can run. The OS or container enforces access. Keep the Hub Admin token with trusted operators: it authorizes Worker enrollment, credential and permission-tier management, and Hub configuration. A Worker credential receives a server-controlled `worker` or `owner` tier. A `worker`-tier Worker can read only Tasks bound to its own Delivery plus direct related-Task summaries. An `owner`-tier Worker can coordinate all Tasks and observe Worker and Worker-reported Agent runtime state, but cannot administer Workers. The CLI supports all three tiers; Web and other non-CLI clients require Admin. [Security model](docs/architecture/security.md)

## How It Works

1. **Submit a task.** A human or owner agent submits a task, which authorizes it. One Hub keeps the task tree, dependencies, and worker inventory in SQLite. A task whose prerequisites have not finished stays `held` and reports which ones block it.
2. **A worker claims it.** A ready (`queued`) task waits for an eligible worker. The Hub wakes one eligible worker; the worker chooses a free slot and explicitly claims the task, and the Hub commits exactly one delivery. Workers connect outbound to the Hub over WSS with TLS verification for remote deployments.
3. **Observe the handoff.** Before starting a harness, the worker records the delivery ID so a replay cannot start it twice. It relays optional Progress and the latest Markdown Result, and reports when handling ends. Read the stored Result with `myrlet tasks show <id>`. A disconnected worker may still be executing. A `finished` task tells its owner when to inspect the work and decide what comes next.

## Quick Start

> **Pre-release (`0.1.0`)**: Build from source with stable Rust (`cargo`), or run the pre-built [containers](#4-or-run-from-containers). Set `MYRLET_HOME` to an absolute path **outside** any Git checkout.

### 1. Build & Start the Hub

```bash
# Builds `hub`, `worker`, and `myrlet` into target/release/
cargo build --release -p myrlet-hub -p myrlet-worker-daemon -p myrlet-cli

export MYRLET_HOME="$HOME/.local/share/myrlet"
export MYRLET_ADMIN_TOKEN="$(openssl rand -hex 32)"

./target/release/hub
```

### 2. Enroll & Start a Worker

For this loopback demo, open a second terminal with the same `MYRLET_HOME` and `MYRLET_ADMIN_TOKEN`.

```bash
export MYRLET_HUB_URL="http://127.0.0.1:4140"

# Issue a short-lived one-time pairing code and pair in one step
code="$(./target/release/myrlet --json workers enroll | jq -er '.enrollmentCode')"
./target/release/worker pair --hub-url "$MYRLET_HUB_URL" --code "$code"
```

Create `$MYRLET_HOME/worker/worker.yaml` (supports `claude`, `codex`, `pi`, and `process` executors):

```yaml
hubUrl: http://127.0.0.1:4140
slots:
  - slotId: mac-default
    roles: [coding, build]
    capabilities:
      - name: rust
        state: available
    executors:
      - kind: claude
        id: claude-code
      - kind: codex
        id: codex-cli
      - kind: pi
        id: pi-cli
```

Set private file permissions, then validate and start the worker with its paired credential.

```bash
chmod 600 "$MYRLET_HOME/worker/worker.yaml"
unset MYRLET_ADMIN_TOKEN
./target/release/worker check-config
./target/release/worker
```

A new Worker is `pending` until an Admin approves it, and its first scheduling report is
applied explicitly. In the owner terminal:

```bash
worker_id="$(./target/release/myrlet --json workers list | jq -er '.workers[0].workerId')"
./target/release/myrlet --json workers approve "$worker_id"
revision="$(./target/release/myrlet --json workers show "$worker_id" | jq -er '.worker.reportedRevision')"
./target/release/myrlet --json workers config apply "$worker_id" --revision "$revision"
```

Both gates default on and can be changed per installation under Web **Settings → Security
& Trust** or `PATCH /v1/settings/security-trust`. Changing a policy never alters an
existing approval or a pending report.

### 3. Submit & Watch a Task

In an owner terminal, set the same Hub URL and admin token used for enrollment.

```bash
# Submit a Task; submission authorizes it for execution
task_id="$(./target/release/myrlet --json tasks submit \
  --idempotency-key demo-1 \
  --title 'Run workspace tests' \
  --prompt 'Run cargo test and write a short summary to the Progress file.' \
  | jq -er '.task.id')"

# Stream live invalidation events or inspect authoritative Task state
./target/release/myrlet --json events watch
./target/release/myrlet tasks show "$task_id"
```

Want an AI coding agent to drive Myrlet directly? Drop [skills/myrlet/SKILL.md](skills/myrlet/SKILL.md) into your agent's skill directory.

### 4. Or Run from Containers

The multi-stage [`Dockerfile`](Dockerfile) exposes one target per binary (`hub`, `worker`, `myrlet`), and [`docker.yml`](.github/workflows/docker.yml) publishes matching images to GHCR:

```bash
docker pull ghcr.io/myrlet/hub:latest
docker pull ghcr.io/myrlet/worker:latest
docker pull ghcr.io/myrlet/myrlet:latest

docker run --rm -e MYRLET_ADMIN_TOKEN="$MYRLET_ADMIN_TOKEN" \
  -v /var/lib/myrlet:/var/lib/myrlet -p 4140:4140 ghcr.io/myrlet/hub:latest
```

The Hub image includes the web UI. The worker image includes the daemon and CLI. Supply the harness and toolchain for each worker environment. The all-in-one `myrlet` image runs a Hub and a paired local worker for trying the system on one machine. [Deployment guide](docs/deployment/README.md)

## Documentation

- **[Developer Quickstart](docs/development/getting-started.md)** · local setup, verification commands, and end-to-end walkthrough
- **[Deployment Guide](docs/deployment/README.md)** · native binaries, containers, GHCR images, TLS (`MYRLET_HUB_CA_FILE`), backup, and restore
- **[Architecture Contract](docs/architecture/README.md)** · normative V1 specs for [Task Model](docs/architecture/task-model.md), [Hub API](docs/architecture/hub-api.md), [Worker Protocol](docs/architecture/worker-protocol.md), [Security](docs/architecture/security.md), and [Local Runtime](docs/architecture/local-runtime.md)
- **[Documentation Index](docs/README.md)** · **[Brand Assets](assets/brand/README.md)**

---

## Project Status & Contributing

Myrlet is in active **pre-release (`0.1.0`)** development. Wire and storage formats are evolving without backward-compatibility shims until `0.1.0` ships.

- Contributions, issues, and architecture proposals are welcome · please read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [SUPPORT.md](SUPPORT.md).
- Dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option.
