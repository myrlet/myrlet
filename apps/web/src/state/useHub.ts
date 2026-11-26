/**
 * Hub session state for the Myrlet Web UI.
 *
 * Ownership rules:
 * - Theme preference is the *only* value persisted (`localStorage["myrlet.theme"]`).
 * - The Admin bearer token lives exclusively in memory (React ref/state) and is
 *   never written to storage, URLs, or logs.
 * - SSE frames are invalidation hints. On each hint we debounce a lightweight
 *   Snapshot refresh (plus the selected Task detail) rather than trusting the
 *   frame payload as state.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { HubApiError, HubClient } from "../api/client";
import type {
  CancelTaskRequest,
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  DispatchTaskRequest,
  DispatchTaskResponse,
  FinishTaskRequest,
  GlobalSnapshot,
  HubEventEnvelope,
  RotateWorkerResponse,
  SnapshotTaskSummary,
  TaskBundleImportRequest,
  TaskBundleImportResponse,
  TaskDefinitionInput,
  TaskRecord,
  TaskTreeResponse,
  WorkerRecord,
} from "../api/types";

export type Theme = "porcelain" | "graphite";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "reconnecting";

/** Non-secret session identity; the token is intentionally not exposed. */
export interface HubSession {
  readonly baseUrl: string;
}

export interface UseHubResult {
  /* Theme */
  readonly theme: Theme;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;

  /* Session and connection */
  readonly connection: ConnectionStatus;
  /** Last Hub URL used (or the current origin); `""` means same-origin. */
  readonly baseUrl: string;
  readonly session: HubSession | null;
  readonly error: string | null;
  clearError: () => void;
  connect: (baseUrl: string, token: string) => Promise<boolean>;
  disconnect: () => void;

  /* Snapshot data */
  readonly snapshot: GlobalSnapshot | null;
  readonly tasks: readonly SnapshotTaskSummary[];
  readonly workers: readonly WorkerRecord[];
  readonly eventCursor: number;
  readonly lastSyncedAtMs: number | null;
  refresh: () => Promise<void>;

  /* Selected Task detail */
  readonly selectedTaskId: string | null;
  readonly selectedTask: TaskRecord | null;
  readonly selectedTree: TaskTreeResponse | null;
  readonly detailLoading: boolean;
  readonly detailError: string | null;
  selectTask: (taskId: string | null) => Promise<void>;

  /* Task commands */
  createTask: (definition: TaskDefinitionInput) => Promise<TaskRecord>;
  importBundle: (request: TaskBundleImportRequest) => Promise<TaskBundleImportResponse>;
  dispatchTask: (taskId: string, request: DispatchTaskRequest) => Promise<DispatchTaskResponse>;
  cancelTask: (taskId: string, request: CancelTaskRequest) => Promise<TaskRecord>;
  finishTask: (taskId: string, request: FinishTaskRequest) => Promise<TaskRecord>;

  /* Worker commands */
  createEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<CreateEnrollmentCodeResponse>;
  rotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  revokeWorker: (workerId: string) => Promise<void>;
}

const THEME_STORAGE_KEY = "myrlet.theme";
const REFRESH_DEBOUNCE_MS = 150;
const RECONNECT_BASE_MS = 1_000;
const RECONNECT_MAX_MS = 8_000;

const EMPTY_TASKS: readonly SnapshotTaskSummary[] = [];
const EMPTY_WORKERS: readonly WorkerRecord[] = [];

function defaultBaseUrl(): string {
  if (typeof window === "undefined" || typeof window.location === "undefined") {
    return "";
  }
  const origin = window.location.origin;
  return origin === "null" ? "" : origin;
}

function readStoredTheme(): Theme {
  if (typeof window === "undefined") {
    return "porcelain";
  }
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "porcelain" || stored === "graphite") {
      return stored;
    }
  } catch {
    // Storage can be unavailable (privacy mode); fall back to Porcelain.
  }
  return "porcelain";
}

/** Abortable delay used by the bounded reconnect backoff. */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    let timer = 0;
    const finish = (): void => {
      window.clearTimeout(timer);
      signal.removeEventListener("abort", finish);
      resolve();
    };
    timer = window.setTimeout(finish, ms);
    signal.addEventListener("abort", finish, { once: true });
  });
}

function describeConnectionError(error: unknown): string {
  if (error instanceof HubApiError) {
    if (error.status === 401) {
      return "The Hub rejected this Admin Bearer Token. Check the token and try again.";
    }
    if (error.status === 0) {
      return "The Hub could not be reached. Check the Hub URL and that the Hub is running.";
    }
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub connection could not be established.";
}

/**
 * Owns the authenticated Hub session, theme, snapshot, selection, and commands.
 * Mount exactly once at the app root.
 */
export function useHub(): UseHubResult {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);
  const [connection, setConnection] = useState<ConnectionStatus>("disconnected");
  const [baseUrl, setBaseUrl] = useState<string>(defaultBaseUrl);
  const [session, setSession] = useState<HubSession | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [snapshot, setSnapshot] = useState<GlobalSnapshot | null>(null);
  const [eventCursor, setEventCursor] = useState(0);
  const [lastSyncedAtMs, setLastSyncedAtMs] = useState<number | null>(null);

  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [selectedTask, setSelectedTask] = useState<TaskRecord | null>(null);
  const [selectedTree, setSelectedTree] = useState<TaskTreeResponse | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);

  const clientRef = useRef<HubClient | null>(null);
  const streamRef = useRef<AbortController | null>(null);
  const refreshTimerRef = useRef<number | null>(null);
  const cursorRef = useRef(0);
  const selectedIdRef = useRef<string | null>(null);
  const selectTokenRef = useRef(0);

  /* Theme: persisted in localStorage only and applied to <html data-theme>. */
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Persisting the theme is best-effort.
    }
  }, [theme]);

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
  }, []);

  const toggleTheme = useCallback(() => {
    setThemeState((current) => (current === "porcelain" ? "graphite" : "porcelain"));
  }, []);

  const getClient = useCallback((): HubClient => {
    const client = clientRef.current;
    if (!client) {
      throw new Error("Not connected to a Myrlet Hub.");
    }
    return client;
  }, []);

  const resetViewState = useCallback(() => {
    cursorRef.current = 0;
    selectedIdRef.current = null;
    selectTokenRef.current += 1;
    setSnapshot(null);
    setEventCursor(0);
    setLastSyncedAtMs(null);
    setSelectedTaskId(null);
    setSelectedTask(null);
    setSelectedTree(null);
    setDetailLoading(false);
    setDetailError(null);
  }, []);

  const handleAuthFailure = useCallback(() => {
    clientRef.current = null;
    streamRef.current?.abort();
    streamRef.current = null;
    if (refreshTimerRef.current !== null) {
      window.clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = null;
    }
    resetViewState();
    setSession(null);
    setConnection("disconnected");
    setError("The Hub rejected this Admin Bearer Token. Check the token and connect again.");
  }, [resetViewState]);

  const applySnapshot = useCallback((next: GlobalSnapshot) => {
    cursorRef.current = next.eventCursor;
    setSnapshot(next);
    setEventCursor(next.eventCursor);
    setLastSyncedAtMs(Date.now());
  }, []);

  const refreshSelected = useCallback(
    async (client: HubClient): Promise<void> => {
      const taskId = selectedIdRef.current;
      if (taskId === null) {
        return;
      }
      // Background refreshes observe the current selection token rather than
      // claiming it, so they can never invalidate a user-initiated selection.
      const requestToken = selectTokenRef.current;
      try {
        const [task, tree] = await Promise.all([
          client.getTask(taskId),
          client.getTaskTree(taskId, "ancestors", 5, 200),
        ]);
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
        setSelectedTask(task);
        setSelectedTree(tree);
        setDetailError(null);
      } catch (refreshError) {
        if (
          selectTokenRef.current !== requestToken ||
          selectedIdRef.current !== taskId
        ) {
          return;
        }
        if (refreshError instanceof HubApiError && refreshError.status === 401) {
          handleAuthFailure();
          return;
        }
        if (refreshError instanceof HubApiError && refreshError.status === 404) {
          selectedIdRef.current = null;
          selectTokenRef.current += 1;
          setSelectedTaskId(null);
          setSelectedTask(null);
          setSelectedTree(null);
          setDetailError("This Task no longer exists on the Hub.");
          return;
        }
        // Keep the last known detail; the next event or manual refresh retries.
      }
    },
    [handleAuthFailure],
  );

  const reloadSnapshot = useCallback(
    async (client: HubClient): Promise<boolean> => {
      try {
        const next = await client.getSnapshot();
        if (clientRef.current !== client) {
          return false;
        }
        applySnapshot(next);
        return true;
      } catch (reloadError) {
        if (clientRef.current !== client) {
          return false;
        }
        if (reloadError instanceof HubApiError && reloadError.status === 401) {
          handleAuthFailure();
        }
        // A transient read failure leaves the last Snapshot in place; the SSE
        // reconnect loop will retry. Snapshot is never fabricated.
        return false;
      }
    },
    [applySnapshot, handleAuthFailure],
  );

  const refreshAll = useCallback(async (): Promise<void> => {
    const client = clientRef.current;
    if (!client) {
      return;
    }
    const reloaded = await reloadSnapshot(client);
    if (!reloaded) {
      return;
    }
    await refreshSelected(client);
  }, [reloadSnapshot, refreshSelected]);

  const handleEvent = useCallback(
    (envelope: HubEventEnvelope) => {
      if (envelope.seq > cursorRef.current) {
        cursorRef.current = envelope.seq;
        setEventCursor(envelope.seq);
      }
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
      }
      refreshTimerRef.current = window.setTimeout(() => {
        refreshTimerRef.current = null;
        void refreshAll();
      }, REFRESH_DEBOUNCE_MS);
    },
    [refreshAll],
  );

  const startEventLoop = useCallback(
    (client: HubClient) => {
      streamRef.current?.abort();
      const controller = new AbortController();
      streamRef.current = controller;
      let attempt = 0;

      const run = async (): Promise<void> => {
        while (!controller.signal.aborted && clientRef.current === client) {
          const outcome = await client.openEvents(
            cursorRef.current > 0 ? cursorRef.current : null,
            controller.signal,
            {
              onOpen: () => {
                attempt = 0;
                if (!controller.signal.aborted && clientRef.current === client) {
                  setConnection("connected");
                }
              },
              onEvent: handleEvent,
            },
          );

          if (controller.signal.aborted || clientRef.current !== client) {
            return;
          }

          if (outcome === "unauthorized") {
            handleAuthFailure();
            return;
          }
          if (outcome === "aborted") {
            return;
          }

          if (outcome === "cursor_expired") {
            attempt = 0;
            setConnection("reconnecting");
            const reloaded = await reloadSnapshot(client);
            if (controller.signal.aborted || clientRef.current !== client) {
              return;
            }
            if (reloaded) {
              void refreshSelected(client);
              setConnection("connected");
              // Immediately reopen SSE from the fresh cursor.
              continue;
            }
          }

          attempt = Math.min(attempt + 1, 4);
          const delay = Math.min(
            RECONNECT_BASE_MS * 2 ** (attempt - 1),
            RECONNECT_MAX_MS,
          );
          setConnection("reconnecting");
          await sleep(delay, controller.signal);
        }
      };

      void run();
    },
    [handleAuthFailure, handleEvent, refreshSelected, reloadSnapshot],
  );

  const clearError = useCallback(() => {
    setError(null);
  }, []);

  const disconnect = useCallback(() => {
    clientRef.current = null;
    streamRef.current?.abort();
    streamRef.current = null;
    if (refreshTimerRef.current !== null) {
      window.clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = null;
    }
    resetViewState();
    setSession(null);
    setConnection("disconnected");
    setError(null);
  }, [resetViewState]);

  const connect = useCallback(
    async (rawBaseUrl: string, rawToken: string): Promise<boolean> => {
      const token = rawToken.trim();
      const nextBaseUrl = rawBaseUrl.trim();

      if (token === "") {
        setError("An Admin Bearer Token is required.");
        return false;
      }

      streamRef.current?.abort();
      streamRef.current = null;
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      }
      resetViewState();

      const client = new HubClient({ baseUrl: nextBaseUrl, token });
      clientRef.current = client;
      setBaseUrl(client.baseUrl);
      setConnection("connecting");
      setError(null);

      try {
        const initial = await client.getSnapshot();
        if (clientRef.current !== client) {
          return false;
        }
        applySnapshot(initial);
        setSession({ baseUrl: client.baseUrl });
        setConnection("connected");
        startEventLoop(client);
        return true;
      } catch (connectError) {
        if (clientRef.current !== client) {
          return false;
        }
        clientRef.current = null;
        resetViewState();
        setSession(null);
        setConnection("disconnected");
        setError(describeConnectionError(connectError));
        return false;
      }
    },
    [applySnapshot, resetViewState, startEventLoop],
  );

  const selectTask = useCallback(
    async (taskId: string | null): Promise<void> => {
      if (taskId === null) {
        selectedIdRef.current = null;
        selectTokenRef.current += 1;
        setSelectedTaskId(null);
        setSelectedTask(null);
        setSelectedTree(null);
        setDetailLoading(false);
        setDetailError(null);
        return;
      }

      selectedIdRef.current = taskId;
      setSelectedTaskId(taskId);
      setDetailError(null);

      const client = clientRef.current;
      if (!client) {
        return;
      }

      const requestToken = ++selectTokenRef.current;
      setDetailLoading(true);
      try {
        const [task, tree] = await Promise.all([
          client.getTask(taskId),
          client.getTaskTree(taskId, "ancestors", 5, 200),
        ]);
        if (selectTokenRef.current !== requestToken) {
          return;
        }
        setSelectedTask(task);
        setSelectedTree(tree);
        setDetailError(null);
      } catch (selectError) {
        if (selectTokenRef.current !== requestToken) {
          return;
        }
        if (selectError instanceof HubApiError && selectError.status === 401) {
          handleAuthFailure();
          return;
        }
        if (selectError instanceof HubApiError && selectError.status === 404) {
          selectedIdRef.current = null;
          setSelectedTaskId(null);
          setSelectedTask(null);
          setSelectedTree(null);
          setDetailError("This Task no longer exists on the Hub.");
          return;
        }
        setDetailError(
          selectError instanceof Error
            ? selectError.message
            : "The Task could not be loaded.",
        );
      } finally {
        if (selectTokenRef.current === requestToken) {
          setDetailLoading(false);
        }
      }
    },
    [handleAuthFailure],
  );

  /**
   * Run one authenticated command, then refresh Snapshot and the selected Task.
   * Errors are rethrown so dialogs can display them inline.
   */
  const mutate = useCallback(
    async <T>(operation: (client: HubClient) => Promise<T>): Promise<T> => {
      const client = getClient();
      try {
        const result = await operation(client);
        await refreshAll();
        return result;
      } catch (mutationError) {
        if (mutationError instanceof HubApiError && mutationError.status === 401) {
          handleAuthFailure();
        }
        throw mutationError;
      }
    },
    [getClient, handleAuthFailure, refreshAll],
  );

  const createTask = useCallback(
    (definition: TaskDefinitionInput) => mutate((client) => client.createTask(definition)),
    [mutate],
  );

  const importBundle = useCallback(
    (request: TaskBundleImportRequest) => mutate((client) => client.importBundle(request)),
    [mutate],
  );

  const dispatchTask = useCallback(
    (taskId: string, request: DispatchTaskRequest) =>
      mutate((client) => client.dispatchTask(taskId, request)),
    [mutate],
  );

  const cancelTask = useCallback(
    (taskId: string, request: CancelTaskRequest) =>
      mutate((client) => client.cancelTask(taskId, request)),
    [mutate],
  );

  const finishTask = useCallback(
    (taskId: string, request: FinishTaskRequest) =>
      mutate((client) => client.finishTask(taskId, request)),
    [mutate],
  );

  const createEnrollmentCode = useCallback(
    async (
      request: CreateEnrollmentCodeRequest,
    ): Promise<CreateEnrollmentCodeResponse> => {
      const client = getClient();
      try {
        return await client.createEnrollmentCode(request);
      } catch (issueError) {
        if (issueError instanceof HubApiError && issueError.status === 401) {
          handleAuthFailure();
        }
        throw issueError;
      }
    },
    [getClient, handleAuthFailure],
  );

  const rotateWorker = useCallback(
    (workerId: string) => mutate((client) => client.rotateWorker(workerId)),
    [mutate],
  );

  const revokeWorker = useCallback(
    (workerId: string) => mutate((client) => client.revokeWorker(workerId)),
    [mutate],
  );

  const tasks = useMemo(
    () => snapshot?.tasks ?? EMPTY_TASKS,
    [snapshot],
  );
  const workers = useMemo(
    () => snapshot?.workers ?? EMPTY_WORKERS,
    [snapshot],
  );

  useEffect(
    () => () => {
      clientRef.current = null;
      streamRef.current?.abort();
      streamRef.current = null;
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      }
    },
    [],
  );

  return {
    theme,
    setTheme,
    toggleTheme,

    connection,
    baseUrl,
    session,
    error,
    clearError,
    connect,
    disconnect,

    snapshot,
    tasks,
    workers,
    eventCursor,
    lastSyncedAtMs,
    refresh: refreshAll,

    selectedTaskId,
    selectedTask,
    selectedTree,
    detailLoading,
    detailError,
    selectTask,

    createTask,
    importBundle,
    dispatchTask,
    cancelTask,
    finishTask,

    createEnrollmentCode,
    rotateWorker,
    revokeWorker,
  };
}
