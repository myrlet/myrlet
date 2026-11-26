# Myrlet UI Design System

Normative visual and interaction contract for `apps/app/` (macOS native and packaged same-origin Web built with [NSH](https://github.com/longbridge/nsh)). Product semantics remain owned by [the canonical architecture](../architecture/README.md).

## 1. Design Intent — Quiet Precision

Myrlet is operational infrastructure, not a chatbot or decorative SaaS dashboard.

- **Calm & precise**: restrained solid surfaces, clear labels, aligned identifiers, explicit state.
- **Truthful**: never invent percentages, step counts, running verdicts, or success/failure from `finished`, `cancelled`, or offline Workers.
- **Accessible without color**: pair every semantic status color with clear text and shape, full keyboard reachability, visible focus rings, and reduced-motion support.
- **Avoid**: nested decorative cards, gradients/glows, ambient animation, hover-only critical actions, or ad-hoc colors/spacing outside shared tokens.

## 2. Tokens and Shared Controls

- **Palette (`src/theme/`)**: `tokens.rs` (semantic roles), `light.rs` (Porcelain), `dark.rs` (Graphite), `nsh.rs` (NSH mapping). Never use raw hex colors in feature views.
- **Shared controls (`src/ui/`)**: `button.rs`, `field.rs`, `select.rs`, `modal.rs`, `disclosure.rs`, `spacing.rs`, `typography.rs`, `motion.rs`. Feature code composes `crate::ui` and NSH layout primitives rather than styling raw `nsh_base` controls.
- **Fact wording (`src/facts.rs`)**: Single formatting source for Task status (`Held`, `Queued`, `Dispatched`, `Handling ended`, `Cancelled`), Delivery milestones, and Progress provenance.
- **Spacing scale**: `4pt`, `8pt`, `12pt`, `16pt`, `24pt`. Compact desktop controls `28–32pt`; dense rows `28–36pt`.
- **Typography**: System sans for UI, monospace only for IDs, shortcuts, and aligned numbers.
- **Motion**: Use `INSTANT_MS` (120ms) / `FAST_MS` (180ms) for state changes; disable transitions under reduced motion. No ambient animation.
- **Storage**: Persist theme preference and client UI state through the single cross-platform NSH storage adapter (`src/client/storage.rs`). Never store credentials in client storage.

## 3. V1 Workspace Information Architecture

V1 is a focused **List/Tree + Task Detail + Worker** operational workspace shared identically across macOS and Web:

- **App header**: Brand mark, Hub connection control (origin, status, cursor), Search, Status/Worker/Executor filters, Theme switch, and explicit New Task / Import Bundle / Workers actions.
- **Task List / Tree**: Dense collapsible tree table (Root + up to 5 levels indented) with sticky headers (`Task`, `Category`, `Status`, `Progress/Execution`, `Worker`). Truncated Snapshots (`tasksTruncated`) state `loaded of total` explicitly.
- **Task Detail**: Read-only detail card/pane showing Overview, Hierarchy (ancestry/parent/children), sibling Dependencies and `sourceTaskId`, current Delivery & latest Harness-authored Progress (text + receipt age, or explicit unavailable state), and footer buttons opening explicit Dispatch / Cancel dialogs.
- **Workers & Pairing panel**: Shows connected/disconnected Workers, reported Slots/capabilities/Executors, one-time pairing code generation, and credential rotate/revoke actions.

**Excluded from V1 UI**: Spatial Graph / Work Atlas canvases, Saved Views managers, static login topology diagrams, and production-bundled demo fixtures or component galleries (keep fixtures/galleries test-only).

## 4. Verification

Before shipping a UI change:
1. Run `cargo fmt --all -- --check`, `cargo check -p myrlet-app --all-targets`, `cargo check -p myrlet-app --target wasm32-unknown-unknown`, and `cargo test -p myrlet-app --locked`.
2. Inspect the running target (macOS and/or Web) in Porcelain and Graphite at desktop and narrow widths, checking keyboard focus, truncation, and reduced motion.
3. Report headless tests and visual checks honestly as separate evidence.
