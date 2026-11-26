# Contributing to Myrlet

## Naming Convention

Use one format for Issues, PRs, and squash commits:

```text
type(scope): description
```

- **Types**: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `security`
- **Scopes**: `hub`, `worker`, `protocol`, `client`, `cli`, `app`, `security`, `architecture`, `ci`, `repo`
- English, lowercase imperative summary, <= 72 chars, no trailing period, no milestone prefixes (`Phase 1:`).
- Link Issues in PR bodies: `Closes #123` only when all acceptance criteria are met; otherwise `Part of #123` or `Refs #123`.

## Anti-Overengineering Rules (Read Before Coding)

**Code is a liability, not an asset.** Retain the smallest amount of code, state, and schema that satisfies [docs/architecture/README.md](docs/architecture/README.md) and its invariants.

Before adding any new table, state, HTTP route, WSS frame, trait, config option, cache, lock, or retry/recovery path, answer all five questions:

1. Is it listed under **Explicit V1 Non-Goals** in `docs/architecture/README.md`? *(If yes, stop immediately.)*
2. Can an existing table, state, or field represent it without adding a new mechanism?
3. Does a concrete, supported V1 runtime path require it—not a hypothetical topology or test-only scenario?
4. Does a new abstraction or config knob have **at least two** real callers today? *(If not, inline it.)*
5. Can narrowing a responsibility boundary eliminate the need for coordination machinery altogether?

### Pre-Release Policy (No Compatibility Shims)

Myrlet `0.1.0` is pre-release and carries **no backward-compatibility obligation** for unreleased wire DTOs, SQLite schemas, config keys, or on-disk layouts:

- Change canonical formats in place and delete the obsolete path.
- Never add migration shims, dual-read paths, compatibility aliases, or `#[allow(dead_code)]` escapes.
- Never silently rewrite or delete operator-owned files.

## Branching, PR Size, and Review Standards

- Work in an isolated branch/worktree; never force-push shared history or enable auto-merge.
- Keep each PR focused on one cohesive outcome (prefer **<= 500–800 net new LOC** for routine PRs; split milestones by crate/module).
- Any change to accepted contracts in `docs/architecture/` requires an **Architecture Change** Issue and human Maintainer approval before implementation.

### PR Review Rules

Review for **correctness, boundary discipline, and net simplicity**—not defensive perfection:

| Severity | Meaning | Disposition |
| --- | --- | --- |
| **P0** | Critical security, data-integrity, or availability bug with a credible trigger | Blocking |
| **P1** | Material correctness/invariant violation, **or clear over-engineering** (unapproved states/tables/routes/non-goals, unused abstractions, test-only RBAC/scaffolding) | Blocking |
| **P2** | Concrete maintainability issue or supported edge case | Fix or track if worthwhile |
| **P3** | Minor suggestion | Optional, no Issue needed |

- **Reviewers MUST NOT** demand extra abstractions, file splitting solely by line count, speculative edge-case guards, or support for excluded topologies.
- **Reviewers SHOULD** actively ask whether code, tables, states, or config options can be deleted or simplified.
- Authors cannot self-approve; if agents share one GitHub account, state the independent reviewer transparently and require human Maintainer approval for architecture, security, and protocol changes.

## Local Verification

Run and report only checks that actually executed on the PR head SHA (never claim a skipped check passed):

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

## Documentation Rules

- **One home per durable fact**: Define invariants, APIs, and lifecycle rules once in `docs/architecture/` and link to them—never copy-paste across docs.
- Keep dynamic project status (Issue/PR state, CI results, milestones) in GitHub, not in Markdown docs.
