# Hub API

Hub exposes authenticated HTTP commands/queries and an SSE event stream over HTTPS (or loopback HTTP for local development). Workers connect over WSS for wakeup hints and control frames, and call dedicated Hub HTTP endpoints for claims, Progress, Result, and Finish reporting.

**Wire convention:** All HTTP request and response JSON bodies use **`camelCase`** consistently (no mixed `snake_case`/`camelCase` aliases). Error responses use `{ "error": "<StableCode>", "message": "<fixed safe message>" }` without leaking internal SQLite causes.

## Authorization Model

The Hub authenticates either the Admin principal or a Worker principal. A Worker principal has a server-controlled `permissionTier` of `worker` or `owner`; both tiers use the same Worker credential and `workerId`. See [Security](security.md) for the complete rules.

- **Worker-tier credential:** Worker-bound claim/report operations; full read of Tasks bound to its own Delivery; one-hop limited summaries of directly related Tasks.
- **Owner-tier Worker credential:** all Worker operations plus global Task reads, supported Task coordination writes, and read-only Worker/Agent observability.
- **Admin credential:** global reads, supported Task coordination writes, and Worker/system administration. Admin cannot impersonate a Worker for WSS, claims, Progress, Result, Finish, or cancellation acknowledgements.

Worker assignment for read authorization is established by a committed Delivery's `workerId`; `preferredWorkerId` is not an assignment. A related-Task read is limited to the direct parent, direct children, direct dependencies, and direct dependents of the assigned Task, and returns only the limited summary defined in [Security](security.md#task-read-visibility). It does not recursively expand visibility.

Global reads and streams are Owner/Admin-only. Worker credentials cannot enumerate all Tasks, events, or Workers. Worker reads of `GET /v1/tasks/{id}`, `/tree`, and `/eligibility` are authorized against the Worker's own Delivery and the direct related-Task boundary. A valid credential that lacks the required tier receives `403 Forbidden`; a Task outside the Worker's visibility boundary may return `404 NotFound` to avoid disclosing its existence. Invalid or missing credentials return `401 Unauthorized`.

Every HTTP, SSE, WSS, Snapshot, and resource path enforces authorization at the Hub. Client-side filtering is not an authorization boundary.

## V1 Endpoint Surface

### Public & Health

| Method & Path | Purpose |
| --- | --- |
| `GET /health`, `GET /v1/health` | Process liveness probe (unauthenticated, unbounded). |
| `POST /v1/pair` | Redeem a one-time pairing code (`{ enrollmentCode, workerId }`) for `{ hubId, workerId, credential }` in one step (body/timeout/concurrency bounded). |

### Embedded Web UI (Admin-only)

`hub` embeds the built React/TypeScript/Vite Client (`apps/web`) in its binary and serves it from the Hub origin, so the browser talks to the same origin as the API (zero CORS).

- Every non-CLI Client, including the Web UI and any future Desktop shell, requires an Admin credential. The Web UI does not expose Worker-tier or Owner-tier access.
- `GET`/`HEAD` on any path outside `/v1/*` and `/health` serves a static asset; unknown non-asset paths fall back to `index.html` for client-side (SPA) routing.
- Enabled by default (`uiEnabled = true`); set `MYRLET_HUB_UI=false` or `uiEnabled = false` in `hub/config.toml` to disable it, after which UI paths return the JSON `404 NotFound` error body.
- `/v1/*` NEVER falls back to HTML: an unknown `/v1/*` path always returns the JSON `404 NotFound` error body, whether or not the UI is enabled.
- Every UI response carries strict security headers: `Content-Security-Policy` (self-only scripts and connections, `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and `X-Frame-Options: DENY`.
- The Admin bearer token stays in browser memory only — never written to `localStorage`/`sessionStorage`, cookies, or URLs.

### Reads (Owner/Admin global; Worker-scoped Task detail)

| Method & Path | Authorization and purpose |
| --- | --- |
| `GET /v1/snapshot` | Owner/Admin. Primary global read view: cluster/Worker inventory, bounded Task summaries, current Deliveries & latest Progress, and current `eventCursor`. Includes Worker-reported Agent runtime observations only where such telemetry is available. |
| `GET /v1/tasks` | Owner/Admin. Bounded newest-first global Task summaries (`?limit=`); returns `totalTasks`, `truncated`, and `eventCursor`. |
| `GET /v1/tasks/{id}` | Owner/Admin may read any Task in full. Worker may read its own Delivery-bound Task in full, or a directly related Task as a limited summary. |
| `GET /v1/tasks/{id}/tree` | Owner/Admin may read the bounded tree. Worker access is anchored to its own Delivery-bound Task and returns only one-hop related-Task summaries. |
| `GET /v1/tasks/{id}/eligibility` | Owner/Admin may inspect any Task. Worker may inspect only its own Task or a directly related Task. |
| `GET /v1/events` | Owner/Admin. Resumable global SSE invalidation stream (`Last-Event-ID` or `?cursor=<seq>`). Worker credentials do not receive the global event stream. |
| `GET /v1/workers` | Owner/Admin. Owner receives operational status and scheduling-relevant inventory; Admin additionally receives administration-only fields. |
| `GET /v1/workers/{id}` | Owner/Admin. Owner receives Worker status, scheduling inventory and reported Agent runtime observations; Admin receives the full administrative view. |
| `GET /v1/settings/security-trust` | Admin only. Read the two Hub-wide policy booleans. |

### Task Coordination (Owner-tier Worker credential or Admin)

| Method & Path | Purpose |
| --- | --- |
| `POST /v1/tasks` | Submit one Task, authorizing execution (`Idempotency-Key` required). Returns `SubmitTaskResponse { task, eligible }`. |
| `POST /v1/tasks/{id}` | Version-fenced pending edit of `dependencies` on an unclaimed Task (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/cancel` | Cancel any non-terminal Task (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/finish` | Explicitly finish a coordination-only Task with no Delivery (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/task-bundles/import` | Atomically submit up to 100 Tasks with local `clientRef` resolution (`Idempotency-Key`). |

This tier exposes the Task coordination operations already defined by the V1 Task Model. It does not introduce a separate dispatch or assignment API; Workers still claim queued Tasks, and `preferredWorkerId` remains a soft preference.

### Admin Control Plane (`Authorization: Bearer <admin-token>`)

| Method & Path | Purpose |
| --- | --- |
| `POST /v1/enrollment-codes` | Issue a short-lived one-time Worker pairing code (`{ ttlSeconds, suggestedWorkerId? } -> { enrollmentCode, expiresAt }`). |
| `POST /v1/workers/{id}/rotate` | Rotate a Worker's bearer credential (fences active WSS session and preserves its permission tier). |
| `POST /v1/workers/{id}/revoke` | Revoke a Worker's credential and disconnect its active WSS session. |
| `PATCH /v1/workers/{id}/permission-tier` | Set `{ permissionTier: "worker" | "owner" }` and return the stored tier. Admin-only; the `admin` tier cannot be assigned to a Worker. |
| `PATCH /v1/settings/security-trust` | Admin updates one or both boolean policies and receives the stored pair. Omitted fields retain their values. |
| `POST /v1/workers/{id}/approve` | Approve a pending Worker. Repeating approval for an approved Worker succeeds. A revoked Worker requires a new Pair. |
| `POST /v1/workers/{id}/config/apply` | Apply `{ revision }` and return `{ appliedRevision }`. The Hub checks and copies the latest pending report in one transaction. |

Apply on a revoked Worker returns `409 InvalidTransition`. With no pending report it returns `409 NoPendingConfig`. With a pending report and a different revision it returns `409 ConfigRevisionMismatch` and asks the operator to inspect the latest report. Each error leaves Applied unchanged.

### Worker Control & Reporting (Worker credential: `worker` or `owner` tier)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/workers/connect` | Authenticated Worker WSS upgrade endpoint. |
| `POST /v1/workers/claim` | Explicitly claim one `queued` Task for one Slot; commits the Task's one Delivery. |
| `POST /v1/worker/deliveries/{id}/progress` | Report latest Harness-authored Progress for a Delivery bound to this Worker. |
| `POST /v1/worker/deliveries/{id}/result` | Reconcile the Delivery's latest Markdown Result, or clear it, for a Delivery bound to this Worker. |
| `GET /v1/worker/deliveries/{id}/result` | Read the current Hub Result copy for this Worker's own Delivery (`result status` comparison; no write). |
| `POST /v1/worker/deliveries/{id}/finish` | Report Worker end of handling for a Delivery bound to this Worker. |

The Admin principal is not accepted on Worker Control & Reporting routes.

## Claiming a Task

`POST /v1/workers/claim`

```json
{ "taskId": "task_…", "slotId": "slot-a", "connectionEpoch": 7, "executorId": null }
```

- The Worker credential identifies the claimant; `connectionEpoch` must equal the Worker's live WSS epoch, otherwise `409 StaleDelivery`.
- To create a new Delivery the Hub also requires Worker approval and a non-null applied revision matching the latest report. Otherwise it returns `409 InvalidTransition`. In the same transaction the Hub rechecks: Task `queued` with every dependency `finished`, no existing Delivery for the Task, the named Slot exists and is enabled, every `requiredCapabilities` entry is `available`, and any requested Executor is present on that Slot.
- If `executorId` is omitted the Hub resolves the Task's own `preferredExecutorId` against the Slot (falling back to the Slot's first Executor); a preference never blocks a claim.
- Exactly one Delivery is created (`state: accepted`) and the Task moves to `dispatched`. Racing claims either lose the version check or violate the unique Delivery index, so exactly one wins; a loser receives `409 InvalidTransition` / `409 AlreadyClaimed`.
- The approval gates apply only to new Deliveries. Retrying an active claim at the live epoch with the same Worker and Slot returns the existing Delivery and binds it to that epoch. Replay emits no second dispatch event. A different Worker or Slot cannot recover that claim.
- Returns `ClaimTaskResponse { task, delivery }`.

## Worker Progress, Result, and Finish Reporting (HTTP)

Progress, Result, and Finish are **HTTP-only** (not WSS frames). All require the Worker's bearer token matching the Delivery's assigned `workerId` and validate `epoch` (a stale epoch or revoked credential returns `409 StaleDelivery` / `401`). The Hub keeps **one latest value per Delivery** for each of Progress and Result; there is no history and no revision counter.

### `POST /v1/worker/deliveries/{delivery_id}/progress`

```json
{ "epoch": 7, "observedAtMs": 1764070000123, "text": "..." }
```

- `text` is a single bounded UTF-8 string (max 16 KiB).
- Hub stores **one latest-value row per Delivery**.
- Equal `observedAtMs` is an idempotent duplicate; older `observedAtMs` is rejected so retries cannot move Progress backwards.
- Returns `ProgressReceipt { deliveryId, acceptedAtMs, firstAcceptedAtMs, duplicate }`.
- The first accepted Progress sets immutable `firstAcceptedAtMs` (execution-start evidence).
- Progress never changes Task status and remains accepted even after cancellation.

### `POST /v1/worker/deliveries/{delivery_id}/finish`

```json
{ "epoch": 7 }
```

- Outcome-free: carries no success/failure flag, exit code, summary, or artifact.
- Returns `FinishReceipt { deliveryId, taskId, taskStatus, changed }`.
- Idempotent on replay (`changed: false`).
- If the Task is already `cancelled`, Hub returns `cancelled` unchanged and never reverts it.
- Otherwise Hub moves Task and Delivery to `finished`, and releases any waiting siblings that become ready.

### `POST /v1/worker/deliveries/{delivery_id}/result`

```json
{ "epoch": 7, "text": "# Findings\n\n- one" }
```

- `text` is `Some(bounded Markdown)` (max 64 KiB, non-blank) to set the value, or `null` to state that a completed local recovery found **no Result** and clear any stored copy.
- The Hub keeps the latest value plus the SHA-256 of its exact text. An identical digest is a no-op (`duplicate: true`). Different content **replaces** the value — there is no write-once rule and no history.
- Accepted for a live, `finished`, or `cancelled` Delivery: a live epoch still fences every write, so a delayed request carrying an old epoch is refused at this boundary and can never undo a newer value.
- If a Result write has an unknown outcome, the Worker invalidates the last confirmed hash and stops issuing Result writes on that epoch. It reconnects, rereads the file, and reconciles on the new epoch even if the content matches the previously confirmed value.
- A `null` clear is only legitimate after the Worker has completed local recovery and confirmed absence or emptiness; an unreadable, unsafe, or partially written file must never be treated as absence.
- Returns `ResultReceipt { deliveryId, taskId, recordedAtMs, resultSha256, cleared, duplicate }` and emits one `delivery.result` invalidation event when it changes anything.
- A Result never changes Task status and is not a verdict. Hub stores it on the Delivery; `GET /v1/tasks/{id}` exposes it as `resultMarkdown` + `resultRecordedAtMs`. It is **not** part of the Snapshot.

### `GET /v1/worker/deliveries/{delivery_id}/result`

Returns `DeliveryResultView { deliveryId, taskId, text, recordedAtMs, resultSha256 }` for the Delivery's own Worker so `myrlet result status` can compare the local copy with the Hub copy without writing either side.

### Result and Progress convergence

The Delivery's Worker is the single writer of both values. It serializes reconciliation per Delivery, rereads current local content on every attempt, and compares content hashes so equal values cause no write. Progress and Result are independent: updating either leaves the other untouched. The Hub write boundary validates Delivery ownership and epoch, which is the whole conflict story — there is no revision counter, history, multi-writer resolution, content CAS, or distinct missing/clear business state.

## Global Snapshot

`GET /v1/snapshot` builds a consistent read from Hub SQLite and in-memory Worker connection state without calling Workers synchronously. It returns:

- `schemaVersion` (`"myrlet.snapshot/v1"`), `generatedAt`, `eventCursor`, and `tasksTruncated` (boolean + total count when Task summaries exceed the Snapshot page bound);
- connected/disconnected Workers with approval status, reported/applied revisions and their applied Slots, capabilities, Executors, observation timestamps, and advisory capacity hints (absent capacity MUST be `null`, never `0`);
- Task summaries (status, version, hierarchy, `dependencies`, `blockedBy`, current Delivery, and latest `DeliveryProgressView`).

Snapshot is purely observational and never reserves capacity.

## Event Log and SSE

Hub writes one row to a single append-only `hub_events(seq INTEGER PRIMARY KEY AUTOINCREMENT, kind, payload_json, created_at_ms)` table inside each committed state-transition transaction, then immediately publishes the event to an in-process `broadcast` channel after commit. There is no separate multi-table polling Outbox worker.

- SSE frames are **invalidation hints**, not authoritative state. Clients refresh Snapshot or Task detail when needed.
- Clients reconnect with `Last-Event-ID` (or `?cursor=<seq>`). Hub backfills from `hub_events` up to the retained window (e.g., last 10,000 events); if the cursor falls behind retention, Hub fails closed with `410 CursorExpired` so the client reloads `GET /v1/snapshot`.

## Ingress Bounds and Shutdown

- Authenticated routes verify the bearer token **before** reading/parsing request bodies (`401` on invalid token).
- Command requests enforce configurable bounds (defaults: 2 MiB max body, 1 MiB for Bundle import, 30s timeout, 128 concurrent requests; over-limit returns `413` / `408` / `503`). `/health`, `/v1/health`, SSE, and WSS are excluded from request timeout and concurrency permits.
- On `SIGTERM` / `SIGINT`, Hub stops admitting new mutations, drains in-flight requests up to `MYRLET_HUB_SHUTDOWN_TIMEOUT_MS` (default 30s), and closes sockets cleanly without fabricating terminal Task outcomes.
