# Hub API

Hub exposes authenticated HTTP commands/queries and an SSE event stream over HTTPS (or loopback HTTP for local development). Workers connect over WSS for wakeup hints and control frames, and call dedicated Hub HTTP endpoints for claims, Progress, Result, and Finish reporting.

**Wire convention:** All HTTP request and response JSON bodies use **`camelCase`** consistently (no mixed `snake_case`/`camelCase` aliases). Error responses use `{ "error": "<StableCode>", "message": "<fixed safe message>" }` without leaking internal SQLite causes.

## Read Authorization

Ordinary reads (`GET /v1/snapshot`, `/v1/tasks`, `/v1/tasks/{id}`, `/v1/tasks/{id}/tree`, `/v1/tasks/{id}/eligibility`, `/v1/events`, `/v1/workers`, `/v1/workers/{id}`) accept **either** the Admin token **or** any enrolled Worker credential. A Harness therefore never needs the Hub Admin token merely to inspect Tasks, trees, Workers, or events. Reads grant **no** management authority: every write route still requires the Admin principal. See [security](security.md#read-visibility).

## V1 Endpoint Surface

### Public & Health

| Method & Path | Purpose |
| --- | --- |
| `GET /health`, `GET /v1/health` | Process liveness probe (unauthenticated, unbounded). |
| `POST /v1/pair` | Redeem a one-time pairing code (`{ enrollmentCode, workerId }`) for `{ hubId, workerId, credential }` in one step (body/timeout/concurrency bounded). |

### Embedded Web UI (same-origin)

`hub` embeds the built React/TypeScript/Vite Client (`apps/web`) in its binary and serves it from the Hub origin, so the browser talks to the same origin as the API (zero CORS).

- `GET`/`HEAD` on any path outside `/v1/*` and `/health` serves a static asset; unknown non-asset paths fall back to `index.html` for client-side (SPA) routing.
- Enabled by default (`uiEnabled = true`); set `MYRLET_HUB_UI=false` or `uiEnabled = false` in `hub/config.toml` to disable it, after which UI paths return the JSON `404 NotFound` error body.
- `/v1/*` NEVER falls back to HTML: an unknown `/v1/*` path always returns the JSON `404 NotFound` error body, whether or not the UI is enabled.
- Every UI response carries strict security headers: `Content-Security-Policy` (self-only scripts and connections, `frame-ancestors 'none'`), `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and `X-Frame-Options: DENY`.
- The Admin bearer token stays in browser memory only — never written to `localStorage`/`sessionStorage`, cookies, or URLs.

### Reads (Admin **or** Worker credential)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/snapshot` | Primary read view: cluster/Worker inventory, bounded Task summaries, current Deliveries & latest Progress, and current `eventCursor`. |
| `GET /v1/tasks` | Bounded newest-first Task summaries (`?limit=`); returns `totalTasks`, `truncated`, and `eventCursor`. |
| `GET /v1/tasks/{id}` | Read one authoritative Task record, its current Delivery, latest Progress, `blockedBy`, and latest Result. |
| `GET /v1/tasks/{id}/tree` | Read bounded ancestors or descendants (depth & node cap; simple limit, no view-fingerprint cursor). |
| `GET /v1/tasks/{id}/eligibility` | Whether the Task can be claimed now, and which dependencies block it. |
| `GET /v1/events` | Resumable SSE invalidation stream (`Last-Event-ID` or `?cursor=<seq>`). |
| `GET /v1/workers` | List enrolled Workers, connectivity, and reported Slot/Executor inventory. |
| `GET /v1/workers/{id}` | One Worker's detail, inventory, and the `claimableTaskIds` its Slots could run next. |

### Admin Control Plane (`Authorization: Bearer <admin-token>`)

| Method & Path | Purpose |
| --- | --- |
| `POST /v1/tasks` | Submit one Task, authorizing execution (`Idempotency-Key` required). Returns `SubmitTaskResponse { task, eligible }`. |
| `POST /v1/tasks/{id}` | Version-fenced pending edit of `dependencies` on an unclaimed Task (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/cancel` | Cancel any non-terminal Task (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/finish` | Explicitly finish a coordination-only Task with no Delivery (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/task-bundles/import` | Atomically submit up to 100 Tasks with local `clientRef` resolution (`Idempotency-Key`). |
| `POST /v1/enrollment-codes` | Issue a short-lived one-time Worker pairing code (`{ ttlSeconds, suggestedWorkerId? } -> { enrollmentCode, expiresAt }`). |
| `POST /v1/workers/{id}/rotate` | Rotate a Worker's bearer credential (fences active WSS session). |
| `POST /v1/workers/{id}/revoke` | Revoke a Worker's credential and disconnect its active WSS session. |

### Worker Control & Reporting (`Authorization: Bearer <worker-token>`)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/workers/connect` | Authenticated Worker WSS upgrade endpoint. |
| `POST /v1/workers/claim` | Explicitly claim one `queued` Task for one Slot; commits the Task's one Delivery. |
| `POST /v1/worker/deliveries/{delivery_id}/progress` | Report latest Harness-authored Progress. |
| `POST /v1/worker/deliveries/{delivery_id}/result` | Reconcile the Delivery's latest Markdown Result, or clear it. |
| `GET /v1/worker/deliveries/{delivery_id}/result` | Read the current Hub Result copy for a `result status` comparison (no write). |
| `POST /v1/worker/deliveries/{delivery_id}/finish` | Report Worker end of handling. |

## Claiming a Task

`POST /v1/workers/claim`

```json
{ "taskId": "task_…", "slotId": "slot-a", "connectionEpoch": 7, "executorId": null }
```

- The Worker credential identifies the claimant; `connectionEpoch` must equal the Worker's live WSS epoch, otherwise `409 StaleDelivery`.
- In one transaction the Hub rechecks: Task `queued` with every dependency `finished`, no existing Delivery for the Task, the named Slot exists and is enabled, every `requiredCapabilities` entry is `available`, and any requested Executor is present on that Slot.
- If `executorId` is omitted the Hub resolves the Task's own `preferredExecutorId` against the Slot (falling back to the Slot's first Executor); a preference never blocks a claim.
- Exactly one Delivery is created (`state: accepted`) and the Task moves to `dispatched`. Racing claims either lose the version check or violate the unique Delivery index, so exactly one wins; a loser receives `409 InvalidTransition` / `409 AlreadyClaimed`.
- Retrying an active claim at the live epoch with the same Worker and Slot returns the existing Delivery and binds it to that epoch. Replay emits no second dispatch event. A different Worker or Slot cannot recover that claim.
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
- connected/disconnected Workers with their reported Slots, capabilities, Executors, observation timestamps, and advisory capacity hints (absent capacity MUST be `null`, never `0`);
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
