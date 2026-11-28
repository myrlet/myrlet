# Owner Agent Integration Guide

Recommended interaction loop for an external Owner Agent operating Myrlet through the `myrlet` CLI or Hub HTTP/SSE API. Myrlet imposes no mandatory Planner, DAG engine, or prompt format. The Hub exposes capabilities; the agent makes the decisions.

## Standard loop

1. **Observe**: Read `GET /v1/snapshot` (or `myrlet --json tasks list` / `workers list` / `workers show <id>`) to inspect Tasks, trees, blockers, Workers, capabilities, active Deliveries, and claimable Task IDs. Every read works with either the Admin token or an enrolled Worker credential.
2. **Submit**: Submit one Task (`POST /v1/tasks`) or an atomic Bundle (`POST /v1/task-bundles/import`) with up to 5 hierarchy levels and sibling dependencies. Submission **authorizes execution**: a Task whose prerequisites are all `finished` becomes `queued`; one that is still blocked stays `held` and lists its `blockedBy`. There is no separate dispatch step.
3. **Diagnose blocked work**: For a `held` Task, read `blockedBy` for the blocking Task IDs and statuses. To unblock it, either wait for the prerequisite to `finish`, or edit the Task's `dependencies` (`POST /v1/tasks/{id}`) to point at other siblings. Cancelling a prerequisite does **not** cascade: dependents stay `held`.
4. **Watch**: Subscribe to SSE (`GET /v1/events` from Snapshot's `eventCursor`, or `myrlet --json events watch`). Treat SSE frames as invalidation hints and re-read Task detail / Snapshot when making decisions.
5. **Let Workers claim**: A `queued` Task is claimable; the Hub wakes an eligible Worker and one claim commits the sole Delivery. You never choose the Worker for a matched claim. Slot and capability are hard constraints; `preferredWorkerId`/role/executor are soft.
6. **Evaluate outside Hub**: Read the latest Harness-authored Progress, the latest Markdown Result (`resultMarkdown` on Task detail), and the `finished` status. Hub never judges success or failure.
7. **Follow up / retry**:
   - A claimed Parent ends via its Worker's `finished` report; a coordination-only Parent with no Delivery can be closed with `POST /v1/tasks/{id}/finish`.
   - To retry work, create a **new** Task linked by `sourceTaskId` and submit it. Never assume a disconnected Worker stopped executing.

## Conflict and Failure Handling

- **`409 Conflict` (stale version)**: Re-read the Task and retry the edit/cancel/finish with the current `expectedTaskVersion`.
- **`409 InvalidTransition` on an edit**: the Task was claimed while you were editing; the Delivery keeps the execution context it committed with. Read the Task before deciding again.
- **Lost response**: Repeat the exact request with the **same `Idempotency-Key`**. Never invent a new key just because a network response timed out.
- **Offline Worker / missing Progress**: Do not treat absence of Progress as failure or termination; inspect last receipt time and decide whether to wait, cancel, or create a separate retry Task.
