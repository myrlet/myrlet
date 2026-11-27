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

V1 is a single, full-bleed **Work Atlas** workspace (`Board` or `List`) with a collapsible right Inspector and a slide-over Workers drawer, served same-origin by `hub`. There is no page navigation and no separate Events surface — SSE frames are internal cache-invalidation hints, not a product view.

- **App bar (52px, three zones — `1fr auto 1fr`)**: Brand mark plus a clickable live-connection pill (`Hub live` / `Reconnecting…` / `Connecting…` / `Disconnected`; the Hub host and SSE cursor live in its tooltip, not as bar clutter) on the left; the centered `Board | List` segmented switch in the middle; `Workers` (online/total pill that opens the drawer), `Import`, `+ New Task`, and a Settings popover on the right. The popover holds the four themes as `menuitemradio` rows with swatch dots, plus `Keyboard shortcuts (?)` and `Disconnect`; it closes on outside click or Escape.
- **Workspace toolbar (42px)**: Instant search (`⌘K` badge), Status and Worker filters, and a `Clear` ghost button only while a filter is active. On the right: `Fit`, `Focus` (only with a selection), `Restore` (only when a layout can be restored), and `Arrange` in Board mode; `Expand all` / `Collapse all` in List mode; and the `Inspector` toggle.
- **Work Atlas (Board — read-only spatial canvas, default)**: Open dot-grid canvas grouping independent root trees (`TASK GROUP`) with horizontal left-to-right tree spines (up to 5 levels) and live cubic Bézier curves for the three real Hub relationships (`parentTaskId`, sibling `dependencies`, and `sourceTaskId`). Local presentation dragging (`Arrange` / `Restore`), zoom LOD, and an `OVERVIEW` minimap never mutate Hub Task data or relationships.
- **Task List / Tree**: Dense collapsible tree outline with rounded connector guides, grouped by Task tree, and hover quick actions (`Dispatch`, `+ Subtask`, copy ID). Truncated Snapshots (`tasksTruncated`) state `loaded of total` explicitly.
- **Full-bleed first (Choice A)**: The right Inspector starts **collapsed**, giving the canvas the whole width. Selecting a Task — on the Board, in the List, from `⌘K`, or from a Workspace Inspector attention row — opens the Inspector on that Task's 4-tab `Task Detail` (`Overview`, `Tree & Deps`, `Delivery`, `JSON`). Clearing the selection (empty-canvas click, `×`, or Escape) retracts the Inspector. Toggling `Inspector`/`I` shows the `Workspace Inspector` (queued/dispatched attention rows, Worker counts, Manage shortcut) when nothing is selected.
- **Workers & Pairing (slide-over drawer)**: Opens from the App Bar `Workers` pill, the Workspace Inspector `Manage` shortcut, or `⌘K`. Contains one-step enrollment code generation (`worker pair` command), Slot/capability/Executor inventory (`null` capacity rendered as `—`), and credential rotate/revoke, laid over the still-mounted canvas. Escape closes the drawer before it touches Task selection.
- **Status bar (26px)**: Five clickable honest status counters (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), then Task and relationship totals, Board zoom, and the `⌘K · I · ?` shortcuts hint. Live connection and Worker counts are not duplicated here — they belong to the App Bar.

**Excluded from V1 UI**: Graph canvas mutation/workflow editors, Saved Views managers, a raw SSE event browser (the former Events tab), static login topology diagrams, and production-bundled demo fixtures or component galleries (keep fixtures/galleries test-only).

## 4. Verification

Before shipping a UI change:
1. Run the frontend typecheck, unit tests, and production build (`apps/web`), plus the Rust workspace checks (`hub` embeds `apps/web/dist`).
2. Inspect the running Web UI in light and dark themes at desktop widths, checking keyboard focus, truncation, and reduced motion.
3. Report automated tests and visual checks honestly as separate evidence.
