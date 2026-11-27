# Worker Protocol and Execution

Workers initiate authenticated **WSS** connections to Hub so Workers behind NAT/firewalls require no inbound ports.

## Proposed wakeup and claim

1. For a `queued` Task the Hub checks prerequisites, required capabilities, and advertised Slot availability. It wakes one eligible Worker. This hint leaves the Task queued and creates neither a reservation nor a Delivery. If the Worker stays silent, the Hub may wake another.
2. A prepared Worker selects an available local Slot and explicitly claims by Task ID and Slot ID. A shutting-down or unprepared Worker skips the claim. The Hub applies the [Task eligibility rules](task-model.md#proposed-task-lifecycle) atomically with creating the unique effective Delivery. Racing claims can yield one winner only.
3. The winning Worker durably registers that Delivery ID before invoking its Harness. A lost claim acknowledgment recovers the committed binding. An uncertain launch enters recovery. Neither silence nor a new connection authorizes another launch.

The Worker supplies the Task and Delivery IDs, Progress and Result paths, and the concrete `done <delivery-id>` command in the prompt. It asks the Harness to run that optional command last before exit. The local receipt requests reconciliation. Worker-observed exit drives final sync and Finish even if the Harness omits the signal. Replayed notifications resolve against the original Delivery, including after its Slot is reused. See [reporting semantics](hub-api.md#proposed-reporting-semantics). Wakeup and claim frame names, claim recovery exchange, and the local notification transport are open design details.

## Current protocol reference

**Channel split:**
- **WSS** carries handshake, heartbeats, inventory updates, Delivery offers, pre-registration rejections, cancellation commands/responses, and reconnect resync.
- **Hub HTTP** carries Harness-authored **Progress** (`POST /v1/worker/deliveries/{id}/progress`), the optional final Harness-written **Result** (`POST /v1/worker/deliveries/{id}/result`), and Worker-reported **Finish** (`POST /v1/worker/deliveries/{id}/finish`).
- **CLI (`myrlet`)** is the **only** supported channel for a Harness to inspect or operate on Myrlet Tasks. Harnesses never connect to Worker WSS or Hub internals directly.

## Worker Topology, Enrollment, and Inventory

- **One Worker per `MYRLET_HOME`**: Each Worker deployment has one stable `workerId` (`wkr_` + 12 Crockford Base32 chars, cryptographically random, never derived from hardware/MAC). Slots are internal concurrency targets inside one Worker—do not create multiple Workers merely for multiple Harnesses or concurrent Tasks.
- **Basic Host facts only**: Worker Core detects only OS, CPU architecture, and deployment mode (`native` / `container`). It **never** probes, installs, upgrades, or manages toolchains, SDKs, or Harness binaries/profiles.
- **Single-step pairing**:
  1. Admin issues a short-lived one-time code via `POST /v1/enrollment-codes`.
  2. Worker redeems it via `POST /v1/pair` and receives `{ hubId, workerId, credential }` in one step.
  3. Hub stores only the credential's SHA-256 verifier; Worker writes `worker/credentials.json` atomically (`0600`).
  4. Worker connects over WSS and reports its Slot/Capability/Executor inventory in `worker.hello` (and `worker.inventory` on config change).
- **Trusted enrolled inventory**: Inventory reported by an enrolled Worker takes effect directly (no secondary enrollment approval or per-claim inventory approval in V1). Capability states use the closed enum `available | degraded | unavailable`.
- Credential rotation (`rotate`) and revocation (`revoke`) immediately fence active WSS sessions.

## Connection Identity and Epoch Fencing

- Every accepted WSS connection receives a Hub-issued `connectionId` and a monotonic per-Worker `connectionEpoch` in `hub.welcome`.
- Opening a newer authenticated connection fences older epochs: late frames from an older epoch are ignored and never mutate state.
- Any valid current-epoch frame from Hub refreshes the Worker's `leaseTtlMs` deadline. Missing heartbeats mark the Worker offline and trigger reconnect, but **never prove execution stopped**.

## Minimal WSS Frame Matrix

The current binary uses the following frame matrix. Wakeup and claim are proposed operations. Every implemented WSS frame uses a closed `camelCase` JSON envelope (`deny_unknown_fields`):
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

Current offer-based wire behavior:

```text
Hub commits offered Delivery ─> sends task.offer
  └─> Worker: Receive ─> Validate ─┬─ Missing local prerequisite ─> persist refusal ─> send task.rejected (Task ─> held)
                                   └─ Ready ─> Register locally ─> Adapter.submit()
                                                 ├─ Watch local Progress file ─> HTTP POST /progress (first accepted ─> accepted/started)
                                                 ├─ Read final local Markdown at end ─> HTTP POST /result (once, also after finish)
                                                 └─ Observe end of handling ─> HTTP POST /finish (Task ─> finished)
```

- **Hub commits before socket write**: If sending the first `task.offer` fails before any bytes are written, Hub marks the Delivery `released` and returns a queued Task to waiting.
- **Worker never reruns Hub policy**: Worker checks only authenticated epoch, immutable `deliveryId`, recovery readiness, and presence of the configured Adapter—never capacity or Hub capability matching.
- **No duplicate execution**: Worker durably writes `deliveryId` to its local registry **before** invoking `Adapter.submit()`. Duplicate `task.offer` frames for an existing `deliveryId` are ignored.
- **Post-registration failure stays unreported**: A local submission/start failure *after* durable registration leaves the Task `dispatched`; Worker never fabricates a failure verdict or silently requeues/resubmits.
- **Restart & reconnect**: Worker loads its local registry before resyncing, waits for a valid non-zero `connectionEpoch` from `hub.welcome`, sends `worker.resync`, and flushes unacknowledged HTTP Progress/Result/Finish (guarded against concurrent duplicate finish posts) and WSS cancel reports. It never blindly resubmits.
- **No `offer_acceptance_overdue` timer fencing**: Because Progress is optional, Hub must never fence an `offered` Delivery merely because no Progress arrived within a timeout.
- **Delivery states**: `offered`, `accepted` (first Progress accepted), `rejected`, `released`, `finished`, `cancelled`.

## Non-Invasive Harness Adapters & Progress File Boundary

The following file and adapter behavior is implemented today; the proposed read, reconcile, and clear behavior is specified in [Hub reporting](hub-api.md#proposed-reporting-semantics).

Adapters are thin, non-invasive bridges over existing Harness interfaces (preference order: native SDK/API → native event callbacks → CLI / structured output → external process bridge). Initial V1 targets are **Claude Code**, **Codex**, **Pi**, and plain **Process**.

- **Strict 2-method common Adapter contract**: `submit(request)` and `cancel(execution_ref)` **only**.
- **Explicitly forbidden on the generic Adapter trait**: `capabilities()`, `validate()`, `inspect()`, `reconcile()`, `reattach()`, `resume_execution()`, and interactive `input()` / `steer()` / `follow_up()`. Capability differences stay honest and unsupported fields remain absent.
- **Short startup prompt**: Worker passes only minimal Task identification (`taskId` / `deliveryId` / prompt) and local Progress and Result paths (`<worker>/progress/<delivery_id>.json`, `<worker>/results/<delivery_id>.md`). Harnesses fetch extra Task context via the `myrlet` CLI when needed.
- **Progress file ownership**: Worker creates the `progress/` directory and **never** creates, writes, or edits the Progress file itself. The Harness optionally writes bounded JSON (`{ "text": "..." }`, unknown fields ignored, max 64 KiB file / 16 KiB text); Worker watches changes and relays them via Hub HTTP. Never make an extra LLM call to generate Progress.

- **Result file ownership**: Worker creates the `results/` directory and **never** creates, writes, or edits `<delivery_id>.md`. The Harness optionally writes one UTF-8 Markdown result (max 64 KiB); when local handling ends, the Worker reads it **once**, persists the text in its local Delivery record, and posts it to Hub HTTP. Result sync is independent of Progress and Finish: it runs even after the Finish ACK, retries across reconnects and Worker restarts with the identical text (Hub accepts identical replays), and never invokes the Harness again. A missing, empty, oversized, invalid-UTF-8, symlinked, or group/world-accessible result file is ignored, never replaced.
- **Zero intrusion**: Never patch/fork a Harness, inject hooks/extensions/MCP servers, rewrite repo files (`AGENTS.md`, `CLAUDE.md`, `.mcp.json`), or manage Harness Sessions, Workspaces, or Provider Credentials.
- **End detection & cancellation**: Worker detects end of handling locally (e.g., child process exit or backend turn-end) and posts HTTP `/finish` without judging success or failure. `cancel(execution_ref)` signals the process group / native interrupt and relays what the backend answered without claiming physical termination of external side effects.
- **Tool Call & Execution Summary**: Outside V1 baseline; never required for Progress or finish.

## Worker Local State and Shutdown

- Worker uses **one lightweight local store** under `MYRLET_HOME/worker/state/` (atomic per-Delivery JSON files + small `inventory.json`, never two separate storage engines) for Delivery correlation, pending finish/cancel/result intent, and inventory revision.
- On `SIGTERM` / `SIGINT`, Worker closes WSS, stops admitting offers, drains active work up to `MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS` (default 30s), reaps owned Unix process groups where applicable, and exits without fabricating `finished` reports.
