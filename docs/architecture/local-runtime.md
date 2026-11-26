# Local Runtime, Workspace Boundaries, and Clients

## Data Root (`MYRLET_HOME`)

`MYRLET_HOME` is the single local data root for Myrlet-owned state. Set an explicit absolute path outside any Git checkout (e.g., `/var/lib/myrlet` or `$HOME/.local/share/myrlet`). Hub and Worker are separate OS processes; they may share one `MYRLET_HOME` on the same Host or run on different Hosts without a shared filesystem.

```text
MYRLET_HOME/
├── hub/
│   ├── config.toml          # optional Hub configuration
│   ├── credentials.json     # Hub installation ID (0600)
│   └── hub.sqlite           # single SQLite database (0600)
└── worker/
    ├── worker.yaml          # Worker Slots, capabilities, and Executors
    ├── credentials.json     # Hub-issued Worker credential binding (0600)
    ├── state/               # Worker local Delivery registry & inventory revision
    └── progress/            # Worker-chosen Progress JSON files (<delivery_id>.json)
```

- Harness binaries, configs, Provider Credentials, Sessions, and Workspaces live **outside** `MYRLET_HOME` and are owned by the Harness/operator.
- Default logs write to `stdout`/`stderr` for external supervisors (`systemd`, `launchd`, Docker).

## Hub SQLite Concurrency Model

- Hub uses **one local SQLite database** in WAL mode (`PRAGMA foreign_keys = ON`, bounded `busy_timeout`).
- Synchronous `rusqlite` work MUST NOT block Tokio async scheduler threads: wrap the single writer connection behind a one-permit semaphore + `tokio::task::block_in_place` on the multi-thread Tokio runtime (running inline only on current-thread test runtimes). Do not introduce separate actor threads, connection pools, or external message brokers.

## Workspace Layout and Crate Boundaries

Myrlet uses single-word directory names under `crates/` and `apps/`, shipping three binaries over four shared crates with strict one-way dependencies (zero duplicated domain vs. wire structs):

```text
crates/protocol (myrlet-protocol: canonical Task, Delivery, IDs, enums, HTTP/WSS DTOs & path safety)
   ▲
   ├── crates/store  (myrlet-store: Hub SQLite persistence & append-only event log)
   ├── crates/worker (myrlet-worker: Worker local Delivery registry, Progress watch, thin Harness adapters)
   └── crates/client (myrlet-client: transport-neutral native + WASM HTTP/SSE Client SDK)
         ▲
         └── apps/
             ├── apps/hub    (pkg: myrlet-hub,           bin: hub)
             ├── apps/worker (pkg: myrlet-worker-daemon, bin: worker)
             └── apps/cli    (pkg: myrlet-cli,           bin: myrlet)
```

- No library crate depends on an app crate. `apps/cli` and `crates/client` MUST NOT depend on any GUI framework.
- Binary names (`hub`, `worker`, `myrlet`) map 1:1 to the [`Dockerfile`](../../Dockerfile) build targets and the published images `ghcr.io/myrlet/{hub,worker,myrlet}`, so a release never ships a container whose entrypoint disagrees with its package.
- Keep modules cohesive and cleanly decoupled (target **<= 300–400 LOC** per production `.rs` file; split distinct domain responsibilities instead of building monolithic files).
- Production builds MUST NOT bundle test fixtures, demo datasets, or developer component galleries.

## Client Surfaces

All clients are thin presentation/automation shells over the Hub HTTP/SSE API; none owns a second Task state machine or scheduler.

V1 (`0.1.0`) ships **three pure-Rust binaries** (`hub`, `worker`, `myrlet`) and one Agent Skill. The graphical Client is deferred to the next iteration.

| Surface | Binary / Crate | V1 Scope |
| --- | --- | --- |
| **CLI** | `myrlet` (`apps/cli`) | Headless Task create/dispatch/submit/bundle-import/list/show/cancel/finish, Worker list, and `events watch` (human + `--json` output). Also the supported interface for Harnesses. |
| **Shared Client SDK** | `myrlet-client` (`crates/client`) | One `Client` over `native` (`reqwest`/Tokio) and `wasm` (`fetch`) transports using canonical `myrlet-protocol` DTOs. |
| **Agent Skill** | [skills/myrlet/SKILL.md](../../skills/myrlet/SKILL.md) | Concise CLI operating instructions for external Owner Agents. |

### Graphical Client (next iteration)

Planned, not shipped in V1:

- **Web UI** — lightweight React/TypeScript/Vite app (`apps/web`) served **same-origin by `hub`**: zero CORS, a strict Content-Security-Policy, and an in-memory token only (never persisted). Scope: Hub connection, **List/Tree Task workspace**, **Task Detail** (hierarchy, dependencies, Delivery, latest Progress), Task create/dispatch/cancel dialogs, and Worker/pairing management.
- **Desktop shell** — an optional Tauri 2 wrapper in a later iteration, hosting the same Web UI as a native window without forking the web surface.
- **Owner Agent (optional, BYOK)** — a future graphical Client MAY offer an optional BYOK Owner Agent (e.g., via Pi SDK) running strictly at the Client boundary through ordinary Hub APIs; it MUST NEVER become a Hub, Worker, CLI, or core Client dependency.
- **Out of scope** — custom spatial graph canvases, saved view managers, and static login diagrams.
