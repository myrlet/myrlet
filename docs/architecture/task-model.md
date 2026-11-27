# Task Model

A **Task** is the durable unit of work owned by Hub. A Subtask is an ordinary Task with `parentTaskId` set.
A **Delivery** identifies one Hub-to-Worker handoff of a Task. Harness Sessions and Workspaces are private to the Harness and are not Task fields or Myrlet identities.

## Proposed Task lifecycle

`submit` authorizes work in one operation. `import` authorizes an atomic batch with the same readiness rules. The five Task states retain distinct observable meanings:

| State | Reason an agent observes it | Available decision |
| --- | --- | --- |
| `held` | At least one named prerequisite has a status other than `finished`, including `cancelled`. | Inspect blocker IDs and statuses, amend the dependencies, cancel the dependent, or wait. |
| `queued` | All prerequisites finished and no Worker has claimed the Task. A wakeup leaves it queued. | Inspect Slot availability and capabilities, update requirements, cancel, or wait. |
| `dispatched` | The Hub committed a claim for a particular Worker and Slot. | Inspect the Delivery or request cancellation. |
| `finished` | For a Task with a Delivery, the Worker observed handling end. | Read the latest Progress and Result and decide on follow-up. |
| `cancelled` | An explicit cancellation took effect. | Inspect affected dependents. Each keeps its existing dependencies. |

Before claim, the Owner may edit dependencies, Slot and capability requirements, and optional preferences. The Hub validates sibling relationships and cycles and recomputes `held` or `queued`. Only Slot and capability constrain routing. Machine, session, and other preferences pass through to execution. Wakeup timing confers no priority over a concurrent Owner edit. At claim commit the Hub checks the latest Task conditions, dependency readiness, Slot eligibility, and absence of another effective Delivery. Pending edits use `expectedTaskVersion`. A losing edit reports the existing claim. The Delivery retains the accepted execution context.

Missing reports never trigger an implicit retry or reassignment. How to represent and close a coordination-only parent under these readiness rules remains open. Parentage alone never finishes a parent. See the [claim protocol](worker-protocol.md#proposed-wakeup-and-claim).

## Task Tree and Dependencies

The following sections document the current binary, including its explicit dispatch and parent-finish operations.

- A Task Tree has at most **5 levels** including the Root (`depth` 1..=5).
- A Root has `parentTaskId: null` and `rootTaskId == taskId`. `parentTaskId` is immutable.
- No self-parenting and no parent cycles.
- **Sibling dependencies only**: `dependencies` may reference only Tasks with the same `parentTaskId` (Roots may depend on other Roots). Cycles are rejected.
- Dependency satisfaction is a deterministic dispatch check, not a DAG workflow engine.
- **Parent vs. subtree**: Creating Subtasks does not finish a Parent. A dispatched Parent ends via its Worker's `finished` report; a coordination-only `held` Parent without a Delivery may be explicitly marked `finished` via `POST /v1/tasks/{id}/finish` (never fabricating a Worker report). Subtree counts are derived views and never change the Parent's status.

## Lifecycle

The current binary interprets the five Hub-owned states as follows. The [proposed lifecycle](#proposed-task-lifecycle) changes the first three.

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

In the current API, client-supplied fields are immutable after creation:

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

The following selection rules apply to the current dispatch API. The proposed Worker claim replaces this ranking.

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
