/**
 * Hub session state for the Myrlet Web UI.
 *
 * Ownership rules:
 * - This hook persists only the appearance preference
 *   (`localStorage["myrlet.theme"]`). Board layout offsets are owned by
 *   `TaskGraphView` (`localStorage["myrlet.atlas.positions"]`).
 * - The Admin bearer token lives exclusively in memory (React ref/state) and is
 *   never written to storage, URLs, or logs.
 * - SSE frames are invalidation hints. On each hint we debounce a lightweight
 *   Snapshot refresh (plus the selected Task detail) rather than trusting the
 *   frame payload as state.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { HubApiError, HubClient } from "../api/client";
import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  GlobalSnapshot,
  HealthResponse,
  HubEventEnvelope,
  RotateWorkerResponse,
  SnapshotTaskSummary,
  TaskRecord,
  TaskTreeResponse,
  WorkerRecord,
} from "../api/types";

export type Theme = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

/** One selectable appearance option. */
export interface ThemeDescriptor {
  readonly id: Theme;
  readonly label: string;
}

/** Ordered appearance options: System follows the browser, or lock Light / Dark. */
export const THEMES: readonly ThemeDescriptor[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "reconnecting";

/** Non-secret session identity; the token is intentionally not exposed. */
export interface HubSession {
  readonly baseUrl: string;
}

export interface UseHubResult {
  /* Theme */
  readonly theme: Theme;
  readonly themeMode: "light" | "dark";
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
  /** Re-read the current selection without changing it. */
  retrySelected: () => Promise<void>;
  clearDetailError: () => void;

  /* Worker commands */
  readonly enrollmentCode: CreateEnrollmentCodeResponse | null;
  readonly issuingCode: boolean;
  createEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  rotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  revokeWorker: (workerId: string) => Promise<void>;
}

const THEME_STORAGE_KEY = "myrlet.theme";
const REFRESH_DEBOUNCE_MS = 150;
const RECONNECT_BASE_MS = 1_000;
const RECONNECT_MAX_MS = 8_000;

const THEME_IDS: readonly Theme[] = THEMES.map((theme) => theme.id);

const EMPTY_TASKS: readonly SnapshotTaskSummary[] = [];
const EMPTY_WORKERS: readonly WorkerRecord[] = [];

function isTheme(value: unknown): value is Theme {
  return typeof value === "string" && (THEME_IDS as readonly string[]).includes(value);
}

/** Normalize a Hub origin for a health probe (`""` means same-origin). */
function normalizeProbeBase(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed === "" || trimmed === "/") {
    return "";
  }
  return trimmed.replace(/\/+$/, "");
}

/**
 * Unauthenticated reachability probe for the Auth gate.
 *
 * `GET /health` is public, so this needs no token and never touches session
 * state. Any failure (network, CORS, non-OK status, unreadable body) resolves
 * `{ ok: false }` rather than throwing, so the pill can state "Unreachable".
 */
export async function probeHubHealth(
  baseUrl: string,
): Promise<{ ok: boolean; version?: string }> {
  let response: Response;
  try {
    response = await fetch(`${normalizeProbeBase(baseUrl)}/health`, {
      method: "GET",
      headers: { Accept: "application/json" },
      cache: "no-store",
    });
  } catch {
    return { ok: false };
  }

  if (!response.ok) {
    return { ok: false };
  }

  try {
    const body: unknown = await response.json();
    if (typeof body === "object" && body !== null) {
      const record = body as Partial<HealthResponse> & Record<string, unknown>;
      return {
        ok: true,
        version: typeof record.version === "string" ? record.version : undefined,
      };
    }
  } catch {
    // A reachable process with an unreadable body is still reachable.
  }

  return { ok: true };
}

function defaultBaseUrl(): string {
  if (typeof window === "undefined" || typeof window.location === "undefined") {
    return "";
  }
  const origin = window.location.origin;
  return origin === "null" ? "" : origin;
}

/** Read the persisted appearance, defaulting to `system` when nothing is stored. */
export function readStoredTheme(): Theme {
  if (typeof window === "undefined") {
    return "system";
  }
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isTheme(stored)) {
      return stored;
    }
  } catch {
    // Storage can be unavailable (privacy mode); follow the browser instead.
  }
  return "system";
}

/** The browser's current `prefers-color-scheme`, safe under jsdom/SSR. */
export function readSystemTheme(): ResolvedTheme {
  if (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  ) {
    return "dark";
  }
  return "light";
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
  const [enrollmentCode, setEnrollmentCode] = useState<CreateEnrollmentCodeResponse | null>(null);
  const [issuingCode, setIssuingCode] = useState(false);
  const issueOwner = useRef<HubClient | null>(null);
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(readSystemTheme);
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

  const themeMode: ResolvedTheme = theme === "system" ? systemTheme : theme;

  /* Theme: the resolved appearance drives <html data-theme>, so CSS and tests
     always see an explicit light/dark. Only a real choice is persisted. */
  useEffect(() => {
    document.documentElement.dataset.theme = themeMode;
  }, [themeMode]);

  /* Follow OS/browser appearance live while the preference is System. */
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    if (typeof media.addEventListener !== "function") {
      return;
    }
    const onChange = (event: MediaQueryListEvent): void => {
      setSystemTheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Persisting the theme is best-effort.
    }
  }, []);

  /** Cycle System -> Light -> Dark -> System. */
  const toggleTheme = useCallback(() => {
    const index = THEME_IDS.indexOf(theme);
    setTheme(THEME_IDS[(index + 1) % THEME_IDS.length] ?? "system");
  }, [setTheme, theme]);

  const getClient = useCallback((): HubClient => {
    const client = clientRef.current;
    if (!client) {
      throw new Error("Not connected to a Myrlet Hub.");
    }
    return client;
  }, []);

  const resetViewState = useCallback(() => {
    issueOwner.current = null;
    setIssuingCode(false);
    setEnrollmentCode(null);
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

  /**
   * Read one Task and its bounded tree, guarded by the selection token so a
   * response for an abandoned selection can never overwrite the current one.
   * A failed read keeps whatever detail is already on screen and only records
   * the error; the panel decides how to surface it.
   */
  const loadTaskDetail = useCallback(
    async (taskId: string, requestToken: number): Promise<void> => {
      const client = clientRef.current;
      if (!client) {
        return;
      }
      setDetailLoading(true);
      try {
        const task = await client.getTask(taskId);
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
        setSelectedTask(task);

        // Root-scoped descendants give the Context Mini-Tree the parent,
        // siblings, the current Task, and its children in one read.
        const tree = await client.getTaskTree(task.rootTaskId, "descendants", 5, 200);
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
        setSelectedTree(tree);
        setDetailError(null);
      } catch (loadError) {
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
        if (loadError instanceof HubApiError && loadError.status === 401) {
          handleAuthFailure();
          return;
        }
        if (loadError instanceof HubApiError && loadError.status === 404) {
          selectedIdRef.current = null;
          selectTokenRef.current += 1;
          setSelectedTaskId(null);
          setSelectedTask(null);
          setSelectedTree(null);
          setDetailLoading(false);
          setDetailError("This Task no longer exists on the Hub.");
          return;
        }
        setDetailError(
          loadError instanceof Error ? loadError.message : "The Task could not be loaded.",
        );
      } finally {
        if (selectTokenRef.current === requestToken) {
          setDetailLoading(false);
        }
      }
    },
    [handleAuthFailure],
  );

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
        const task = await client.getTask(taskId);
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
        setSelectedTask(task);

        // Read the whole tree from the root so the Context Mini-Tree always
        // contains the parent, siblings, the current Task, and its children —
        // not only the ancestry path.
        const tree = await client.getTaskTree(task.rootTaskId, "descendants", 5, 200);
        if (selectTokenRef.current !== requestToken || selectedIdRef.current !== taskId) {
          return;
        }
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
          setDetailLoading(false);
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

      // The frame is an invalidation hint: cursor tracking and the debounced
      // Snapshot refresh below are the whole contract, no event log is kept.
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
      setSelectedTask((current) => (current !== null && current.id === taskId ? current : null));
      setSelectedTree(null);
      setDetailError(null);

      const requestToken = ++selectTokenRef.current;
      await loadTaskDetail(taskId, requestToken);
    },
    [loadTaskDetail],
  );

  /** Re-read the current selection in place, keeping any detail already shown. */
  const retrySelected = useCallback(async (): Promise<void> => {
    const taskId = selectedIdRef.current;
    if (taskId === null) {
      return;
    }
    const requestToken = ++selectTokenRef.current;
    setDetailError(null);
    await loadTaskDetail(taskId, requestToken);
  }, [loadTaskDetail]);

  const clearDetailError = useCallback(() => {
    setDetailError(null);
  }, []);

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

  useEffect(() => {
    if (!enrollmentCode) return;
    const timer = window.setTimeout(() => setEnrollmentCode(null), Math.max(0, enrollmentCode.expiresAtMs - Date.now()));
    return () => window.clearTimeout(timer);
  }, [enrollmentCode]);

  const createEnrollmentCode = useCallback(async (request: CreateEnrollmentCodeRequest) => {
    const client = getClient();
    if (issueOwner.current === client || (enrollmentCode && enrollmentCode.expiresAtMs > Date.now())) return;
    issueOwner.current = client;
    setIssuingCode(true);
    try {
      const response = await client.createEnrollmentCode(request);
      if (clientRef.current === client) setEnrollmentCode(response);
    } catch (error) {
      if (clientRef.current === client && error instanceof HubApiError && error.status === 401) handleAuthFailure();
      throw error;
    } finally {
      if (issueOwner.current === client) {
        issueOwner.current = null;
        setIssuingCode(false);
      }
    }
  }, [getClient, handleAuthFailure, enrollmentCode]);

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
    themeMode,
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
    retrySelected,
    clearDetailError,

    enrollmentCode,
    issuingCode,
    createEnrollmentCode,
    rotateWorker,
    revokeWorker,
  };
}
