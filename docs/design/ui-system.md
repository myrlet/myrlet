# Myrlet UI Design System

Visual and interaction principles for the Myrlet Web UI (`apps/web/`, React/TypeScript/Vite served same-origin by `hub`, with an optional Tauri 2 desktop shell in a later iteration). Product semantics remain owned by [the canonical architecture](../architecture/README.md).

## 1. Design Intent — Quiet Precision

Myrlet is operational infrastructure, not a chatbot or decorative SaaS dashboard.

- **Calm & precise**: restrained solid surfaces, clear labels, aligned identifiers, explicit state.
- **Truthful**: never invent percentages, step counts, running verdicts, or success/failure from `finished`, `cancelled`, or offline Workers.
- **Read-only by contract**: the Web UI observes the Hub. Creating, importing, dispatching, cancelling, finishing, adding subtasks, and copy/retry are CLI operations and appear nowhere in this UI.
- **Accessible without color**: pair every semantic status color with clear text and shape, full keyboard reachability, visible focus rings, and reduced-motion support.
- **Avoid**: nested decorative cards, gradients/glows, ambient animation, hover-only critical actions, native `<select>` elements, or ad-hoc colors/spacing outside shared tokens.

## 2. Tokens and Shared Controls

- **Appearance**: exactly two visual themes — **light** and **dark** — selected by **System** (default, follows `prefers-color-scheme`), **Light**, or **Dark**. `hub.themeMode` resolves to `light` / `dark` and is written to `<html data-theme>`; only an explicit choice persists (`myrlet.theme`).
- **Brand**: `BrandMark` (the ant collaboration silhouette) paints with `currentColor`; `public/favicon.svg` mirrors the same path and swaps ink to `prefers-color-scheme`.
- **Fact wording**: Single formatting source for Task status (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), Delivery milestones, and Progress provenance.
- **Spacing scale**: `4px`, `8px`, `12px`, `16px`, `24px`. Compact desktop controls `28–32px`; dense rows `28–38px`.
- **Typography**: System sans for UI, monospace only for IDs, shortcuts, and aligned numbers.
- **Motion**: `120ms` / `180ms` for state changes; instant command palette; respect `prefers-reduced-motion`. No ambient animation.
- **Storage**: only the non-sensitive appearance preference (`myrlet.theme`) and Board layout offsets (`myrlet.atlas.positions`) are persisted; **never** store bearer tokens in `localStorage`, URLs, or packaged assets (keep credentials in memory only).
- **Controls**: no native `<select>` anywhere. Segmented controls (`.mode-switch`, `.theme-segment`), pills, and buttons carry every choice.

## 3. V1 Workspace Information Architecture

V1 is a single, full-bleed **Work Atlas** workspace (`Board` or `List`) with a collapsible right Inspector and a slide-over Workers drawer, served same-origin by `hub`. There is no page navigation, no second toolbar row, and no separate Events surface — SSE frames are internal cache-invalidation hints, not a product view.

- **App bar (one 52px row, three zones — `1fr auto 1fr`)**: Brand mark plus a clickable live-connection pill (`Hub live` / `Reconnecting…` / `Connecting…` / `Disconnected`; the Hub host and SSE cursor live in its tooltip) on the left; the centered `Board | List` segmented switch in the middle; a `Workers` online/total pill (drawer trigger) and a Settings popover on the right. The popover holds the `APPEARANCE` segment (`System | Light | Dark`, `menuitemradio` rows with `Monitor` / `Sun` / `Moon` icons), a divider, `Keyboard shortcuts (?)`, and `Disconnect`; it closes on outside click or Escape. There is no search field, Status select, Worker select, `Import`, or `+ New Task` in the chrome.
- **Work Atlas (Board — read-only spatial canvas, default)**: Open dot-grid canvas grouping independent root trees (`TASK GROUP`) with horizontal left-to-right tree spines (up to 5 levels) and live cubic Bézier curves for the three real Hub relationships (`parentTaskId`, sibling `dependencies`, and `sourceTaskId`). Board controls float inside the canvas's top-right corner (`.atlas-floating-bar`): `Fit`, `Focus` (only with a selection), `Restore` (only when a layout can be restored), `Arrange`, then a divider and the `Inspector` toggle. An active status filter floats as a dismissible `.active-filter-pill` in the top-left. Local presentation dragging, zoom LOD, and the `OVERVIEW` minimap never mutate Hub Task data or relationships.
- **Task List / Tree**: Dense collapsible tree outline with rounded connector guides, grouped by Task tree. List controls sit inline above the cards (`.list-inline-controls`, max-width `1120px`): the active-filter pill on the left; `Expand all` / `Collapse all` and the `Inspector` toggle on the right. Rows are read-only; the only row-level action is `Copy ID`, and there is no `Dispatch`, `Subtask`, `Retry`, or other Task write. Truncated Snapshots (`tasksTruncated`) state `loaded of total` explicitly.
- **Filters**: one dimension only. Clicking any of the five status counters in the status bar toggles `statusFilter`; Board and List both narrow to that status, and the dismissible pill (or `Esc`) clears it. There is no free-text or per-Worker filter.
- **Full-bleed first (Choice A)**: The right Inspector starts **collapsed**, giving the canvas the whole width. Selecting a Task — on the Board, in the List, from `⌘K`, or from a Workspace Inspector attention row — opens the Inspector on that Task's read-only 4-tab `Task Detail` (`Overview`, `Tree & Deps`, `Delivery`, `JSON`). Clearing the selection (empty-canvas click, `×`, or Escape) retracts the Inspector. Toggling `Inspector`/`I` shows the `Workspace Inspector` (queued/dispatched attention rows, Worker counts, Manage shortcut) when nothing is selected.
- **Workers & Pairing (slide-over drawer)**: Opens from the App Bar `Workers` pill, the Workspace Inspector `Manage` shortcut, or `⌘K`. Contains one-step enrollment code generation (`worker pair` command), Slot/capability/Executor inventory (`null` capacity rendered as `—`), and credential rotate/revoke, laid over the still-mounted canvas. Escape closes the drawer before it touches Task selection.
- **⌘K Command Palette**: Task jump list, `Board` / `List`, `Workers & Pairing`, `Toggle Inspector`, `Fit Board`, `Arrange Board`, and the three appearance options. No create, import, or dispatch commands.
- **Status bar (26px)**: Five clickable honest status counters (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), then Task and relationship totals, Board zoom, and the `⌘K · I · ?` shortcuts hint. Live connection and Worker counts are not duplicated here — they belong to the App Bar.
- **Keyboard**: `⌘K` palette, `F` fit, `Shift+F` focus the selected Task, `I` Inspector, `↑` / `↓` (or `j` / `k`) move the List selection, `Esc` close overlay / clear selection, `?` shortcut sheet. No write shortcuts exist.

**Excluded from V1 UI**: Task creation/import/dispatch/cancel/finish/subtask/retry entry points, graph canvas mutation/workflow editors, Saved Views managers, a raw SSE event browser (the former Events tab), static login topology diagrams, and production-bundled demo fixtures or component galleries (keep fixtures/galleries test-only).

## 4. Verification

Before shipping a UI change:
1. Run the frontend typecheck, unit tests, and production build (`apps/web`), plus the Rust workspace checks (`hub` embeds `apps/web/dist`).
2. Inspect the running Web UI in both resolved appearances (light and dark, including System following the OS) at desktop widths, checking keyboard focus, truncation, and reduced motion.
3. Report automated tests and visual checks honestly as separate evidence.
