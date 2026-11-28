/**
 * Worker trust in the Web UI: a pending Worker cannot schedule, a changed report
 * stays under review until the exact revision is applied, and the Security &
 * Trust settings change one policy without touching the other.
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

const SLOT = (capability: string) => ({
  slotId: "mac-default",
  enabled: true,
  advisoryCapacity: null,
  roles: ["coding"],
  capabilities: [{ name: capability, state: "available" as const }],
  executors: [{ id: "claude-code", kind: "claude" as const }],
});

function appliedConfig(capability: string): WorkerSchedulingConfig {
  return { slots: [SLOT(capability)] };
}

function pendingWorker(): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    approval: "pending",
    connectionEpoch: 4,
    lastSeenAtMs: Date.now(),
    activeDeliveries: 0,
    host: { os: "darwin", arch: "arm64", deploymentMode: "native" },
    reportedRevision: 1,
    appliedRevision: null,
    appliedConfig: null,
  };
}

function reviewWorker(): WorkerRecord {
  return {
    ...pendingWorker(),
    approval: "approved",
    reportedRevision: 3,
    appliedRevision: 2,
    appliedConfig: appliedConfig("rust"),
  };
}

function snapshot(workers: WorkerRecord[]): GlobalSnapshot {
  return {
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 1,
    eventCursor: 1,
    tasksTruncated: false,
    totalTasks: 1,
    workers,
    tasks: [taskSummary({ id: "t1", title: "Work", status: "queued" })],
  };
}

async function openWorkers(): Promise<HTMLElement> {
  fireEvent.click(within(appBar()).getByRole("button", { name: /workers/i }));
  return screen.findByRole("dialog", { name: /workers and pairing/i });
}

beforeEach(installWorkspaceHub);
afterEach(resetWorkspaceTest);

describe("Worker trust", () => {
  let worker: WorkerRecord;
  let reported: WorkerSchedulingConfig | null;
  let policies: { requireWorkerApproval: boolean; requireConfigurationApply: boolean };
  let calls: { method: string; path: string; body: unknown }[];

  beforeEach(() => {
    worker = pendingWorker();
    reported = null;
    policies = { requireWorkerApproval: true, requireConfigurationApply: true };
    calls = [];
    stubFetch((url, init) => {
      const path = url.replace(/^https?:\/\/[^/]+/, "");
      const method = init.method ?? "GET";
      const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push({ method, path, body });
      if (path.startsWith("/v1/events")) return idleEventStream();
      if (path.startsWith("/v1/snapshot")) return jsonResponse(snapshot([worker]));
      if (path.startsWith("/v1/settings/security-trust")) {
        if (method === "PATCH") {
          policies = { ...policies, ...body };
        }
        return jsonResponse(policies);
      }
      if (path.endsWith("/config/apply") && method === "POST") {
        worker = { ...worker, appliedRevision: body.revision };
        return jsonResponse({ appliedRevision: body.revision });
      }
      if (path.endsWith("/approve") && method === "POST") {
        worker = { ...worker, approval: "approved" };
        return new Response(null, { status: 204 });
      }
      if (path.startsWith("/v1/workers/")) {
        return jsonResponse({
          worker,
          reportedConfig: reported,
          claimableTaskIds: [],
        });
      }
      if (path.startsWith("/v1/workers")) {
        return jsonResponse({ workers: [worker] });
      }
      return jsonResponse({ error: "NotFound", message: "Not found." }, 404);
    });
  });

  it("shows a pending Worker as awaiting approval and approves it", async () => {
    await connectWorkspace();
    const drawer = await openWorkers();
    const card = drawer.querySelector<HTMLElement>('[data-worker-id="wkr_00000000000a"]');
    expect(card).not.toBeNull();
    expect(card?.querySelector(".status-badge")?.textContent ?? "").toMatch(
      /awaiting approval/i,
    );

    fireEvent.click(within(drawer).getByRole("button", { name: /approve wkr_00000000000a/i }));
    await waitFor(() => {
      const approve = calls.find((call) => call.path.endsWith("/approve"));
      expect(approve).toBeDefined();
      expect(approve?.method).toBe("POST");
    });
    await waitFor(() =>
      expect(card?.querySelector(".status-badge")?.textContent).toBe("Online"),
    );
  });

  it("applies exactly the revision under review after the Admin inspects it", async () => {
    worker = reviewWorker();
    reported = appliedConfig("gpu");
    await connectWorkspace();
    const drawer = await openWorkers();

    const review = drawer.querySelector<HTMLElement>(".config-review");
    expect(review).not.toBeNull();
    // The reported side is labeled with the revision of the body it loaded.
    await waitFor(() => expect(review?.textContent ?? "").toMatch(/Reported rev 3/));
    expect(review?.textContent ?? "").toMatch(/Applied rev 2/);
    await waitFor(() =>
      expect(review?.textContent ?? "").toMatch(/changed Slot mac-default/),
    );
    // The report under review is loaded automatically, so Apply targets its exact revision.
    const apply = await within(drawer).findByRole("button", { name: /apply revision 3/i });
    fireEvent.click(apply);
    await waitFor(() => {
      const posted = calls.find((call) => call.path.endsWith("/config/apply"));
      expect(posted?.body).toEqual({ revision: 3 });
    });
    // The review never turns into an approval of the newer report.
    expect(calls.some((call) => call.path.endsWith("/approve"))).toBe(false);
  });

  it("toggles one Security & Trust policy and leaves the other stored", async () => {
    await connectWorkspace();
    fireEvent.click(
      within(appBar()).getByRole("button", { name: /settings and theme/i }),
    );
    const settings = await screen.findByRole("menu", { name: /settings/i });
    const approval = within(settings).getByRole("menuitemcheckbox", {
      name: /require worker approval/i,
    });
    expect(approval).toHaveAttribute("aria-checked", "true");
    fireEvent.click(approval);
    await waitFor(() => {
      const patch = calls.find(
        (call) => call.method === "PATCH" && call.path === "/v1/settings/security-trust",
      );
      expect(patch?.body).toEqual({ requireWorkerApproval: false });
    });
    await waitFor(() =>
      expect(
        within(settings).getByRole("menuitemcheckbox", { name: /require worker approval/i }),
      ).toHaveAttribute("aria-checked", "false"),
    );
    expect(
      within(settings).getByRole("menuitemcheckbox", { name: /require config apply/i }),
    ).toHaveAttribute("aria-checked", "true");
  });
});
