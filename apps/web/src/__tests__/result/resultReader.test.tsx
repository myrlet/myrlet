/**
 * Result reader: a native modal dialog with the full Markdown, Escape closing
 * only the reader, focus and scroll restoration, and a late SSE arrival never
 * changing tab, opening the reader, or stealing focus.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  LONG_RESULT,
  connectAndSelectTask,
  detailBody,
  stubClipboard,
  stubHub,
  stubPreviewLayout,
} from "../support/resultHub";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe("Result reader", () => {
  beforeEach(stubPreviewLayout);

  async function openReader(): Promise<{ dialog: HTMLElement; trigger: HTMLElement }> {
    const section = document.querySelector(".result-section") as HTMLElement;
    const trigger = await within(section).findByRole("button", { name: /read full result/i });
    fireEvent.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: /result — ship release/i });
    return { dialog, trigger };
  }

  it("opens a native dialog with the task name and full Markdown, and copies the full text", async () => {
    const writeText = stubClipboard();
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const dialog = (await openReader()).dialog;
    expect(dialog.tagName).toBe("DIALOG");
    expect(dialog).toHaveClass("result-reader");
    expect(dialog).toHaveAttribute("open");
    expect(within(dialog).getByRole("heading", { name: "Ship release" })).toBeInTheDocument();

    await waitFor(() => {
      expect(dialog.querySelector(".result-markdown")).not.toBeNull();
    });
    const full = dialog.querySelector(".result-markdown") as HTMLElement;
    expect(full.textContent ?? "").toContain("Long trailing paragraph");
    expect(within(full).getByRole("link", { name: /the hidden pr link/i })).toBeInTheDocument();
    // Nothing is clipped inside the reader.
    expect(full.querySelector("[inert]")).toBeNull();

    // Tables and fenced code scroll inside their own containers.
    expect(full.querySelector(".result-table-scroll table")).not.toBeNull();
    expect(full.querySelector("pre code")?.textContent).toContain("cargo test");

    fireEvent.click(within(dialog).getByRole("button", { name: /copy markdown/i }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(LONG_RESULT);
    });
  });

  it("Escape closes only the reader, then restores focus, tab, task, and Overview scroll", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const body = detailBody();
    body.scrollTop = 180;
    const { dialog, trigger } = await openReader();
    await waitFor(() => {
      expect(document.activeElement).toBe(
        within(dialog).getByRole("button", { name: /close result reader/i }),
      );
    });

    // Pretend layout shifted while the reader was open.
    body.scrollTop = 0;
    fireEvent.keyDown(dialog, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    // Escape must not fall through and clear the Task selection.
    expect(document.querySelector(".detail-pane")).not.toBeNull();
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await waitFor(() => {
      expect(document.activeElement).toBe(trigger);
    });
    expect(body.scrollTop).toBe(180);
  });

  it("closes from its own close button and returns focus to the trigger", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const { dialog, trigger } = await openReader();
    fireEvent.click(within(dialog).getByRole("button", { name: /close result reader/i }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
    await waitFor(() => {
      expect(document.activeElement).toBe(trigger);
    });
  });

  it("keeps the reading position, tab, and focus when a late SSE frame adds a Result", async () => {
    const hub = stubHub(null);
    await connectAndSelectTask();

    // No Result yet: wait for the Task heading, not for a preview.
    expect(document.querySelector(".detail-pane .result-section")).toBeNull();
    const body = detailBody();
    body.scrollTop = 240;
    const overviewTab = screen.getByRole("tab", { name: "Overview" });
    overviewTab.focus();
    expect(document.activeElement).toBe(overviewTab);

    hub.setResult(LONG_RESULT);
    hub.events.push(13);

    await waitFor(() => {
      expect(document.querySelector(".detail-pane .result-section")).not.toBeNull();
    });

    // Late facts arrive in place: same tab, no reader, no focus or scroll change.
    expect(screen.getByRole("tab", { name: "Overview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(overviewTab);
    expect(detailBody().scrollTop).toBe(240);
  });
});
