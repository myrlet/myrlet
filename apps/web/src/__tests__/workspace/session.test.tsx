/**
 * Session teardown: Disconnect and auth loss both start a fresh, empty
 * workspace instead of carrying filter, Inspector, or selection state over.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HubApiError, HubClient } from "../../api/client";
import { buildSnapshot, taskSummary } from "../support/fixtures";
import {
  connectWorkspace,
  findTokenInput,
  installWorkspaceHub,
  openList,
  resetWorkspaceTest,
  submitAuthGate,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Workspace session reset", () => {
  it.each(["disconnect", "auth loss"])("starts a fresh workspace after %s", async (reason) => {
    const snapshot = buildSnapshot();
    snapshot.tasks.push(taskSummary({
      id: "child", parentTaskId: "t1", rootTaskId: "t1", depth: 2, title: "Child task",
    }));
    const getSnapshot = vi.spyOn(HubClient.prototype, "getSnapshot").mockResolvedValue({
      ...snapshot, totalTasks: snapshot.tasks.length,
    });
    await connectWorkspace();
    openList();
    fireEvent.click(screen.getByRole("button", { name: /collapse ship release/i }));
    expect(document.querySelector('.list-row[data-task-id="child"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^queued\b/i }));
    fireEvent.click(within(document.querySelector(".task-pane") as HTMLElement)
      .getByRole("button", { name: /inspector/i }));
    if (reason === "disconnect") {
      fireEvent.click(screen.getByRole("button", { name: /settings/i }));
      fireEvent.click(screen.getByRole("menuitem", { name: /disconnect/i }));
    } else {
      getSnapshot.mockRejectedValueOnce(new HubApiError(401, "Unauthorized", "Expired"));
      fireEvent.click(screen.getByRole("button", { name: /refresh the snapshot/i }));
    }
    await screen.findByRole("button", { name: /connect to hub/i });
    fireEvent.change(findTokenInput(), { target: { value: "another-test-token" } });
    submitAuthGate();

    await screen.findByRole("toolbar", { name: /board controls/i });
    expect(document.querySelector(".active-filter-pill")).toBeNull();
    expect(document.querySelector(".inspector-slot")).toBeNull();
    openList();
    expect(document.querySelector('.list-row[data-task-id="child"]')).toBeInTheDocument();
  });
});
