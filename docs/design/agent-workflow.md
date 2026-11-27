# Proposed agent workflow

Architecture review for [issue #10](https://github.com/myrlet/myrlet/issues/10). The [V1 contract](../architecture/README.md) describes the current explicit dispatch and write-once Result. The operations below describe the proposed replacement. Command names are provisional.

## What an agent can infer

An Owner looking at a waiting Task should be able to answer three questions from the CLI: What is it waiting for? Which recorded fact explains that? What can I change? A Harness that reports work needs to know which Delivery it reported on, who acknowledged it, and what the Hub can confirm. Expose these facts in structured output as well as human-readable text. Group help by Hub queries, Owner writes, and local Worker operations.

Owners and Harnesses can query all Tasks, dependencies, Workers, and events. A Harness can look up context without receiving an Admin credential. Global visibility grants read access only. Keep credential administration outside the everyday Agent CLI.

## Submission and pickup

`submit` authorizes one Task for execution. `import` authorizes an atomic batch. Dependencies and routing answer different questions:

| What the Owner reads | What it means | What the Owner can do |
| --- | --- | --- |
| `held`, with blocking Task IDs and statuses | At least one prerequisite has yet to reach `finished`. A cancelled prerequisite continues to block. | Inspect the prerequisites. Explicitly replace or remove a dependency, cancel the dependent, or leave it held. |
| `queued`, with Slot and capability requirements | All dependencies are satisfied. The Task is available for an eligible Worker to pick. | Inspect Worker inventory, update requirements or preferences, or wait. Inventory describes the last report and may be stale. |
| `dispatched`, with a Delivery ID and assigned Worker and Slot | The Hub committed a claim. Starting the Harness and observing its exit are separate Worker actions. | Inspect the Delivery and latest Progress. A disconnect alone cannot establish what happened to the process. |

For example, before cancelling build A, the Owner can discover that test B depends on it. After cancellation, B stays `held` and names A as its blocker. If the Owner substitutes build C, the Hub validates the sibling relationship and checks for cycles. When every prerequisite reaches `finished`, B becomes `queued`. This transition follows directly from submission and the recorded dependencies. Cancellation never edits another Task's dependencies.

Slot and capability are the only hard routing rules. Machine, session, and other preferences travel with the Task. The Hub applies no hidden ranking or automatic fallback and never relaxes the requirements. Pickup has three observable steps.

1. The Hub checks dependencies, capabilities, and advertised Slot availability, then wakes one eligible Worker. The Task stays `queued`, with no reservation or Delivery. If the Worker stays silent, the Hub can wake the next eligible Worker. Workers need no candidate polling loop.
2. A prepared Worker chooses an eligible local Slot and explicitly claims the Task with its Task ID and Slot ID. The Hub checks readiness, routing constraints, and the absence of an effective Delivery in the same transaction that creates the Delivery. Even if the first Worker responds after a second Worker was notified, only one claim can win.
3. The winner durably registers the Delivery ID before starting the Harness. If the claim acknowledgment is lost, the Worker recovers that same Delivery. A disconnect after a committed claim leaves the execution uncertain. Recovery preserves the assignment and prevents a second launch.

The Owner can update requirements, preferences, and dependencies on unclaimed Tasks, including after a wakeup. If an edit races a claim, either the claim uses the new Task version or the edit reports the existing Delivery. Cancellation that commits first makes the Task ineligible.

## Harness reporting

The Worker supplies the actual Delivery ID in the startup prompt. Every execution-local command takes that ID, even on a single-Slot machine. The Worker resolves its own local record and rejects an unknown ID. Slot reuse cannot redirect a late command to the new occupant.

| Operation | Observable effect |
| --- | --- |
| `result check <delivery-id>` | Returns local, advisory feedback on Markdown or artifact references. A warning allows sync and Finish to proceed. File privacy, path safety, and size limits still apply. |
| `result sync <delivery-id>` | Requests publication of the current local Result. A local receipt and a Hub confirmation are distinguishable. Results can be updated during execution and after Finish. |
| `result status <delivery-id>` | Compares local and Hub content without writing. Distinguishes equal, different, missing on either side, and unknown when the Hub is unreachable. |
| `done <delivery-id>` | Repeatable notice to the local Worker that the Harness has finished its work. It requests reconciliation of the local Result. The Worker reports Finish only after observing the execution end. |

The prompt gives `done` as the last command before exit. If it is skipped, observed exit still triggers final sync and Finish. A missing Result permits Finish. A `finished` Task tells the Owner that handling ended; the Owner evaluates the work.

## Progress and Result converge independently

A Task has one effective Delivery at a time, assigned to one Worker and one Slot. That Worker is the sole writer of its Progress and Result. The Hub keeps the latest copy of each. The Worker serializes sync for the Delivery and rereads the current local value on every attempt. For each value independently, matching hashes require no write. On a mismatch the Worker sends its current value. A Markdown digest covers the text and its reference strings. Verifying attachment contents would require their bytes as well.

After local recovery completes, a missing or empty Result file means **no Result**. Either observation clears any prior Hub Result on sync, including after Finish. Incomplete recovery, a read error, or an unsafe file cannot establish absence. The Hub keeps its copy until the Worker can establish the local state.

A timed-out write can still arrive. Before sending newer content, the Worker must establish that the earlier write has settled or fence its connection epoch. The Hub validates Delivery ownership and epoch when committing the write. On reconnect the Worker reconciles from the recovered local state. This single-writer design needs only the latest content, without revision counters, history, or content CAS.

## Acceptance scenarios

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

Maintainer approval precedes changes to the normative contracts and implementation. Pre-release formats can change directly. The [Result reader](https://github.com/myrlet/myrlet/issues/9) can continue to render the latest Task detail independently.
