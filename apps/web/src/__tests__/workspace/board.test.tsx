/**
 * Work Atlas Board: controls dock in a real header row, Focus appears only with
 * a selection, and a parent card keeps its fixed geometry while showing the
 * subtree summary directly under the title.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { buildSnapshot, taskSummary, workerRecord } from "../support/fixtures";
import { idleEventStream, jsonResponse, stubFetch } from "../support/hub";
import {
  boardFloatingBar,
  connectWorkspace,
  installWorkspaceHub,
  openBoard,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Work Atlas Board", () => {
  it("docks Board controls in a real header row so they never cover group labels", async () => {
    await connectWorkspace();
    await openBoard();

    const dock = boardFloatingBar();
    expect(dock).toHaveClass("atlas-dock");
    expect(dock.closest(".atlas-stage")).toBeNull();
    expect(dock.closest(".atlas-board")).not.toBeNull();
    expect(within(dock).getByText(/relationships/i)).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /fit/i })).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /arrange/i })).toBeInTheDocument();
    expect(within(dock).getByRole("button", { name: /inspector/i })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(within(dock).queryByRole("button", { name: /focus/i })).toBeNull();
  });

  it("reveals Board Focus only with a selection and keeps dimmed titles readable", async () => {
    await connectWorkspace();
    await openBoard();

    fireEvent.click(document.querySelector('.atlas-node[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    const focused = boardFloatingBar();
    expect(within(focused).getByRole("button", { name: /focus/i })).toBeInTheDocument();
    expect(within(focused).getByRole("button", { name: /inspector/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    // Selection dims unrelated cards but their titles stay present and legible.
    const dimmed = document.querySelector('.atlas-node[data-dimmed="true"]');
    expect(dimmed).not.toBeNull();
    expect(dimmed?.querySelector(".atlas-node-title")?.textContent ?? "").not.toBe("");
  });

  it("places a parent Task's subtask summary under its title without changing the card size", async () => {
    const snapshotTasks = [
      taskSummary({
        id: "p1",
        title: "Parent work",
        status: "dispatched",
        subtree: { total: 2, held: 0, queued: 0, dispatched: 2, finished: 0, cancelled: 0 },
      }),
      taskSummary({
        id: "c1",
        title: "Child work",
        parentTaskId: "p1",
        rootTaskId: "p1",
        depth: 2,
        status: "dispatched",
      }),
    ];
    stubFetch((url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return idleEventStream();
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse({ ...buildSnapshot(), totalTasks: 2, tasks: snapshotTasks });
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [workerRecord()] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });

    await connectWorkspace();
    await openBoard();

    const card = document.querySelector('.atlas-node[data-task-id="p1"]') as HTMLElement;
    expect(card).not.toBeNull();
    const title = card.querySelector(".atlas-node-title") as HTMLElement;
    const summary = card.querySelector(".atlas-node-summary") as HTMLElement;
    expect(title.textContent).toBe("Parent work");
    expect(summary).not.toBeNull();
    // The full single-line summary sits directly below the title, not at the
    // bottom of the card.
    expect(title.nextElementSibling).toBe(summary);
    expect(summary.textContent ?? "").toContain("1 subtask");
    // Card geometry stays fixed: height comes from the layout constant.
    expect(card.style.height).toBe("140px");
  });
});
