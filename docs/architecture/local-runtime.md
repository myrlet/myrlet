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
    ├── control.sock         # local CLI control channel (0600)
    ├── state/               # Worker local Delivery registry & inventory revision
    ├── progress/            # Worker-chosen Progress JSON files (<delivery_id>.json)
    └── results/             # Worker-chosen Markdown result files (<delivery_id>.md)
```

- Harness binaries, configs, Provider Credentials, Sessions, and Workspaces live **outside** `MYRLET_HOME` and are owned by the Harness/operator.
- Default logs write to `stdout`/`stderr` for external supervisors (`systemd`, `launchd`, Docker).

## Hub SQLite Concurrency Model

- Hub uses **one local SQLite database** in WAL mode (`PRAGMA foreign_keys = ON`, bounded `busy_timeout`).
- Synchronous `rusqlite` work MUST NOT block Tokio async scheduler threads: wrap the single writer connection behind a one-permit semaphore + `tokio::task::block_in_place` on the multi-thread Tokio runtime (running inline only on current-thread test runtimes). Do not introduce separate actor threads, connection pools, or external message brokers.

The Hub stores the two Security & Trust booleans in one singleton `hub_settings` row. The existing `workers` row holds the server-controlled `permissionTier` (`worker` or `owner`), approval, reported/applied scheduling inventory and their revisions. New pairing defaults to `worker`; only Admin can change the tier. Policy decisions and their associated Pair, report, Apply, or tier change are serialized by the existing SQLite writer.

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
- `apps/web` (React/TypeScript/Vite) builds to `apps/web/dist`, which the `hub` build script embeds into the `hub` binary at compile time (no runtime filesystem dependency). The same-origin Web UI is **enabled by default**; disable it with `MYRLET_HUB_UI=false` or `uiEnabled = false` in `hub/config.toml`. A build without `apps/web/dist` still compiles and serves API-only (UI paths return the JSON `404 NotFound`).
- Binary names (`hub`, `worker`, `myrlet`) map 1:1 to the [`Dockerfile`](../../Dockerfile) build targets, so a release never ships a container whose entrypoint disagrees with its package.
- Three images are published from those targets: `ghcr.io/myrlet/hub` (Hub plus the built-in Web UI), `ghcr.io/myrlet/worker` (Worker only), and `ghcr.io/myrlet/myrlet` (all-in-one local/homelab image with Hub + built-in Web UI + auto-paired local Worker).
- Keep modules cohesive and cleanly decoupled (target **<= 300–400 LOC** per production `.rs` file; split distinct domain responsibilities instead of building monolithic files).
- Production builds MUST NOT bundle test fixtures, demo datasets, or developer component galleries.

## Client Surfaces

All clients are thin presentation/automation shells over the Hub HTTP/SSE API; none owns a second Task state machine or scheduler. The Hub evaluates the credential's actual permission tier for every operation.

V1 (`0.1.0`) ships **three pure-Rust binaries** (`hub`, `worker`, `myrlet`), the same-origin **Web UI** compiled into `hub` from `apps/web`, and one Agent Skill.

| Surface | Binary / Crate | V1 Scope and authorization |
| --- | --- | --- |
| **CLI** | `myrlet` (`apps/cli`) | Supports all three tiers. A `worker`-tier Worker credential is limited to its own Delivery-bound Task and direct related-Task summaries plus execution-local commands. An `owner`-tier Worker credential adds global Task coordination and Worker/Agent observability. The Admin token adds Worker and system administration. The CLI does not grant or infer authority; Hub checks every operation. |
| **Shared Client SDK** | `myrlet-client` (`crates/client`) | One `Client` over `native` (`reqwest`/Tokio) and `wasm` (`fetch`) transports using canonical `myrlet-protocol` DTOs. Any non-CLI application using it must authenticate as Admin. |
| **Web UI** | `apps/web` → embedded in `hub` | React/TypeScript/Vite app compiled and embedded into the `hub` binary at build time and served same-origin: zero CORS, strict CSP, Admin credential in memory only. All Web UI access requires Admin. Scope: Hub connection, **List and read-only Board**, **Task Detail** (hierarchy, dependencies and blockers, Delivery, latest Progress, latest Result Markdown), and Worker/pairing management. |
| **Agent Skill** | [skills/myrlet/SKILL.md](../../skills/myrlet/SKILL.md) | Concise CLI operating instructions for external Agents and Harnesses. The Agent uses its Worker's credential and inherits the Worker's `worker` or `owner` tier; there is no separate Agent credential. |

### Tiered CLI authorization

- Worker and Owner operations authenticate with the Hub-issued Worker credential stored under `MYRLET_HOME/worker/credentials.json`. The Hub determines whether that Worker has `worker` or `owner` permission.
- Admin operations authenticate with `MYRLET_ADMIN_TOKEN`. The Admin token is a separate principal and cannot be used for Worker claims or Worker-originated Progress/Result/Finish reports.
- Worker-tier CLI reads are restricted to the Worker's Delivery-bound Task and one-hop related-Task summaries. Owner-tier CLI can read global Tasks, Workers, and reported Agent runtime status and perform the Task coordination operations defined by the Task Model.
- Insufficient permission returns an explicit authorization error from Hub; CLI commands do not bypass server-side checks.

### Worker trust operations

`myrlet workers show <worker-id>` is available to Owner and Admin for Worker status and scheduling observation. Owner sees operational status and scheduling-relevant inventory; Admin also sees administration-only fields. `myrlet workers config show <worker-id>` is Admin-only and displays the Reported and Applied snapshots. The following operations require Admin: `workers enroll`, `workers rotate`, `workers revoke`, `workers approve`, `workers config apply`, and `workers permission set <worker-id> --tier worker|owner`. The permission command changes the server-stored Worker tier; it does not create a new credential or Agent identity. Each command supports `--json`. Apply prints the applied revision on success and surfaces the Hub's specific error on failure.

The Workers page identifies and filters pending Workers, shows the Reported and Applied snapshots and their field differences, and submits the revision the Admin reviewed. Pending Workers offer Approve and Revoke, approved Workers offer Revoke, and revoked Workers link to pairing. The Web Settings **Security & Trust** group edits the two Hub policies independently. The Workers page and Security & Trust settings are Admin-only; the Web UI cannot be used with Worker-tier or Owner-tier credentials. The CLI mirrors the two policies as `myrlet policy show` and `myrlet policy set [--auto-approve <bool>] [--auto-apply <bool>]`, where each `--auto-*` flag maps to the corresponding stored requirement (`--auto-approve true` clears `requireWorkerApproval`); an omitted flag leaves that policy unchanged.

### Desktop Shell (next iteration)

Planned, not shipped in V1:

- **Desktop shell** — an optional Tauri 2 wrapper in a later iteration, hosting the same embedded Web UI as a native window without forking the web surface. As a non-CLI client, it requires Admin.
- **Owner Agent (optional, BYOK)** — a future graphical Client MAY offer an optional BYOK Owner Agent (e.g., via Pi SDK) running strictly at the Client boundary through ordinary Hub APIs; it MUST NEVER become a Hub, Worker, CLI, or core Client dependency. If the Owner Agent runs inside a Worker, it uses that Worker's identity and tier.
- **Out of scope** — graph editing and workflow builders, saved view managers, and static login diagrams.

### Execution-local CLI commands

`done <delivery-id>`, `result check <delivery-id>`, `result sync <delivery-id>`, and `result status <delivery-id>` take no Hub credential. They write one JSON request to `worker/control.sock` under `MYRLET_HOME` and read one `LocalReply`. Each reply separates **local receipt** (the Worker received and acted on the command) from **Hub confirmation** (the Hub stored the resulting value); `result status` reports `equal`, `different`, `missing_locally`, `missing_remotely`, `absent`, or `unknown` (Hub unreachable) and never writes. A Delivery ID the Worker never registered is rejected locally, so a stale command cannot act on a newer Delivery reusing the same Slot.

### Desktop Shell (next iteration)

Planned, not shipped in V1:

- **Desktop shell** — an optional Tauri 2 wrapper in a later iteration, hosting the same embedded Web UI as a native window without forking the web surface.
- **Owner Agent (optional, BYOK)** — a future graphical Client MAY offer an optional BYOK Owner Agent (e.g., via Pi SDK) running strictly at the Client boundary through ordinary Hub APIs; it MUST NEVER become a Hub, Worker, CLI, or core Client dependency.
- **Out of scope** — graph editing and workflow builders, saved view managers, and static login diagrams.
