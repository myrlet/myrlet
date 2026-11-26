# Myrlet V1 Architecture

This directory is the **single normative contract** for Myrlet's first release (`0.1.0`).
Myrlet is a self-hosted, single-Hub, cross-host Task Broker connecting Humans, Owner Agents, and enrolled Workers across machines.

> **Core boundary:** Hub owns Task and Worker facts. Owner Agents make decisions.
> Harnesses own execution semantics and their own installation, config, credentials, Sessions, and Workspaces.
> Workers start Harnesses through thin adapters, watch one local Progress file, and report when handling ends.
> Harnesses use the documented `myrlet` CLI for any Myrlet operations; they never connect to Worker WSS or Hub internals.
> Myrlet is not a Planner, Workflow/DAG Engine, Agent Harness, Result Verifier, Approval Engine, Sandbox, or LLM Gateway.

## Authority Order

1. This directory (`docs/architecture/`).
2. Wire DTOs implementing this contract (all HTTP & WSS JSON uses `camelCase`).
3. Source code and tests.
4. Historical Issues, PRs, and Git history (context only).

Pre-release formats carry **no backward-compatibility obligation**. Change canonical formats in place and delete obsolete paths rather than adding shims, dual-read paths, or migrations.

## Canonical Documents

| Document | Owns |
| --- | --- |
| [Task model](task-model.md) | 5-state Task lifecycle, 5-level Task Tree, sibling dependencies, minimal `TaskDefinition`, dispatch & FIFO queue, Task Bundle import, copy/retry |
| [Hub API](hub-api.md) | Minimal V1 HTTP/SSE surface, Worker HTTP Progress & Finish reporting, Global Snapshot, single-table event log + broadcast SSE, ingress & shutdown |
| [Worker protocol](worker-protocol.md) | Worker WSS transport, single-step pairing, trusted inventory, epoch fencing, non-invasive Harness adapters, CLI/Progress boundary, local state |
| [Security](security.md) | `Admin` + `Worker` bearer authentication, one-time pairing codes, mandatory WSS TLS verification, file permission safety, explicit no-sandbox Harness boundary |
| [Local runtime and platform](local-runtime.md) | `MYRLET_HOME` layout, SQLite concurrency model, workspace binaries, crate boundaries, and Client surfaces |

## Distinct Identity Boundaries

Never collapse these identities into one another or let one grant the authority of another:

| Identity | Meaning |
| --- | --- |
| `taskId` | Durable logical unit of work owned by Hub. |
| `deliveryId` | One durable Hub-to-Worker handoff of a Task. Transport replay keeps the same `deliveryId`. |
| `workerId` | One enrolled machine (`wkr_` + 12 Crockford Base32 chars, randomly generated, never hardware-fingerprinted). |
| `slotId` | Selectable execution/concurrency unit inside a Worker (not a separate Worker identity or OS sandbox). |
| `executorId` | Operator-configured adapter selected for a Delivery. |
| `executionRef` | Opaque backend-local handle to one submitted Harness execution or process. |

Harness Session, Thread, and Workspace identifiers are **Harness-private** and are not Myrlet identities.

## V1 Core Invariants

1. **Single authoritative Hub**: One Hub process with one local SQLite database coordinates its enrolled Workers. Multi-Hub coordination and shared-DB multi-Hub operation are unsupported.
2. **Creation is not dispatch**: Every Task starts in `held`. Execution requires an explicit dispatch command (`targeted` or `match`).
3. **Five Task states, no verdict**: `held`, `queued`, `dispatched`, `finished`, `cancelled`. There is no `running`, `completed`, `failed`, or `uncertain` Task state. Hub never judges execution success or failure.
4. **Progress is start evidence and latest-state only**: A Harness optionally writes one bounded local JSON Progress file; Worker relays it to Hub over HTTP. First accepted Progress is execution-start evidence. Progress is optional, latest-state only, never changes Task status, and carries no Result, Artifact, Tool Call, Plan, or Step model.
5. **Finish means handling ended**: Worker reports `finished` over Hub HTTP when its local handling ends (even if no Progress was written). `cancelled` is never overwritten by a later finish report.
6. **Non-invasive Harness integration**: Myrlet never modifies, forks, injects hooks/plugins/MCP servers into, or sandboxes a Harness, and never makes an extra LLM call to produce Progress.
7. **Capacity is advisory**: Reported Slot capacity informs Owner selection only. Neither Hub nor Worker reserves Slots or rejects work based on capacity. Missing capacity is `null` (unknown), never `0`.
8. **Idempotent Delivery, no silent retries**: Worker durably registers `deliveryId` before invoking an adapter; replay never double-submits. Business retries create a new `held` Task linked by `sourceTaskId`.
9. **Uncertainty stays visible**: Disconnects, timeouts, and missing reports never prove process termination and never synthesize terminal outcomes.
10. **Model-free control plane**: Hub, Worker, CLI, and core App require no LLM.
11. **Minimal auth and fencing**: Control-plane mutations use constant-time SHA-256 bearer verification (`Admin` or `Worker`), `Idempotency-Key`, optimistic `expectedTaskVersion`, and monotonic WSS `connectionEpoch` fencing.
12. **Code is a liability**: Keep the smallest responsibility, schema, and state space that satisfies these invariants.

## Explicit V1 Non-Goals (Cut from Baseline)

To prevent over-engineering in `0.1.0`, the following are explicitly **excluded** from V1:

- Multi-tenant RBAC, custom Principal roles, multi-Scope isolation, Task Assignments / grant tokens, nested subtree Ownership Boundaries, ownership transfer, and one-hop `adjacentTasks` projections.
- Double-stage administrative enrollment approval, per-claim Worker inventory approval state machines, deterministic credential seed derivation, and proxy `X-Forwarded-For` rate limiters.
- Scope Category Catalog API/table, Task `priority` scheduling or validation ceremony, Human Inbox, and Owner notification delivery.
- `GET /v1/work`, `GET /v1/history`, `POST /v1/matches/preview`, `POST /v1/task-bundles/preview`, Queue Intent stale reauthorization, and SHA-256 view-fingerprint / signed pagination cursors.
- `worker.reconciliation_query` / `hub.reconciliation` paginated scans, cancel report-fingerprint refusal state machines, 60-second `offer_acceptance_overdue` heartbeat sweeps, and 3-table polling Outbox workers.
- Worker-managed Git worktree leases, toolchain/SDK probing or Harness installation/profile management in Worker Core, Harness Session/Workspace management, Tool Call / Execution Summary / transcript extraction, and Model Proxy routes.
- Custom spatial Graph / Work Atlas canvas editors, Saved Views, static topology diagrams, and production-bundled demo fixtures or component galleries.
