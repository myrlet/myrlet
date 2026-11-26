# Hub API

Hub exposes authenticated HTTP commands/queries and an SSE event stream over HTTPS (or loopback HTTP for local development). Workers connect over WSS for control frames and call two dedicated Hub HTTP endpoints for Progress and Finish reporting.

**Wire convention:** All HTTP request and response JSON bodies use **`camelCase`** consistently (no mixed `snake_case`/`camelCase` aliases). Error responses use `{ "error": "<StableCode>", "message": "<fixed safe message>" }` without leaking internal SQLite causes.

## V1 Endpoint Surface

### Public & Health

| Method & Path | Purpose |
| --- | --- |
| `GET /health`, `GET /v1/health` | Process liveness probe (unauthenticated, unbounded). |
| `POST /v1/pair` | Redeem a one-time pairing code (`{ enrollmentCode, workerId }`) for `{ hubId, workerId, credential }` in one step (body/timeout/concurrency bounded). |

### Admin / Client Control Plane (`Authorization: Bearer <admin-token>`)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/snapshot` | Primary read view: cluster/Worker inventory, bounded Task summaries, current Deliveries & latest Progress, and current `eventCursor`. |
| `GET /v1/events` | Resumable SSE invalidation stream (`Last-Event-ID` or `?cursor=<seq>`). |
| `POST /v1/tasks` | Create one `held` Task (`Idempotency-Key` required). |
| `GET /v1/tasks/{id}` | Read one authoritative Task record, its current Delivery, and latest Progress. |
| `GET /v1/tasks/{id}/tree` | Read bounded ancestors or descendants (depth & node cap; simple limit, no view-fingerprint cursor). |
| `POST /v1/tasks/{id}/dispatch` | Explicitly dispatch (`targeted` or `match`, optional `queue` & `fallbackAllowed`) (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/cancel` | Cancel any non-terminal Task (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/tasks/{id}/finish` | Explicitly finish a coordination-only `held` Parent with no Delivery (`Idempotency-Key` + `expectedTaskVersion`). |
| `POST /v1/task-bundles/import` | Atomically create up to 100 `held` Tasks with local `clientRef` resolution (`Idempotency-Key`). |
| `POST /v1/enrollment-codes` | Issue a short-lived one-time Worker pairing code (`{ ttlSeconds, suggestedWorkerId? } -> { enrollmentCode, expiresAt }`). |
| `GET /v1/workers` | List enrolled Workers, connectivity, and reported Slot/Executor inventory. |
| `GET /v1/workers/{id}` | Read one Worker's detail and inventory. |
| `POST /v1/workers/{id}/rotate` | Rotate a Worker's bearer credential (fences active WSS session). |
| `POST /v1/workers/{id}/revoke` | Revoke a Worker's credential and disconnect its active WSS session. |

### Worker Control & Reporting (`Authorization: Bearer <worker-token>`)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/workers/connect` | Authenticated Worker WSS upgrade endpoint. |
| `POST /v1/worker/deliveries/{delivery_id}/progress` | Report latest Harness-authored Progress. |
| `POST /v1/worker/deliveries/{delivery_id}/finish` | Report Worker end of handling. |

## Worker Progress and Finish Reporting (HTTP)

Progress and Finish are **HTTP-only** (not WSS frames). Both require the Worker's bearer token matching the Delivery's assigned `workerId` and validate `epoch` against the Delivery's current `connectionEpoch` (stale epoch returns `409 StaleDelivery`).

### `POST /v1/worker/deliveries/{delivery_id}/progress`

```json
{ "epoch": 7, "observedAtMs": 1764070000123, "text": "..." }
```

- `text` is a single bounded UTF-8 string (max 16 KiB).
- Hub stores **one latest-state row per Delivery** (no Progress history).
- Equal `observedAtMs` is an idempotent duplicate; older `observedAtMs` is rejected so retries cannot move Progress backwards.
- Returns `ProgressReceipt { deliveryId, acceptedAtMs, firstAcceptedAtMs, duplicate }`.
- The first accepted Progress sets immutable `firstAcceptedAtMs` (execution-start evidence) and promotes an `offered` Delivery to `accepted` in the same transaction.
- Progress never changes Task status and remains accepted even after cancellation.

### `POST /v1/worker/deliveries/{delivery_id}/finish`

```json
{ "epoch": 7 }
```

- Outcome-free: carries no success/failure flag, exit code, summary, or artifact.
- Returns `FinishReceipt { deliveryId, taskId, taskStatus, changed }`.
- Idempotent on replay (`changed: false`).
- If the Task is already `cancelled`, Hub returns `cancelled` unchanged and never reverts it.
- Otherwise Hub moves Task and Delivery to `finished`. An `offered` Delivery may transition directly to `finished` when a Harness wrote no Progress.

## Global Snapshot

`GET /v1/snapshot` builds a consistent read from Hub SQLite and in-memory Worker connection state without calling Workers synchronously. It returns:

- `schemaVersion` (`"myrlet.snapshot/v1"`), `generatedAt`, `eventCursor`, and `tasksTruncated` (boolean + total count when Task summaries exceed the Snapshot page bound);
- connected/disconnected Workers with their reported Slots, capabilities, Executors, observation timestamps, and advisory capacity hints (absent capacity MUST be `null`, never `0`);
- Task summaries (status, version, hierarchy, current Delivery, and latest `DeliveryProgressView` including `firstReceivedAtMs` / `receivedAtMs` / `text`).

Snapshot is purely observational and never reserves capacity.

## Event Log and SSE

Hub writes one row to a single append-only `hub_events(seq INTEGER PRIMARY KEY AUTOINCREMENT, kind, payload_json, created_at_ms)` table inside each committed state-transition transaction, then immediately publishes the event to an in-process `broadcast` channel after commit. There is no separate multi-table polling Outbox worker.

- SSE frames are **invalidation hints**, not authoritative state. Clients refresh Snapshot or Task detail when needed.
- Clients reconnect with `Last-Event-ID` (or `?cursor=<seq>`). Hub backfills from `hub_events` up to the retained window (e.g., last 10,000 events); if the cursor falls behind retention, Hub fails closed with `410 CursorExpired` so the client reloads `GET /v1/snapshot`.

## Ingress Bounds and Shutdown

- Authenticated routes verify the bearer token **before** reading/parsing request bodies (`401` on invalid token).
- Command requests enforce configurable bounds (defaults: 2 MiB max body, 1 MiB for Bundle import, 30s timeout, 128 concurrent requests; over-limit returns `413` / `408` / `503`). `/health`, `/v1/health`, SSE, and WSS are excluded from request timeout and concurrency permits.
- On `SIGTERM` / `SIGINT`, Hub stops admitting new mutations, drains in-flight requests up to `MYRLET_HUB_SHUTDOWN_TIMEOUT_MS` (default 30s), and closes sockets cleanly without fabricating terminal Task outcomes.
