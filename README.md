<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/myrlet-mark-inverse.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/myrlet-mark.svg">
    <img src="assets/brand/myrlet-mark.svg" alt="Myrlet, the ant-inspired collaboration mark" width="168">
  </picture>
</p>

<h1 align="center">Myrlet</h1>

<p align="center">
  <strong>The task broker for whatever comes next.</strong>
</p>

<p align="center">
  A lightweight, self-hosted Rust task broker connecting humans, owner agents, and coding harnesses<br>
  (<strong>Claude Code</strong>, <strong>Codex</strong>, <strong>Pi</strong>, and plain processes) across all of your machines.<br>
  <strong>Your machines. Your agents. Your workflows.</strong>
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

Your Mac builds iOS apps. Your Linux server runs containers. Your workstation has a GPU. Your AI coding agents keep getting better every month—but wiring them across machines still means brittle SSH loops, bloated DAG platforms, or proprietary cloud lock-in.

Myrlet gives your fleet one durable, boring, self-hosted **Task Broker**:

- **Zero-intrusion Harness integration** — Launches **Claude Code**, **Codex**, **Pi**, or any CLI/process natively. No harness forks, no hook/MCP injection, no touching your `CLAUDE.md` or LLM API keys.
- **Works behind NAT & firewalls** — Workers connect *outbound* to one Hub over verified TLS (`wss://`). Worker hosts need zero inbound open ports.
- **Single Hub, single SQLite file** — Pure Rust control plane. No Redis, no Kafka, no Postgres cluster, no LLM calls inside the broker.
- **Honest 5-state lifecycle** — `held` → `queued` / `dispatched` → `finished` | `cancelled`. Creation never auto-dispatches, and Hub never pretends to judge execution success or failure.
- **Safe under retries & disconnects** — `Idempotency-Key`, optimistic `expectedTaskVersion`, monotonic WSS `connectionEpoch` fencing, and local atomic Delivery registration prevent silent double-execution.
- **Built for both Humans & Owner Agents** — First-class `myrlet --json` CLI, resumable SSE event stream, native/WASM Rust SDK, and an agent skill ready to drop into your orchestrator.

> **Agents decide. Workers execute. Myrlet keeps the work moving.**

---

## How It Works

```text
  Humans · Owner Agents · CI · Apps
                 │
      HTTP API + SSE (/v1/events)
                 ▼
  ┌──────────────────────────────┐
  │             hub              │   One Rust binary · One SQLite DB
  │  Tasks · Deliveries · Queue  │   Constant-time SHA-256 Auth
  └──────────────┬───────────────┘
                 │
    Outbound WSS │ (offers, cancel, resync)
      + Hub HTTP │ (optional Progress, Finish)
                 ▼
  ┌──────────────────────────────┐
  │            worker            │   No inbound ports · Verified TLS
  │  Local Registry · Progress   │   2-method Adapter (submit/cancel)
  └──────────────┬───────────────┘
                 │
                 ▼
    Claude Code · Codex · Pi · Any Process
    (reads/acts on Myrlet via `myrlet` CLI)
```

1. **Create as `held`**: Every Task starts durable in `held` (including atomic Task Tree bundles up to 100 Tasks and 5 levels deep).
2. **Dispatch explicitly**: Target a specific `(workerId, slotId)` or let deterministic **Simple Match** pick an eligible online Slot by required capabilities, preferred role/executor, and load—or enqueue FIFO until a Worker is ready.
3. **Run natively, report minimally**: The Worker durably registers the `deliveryId` before starting the Harness (so replays never double-submit), optionally relays one bounded local Progress JSON file (`firstReceivedAtMs` = execution-start evidence), and reports `finished` over Hub HTTP when handling ends.

---

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

In a second terminal (with the same `MYRLET_HOME` and `MYRLET_ADMIN_TOKEN`):

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

Validate and start the Worker:

```bash
./target/release/worker check-config
./target/release/worker
```

### 3. Create, Dispatch & Watch a Task

```bash
# Create a held Task
task_id="$(./target/release/myrlet --json tasks create \
  --idempotency-key demo-1 \
  --title 'Run workspace tests' \
  --prompt 'Run cargo test and write a short summary to the Progress file.' \
  | jq -er '.id')"

# Dispatch via deterministic Simple Match
./target/release/myrlet --json tasks dispatch "$task_id" \
  --expected-version 1 \
  --idempotency-key demo-dispatch-1 \
  --match

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

Images are lean and headless (release binaries + `ca-certificates` on `debian:bookworm-slim`, running as unprivileged UID `10001`) — no Python, no toolchain, no demo fixtures. See the [Deployment guide](docs/deployment/README.md).

---

## What Myrlet Is — and Is Not

Myrlet stays lean on purpose. Every boundary in V1 is explicit:

| Myrlet **IS** | Myrlet **IS NOT** |
| --- | --- |
| A durable, single-Hub cross-host **Task Broker** | A workflow/DAG engine, planner, or autonomous agent |
| A thin, 2-method (`submit`/`cancel`) **Harness launcher** | A Harness fork, MCP injector, or Session/Workspace manager |
| An **honest state & Delivery tracker** (5 states, no verdict) | A result verifier, approval engine, or LLM gateway |
| A **control-plane security boundary** (SHA-256 auth, mandatory WSS TLS, `0600` files) | An OS/container sandbox (apply OS users/containers/VMs externally) |

---

## Workspace & Surfaces

V1 (`0.1.0`) ships **three pure-Rust binaries** over four small, single-responsibility crates:

| Binary | Package / Path | Role |
| --- | --- | --- |
| **`hub`** | `myrlet-hub` · [apps/hub](apps/hub) | Single-SQLite Hub daemon (HTTP API, SSE `/v1/events`, Worker WSS server, FIFO queue). Image: `ghcr.io/myrlet/hub` |
| **`worker`** | `myrlet-worker-daemon` · [apps/worker](apps/worker) | Outbound WSS Worker daemon, pairing, verified TLS, Progress/Finish HTTP reporter. Image: `ghcr.io/myrlet/worker` |
| **`myrlet`** | `myrlet-cli` · [apps/cli](apps/cli) | Headless CLI (`--json` & human output) for humans, scripts, and Harnesses. Image: `ghcr.io/myrlet/myrlet` |

Shared foundations:

| Component | Path | Role |
| --- | --- | --- |
| **Shared Crates** | `crates/{protocol,store,worker,client}` | Canonical wire DTOs, 8-table SQLite store, local Delivery registry & adapters, native/WASM HTTP+SSE SDK |
| **Agent Skill** | [skills/myrlet/SKILL.md](skills/myrlet/SKILL.md) | Ready-to-use CLI operating instructions for external Owner Agents |

### Roadmap: Graphical Client

The graphical Client is planned for the **next iteration**, layered strictly on top of the Hub API:

- **Web UI first** — a lightweight React/TypeScript/Vite app (`apps/web`) served **same-origin by `hub`**: zero CORS, a strict Content-Security-Policy, and an in-memory token only (never persisted to storage).
- **Desktop shell later** — an optional Tauri 2 wrapper can host the same UI as a native window without forking the web surface.

No graphical Client is bundled or required by V1: `hub`, `worker`, and `myrlet` stay headless and independently deployable.

---

## Documentation

- **[Developer Quickstart](docs/development/getting-started.md)** — local setup, verification commands, and end-to-end walkthrough
- **[Deployment Guide](docs/deployment/README.md)** — native binaries, containers, GHCR images, TLS (`MYRLET_HUB_CA_FILE`), backup, and restore
- **[Architecture Contract](docs/architecture/README.md)** — normative V1 specs for [Task Model](docs/architecture/task-model.md), [Hub API](docs/architecture/hub-api.md), [Worker Protocol](docs/architecture/worker-protocol.md), [Security](docs/architecture/security.md), and [Local Runtime](docs/architecture/local-runtime.md)
- **[Documentation Index](docs/README.md)** · **[Brand Assets](assets/brand/README.md)**

---

## Project Status & Contributing

Myrlet is in active **pre-release (`0.1.0`)** development. Wire and storage formats are evolving without backward-compatibility shims until `0.1.0` ships.

- Contributions, issues, and architecture proposals are welcome — please read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [SUPPORT.md](SUPPORT.md).
- Dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option.
