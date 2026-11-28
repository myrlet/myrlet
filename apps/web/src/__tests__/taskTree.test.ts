/**
 * Pure Task Tree projection and List grouping, moved out of `format.ts` and the
 * table component so both are testable without rendering.
 */

import { describe, expect, it } from "vitest";

import type { SnapshotTaskSummary, WorkerRecord } from "../api/types";
import { groupRowsForOutline } from "../components/taskOutlineGroups";
import { buildTaskTreeRows, taskTitle } from "../model/taskTree";

function summary(
  overrides: Partial<SnapshotTaskSummary> & { id: string },
): SnapshotTaskSummary {
  return {
    parentTaskId: null,
    rootTaskId: overrides.id,
    depth: 1,
    sourceTaskId: null,
    dependencies: [],
    blockedBy: [],
    category: "general",
    title: overrides.id,
    status: "held",
    version: 1,
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: null,
    currentDelivery: null,
    subtree: { total: 1, held: 1, queued: 0, dispatched: 0, finished: 0, cancelled: 0 },
    ...overrides,
  };
}

describe("buildTaskTreeRows", () => {
  it("orders siblings by createdAtMs then id, and keeps collapsed children off the outline", () => {
    const rows = buildTaskTreeRows(
      [
        summary({ id: "later", createdAtMs: 2 }),
        summary({ id: "b", createdAtMs: 1 }),
        summary({ id: "a", createdAtMs: 1 }),
        summary({ id: "child", parentTaskId: "a", depth: 2, createdAtMs: 3 }),
      ],
      new Set(),
    );

    expect(rows.map((row) => row.id)).toEqual(["a", "b", "later"]);
    expect(rows.find((row) => row.id === "a")?.hasChildren).toBe(true);
    expect(rows.find((row) => row.id === "a")?.expanded).toBe(false);
  });

  it("draws ancestor continuation stems only for siblings that still follow", () => {
    const rows = buildTaskTreeRows(
      [
        summary({ id: "root" }),
        summary({ id: "c1", parentTaskId: "root", depth: 2 }),
        summary({ id: "c2", parentTaskId: "root", depth: 2 }),
        summary({ id: "g1", parentTaskId: "c1", depth: 3 }),
      ],
      new Set(["root", "c1"]),
    );

    const byId = new Map(rows.map((row) => [row.id, row]));
    expect(rows.map((row) => row.id)).toEqual(["root", "c1", "g1", "c2"]);
    expect(byId.get("c1")?.isLastSibling).toBe(false);
    expect(byId.get("c2")?.isLastSibling).toBe(true);
    // g1 sits under c1, which still has a following sibling.
    expect(byId.get("g1")?.ancestorContinuations).toEqual([true]);
    expect(byId.get("g1")?.isLastSibling).toBe(true);
  });

  it("filters on the assigned Worker and keeps the ancestor chain, but never rewrites counts", () => {
    const worker: WorkerRecord["workerId"] = "wkr_00000000000a";
    const tasks = [
      summary({
        id: "root",
        subtree: { total: 2, held: 1, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
      }),
      summary({
        id: "child",
        parentTaskId: "root",
        depth: 2,
        currentDelivery: {
          id: "d1",
          taskId: "child",
          workerId: worker,
          slotId: "slot",
          executorId: "claude-code",
          connectionEpoch: 1,
          state: "accepted",
          cancelState: null,
          cancelReason: null,
          createdAtMs: 1,
          updatedAtMs: 1,
          progress: null,
        },
      }),
      summary({ id: "other" }),
    ];

    const rows = buildTaskTreeRows(tasks, new Set(), { workerId: worker });
    expect(rows.map((row) => row.id)).toEqual(["root", "child"]);
    expect(rows[1]?.matchesFilter).toBe(true);
    expect(rows[0]?.matchesFilter).toBe(false);
    expect(rows[0]?.childCount).toBe(1);
  });

  it("reads the title through either wire shape", () => {
    const record = {
      ...summary({ id: "r" }),
      task: {
        clientRef: null,
        parentTaskId: null,
        sourceTaskId: null,
        dependencies: [],
        category: "ops",
        tags: [],
        title: "Record title",
        prompt: "p",
        input: null,
        expectedOutput: null,
        externalReferences: [],
        routing: {},
        execution: {},
        metadata: {},
      },
    };
    expect(taskTitle(record)).toBe("Record title");
    expect(taskTitle(summary({ id: "s", title: "Summary title" }))).toBe("Summary title");
  });
});

describe("groupRowsForOutline", () => {
  it("buckets tree rows under one divider with a subtree summary and standalone roots without one", () => {
    const rows = buildTaskTreeRows(
      [
        summary({
          id: "root",
          subtree: { total: 3, held: 0, queued: 1, dispatched: 1, finished: 1, cancelled: 0 },
        }),
        summary({ id: "child", parentTaskId: "root", depth: 2 }),
        summary({ id: "alone" }),
      ],
      new Set(["root"]),
    );

    const groups = groupRowsForOutline(rows);
    expect(groups.map((group) => group.kind)).toEqual(["tree", "standalone"]);
    expect(groups[0]?.key).toBe("root");
    expect(groups[0]?.summary).toBe("2 subtasks · 1 queued · 1 dispatched · 1 ended");
    expect(groups[0]?.rows.map((row) => row.id)).toEqual(["root", "child"]);
    expect(groups[1]?.summary).toBe("");
    expect(groups[1]?.rows.map((row) => row.id)).toEqual(["alone"]);
  });

  it("does not repeat a finished root as an ended descendant", () => {
    const rows = buildTaskTreeRows(
      [
        summary({
          id: "done",
          status: "finished",
          subtree: { total: 2, held: 0, queued: 0, dispatched: 0, finished: 2, cancelled: 0 },
        }),
        summary({ id: "kid", parentTaskId: "done", depth: 2, status: "finished" }),
      ],
      new Set(["done"]),
    );

    expect(groupRowsForOutline(rows)[0]?.summary).toBe("1 subtask · 1 ended");
  });
});
