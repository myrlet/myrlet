# Product

## Purpose

Myrlet is a self-hosted, single-Hub Task Broker connecting developers, operators, and Owner Agents to heterogeneous machines and Harnesses. It makes cross-host work durable, authorized, claimable, and observable without turning the Hub into an Agent planner, workflow engine, or execution sandbox.

## Target Users & Primary Workflow

- **Owner Agents** inspect Tasks and Workers, submit work, diagnose why a Task is waiting, and explicitly decide dependency changes, cancellation, or business retries.
- **Execution Harnesses** receive the Task and Delivery identity, write Progress and a Markdown Result, and call `myrlet done` when their work finishes. The Worker observes execution end and owns communication with the Hub.
- **Human operators** use the Web List, read-only Board, and Inspector to observe Tasks and their results. The Workers drawer handles pairing and credential administration.

## Product Principles

An agent should be able to discover each supported operation, see who acts on it, and inspect the facts that explain its outcome. When work is blocked, expose the reason and the actions available to the caller. Decisions to relax requirements, change dependencies, or retry work belong to the agent.

1. **Preserve authority and truth**: Task coordination state (`held`, `queued`, `dispatched`, `finished`, `cancelled`), Delivery state, and Harness-authored Progress and Result stay distinct. Never display `finished` as success or `cancelled` as system failure.
2. **Make uncertainty visible**: Offline Workers, missing Progress, and stale reports are shown explicitly—never fabricated as zeros, running states, or outcomes.
3. **Calm, dense, native UI**: Use the Porcelain (light) and Graphite (dark) palettes, lavender accent, system typography, restrained motion, and text+shape status labels accessible without color.
4. **Keep V1 focused**: Use the List, read-only Board, Task Inspector, and Worker management to observe work. Graph editing, saved view managers, and workflow builders are outside this scope.

## Portable Task results

An Owner who delegates a review can read the Harness's Markdown findings from Task detail even after the producing Worker disconnects. The Worker publishes the latest Result content, compares it by content hash, and clears the Hub copy after confirming an empty or deleted file. Result, Progress, and end of handling remain independent observations; the Owner decides what the work means.

A future attachment feature could let an operator download a generated chart or PDF. That proposal would add safe inline previews, downloads, and authorization for both Markdown and files. Artifact storage, verification, retention, and preview limits remain outside this contract. A link in Markdown identifies a reference; the Result hash covers its text only.
