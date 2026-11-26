# syntax=docker/dockerfile:1
#
# Myrlet V1 (0.1.0) runtime images: Hub, Worker, and CLI. One target per
# published image, so `--target` matches `ghcr.io/myrlet/{hub,worker,myrlet}`:
#
#   docker build --target hub    -t myrlet/hub:0.1.0 .
#   docker build --target worker -t myrlet/worker:0.1.0 .
#   docker build --target myrlet -t myrlet/myrlet:0.1.0 .
#
# `hub` is the last stage, so a plain `docker build .` produces the Hub image.
# Runtime images hold only the release binaries, `ca-certificates`, and a private
# `MYRLET_HOME`: no Python, no toolchain, no dev tools, and no demo fixtures.

# --- Stage 1: builder -------------------------------------------------------
# `rust:1-bookworm` tracks stable Rust (modern workspace crates require it) and
# matches the `bookworm` glibc ABI of the slim runtime base.
FROM rust:1-bookworm AS builder

WORKDIR /src

# The context is trimmed by `.dockerignore` to manifests plus `crates/` and `apps/`.
COPY . .

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

# --- Target: Worker (outbound WSS daemon) -----------------------------------
FROM runtime-base AS worker

COPY --from=builder /usr/local/bin/worker /usr/local/bin/worker
# Harnesses launched inside this container act on Myrlet Tasks through this CLI.
COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet

ENTRYPOINT ["/usr/local/bin/worker"]

# --- Target: CLI (human and Owner Agent client) -----------------------------
# Target name matches the published image `ghcr.io/myrlet/myrlet`.
FROM runtime-base AS myrlet

COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet

ENTRYPOINT ["/usr/local/bin/myrlet"]

# --- Target: Hub (HTTP/SSE control plane and Worker WSS server) -------------
# Last stage on purpose: a plain `docker build .` yields the Hub image.
# `MYRLET_ADMIN_TOKEN` must be supplied at run time; the Hub fails closed without it.
FROM runtime-base AS hub

COPY --from=builder /usr/local/bin/hub /usr/local/bin/hub
COPY --from=builder /usr/local/bin/myrlet /usr/local/bin/myrlet

ENV MYRLET_HUB_BIND=0.0.0.0:4140
EXPOSE 4140
ENTRYPOINT ["/usr/local/bin/hub"]
