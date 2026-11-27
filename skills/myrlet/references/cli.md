# Myrlet CLI Reference

Executable: `myrlet`.

## Commands

| Command | Credential | Purpose |
| --- | --- | --- |
| `myrlet [--json] tasks list [--limit <n>]` | Admin or Worker | Bounded newest-first Task summaries |
| `myrlet [--json] tasks show <id>` | Admin or Worker | One authoritative Task record, `blockedBy`, latest Progress, and latest Result |
| `myrlet [--json] tasks tree <id> [--direction <d>] [--max-depth <n>] [--limit <n>]` | Admin or Worker | Bounded ancestors or descendants |
| `myrlet [--json] workers list` | Admin or Worker | Enrolled Workers and connectivity |
| `myrlet [--json] workers show <id>` | Admin or Worker | One Worker, inventory, and claimable Task IDs |
| `myrlet [--json] events watch [--cursor <seq>] [--limit <n>]` | Admin or Worker | NDJSON snapshots and SSE invalidation hints |
| `myrlet [--json] tasks submit --idempotency-key <key> --title <t> --prompt <p>` | Admin | Submit inline; add `--parent-task-id`, `--depends-on`, `--capability`, `--preferred-executor-id` |
| `myrlet [--json] tasks submit --file <def.json> --idempotency-key <key>` | Admin | Submit from a full `TaskDefinition` |
| `myrlet [--json] tasks import --file <bundle> --idempotency-key <key>` | Admin | Atomically submit up to 100 Tasks from a Bundle |
| `myrlet [--json] tasks update <id> --expected-version <n> [--depends-on <id>…]` | Admin | Version-fenced dependency edit of an unclaimed Task |
| `myrlet [--json] tasks cancel <id> [--expected-version <n>] [--reason <t>]` | Admin | Cancel a non-terminal Task |
| `myrlet [--json] tasks finish <id> [--expected-version <n>]` | Admin | Finish a coordination-only Task with no Delivery |
| `myrlet done <delivery-id>` | local socket | Harness finished its work; requests Result sync and Finish |
| `myrlet result check <delivery-id>` | local socket | Advisory Markdown feedback; never blocks |
| `myrlet result sync <delivery-id>` | local socket | Reconcile the Hub Result copy now |
| `myrlet result status <delivery-id>` | local socket | Compare local and Hub copies without writing |
| `myrlet [--json] workers enroll [--ttl-seconds <n>]` | Admin | Operator bootstrap: issue a one-time pairing code |
| `myrlet [--json] workers rotate <id>` / `workers revoke <id>` | Admin | Operator credential administration |

`--json` may appear before or after the subcommand. Diagnostics write to `stderr`; structured output writes to `stdout`.

## Execution-local Commands

`done`, `result check`, `result sync`, and `result status` take **no Hub credential**. They write one JSON request to `$MYRLET_HOME/worker/control.sock` and print a `LocalReply` that separates `localReceipt` from `hubConfirmed`. A Delivery ID this Worker never registered is rejected locally.

## Environment & Exit Codes

- `MYRLET_HUB_URL`: Hub base URL (default `http://127.0.0.1:4140`).
- `MYRLET_ADMIN_TOKEN`: Hub Admin bearer token, required for Owner writes and management (never printed).
- `MYRLET_HOME`: data root; required for execution-local commands (the control socket lives under `worker/`).
- Exit codes: `0` success, `1` Hub/network/decode/IO error, `2` CLI usage or missing env.
