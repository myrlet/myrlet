# E2E Testing

Myrlet tests drive real HTTP/WSS/CLI interfaces over disposable temporary `MYRLET_HOME` directories and ephemeral loopback ports, inspecting SQLite and local Worker state read-only. No production debug or mutation endpoints exist.

## Test Layers

| Layer | Scope |
| --- | --- |
| **Protocol & Store** | DTO round-trips, SQLite schema invariants, idempotency, version/epoch fencing, event backfill |
| **Hub API & WSS** | Auth, pairing, Task CRUD/Tree/Bundle, dispatch/FIFO queue, cancel, SSE resume, HTTP Progress/Finish |
| **Worker & Adapters** | Local Delivery registry idempotency, Progress file watch, Process/Harness adapter launch & cancel |
| **Integration E2E** | Real `myrlet-hub` + `myrlet-worker` + `myrlet` CLI end-to-end over WSS/HTTP, including reconnect & restart |

## Evidence Boundaries

- Process Executor E2E proves the tested process path, not every external LLM Harness.
- Missing Progress or disconnects preserve uncertainty and never prove termination or failure.
