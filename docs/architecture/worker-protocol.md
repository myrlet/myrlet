# Worker Protocol and Execution

Workers initiate authenticated **WSS** connections to Hub so Workers behind NAT/firewalls require no inbound ports.
**Channel split:**
- **WSS** carries handshake, heartbeats, inventory updates, Delivery offers, pre-registration rejections, cancellation commands/responses, and reconnect resync.
- **Hub HTTP** carries Harness-authored **Progress** (`POST /v1/worker/deliveries/{id}/progress`) and Worker-reported **Finish** (`POST /v1/worker/deliveries/{id}/finish`).

## Enrollment and Inventory (Single-Step)

1. An Admin issues a short-lived, one-time pairing code via `POST /v1/enrollment-codes` (optionally bound to a `workerId`).
2. The Worker redeems the code via `POST /v1/pair` and receives its `workerId`, `hubId`, and a CSPRNG machine bearer credential in one step.
3. Hub stores only the credential's SHA-256 verifier; Worker writes its credential atomically (`0600`) to `worker/credentials.json`.
4. Worker connects over WSS and sends its current Slot/Capability/Executor inventory in `worker.hello` (and later via `worker.inventory` if config changes).
5. **Trusted enrolled inventory**: Inventory reported by an enrolled Worker takes effect directly; there is no secondary administrative enrollment approval or per-claim inventory approval workflow in V1.

Credential rotation (`POST /v1/workers/{id}/rotate`) and revocation (`POST /v1/workers/{id}/revoke`) immediately fence active WSS connections.

## Connection Identity and Epoch Fencing

- Every accepted WSS connection receives a Hub-issued `connectionId` and a monotonic per-Worker `connectionEpoch` in `hub.welcome`.
- Opening a newer authenticated connection fences older epochs: late frames from an older epoch are ignored and never mutate state.
- Any valid current-epoch frame from Hub refreshes the Worker's `leaseTtlMs` deadline. Missing heartbeats mark the Worker offline and trigger reconnect, but **never prove execution stopped**.

## Minimal WSS Frame Matrix

Every WSS frame uses a closed camelCase JSON envelope:
`{ v, kind, messageId, workerId, slotId?, connectionId?, connectionEpoch?, sentAtMs, replyTo?, payload }`.
Frames are capped at 512 KiB; inventory payloads are capped at 256 KiB and 256 Slots.

| Kind | Direction | Purpose |
| --- | --- | --- |
| `worker.hello` | Worker → Hub | Present `workerId`, current inventory (host, Slots, Executors, capabilities), and locally retained active Delivery IDs. |
| `hub.welcome` | Hub → Worker | Confirm `hubId`, `workerId`, `connectionId`, monotonic `connectionEpoch`, and heartbeat/lease timings. |
| `worker.heartbeat` / `hub.heartbeat_ack` | Bidirectional | Connection liveness keepalive (does not resend full inventory). |
| `worker.inventory` / `worker.inventory_ack` | Bidirectional | Synchronize updated Worker configuration/inventory. |
| `task.offer` | Hub → Worker | Offer one committed Delivery (`taskId`, `deliveryId`, `slotId`, `executorId`, immutable `TaskDefinition`). |
| `task.rejected` / `task.rejected_ack` | Bidirectional | Pre-registration local refusal (`executor_unavailable`, `worker_shutting_down`, `recovery_incomplete`); returns Task to `held`. |
| `task.cancel` | Hub → Worker | Request cancellation of a Delivery. |
| `task.cancelled` / `task.cancelled_ack` | Bidirectional | Report Harness cancellation response (`confirmed`, `failed`, `uncertain`, `unsupported`); idempotent ACK by `(deliveryId, epoch)`. |
| `worker.resync` | Worker → Hub | On reconnect, rebind locally retained non-terminal Deliveries to the new `connectionEpoch` so pending HTTP Progress/Finish and WSS cancel reports can flush. |
| `worker.bye` | Worker → Hub | Graceful disconnect notice. |

### Representative Frame Example

```json
{
  "v": "myrlet.worker/1",
  "kind": "task.cancel",
  "messageId": "msg_cancel",
  "workerId": "worker_01",
  "connectionId": "connection_01",
  "connectionEpoch": 1,
  "sentAtMs": 1010,
  "payload": {
    "taskId": "task_01",
    "deliveryId": "delivery_01",
    "reason": "operator requested cancellation"
  }
}
```

## Delivery Lifecycle and Idempotency

```text
Hub offers Delivery (offered)
  └─> Worker receives task.offer on current epoch
        ├─ Missing local prerequisite ─> persist refusal ─> send task.rejected (Task returns to held)
        └─ Ready ─> persist Delivery record locally ─> invoke Adapter.submit()
                     ├─ Watch local Progress file ─> HTTP POST /progress (first accepted => accepted/started)
                     └─ Observe local end of handling ─> HTTP POST /finish (Task => finished)
```

- **No duplicate execution**: Before invoking an Adapter, Worker durably records `deliveryId` in its local registry. Any duplicate `task.offer` with an already-recorded `deliveryId` is ignored.
- **No blind resubmit after crash/restart**: On restart, Worker reloads its local registry, reconnects, sends `worker.resync` to rebind active Deliveries to the new `connectionEpoch`, and flushes any unacknowledged HTTP Progress/Finish or WSS cancel reports. It never re-invokes `submit()` for a previously registered Delivery.
- **No `offer_acceptance_overdue` false fencing**: Because Progress is optional, Hub must never fence an `offered` Delivery merely because no Progress arrived within a timer window.
- **Delivery states**: `offered`, `accepted` (first Progress accepted), `rejected`, `released` (offer failed before socket write), `finished`, `cancelled`.

## Non-Invasive Harness Adapters

Adapters are thin bridges exposing two operations: `submit(request)` and `cancel(execution_ref)`.

- **One Progress mechanism**: Worker chooses `<worker_state>/progress/<delivery_id>.json`, ensures the parent directory exists, and passes that path in the short startup prompt. The Harness optionally writes bounded JSON (`{ "text": "..." }`, max 64 KiB file / 16 KiB text); Worker watches changes and relays them via Hub HTTP.
- **Zero intrusion**: Adapters invoke Harnesses through their native public CLI/RPC interfaces (e.g., Pi, Codex, Claude Code, or plain Process). Myrlet must **never** patch/fork Harnesses, inject hooks/extensions/MCP servers, rewrite repo files (`AGENTS.md`, `CLAUDE.md`, `.mcp.json`), or manage Harness Sessions, Workspaces, or Provider Credentials.
- **End detection**: Worker detects end of handling locally (e.g., child process exit or backend turn-end) and posts `POST /v1/worker/deliveries/{id}/finish` without judging success or failure.
- **Cancellation**: `cancel(execution_ref)` forwards an interrupt/stop signal to the backend/process group and reports what the backend answered. It does not guarantee every external side effect stopped.

## Worker Local State and Shutdown

- Worker uses **one lightweight local store** under `MYRLET_HOME/worker/state/` (atomic per-Delivery JSON files or a single small store — never two redundant persistence engines) for Delivery correlation, pending finish/cancel intent, and inventory revision.
- On `SIGTERM` / `SIGINT`, Worker closes its WSS control channel, stops admitting new offers, allows in-flight work to drain up to `MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS` (default 30s), and exits without fabricating `finished` reports.
