/**
 * List and status filtering: five honest counters (or one phone menu) narrow
 * the List and Board, the active pill dismisses, and switching views keeps the
 * filter and selection without ever claiming a verdict.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { stubColorScheme } from "../support/appearance";
import {
  connectWorkspace,
  installWorkspaceHub,
  modeSwitch,
  openBoard,
  openList,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("List and status filtering", () => {
  it("switches List and Board without losing the status filter or selection, and never claims a verdict", async () => {
    await connectWorkspace();
    openList();

    // Filter from the status bar; the List narrows to the finished Task.
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    fireEvent.click(within(statusBar).getByRole("button", { name: /^handling ended\b/i }));

    await waitFor(() => {
      expect(document.querySelector('.list-row[data-task-id="t1"]')).not.toBeNull();
    });
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();
    const pill = document.querySelector(".active-filter-pill") as HTMLElement;
    expect(pill).not.toBeNull();
    expect(pill.textContent ?? "").toContain("Handling ended");

    fireEvent.click(document.querySelector('.list-row[data-task-id="t1"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });

    await openBoard();
    expect(within(modeSwitch()).getByRole("button", { name: /^board$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Filter and selection survive the switch; the Inspector stays open.
    expect(document.querySelector(".active-filter-pill")?.textContent ?? "").toContain(
      "Handling ended",
    );
    expect(document.querySelector(".inspector-slot")).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-selected="true"]')).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-task-id="t1"]')).not.toBeNull();
    expect(document.querySelector('.atlas-node[data-task-id="t2"]')).toBeNull();

    // The chip clears the filter without leaving the view.
    fireEvent.click(
      within(document.querySelector(".active-filter-pill") as HTMLElement).getByRole("button", {
        name: /clear status filter/i,
      }),
    );
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
    });
    expect(document.querySelectorAll(".atlas-node").length).toBeGreaterThan(1);

    fireEvent.click(within(modeSwitch()).getByRole("button", { name: /^list$/i }));
    await waitFor(() => {
      expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();
    });
    expect(document.querySelector(".detail-pane")).not.toBeNull();

    // The List stays truthful about the state it shows.
    const listPane = document.querySelector(".task-pane") as HTMLElement;
    expect(listPane).toHaveTextContent("Handling ended");
    expect(listPane).not.toHaveTextContent("Completed");
    expect(listPane).not.toHaveTextContent("Succeeded");
  });

  it("filters the List from the status bar counters with a dismissible pill", async () => {
    await connectWorkspace();
    openList();

    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    const queued = within(statusBar).getByRole("button", { name: /^queued\b/i });
    fireEvent.click(queued);

    await waitFor(() => {
      expect(queued).toHaveAttribute("aria-pressed", "true");
    });
    const pill = document.querySelector(".active-filter-pill") as HTMLElement;
    expect(pill.textContent ?? "").toContain("Filtered: Queued");
    expect(document.querySelector('.list-row[data-task-id="t3"]')).not.toBeNull();
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();

    fireEvent.click(within(pill).getByRole("button", { name: /clear status filter/i }));
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
    });
    expect(queued).toHaveAttribute("aria-pressed", "false");
    expect(document.querySelector('.list-row[data-task-id="t2"]')).not.toBeNull();
  });

  it("collapses the five status counters into one accessible filter menu on a phone", async () => {
    stubColorScheme(false, true);
    await connectWorkspace();
    openList();

    // No scattered counters and no native select: one trigger opens a menu.
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    expect(statusBar.querySelectorAll(".status-counter")).toHaveLength(0);
    expect(document.querySelectorAll("select")).toHaveLength(0);
    const trigger = within(statusBar).getByRole("button", {
      name: /filter by status/i,
    });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(trigger).toHaveTextContent("All statuses");

    fireEvent.click(trigger);
    const menu = await screen.findByRole("menu", { name: /filter by status/i });
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(within(menu).getByRole("menuitemradio", { name: /all statuses/i })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    const queuedOption = within(menu).getByRole("menuitemradio", { name: /queued/i });
    fireEvent.click(queuedOption);

    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).not.toBeNull();
    });
    expect(screen.queryByRole("menu", { name: /filter by status/i })).toBeNull();
    expect(trigger).toHaveTextContent("Queued");
    expect(document.querySelector('.list-row[data-task-id="t3"]')).not.toBeNull();
    expect(document.querySelector('.list-row[data-task-id="t2"]')).toBeNull();

    // Escape peels the filter off the workspace without touching selection.
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
      expect(trigger).toHaveTextContent("All statuses");
    });
  });

  it("clears the active status filter before closing an open Inspector", async () => {
    await connectWorkspace();
    openList();

    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    const queued = within(statusBar).getByRole("button", { name: /^queued\b/i });
    fireEvent.click(queued);
    await waitFor(() => {
      expect(queued).toHaveAttribute("aria-pressed", "true");
      expect(document.querySelector(".active-filter-pill")).not.toBeNull();
    });

    const listPane = document.querySelector(".task-pane") as HTMLElement;
    fireEvent.click(within(listPane).getByRole("button", { name: /inspector/i }));
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".active-filter-pill")).toBeNull();
      expect(queued).toHaveAttribute("aria-pressed", "false");
    });
    expect(document.querySelector(".inspector-pane")).not.toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
    });
  });
});
