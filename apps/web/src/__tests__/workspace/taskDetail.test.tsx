/**
 * Task Detail reads: a failed detail read keeps the Snapshot facts and offers
 * retry or dismiss, and a stale response never overwrites the current Task.
 */

import "@testing-library/jest-dom/vitest";

import { act, fireEvent, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { buildSnapshot, taskRecord, trustSettings, workerRecord } from "../support/fixtures";
import { idleEventStream, jsonResponse, stubFetch } from "../support/hub";
import {
  connectWorkspace,
  installWorkspaceHub,
  openList,
  resetWorkspaceTest,
} from "../support/workspace";

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Task Detail reads", () => {
  it("keeps the snapshot Task name and status when the detail read fails, then retries or dismisses it", async () => {
    let detailAttempts = 0;
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
      const id = match === null ? "t3" : decodeURIComponent(match[1]);
      const summary = buildSnapshot().tasks.find((task) => task.id === id);
      if (summary === undefined) {
        return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
      }
      if (match !== null && match[2] !== undefined) {
        return jsonResponse({
          rootTaskId: summary.rootTaskId,
          tasks: [taskRecord(summary)],
          related: [],
          truncated: false,
        });
      }
      detailAttempts += 1;
      if (detailAttempts < 3) {
        return jsonResponse(
          { error: "Internal", message: "The Hub returned status 500." },
          500,
        );
      }
      return jsonResponse({ task: taskRecord(summary), summary: null, related: [] });
    });

    await connectWorkspace();
    openList();
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);

    const detailPane = (): HTMLElement =>
      document.querySelector(".detail-pane") as HTMLElement;

    // A failed detail read keeps the Snapshot name/status and states the failure
    // inside the panel instead of claiming that nothing is selected.
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).not.toBeNull();
    });
    expect(detailPane().querySelector(".detail-title")?.textContent).toBe("Run tests");
    expect(detailPane().textContent ?? "").toContain("Queued");
    expect(detailPane().textContent ?? "").not.toContain("No Task selected");
    // No fabricated detail from the Snapshot summary.
    expect(detailPane().textContent ?? "").not.toContain("Work on Run tests.");

    // Dismissing closes only the failure notice; the selection stays.
    fireEvent.click(
      within(detailPane()).getByRole("button", { name: /dismiss load failure/i }),
    );
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).toBeNull();
    });
    expect(detailPane().querySelector(".detail-title")?.textContent).toBe("Run tests");

    // Selecting the same Task again fails once more, then Retry reloads it.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).not.toBeNull();
    });
    fireEvent.click(within(detailPane()).getByRole("button", { name: /retry/i }));
    await waitFor(() => {
      expect(detailPane().querySelector(".detail-error")).toBeNull();
      expect(detailPane().textContent ?? "").toContain("Work on Run tests.");
    });
    expect(detailAttempts).toBe(3);
  });

  it("ignores a stale detail response after switching Tasks", async () => {
    let releaseT3: () => void = () => undefined;
    const t3Gate = new Promise<void>((resolve) => {
      releaseT3 = resolve;
    });

    stubFetch(async (url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) {
        return idleEventStream();
      }
      if (path.startsWith("/v1/snapshot")) {
        return jsonResponse(buildSnapshot());
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
          related: [],
          truncated: false,
        });
      }
      if (id === "t3") {
        await t3Gate;
      }
      return jsonResponse({ task: taskRecord(summary), summary: null, related: [] });
    });

    await connectWorkspace();
    openList();
    fireEvent.click(document.querySelector('.list-row[data-task-id="t3"]') as HTMLElement);

    // Loading is distinct from "nothing selected": the header already names the
    // pending Task from the Snapshot.
    await waitFor(() => {
      const pane = document.querySelector(".detail-pane") as HTMLElement;
      expect(pane.querySelector(".detail-title")?.textContent).toBe("Run tests");
      expect(pane.textContent ?? "").toContain("Loading");
    });
    expect(document.querySelector(".detail-pane")?.textContent ?? "").not.toContain(
      "No Task selected",
    );

    // Switch to another Task before the first response arrives.
    fireEvent.click(document.querySelector('.list-row[data-task-id="t1"]') as HTMLElement);
    await waitFor(() => {
      const pane = document.querySelector(".detail-pane") as HTMLElement;
      expect(pane.querySelector(".detail-title")?.textContent).toBe("Ship release");
      expect(pane.textContent ?? "").toContain("Work on Ship release.");
    });

    // The late response for the abandoned selection must not overwrite t1.
    releaseT3();
    await act(async () => {
      await t3Gate;
      await Promise.resolve();
    });
    const pane = document.querySelector(".detail-pane") as HTMLElement;
    expect(pane.querySelector(".detail-title")?.textContent).toBe("Ship release");
    expect(pane.textContent ?? "").toContain("Work on Ship release.");
    expect(pane.textContent ?? "").not.toContain("Work on Run tests.");
  });
});
