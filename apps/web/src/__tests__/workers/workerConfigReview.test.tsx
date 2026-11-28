/**
 * Applying a reviewed configuration pins the exact revision the Admin is looking at.
 *
 * The reviewed body, its revision, and the applied side all come from one Worker Detail
 * read, so a Worker that keeps reporting while an Admin reviews can never cause Apply to
 * submit a body under a different revision. The Global Snapshot carries no revisions and
 * is deliberately not consulted: a newer report in the Snapshot does not invalidate or
 * control an active review. Correctness against a stale Apply is the Hub's atomic check.
 */

import "@testing-library/jest-dom/vitest";

import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type {
  GlobalSnapshot,
  WorkerDetail,
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
    permissionTier: "worker",
    connectionEpoch: 4,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 0,
    host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
    configurationPending: true,
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
  // What the next `GET /v1/workers/{id}` returns.
  let detail: WorkerDetail;
  let applies: { revision: number }[];
  let applyStatus: number;

  beforeEach(() => {
    record = worker();
    detail = {
      worker: record,
      reportedRevision: 3,
      appliedRevision: 2,
      appliedConfig: config("rust"),
      reportedConfig: config("gpu"),
      claimableTaskIds: [],
    };
    applies = [];
    applyStatus = 200;
    stubFetch((url, init) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      const method = init.method ?? "GET";
      const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
      if (path.startsWith("/v1/events")) return idleEventStream();
      // The Snapshot reflects the summary only; it never carries revisions.
      if (path.startsWith("/v1/snapshot")) return jsonResponse(snapshot(record));
      if (path.startsWith("/v1/settings/security-trust")) {
        return jsonResponse({
          requireWorkerApproval: true,
          requireConfigurationApply: true,
        });
      }
      if (path.endsWith("/config/apply") && method === "POST") {
        if (applyStatus !== 200) {
          return jsonResponse(
            { error: "ConfigRevisionMismatch", message: "The report changed since you looked." },
            applyStatus,
          );
        }
        applies.push({ revision: body.revision });
        return jsonResponse({ appliedRevision: body.revision });
      }
      if (path.startsWith("/v1/workers/")) {
        return jsonResponse(detail);
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
    expect(review?.textContent ?? "").toMatch(/Applied rev 2/);

    const apply = await within(drawer).findByRole("button", { name: /apply revision 3/i });
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 3 }]));
  });

  it("does not let a newer Snapshot invalidate an active review", async () => {
    await connectWorkspace();
    const drawer = await openWorkers();
    await waitFor(() =>
      expect(within(drawer).getByRole("button", { name: /apply revision 3/i })).toBeInTheDocument(),
    );

    // A new report appears in the Snapshot; it must not disturb the loaded review, whose
    // revision and body came from the Detail read the Admin saw.
    record = worker();
    fireEvent.click(within(drawer).getByRole("button", { name: /^refresh$/i }));

    // The review is still revision 3 and Apply still submits the displayed body.
    const apply = within(drawer).getByRole("button", { name: /apply revision 3/i });
    expect(apply).toBeInTheDocument();
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 3 }]));
  });

  it("reports a stale Apply and reloads the newer revision on demand", async () => {
    await connectWorkspace();
    const drawer = await openWorkers();
    const review = drawer.querySelector<HTMLElement>(".config-review");
    await waitFor(() =>
      expect(within(drawer).getByRole("button", { name: /apply revision 3/i })).toBeInTheDocument(),
    );

    // The Hub rejects the Apply because a newer report arrived after the review loaded.
    applyStatus = 409;
    fireEvent.click(within(drawer).getByRole("button", { name: /apply revision 3/i }));
    await screen.findByText(/report changed since you looked/i);

    // The Admin reloads explicitly; the newer Detail response updates revision and body
    // together, and a fresh Apply submits the new revision.
    applyStatus = 200;
    detail = {
      ...detail,
      reportedRevision: 4,
      reportedConfig: config("tensor"),
    };
    fireEvent.click(within(drawer).getByRole("button", { name: /reload report/i }));
    await waitFor(() => expect(review?.textContent ?? "").toMatch(/Reported rev 4/));
    const apply = await within(drawer).findByRole("button", { name: /apply revision 4/i });
    fireEvent.click(apply);
    await waitFor(() => expect(applies).toEqual([{ revision: 4 }]));
  });

  it("leaves Apply unavailable when the Detail read fails", async () => {
    // The Snapshot says configuration is pending, so the card tries to load the review,
    // but the Detail read fails. No reviewed pair exists, so Apply stays disabled.
    await connectWorkspace();
    record = worker();
    stubFetch((url) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      if (path.startsWith("/v1/events")) return idleEventStream();
      if (path.startsWith("/v1/snapshot")) return jsonResponse(snapshot(record));
      if (path.startsWith("/v1/settings/security-trust")) {
        return jsonResponse({
          requireWorkerApproval: true,
          requireConfigurationApply: true,
        });
      }
      if (path.startsWith("/v1/workers")) {
        // The Detail read (and the summary list) carry no report; the Detail read is the
        // one the review depends on, and it fails.
        return path.startsWith("/v1/workers/")
          ? jsonResponse({ error: "Unavailable", message: "The Hub is restarting." }, 503)
          : jsonResponse({ workers: [record] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
    const drawer = await openWorkers();
    // The failure surfaces, and the Apply control is present but disabled (no revision).
    await waitFor(() => expect(drawer.textContent ?? "").toMatch(/restarting/i));
    const apply = within(drawer).getByRole("button", { name: /apply revision/i });
    expect(apply).toBeDisabled();
  });
});
