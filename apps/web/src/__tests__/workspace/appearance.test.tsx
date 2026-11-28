/**
 * Appearance: System follows the OS by default, an explicit choice persists,
 * and only `myrlet.theme` may be written.
 */

import "@testing-library/jest-dom/vitest";

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import App from "../../App";
import { stubColorScheme } from "../support/appearance";
import {
  connectWorkspace,
  installWorkspaceHub,
  localStorageKeys,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Appearance selection", () => {
  it("resolves System and follows a same-mount prefers-color-scheme change", async () => {
    const colorScheme = stubColorScheme(true);
    render(<App />);

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();

    act(() => colorScheme.setDark(false));
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("light");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();
  });

  it("switches System | Light | Dark from Settings and persists only myrlet.theme", async () => {
    await connectWorkspace();

    // System is the default: no explicit choice has been persisted yet.
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(window.localStorage.getItem("myrlet.theme")).toBeNull();

    const settingsButton = screen.getByRole("button", { name: /settings and theme/i });
    expect(settingsButton).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(settingsButton);

    const menu = await screen.findByRole("menu", { name: /settings/i });
    expect(within(menu).getByText("APPEARANCE")).toBeInTheDocument();
    const segment = within(menu).getByRole("group", { name: /appearance/i });
    const system = within(segment).getByRole("menuitemradio", { name: /system/i });
    const light = within(segment).getByRole("menuitemradio", { name: /light/i });
    const dark = within(segment).getByRole("menuitemradio", { name: /dark/i });
    expect(system).toHaveAttribute("aria-checked", "true");
    expect(light).toHaveAttribute("aria-checked", "false");
    expect(dark).toHaveAttribute("aria-checked", "false");

    fireEvent.click(dark);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("dark");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("dark");
    expect(dark).toHaveAttribute("aria-checked", "true");

    fireEvent.click(light);
    await waitFor(() => {
      expect(document.documentElement.dataset.theme).toBe("light");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("light");

    fireEvent.click(system);
    await waitFor(() => {
      expect(system).toHaveAttribute("aria-checked", "true");
    });
    expect(window.localStorage.getItem("myrlet.theme")).toBe("system");

    expect(localStorageKeys()).toEqual(["myrlet.theme"]);
    expect(window.localStorage.getItem("myrlet.token")).toBeNull();
  });
});
