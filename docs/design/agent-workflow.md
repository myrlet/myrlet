# Agent workflow review scenarios

Use these scenarios to check the observable behavior across process boundaries. The commands and frames below are the implemented contract; the linked architecture documents are authoritative.

## A blocked review Task

A build has been cancelled and its dependent review Task is still `held`. The Owner can inspect the blocking build ID, see its `cancelled` status, and discover which other Tasks depend on it. Replacing that dependency with build C makes the review eligible when C reaches `finished`. If C already finished but no suitable Slot is available, the review appears `queued`. The Owner can inspect the hard requirements and edit them explicitly. See the [Task lifecycle](../architecture/task-model.md#submission-and-readiness).

## Two Harnesses finishing on one machine

The prompt for execution A supplies `done` with A’s Delivery ID. A calls it and receives a local acknowledgment while its process is still alive. The Hub can have its Result before the Worker observes exit and reports Finish. After Slot reuse, replay A’s command and verify that B’s Result and lifecycle remain untouched. Compare the [local command boundary](../architecture/local-runtime.md#execution-local-cli-commands) with the [Worker’s wakeup and claim](../architecture/worker-protocol.md#wakeup-and-claim).

## Acceptance scenarios

Exercise the [reporting contract](../architecture/hub-api.md#worker-progress-result-and-finish-reporting-http) and [read visibility](../architecture/security.md#read-visibility) as well as ordinary pickup.

| Scenario | Expected observation |
| --- | --- |
| Worker A stays silent. The Hub wakes B, then both claim. | Before either claim the Task is `queued` with no Delivery. After the race exactly one Delivery exists. A lost claim acknowledgment recovers the winning assignment. |
| Sync A times out, local content changes to B, and the Worker reconnects. | After the old epoch is fenced and B is confirmed, delayed A cannot overwrite B. If A committed but its acknowledgment was lost, restart also converges to B. Exercise both cases independently for Progress and Result. |
| A stored Hub Result exists when the Worker restarts. | Incomplete recovery and read errors preserve it. After recovery, test an empty file and a confirmed deletion separately. Each clears the Hub Result, and repeating the sync leaves it absent. |

## Boundaries these scenarios pin down

- The atomic pickup exchange covers competing Workers, concurrent cancellation, and a crash between claim and Harness launch. A coordination-only parent closes with `tasks finish` once its own work is done.
- Local Worker signaling and its acknowledgment boundary: a duplicate `done` for an old Delivery leaves a newer Delivery on the same Slot untouched.
- Digest inputs are the exact Markdown bytes; the Hub enforces Delivery ownership and epoch fencing at its write boundary.
- Global reads and operator bootstrap: a Harness can read Tasks but cannot cancel one or rotate a credential. Bearer, Task version, connection epoch, and file-safety checks are preserved.

Pre-release formats can change directly, with no shims. The [Result reader](https://github.com/myrlet/myrlet/issues/9) renders the latest Task detail independently.
