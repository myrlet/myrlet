/**
 * Worker inventory predicates and local credential-action lifecycle: pure
 * filters plus the `useWorkerActions` hook, which owns the armed confirmation,
 * the in-flight flag, honest error wording, and unmount safety.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HubApiError } from "../../api/client";
import type {
  RotateWorkerResponse,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../../api/types";
import { describeError, describeIssueError } from "../../components/workers/workerErrors";
import { useWorkerActions } from "../../components/workers/useWorkerActions";
import { useWorkerConfigReview } from "../../state/useWorkerConfigReview";
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
  function report(capability: string): WorkerSchedulingConfig {
    return {
      slots: [
        {
          slotId: "gpu-1",
          enabled: true,
          advisoryCapacity: null,
          roles: ["build"],
          capabilities: [{ name: capability, state: "available" }],
          executors: [{ id: "codex-cli", kind: "codex" }],
        },
      ],
    };
  }

  function setup(
    rotate: (workerId: string) => Promise<RotateWorkerResponse> = async () => ({
      hubId: "hub_test",
      workerId: "wkr_00000000000a",
      credential: "cred-once",
    }),
    revoke: (workerId: string) => Promise<void> = async () => undefined,
    options: {
      readonly reportedRevision?: number | null;
      readonly onFetchReported?: (
        workerId: string,
      ) => Promise<WorkerSchedulingConfig | null>;
      readonly onApproveWorker?: (workerId: string) => Promise<void>;
      readonly onApplyWorkerConfig?: (
        workerId: string,
        revision: number,
      ) => Promise<void>;
    } = {},
  ) {
    const onRotateWorker = vi.fn(rotate);
    const onRevokeWorker = vi.fn(revoke);
    const onFetchReported = vi.fn(
      options.onFetchReported ?? (async () => report("cuda")),
    );
    const onApproveWorker = vi.fn(options.onApproveWorker ?? (async () => undefined));
    const onApplyWorkerConfig = vi.fn(
      options.onApplyWorkerConfig ?? (async () => undefined),
    );
    const reportedRevision =
      options.reportedRevision === undefined ? 2 : options.reportedRevision;
    const hook = renderHook(() => {
      const review = useWorkerConfigReview({
        workerId: "wkr_00000000000a",
        reportedRevision,
        fetchReported: onFetchReported,
      });
      const actions = useWorkerActions({
        workerId: "wkr_00000000000a",
        reportedRevision,
        review,
        onApproveWorker,
        onApplyWorkerConfig,
        onRotateWorker,
        onRevokeWorker,
      });
      return { review, actions };
    });
    return {
      onRotateWorker,
      onRevokeWorker,
      onFetchReported,
      onApproveWorker,
      onApplyWorkerConfig,
      ...hook,
    };
  }

  it("arms a confirmation without calling the Hub, then rotates and keeps the once-shown credential", async () => {
    const { result, onRotateWorker } = setup();
    expect(result.current.actions.pending).toBeNull();

    act(() => result.current.actions.request("rotate"));
    expect(result.current.actions.pending).toBe("rotate");
    expect(onRotateWorker).not.toHaveBeenCalled();

    await act(async () => result.current.actions.rotate());
    expect(onRotateWorker).toHaveBeenCalledWith("wkr_00000000000a");
    expect(result.current.actions.pending).toBeNull();
    expect(result.current.actions.busy).toBe(false);
    expect(result.current.actions.rotatedCredential).toBe("cred-once");
  });

  it("cancels an armed confirmation without a request", () => {
    const { result, onRevokeWorker } = setup();
    act(() => result.current.actions.request("revoke"));
    act(() => result.current.actions.cancel());
    expect(result.current.actions.pending).toBeNull();
    expect(onRevokeWorker).not.toHaveBeenCalled();
  });

  it("keeps the confirmation armed and reports the honest failure when revoke fails", async () => {
    const failure = new HubApiError(500, "Internal", "The Hub returned status 500.");
    const { result } = setup(undefined, async () => {
      throw failure;
    });
    act(() => result.current.actions.request("revoke"));
    await act(async () => result.current.actions.revoke());

    expect(result.current.actions.pending).toBe("revoke");
    expect(result.current.actions.error).toBe("The Hub returned status 500.");
    expect(result.current.actions.busy).toBe(false);
    expect(result.current.actions.rotatedCredential).toBeNull();
  });

  it("clears a previously shown credential once revoke succeeds", async () => {
    const { result } = setup();
    act(() => result.current.actions.request("rotate"));
    await act(async () => result.current.actions.rotate());
    expect(result.current.actions.rotatedCredential).toBe("cred-once");

    act(() => result.current.actions.request("revoke"));
    await act(async () => result.current.actions.revoke());
    expect(result.current.actions.rotatedCredential).toBeNull();
    expect(result.current.actions.pending).toBeNull();
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
    act(() => result.current.actions.request("rotate"));
    let rotation!: Promise<void>;
    act(() => {
      rotation = result.current.actions.rotate();
    });
    unmount();
    await act(async () => {
      release();
      await rotation;
    });
    // No state update after unmount: React would warn and the credential would
    // be lost with the card anyway.
    expect(result.current.actions.rotatedCredential).toBeNull();
  });

  it("loads the report and submits exactly the revision it belongs to", async () => {
    const applied: { workerId: string; revision: number }[] = [];
    const { result, onApplyWorkerConfig } = setup(undefined, undefined, {
      reportedRevision: 3,
      onApplyWorkerConfig: async (workerId, revision) => {
        applied.push({ workerId, revision });
      },
    });

    // Reviewing reads the report and arms Apply for exactly the revision it was reported at.
    await act(async () => result.current.actions.review());
    expect(result.current.review.reviewed).toEqual({
      revision: 3,
      config: report("cuda"),
    });
    expect(result.current.actions.pending).toBe("apply");
    expect(result.current.review.reviewed?.revision).toBe(3);

    await act(async () => result.current.actions.apply());
    expect(onApplyWorkerConfig).toHaveBeenCalledWith("wkr_00000000000a", 3);
    expect(applied).toEqual([{ workerId: "wkr_00000000000a", revision: 3 }]);
    expect(result.current.actions.pending).toBeNull();
    expect(result.current.review.reviewed).toBeNull();
  });

  it("submits the revision of the displayed body, not a newer Snapshot revision", async () => {
    let releaseReport!: () => void;
    const reportGate = new Promise<void>((resolve) => {
      releaseReport = resolve;
    });
    const { result, onApplyWorkerConfig } = setup(undefined, undefined, {
      reportedRevision: 3,
      onFetchReported: async () => {
        await reportGate;
        return report("cuda");
      },
    });

    let reviewing!: Promise<void>;
    act(() => {
      reviewing = result.current.actions.review();
    });
    await act(async () => {
      releaseReport();
      await reviewing;
    });
    expect(result.current.review.reviewed?.revision).toBe(3);

    await act(async () => result.current.actions.apply());
    // The submitted revision is the one the reviewed body carries.
    expect(onApplyWorkerConfig).toHaveBeenCalledWith("wkr_00000000000a", 3);
  });

  it("drops a stale report response instead of replacing newer reviewed content", async () => {
    let releaseFirst!: () => void;
    const firstRead = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    let call = 0;
    const { result } = setup(undefined, undefined, {
      reportedRevision: 3,
      onFetchReported: async () => {
        call += 1;
        if (call === 1) {
          await firstRead;
          return report("old");
        }
        return report("new");
      },
    });

    // The first read is slow; a manual reload is the second, faster one.
    let firstReadPromise!: Promise<unknown>;
    act(() => {
      firstReadPromise = result.current.review.load(3);
    });
    await act(async () => {
      await result.current.review.load(3);
    });
    expect(result.current.review.reviewed?.config.slots[0].capabilities[0].name).toBe("new");

    // The older response lands last and must not overwrite the newer reviewed body.
    await act(async () => {
      releaseFirst();
      await firstReadPromise;
    });
    expect(result.current.review.reviewed?.config.slots[0].capabilities[0].name).toBe("new");
  });

  it("keeps the reviewed revision visible when Apply is refused", async () => {
    const { result } = setup(undefined, undefined, {
      reportedRevision: 3,
      onApplyWorkerConfig: async () => {
        throw new HubApiError(409, "Conflict", "The revision changed.");
      },
    });
    await act(async () => result.current.actions.review());
    expect(result.current.review.reviewed?.revision).toBe(3);

    await act(async () => result.current.actions.apply());
    expect(result.current.actions.error).toBe("The revision changed.");
    expect(result.current.actions.pending).toBe("apply");
    expect(result.current.review.reviewed?.revision).toBe(3);
  });

  it("never arms Apply without a reported revision", async () => {
    const { result, onApplyWorkerConfig, onFetchReported } = setup(undefined, undefined, {
      reportedRevision: null,
    });
    await act(async () => result.current.actions.review());
    expect(result.current.actions.pending).toBeNull();
    expect(result.current.review.reviewed).toBeNull();
    expect(onFetchReported).not.toHaveBeenCalled();
    await act(async () => result.current.actions.apply());
    expect(onApplyWorkerConfig).not.toHaveBeenCalled();
  });

  it("approves through the shared busy and error state", async () => {
    const { result, onApproveWorker } = setup(undefined, undefined, {
      onApproveWorker: async () => {
        throw new HubApiError(503, "Unavailable", "The Hub is restarting.");
      },
    });
    await act(async () => result.current.actions.approve());
    expect(onApproveWorker).toHaveBeenCalledWith("wkr_00000000000a");
    expect(result.current.actions.error).toBe("The Hub is restarting.");
    expect(result.current.actions.busy).toBe(false);
  });
});
