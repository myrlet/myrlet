# Security and Trust Boundaries

Myrlet secures its own control plane (HTTP, SSE, WSS, and Myrlet-owned files). It does **not** sandbox Harnesses or manage Harness Provider Credentials.

## Control-Plane Principals and Permission Tiers

V1 has two principal kinds and three permission tiers. The `worker` and `owner` tiers use the **same Worker principal and credential**; `admin` is a separate system principal.

| Permission tier | Principal / credential | Authority |
| --- | --- | --- |
| `worker` | Worker principal authenticated by its Hub-issued bearer credential | Execute assigned Deliveries, read its assigned Task and direct related-Task summaries, and report its own execution state. |
| `owner` | The same Worker principal and credential, with an Admin-assigned tier | All Worker permissions, global Task coordination, and read-only observation of Worker and Worker-reported Agent runtime state. |
| `admin` | Admin principal authenticated by `MYRLET_ADMIN_TOKEN` | Global Task coordination plus Worker, credential, permission-tier, Hub, and system administration. |

The `owner` tier is an authorization level on a Worker identity, **not a separate Agent identity** and not the same thing as a Task Owner. A Worker credential authenticates `workerId`; the Hub loads the Worker's permission tier from server-controlled state. The client cannot select or elevate its tier. A newly paired Worker defaults to `worker`; only Admin may promote or demote it to `owner`. The `admin` tier is never assigned to a Worker.

All Agents, Harnesses, and execution processes running within one Worker share that Worker's complete Myrlet authority. Agent IDs and Harness/session identifiers may be used for local correlation or reported observability, but they are not authorization principals. Myrlet does not claim to isolate Agents that share one Worker runtime and credential.

Rules:
- Hub stores only **SHA-256 verifiers** (never plaintext Worker tokens or pairing codes) and compares verifiers in constant time.
- Authentication runs **before** request bodies are buffered or parsed (`401` on missing, invalid, or revoked credentials).
- Credentials in URL query parameters are forbidden.
- Worker credentials with either `worker` or `owner` tier may use Worker-bound WSS, claim, Progress, Result, and Finish operations for their own `workerId`.
- Owner-tier credentials additionally authorize global Task coordination and Worker/Agent observability, but never Worker administration.
- Admin credentials authorize global reads, Task coordination, and system administration. They **cannot spoof** Worker claims, Progress, Result, Finish, cancellation acknowledgements, or WSS connections; those operations must originate from the assigned Worker credential.
- Browser Clients keep the Admin token in memory only (never in `localStorage`, URLs, or packaged Web assets) and default to same-origin Hub hosting with a restrictive CSP.

## Permission Tiers

### Worker — execution

A `worker` principal may:
- Read a Task whose current Delivery is bound to its authenticated `workerId`, including that Task's execution context and latest Result/Progress.
- Read the direct related-Task summaries defined below.
- Claim eligible Tasks and report Progress, Result, and Finish for Deliveries bound to its own `workerId`.
- Respond to cancellation for its own Deliveries through the Worker protocol.

It may not list or inspect unrelated Tasks, read global events or other Workers, create or coordinate Tasks, alter Task definitions or relationships, or administer Workers and Hub configuration.

### Owner — global task coordination

An `owner` principal includes all Worker permissions and may:
- Read all Tasks, Task relationships, blockers, Deliveries, Executions, Progress, and Results.
- Use all Task coordination operations currently defined by the V1 Task Model and Hub API, including submission/import, supported pending edits, cancellation, and coordination-only finish.
- Read all Worker operational status and scheduling-relevant inventory, including connectivity, health/availability, Slots, capabilities, Executors, and advisory capacity.
- Read internal Agent runtime state when it is reported by the Worker.

Owner is observational and coordinative with respect to Workers. It cannot enroll, approve, enable, disable, remove, configure, or otherwise administer a Worker; change Worker capabilities/labels/inventory; issue, rotate, or revoke credentials; change permission tiers; or modify Hub/system/security configuration.

This tier does not add a separate dispatch or Task Assignment API. Workers still claim queued Tasks under the existing Task Model; `preferredWorkerId` remains a soft preference, not an assignment or read grant.

### Admin — system administration

The Admin principal can perform all global Task reads and supported Task coordination operations, and additionally:
- Enroll, approve, enable/disable, revoke, and administer Workers.
- Issue, rotate, and revoke Worker credentials.
- Set a Worker's permission tier to `worker` or `owner`.
- Modify Worker configuration and administrative metadata, including capabilities/labels where supported.
- Modify Hub and system configuration, including security-related settings.

Admin remains a distinct principal and cannot impersonate a Worker for Worker-originated execution operations.

## Task Read Visibility

For authorization, a Task is assigned to a Worker only after Hub commits a Delivery whose `workerId` matches that Worker. A Task's `preferredWorkerId` is a routing preference and grants no read access.

A `worker` principal may read:
- Its assigned Task in full.
- One-hop related Tasks: the direct parent, direct children, direct declared dependencies, and direct dependents (Tasks that declare the assigned Task as a dependency). Dependencies remain subject to the sibling-only rule in the Task Model.

Related Tasks are returned as a limited coordination summary: identifiers, title, status, hierarchy/dependency relationships, and blocker information. Related-task access does not expose the related Task's prompt, input, expected output, external references, arbitrary metadata, Delivery details, Progress, or Result. Visibility does not recursively expand from a related Task to its other relatives or subtree.

Owner and Admin may read all Task records and their full supported coordination/execution data. Owner and Admin global reads do not imply access to Worker-local files, environment variables, Harness Provider Credentials, or other local secrets.

## Worker and Agent Observability

Owner and Admin may read Worker status and scheduling-relevant inventory. Owner may also read internal Agent runtime state that the Worker reports for coordination. Such state is telemetry owned by the Worker, not a separate Agent identity, and grants no control over the Agent process.

Myrlet can expose only runtime information actually reported by the Worker. The current V1 Worker protocol does not define a separate Agent-state frame or introspection of Harness-private subagents/Sessions; richer Agent telemetry requires its own approved protocol contract. Worker-tier principals do not receive global Worker or Agent observability.

## Pairing and Worker Credentials

- Pairing codes are one-time, short-lived, and issued only by an authenticated Admin.
- Redeeming a valid code via `POST /v1/pair` immediately issues a Worker bearer credential bound to `(hubId, workerId)` and consumes the code. A newly paired Worker starts at `permissionTier: worker`.
- Only Admin may change the server-stored Worker permission tier to `worker` or `owner`; a Worker cannot elevate itself through pairing, inventory, or API payloads.
- Credential rotation invalidates the previous credential immediately and fences active WSS sessions while preserving the Worker's permission tier.
- Re-pairing a revoked Worker starts a fresh trust decision and resets its permission tier to `worker`; an Admin must explicitly promote it again if needed.
- Worker stores its binding atomically in `worker/credentials.json` with mode `0600` (POSIX) or equivalent owner-only ACL. Missing or corrupt credential files fail closed rather than silently generating a new identity.
- Revocation removes trust and disconnects the Worker. Token and pairing-code plaintext must never appear in Task data, logs, code, or release artifacts.

## Transport and Filesystem Safety

- **Mandatory TLS verification**: `wss://` connections always verify the Hub TLS certificate chain and hostname against Mozilla roots or an operator-supplied `MYRLET_HUB_CA_FILE` bundle. No option disables verification. Plaintext `ws://` / `http://` is for loopback or private container networks only.
- **Epoch and version fencing**: Monotonic `connectionEpoch` fences stale Worker sockets and every Worker write; `expectedTaskVersion` fences stale Task mutations. A delayed write from an old epoch, including a Result or Progress report, is refused at the Hub boundary and can never undo a newer committed value.
- **Path safety**: Hub and Worker reject `MYRLET_HOME` inside a Git repository, `..` traversal, symlinked databases/credentials/state directories, and group/world-accessible (`>0700` dir / `>0600` file) managed paths. Myrlet fails closed instead of silently `chmod`-ing operator files.

## Non-Invasive Harness Boundary (Explicitly No Sandbox)

- Myrlet provides **no** filesystem, network, process, device, resource, Session, or Workspace isolation for Harnesses. A Harness runs with the OS permissions of the Worker process/container chosen by the operator.
- Harness Provider Credentials (LLM API keys, CLI logins) are 100% Harness/operator-owned. Myrlet never stores, injects, rotates, or inspects them.
- **Accepted risk**: When a Harness invokes the `myrlet` CLI on the same OS account, it operates under whatever credentials are exposed to that account. Operators who require isolation must apply OS users, containers, VMs, or network policies externally.
- **Progress is untrusted text**: Myrlet bounds payload sizes and validates JSON shape, but does not guarantee secret redaction inside Harness-authored Progress text.
- **Result is untrusted Markdown**: Hub stores and bounds it as opaque text. The Web UI renders it with raw HTML disabled and no remote image loading, and only `http(s)`/`mailto` links are clickable. The Worker reads `<delivery_id>.md` through a no-symlink, owner-only (`0600` file, `0700` directory) open and never modifies it.
