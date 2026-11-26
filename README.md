<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/myrlet-mark-inverse.svg">
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/myrlet-mark.svg">
    <img src="assets/brand/myrlet-mark.svg" alt="Myrlet, the ant-inspired collaboration mark" width="180">
  </picture>
</p>

<h1 align="center">Myrlet</h1>

<p align="center">
  <strong>The task broker for whatever comes next.</strong>
</p>

<p align="center">
  An open-source, self-hosted execution layer connecting agents, people, and tools to the capabilities of your machines.<br>
  <strong>Your machines. Your agents. Your workflows.</strong>
</p>

<p align="center">
  <a href="#get-started">Get started</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="docs/architecture/README.md">Architecture</a> ·
  <a href="docs/README.md">Documentation</a> ·
  <a href="assets/brand/README.md">Brand assets</a>
</p>

---

## Project Status

Myrlet is in **pre-release development (`0.1.0`)**. There is no stable release or wire/storage compatibility guarantee yet. The [Architecture contract](docs/architecture/README.md) defines the simplified V1 target; check live [Issues](https://github.com/myrlet/myrlet/issues) and [Actions](https://github.com/myrlet/myrlet/actions) for current status.

## Why Myrlet

Agents keep getting better at planning and solving problems. The infrastructure underneath them should stay simple, durable, and non-invasive.

Your Mac can build an iOS app. Your Linux server runs containers. Another host has a GPU. Myrlet connects those machines through a single self-hosted **Task Broker** without forcing your agents into a specific model, harness, planner, or workflow engine.

**Agents decide. Workers execute. Myrlet keeps the work moving.**

## How It Works

- **One Hub** owns durable Task state, Deliveries, Worker inventory, and an SSE event stream in a single SQLite database.
- **Enrolled Workers** connect outbound over authenticated WSS (no inbound ports needed on Worker hosts), register local Deliveries idempotently, and launch Harnesses or processes through thin, non-invasive adapters.
- **Five Task states**: `held` → `queued` / `dispatched` → `finished` | `cancelled`. Creation never dispatches automatically. Hub never judges execution success or failure.
- **Minimal reporting**: Harnesses optionally write a single local JSON Progress file; Worker relays the latest Progress and reports `finished` over Hub HTTP when handling ends. First accepted Progress is execution-start evidence.
- **Non-invasive boundary**: Myrlet does not modify Harnesses, inject hooks/plugins/MCP servers, manage Harness Sessions/Workspaces/Provider Credentials, or act as a sandbox.
- **Business retries create new Tasks**: Transport replays reuse the same `Delivery ID` without double-submitting; retrying work creates a new `held` Task linked via `sourceTaskId`.

## Surfaces

| Surface | Purpose |
| --- | --- |
| **Myrlet CLI (`myrlet`)** | Headless commands with human and `--json` output for scripts and Owner Agents. |
| **Myrlet App (`myrlet-app`)** | Calm NSH desktop (macOS) and same-origin Web workspace: List/Tree Tasks, Task Detail, Progress, and Worker management. |
| **[Myrlet Skill](skills/myrlet/SKILL.md)** | Lightweight CLI instructions for external Owner Agents. |
| **Client SDK (`myrlet-client`)** | Shared authenticated HTTP/SSE client for native Rust and browser WASM. |

## Get Started

- **[Developer quickstart](docs/development/getting-started.md)** — build the workspace, start Hub and Worker, and dispatch a Task.
- **[Deployment](docs/deployment/README.md)** — run native binaries or OCI containers (`myrlet`, `hub`, `worker`).
- **[Architecture](docs/architecture/README.md)** — canonical V1 contracts for [Task model](docs/architecture/task-model.md), [Hub API](docs/architecture/hub-api.md), [Worker protocol](docs/architecture/worker-protocol.md), [Security](docs/architecture/security.md), and [Local runtime](docs/architecture/local-runtime.md).
- **[Documentation index](docs/README.md)** — full guide index.

## Contributing & License

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [SUPPORT.md](SUPPORT.md).

Dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE), at your option.
