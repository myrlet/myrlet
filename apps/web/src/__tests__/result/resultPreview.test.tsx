/**
 * Result preview in the Task Detail Overview: placement after the metadata, a
 * short Result shown in full, and a long one measured to about six lines with
 * the clipped remainder inert.
 */

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  LONG_RESULT,
  SHORT_RESULT,
  connectAndSelectTask,
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

describe("Task Result preview", () => {
  beforeEach(stubPreviewLayout);

  it("places Result after the Overview metadata and shows a short result in full", async () => {
    stubHub(SHORT_RESULT);
    await connectAndSelectTask();

    const pane = document.querySelector(".detail-pane") as HTMLElement;
    await waitFor(() => {
      expect(pane.querySelector(".result-preview .result-markdown")).not.toBeNull();
    });
    const meta = pane.querySelector(".meta-grid-2x2") as HTMLElement;
    const result = pane.querySelector(".result-section") as HTMLElement;
    expect(meta.compareDocumentPosition(result) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    expect(within(result).getByRole("heading", { name: "Result" })).toBeInTheDocument();
    expect(result.textContent ?? "").toContain("Synced");
    expect(result.textContent ?? "").toContain("All checks passed.");
    expect(within(result).getByRole("link", { name: /ci run/i })).toBeInTheDocument();

    // A short result needs no reader affordance at all.
    expect(within(result).queryByRole("button", { name: /read full result/i })).toBeNull();
  });

  it("measures a long result to about six lines and makes everything below inert", async () => {
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const result = document.querySelector(".result-section") as HTMLElement;
    await waitFor(() => {
      expect(result.querySelector(".result-preview .result-markdown")).not.toBeNull();
      expect((result.querySelector(".result-preview-clip") as HTMLElement).style.maxHeight).toBe(
        "120px",
      );
    });
    const clip = result.querySelector(".result-preview-clip") as HTMLElement;
    expect(clip.style.maxHeight).toBe("120px");

    const preview = result.querySelector(".result-preview .result-markdown") as HTMLElement;
    // The whole document is rendered; only the tail is hidden and inert.
    expect(preview.textContent ?? "").toContain("Long trailing paragraph");
    const hiddenBlocks = Array.from(preview.children).filter(
      (child) => child.getAttribute("data-result-clipped") !== null,
    );
    expect(hiddenBlocks.length).toBeGreaterThan(0);
    for (const block of hiddenBlocks) {
      expect(block).toHaveAttribute("inert");
    }
    // The link in the hidden remainder is inert, so it cannot take focus.
    const hiddenLink = preview.querySelector("a") as HTMLElement;
    expect(hiddenLink.closest("[data-result-clipped]")).not.toBeNull();
    expect(hiddenLink.closest("[inert]")).not.toBeNull();

    const readFull = within(result).getByRole("button", { name: /read full result/i });
    expect(readFull).toBeInTheDocument();
  });

  it("copies the complete original Markdown from the Overview, not the preview", async () => {
    const writeText = stubClipboard();
    stubHub(LONG_RESULT);
    await connectAndSelectTask();

    const result = document.querySelector(".result-section") as HTMLElement;
    fireEvent.click(within(result).getByRole("button", { name: /copy markdown/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(LONG_RESULT);
    });
  });
});
