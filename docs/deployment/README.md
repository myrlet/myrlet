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

Published by [`.github/workflows/docker.yml`](../../.github/workflows/docker.yml) on `main` (tag `edge` + `sha-*`), on `v*` tags (`latest` + semver), and on `workflow_dispatch`. Pull requests build with `push: false` to verify the Dockerfile. Bind-mount data dirs with the `10001` owner so state stays readable/writable, and keep Hub/Workers on the same release per the rollback rules below.

## Backup and Rollback

Use SQLite's online backup (`sqlite3 /var/lib/myrlet/hub/hub.sqlite ".backup '/path/to/backup.sqlite'"`) and back up `hub/credentials.json` and each Worker's `worker/credentials.json` separately with mode `0600`. Treat database backups as sensitive (they contain token verifiers).

Upgrade Hub and Workers together on wire changes (no mixed-version compatibility in pre-release): stop Workers, stop Hub, back up `hub.sqlite` + credentials, deploy new binaries, start Hub then Workers, and verify handshake + one Task dispatch.
