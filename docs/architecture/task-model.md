# Task Model

A **Task** is the durable unit of work owned by Hub. A Subtask is an ordinary Task with `parentTaskId` set.
A **Delivery** identifies one Hub-to-Worker handoff of a Task, created by exactly one claim.
For authorization, a Task is assigned to a Worker only after Hub commits a Delivery whose `workerId` matches that Worker. `preferredWorkerId` is a soft routing preference and never grants assignment or read visibility.
Harness Sessions and Workspaces are private to the Harness and are not Task fields or Myrlet identities.

## Task Tree and Dependencies

- A Task Tree has at most **5 levels** including the Root (`depth` 1..=5).
- A Root has `parentTaskId: null` and `rootTaskId == taskId`. `parentTaskId` is immutable.
- No self-parenting and no parent cycles.
- **Sibling dependencies only**: `dependencies` may reference only Tasks with the same `parentTaskId` (Roots may depend on other Roots). Cycles are rejected, both at submission and when dependencies are edited.
- Dependency satisfaction is a deterministic readiness check, not a DAG workflow engine: a declared dependency is satisfied **only** when its status is `finished`.
- **Parent vs. subtree**: Creating Subtasks does not finish a Parent. A claimed Parent ends via its Worker's `finished` report; a coordination-only Parent without a Delivery may be explicitly marked `finished` via `POST /v1/tasks/{id}/finish` (never fabricating a Worker report). Subtree counts are derived views and never change the Parent's status.

## Submission and Readiness

There is **no separate dispatch step**: `submit` authorizes a Task for execution, and `import` authorizes an atomic batch with the same rules.

| Submitted status | Condition | What the caller observes |
| --- | --- | --- |
| `queued` | Every declared dependency is already `finished` (or there are none). | The Task waits for an eligible Worker to claim it. It is claimable immediately. |
| `held` | At least one declared dependency has any other status, including `cancelled`. | The Task names its blocking dependency IDs and statuses in `blockedBy`. |

- A `finished` prerequisite releases its waiting siblings; a `cancelled` prerequisite **keeps them `held`**.
- Only an explicit Owner action changes a dependency or cancels a dependent. Cancellation never edits the graph or cascades on its own.
- `submit` and `import` are idempotent under `Idempotency-Key`; replay returns the originally committed Task(s).

### Owner edits to an unclaimed Task

Before a Task has a Delivery, the Owner may change its `dependencies` (`POST /v1/tasks/{id}`). The Hub revalidates sibling rules and rejects cycles, then recomputes `held` ⇄ `queued`. Edits are fenced by `expectedTaskVersion`. If an edit races a claim, the claim wins and the edit fails with `409 InvalidTransition`, so the Delivery keeps the execution context it committed with.

## Lifecycle

Every Task has one Hub-owned `status` from five values:

| Status | Meaning |
| --- | --- |
| `held` | Submitted, but at least one dependency is not `finished`. Not claimable. |
| `queued` | Authorized and readiness-satisfied, waiting for an eligible Worker to claim it. No Delivery exists. |
| `dispatched` | A Worker claimed the Task and the Hub committed its one Delivery. |
| `finished` | The Delivery's Worker reported that handling ended (or the Owner finished a coordination-only Task). Not a success/failure verdict. |
| `cancelled` | Explicitly cancelled. Terminal. |

### Legal Transitions

- `held -> queued` (a dependency `finished` or an Owner edit made the Task ready) or `held -> cancelled`.
- `queued -> dispatched` (a Worker's committed claim) or `queued -> cancelled`.
- `held` or `queued` -> `finished` **only** via `POST /v1/tasks/{id}/finish` for a coordination-only Task with no Delivery.
- `dispatched -> finished` (Worker HTTP finish report).
- Any non-terminal state (`held | queued | dispatched`) `-> cancelled`.
- `finished` and `cancelled` are terminal. **`cancelled` is never overwritten by a later finish report.**
- Progress and Result updates never change Task status. Execution start is derived from the Delivery's Progress `firstReceivedAtMs`; if no Progress ever arrives, the Task stays `dispatched` until `finished` or `cancelled`.

## Task Definition

Client-supplied fields are immutable after submission, except the pending edits above:

- **Relationships**: `parentTaskId`, `sourceTaskId`, `dependencies`.
- **Classification**: `category` (free ASCII token up to 64 chars, default `"general"`, retrieval/filter only), `tags`.
- **Instruction**: `title`, `prompt`, optional `input`, `expectedOutput`, `externalReferences`, `metadata`.
- **Routing**: `requiredCapabilities` and Slot choice are **hard** constraints; `preferredWorkerId`, `preferredRole`, `preferredExecutorId` are **soft** preferences the Hub never uses to block a claim.
- **Execution**: optional `preferredModel` (soft pass-through only; omitted if Harness lacks a native flag), optional `timeoutSeconds`, opaque `parameters`.

Hub generates and owns: `id`, `rootTaskId`, `depth`, `status`, `version`, `createdAtMs`, `updatedAtMs`, `finishedAtMs`, current Delivery summary, `blockedBy`, the latest Result, and derived subtree counts.

### Optimistic Concurrency

Task mutations (submit, pending edit, cancel, coordination-only finish) accept an `Idempotency-Key`; edits, cancel, and finish additionally require `expectedTaskVersion`, and mismatched versions return `409 Conflict`. There is no nested ownership boundary or 3-tuple ownership fence in V1.

### Minimal Task Definition Example

```json
{
  "title": "Verify the build",
  "prompt": "Run the test suite and write a brief summary to the Progress file."
}
```

## Task Bundle Import

- **Task Bundle Import**: A single atomic endpoint `POST /v1/task-bundles/import` submits up to 100 Tasks in one SQLite transaction, resolving bundle-local `clientRef` parent and sibling dependency references. Each Task is authorized with the same readiness rules: those whose prerequisites are all `finished` after the transaction become `queued` (and appear in `eligibleTaskIds`); the rest stay `held`. It never picks a Worker and never creates a Delivery.

## Business Retry

A business retry creates a new `held`/`queued` Task linked via `sourceTaskId` and submits it explicitly. Copying or retrying never cancels, replaces, or overwrites the source Task.

## Coordination-only Parents

A Parent used purely to group work has no Delivery of its own. It stays `held` while its own declared dependencies are unfinished and can be closed with `POST /v1/tasks/{id}/finish` once that work is done; parentage alone never finishes a parent. This is the whole coordination model: there is no Planner, DAG engine, OR-join, or approval step.
