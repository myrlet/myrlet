/**
 * Result rendering in Task Detail: sanitized Markdown, safe links, no raw HTML
 * or remote images, and the full reader behind a clipped Overview.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { buildSnapshot, taskRecord, trustSettings, workerRecord } from "../support/fixtures";
import { idleEventStream, jsonResponse, stubFetch } from "../support/hub";
import {
  connectWorkspace,
  installWorkspaceHub,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Result in Task Detail", () => {
  it("renders the Markdown result safely in Task Detail", async () => {
    const result =
      "# Findings\n\n- one\n- two\n\n| Item | State |\n| --- | --- |\n| Build | ok |\n\n" +
      "```sh\ncargo test\n```\n\nSee [the PR](https://example.test/pr/1).\n\n" +
      '<script>alert("x")</script>\n\n![chart](https://example.test/chart.png)\n';
    // jsdom has no layout; give the preview a deterministic box model so this
    // long Result is measurably clipped and offers the full reader.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ): DOMRect {
      if (this.classList.contains("result-markdown")) {
        return new DOMRect(0, 0, 480, this.children.length * 24);
      }
      if (this.parentElement?.classList.contains("result-markdown")) {
        const index = Array.from(this.parentElement.children).indexOf(this);
        return new DOMRect(0, index * 24, 480, 24);
      }
      return new DOMRect(0, 0, 0, 0);
    });
    stubFetch((url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return idleEventStream();
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse(buildSnapshot());
      }
      if (path.startsWith("/v1/settings/security-trust")) {
        return jsonResponse(trustSettings());
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [workerRecord()] });
      }
      const [pathname] = path.split("?");
      const match = /^\/v1\/tasks\/([^/]+)(\/tree)?$/.exec(pathname);
      const id = match === null ? "t1" : decodeURIComponent(match[1]);
      const summary = buildSnapshot().tasks.find((task) => task.id === id);
      if (summary === undefined) {
        return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
      }
      if (match !== null && match[2] !== undefined) {
        return jsonResponse({
          rootTaskId: summary.rootTaskId,
          tasks: [taskRecord(summary)],
          truncated: false,
        });
      }
      return jsonResponse({
        ...taskRecord(summary),
        resultMarkdown: result,
        resultRecordedAtMs: 1_700_000_000_600,
      });
    });

    await connectWorkspace();
    fireEvent.click(document.querySelector('.atlas-node[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(document.querySelector(".detail-pane .result-markdown")).not.toBeNull();
    });

    const pane = document.querySelector(".detail-pane") as HTMLElement;
    await waitFor(() => {
      expect(within(pane).getByRole("button", { name: /read full result/i })).toBeInTheDocument();
    });
    const markdown = pane.querySelector(".result-preview .result-markdown") as HTMLElement;
    expect(within(markdown).getByRole("heading", { name: "Findings" })).toBeInTheDocument();
    expect(markdown.querySelectorAll("li")).toHaveLength(2);
    expect(markdown.querySelector("table")).not.toBeNull();
    expect(pane.textContent ?? "").toMatch(/Synced|Recorded/);

    // The Overview clips this long result; the complete, sanitized document
    // lives in the reader behind `Read full result`.
    fireEvent.click(within(pane).getByRole("button", { name: /read full result/i }));
    const reader = await screen.findByRole("dialog");
    await waitFor(() => {
      expect(reader.querySelector(".result-markdown")).not.toBeNull();
    });
    const full = reader.querySelector(".result-markdown") as HTMLElement;
    expect(full.querySelector("pre code")?.textContent).toContain("cargo test");

    const link = within(full).getByRole("link", { name: /the pr/i });
    expect(link).toHaveAttribute("href", "https://example.test/pr/1");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link.getAttribute("rel") ?? "").toMatch(/noopener/);

    // No raw HTML, no remotely loaded image: markdown shows the alt text instead.
    expect(full.querySelector("script")).toBeNull();
    expect(full.querySelector("img")).toBeNull();
    expect(full.textContent ?? "").not.toContain("alert(\"x\")");
    expect(full.textContent ?? "").toContain("[image: chart]");
  });
});
