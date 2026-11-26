---
name: myrlet
description: Create, dispatch, inspect, cancel, and monitor Myrlet Hub Tasks through the headless Myrlet CLI.
---

# Myrlet Tasks

Use the `myrlet` CLI (built on `myrlet-client`). Do not build another HTTP client, scheduler, or Worker loop.
See [references/cli.md](references/cli.md) for exact command syntax and [docs/architecture/README.md](../../docs/architecture/README.md) for the canonical V1 contract.

## Authenticate

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'
export MYRLET_ADMIN_TOKEN='<hub-admin-token>'
```

Never place tokens in command arguments, committed files, Task prompts, or logs.

## Create and Dispatch

Creation persists a `held` Task; dispatch is always an explicit step.

```bash
# Create a held Task
myrlet --json tasks create \
  --idempotency-key issue-42-fix-1 \
  --title 'Fix issue 42' \
  --prompt 'Implement the requested fix and run verification'

# Dispatch via Simple Match
myrlet --json tasks dispatch '<task-id>' \
  --expected-version 1 \
  --idempotency-key issue-42-dispatch-1 \
  --match

# Or create + dispatch from a full TaskDefinition JSON file
myrlet --json tasks submit \
  --file task.json \
  --idempotency-key issue-42-submit-1

# Or atomically import held Tasks from a Bundle (never dispatches)
myrlet --json tasks bundle-import \
  --file bundle.json \
  --idempotency-key issue-42-bundle-1
```

Targeted Dispatch requires explicit `--worker-id`, `--slot-id`, and `--connection-epoch` (`--executor-id` is optional).

## Inspect, Watch, and Cancel

```bash
myrlet --json tasks list
myrlet --json tasks show '<task-id>'
myrlet --json workers list
myrlet --json events watch
myrlet --json tasks cancel '<task-id>'
```

- `tasks list` shows the Snapshot Task summary page (`metadata.tasksTruncated` indicates if truncated). Use `tasks show` for one authoritative Task record.
- `events watch` outputs NDJSON snapshots and SSE invalidation hints; re-read `tasks show` when making decisions.

## Interpret Task Status Honestly

- `held`: durable intent exists; not dispatched.
- `queued`: waiting in Hub's FIFO queue; no Delivery offered and no capacity reserved.
- `dispatched`: Hub offered a Delivery; execution appears started only after Hub accepts a Harness-authored Progress report (`firstReceivedAtMs`).
- `finished`: assigned Worker reported its handling ended. **Not a success or failure verdict.**
- `cancelled`: explicitly cancelled on Hub. Terminal (`finished` never overwrites `cancelled`), and **not** proof that a remote process or side effect stopped.

## Retry Safely

- Repeat a lost create/dispatch request with the **same `--idempotency-key`** and unchanged payload.
- Never assume a disconnected Worker stopped executing. A business retry creates a **new `held` Task** linked via `sourceTaskId` and dispatches it explicitly.
