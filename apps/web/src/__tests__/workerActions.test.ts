/**
 * Worker inventory predicates and local credential-action lifecycle: pure
 * filters plus the `useWorkerActions` hook, which owns the armed confirmation,
 * the in-flight flag, honest error wording, and unmount safety.
 */

import "@testing-library/jest-dom/vitest";

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HubApiError } from "../api/client";
import type { RotateWorkerResponse, WorkerRecord } from "../api/types";
import { describeError, describeIssueError } from "../components/workers/workerErrors";
import { useWorkerActions } from "../components/workers/useWorkerActions";
import {
  workerCounts,
  workerMatchesQuery,
  workerState,
} from "../components/workers/workerFilters";

function worker(overrides: Partial<WorkerRecord> = {}): WorkerRecord {
  return {
    workerId: "wkr_00000000000a",
    online: true,
    revoked: false,
    connectionEpoch: 4,
    lastSeenAtMs: 1_700_000_000_000,
    activeDeliveries: 2,
    inventory: {
      revision: 2,
      observedAtMs: 1_700_000_000_000,
      host: { os: "linux", arch: "x86_64", deploymentMode: "container" },
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
  it("derives revoked before connection state", () => {
    expect(workerState(worker())).toBe("online");
    expect(workerState(worker({ online: false }))).toBe("offline");
    expect(workerState(worker({ online: true, revoked: true }))).toBe("revoked");
    expect(workerState(worker({ online: false, revoked: true }))).toBe("revoked");
  });

  it("counts revoked Workers as registered but neither online nor offline", () => {
    const counts = workerCounts([
      worker({ workerId: "w1" }),
      worker({ workerId: "w2", online: false }),
      worker({ workerId: "w3", revoked: true }),
    ]);
    expect(counts).toEqual({ registered: 3, online: 1, offline: 1, activeDeliveries: 6 });
  });

  it("matches the Worker ID, host, Slot, role, capability, and executor", () => {
    const record = worker();
    for (const query of ["00000000000a", "linux", "container", "gpu-1", "build", "cuda", "codex"]) {
      expect(workerMatchesQuery(record, query)).toBe(true);
    }
    expect(workerMatchesQuery(record, "windows")).toBe(false);
    expect(workerMatchesQuery(record, "")).toBe(true);
    expect(workerMatchesQuery(worker({ inventory: null }), "gpu")).toBe(false);
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
  ) {
    const onRotateWorker = vi.fn(rotate);
    const onRevokeWorker = vi.fn(revoke);
    const hook = renderHook(() =>
      useWorkerActions({
        workerId: "wkr_00000000000a",
        onRotateWorker,
        onRevokeWorker,
      }),
    );
    return { onRotateWorker, onRevokeWorker, ...hook };
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
});
