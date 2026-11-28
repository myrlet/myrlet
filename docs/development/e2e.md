# E2E Testing

Myrlet tests drive real HTTP/WSS/CLI interfaces over disposable temporary `MYRLET_HOME` directories and ephemeral loopback ports, inspecting SQLite and local Worker state read-only. No production debug or mutation endpoints exist.

## Current test layers

| Layer | Scope |
| --- | --- |
| **Protocol & Store** | DTO round-trips, SQLite schema invariants, idempotency, version/epoch fencing, event backfill |
| **Hub API & WSS** | Auth, pairing, Task submit/Tree/Bundle, wakeup and claim, cancel, SSE resume, HTTP Progress/Result/Finish |
| **Worker & Adapters** | Local Delivery registry idempotency, Progress file watch, Process/Harness adapter launch & cancel |
| **Integration E2E** | Real `hub` + `worker` + `myrlet` CLI end-to-end over WSS/HTTP, including reconnect & restart |

## Submission, claim, and Result acceptance

The [scenarios](../design/agent-workflow.md#acceptance-scenarios) cover an unclaimed wakeup, two racing claims, a lost claim acknowledgment, stale writes following reconnect, and recovered empty or deleted Result files, plus global reads and denied administrative writes. These are the implemented behaviors the suite exercises.

## Evidence Boundaries

- Process Executor E2E proves the tested process path, not every external LLM Harness.
- Missing Progress or disconnects preserve uncertainty and never prove termination or failure.
