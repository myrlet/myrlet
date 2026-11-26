# Myrlet UI Design System

Visual and interaction principles for the planned Myrlet Web UI (`apps/web/`, React/TypeScript/Vite served same-origin by `hub`, with an optional Tauri 2 desktop shell in a later iteration). Product semantics remain owned by [the canonical architecture](../architecture/README.md).

## 1. Design Intent — Quiet Precision

Myrlet is operational infrastructure, not a chatbot or decorative SaaS dashboard.

- **Calm & precise**: restrained solid surfaces, clear labels, aligned identifiers, explicit state.
- **Truthful**: never invent percentages, step counts, running verdicts, or success/failure from `finished`, `cancelled`, or offline Workers.
- **Accessible without color**: pair every semantic status color with clear text and shape, full keyboard reachability, visible focus rings, and reduced-motion support.
- **Avoid**: nested decorative cards, gradients/glows, ambient animation, hover-only critical actions, or ad-hoc colors/spacing outside shared tokens.

## 2. Tokens and Shared Controls

- **Palette**: Semantic light (**Porcelain**) and dark (**Graphite**) CSS/design tokens. Never scatter ad-hoc hex colors in feature views.
- **Fact wording**: Single formatting source for Task status (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), Delivery milestones, and Progress provenance.
- **Spacing scale**: `4px`, `8px`, `12px`, `16px`, `24px`. Compact desktop controls `28–32px`; dense rows `28–36px`.
- **Typography**: System sans for UI, monospace only for IDs, shortcuts, and aligned numbers.
- **Motion**: `120ms` / `180ms` for state changes; respect `prefers-reduced-motion`. No ambient animation.
- **Storage**: Theme preference may persist in browser storage; **never** store bearer tokens in `localStorage`, URLs, or packaged assets (keep credentials in memory only).

## 3. V1 Workspace Information Architecture

V1 is a focused **List/Tree + Task Detail + Worker** operational workspace shared identically across macOS and Web:

- **App header**: Brand mark, Hub connection control (origin, status, cursor), Search, Status/Worker/Executor filters, Theme switch, and explicit New Task / Import Bundle / Workers actions.
- **Task List / Tree**: Dense collapsible tree table (Root + up to 5 levels indented) with sticky headers (`Task`, `Category`, `Status`, `Progress/Execution`, `Worker`). Truncated Snapshots (`tasksTruncated`) state `loaded of total` explicitly.
- **Task Detail**: Read-only detail card/pane showing Overview, Hierarchy (ancestry/parent/children), sibling Dependencies and `sourceTaskId`, current Delivery & latest Harness-authored Progress (text + receipt age, or explicit unavailable state), and footer buttons opening explicit Dispatch / Cancel dialogs.
- **Workers & Pairing panel**: Shows connected/disconnected Workers, reported Slots/capabilities/Executors, one-time pairing code generation, and credential rotate/revoke actions.

**Excluded from V1 UI**: Spatial Graph / Work Atlas canvases, Saved Views managers, static login topology diagrams, and production-bundled demo fixtures or component galleries (keep fixtures/galleries test-only).

## 4. Verification

Before shipping a UI change:
1. Run the frontend typecheck, lint, production build, and unit/E2E checks.
2. Inspect the running Web UI served same-origin by `hub` in Porcelain and Graphite at desktop and mobile widths, checking keyboard focus, truncation, and reduced motion.
3. Report automated tests and visual checks honestly as separate evidence.
