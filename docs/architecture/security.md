# Security and Trust Boundaries

Myrlet secures its own control plane (HTTP, SSE, WSS, and Myrlet-owned files). It does **not** sandbox Harnesses or manage Harness Provider Credentials.

## Proposed agent access

Authorized Owner Agents and Harnesses can query all Tasks, dependency trees, Workers, and events. Read access alone cannot submit, cancel, rotate, or revoke. A Harness needs no Admin token for these proposed reads. The read credential or authenticated proxy and operator bootstrap require review before implementation. Until then, use the current credential matrix below.

A Delivery ID identifies an execution and confers no authority. For local reporting the Worker resolves the recorded Delivery, verifies safe paths, and uses its own credential. The Hub checks Worker binding and current connection epoch at the write boundary. An unreadable or unsafe Result file provides no authority to clear the Hub copy. See [reporting semantics](hub-api.md#proposed-reporting-semantics).

## Control-Plane Authentication

The current binary has two authenticated Principal kinds:

| Principal | Credential | Authorized Surface |
| --- | --- | --- |
| **Admin** | `MYRLET_ADMIN_TOKEN` (operator-provisioned secret) | All Client/Admin HTTP endpoints, SSE `/v1/events`, Worker enrollment code issuance, Worker rotation/revocation. |
| **Worker** | Hub-issued CSPRNG machine bearer token | `GET /v1/workers/connect` (WSS) and `POST /v1/worker/deliveries/{id}/{progress,result,finish}` for Deliveries bound to that `workerId` only. |

Rules:
- Hub stores only **SHA-256 verifiers** (never plaintext tokens or pairing codes) and compares verifiers in constant time.
- Authentication runs **before** request bodies are buffered or parsed (`401` on missing/invalid/revoked tokens).
- Credentials in URL query parameters are forbidden.
- Worker credentials cannot call Admin routes; Admin credentials cannot spoof Worker Progress/Result/Finish or WSS connections.
- Browser Clients keep tokens in memory only (never in `localStorage`, URLs, or packaged Web assets) and default to same-origin Hub hosting with a restrictive CSP.

## Pairing and Worker Credentials

- Pairing codes are one-time, short-lived, and issued only by an authenticated Admin.
- Redeeming a valid code via `POST /v1/pair` immediately issues a Worker bearer credential bound to `(hubId, workerId)` and consumes the code.
- Worker stores its binding atomically in `worker/credentials.json` with mode `0600` (POSIX) or equivalent owner-only ACL.
- Rotation invalidates the previous credential immediately and fences active WSS sessions. Revocation removes trust and disconnects the Worker. Missing or corrupt credential files fail closed rather than silently generating a new identity.

## Transport and Filesystem Safety

- **Mandatory TLS verification**: `wss://` connections always verify the Hub TLS certificate chain and hostname against Mozilla roots or an operator-supplied `MYRLET_HUB_CA_FILE` bundle. No option disables verification. Plaintext `ws://` / `http://` is for loopback or private container networks only.
- **Epoch and version fencing**: Monotonic `connectionEpoch` fences stale Worker sockets; `expectedTaskVersion` fences stale Task mutations.
- **Path safety**: Hub and Worker reject `MYRLET_HOME` inside a Git repository, `..` traversal, symlinked databases/credentials/state directories, and group/world-accessible (`>0700` dir / `>0600` file) managed paths. Myrlet fails closed instead of silently `chmod`-ing operator files.

## Non-Invasive Harness Boundary (Explicitly No Sandbox)

- Myrlet provides **no** filesystem, network, process, device, resource, Session, or Workspace isolation for Harnesses. A Harness runs with the OS permissions of the Worker process/container chosen by the operator.
- Harness Provider Credentials (LLM API keys, CLI logins) are 100% Harness/operator-owned. Myrlet never stores, injects, rotates, or inspects them.
- **Accepted risk**: When a Harness invokes the `myrlet` CLI on the same OS account, it operates under whatever credentials are exposed to that account. Operators who require isolation must apply OS users, containers, VMs, or network policies externally.
- **Progress is untrusted text**: Myrlet bounds payload sizes and validates JSON shape, but does not guarantee secret redaction inside Harness-authored Progress text.
- **Result is untrusted Markdown**: Hub stores and bounds it as opaque text. The Web UI renders it with raw HTML disabled and no remote image loading, and only `http(s)`/`mailto` links are clickable. The Worker reads `<delivery_id>.md` through a no-symlink, owner-only (`0600` file, `0700` directory) open and never modifies it.
