/**
 * Workspace keyboard ownership: shortcuts belong to the active surface and are
 * released when an overlay or the phone status menu takes the keys.
 */

import "@testing-library/jest-dom/vitest";

import { act, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { stubColorScheme } from "../support/appearance";
import {
  appBar,
  boardFloatingBar,
  connectWorkspace,
  installWorkspaceHub,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Workspace shortcuts", () => {
  it("reserves Shift+F for a selected Board task", async () => {
    await connectWorkspace();
    fireEvent.click(within(boardFloatingBar()).getByRole("button", { name: /inspector/i }));
    expect(fireEvent.keyDown(document, { key: "F", shiftKey: true })).toBe(true);
    fireEvent.click(document.querySelector('.atlas-node[data-task-id="t1"]') as HTMLElement);
    expect(fireEvent.keyDown(document, { key: "F", shiftKey: true })).toBe(false);
    await screen.findByText("Work on Ship release.");
  });

  it("only resets Board zoom when the workspace owns the shortcut", async () => {
    await connectWorkspace();
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    const zoom = screen.getByRole("button", { name: /reset to 100%/i });
    const previous = zoom.textContent;
    expect(previous).not.toBe("100%");
    fireEvent.click(within(appBar()).getByRole("button", { name: /workers/i }));
    fireEvent.keyDown(screen.getByRole("button", { name: /close workers drawer/i }), { key: "0" });
    expect(zoom.textContent).toBe(previous);
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.keyDown(document, { key: "0" });
    expect(zoom.textContent).toBe("100%");
  });

  it("releases the phone status menu shortcuts when the desktop counters appear", async () => {
    const media = stubColorScheme(false, true);
    await connectWorkspace();
    fireEvent.click(await screen.findByRole("button", { name: /filter by status/i }));
    expect(screen.getByRole("menu", { name: /filter by status/i })).toBeInTheDocument();
    act(() => media.setNarrow(false));
    fireEvent.keyDown(document, { key: "i" });
    expect(document.querySelector(".inspector-slot")).toBeInTheDocument();
  });
});
