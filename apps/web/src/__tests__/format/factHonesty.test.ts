/**
 * Fact honesty: status wording, advisory capacity, and tree filtering always
 * render the truth the design system promises.
 */

import { describe, expect, it } from "vitest";

import { TASK_STATUS_META, buildTaskTreeRows, formatCapacity } from "../../format";
import { taskSummary } from "../support/fixtures";

describe("format.ts fact honesty", () => {
  it("labels a finished Task as Handling ended, never as a verdict", () => {
    const finished = TASK_STATUS_META.finished;
    expect(finished.label).toBe("Handling ended");
    expect(finished.label).not.toBe("Completed");
    expect(finished.label).not.toBe("Succeeded");
    expect(finished.terminal).toBe(true);
    expect(finished.description.toLowerCase()).toContain("not a success verdict");
  });

  it("renders unknown advisory capacity as an em dash, never zero", () => {
    expect(formatCapacity(null)).toBe("—");
    expect(formatCapacity(undefined)).toBe("—");
    expect(formatCapacity(2)).toBe("2");
  });

  it("keeps the ancestor chain visible when filtering on a descendant title", () => {
    const root = taskSummary({ id: "t1", title: "Prepare release" });
    const child = taskSummary({ id: "t2", title: "Run unit tests", parentTaskId: "t1", depth: 2 });
    const grandchild = taskSummary({
      id: "t3",
      title: "Run wasm tests",
      parentTaskId: "t2",
      depth: 3,
    });
    const unrelated = taskSummary({ id: "t4", title: "Unrelated root" });

    const unfiltered = buildTaskTreeRows([root, child, grandchild, unrelated], new Set());
    expect(unfiltered.map((row) => row.id)).toEqual(["t1", "t4"]);

    const filtered = buildTaskTreeRows(
      [root, child, grandchild, unrelated],
      new Set(),
      { query: "wasm" },
    );

    expect(filtered.map((row) => row.id)).toEqual(["t1", "t2", "t3"]);
    expect(filtered.map((row) => row.depth)).toEqual([0, 1, 2]);
    expect(filtered.map((row) => row.expanded)).toEqual([true, true, false]);
    expect(filtered[2]?.matchesFilter).toBe(true);
    expect(filtered[0]?.matchesFilter).toBe(false);
  });
});
