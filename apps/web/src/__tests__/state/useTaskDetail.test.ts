/**
 * Regression coverage for the selected Task detail hook: one canonical read
 * path, request fencing across selection and session changes, and the split
 * between a foreground read (owns the spinner and its error) and a silent
 * background refresh.
 */

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { HubApiError, HubClient } from "../../api/client";
import type { TaskRecord, TaskTreeResponse } from "../../api/types";
import { useTaskDetail } from "../../state/useTaskDetail";

function taskRecord(id: string, rootTaskId = id): TaskRecord {
  return {
    id,
    rootTaskId,
    depth: 1,
    status: "queued",
    version: 1,
    createdAtMs: 1,
    updatedAtMs: 1,
    finishedAtMs: null,
    currentDelivery: null,
    blockedBy: [],
    resultMarkdown: null,
    resultRecordedAtMs: null,
    subtree: { total: 1, held: 0, queued: 1, dispatched: 0, finished: 0, cancelled: 0 },
    task: {
      clientRef: null,
      parentTaskId: null,
      sourceTaskId: null,
      dependencies: [],
      category: "general",
      tags: [],
      title: id,
      prompt: `Work on ${id}.`,
      input: null,
      expectedOutput: null,
      externalReferences: [],
      routing: {},
      execution: {},
      metadata: {},
    },
  };
}

function taskTree(rootTaskId: string): TaskTreeResponse {
  return { rootTaskId, tasks: [taskRecord(rootTaskId)], truncated: false };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function setup() {
  const client = new HubClient({ baseUrl: "", token: "{{SECRET_ee7mk2pq}}" });
  const clientRef: { current: HubClient | null } = { current: client };
  const onAuthFailure = vi.fn();
  const hook = renderHook(() => useTaskDetail({ clientRef, onAuthFailure }));
  return { client, clientRef, onAuthFailure, ...hook };
}

describe("useTaskDetail", () => {
  it("reads the selected task and then its root tree at depth 5 with a 200-task limit", async () => {
    const { client, result } = setup();
    const getTask = vi.spyOn(client, "getTask").mockResolvedValue(taskRecord("child", "root"));
    const getTree = vi.spyOn(client, "getTaskTree").mockResolvedValue(taskTree("root"));

    await act(async () => {
      await result.current.selectTask("child");
    });

    expect(getTask).toHaveBeenCalledTimes(1);
    expect(getTask).toHaveBeenCalledWith("child");
    expect(getTree).toHaveBeenCalledTimes(1);
    expect(getTree).toHaveBeenCalledWith("root", "descendants", 5, 200);
    expect(result.current.selectedTask?.id).toBe("child");
    expect(result.current.selectedTree?.rootTaskId).toBe("root");
    expect(result.current.detailLoading).toBe(false);
    expect(result.current.detailError).toBeNull();
  });

  it("keeps the last detail and leaves loading alone when a background tree read fails", async () => {
    const { client, result } = setup();
    const task = taskRecord("child", "root");
    const tree = taskTree("root");
    vi.spyOn(client, "getTask").mockResolvedValue(task);
    const getTree = vi.spyOn(client, "getTaskTree").mockResolvedValue(tree);
    await act(async () => { await result.current.selectTask("child"); });

    let rejectTree!: (error: Error) => void;
    getTree.mockImplementationOnce(
      () => new Promise<TaskTreeResponse>((_, reject) => { rejectTree = reject; }),
    );
    let refresh!: Promise<void>;
    await act(async () => { refresh = result.current.refreshSelected(); });
    expect(getTree).toHaveBeenCalledTimes(2);
    expect(result.current.detailLoading).toBe(false);

    await act(async () => {
      rejectTree(new Error("tree unavailable"));
      await refresh;
    });
    expect(result.current.selectedTask).toEqual(task);
    expect(result.current.selectedTree).toEqual(tree);
    expect(result.current.detailError).toBeNull();
    expect(result.current.detailLoading).toBe(false);
  });

  it.each(["current", "replaced"])("handles a late tree 401 from a %s Hub session", async (session) => {
    const { client, clientRef, onAuthFailure, result } = setup();
    vi.spyOn(client, "getTask").mockResolvedValue(taskRecord("child", "root"));
    let rejectTree!: (error: Error) => void;
    vi.spyOn(client, "getTaskTree").mockImplementationOnce(
      () => new Promise<TaskTreeResponse>((_, reject) => { rejectTree = reject; }),
    );
    let read!: Promise<void>;
    await act(async () => { read = result.current.selectTask("child"); });
    expect(result.current.selectedTask?.id).toBe("child");
    expect(result.current.detailLoading).toBe(true);

    if (session === "replaced") {
      clientRef.current = new HubClient({ baseUrl: "", token: "second-test-token" });
    }
    await act(async () => {
      rejectTree(new HubApiError(401, "Unauthorized", "Expired"));
      await read;
    });
    if (session === "current") {
      expect(onAuthFailure).toHaveBeenCalledWith(client);
      expect(result.current.selectedTask).toBeNull();
      expect(result.current.detailLoading).toBe(false);
    } else {
      expect(onAuthFailure).not.toHaveBeenCalled();
    }
    expect(result.current.detailError).toBeNull();
  });

  it("drops a Task read that resolves after another Task is selected", async () => {
    const { client, result } = setup();
    const pending = new Map<string, (task: TaskRecord) => void>();
    vi.spyOn(client, "getTask").mockImplementation(
      (taskId) =>
        new Promise<TaskRecord>((resolve) => {
          pending.set(taskId, resolve);
        }),
    );
    vi.spyOn(client, "getTaskTree").mockImplementation(async (rootTaskId) =>
      taskTree(rootTaskId),
    );

    let first!: Promise<void>;
    act(() => { first = result.current.selectTask("t1"); });
    let second!: Promise<void>;
    act(() => { second = result.current.selectTask("t2"); });

    await act(async () => {
      pending.get("t2")?.(taskRecord("t2"));
      await second;
    });
    await act(async () => {
      pending.get("t1")?.(taskRecord("t1"));
      await first;
    });
    expect(result.current.selectedTaskId).toBe("t2");
    expect(result.current.selectedTask?.id).toBe("t2");
  });

  it("never shows a stale error from an abandoned selection", async () => {
    const { client, result } = setup();
    let rejectFirst!: (error: Error) => void;
    vi.spyOn(client, "getTask")
      .mockImplementationOnce(
        () => new Promise<TaskRecord>((_, reject) => { rejectFirst = reject; }),
      )
      .mockResolvedValueOnce(taskRecord("t2"));
    vi.spyOn(client, "getTaskTree").mockImplementation(async (rootTaskId) =>
      taskTree(rootTaskId),
    );

    let first!: Promise<void>;
    act(() => { first = result.current.selectTask("t1"); });
    let second!: Promise<void>;
    act(() => { second = result.current.selectTask("t2"); });
    await act(async () => { await second; });

    await act(async () => {
      rejectFirst(new Error("stale failure"));
      await first;
    });
    expect(result.current.selectedTask?.id).toBe("t2");
    expect(result.current.detailError).toBeNull();
  });

  it("drops a Task read that resolves after the Hub session is replaced", async () => {
    const { client, clientRef, onAuthFailure, result } = setup();
    let resolveTask!: (task: TaskRecord) => void;
    vi.spyOn(client, "getTask").mockImplementation(
      () => new Promise<TaskRecord>((resolve) => { resolveTask = resolve; }),
    );

    let read!: Promise<void>;
    act(() => { read = result.current.selectTask("t1"); });
    clientRef.current = new HubClient({ baseUrl: "", token: "{{SECRET_m6p1r4tz}}" });
    await act(async () => {
      resolveTask(taskRecord("t1"));
      await read;
    });

    expect(result.current.selectedTask).toBeNull();
    expect(result.current.selectedTree).toBeNull();
    expect(onAuthFailure).not.toHaveBeenCalled();
  });

  it("keeps the foreground spinner off while a background read is in flight", async () => {
    const { client, result } = setup();
    vi.spyOn(client, "getTask").mockResolvedValue(taskRecord("t1"));
    vi.spyOn(client, "getTaskTree").mockResolvedValue(taskTree("t1"));
    await act(async () => { await result.current.selectTask("t1"); });

    let release!: (task: TaskRecord) => void;
    vi.spyOn(client, "getTask").mockImplementationOnce(
      () => new Promise<TaskRecord>((resolve) => { release = resolve; }),
    );
    let refresh!: Promise<void>;
    act(() => { refresh = result.current.refreshSelected(); });
    expect(result.current.detailLoading).toBe(false);

    await act(async () => {
      release(taskRecord("t1"));
      await refresh;
    });
    expect(result.current.detailLoading).toBe(false);
    expect(result.current.selectedTask?.id).toBe("t1");
  });
});
