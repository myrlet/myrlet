import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { HubApiError, HubClient } from "../../api/client";
import { useHub } from "../../state/useHub";

beforeEach(() => {
  vi.spyOn(HubClient.prototype, "getSnapshot").mockResolvedValue({
    schemaVersion: "myrlet.snapshot/v1",
    generatedAtMs: 0,
    eventCursor: 0,
    tasksTruncated: false,
    totalTasks: 0,
    workers: [],
    tasks: [],
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

it.each(["stream rejects the token", "refresh finds the task missing"] as const)(
  "clears a pending selection when the %s",
  async (scenario) => {
    let expireStream!: () => void;
    vi.spyOn(HubClient.prototype, "openEvents").mockImplementation(
      () => new Promise((resolve) => { expireStream = () => resolve("unauthorized"); }),
    );
    let finishSelection!: () => void;
    vi.spyOn(HubClient.prototype, "getTask")
      .mockImplementationOnce(() => new Promise((_, reject) => {
        finishSelection = () => reject(new Error("late detail response"));
      }))
      .mockRejectedValueOnce(new HubApiError(404, "NotFound", "Missing task"));

    const { result } = renderHook(useHub);
    await act(async () => { await result.current.connect("", "test-token"); });
    let selection!: Promise<void>;
    act(() => { selection = result.current.selectTask("selected"); });
    expect(result.current.detailLoading).toBe(true);
    expect(result.current.selectedTaskId).toBe("selected");

    if (scenario === "stream rejects the token") {
      await act(async () => { expireStream(); });
      expect(result.current.session).toBeNull();
    } else {
      await act(async () => { await result.current.refresh(); });
      expect(result.current.session).not.toBeNull();
    }
    await act(async () => { finishSelection(); await selection; });
    expect(result.current.selectedTaskId).toBeNull();
    expect(result.current.detailLoading).toBe(false);
    expect(result.current.detailError).toBe(
      scenario === "stream rejects the token" ? null : "This Task no longer exists on the Hub.",
    );
  },
);

it("keeps the new session's selection when an old worker command returns 401", async () => {
  vi.spyOn(HubClient.prototype, "openEvents").mockResolvedValue("aborted");
  vi.spyOn(HubClient.prototype, "getTask").mockRejectedValue(new Error("detail unavailable"));
  let rejectOldCommand!: () => void;
  vi.spyOn(HubClient.prototype, "revokeWorker").mockImplementation(
    () => new Promise((_, reject) => {
      rejectOldCommand = () => reject(new HubApiError(401, "Unauthorized", "Expired"));
    }),
  );
  const { result } = renderHook(useHub);
  await act(async () => { await result.current.connect("", "first-token"); });
  const command = result.current.revokeWorker("worker");
  await act(async () => {
    await result.current.connect("", "second-token");
    await result.current.selectTask("new-selection");
  });
  await act(async () => {
    rejectOldCommand();
    await expect(command).rejects.toMatchObject({ status: 401 });
  });
  expect(result.current.session).not.toBeNull();
  expect(result.current.selectedTaskId).toBe("new-selection");
});
