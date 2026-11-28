/**
 * Applying a reviewed configuration pins the exact revision the Admin is looking at.
 *
 * The reviewed body and the revision submitted to Apply come from the same read, so a
 * Worker that keeps reporting while an Admin reviews can never cause Apply to submit a
 * body under a different revision. When the report moves on, the stale review is dropped
 * and the Admin reloads the latest body before applying.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type {
  GlobalSnapshot,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../../api/types";
import { taskSummary } from "../support/fixtures";
import { idleEventStream, jsonResponse, stubFetch } from "../support/hub";
import {
  appBar,
  connectWorkspace,
  installWorkspaceHub,
  resetWorkspaceTest,
} from "../support/workspace";

function slot(capability: string) {
  return {
    slotId: "mac-default",
    enabled: true,
    advisoryCapacity: null,
    roles: ["coding"],
    capabilities: [{ name: capability, state: "available" as const }],
    executors: [{ id: "claude-code", kind: "claude" as const }],
  };
}

function config(capability: string): WorkerSchedulingConfig {
  return { slots: [slot(capability)] };
}

function worker(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    approval: "approved",
    connectionEpoch: 4,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 0,
    host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
    reportedRevision: 3,
    appliedRevision: 2,
    appliedConfig: config("rust"),
    ...overrides,
  };
}

function snapshot(record: WorkerRecord): GlobalSnapshot {
  return {
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 1,
    eventCursor: 1,
    tasksTruncated: false,
    totalTasks: 1,
    workers: [record],
    tasks: [taskSummary({ id: "t1", title: "Work", status: "queued" })],
  };
}

async function openWorkers(): Promise<HTMLElement> {
  fireEvent.click(within(appBar()).getByRole("button", { name: /workers/i }));
  return screen.findByRole("dialog", { name: /workers and pairing/i });
}

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Reviewed configuration revision", () => {
  let record: WorkerRecord;
  let reportedConfig: WorkerSchedulingConfig | null;
  let applies: { revision: number }[];

  beforeEach(() => {
    record = worker();
    reportedConfig = config("gpu");
    applies = [];
    stubFetch((url, init) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      const method = init.method ?? "GET";
      const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
      if (path.startsWith("/v1/events")) return idleEventStream();
      if (path.startsWith("/v1/snapshot")) return jsonResponse(snapshot(record));
      if (path.startsWith("/v1/settings/security-trust")) {
        return jsonResponse({
          requireWorkerApproval: true,
          requireConfigurationApply: true,
        });
      }
      if (path.endsWith("/config/apply") && method === "POST") {
        applies.push({ revision: body.revision });
        return jsonResponse({ appliedRevision: body.revision });
      }
      if (path.startsWith("/v1/workers/")) {
        return jsonResponse({
          worker: record,
          reportedConfig,
          claimableTaskIds: [],
        });
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [record] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
  });

  it("labels the reviewed body with the revision it will submit", async () => {
    await connectWorkspace();
    const drawer = await openWorkers();

    const review = drawer.querySelector<HTMLElement>(".config-review");
    expect(review).not.toBeNull();
    await waitFor(() => expect(review?.textContent ?? "").toMatch(/Reported rev 3/));

    const apply = await within(drawer).findByRole("button", { name: /apply revision 3/i });
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 3 }]));
  });

  it("never pairs the Snapshot revision with a body a newer Detail read returned", async () => {
    // The Snapshot still reports revision 3, but the Detail read sees revision 4. The
    // reviewed pair must come entirely from the Detail response: 4 with the "tensor" body,
    // and Apply must submit 4 — not the Snapshot's stale 3.
    record = { ...record, reportedRevision: 3 };
    reportedConfig = config("tensor");
    stubFetch((url, init) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      const method = init.method ?? "GET";
      const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
      if (path.startsWith("/v1/events")) return idleEventStream();
      if (path.startsWith("/v1/snapshot")) {
        // The Snapshot lags one revision behind the Worker's latest report.
        return jsonResponse(snapshot(worker({ reportedRevision: 3 })));
      }
      if (path.startsWith("/v1/settings/security-trust")) {
        return jsonResponse({
          requireWorkerApproval: true,
          requireConfigurationApply: true,
        });
      }
      if (path.endsWith("/config/apply") && method === "POST") {
        applies.push({ revision: body.revision });
        return jsonResponse({ appliedRevision: body.revision });
      }
      if (path.startsWith("/v1/workers/")) {
        // The Detail read carries a newer revision than the Snapshot above.
        return jsonResponse({
          worker: worker({ reportedRevision: 4 }),
          reportedConfig: config("tensor"),
          claimableTaskIds: [],
        });
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [worker({ reportedRevision: 3 })] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
    await connectWorkspace();
    const drawer = await openWorkers();

    const review = drawer.querySelector<HTMLElement>(".config-review");
    // Revision and body are labeled from the same Detail response.
    await waitFor(() => expect(review?.textContent ?? "").toMatch(/Reported rev 4/));
    expect(review?.textContent ?? "").toMatch(/tensor/);
    const apply = await within(drawer).findByRole("button", { name: /apply revision 4/i });
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 4 }]));
  });

  it("reloads the newer report before applying, replacing both revision and body", async () => {
    await connectWorkspace();
    const drawer = await openWorkers();
    const review = drawer.querySelector<HTMLElement>(".config-review");
    await waitFor(() =>
      expect(within(drawer).getByRole("button", { name: /apply revision 3/i })).toBeInTheDocument(),
    );

    // The Worker reports revision 4 while the Admin is looking at revision 3. The
    // invalidating event drives a Snapshot refresh, which carries the new revision.
    record = { ...record, reportedRevision: 4 };
    reportedConfig = config("tensor");
    fireEvent.click(within(drawer).getByRole("button", { name: /^refresh$/i }));

    // The reviewed revision no longer matches the Snapshot, so its pair is dropped and
    // Apply is unavailable until the Admin reloads the newer report.
    await waitFor(() =>
      expect(within(drawer).queryByRole("button", { name: /apply revision 3/i })).toBeNull(),
    );
    fireEvent.click(within(drawer).getByRole("button", { name: /reload report/i }));

    // The reload binds revision 4 to its own body; the label and the submit agree.
    const apply = await within(drawer).findByRole("button", { name: /apply revision 4/i });
    await waitFor(() => expect(review?.textContent ?? "").toMatch(/Reported rev 4/));
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 4 }]));
  });
});
