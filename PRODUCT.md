# Product

## Purpose

Myrlet is a self-hosted, single-Hub Task Broker connecting developers, operators, and Owner Agents to heterogeneous machines and Harnesses. It makes cross-host work durable, dispatchable, and observable without turning the Hub into an Agent planner, workflow engine, or execution sandbox.

## Target Users & Primary Workflow

- **Owner Agents / CLI users**: Inspect available Workers via Snapshot, create `held` Tasks or Task Trees (up to 5 levels with sibling dependencies), explicitly dispatch work (`targeted` or `match`), observe latest Harness Progress and `finished` transitions, and decide follow-ups or retries outside Hub.
- **Human operators (App)**: Use a calm, dense **List/Tree + Task Detail + Worker** workspace (macOS native and same-origin Web) to inspect active workstreams, read latest Progress, see offline/stale Workers truthfully, and trigger explicit create/dispatch/cancel/pairing actions.

## Product Principles

1. **Preserve authority and truth**: Task coordination state (`held`, `queued`, `dispatched`, `finished`, `cancelled`), Delivery handoff state, and Harness-authored Progress stay distinct. Never display `finished` as success or `cancelled` as system failure.
2. **Make uncertainty visible**: Offline Workers, missing Progress, and stale reports are shown explicitly—never fabricated as zeros, running states, or outcomes.
3. **Calm, dense, native UI**: Use the Porcelain (light) and Graphite (dark) palettes, lavender accent, system typography, restrained motion, and text+shape status labels accessible without color.
4. **Keep V1 focused**: List/Tree + Task Detail + Worker management is the V1 graphical surface. Custom spatial graph canvases, saved view managers, and workflow builders are out of V1 scope.

## Proposed capability: portable Task results

An Owner Agent that delegates a review should be able to read the reviewer's findings.
A human who asks a remote Worker to generate a chart should be able to open it from another machine.
Both recipients should be able to retrieve the work after the producing Worker goes offline.

- The Harness may write a result locally as Markdown with optional files. The Worker syncs that output to the Hub. A review can link to a PR, and a Task with no repository can return its findings as text alone.
- Clients render headings, lists, tables, code blocks, and links consistently. Recipients can preview common images inline and download attached files such as a PDF report.
- The standard API exposes the Markdown source and artifact references to authorized consumers, including a subsequent Worker. A Worker without read access must be denied access to both the text and the files.

Result sync is independent of Delivery acknowledgment, latest Progress, and `finished`. Handling can end without a result, including when a transfer fails. The Owner decides what the work means. See the [Task and Delivery contract](docs/architecture/README.md#v1-core-invariants).

Follow-up design should settle when the Worker syncs a result, who may read it, and how files are identified, verified, stored, and retained. It should also define preview and size limits and what the recipient sees after an interrupted transfer.
