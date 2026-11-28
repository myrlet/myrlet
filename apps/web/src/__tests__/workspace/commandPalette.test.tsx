/**
 * Command Palette: read-only commands and appearance options only, opened with
 * Ctrl+K and closed on Escape.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { connectWorkspace, installWorkspaceHub, resetWorkspaceTest } from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Command Palette", () => {
  it("opens the Command Palette with Ctrl+K without Task write actions, and closes on Escape", async () => {
    await connectWorkspace();

    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: /command palette/i });
    expect(palette).toBeInTheDocument();
    expect(
      within(palette).getByPlaceholderText(/search tasks, views, actions, appearance/i),
    ).toBeInTheDocument();

    expect(within(palette).getByRole("option", { name: /^board/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^list/i })).toBeInTheDocument();
    expect(
      within(palette).getByRole("option", { name: /^workers & pairing/i }),
    ).toBeInTheDocument();
    expect(
      within(palette).getByRole("option", { name: /toggle inspector/i }),
    ).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /fit board to view/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /arrange board layout/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^system/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^light/i })).toBeInTheDocument();
    expect(within(palette).getByRole("option", { name: /^dark/i })).toBeInTheDocument();

    // The palette is strictly read-only: no create / import / dispatch.
    expect(within(palette).queryByRole("option", { name: /^new task/i })).toBeNull();
    expect(within(palette).queryByRole("option", { name: /^import/i })).toBeNull();
    expect(within(palette).queryByRole("option", { name: /^dispatch/i })).toBeNull();

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: /command palette/i })).toBeNull();
    });
  });
});
