/**
 * Single-row chrome: one App Bar with the Board | List switch, no page tabs,
 * no Events surface, no native selects, and no Task write entry points.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import App from "../../App";
import { stubColorScheme } from "../support/appearance";
import {
  appBar,
  connectWorkspace,
  findTokenInput,
  installWorkspaceHub,
  modeSwitch,
  openBoard,
  openList,
  resetWorkspaceTest,
  submitAuthGate,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Workspace chrome", () => {
  it("opens the Board by default with one 52px row, no toolbar, no sidebar, no Events", async () => {
    await connectWorkspace();

    // No left rail, no page tabs, no Events anywhere in the chrome.
    expect(document.querySelector(".sidebar")).toBeNull();
    expect(document.querySelector(".sidebar-rail")).toBeNull();
    expect(screen.queryByRole("navigation", { name: /workspace views/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /^events/i })).toBeNull();
    expect(screen.queryByText(/event stream/i)).toBeNull();

    // Exactly one top row: the App Bar. The old 42px toolbar row is gone.
    expect(document.querySelector(".board-toolbar")).toBeNull();
    expect(screen.queryByRole("toolbar", { name: /board toolbar/i })).toBeNull();
    const frame = document.querySelector(".app-frame") as HTMLElement;
    expect(frame.firstElementChild).toHaveClass("app-bar");

    // No global search field survives in the chrome or the workspace views.
    expect(document.querySelector(".view-body [data-view-search]")).toBeNull();
    expect(within(appBar()).queryByRole("searchbox")).toBeNull();
    expect(document.querySelector(".toolbar-search")).toBeNull();

    // The Board | List switch is centered in the App Bar and defaults to Board.
    const center = appBar().querySelector(".app-bar-center");
    expect(center).not.toBeNull();
    expect(center?.contains(modeSwitch())).toBe(true);
    expect(within(modeSwitch()).getByRole("button", { name: /^board$/i })).toHaveAttribute(
      "data-active",
      "true",
    );

    // Board is the home view, rendered full-width with no Inspector.
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    expect(document.querySelector(".atlas-board")).not.toBeNull();
    expect(document.querySelector(".inspector-slot")).toBeNull();

    openList();
    // Desktop rows: a sticky column header, title-first cells, status, progress,
    // and Worker aligned on one line, plus a copy-ID row action.
    const outline = document.querySelector(".list-outline") as HTMLElement;
    expect(outline).not.toBeNull();
    const labels = Array.from(outline.querySelectorAll(".list-columns .list-col-label")).map(
      (node) => (node.textContent ?? "").trim(),
    );
    expect(labels).toEqual(["Task", "Status", "Progress", "Worker", "Actions"]);
    expect(document.querySelectorAll(".list-row").length).toBe(5);
    expect(
      document.querySelector('.list-row[data-task-id="t1"] .list-action-btn'),
    ).not.toBeNull();
    expect(document.querySelector(".list-group-card")).toBeNull();

    // The status bar carries counters, totals, and hints — but no duplicate
    // live-stream or Workers readouts (those live in the App Bar now).
    const statusBar = document.querySelector(".status-bar") as HTMLElement;
    expect(statusBar.textContent ?? "").toContain("relations");
    expect(statusBar.textContent ?? "").not.toMatch(/live · event stream/i);
    expect(within(statusBar).queryByRole("button", { name: /^workers/i })).toBeNull();
    expect(statusBar.textContent ?? "").toContain("⌘K · I · ?");

    // Workers live in the App Bar as a drawer trigger, not a page.
    expect(
      within(appBar()).getByRole("button", { name: /workers/i }),
    ).toHaveTextContent("1/1");
  });

  it("offers no Task write entry points anywhere in the chrome, List, Board, or Detail", async () => {
    await connectWorkspace();
    openList();

    // Anchored so a Task whose status label reads "Dispatched" is not mistaken
    // for a Dispatch action.
    const writeLabels = [
      /^new task/i,
      /^import/i,
      /^dispatch\b/i,
      /^add subtask/i,
      /^subtask/i,
      /^cancel\b/i,
      /copy \/ retry/i,
    ];

    const assertNoWriteButton = (root: ParentNode, where: string): void => {
      for (const pattern of writeLabels) {
        expect(
          within(root as HTMLElement).queryByRole("button", { name: pattern }),
          `${where} must not offer ${pattern}`,
        ).toBeNull();
      }
    };

    const appBarElement = appBar();
    assertNoWriteButton(appBarElement, "the App Bar");
    expect(within(appBarElement).queryByText(/new task/i)).toBeNull();
    expect(within(appBarElement).queryByText(/import/i)).toBeNull();

    assertNoWriteButton(document.querySelector(".view-body") as HTMLElement, "the List");

    // Read-only Detail: selecting a Task never offers Dispatch / Cancel / Finish.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    assertNoWriteButton(document.querySelector(".detail-pane") as HTMLElement, "Task Detail");
    expect(document.querySelector(".detail-pane")?.textContent ?? "").not.toMatch(
      /dispatch|cancel task|finish task/i,
    );

    await openBoard();
    assertNoWriteButton(document.querySelector(".view-body") as HTMLElement, "the Board");
    expect(
      document.querySelector('.view-body [aria-label="Dispatch Run tests"]'),
    ).toBeNull();
  });

  it("never renders a native select in the gate, workspace, or Workers drawer", async () => {
    stubColorScheme(false);
    render(<App />);
    expect(document.querySelectorAll("select")).toHaveLength(0);

    const tokenField = findTokenInput();
    fireEvent.change(tokenField, { target: { value: "admin-token-value" } });
    submitAuthGate();
    await waitFor(() => {
      expect(document.querySelector(".app-bar")).not.toBeNull();
    });
    expect(document.querySelectorAll("select")).toHaveLength(0);

    fireEvent.click(
      within(appBar()).getByRole("button", { name: /workers/i }),
    );
    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(drawer.querySelectorAll("select")).toHaveLength(0);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector('.view-body[data-view="board"]')).not.toBeNull();
    openList();
    expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();
    expect(document.querySelectorAll("select")).toHaveLength(0);
  });

  it("lists the read-only shortcuts only", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "?" });
    const sheet = await screen.findByRole("dialog", { name: /keyboard shortcuts/i });
    const text = sheet.textContent ?? "";
    expect(text).toContain("Command Palette");
    expect(text).toContain("Fit the Board");
    expect(text).toContain("Show or hide the Inspector");
    expect(text).not.toMatch(/New Task/);
    expect(text).not.toMatch(/^Dispatch the selected Task/);
  });
});
