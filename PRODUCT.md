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
