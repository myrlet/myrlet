/**
 * Selected Task detail for the Myrlet Web UI.
 *
 * There is one canonical read path: the authoritative Task record
 * (`GET /v1/tasks/{id}`) and then its root's bounded descendants
 * (`descendants`, depth 5, limit 200), so the Context Mini-Tree always contains
 * the parent, siblings, the Task, and its children in one read.
 *
 * Every read carries the selection token and the session client it ran on, so a
 * response for an abandoned selection or a replaced Hub session can never
 * commit. A foreground read owns `detailLoading` and records its failure; a
 * background refresh never touches the spinner and stays silent on transient
 * failures, keeping the last known detail.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { RefObject } from "react";

import { HubApiError } from "../api/client";
import type { HubClient } from "../api/client";
import type { TaskRecord, TaskTreeResponse } from "../api/types";

interface UseTaskDetailOptions {
  /** The current session client; `null` while no Hub session exists. */
  readonly clientRef: RefObject<HubClient | null>;
  /** Report an auth failure together with the session that produced it. */
  readonly onAuthFailure: (client: HubClient) => void;
}

export function useTaskDetail(options: UseTaskDetailOptions) {
  const { clientRef, onAuthFailure } = options;

  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedTask, setSelectedTask] = useState<TaskRecord | null>(null);
  const [selectedTree, setSelectedTree] = useState<TaskTreeResponse | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const selectedIdRef = useRef<string | null>(null);
  const selectTokenRef = useRef(0);

  const resetDetail = useCallback(() => {
    selectedIdRef.current = null;
    selectTokenRef.current += 1;
    setSelectedTaskId(null);
    setSelectedTask(null);
    setSelectedTree(null);
    setDetailLoading(false);
    setDetailError(null);
  }, []);

  const loadDetail = useCallback(
    async (mode: "foreground" | "background"): Promise<void> => {
      const client = clientRef.current;
      const taskId = selectedIdRef.current;
      if (!client || taskId === null) return;

      const foreground = mode === "foreground";
      // A background refresh observes the selection. Only an explicit read
      // starts a new generation and owns the loading indicator.
      const requestToken = foreground ? ++selectTokenRef.current : selectTokenRef.current;
      const isCurrent = () =>
        clientRef.current === client && selectTokenRef.current === requestToken;
      if (foreground) {
        setDetailError(null);
        setDetailLoading(true);
      }
      try {
        const task = await client.getTask(taskId);
        if (!isCurrent()) {
          return;
        }
        setSelectedTask(task);

        // Root-scoped descendants give the Context Mini-Tree the parent,
        // siblings, the current Task, and its children in one read.
        const tree = await client.getTaskTree(task.rootTaskId, "descendants", 5, 200);
        if (!isCurrent()) {
          return;
        }
        setSelectedTree(tree);
        setDetailError(null);
      } catch (loadError) {
        if (!isCurrent()) {
          return;
        }
        if (loadError instanceof HubApiError && loadError.status === 401) {
          // A rejected token ends the session; drop this detail either way so a
          // background refresh cannot leave stale detail after signing out.
          resetDetail();
          onAuthFailure(client);
          return;
        }
        if (loadError instanceof HubApiError && loadError.status === 404) {
          resetDetail();
          setDetailError("This Task no longer exists on the Hub.");
          return;
        }
        // A foreground failure is stated in the panel with the last known
        // detail left in place; a background one is transient and stays silent.
        if (foreground) {
          setDetailError(
            loadError instanceof Error ? loadError.message : "The Task could not be loaded.",
          );
        }
      } finally {
        if (foreground && isCurrent()) {
          setDetailLoading(false);
        }
      }
    },
    [clientRef, onAuthFailure, resetDetail],
  );

  const selectTask = useCallback(
    async (taskId: string | null): Promise<void> => {
      if (taskId === null) {
        resetDetail();
        return;
      }

      selectedIdRef.current = taskId;
      setSelectedTaskId(taskId);
      setSelectedTask((current) => (current !== null && current.id === taskId ? current : null));
      setSelectedTree(null);
      setDetailError(null);
      await loadDetail("foreground");
    },
    [loadDetail, resetDetail],
  );

  const retrySelected = useCallback(async (): Promise<void> => {
    setDetailError(null);
    await loadDetail("foreground");
  }, [loadDetail]);
  const refreshSelected = useCallback(() => loadDetail("background"), [loadDetail]);
  const clearDetailError = useCallback(() => setDetailError(null), []);

  useEffect(
    () => () => {
      // Fence every in-flight read on unmount.
      selectedIdRef.current = null;
      selectTokenRef.current += 1;
    },
    [],
  );

  return {
    selectedTaskId,
    selectedTask,
    selectedTree,
    detailLoading,
    detailError,
    selectTask,
    retrySelected,
    clearDetailError,
    refreshSelected,
    resetDetail,
  };
}
