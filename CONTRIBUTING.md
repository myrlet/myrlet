# Contributing to Myrlet

## Commit, Issue, and PR Naming

Use one convention for Issues, PRs, and squash commits:

```text
type(scope): description
```

- **Types**: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `security`
- **Scopes**: `hub`, `worker`, `engine`, `protocol`, `client`, `desktop`, `security`, `architecture`, `ci`, `repo`
- English, lowercase imperative summary, <= 72 chars, no trailing period, no milestone prefixes (`Phase 1:`).
- Link Issues in PR bodies: `Closes #123` only when all acceptance criteria are met; otherwise `Part of #123` or `Refs #123`.

## Engineering Philosophy

### Code Is a Liability, Not an Asset

Retain the **smallest amount of code, state, and schema** that reliably satisfies the [canonical architecture](docs/architecture/README.md) and its invariants.

- Prefer deleting or simplifying over adding new abstractions, indirection layers, configuration knobs, or defensive recovery paths for hypothetical scenarios.
- Do not build for unsupported topologies (multi-Hub, multi-tenant RBAC, hypothetical scale).
- Never weaken core invariants: authentication, file/TLS safety, epoch/version fencing, Delivery idempotency, and honest uncertainty.

### Pre-Release Policy (No Compatibility Shims)

Myrlet has no official release (`0.1.0` pre-release). Unreleased wire DTOs, SQLite schemas, config keys, and on-disk layouts carry **no backward-compatibility obligation**:

- Change canonical formats in place and delete the obsolete path.
- Do **not** add migration shims, dual-read paths, compatibility aliases, or `#[allow(dead_code)]` escapes.
- Never silently rewrite or delete operator-owned files.
- When changing a protocol contract, update the architecture doc, DTO/schema, clients, and tests together.

## Branching, Scope, and Review

- Work in an isolated feature branch / worktree; never force-push shared history or enable auto-merge.
- Keep each PR focused on one cohesive, reviewable outcome.
- If a change requires altering an accepted contract in `docs/architecture/`, state the decision explicitly before expanding scope.
- **Review for correctness and net simplicity, not perfection:**
  - **P0 / P1 (blocking)**: Credible security, data-integrity, availability, or contract-invariant defect. Fix and re-verify before merge.
  - **P2 (non-blocking)**: Concrete maintainability defect or supported edge case; fix or track if worthwhile.
  - **P3 (optional)**: Minor suggestion; no Issue required.
- Authors cannot self-approve; if agents share one GitHub account, record the independent reviewer identity transparently and require human Maintainer review for sensitive architecture, security, or protocol changes.

## Local Verification

Run the checks relevant to what you changed and report only checks that actually executed against the PR head SHA (never claim a skipped check passed):

```bash
# Rust & workspace checks
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets

# Documentation changes
python3 scripts/check-doc-links.py
```

## Documentation Rules

- **One home per durable fact**: Do not copy invariants, endpoint tables, or lifecycle rules across multiple files—link to the canonical file in `docs/architecture/`.
- Keep dynamic project status (Issue/PR state, CI results, milestone progress) in GitHub, not in Markdown docs.
- Run `python3 scripts/check-doc-links.py` on every documentation change.
