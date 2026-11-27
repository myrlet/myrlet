# Agent workflow review scenarios

The [architecture index](../architecture/README.md#contract-under-review) identifies the proposed changes to the V1 baseline. Use these scenarios to review the observable behavior across process boundaries. Command names are provisional. The runtime requirements below are acceptance criteria for implementation, not test results.

## A blocked review Task

A build has been cancelled and its dependent review Task is still `held`. The Owner can inspect the blocking build ID, see its `cancelled` status, and discover which other Tasks depend on it. Replacing that dependency with build C makes the review eligible when C reaches `finished`. If C already finished but no suitable Slot is available, the review appears `queued`. The Owner can inspect the hard requirements and edit them explicitly. See the [Task lifecycle](../architecture/task-model.md#proposed-task-lifecycle).

## Two Harnesses finishing on one machine

The prompt for execution A supplies `done` with A’s Delivery ID. A calls it and receives a local acknowledgment while its process is still alive. The Hub can have its Result before the Worker observes exit and reports Finish. After Slot reuse, replay A’s command and verify that B’s Result and lifecycle remain untouched. Compare the [local command boundary](../architecture/local-runtime.md#proposed-agent-commands) with the [Worker’s wakeup and claim](../architecture/worker-protocol.md#proposed-wakeup-and-claim).

## Acceptance scenarios

Exercise the [reporting contract](../architecture/hub-api.md#proposed-reporting-semantics) and [agent access boundary](../architecture/security.md#proposed-agent-access) as well as ordinary pickup.

| Scenario | Expected observation |
| --- | --- |
| Worker A stays silent. The Hub wakes B, then both claim. | Before either claim the Task is `queued` with no Delivery. After the race exactly one Delivery exists. A lost claim acknowledgment recovers the winning assignment. |
| Sync A times out, local content changes to B, and the Worker reconnects. | After the old epoch is fenced and B is confirmed, delayed A cannot overwrite B. If A committed but its acknowledgment was lost, restart also converges to B. Exercise both cases independently for Progress and Result. |
| A stored Hub Result exists when the Worker restarts. | Incomplete recovery and read errors preserve it. After recovery, test an empty file and a confirmed deletion separately. Each clears the Hub Result, and repeating the sync leaves it absent. |

## Decisions required before implementation

- Define the atomic pickup and recovery exchange, including competing Workers, concurrent cancellation, and a crash between claim and Harness launch. Specify how a coordination-only parent closes under the proposed `held` rule.
- Define local Worker signaling and its acknowledgment boundary. A duplicate `done` for an old Delivery must leave a newer Delivery on the same Slot untouched.
- Specify digest inputs and how the Hub enforces Delivery ownership and epoch fencing atomically at the write boundary.
- Define global read authorization and operator bootstrap. Verify that a Harness can read Tasks but cannot use that access to cancel one or rotate a credential. Preserve bearer, Task version, connection epoch, and file safety checks.

The proposed changes take effect following maintainer approval and implementation. Pre-release formats can change directly. The [Result reader](https://github.com/myrlet/myrlet/issues/9) can continue to render the latest Task detail independently.
