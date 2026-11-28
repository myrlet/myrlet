/**
 * Workers drawer: opened from the App Bar and from the Command Palette, laid
 * over the still-mounted workspace, with unknown capacity rendered as `—`.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  appBar,
  connectWorkspace,
  installWorkspaceHub,
  openList,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Workers drawer", () => {
  it("opens the Workers drawer over the List with unknown capacity as an em dash", async () => {
    await connectWorkspace();
    openList();

    fireEvent.click(
      within(appBar()).getByRole("button", {
        name: /workers/i,
      }),
    );

    const drawer = await screen.findByRole("dialog", { name: /workers and pairing/i });
    expect(drawer).toHaveClass("workers-drawer-panel");
    expect(within(drawer).getByText(/1 of 1 online/i)).toBeInTheDocument();

    // The workspace stays mounted underneath: this is a drawer, not a page.
    expect(document.querySelector('.view-body[data-view="list"]')).not.toBeNull();

    const card = drawer.querySelector<HTMLElement>('[data-worker-id="wkr_00000000000a"]');
    expect(card).not.toBeNull();
    const slotRow = within(card as HTMLElement).getByText("mac-default").closest("tr");
    expect(slotRow).not.toBeNull();
    const slotRowElement = slotRow as HTMLElement;
    expect(within(slotRowElement).getByText("—")).toBeInTheDocument();
    expect(slotRowElement.textContent ?? "").not.toMatch(/capacity\s*0/i);
    expect(slotRowElement.textContent ?? "").not.toContain("0");

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: /workers and pairing/i })).toBeNull();
    });
  });

  it("opens the Workers drawer from the Command Palette action", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    fireEvent.click(within(palette).getByRole("option", { name: /^workers & pairing/i }));

    await waitFor(() => {
      expect(screen.getByRole("dialog", { name: /workers and pairing/i })).toBeInTheDocument();
    });
    expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
  });
});
