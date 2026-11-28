/**
 * Worker inventory predicates and local credential-action lifecycle: pure
 * filters plus the `useWorkerActions` hook, which owns the armed confirmation,
 * the in-flight flag, honest error wording, and unmount safety.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HubApiError } from "../../api/client";
import type { RotateWorkerResponse, WorkerRecord } from "../../api/types";
import { describeError, describeIssueError } from "../../components/workers/workerErrors";
import { useWorkerActions } from "../../components/workers/useWorkerActions";
import {
  applyPending,
  workerCounts,
  workerMatchesQuery,
  workerState,
} from "../../components/workers/workerFilters";

function worker(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    approval: "approved",
    connectionEpoch: 4,
    lastSeenAtMs: 1_700_000_000_000,
    activeDeliveries: 2,
    host: { os: "linux", arch: "x86_64", deploymentMode: "container" },
    reportedRevision: 2,
    appliedRevision: 2,
    appliedConfig: {
      slots: [
        {
          slotId: "gpu-1",
          enabled: true,
          advisoryCapacity: null,
          roles: ["build"],
          capabilities: [{ name: "cuda", state: "available" }],
          executors: [{ id: "codex-cli", kind: "codex" }],
        },
      ],
    },
    ...overrides,
  };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("worker inventory predicates", () => {
  it("derives revoked before approval, and approval before connection state", () => {
    expect(workerState(worker())).toBe("approved");
    expect(workerState(worker({ online: false }))).toBe("offline");
    expect(workerState(worker({ approval: "pending" }))).toBe("pending");
    expect(workerState(worker({ approval: "pending", online: false }))).toBe("pending");
    expect(workerState(worker({ online: true, revoked: true }))).toBe("revoked");
    expect(workerState(worker({ online: false, revoked: true }))).toBe("revoked");
  });

  it("counts revoked Workers as registered but neither online, offline, nor pending", () => {
    const counts = workerCounts([
      worker({ workerId: "w1" }),
      worker({ workerId: "w2", online: false }),
      worker({ workerId: "w3", revoked: true }),
      worker({ workerId: "w4", approval: "pending" }),
    ]);
    expect(counts).toEqual({
      registered: 4,
      online: 1,
      offline: 1,
      pending: 1,
      activeDeliveries: 8,
    });
  });

  it("only treats an unapplied report as pending Apply", () => {
    expect(applyPending(worker())).toBe(false);
    expect(applyPending(worker({ reportedRevision: 3 }))).toBe(true);
    expect(applyPending(worker({ reportedRevision: null, appliedRevision: null }))).toBe(false);
  });

  it("matches the Worker ID, host, Slot, role, capability, and executor", () => {
    const record = worker();
    for (const query of ["00000000000a", "linux", "container", "gpu-1", "build", "cuda", "codex"]) {
      expect(workerMatchesQuery(record, query)).toBe(true);
    }
    expect(workerMatchesQuery(record, "windows")).toBe(false);
    expect(workerMatchesQuery(record, "")).toBe(true);
    expect(workerMatchesQuery(worker({ host: null, appliedConfig: null }), "gpu")).toBe(false);
  });
});

describe("worker error wording", () => {
  it("keeps the Hub message and prefixes the HTTP status only for issuance", () => {
    const rejected = new HubApiError(409, "Conflict", "That code is already spent.");
    expect(describeIssueError(rejected)).toBe("HTTP 409: That code is already spent.");
    expect(describeError(rejected)).toBe("That code is already spent.");
    expect(describeIssueError(new Error("offline"))).toBe("offline");
    expect(describeError("not an error")).toBe("The Hub command could not be completed.");
  });
});

describe("useWorkerActions", () => {
  function setup(
    rotate: (workerId: string) => Promise<RotateWorkerResponse> = async () => ({
      hubId: "hub_test",
      workerId: "wkr_00000000000a",
      credential: "cred-once",
    }),
    revoke: (workerId: string) => Promise<void> = async () => undefined,
    options: {
      readonly reportedRevision?: number | null;
      readonly onLoadReported?: (workerId: string) => Promise<void>;
      readonly onApproveWorker?: (workerId: string) => Promise<void>;
      readonly onApplyWorkerConfig?: (
        workerId: string,
        revision: number,
      ) => Promise<void>;
    } = {},
  ) {
    const onRotateWorker = vi.fn(rotate);
    const onRevokeWorker = vi.fn(revoke);
    const onLoadReported = vi.fn(options.onLoadReported ?? (async () => undefined));
    const onApproveWorker = vi.fn(options.onApproveWorker ?? (async () => undefined));
    const onApplyWorkerConfig = vi.fn(
      options.onApplyWorkerConfig ?? (async () => undefined),
    );
    const hook = renderHook(() =>
      useWorkerActions({
        workerId: "wkr_00000000000a",
        reportedRevision:
          options.reportedRevision === undefined ? 2 : options.reportedRevision,
        onLoadReported,
        onApproveWorker,
        onApplyWorkerConfig,
        onRotateWorker,
        onRevokeWorker,
      }),
    );
    return {
      onRotateWorker,
      onRevokeWorker,
      onLoadReported,
      onApproveWorker,
      onApplyWorkerConfig,
      ...hook,
    };
  }

  it("arms a confirmation without calling the Hub, then rotates and keeps the once-shown credential", async () => {
    const { result, onRotateWorker } = setup();
    expect(result.current.pending).toBeNull();

    act(() => result.current.request("rotate"));
    expect(result.current.pending).toBe("rotate");
    expect(onRotateWorker).not.toHaveBeenCalled();

    await act(async () => result.current.rotate());
    expect(onRotateWorker).toHaveBeenCalledWith("wkr_00000000000a");
    expect(result.current.pending).toBeNull();
    expect(result.current.busy).toBe(false);
    expect(result.current.rotatedCredential).toBe("cred-once");
  });

  it("cancels an armed confirmation without a request", () => {
    const { result, onRevokeWorker } = setup();
    act(() => result.current.request("revoke"));
    act(() => result.current.cancel());
    expect(result.current.pending).toBeNull();
    expect(onRevokeWorker).not.toHaveBeenCalled();
  });

  it("keeps the confirmation armed and reports the honest failure when revoke fails", async () => {
    const failure = new HubApiError(500, "Internal", "The Hub returned status 500.");
    const { result } = setup(undefined, async () => {
      throw failure;
    });
    act(() => result.current.request("revoke"));
    await act(async () => result.current.revoke());

    expect(result.current.pending).toBe("revoke");
    expect(result.current.error).toBe("The Hub returned status 500.");
    expect(result.current.busy).toBe(false);
    expect(result.current.rotatedCredential).toBeNull();
  });

  it("clears a previously shown credential once revoke succeeds", async () => {
    const { result } = setup();
    act(() => result.current.request("rotate"));
    await act(async () => result.current.rotate());
    expect(result.current.rotatedCredential).toBe("cred-once");

    act(() => result.current.request("revoke"));
    await act(async () => result.current.revoke());
    expect(result.current.rotatedCredential).toBeNull();
    expect(result.current.pending).toBeNull();
  });

  it("drops a response that lands after the card unmounts", async () => {
    let release!: () => void;
    const gate = new Promise<RotateWorkerResponse>((resolve) => {
      release = () => resolve({
        hubId: "hub_test",
        workerId: "wkr_00000000000a",
        credential: "late",
      });
    });
    const { result, unmount } = setup(() => gate);
    act(() => result.current.request("rotate"));
    let rotation!: Promise<void>;
    act(() => {
      rotation = result.current.rotate();
    });
    unmount();
    await act(async () => {
      release();
      await rotation;
    });
    // No state update after unmount: React would warn and the credential would
    // be lost with the card anyway.
    expect(result.current.rotatedCredential).toBeNull();
  });

  it("arms Apply only after the report under review loads, and pins its revision", async () => {
    let releaseReport!: () => void;
    const reportGate = new Promise<void>((resolve) => {
      releaseReport = resolve;
    });
    const applied: { workerId: string; revision: number }[] = [];
    const { result, onLoadReported, onApplyWorkerConfig } = setup(undefined, undefined, {
      reportedRevision: 3,
      onLoadReported: () => reportGate,
      onApplyWorkerConfig: async (workerId, revision) => {
        applied.push({ workerId, revision });
      },
    });

    let reviewing!: Promise<void>;
    act(() => {
      reviewing = result.current.review();
    });
    expect(onLoadReported).toHaveBeenCalledWith("wkr_00000000000a");
    expect(result.current.pending).toBeNull();
    expect(onApplyWorkerConfig).not.toHaveBeenCalled();

    await act(async () => {
      releaseReport();
      await reviewing;
    });
    expect(result.current.pending).toBe("apply");
    expect(result.current.reviewedRevision).toBe(3);

    await act(async () => result.current.apply());
    expect(onApplyWorkerConfig).toHaveBeenCalledWith("wkr_00000000000a", 3);
    expect(applied).toEqual([{ workerId: "wkr_00000000000a", revision: 3 }]);
    expect(result.current.pending).toBeNull();
    expect(result.current.reviewedRevision).toBeNull();
  });

  it("keeps the reviewed revision visible when Apply is refused", async () => {
    const { result } = setup(undefined, undefined, {
      reportedRevision: 3,
      onApplyWorkerConfig: async () => {
        throw new HubApiError(409, "Conflict", "The revision changed.");
      },
    });
    await act(async () => result.current.review());
    expect(result.current.reviewedRevision).toBe(3);

    await act(async () => result.current.apply());
    expect(result.current.error).toBe("The revision changed.");
    expect(result.current.pending).toBe("apply");
    expect(result.current.reviewedRevision).toBe(3);
  });

  it("never arms Apply without a reported revision", async () => {
    const { result, onApplyWorkerConfig } = setup(undefined, undefined, {
      reportedRevision: null,
    });
    await act(async () => result.current.review());
    expect(result.current.pending).toBeNull();
    expect(result.current.reviewedRevision).toBeNull();
    await act(async () => result.current.apply());
    expect(onApplyWorkerConfig).not.toHaveBeenCalled();
  });

  it("approves through the shared busy and error state", async () => {
    const { result, onApproveWorker } = setup(undefined, undefined, {
      onApproveWorker: async () => {
        throw new HubApiError(503, "Unavailable", "The Hub is restarting.");
      },
    });
    await act(async () => result.current.approve());
    expect(onApproveWorker).toHaveBeenCalledWith("wkr_00000000000a");
    expect(result.current.error).toBe("The Hub is restarting.");
    expect(result.current.busy).toBe(false);
  });
});
