# Deployment

Covers native binaries, TLS, containers, backup/restore, and rollback for Myrlet V1. For local source development, see [Developer quickstart](../development/getting-started.md).

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
    └── progress/             # Harness Progress JSON files
```

Pre-release development state has no in-place migration contract: back up and start from a fresh `MYRLET_HOME` across breaking schema changes.

## Build from Source

```bash
cargo build --release -p myrlet-hub -p myrlet-worker-daemon -p myrlet-cli
```

## Native Hub and Worker

### 1. Start Hub

```bash
export MYRLET_HOME=/var/lib/myrlet
export MYRLET_HUB_BIND=127.0.0.1:4140
export MYRLET_ADMIN_TOKEN='<long-random-secret>'
myrlet-hub
```

Optional overrides live in `$MYRLET_HOME/hub/config.toml` (defaults: 2 MiB max body, 1 MiB bundle import, 30s request timeout, 128 concurrent requests, `MYRLET_HUB_SHUTDOWN_TIMEOUT_MS=30000`). Put a TLS reverse proxy in front of Hub for any non-loopback network.

### 2. Enroll and Start Worker (Single-Step Pairing)

```bash
export MYRLET_HUB_URL='https://hub.example.invalid'
code="$(myrlet --json workers enroll | jq -er '.enrollmentCode')"
myrlet-worker pair --hub-url "$MYRLET_HUB_URL" --code "$code"
```

Configure `/var/lib/myrlet/worker/worker.yaml` (YAML only), validate with `check-config`, and run:

```bash
export MYRLET_HOME=/var/lib/myrlet
myrlet-worker check-config
myrlet-worker
```

### Worker TLS (`wss://`)

`wss://` always verifies the Hub TLS certificate chain and hostname (never disabled). Mozilla public CA roots work out of the box; for a private CA, set `MYRLET_HUB_CA_FILE=/etc/myrlet/hub-ca.pem` to trust your private CA bundle. Plaintext `ws://` is for loopback or private container networks only.

### Harness Environment Boundary

Myrlet does not bundle Harnesses, manage Provider Credentials, or sandbox execution. Install Harnesses and configure OS users, container mounts, and network controls externally.

## Backup and Rollback

Use SQLite's online backup (`sqlite3 /var/lib/myrlet/hub/hub.sqlite ".backup '/path/to/backup.sqlite'"`) and back up `hub/credentials.json` and each Worker's `worker/credentials.json` separately with mode `0600`. Treat database backups as sensitive (they contain token verifiers).

Upgrade Hub and Workers together on wire changes (no mixed-version compatibility in pre-release): stop Workers, stop Hub, back up `hub.sqlite` + credentials, deploy new binaries, start Hub then Workers, and verify handshake + one Task dispatch.
