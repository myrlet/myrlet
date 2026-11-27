# Myrlet CLI Reference

Executable: `myrlet`.

## Commands

| Command | Purpose |
| --- | --- |
| `myrlet [--json] tasks list` | List Tasks from the Hub Snapshot |
| `myrlet [--json] tasks show <id>` | Read one authoritative Task record, including the immutable result Markdown |
| `myrlet [--json] tasks create --idempotency-key <key> --title <t> --prompt <p>` | Create a `held` Task |
| `myrlet [--json] tasks dispatch <id> --expected-version <n> --idempotency-key <key> --match` | Dispatch via Simple Match |
| `myrlet [--json] tasks dispatch <id> --expected-version <n> --idempotency-key <key> --worker-id <id> --slot-id <id> --connection-epoch <n> [--executor-id <id>]` | Targeted Dispatch |
| `myrlet [--json] tasks submit --file <file> --idempotency-key <key> [--dispatch-idempotency-key <key>]` | Create from JSON file + explicit Simple Match dispatch |
| `myrlet [--json] tasks bundle-import --file <bundle> --idempotency-key <key>` | Atomically import `held` Tasks from a Bundle |
| `myrlet [--json] tasks cancel <id>` | Cancel a non-terminal Task |
| `myrlet [--json] tasks finish <id>` | Explicitly finish a coordination-only `held` Parent with no Delivery |
| `myrlet [--json] workers list` | List enrolled Workers and connectivity |
| `myrlet [--json] events watch` | Stream NDJSON snapshots and SSE events |

`--json` may appear before or after the subcommand. Diagnostics write to `stderr`; structured output writes to `stdout`.

## Environment & Exit Codes

- `MYRLET_HUB_URL`: Hub base URL (default `http://127.0.0.1:4140`).
- `MYRLET_ADMIN_TOKEN`: Hub bearer token (required; never printed).
- Exit codes: `0` success, `1` Hub/network/decode/IO error, `2` CLI usage or missing env.
