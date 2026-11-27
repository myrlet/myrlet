# Hub API

Hub exposes authenticated HTTP commands/queries and an SSE event stream over HTTPS (or loopback HTTP for local development). Workers connect over WSS for control frames and report Progress, Result, and Finish over HTTP.

**Wire convention:** All HTTP request and response JSON bodies use **`camelCase`** consistently (no mixed `snake_case`/`camelCase` aliases). Error responses use `{ "error": "<StableCode>", "message": "<fixed safe message>" }` without leaking internal SQLite causes.

## Proposed reporting semantics

One effective Delivery binds a Task to one Worker and one Slot. That Worker is the single writer for two independent values, Progress and Result. The Hub keeps the latest copy of each. Compare hashes over agreed content bytes and reconcile differing values to the current Worker observation. A Markdown digest covers its text and reference strings. Equal content requires no write. Result can change after Finish. Reconciliation never changes the Task outcome, and the Hub stores no revision counters or version history.

After local recovery completes, an absent or empty Result file means **no Result** and clears a previous Hub value. A read error, an unsafe file, an invalid payload, or incomplete recovery cannot establish absence. Preserve the Hub copy until the Worker can establish its local state. The Result clearing rule applies only to Result.

Serialize sync per Delivery and reread current content on each attempt. Before sending newer content after a timeout, settle the outstanding write or fence its connection epoch. Validate Delivery ownership and epoch at the Hub commit boundary. A delayed request carrying an old epoch cannot undo a newer committed value. Distinguish local receipt, Hub confirmation, and inability to compare when disconnected. Exact digest inputs and wire encodings remain open.

Client operations in the proposal include submit, atomic import, version-fenced pending edits, cancellation, and global reads. The [Task model](task-model.md#proposed-task-lifecycle) specifies their effects. The proposed read credential and claim exchange await review; no new route is specified here.

## V1 Endpoint Surface

The route tables and reporting payloads below document the **currently implemented** API. In particular, the write-once Result response below applies to the current binary.

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

### Admin / Client Control Plane (`Authorization: Bearer <admin-token>`)

| Method & Path | Purpose |
| --- | --- |
| `GET /v1/snapshot` | Primary read view: cluster/Worker inventory, bounded Task summaries, current Deliveries & latest Progress, and current `eventCursor`. |
| `GET /v1/events` | Resumable SSE invalidation stream (`Last-Event-ID` or `?cursor=<seq>`). |
| `POST /v1/tasks` | Create one `held` Task (`Idempotency-Key` required). |
| `GET /v1/tasks/{id}` | Read one authoritative Task record, its current Delivery, latest Progress, and immutable result. |
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
| `POST /v1/worker/deliveries/{delivery_id}/result` | Report the optional final Harness-written Markdown result. |
| `POST /v1/worker/deliveries/{delivery_id}/finish` | Report Worker end of handling. |

## Worker Progress, Result, and Finish Reporting (HTTP)

Progress, Result, and Finish are **HTTP-only** (not WSS frames). All require the Worker's bearer token matching the Delivery's assigned `workerId` and validate `epoch` (stale epoch returns `409 StaleDelivery`).

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

### `POST /v1/worker/deliveries/{delivery_id}/result`

```json
{ "epoch": 7, "text": "# Findings\n\n- one" }
```

- `text` is one bounded UTF-8 Markdown string (max 64 KiB, non-blank).
- Accepted for an `accepted` Delivery and, independently of Finish, for a `finished` or `cancelled` Delivery: the Worker's live `epoch` still fences each report, so a late sync after reconnect lands.
- **Current wire behavior**: the first stored text is immutable. An identical replay returns `ResultReceipt { deliveryId, taskId, recordedAtMs, duplicate: true }` without a new event; different text returns `409 InvalidTransition`.
- Returns `ResultReceipt { deliveryId, taskId, recordedAtMs, duplicate }` and emits one `delivery.result` invalidation event.
- A Result never changes Task status and is not a verdict. Hub stores it on the Delivery; `GET /v1/tasks/{id}` exposes it as `resultMarkdown` + `resultRecordedAtMs`, and it is **never** part of the Snapshot or a cross-Task read.

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
