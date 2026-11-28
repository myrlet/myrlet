# Deployment

Covers native binaries, containers, TLS, backup/restore, and rollback for Myrlet V1. For local source development, see [Developer quickstart](../development/getting-started.md).

V1 ships three binaries — `hub`, `worker`, and `myrlet` — or the equivalent pre-built container images (`ghcr.io/myrlet/{hub,worker,myrlet}`). The Hub (and the all-in-one image) also serves the built-in Web UI from the same origin.

## Canonical Local State (`MYRLET_HOME`)

Set an explicit absolute `MYRLET_HOME` outside any Git repository (e.g. `/var/lib/myrlet`). Hub and Worker fail closed on symlinked paths, `..` traversal, Git checkout roots, or group/world-accessible directories (`>0700`) and credential/database files (`>0600`).

```text
MYRLET_HOME/
├── hub/
│   ├── config.toml
│   ├── credentials.json      # Hub installation ID (0600)
│   └── hub.sqlite            # Hub database (0600)
└── worker/
    ├── worker.yaml           # Worker Slots, capabilities, Executors
    ├── credentials.json      # Worker machine credential (0600)
    ├── state/                # Local Delivery registry & inventory state
    ├── progress/             # Harness Progress JSON files
    └── results/              # Harness Markdown result files (<delivery-id>.md)
```

Pre-release development state has no in-place migration contract: back up and start from a fresh `MYRLET_HOME` across breaking schema changes.

## Build from Source

```bash
# Produces binaries `hub`, `worker`, and `myrlet` under target/release/
cargo build --release -p myrlet-hub -p myrlet-worker-daemon -p myrlet-cli
```

## Native Hub and Worker

### 1. Start Hub

```bash
export MYRLET_HOME=/var/lib/myrlet
export MYRLET_HUB_BIND=127.0.0.1:4140
export MYRLET_ADMIN_TOKEN='<long-random-secret>'
# export MYRLET_HUB_UI=false   # optional: disable the built-in Web UI (API only)
hub
```

The Hub embeds and serves the Web UI same-origin on `http://127.0.0.1:4140/` by default. To run API-only, set `MYRLET_HUB_UI=false` (or `uiEnabled = false` in `$MYRLET_HOME/hub/config.toml`); UI paths then return `404 NotFound`.

Optional overrides live in `$MYRLET_HOME/hub/config.toml` (defaults: 2 MiB max body, 1 MiB bundle import, 30s request timeout, 128 concurrent requests, `MYRLET_HUB_SHUTDOWN_TIMEOUT_MS=30000`, `MYRLET_HUB_UI=true`). Put a TLS reverse proxy in front of Hub for any non-loopback network.

### 2. Enroll and Start Worker (Single-Step Pairing)

```bash
export MYRLET_HUB_URL='https://hub.example.invalid'
code="$(myrlet --json workers enroll | jq -er '.enrollmentCode')"
worker pair --hub-url "$MYRLET_HUB_URL" --code "$code"
```

Configure `/var/lib/myrlet/worker/worker.yaml` (YAML only), validate with `check-config`, and run:

```bash
export MYRLET_HOME=/var/lib/myrlet
worker check-config
worker
```

### 3. Approve the Worker and Apply its Inventory

The Hub starts with both **Security & Trust** policies on. A newly paired Worker is
`pending`: it connects, reports inventory, and can read, but it **claims nothing** until an
Admin approves it, and its first scheduling report takes effect only after an explicit
Apply.

```bash
myrlet --json workers list                        # find approval + reportedRevision
myrlet --json workers config show '<worker-id>'   # inspect both snapshots
myrlet --json workers approve '<worker-id>'
myrlet --json workers config apply '<worker-id>' --revision '<reportedRevision>'
```

Approval and Apply may happen in either order. Rotation keeps both decisions; revoking a
Worker and re-pairing the same ID starts a fresh approval and clears both snapshots. Apply
answers `409 NoPendingConfig` when nothing awaits review and `409 ConfigRevisionMismatch`
when a newer report arrived — re-read the Worker before retrying. Neither change can
retroactively alter a Worker that is already approved or a committed Delivery.

The same operations live in the Web UI: the Workers page filters pending Workers, shows
both snapshots field by field, and submits the revision the Admin actually viewed. Web
**Settings → Security & Trust** edits the two Hub-wide policies independently. A settings
write applies to the next Pair or inventory report; it never changes an existing decision
or a pending report. The same policies are editable without the Web UI through the Admin
CLI (`myrlet policy show`, `myrlet policy set --auto-approve/--auto-apply`) or
`GET`/`PATCH /v1/settings/security-trust`.

### Worker Configuration Reload

The Worker watches `worker.yaml`. An invalid edit is rejected locally and the last valid
runtime configuration stays active. A valid scheduling change pauses new claims, swaps the
adapters, reports the new inventory, and resumes once the Hub acknowledges it. Existing
Deliveries keep the adapter and reporting path that admitted them, so a reload never
strands a running execution. Apply never writes `worker.yaml`, installs a Harness, or
restarts one.

### Worker TLS (`wss://`)

`wss://` always verifies the Hub TLS certificate chain and hostname (never disabled). Mozilla public CA roots work out of the box; for a private CA, set `MYRLET_HUB_CA_FILE=/etc/myrlet/hub-ca.pem` to trust your private CA bundle. Plaintext `ws://` is for loopback or private container networks only.

### Harness Environment Boundary

Myrlet does not bundle Harnesses, manage Provider Credentials, or sandbox execution. Install Harnesses and configure OS users, container mounts, and network controls externally.

## Containers

The repo-root [`Dockerfile`](../../Dockerfile) is a multi-stage build matching the three published GHCR images 1:1:

| Target | Image | Entrypoint |
| --- | --- | --- |
| `hub` | `ghcr.io/myrlet/hub` | `hub` (Hub only, with the built-in Web UI embedded; toggle it with `MYRLET_HUB_UI=false`; bundles `myrlet` for in-container administration) |
| `worker` | `ghcr.io/myrlet/worker` | `worker` (Worker only; bundles `myrlet` for Harnesses inside the container) |
| `myrlet` | `ghcr.io/myrlet/myrlet` | all-in-one local/homelab image: Hub + built-in Web UI + auto-paired local Worker |

```bash
docker build --target hub    -t myrlet/hub:0.1.0 .
docker build --target worker -t myrlet/worker:0.1.0 .
docker build --target myrlet -t myrlet/myrlet:0.1.0 .

# Or pull the published images
docker pull ghcr.io/myrlet/hub:latest
docker pull ghcr.io/myrlet/worker:latest
docker pull ghcr.io/myrlet/myrlet:latest
```

Images are lean and headless (no GUI dependencies): release binaries plus `ca-certificates` on `debian:bookworm-slim`, with no Python, toolchain, dev tools, or demo fixtures. Each runs as unprivileged UID/GID `10001` with a private `0700` data root at `MYRLET_HOME=/var/lib/myrlet`. `--target hub` is the last stage, so a plain `docker build .` yields the Hub image.

```bash
# Hub only: serves the API and (by default) the built-in Web UI on the same origin
docker run --rm \
  -e MYRLET_ADMIN_TOKEN="$MYRLET_ADMIN_TOKEN" \
  -e MYRLET_HUB_BIND=0.0.0.0:4140 \
  -v /var/lib/myrlet:/var/lib/myrlet \
  -p 4140:4140 \
  ghcr.io/myrlet/hub:latest

# All-in-one local/homelab: Hub + built-in Web UI + auto-paired local Worker
docker run --rm \
  -e MYRLET_ADMIN_TOKEN="$MYRLET_ADMIN_TOKEN" \
  -e MYRLET_HUB_BIND=0.0.0.0:4140 \
  -v /var/lib/myrlet:/var/lib/myrlet \
  -p 4140:4140 \
  ghcr.io/myrlet/myrlet:latest
```

Set `MYRLET_HUB_UI=false` on the `hub` or `myrlet` image to serve the API only.

The all-in-one image starts the Hub with both Security & Trust policies on, exactly like
the other images. On first boot only, the container entrypoint enables the Hub's own
automatic policies (`myrlet policy set --auto-approve true --auto-apply true`) and then
pairs its local Worker, so that Worker is approved and its first report applied through the
ordinary Hub rules — no per-Worker discovery or manual CLI approval step. Data directories
that already hold credentials are left untouched, so an operator who deliberately turned a
policy off keeps it. The default local `slotId` is `local`, and its single `process`
Executor writes one Progress line so a first Task visibly completes.

### Readiness and Health

`GET /health` (also `/v1/health`) is the unauthenticated liveness probe used by the
all-in-one entrypoint and container health checks; it answers once the Hub is serving.
Readiness for scheduling is a separate, explicit fact: a Worker appears in
`GET /v1/workers` with `online: true`, `approval: "approved"`, and matching
`reportedRevision` / `appliedRevision`. A Worker whose revisions differ is healthy but not
yet scheduling; it will claim nothing until an Admin applies the reviewed revision.

Published by [`.github/workflows/docker.yml`](../../.github/workflows/docker.yml) on `main` (tag `edge` + `sha-*`), on `v*` tags (`latest` + semver), and on `workflow_dispatch`. Pull requests build with `push: false` to verify the Dockerfile. Bind-mount data dirs with the `10001` owner so state stays readable/writable, and keep Hub/Workers on the same release per the rollback rules below.

## Backup and Rollback

Restore the Worker’s Delivery registry and its local Progress/Result content before reconciliation. A missing volume, failed read, or incomplete restore cannot establish that an operator cleared a Result, so it preserves the Hub copy; only confirmed absence or emptiness after recovery clears it. See [reporting semantics](../architecture/hub-api.md#result-and-progress-convergence).

Use SQLite's online backup (`sqlite3 /var/lib/myrlet/hub/hub.sqlite ".backup '/path/to/backup.sqlite'"`) and back up `hub/credentials.json` and each Worker's `worker/credentials.json` separately with mode `0600`. Treat database backups as sensitive (they contain token verifiers).

Upgrade Hub and Workers together on wire changes (no mixed-version compatibility in pre-release): stop Workers, stop Hub, back up `hub.sqlite` + credentials, deploy new binaries, start Hub then Workers, and verify handshake + one submitted Task being claimed to `finished`.
