/**
 * Unit coverage for the React-free Task Detail projections: root-to-current
 * ancestry (missing parents, cycles, and the display bound) and the bounded
 * Context Mini-Tree ordering/deduplication.
 */

import { describe, expect, it } from "vitest";

import { compareTasks } from "../model/taskIndex";
import type { TaskRecord, TaskTreeResponse } from "../api/types";
import {
  MAX_ANCESTRY_LENGTH,
  buildAncestry,
  miniTreeRows,
  minimumDepth,
} from "../components/taskDetail/projections";

function record(
  id: string,
  parentTaskId: string | null = null,
  options: { depth?: number; createdAtMs?: number } = {},
): TaskRecord {
  return {
    id,
    rootTaskId: "root",
    depth: options.depth ?? 1,
    status: "queued",
    version: 1,
    createdAtMs: options.createdAtMs ?? 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    finishedAtMs: null,
    currentDelivery: null,
    blockedBy: [],
    resultMarkdown: null,
    resultRecordedAtMs: null,
    subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    task: {
      clientRef: null,
      parentTaskId,
      sourceTaskId: null,
      dependencies: [],
      category: "general",
      tags: [],
      title: id,
      prompt: `Work on ${id}.`,
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

function tree(tasks: TaskRecord[], truncated = false): TaskTreeResponse {
  return { rootTaskId: "root", tasks, truncated };
}

describe("compareTasks", () => {
  it("orders by creation time, then Task ID as a stable tie-break", () => {
    const newer = record("a", null, { createdAtMs: 20 });
    const older = record("b", null, { createdAtMs: 10 });
    const tiedA = record("a", null, { createdAtMs: 10 });
    expect([newer, older, tiedA].sort(compareTasks).map((task) => task.id)).toEqual([
      "a",
      "b",
      "a",
    ]);
    expect(compareTasks(record("x"), record("x"))).toBe(0);
    expect(compareTasks(record("a"), record("b"))).toBeLessThan(0);
    expect(compareTasks(record("b"), record("a"))).toBeGreaterThan(0);
  });
});

describe("buildAncestry", () => {
  it("returns only the current Task when the tree read is absent or empty", () => {
    const child = record("child", "parent");
    expect(buildAncestry(child, null).map((task) => task.id)).toEqual(["child"]);
    expect(buildAncestry(child, tree([])).map((task) => task.id)).toEqual(["child"]);
  });

  it("walks root-to-current through the bounded tree read", () => {
    const tasks = [record("root"), record("mid", "root"), record("leaf", "mid")];
    const chain = buildAncestry(tasks[2], tree(tasks));
    expect(chain.map((task) => task.id)).toEqual(["root", "mid", "leaf"]);
  });

  it("stops at a parent the bounded read did not return", () => {
    const tasks = [record("mid", "outside"), record("leaf", "mid")];
    expect(buildAncestry(tasks[1], tree(tasks, true)).map((task) => task.id)).toEqual([
      "mid",
      "leaf",
    ]);
  });

  it("cuts a parent cycle instead of looping", () => {
    const tasks = [record("a", "b"), record("b", "a")];
    expect(buildAncestry(tasks[0], tree(tasks)).map((task) => task.id)).toEqual(["b", "a"]);
  });

  it("bounds the displayed ancestry even when the chain is longer", () => {
    const tasks: TaskRecord[] = [];
    for (let index = 0; index < MAX_ANCESTRY_LENGTH + 4; index += 1) {
      tasks.push(record(`t${index}`, index === 0 ? null : `t${index - 1}`));
    }
    const chain = buildAncestry(tasks[tasks.length - 1], tree(tasks));
    // The chain keeps the current Task and the nearest ancestors; it never
    // reaches the root once the display bound is hit.
    expect(chain).toHaveLength(MAX_ANCESTRY_LENGTH);
    expect(chain[chain.length - 1].id).toBe(`t${MAX_ANCESTRY_LENGTH + 3}`);
    expect(chain[0].id).toBe("t4");
  });

  it("prefers the tree read's own copy of the selected Task", () => {
    const fromTree = record("leaf", "mid");
    fromTree.task.title = "From tree";
    const tasks = [record("mid"), fromTree];
    const chain = buildAncestry(record("leaf", "mid"), tree(tasks));
    expect(chain[chain.length - 1].task.title).toBe("From tree");
  });
});

describe("miniTreeRows", () => {
  it("renders a depth-first tree with siblings in deterministic order", () => {
    const rows = miniTreeRows(
      tree([
        record("root"),
        record("b", "root", { createdAtMs: 20 }),
        record("a", "root", { createdAtMs: 10 }),
        record("a1", "a", { createdAtMs: 5 }),
      ]),
    );
    expect(rows.map((row) => row.id)).toEqual(["root", "a", "a1", "b"]);
  });

  it("promotes a Task whose parent is missing from the bounded read to a root", () => {
    const rows = miniTreeRows(tree([record("orphan", "outside"), record("root")], true));
    expect(rows.map((row) => row.id).sort()).toEqual(["orphan", "root"]);
  });

  it("lists a self-parented Task exactly once", () => {
    const rows = miniTreeRows(tree([record("self", "self")]));
    expect(rows.map((row) => row.id)).toEqual(["self"]);
  });

  it("lists every member of a parent cycle exactly once", () => {
    const rows = miniTreeRows(tree([record("a", "b"), record("b", "a")]));
    expect(rows.map((row) => row.id).sort()).toEqual(["a", "b"]);
  });

  it("returns an empty projection for an empty read and scales to the read bound", () => {
    expect(miniTreeRows(tree([]))).toEqual([]);
    const many = Array.from({ length: 200 }, (_, index) =>
      record(`t${index}`, index === 0 ? null : `t${index - 1}`),
    );
    expect(miniTreeRows(tree(many))).toHaveLength(200);
  });
});

describe("minimumDepth", () => {
  it("is 0 for an empty projection and the shallowest row otherwise", () => {
    expect(minimumDepth([])).toBe(0);
    expect(
      minimumDepth([record("a", null, { depth: 3 }), record("b", null, { depth: 2 })]),
    ).toBe(2);
  });
});
