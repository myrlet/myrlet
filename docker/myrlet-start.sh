#!/usr/bin/env bash
#
# All-in-one Myrlet entrypoint for `ghcr.io/myrlet/myrlet`.
#
# One container runs the Hub (with the built-in Web UI), auto-pairs a local
# Worker on first boot, and supervises both daemons until one of them exits or
# the container is asked to stop:
#
#   /usr/bin/tini -- /usr/local/bin/myrlet-start
#
# Runs as UID 10001 (`myrlet`) with `MYRLET_HOME` pointing at a private data
# root. `tini` is PID 1, so this script can forward signals and reap both
# children instead of orphaning them.
#
# Environment:
#   MYRLET_ADMIN_TOKEN   required, non-empty (the Hub fails closed without it)
#   MYRLET_HOME          data root (default: /var/lib/myrlet)
#   MYRLET_HUB_BIND      Hub listen address (default: 0.0.0.0:4140); the local
#                        Worker always reaches the Hub over 127.0.0.1:<port>

set -euo pipefail

readonly HUB_BIN="/usr/local/bin/hub"
readonly WORKER_BIN="/usr/local/bin/worker"
readonly CLI_BIN="/usr/local/bin/myrlet"
readonly READY_TIMEOUT_SECONDS=30

log() { printf 'myrlet-start: %s\n' "$*" >&2; }
die() {
  printf 'myrlet-start: error: %s\n' "$*" >&2
  exit 1
}

hub_pid=""
worker_pid=""
shutting_down=0

# --- Shutdown ---------------------------------------------------------------
# Forward SIGTERM to the Worker first (it drains in-flight deliveries), then to
# the Hub (it drains requests and flushes SQLite), wait for both, and exit 0 so
# `docker stop` is reported as a clean shutdown.
shutdown() {
  set +e
  shutting_down=1
  trap '' SIGTERM SIGINT
  log "shutdown signal received; stopping the local worker and hub"
  if [ -n "$worker_pid" ] && kill -0 "$worker_pid" 2>/dev/null; then
    kill -TERM "$worker_pid" 2>/dev/null
  fi
  if [ -n "$hub_pid" ] && kill -0 "$hub_pid" 2>/dev/null; then
    kill -TERM "$hub_pid" 2>/dev/null
  fi
  if [ -n "$worker_pid" ]; then
    wait "$worker_pid" 2>/dev/null
  fi
  if [ -n "$hub_pid" ]; then
    wait "$hub_pid" 2>/dev/null
  fi
  exit 0
}
trap shutdown SIGTERM SIGINT

# --- Configuration ----------------------------------------------------------
if [ -z "${MYRLET_ADMIN_TOKEN:-}" ]; then
  die "MYRLET_ADMIN_TOKEN must be set to a non-empty value (e.g. \`openssl rand -hex 32\`); refusing to start."
fi
export MYRLET_ADMIN_TOKEN

MYRLET_HOME="${MYRLET_HOME:-/var/lib/myrlet}"
MYRLET_HUB_BIND="${MYRLET_HUB_BIND:-0.0.0.0:4140}"
export MYRLET_HOME MYRLET_HUB_BIND

# `MYRLET_HUB_BIND` is `<host>:<port>`; the local Worker and CLI always talk to
# the loopback side of that same port.
port="${MYRLET_HUB_BIND##*:}"
case "$port" in
  '' | *[!0-9]*) die "MYRLET_HUB_BIND must look like <host>:<port> (got '$MYRLET_HUB_BIND')." ;;
esac
local_hub_url="http://127.0.0.1:${port}"

# --- Hub --------------------------------------------------------------------
log "starting hub on ${MYRLET_HUB_BIND} (MYRLET_HOME=${MYRLET_HOME})"
"$HUB_BIN" &
hub_pid=$!

# Wait for the Hub to answer an authenticated CLI call. `kill -0` on every pass
# means an early Hub startup failure (bad token, busy port, unreadable data
# root) aborts immediately instead of burning the whole timeout.
waited=0
ready=0
while [ "$waited" -lt "$READY_TIMEOUT_SECONDS" ]; do
  if ! kill -0 "$hub_pid" 2>/dev/null; then
    if wait "$hub_pid"; then
      hub_status=0
    else
      hub_status=$?
    fi
    die "the hub exited during startup with status ${hub_status}."
  fi
  if MYRLET_HUB_URL="$local_hub_url" "$CLI_BIN" --json workers list >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
  waited=$((waited + 1))
done
if [ "$ready" -ne 1 ]; then
  die "the hub did not answer at ${local_hub_url} within ${READY_TIMEOUT_SECONDS}s."
fi
log "hub is ready at ${local_hub_url}"

worker_dir="${MYRLET_HOME}/worker"
worker_credentials="${worker_dir}/credentials.json"
worker_config="${worker_dir}/worker.yaml"

# --- Auto-pair the local Worker (first boot only) ---------------------------
if [ ! -f "$worker_credentials" ]; then
  log "no worker credentials in ${worker_credentials}; enrolling a local worker"
  if ! enroll_json="$(MYRLET_HUB_URL="$local_hub_url" "$CLI_BIN" --json workers enroll)"; then
    die "workers enroll failed against ${local_hub_url}."
  fi
  # POSIX sed only: the image ships no Python and no jq.
  code="$(printf '%s' "$enroll_json" | sed -n 's/.*"enrollmentCode"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  if [ -z "$code" ]; then
    die "could not extract an enrollmentCode from the Hub response: ${enroll_json}"
  fi
  "$WORKER_BIN" pair --hub-url "$local_hub_url" --code "$code"
  log "local worker paired"
fi

# --- Default Worker configuration (first boot only) -------------------------
# Field names mirror `apps/worker/src/config.rs` (`hubUrl`, `slots`, `slotId`,
# `advisoryCapacity`, `roles`, `capabilities`, `name`, `state`, `executors`,
# `id`, `kind`, `program`, `args`). `worker.yaml` must stay owner-only (`0600`),
# hence the explicit `umask 077` around the write.
if [ ! -f "$worker_config" ]; then
  log "writing default worker configuration to ${worker_config}"
  install -d -m 0700 "$worker_dir"
  (
    umask 077
    {
      printf 'hubUrl: %s\n' "$local_hub_url"
      # Quoted heredoc: the shell snippet below must reach the file verbatim.
      cat <<'WORKER_YAML'
slots:
  - slotId: local
    advisoryCapacity: 2
    roles:
      - general
    capabilities:
      - name: shell
        state: available
    executors:
      - id: exec-shell
        kind: process
        program: /bin/sh
        args:
          - "-c"
          - 'p=$(sed -n "s/.*to \(\/.*\.json\).*/\1/p" | head -n1); if [ -n "$p" ]; then printf "{\"text\":\"Handled by local shell executor\"}" > "$p"; fi'
WORKER_YAML
    } >"$worker_config"
  )
fi

# Fail fast on a user-mounted, invalid `worker.yaml` instead of crash-looping
# the daemon after the Hub is already up.
log "validating worker configuration"
"$WORKER_BIN" check-config

# --- Worker -----------------------------------------------------------------
log "starting worker"
"$WORKER_BIN" &
worker_pid=$!

# --- Supervise --------------------------------------------------------------
log "myrlet is running (hub pid ${hub_pid}, worker pid ${worker_pid})"
if wait -n "$hub_pid" "$worker_pid"; then
  status=0
else
  status=$?
fi

if [ "$shutting_down" -eq 1 ]; then
  exit 0
fi

# One daemon died: stop the other and report the failure so the container
# restarts as a unit rather than running half-alive.
log "a Myrlet daemon exited unexpectedly (status ${status}); stopping the other"
set +e
if kill -0 "$worker_pid" 2>/dev/null; then
  kill -TERM "$worker_pid" 2>/dev/null
fi
if kill -0 "$hub_pid" 2>/dev/null; then
  kill -TERM "$hub_pid" 2>/dev/null
fi
wait "$worker_pid" 2>/dev/null
wait "$hub_pid" 2>/dev/null
if [ "$status" -eq 0 ]; then
  status=1
fi
exit "$status"
