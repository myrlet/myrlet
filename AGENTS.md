# Repository Agent Instructions

- Read this file, `CONTRIBUTING.md`, and `docs/architecture/README.md` before making changes.
- `docs/architecture/` is the **single normative contract** for Myrlet V1 (`0.1.0`). Do not introduce anything listed under Explicit V1 Non-Goals.
- **Keep it small and strict**: no multi-tenant RBAC, no Task Assignments, no nested Ownership Boundaries, no Scope Category Catalog, no Priority ceremony, no double-approval enrollment, no per-claim Inventory approval, no 3-table polling Outbox, no signed/fingerprint pagination cursors, no Work Atlas spatial graph, and no Harness sandboxing/injection.
- **Pre-release policy**: no backward-compatibility shims, migration scripts, dual-read paths, or `#[allow(dead_code)]` escapes.
- **Core invariants never weaken**: constant-time SHA-256 bearer verification (`Admin` / `Worker`), mandatory WSS TLS verification, POSIX `0600`/`0700` & symlink/traversal checks, `expectedTaskVersion` and `connectionEpoch` fencing, Delivery idempotency (never double-submit), and honest uncertainty (5 Task states, no success/failure verdict).
- Work in an isolated branch/worktree for independent tasks. Never force-push shared history or auto-merge.
- Run and report actual local checks before claiming completion:
  - `cargo fmt --all -- --check`
  - `cargo check --workspace --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`
