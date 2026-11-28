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
# The local Worker is identified by the `workerId` its pairing reported. This
# entrypoint approves and applies configuration for that one Worker only, while
# leaving the Hub's Security & Trust policies untouched, so any other Worker
# enrolled with the same Hub stays subject to the configured rules.
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
#   MYRLET_HUB_BIN, MYRLET_WORKER_BIN, MYRLET_CLI_BIN
#                        optional binary paths; default to the image layout, and let an
#                        integration test run this exact script against freshly built
#                        binaries instead of the baked-in ones.

set -euo pipefail

readonly HUB_BIN="${MYRLET_HUB_BIN:-/usr/local/bin/hub}"
readonly WORKER_BIN="${MYRLET_WORKER_BIN:-/usr/local/bin/worker}"
readonly CLI_BIN="${MYRLET_CLI_BIN:-/usr/local/bin/myrlet}"
readonly READY_TIMEOUT_SECONDS=30

log() { printf 'myrlet-start: %s\n' "$*" >&2; }
die() {
  printf 'myrlet-start: error: %s\n' "$*" >&2
  exit 1
}

hub_pid=""
worker_pid=""
shutting_down=0
# Identity of the Worker this entrypoint enrolled. Empty until pairing establishes it.
local_worker_id=""

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
# Trust stays exactly as the Hub is configured: this entrypoint never edits the
# Security & Trust policies, so any other Worker enrolled with the same Hub keeps
# whatever approval and review rules the operator set. Instead it approves and
# applies configuration for this one local Worker, identified by the `workerId`
# bound in this image's credential file — never by position in `workers list`.
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
  # `worker pair` writes the Hub-assigned identity to credentials.json.
  if ! pair_out="$("$WORKER_BIN" pair --hub-url "$local_hub_url" --code "$code" 2>&1)"; then
    die "pairing the local worker failed: ${pair_out}"
  fi
  log "$pair_out"
fi

# --- Resolve the local Worker identity (both fresh install and restart) -----
# The credential binding this image wrote is the authoritative, restart-stable local
# identity. Resolving it here — not only inside the first-boot branch — is what lets a
# restarted container converge its own Worker. It is read from the local file, never
# inferred from `workers list` order.
if [ ! -f "$worker_credentials" ]; then
  die "no worker credentials at ${worker_credentials}; the local worker was never paired"
fi
local_worker_id="$(sed -n 's/.*"workerId"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$worker_credentials" | head -n 1)"
case "$local_worker_id" in
  wkr_*) ;;
  *)
    die "could not resolve the local worker identity from ${worker_credentials} (got '${local_worker_id}')"
    ;;
esac
log "local worker identity ${local_worker_id}"

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

# --- Approve and apply for this Worker only ---------------------------------
# Resolve the pair from one `workers show` read so the approval and the applied
# revision describe the same observation. Every step is idempotent (approval repeats
# harmlessly; Apply only runs while the reported revision is newer), so a container
# restart re-converges without touching the Hub's policies or any other Worker.
local_worker_admin() {
  MYRLET_HUB_URL="$local_hub_url" "$CLI_BIN" --json "$@"
}

converge_local_worker() {
  local show revision applied approval
  if ! show="$(local_worker_admin workers show "$local_worker_id")"; then
    return 1
  fi
  revision="$(printf '%s' "$show" | sed -n 's/.*"reportedRevision"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  applied="$(printf '%s' "$show" | sed -n 's/.*"appliedRevision"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)"
  approval="$(printf '%s' "$show" | sed -n 's/.*"approval"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  # Wait until the Worker has connected and reported at least one inventory.
  if [ -z "$revision" ]; then
    return 1
  fi
  if [ "$approval" != "approved" ]; then
    if ! local_worker_admin workers approve "$local_worker_id" >/dev/null 2>&1; then
      return 1
    fi
    log "approved the local worker (approval is required by this Hub)"
  fi
  if [ "$revision" != "$applied" ]; then
    if ! local_worker_admin workers config apply "$local_worker_id" \
      --revision "$revision" >/dev/null 2>&1; then
      return 1
    fi
    log "applied the local worker inventory at revision ${revision}"
  fi
  return 0
}

waited=0
converged=0
while [ "$waited" -lt "$READY_TIMEOUT_SECONDS" ]; do
  if ! kill -0 "$worker_pid" 2>/dev/null; then
    die "the local worker exited during startup."
  fi
  if converge_local_worker; then
    converged=1
    break
  fi
  sleep 1
  waited=$((waited + 1))
done
if [ "$converged" -eq 1 ]; then
  log "local worker ${local_worker_id} is approved and its inventory is applied"
else
  log "warning: could not approve/apply the local worker ${local_worker_id} within ${READY_TIMEOUT_SECONDS}s; it stays subject to the Hub's policies until the next restart"
fi

# --- Supervise --------------------------------------------------------------
log "myrlet is running (hub pid ${hub_pid}, worker pid ${worker_pid})"

# Join whichever daemon exits first and return its status. `wait -n` does this directly on
# bash 5+ (the Debian image); a polling fallback keeps this script runnable on an older
# host shell (bash 3.2 on macOS) for non-container integration tests.
wait_first_exit() {
  if wait -n 2>/dev/null; then
    return 0
  else
    first_status=$?
  fi
  # `wait -n` is unsupported (it rejects its own usage), so poll the two children instead.
  if [ "$first_status" -eq 2 ]; then
    while kill -0 "$hub_pid" 2>/dev/null && kill -0 "$worker_pid" 2>/dev/null; do
      sleep 1
    done
    set +e
    wait "$hub_pid"
    hub_exit=$?
    wait "$worker_pid"
    worker_exit=$?
    set -e
    # Report the daemon that stopped first; a clean stop is 0.
    if [ "$hub_exit" -eq 0 ] || [ "$worker_exit" -eq 0 ]; then
      return 0
    fi
    return "$hub_exit"
  fi
  return "$first_status"
}
wait_first_exit
status=$?

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
