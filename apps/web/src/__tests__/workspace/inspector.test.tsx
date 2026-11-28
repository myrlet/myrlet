/**
 * Inspector: List selection opens Task Detail, the Workspace overview appears
 * with no selection, pointer-cancel stops a resize, and the narrow-screen sheet
 * uses one sibling scrim with an inert background and focus return.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { stubColorScheme } from "../support/appearance";
import {
  boardFloatingBar,
  connectWorkspace,
  installWorkspaceHub,
  openList,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Workspace Inspector", () => {
  it("opens Task Detail on click in the List and toggles the Workspace Inspector when nothing is selected", async () => {
    await connectWorkspace();
    openList();

    const row = document.querySelector<HTMLElement>('.list-row[data-task-id="t3"]');
    expect(row).not.toBeNull();
    fireEvent.click(row as HTMLElement);

    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    expect(document.querySelector(".inspector-slot")).not.toBeNull();

    // Escape clears the selection and retracts the Inspector (full-width again).
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
    });

    // Toggling Inspector with no selection shows the Workspace overview.
    const listPane = document.querySelector(".task-pane") as HTMLElement;
    fireEvent.click(within(listPane).getByRole("button", { name: /inspector/i }));
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });
    expect(document.querySelector(".detail-pane")).toBeNull();
  });

  it("stops resizing the Inspector when a pointer drag is cancelled", async () => {
    await connectWorkspace();
    fireEvent.click(within(boardFloatingBar()).getByRole("button", { name: /inspector/i }));
    const handle = screen.getByRole("separator", { name: /resize inspector/i });
    const pointer = (type: string, clientX: number) =>
      new MouseEvent(type, { bubbles: true, clientX });
    fireEvent(handle, pointer("pointerdown", 800));
    fireEvent(window, pointer("pointermove", 760));
    expect(handle).toHaveAttribute("aria-valuenow", "454");
    fireEvent(window, pointer("pointercancel", 760));
    fireEvent(window, pointer("pointermove", 720));
    expect(handle).toHaveAttribute("aria-valuenow", "454");
  });

  it("uses one sibling scrim, inert background, and focus return for both Inspector modes", async () => {
    stubColorScheme(false, true);
    await connectWorkspace();
    openList();

    const listPane = document.querySelector(".task-pane") as HTMLElement;
    const inspectorTrigger = within(listPane).getByRole("button", { name: /inspector/i });
    inspectorTrigger.focus();
    fireEvent.click(inspectorTrigger);
    await waitFor(() => {
      expect(document.querySelector(".inspector-pane")).not.toBeNull();
    });
    const viewBody = document.querySelector(".view-body") as HTMLElement;
    const slot = document.querySelector(".inspector-slot") as HTMLElement;
    const scrim = document.querySelector(".inspector-scrim") as HTMLElement;
    expect(scrim).not.toBeNull();
    // The scrim is a sibling behind the panel, never a wrapping child: the
    // panel's own close button must win the hit test.
    expect(scrim.parentElement).toBe(slot.parentElement);
    expect(scrim.contains(slot)).toBe(false);
    expect(viewBody).toHaveAttribute("inert");
    await waitFor(() => {
      expect(document.activeElement).toBe(slot);
    });

    // The panel's own close button is reachable and closes the Inspector.
    const closeButton = within(slot).getByRole("button", {
      name: /close workspace inspector/i,
    });
    closeButton.focus();
    expect(document.activeElement).toBe(closeButton);
    fireEvent.click(closeButton);
    await waitFor(() => {
      expect(document.querySelector(".inspector-slot")).toBeNull();
      expect(document.activeElement).toBe(inspectorTrigger);
    });
    expect(viewBody).not.toHaveAttribute("inert");

    const row = document.querySelector<HTMLElement>('.list-row[data-task-id="t3"]');
    expect(row).not.toBeNull();
    row?.focus();
    fireEvent.click(row as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).not.toBeNull();
    });
    expect(document.querySelector(".inspector-scrim")).not.toBeNull();
    expect(viewBody).toHaveAttribute("inert");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(document.querySelector(".detail-pane")).toBeNull();
      expect(document.activeElement).toBe(row);
    });
  });
});
