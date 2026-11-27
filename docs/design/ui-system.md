# Myrlet UI Design System

Visual and interaction principles for the planned Myrlet Web UI (`apps/web/`, React/TypeScript/Vite served same-origin by `hub`, with an optional Tauri 2 desktop shell in a later iteration). Product semantics remain owned by [the canonical architecture](../architecture/README.md).

## 1. Design Intent — Quiet Precision

Myrlet is operational infrastructure, not a chatbot or decorative SaaS dashboard.

- **Calm & precise**: restrained solid surfaces, clear labels, aligned identifiers, explicit state.
- **Truthful**: never invent percentages, step counts, running verdicts, or success/failure from `finished`, `cancelled`, or offline Workers.
- **Accessible without color**: pair every semantic status color with clear text and shape, full keyboard reachability, visible focus rings, and reduced-motion support.
- **Avoid**: nested decorative cards, gradients/glows, ambient animation, hover-only critical actions, or ad-hoc colors/spacing outside shared tokens.

## 2. Tokens and Shared Controls

- **Palette**: Semantic light (**Porcelain**, **Paper**) and dark (**Obsidian**, **Graphite**) CSS/design tokens. Never scatter ad-hoc hex colors in feature views.
- **Fact wording**: Single formatting source for Task status (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), Delivery milestones, and Progress provenance.
- **Spacing scale**: `4px`, `8px`, `12px`, `16px`, `24px`. Compact desktop controls `28–32px`; dense rows `28–38px`.
- **Typography**: System sans for UI, monospace only for IDs, shortcuts, and aligned numbers.
- **Motion**: `120ms` / `180ms` for state changes; instant command palette; respect `prefers-reduced-motion`. No ambient animation.
- **Storage**: Theme preference (`myrlet.theme`) may persist in browser storage; **never** store bearer tokens in `localStorage`, URLs, or packaged assets (keep credentials in memory only).

## 3. V1 Workspace Information Architecture

V1 is a Graph-centric **Work Atlas (Board) + List/Tree + Inspector + Workers/Events** operational workspace served same-origin by `hub`:

- **App bar (52px)**: Brand mark, Hub origin & live SSE status pill, workspace view switcher (`Workspace`, `Workers`, `Events`), 4-theme selector, explicit `Import` / `+ New Task` actions, and disconnect control.
- **Board / List toolbar (48px)**: Instant search (`⌘K` Command Palette entry), Status and Worker filters, `Board | List` mode switch (defaulting to `Board`), canvas controls (`Fit`, `Focus`, `Restore`, `Arrange`) in Board mode or `Expand all` in List mode, and Inspector toggle.
- **Work Atlas (Board — read-only spatial canvas)**: Open dot-grid canvas grouping independent root trees (`TASK GROUP`) with horizontal left-to-right tree spines (up to 5 levels) and live cubic Bézier curves for the three real Hub relationships (`parentTaskId`, sibling `dependencies`, and `sourceTaskId`). Local presentation dragging (`Arrange` / `Restore`), zoom LOD, and an `OVERVIEW` minimap never mutate Hub Task data or relationships.
- **Task List / Tree**: Dense collapsible tree table with orthogonal connector guides, sticky headers (`Task`, `Category`, `Status`, `Progress`, `Worker`), and hover quick actions (`Dispatch`, `+ Subtask`, copy ID). Truncated Snapshots (`tasksTruncated`) state `loaded of total` explicitly.
- **Right Inspector**: Shows `Workspace Inspector` (queued/dispatched tasks, Workers summary, workspace counts) when nothing is selected, or the 4-tab `Task Detail` pane (`Overview`, `Tree & Deps`, `Delivery`, `JSON`) with explicit footer actions (`Dispatch`, `+ Subtask`, `Finish`, `Copy / Retry`, `Cancel`).
- **Workers & Pairing / Live Events**: Dedicated views for one-step enrollment code generation (`worker pair` command), Worker Slot/capability/Executor inventory (`null` capacity rendered as `—`), credential rotate/revoke, and raw SSE invalidation frame inspection.

**Excluded from V1 UI**: Graph canvas mutation/workflow editors, Saved Views managers, static login topology diagrams, and production-bundled demo fixtures or component galleries (keep fixtures/galleries test-only).

## 4. Verification

Before shipping a UI change:
1. Run the frontend typecheck, unit tests, and production build (`apps/web`), plus the Rust workspace checks (`hub` embeds `apps/web/dist`).
2. Inspect the running Web UI in light and dark themes at desktop widths, checking keyboard focus, truncation, and reduced motion.
3. Report automated tests and visual checks honestly as separate evidence.
