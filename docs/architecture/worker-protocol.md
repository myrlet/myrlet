# Worker Protocol and Execution

Workers initiate authenticated **WSS** connections to Hub so Workers behind NAT/firewalls require no inbound ports.

**Channel split:**
- **WSS** carries handshake, heartbeats, inventory updates, **wakeup hints**, `task.cancel` commands/responses, and reconnect resync.
- **Hub HTTP** carries **claims** (`POST /v1/workers/claim`), Harness-authored **Progress** (`POST /v1/worker/deliveries/{id}/progress`), the latest Markdown **Result** (`POST/GET /v1/worker/deliveries/{id}/result`), and Worker-reported **Finish** (`POST /v1/worker/deliveries/{id}/finish`).
- **CLI (`myrlet`)** is the **only** supported channel for a Harness to inspect or operate on Myrlet Tasks. Harnesses never connect to Worker WSS or Hub internals directly. Execution-local commands (`done`, `result check/sync/status`) reach the Worker's local control socket; they never talk to the Hub directly.

## Worker Topology, Enrollment, and Inventory

- **One Worker per `MYRLET_HOME`**: Each Worker deployment has one stable `workerId` (`wkr_` + 12 Crockford Base32 chars, cryptographically random, never derived from hardware/MAC). Slots are internal concurrency targets inside one Worker—do not create multiple Workers merely for multiple Harnesses or concurrent Tasks.
- **Shared Agent authority**: Every Agent/Harness started within a Worker uses that Worker's Myrlet identity and credential, with its Hub-assigned `worker` or `owner` tier. Agent IDs and Harness/session identifiers are observational metadata only; they cannot authenticate or narrow the Worker's authority. Owner may read Agent runtime state only to the extent the Worker reports it. V1 does not define a separate Agent-state WSS frame or introspect Harness-private subagents/Sessions; richer Agent telemetry requires a separate approved protocol contract.
- **Basic Host facts only**: Worker Core detects only OS, CPU architecture, and deployment mode (`native` / `container`). It **never** probes, installs, upgrades, or manages toolchains, SDKs, or Harness binaries/profiles.
- **Single-step pairing**:
  1. Admin issues a short-lived one-time code via `POST /v1/enrollment-codes`.
  2. Worker redeems it via `POST /v1/pair` and receives `{ hubId, workerId, credential }` in one step.
  3. Hub stores only the credential's SHA-256 verifier; Worker writes `worker/credentials.json` atomically (`0600`).
  4. Worker connects over WSS and reports its Slot/Capability/Executor inventory in `worker.hello` (and `worker.inventory` on config change).
- Capability states use the closed enum `available | degraded | unavailable`.
- Credential rotation (`rotate`) and revocation (`revoke`) immediately fence active WSS sessions.

## Worker approval and configuration apply

The Hub stores two independent policies under **Security & Trust**. Each defaults to on, including for the auto-paired Worker in the all-in-one image. A settings write takes effect for subsequent operations. Changing either switch leaves existing approval decisions and pending reports in place.

| Policy | Decision point | On | Off |
| --- | --- | --- | --- |
| `requireWorkerApproval` | Successful Pair | Pending until Admin selects Approve | Approved immediately |
| `requireConfigurationApply` | First or changed scheduling inventory | Keep current Applied Config until Admin selects Apply | Apply the report in the same transaction |

A pending Worker can connect, send heartbeats, report inventory and perform ordinary reads. Admin may approve or revoke it. Approval and Apply may occur in either order. Rotation preserves both decisions. Re-pairing a revoked ID starts a fresh approval decision and clears both snapshots. The Hub retains its revision high-water mark so an Apply from a previous pairing cannot match a new report.

**Reported Config** is the latest valid scheduling inventory received through `worker.hello` or `worker.inventory`. **Applied Config** is the snapshot the Hub uses for Slot matching. Before the first report both are absent. With manual Apply the first report is pending and Applied remains absent. The Hub retains at most these two snapshots per Worker.

The reviewable content is the advertised Slots, including enabled flags, roles, capabilities and their states, advisory capacity, and ordered Executor IDs and kinds. Executor order matters for default selection. Host observations and timestamps remain ordinary telemetry. Hub URL, process commands, arguments, timeouts and credentials stay local. The Hub assigns an increasing per-Worker revision when reviewable content changes. A repeated report, including reconnect, refreshes observations and preserves any pending decision. The Worker-local inventory revision is independent of the revision an Admin approves. Stale epochs cannot change either snapshot.

The Worker watches its local `worker.yaml` and validates a replacement before activating it. An invalid edit keeps the last valid runtime configuration and reports a local diagnostic. For a valid scheduling change it pauses new claims before switching adapters and reporting the change. It resumes processing hints once the Hub has acknowledged the current inventory. The Hub only offers new work when the Worker is approved and the latest report is applied. Configuration changes preserve the execution handle and cancellation/reporting path for each committed Delivery, including a claim whose reply is in flight. Apply never writes `worker.yaml`, installs a Harness or restarts one.

## Wakeup and Claim

1. For a `queued` Task whose dependencies are all `finished`, the Hub selects one online, approved and unrevoked Worker whose reported revision is applied, with an enabled Slot that meets the Task's required capabilities. It sends `hub.wakeup { taskId, eligibleSlotIds }`. The first hint may honor `preferredWorkerId`. The bounded periodic scan rotates subsequent hints in Worker ID order while the Task remains queued. Rotation is held in memory.
2. The hint leaves the Task queued and creates neither a reservation nor a Delivery. The Worker selects one of the hinted Slots that is free locally and explicitly claims with the Task ID, Slot ID, and live `connectionEpoch` (`POST /v1/workers/claim`).
3. Before creating a Delivery the Hub atomically rechecks approval, the reported and applied revisions, readiness, ownership, applied Slot eligibility, and Delivery uniqueness. Clearing the last approval gate triggers a wakeup pass for queued Tasks. The Worker keeps an unanswered claim in memory and retries the same Task and Slot after reconnect. The Hub returns the same active Delivery to its original Worker and Slot at the current epoch.
4. The winning Worker durably registers the returned `deliveryId` before invoking the Harness. A replay for a registered Delivery does not invoke the Harness again. If the Worker process itself is lost before registration, the Hub retains the assignment and exposes the outstanding Delivery.

Slot and Capability are hard constraints. `preferredWorkerId`, `preferredRole`, and `preferredExecutorId` are soft preferences that never block a claim.

## Connection Identity and Epoch Fencing

- Every accepted WSS connection receives a Hub-issued `connectionId` and a monotonic per-Worker `connectionEpoch` in `hub.welcome`.
- Opening a newer authenticated connection fences older epochs: late frames from an older epoch are ignored and never mutate state.
- Any valid current-epoch frame from Hub refreshes the Worker's `leaseTtlMs` deadline. Missing heartbeats mark the Worker offline and trigger reconnect, but **never prove execution stopped**.

## Minimal WSS Frame Matrix

Every WSS frame uses a closed `camelCase` JSON envelope (`deny_unknown_fields`):
`{ v, kind, messageId, workerId, slotId?, connectionId?, connectionEpoch?, sentAtMs, replyTo?, payload }`.
Frames are capped at 512 KiB; inventory payloads are capped at 256 KiB and 256 Slots.

| Kind | Direction | Purpose |
| --- | --- | --- |
| `worker.hello` | Worker → Hub | Present `workerId`, current inventory (host, Slots, Executors, capabilities), and locally retained active Delivery IDs. |
| `hub.welcome` | Hub → Worker | Confirm `hubId`, `workerId`, `connectionId`, monotonic `connectionEpoch`, and heartbeat/lease timings. |
| `worker.heartbeat` / `hub.heartbeat_ack` | Bidirectional | Connection liveness keepalive (does not resend full inventory). |
| `worker.inventory` / `worker.inventory_ack` | Bidirectional | Synchronize updated Worker configuration/inventory. |
| `hub.wakeup` | Hub → Worker | Hint that one `queued` Task may be claimable (`taskId`, `eligibleSlotIds`); reserves nothing and creates no Delivery. |
| `task.cancel` | Hub → Worker | Request cancellation of a Delivery. |
| `task.cancelled` / `task.cancelled_ack` | Bidirectional | Report Harness cancellation response (`confirmed`, `failed`, `uncertain`, `unsupported`); idempotent ACK by `(deliveryId, epoch)`. |
| `worker.resync` | Worker → Hub | On reconnect, rebind locally retained non-terminal Deliveries to the new `connectionEpoch` so pending HTTP Progress/Result/Finish and WSS cancel reports can flush. |
| `worker.bye` | Worker → Hub | Graceful disconnect notice. |

### Representative Frame Example

```json
{
  "v": "myrlet.worker/1",
  "kind": "hub.wakeup",
  "messageId": "msg_wakeup",
  "workerId": "worker_01",
  "connectionId": "connection_01",
  "connectionEpoch": 1,
  "sentAtMs": 1010,
  "payload": { "taskId": "task_01", "eligibleSlotIds": ["slot-a"] }
}
```

## Delivery Lifecycle and Idempotency

```text
Hub commits the claim ─> replies ClaimTaskResponse
  └─> Worker: register deliveryId durably ─> Adapter.submit()
                 ├─ Watch local Progress file ─> HTTP POST /progress (first accepted ─> start evidence)
                 ├─ Poll local Markdown Result ─> HTTP POST /result (Option Some = set, None = clear)
                 └─ Observe end of handling ─> HTTP POST /finish (Task ─> finished)
```

- **No offer to release**: because the Worker claims after it has already chosen a Slot, there is no `released` state and no pre-send rollback. A claim either commits or loses the race.
- **Worker never reruns Hub policy**: the Worker checks only authenticated epoch, delivery readiness, and presence of the configured Adapter—never capacity or Hub capability matching.
- **No duplicate execution**: Worker durably writes `deliveryId` to its local registry **before** invoking `Adapter.submit()`. A replayed claim reply for an existing `deliveryId` is ignored.
- **Post-registration failure stays unreported**: A local submission/start failure *after* durable registration leaves the Task `dispatched`; Worker never fabricates a failure verdict or silently requeues/resubmits.
- **Restart & reconnect**: Worker loads its local registry before resyncing, waits for a valid non-zero `connectionEpoch` from `hub.welcome`, sends `worker.resync`, and flushes unacknowledged HTTP Progress/Result/Finish (guarded against concurrent duplicate finish posts) and WSS cancel reports. It never blindly resubmits.
- **Delivery states**: `accepted` (committed by a claim), `finished` (handling ended), `cancelled`. There is no `offered`, `rejected`, or `released`.
- **No `offer_acceptance_overdue` timer fencing**: because Progress is optional, Hub must never fence a Delivery merely because no Progress arrived within a timeout.

## Non-Invasive Harness Adapters, Progress, and Result Files

Adapters are thin, non-invasive bridges over existing Harness interfaces (preference order: native SDK/API → native event callbacks → CLI / structured output → external process bridge). Initial V1 targets are **Claude Code**, **Codex**, **Pi**, and plain **Process**.

- **Strict 2-method common Adapter contract**: `submit(request)` and `cancel(execution_ref)` **only**.
- **Explicitly forbidden on the generic Adapter trait**: `capabilities()`, `validate()`, `inspect()`, `reconcile()`, `reattach()`, `resume_execution()`, and interactive `input()` / `steer()` / `follow_up()`. Capability differences stay honest and unsupported fields remain absent.
- **Short startup prompt**: Worker passes only minimal Task identification (`taskId` / `deliveryId` / prompt), the local Progress and Result paths (`<worker>/progress/<delivery_id>.json`, `<worker>/results/<delivery_id>.md`), and the concrete `myrlet done <delivery-id>` command. Harnesses fetch extra Task context via the `myrlet` CLI when needed.
- **Progress file ownership**: Worker creates the `progress/` directory and **never** creates, writes, or edits the Progress file itself. The Harness optionally writes bounded JSON (`{ "text": "..." }`, unknown fields ignored, max 64 KiB file / 16 KiB text); Worker watches changes and relays them via Hub HTTP. Never make an extra LLM call to generate Progress.
- **Result file ownership**: Worker creates the `results/` directory and **never** creates, writes, or edits `<delivery_id>.md`. The Harness optionally writes one UTF-8 Markdown result (max 64 KiB); Worker polls it, classifies the reading, and reconciles the Hub copy by content hash. A missing file is `Absent`; a whitespace-only file is `Empty`; either clears the Hub copy. A symlinked, group/world-accessible, oversized, non-UTF-8, or partially written file is `Unreadable` and **never** clears it. Result sync is independent of Progress and Finish, and never invokes the Harness again.
- **`done` and end of handling**: The Harness may call `myrlet done <delivery-id>` as its last command; the signal is optional, repeatable, and idempotent. It tells the local Worker that handling finished and requests Result reconciliation, but never terminates the Harness. Worker-observed process exit remains the authority for Finish; if `done` is omitted, exit still triggers the final sync and Finish.
- **Zero intrusion**: Never patch/fork a Harness, inject hooks/extensions/MCP servers, rewrite repo files (`AGENTS.md`, `CLAUDE.md`, `.mcp.json`), or manage Harness Sessions, Workspaces, or Provider Credentials.
- **End detection & cancellation**: Worker detects end of handling locally (e.g., child process exit or backend turn-end) and posts HTTP `/finish` without judging success or failure. `cancel(execution_ref)` signals the process group / native interrupt and relays what the backend answered without claiming physical termination of external side effects.
- **Tool Call & Execution Summary**: Outside V1 baseline; never required for Progress or finish.

## Worker Local State, Control Socket, and Shutdown

- Worker uses **one lightweight local store** under `MYRLET_HOME/worker/state/` (atomic per-Delivery JSON files + small `inventory.json`, never two separate storage engines) for Delivery correlation, pending finish/cancel/result intent, the last content hashes reconciled with the Hub, and inventory revision.
- **Local control socket** (`worker/control.sock`, mode `0600`): the `myrlet done` and `myrlet result check/sync/status <delivery-id>` commands write one JSON request line here and read one `LocalReply`. Every such operation names its actual Delivery ID; the Worker resolves its own recorded paths and rejects an ID it never registered. A late or duplicate command for an old Delivery therefore cannot affect a newer Delivery on the same Slot.
- On `SIGTERM` / `SIGINT`, Worker closes WSS, stops admitting claims, drains active work up to `MYRLET_WORKER_SHUTDOWN_TIMEOUT_MS` (default 30s), reaps owned Unix process groups where applicable, and exits without fabricating `finished` reports.
