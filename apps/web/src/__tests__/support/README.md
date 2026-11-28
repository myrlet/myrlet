# Frontend test support

Shared, test-only modules behind the suites under `src/__tests__/`.

| Module | Owns |
| --- | --- |
| `fixtures.ts` | `taskSummary`, `workerRecord`, `taskRecord`, `buildSnapshot` — the canonical Snapshot and Worker shapes, including unknown (`null`) Slot capacity. |
| `hub.ts` | `stubFetch`, `jsonResponse`, `streamResponse`, `idleEventStream` — a stubbed `fetch` and the SSE bodies the Hub can return. |
| `appearance.ts` | `stubColorScheme` — a controllable `matchMedia` for System/dark and narrow viewport tests. |
| `workspace.tsx` | The default Hub stub, the Auth gate flow (`connectWorkspace`, `findTokenInput`, `submitAuthGate`), and navigation helpers (`modeSwitch`, `openList`, `openBoard`, `appBar`, `boardFloatingBar`). |
| `resultHub.tsx` | The Result fixtures (`LONG_RESULT`, `SHORT_RESULT`), a Hub stub whose Result can change mid-test, the deterministic preview box model, and the Result navigation helper. |

Rule: a helper lives here only when more than one suite needs it.
Feature-specific behaviour stays in the suite that asserts it.

## Layout

- `format/` — pure `format.ts` fact wording and tree rows.
- `api/` — `HubClient` transport and the public health probe.
- `state/` — `useHub` and `useTaskDetail` hooks.
- `workspace/` — one file per App-level workspace surface (chrome, auth gate, session, appearance, Board, List/filtering, Inspector, keyboard, Task Detail, Workers, Command Palette).
- `result/` — the Result preview and the full-screen reader.

Assertions are preserved one-for-one from the former monolithic suites.
