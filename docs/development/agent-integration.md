# Owner Agent Integration Guide

Recommended interaction loop for an external Owner Agent operating Myrlet through the `myrlet` CLI or Hub HTTP/SSE API. Myrlet imposes no mandatory Planner, DAG engine, or prompt format.

## Workflow proposed for Owners and Harnesses

An Owner submits once, inspects blockers and eligible Slot requirements, and explicitly edits a pending Task if necessary. Cancelling a prerequisite leaves dependents held. A Harness can request a local sync and report `done` with its own Delivery ID as the last command before exit. Distinguish local receipt, Hub confirmation, and Worker-observed Finish. See the [Task transitions](../architecture/task-model.md#proposed-task-lifecycle) and [local commands](../architecture/local-runtime.md#proposed-agent-commands). These proposed operations require implementation before use.

## Standard loop with current binaries

1. **Observe**: Read `GET /v1/snapshot` (or `myrlet --json tasks list` / `workers list`) to inspect online Workers, capabilities, active Deliveries, and Task Trees.
2. **Create**: Create a `held` Task (`POST /v1/tasks`) or atomic `held` Task Bundle (`POST /v1/task-bundles/import`) with up to 5 hierarchy levels and sibling dependencies. Creation never dispatches.
3. **Dispatch**: Explicitly dispatch each ready Task using either:
   - **Targeted Dispatch**: exact `workerId` + `slotId` (and optional `executorId`), or
   - **Simple Match**: let Hub deterministically pick an online compatible Worker/Slot.
   Pass `queue: true` if you want Hub to hold the Task in the FIFO queue until a compatible Worker is online.
4. **Watch**: Subscribe to SSE (`GET /v1/events` from Snapshot's `eventCursor`, or `myrlet --json events watch`). Treat SSE frames as invalidation hints and re-read Task detail / Snapshot when making decisions.
5. **Evaluate outside Hub**: Read the latest Harness-authored Progress, the optional immutable Markdown result (`resultMarkdown` on Task detail), and `finished` status from Task/Snapshot detail. Hub never judges success or failure.
6. **Follow up / retry**:
   - After delegating children, a dispatched Parent ends via its Worker's `finished` report; a coordination-only `held` Parent can be explicitly marked `finished`.
   - To retry work, create a **new `held` Task** linked by `sourceTaskId` and dispatch it explicitly. Never assume a disconnected Worker stopped executing.

## Conflict and Failure Handling

- **`409 Conflict` (stale version)**: Re-read the Task and retry with the current `expectedTaskVersion`.
- **Lost create/dispatch response**: Repeat the exact request with the **same `Idempotency-Key`**. Never invent a new key just because a network response timed out.
- **Offline Worker / missing Progress**: Do not treat absence of Progress as failure or termination; inspect last receipt time and decide whether to wait, cancel, or create a separate retry Task.
