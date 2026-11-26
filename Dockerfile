# syntax=docker/dockerfile:1
#
# Myrlet V1 (0.1.0) runtime images. Four stages produce three published targets
# that map 1:1 to `ghcr.io/myrlet/{hub,worker,myrlet}`:
#
#   docker build --target hub    -t myrlet/hub:0.1.0 .
#   docker build --target worker -t myrlet/worker:0.1.0 .
#   docker build --target myrlet -t myrlet/myrlet:0.1.0 .
#
#   hub    Hub only, plus the built-in Web UI (embedded at compile time) and the
#          `myrlet` CLI for Owner Agents.
#   worker Worker only (`worker.yaml` + paired credentials are mounted in),
#          plus the `myrlet` CLI so in-container harnesses can act on Tasks.
#   myrlet All-in-One image for local machine / homelab use: Hub + Web UI + an
#          auto-paired local Worker, supervised by `docker/myrlet-start.sh`.
#
# `myrlet` is the last stage, so a plain `docker build .` produces the local
# all-in-one image. Runtime images hold only the release binaries,
# `ca-certificates`, and a private `MYRLET_HOME`: no Python, no toolchain, no dev
# tools, and no demo fixtures.

# --- Stage 1: web-builder ---------------------------------------------------
# The Hub embeds `apps/web/dist`, so the SPA is built first with Node and the
# resulting static bundle is then dropped into the Rust build tree.
FROM node:22-bookworm-slim AS web-builder

WORKDIR /src/apps/web

# Copy manifests first so `npm ci` stays cached across source-only changes.
# `.dockerignore` keeps a stale local `node_modules/` and `dist/` out of context.
COPY apps/web/package.json apps/web/package-lock.json* ./
RUN if [ -f package-lock.json ]; then npm ci; else npm install; fi

COPY apps/web/ ./
RUN npm run build

# --- Stage 2: builder -------------------------------------------------------
# `rust:1-bookworm` tracks stable Rust (modern workspace crates require it) and
# matches the `bookworm` glibc ABI of the slim runtime base.
FROM rust:1-bookworm AS builder

WORKDIR /src

# The context is trimmed by `.dockerignore` to manifests plus `crates/`, `apps/`,
# and `docker/`.
COPY . .

# The embedded Web UI bundle, built by `web-builder` from the very same sources.
COPY --from=web-builder /src/apps/web/dist /src/apps/web/dist

# Ship strictly the three headless binaries, one per published image target.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
        -p myrlet-hub \
        -p myrlet-worker-daemon \
        -p myrlet-cli \
 && install -m 0755 \
        target/release/hub \
        target/release/worker \
        target/release/myrlet \
        /usr/local/bin/

# --- Shared runtime base ----------------------------------------------------
FROM debian:bookworm-slim AS runtime-base

# Verified TLS (`wss://`) needs the Mozilla roots and nothing else: `rusqlite`
# vendors SQLite and all TLS is pure Rust (`ring`), so no OpenSSL is linked.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Unprivileged runtime account (fixed IDs, so bind-mounted data dirs stay owned
# by the same user) with a private `0700` data root.
RUN groupadd --gid 10001 myrlet \
 && useradd --uid 10001 --gid myrlet --no-create-home \
        --home-dir /var/lib/myrlet --shell /usr/sbin/nologin myrlet \
 && install -d -m 0700 -o myrlet -g myrlet /var/lib/myrlet

ENV MYRLET_HOME=/var/lib/myrlet
WORKDIR /var/lib/myrlet
USER myrlet

# --- Target: Hub (HTTP/SSE control plane, Worker WSS server, built-in UI) ----
# Published as `ghcr.io/myrlet/hub`. `MYRLET_ADMIN_TOKEN` must be supplied at run
# time; the Hub fails closed without it.
FROM runtime-base AS hub

COPY --from=builder /usr/local/bin/hub /usr/local/bin/hub
# Operator and Owner Agent CLI, pointed at this Hub with `MYRLET_HUB_URL`.
COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet

ENV MYRLET_HUB_BIND=0.0.0.0:4140
EXPOSE 4140
ENTRYPOINT ["/usr/local/bin/hub"]

# --- Target: Worker (outbound WSS daemon) -----------------------------------
# Published as `ghcr.io/myrlet/worker`. `MYRLET_HOME/worker/worker.yaml` and the
# paired `worker/credentials.json` are mounted or imported by the operator.
FROM runtime-base AS worker

COPY --from=builder /usr/local/bin/worker /usr/local/bin/worker
# Harnesses launched inside this container act on Myrlet Tasks through this CLI.
COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet

ENTRYPOINT ["/usr/local/bin/worker"]

# --- Target: All-in-One (Hub + Web UI + auto-paired local Worker) -----------
# Published as `ghcr.io/myrlet/myrlet`; last stage on purpose, so a plain
# `docker build .` builds the local all-in-one image. `myrlet-start` runs the Hub,
# enrolls and pairs a local Worker on first boot, then supervises both under
# `tini` (PID 1) so `docker stop` reaches both daemons.
FROM runtime-base AS myrlet

USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends tini \
 && rm -rf /var/lib/apt/lists/*
USER myrlet

COPY --from=builder /usr/local/bin/hub /usr/local/bin/hub
COPY --from=builder /usr/local/bin/worker /usr/local/bin/worker
COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet
COPY --chmod=0755 docker/myrlet-start.sh /usr/local/bin/myrlet-start

ENV MYRLET_HUB_BIND=0.0.0.0:4140
EXPOSE 4140
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/myrlet-start"]
