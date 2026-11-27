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

Slot and capability are the only hard routing rules. Machine, session, and other preferences travel with the Task. An unprepared or shutting-down Worker skips pickup. When a prepared Worker requests work for an available Slot, the Hub checks the requirements and dependencies and grants a single Delivery atomically. It does not silently relax requirements or rank Workers by an undisclosed policy.

The Owner can update requirements, preferences, and dependencies on unclaimed Tasks. If an update races a pick, either the claim uses the new Task version or the edit reports that a Delivery already exists. The Worker durably registers that Delivery ID before invoking the Harness. A crash with an uncertain start must never trigger a second invocation merely because the Worker disconnected.

## Harness reporting

The Worker supplies the actual Delivery ID in the startup prompt. Every execution-local command takes that ID, even on a single-Slot machine. The Worker resolves its own local record and rejects an unknown ID. Slot reuse cannot redirect a late command to the new occupant.

| Operation | Observable effect |
| --- | --- |
| `result check <delivery-id>` | Returns local, advisory feedback on Markdown or artifact references. A warning allows sync and Finish to proceed. File privacy, path safety, and size limits still apply. |
| `result sync <delivery-id>` | Requests publication of the current local Result. A local receipt and a Hub confirmation are distinguishable. Results can be updated during execution and after Finish. |
| `result status <delivery-id>` | Compares local and Hub content without writing. Distinguishes equal, different, missing on either side, and unknown when the Hub is unreachable. |
| `done <delivery-id>` | Repeatable notice to the local Worker that the Harness has finished its work. It requests sync of any current Result. The Worker reports Finish only after observing the execution end. |

The prompt gives `done` as the last command before exit. If it is skipped, observed exit still triggers final sync and Finish. A missing Result permits Finish. A `finished` Task tells the Owner that handling ended; the Owner evaluates the work.

## Progress and Result converge independently

For each Delivery the Worker owns the current Progress and Result content. The Hub stores the latest copy of each. Compare hashes over the agreed content bytes. Equal hashes need no write. On a mismatch, sync the current Worker value. For Markdown, the digest covers the text, including reference strings. Verifying attachment contents would require their bytes as well.

If the Worker sends A, loses the acknowledgment, then writes B, it must reread current content on reconnect. Reconcile until the Hub confirms B, including if the old request arrives late. A hash establishes equality and cannot establish ordering. The protocol must handle that race without revision counters, historical copies, or replaying intermediate updates. Progress may converge while Result is still pending, and vice versa.

## Decisions required before implementation

- Define the atomic pickup and recovery exchange, including competing Workers, concurrent cancellation, and a crash between claim and Harness launch. Specify how a coordination-only parent closes under the proposed `held` rule.
- Define local Worker signaling and its acknowledgment boundary. A duplicate `done` for an old Delivery must leave a newer Delivery on the same Slot untouched.
- Specify the digest inputs, absent versus explicitly cleared content, and the conditional-write or ordering rule for delayed requests. Exercise lost acknowledgments and restart with different local and Hub content.
- Define global read authorization and operator bootstrap. Verify that a Harness can read Tasks but cannot use that access to cancel one or rotate a credential. Preserve bearer, Task version, connection epoch, and file safety checks.

Maintainer approval precedes changes to the normative contracts and implementation. Pre-release formats can change directly. The [Result reader](https://github.com/myrlet/myrlet/issues/9) can continue to render the latest Task detail independently.
