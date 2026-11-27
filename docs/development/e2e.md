# E2E Testing

Myrlet tests drive real HTTP/WSS/CLI interfaces over disposable temporary `MYRLET_HOME` directories and ephemeral loopback ports, inspecting SQLite and local Worker state read-only. No production debug or mutation endpoints exist.

## Current test layers

| Layer | Scope |
| --- | --- |
| **Protocol & Store** | DTO round-trips, SQLite schema invariants, idempotency, version/epoch fencing, event backfill |
| **Hub API & WSS** | Auth, pairing, Task CRUD/Tree/Bundle, dispatch/FIFO queue, cancel, SSE resume, HTTP Progress/Finish |
| **Worker & Adapters** | Local Delivery registry idempotency, Progress file watch, Process/Harness adapter launch & cancel |
| **Integration E2E** | Real `hub` + `worker` + `myrlet` CLI end-to-end over WSS/HTTP, including reconnect & restart |

## Proposed workflow acceptance

Use the [review scenarios](../design/agent-workflow.md#acceptance-scenarios) for dispatch replacement and mutable Result work. Cover an unclaimed wakeup, two racing claims, a lost claim acknowledgment, stale writes following reconnect, and recovered empty/deleted Result files. Check global reads and denied administrative writes separately. These are implementation acceptance cases; the current baseline tests exercise the offer and write-once protocols.

## Evidence Boundaries

- Process Executor E2E proves the tested process path, not every external LLM Harness.
- Missing Progress or disconnects preserve uncertainty and never prove termination or failure.
