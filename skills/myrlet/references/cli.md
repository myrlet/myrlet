# Myrlet CLI Reference

Executable: `myrlet`.

## Commands

| Command | Credential | Purpose |
| --- | --- | --- |
| `myrlet [--json] tasks list [--limit <n>]` | Owner or Admin | Bounded newest-first global Task summaries |
| `myrlet [--json] tasks show <id>` | Worker, Owner, or Admin | One Task: full for the Worker's own Delivery-bound Task, Owner/Admin, or a one-hop summary for a directly related Task |
| `myrlet [--json] tasks tree <id>` | Worker, Owner, or Admin | Bounded ancestors/descendants for Owner/Admin; Worker access anchors to its own Task and returns one-hop related summaries |
| `myrlet [--json] workers list` | Owner or Admin | Enrolled Workers, tier, approval, and revisions |
| `myrlet [--json] workers show <id>` | Owner or Admin | One Worker, tier, approval, inventory revision, Apply pending flag, and claimable Task IDs |
| `myrlet [--json] workers config show <id>` | Admin | Both scheduling snapshots (reported and applied) for review before an Apply |
| `myrlet [--json] events watch [--cursor <seq>] [--limit <n>]` | Owner or Admin | NDJSON snapshots and SSE invalidation hints |
| `myrlet [--json] tasks submit --idempotency-key <key> --title <t> --prompt <p>` | Owner or Admin | Submit inline; add `--parent-task-id`, `--depends-on`, `--capability`, `--preferred-executor-id` |
| `myrlet [--json] tasks submit --file <def.json> --idempotency-key <key>` | Owner or Admin | Submit from a full `TaskDefinition` |
| `myrlet [--json] tasks import --file <bundle> --idempotency-key <key>` | Owner or Admin | Atomically submit up to 100 Tasks from a Bundle |
| `myrlet [--json] tasks update <id> --expected-version <n> [--depends-on <id>…]` | Owner or Admin | Version-fenced dependency edit of an unclaimed Task |
| `myrlet [--json] tasks cancel <id> [--expected-version <n>] [--reason <t>]` | Owner or Admin | Cancel a non-terminal Task |
| `myrlet [--json] tasks finish <id> [--expected-version <n>]` | Owner or Admin | Finish a coordination-only Task with no Delivery |
| `myrlet done <delivery-id>` | local socket | Harness finished its work; requests Result sync and Finish |
| `myrlet result check <delivery-id>` | local socket | Advisory Markdown feedback; never blocks |
| `myrlet result sync <delivery-id>` | local socket | Reconcile the Hub Result copy now |
| `myrlet result status <delivery-id>` | local socket | Compare local and Hub copies without writing |
| `myrlet [--json] workers enroll [--ttl-seconds <n>]` | Admin | Operator bootstrap: issue a one-time pairing code |
| `myrlet [--json] workers rotate <id>` / `workers revoke <id>` | Admin | Operator credential administration |
| `myrlet [--json] workers approve <id>` | Admin | Admit a pending Worker to scheduling; repeating it succeeds |
| `myrlet [--json] workers config apply <id> --revision <n>` | Admin | Apply the reported revision previously inspected; prints `appliedRevision` |
| `myrlet [--json] workers permission set <id> --tier worker\|owner` | Admin | Set the server-stored permission tier; never creates a credential or Agent identity |

`--json` may appear before or after the subcommand. Diagnostics write to `stderr`; structured output writes to `stdout`.

## Credentials and Tiers

The CLI presents the credential the command needs and never selects or infers a tier; the Hub decides.

- Admin commands (`workers enroll/rotate/revoke/approve`, `workers config show/apply`, `workers permission set`) require `MYRLET_ADMIN_TOKEN`. They never fall back to a Worker credential.
- Worker and Owner commands use the Hub-issued Worker credential at `$MYRLET_HOME/worker/credentials.json` when no Admin token is set. A `worker`-tier credential is limited to its own Delivery-bound Task and one-hop related summaries; an `owner`-tier credential adds global reads and Task coordination.
- With `MYRLET_ADMIN_TOKEN` set, every command uses the Admin credential. The Admin principal cannot impersonate Worker execution operations.
- A valid credential with an insufficient tier receives `403 Forbidden` naming the required and current tiers; an invalid or missing credential receives `401`. A Task outside a Worker's visibility boundary reads as `404`.

## Worker Trust

The Hub defaults to requiring both Worker approval and configuration Apply. A newly paired Worker is `pending`: it can connect, report inventory, and read, but cannot claim until an Admin runs `workers approve`. Its first scheduling report also stays under review until an Admin runs `workers config apply <id> --revision <n>` with the same revision `workers config show` displayed. Apply answers `409 NoPendingConfig` when nothing is pending and `409 ConfigRevisionMismatch` when a newer report arrived; re-read the Worker before retrying. Approval and Apply may happen in either order, and neither acts retroactively on existing Workers or reports. The two policies are editable only through the Hub API (`GET`/`PATCH /v1/settings/security-trust`) and the Web Settings **Security & Trust** group; the CLI has no settings command.

## Execution-local Commands

`done`, `result check`, `result sync`, and `result status` take **no Hub credential**. They write one JSON request to `$MYRLET_HOME/worker/control.sock` and print a `LocalReply` that separates `localReceipt` from `hubConfirmed`. A Delivery ID this Worker never registered is rejected locally.

## Environment & Exit Codes

- `MYRLET_HUB_URL`: Hub base URL (default `http://127.0.0.1:4140`).
- `MYRLET_ADMIN_TOKEN`: Hub Admin bearer token, required for Admin commands (never printed).
- `MYRLET_HOME`: data root; supplies the Worker credential for Worker/Owner commands and the control socket for execution-local commands.
- Exit codes: `0` success, `1` Hub/network/decode/IO error, `2` CLI usage or missing env.
