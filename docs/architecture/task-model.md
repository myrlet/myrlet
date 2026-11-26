# Task Model

A **Task** is the durable unit of work owned by Hub. A Subtask is an ordinary Task with `parentTaskId` set.
A **Delivery** identifies one Hub-to-Worker handoff of a Task. Harness Sessions and Workspaces are private to the Harness and are not Task fields or Myrlet identities.

## Task Tree and Dependencies

- A Task Tree has at most **5 levels** including the Root (`depth` 1..=5).
- A Root has `parentTaskId: null` and `rootTaskId == taskId`. `parentTaskId` is immutable.
- No self-parenting and no parent cycles.
- **Sibling dependencies only**: `dependencies` may reference only Tasks with the same `parentTaskId` (Roots may depend on other Roots). Cycles are rejected.
- Dependency satisfaction is a deterministic dispatch check, not a DAG workflow engine.
- **Parent vs. subtree**: Creating Subtasks does not finish a Parent. A dispatched Parent ends via its Worker's `finished` report; a coordination-only `held` Parent without a Delivery may be explicitly marked `finished` via `POST /v1/tasks/{id}/finish` (never fabricating a Worker report). Subtree counts are derived views and never change the Parent's status.

## Lifecycle

Every Task has one Hub-owned `status` from five values:

| Status | Meaning |
| --- | --- |
| `held` | Durable intent created; not authorized to execute. |
| `queued` | Dispatch authorized and waiting in FIFO queue for an eligible Worker/Slot; no Delivery exists. |
| `dispatched` | Hub created and offered a Delivery to a Worker. |
| `finished` | Assigned Worker reported that its handling ended (or Owner finished a coordination-only `held` Parent). Not a success/failure verdict. |
| `cancelled` | Explicitly cancelled. Terminal. |

### Legal Transitions

- `held -> queued` or `held -> dispatched` (explicit dispatch command).
- `queued -> dispatched` (FIFO queue admission when an eligible Worker/Slot is online) or `queued -> cancelled`.
- `dispatched -> finished` (Worker HTTP finish report).
- `dispatched -> held` **only** on a proven pre-registration Worker refusal (`task.rejected` before any registration or Progress).
- Any non-terminal state (`held | queued | dispatched`) `-> cancelled`.
- `finished` and `cancelled` are terminal. **`cancelled` is never overwritten by a later finish report.**
- Progress updates never change Task status (including after cancellation). Execution start is derived from the Delivery's `firstReceivedAtMs` of Progress; if no Progress ever arrives, the Task remains `dispatched` until `finished` or `cancelled`.

## Task Definition

Client-supplied fields are immutable after creation:

- **Relationships**: `parentTaskId`, `sourceTaskId`, `dependencies`.
- **Classification**: `category` (free ASCII token up to 64 chars, default `"general"`, retrieval/filter only), `tags`.
- **Instruction**: `title`, `prompt`, optional `input`, `expectedOutput`, `externalReferences`, `metadata`.
- **Routing**: `requiredCapabilities` (hard constraints), optional `preferredWorkerId`, `preferredRole`, `preferredExecutorId`.
- **Execution**: optional `preferredModel` (soft pass-through only; omitted if Harness lacks a native flag), optional `timeoutSeconds`, opaque `parameters`.

Hub generates and owns: `id`, `rootTaskId`, `depth`, `status`, `version`, `createdAt`, `updatedAt`, `finishedAt`, current Delivery summary, and derived subtree counts.

### Optimistic Concurrency

Task mutations (dispatch, cancel, finish coordination-only parent) require `expectedTaskVersion` plus an `Idempotency-Key`. Mismatched versions return `409 Conflict`. There is no nested ownership boundary or 3-tuple ownership fence in V1.

### Minimal Task Creation Example

```json
{
  "title": "Verify the build",
  "prompt": "Run the test suite and write a brief summary to the Progress file."
}
```

## Dispatch, Queueing, Bundles, and Retry

- **Targeted Dispatch**: Names an exact `workerId`, `slotId`, and optional `executorId`. Hub never falls back to another Worker or Slot. If offline, Hub either rejects (`409`/`503`) or enqueues in FIFO order when `queue: true`.
- **Simple Match**: Deterministically picks an online Worker/Slot where every `requiredCapabilities` entry is in the `available` state (closed set: `available`, `degraded`, `unavailable`), preferring:
  1. `preferredWorkerId` match (when `preferredWorkerId` is set, Hub **never** falls back to another Worker unless the dispatch request explicitly sets `fallbackAllowed: true`);
  2. `preferredRole` match on the same Slot;
  3. `preferredExecutorId` match;
  4. fewer observed active Deliveries;
  5. greater advisory reported capacity (`null` last);
  6. lexical `(workerId, slotId)` tie-breaker.
  No semantic scoring or LLM scheduling.
- **Simple FIFO Queue**: Queued Tasks wait without reserving capacity. Whenever a Worker connects/updates inventory or a Delivery finishes/cancels, Hub scans waiting Tasks in FIFO order and dispatches those whose dependencies and target requirements are now satisfied.
- **Task Bundle Import**: Single atomic endpoint `POST /v1/task-bundles/import` creates up to 100 `held` Tasks in one SQLite transaction, resolving bundle-local `clientRef` parent and sibling dependency references. It never dispatches.
- **Business Retry**: Creates a new `held` Task linked via `sourceTaskId` and dispatches it explicitly. Copying/retrying never cancels, replaces, or overwrites the source Task.
