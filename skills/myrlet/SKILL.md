---
name: myrlet
description: Submit, inspect, claim, and monitor Myrlet Hub Tasks, and report done or read results through the headless Myrlet CLI.
---

# Myrlet Tasks

Use the `myrlet` CLI (built on `myrlet-client`). Do not build another HTTP client, scheduler, or Worker loop.
See [references/cli.md](references/cli.md) for exact command syntax and [docs/architecture/README.md](../../docs/architecture/README.md) for the canonical V1 contract.

## Authenticate

```bash
export MYRLET_HUB_URL='http://127.0.0.1:4140'
export MYRLET_ADMIN_TOKEN='<hub-admin-token>'   # Owner writes only
```

Never place tokens in command arguments, committed files, Task prompts, or logs.

## Submit and Inspect

Submission authorizes execution. A Task with unfinished prerequisites stays `held`; one whose prerequisites are all `finished` becomes `queued` and waits for a Worker to claim it. You never dispatch.

```bash
# Submit one Task (authorizes it)
myrlet --json tasks submit \
  --idempotency-key issue-42-fix-1 \
  --title 'Fix issue 42' \
  --prompt 'Implement the requested fix and run verification'

# Or submit a full TaskDefinition JSON file
myrlet --json tasks submit --file task.json --idempotency-key issue-42-1

# Or atomically submit a Bundle (up to 100 Tasks, local clientRef references)
myrlet --json tasks import --file bundle.json --idempotency-key issue-42-bundle-1

# Inspect every Task, tree, blocker, Worker, and event (Admin or Worker credential)
myrlet --json tasks list
myrlet --json tasks show '<task-id>'
myrlet --json tasks tree '<task-id>'
myrlet --json workers list
myrlet --json workers show '<worker-id>'
myrlet --json events watch
```

`tasks show` includes `blockedBy` (the dependencies keeping a Task `held`), the current Delivery, latest Progress, and the latest result Markdown.

## Worker Trust

A freshly paired Worker is `pending` by default: it can connect and read, but claims nothing until an Admin approves it. Its first scheduling report also waits for an explicit Apply. Operators handle both:

```bash
myrlet --json workers list                              # approval + reported/applied revisions
myrlet --json workers show '<worker-id>'                # same detail for one Worker
myrlet --json workers config show '<worker-id>'         # both snapshots, for review
myrlet --json workers approve '<worker-id>'
myrlet --json workers config apply '<worker-id>' --revision '<revision>'
```

Approval and Apply are independent and may happen in either order. If Apply answers `ConfigRevisionMismatch`, re-read the Worker; the report changed since you looked. These commands never change an existing Worker retroactively.

## Change Pending Work and Cancel

Before a Task is claimed you may change its dependencies; the Hub recomputes `held`/`queued`.

```bash
myrlet --json tasks update '<task-id>' --expected-version '<n>' --depends-on '<other-id>'
myrlet --json tasks cancel '<task-id>'
myrlet --json tasks finish '<task-id>'   # coordination-only Task with no Delivery
```

Cancelling a prerequisite does **not** change its dependents: they stay `held` until you edit their dependencies or cancel them.

## Report from a Harness

Every execution-local command names its real Delivery ID (from the startup prompt) and uses only `MYRLET_HOME`; it needs no Hub token. A Delivery ID from an old execution never affects a newer one on the same Slot.

```bash
myrlet done '<delivery-id>'              # last command before exit; requests sync and finish
myrlet result check '<delivery-id>'      # advisory Markdown feedback; warnings never block
myrlet result sync '<delivery-id>'       # reconcile the Hub copy now
myrlet result status '<delivery-id>'     # compare local and Hub copies; never writes
```

The reply separates local receipt from Hub confirmation. `done` is optional and repeatable: Worker-observed exit still triggers the final sync and Finish.

## Interpret Task Status Honestly

- `held`: submitted, but a named prerequisite is not `finished`.
- `queued`: authorized and ready; waiting for an eligible Worker to claim it.
- `dispatched`: a Worker claimed the Task; a Delivery exists. Execution appears started only after Hub accepts a Progress report (`firstReceivedAtMs`).
- `finished`: the Delivery's Worker reported handling ended. **Not a success or failure verdict.**
- `cancelled`: explicitly cancelled. Terminal (`finished` never overwrites `cancelled`), and **not** proof that a remote process or side effect stopped.

## Retry Safely

- Repeat a lost request with the **same `--idempotency-key`** and unchanged payload.
- Never assume a disconnected Worker stopped executing. A business retry creates a **new** Task linked via `sourceTaskId` and submits it explicitly.
