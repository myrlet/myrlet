/**
 * Hub session state for the Myrlet Web UI.
 *
 * Ownership rules:
 * - Appearance (`localStorage["myrlet.theme"]`) is owned by `useAppearance`;
 *   selected Task detail by `useTaskDetail`. Board layout offsets are owned by
 *   `TaskGraphView` (`localStorage["myrlet.atlas.positions"]`).
 * - This hook owns the authenticated session, Snapshot, SSE loop, and Worker
 *   commands, and composes session cleanup for the hooks above.
 * - The Admin bearer token lives exclusively in memory (React ref/state) and is
 *   never written to storage, URLs, or logs.
 * - SSE frames are invalidation hints. On each hint we debounce a lightweight
 *   Snapshot refresh (plus a silent selected Task refresh) rather than trusting
 *   the frame payload as state.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { HubApiError, HubClient } from "../api/client";
import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  GlobalSnapshot,
  HubEventEnvelope,
  SecurityTrustSettings,
  SnapshotTaskSummary,
  UpdateSecurityTrustRequest,
  WorkerRecord,
  WorkerSchedulingConfig,
} from "../api/types";
import { useTaskDetail } from "./useTaskDetail";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "reconnecting";

/** Non-secret session identity; the token is intentionally not exposed. */
export interface HubSession {
  readonly baseUrl: string;
}

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
 * Owns the authenticated Hub session, Snapshot, SSE loop, and Worker commands.
 * Appearance and selected Task detail live in their own hooks. Mount exactly
 * once at the app root.
 */
export function useHub() {
  const [enrollmentCode, setEnrollmentCode] = useState<CreateEnrollmentCodeResponse | null>(null);
  const [issuingCode, setIssuingCode] = useState(false);
  const issueOwner = useRef<HubClient | null>(null);
  const [connection, setConnection] = useState<ConnectionStatus>("disconnected");
  const [baseUrl, setBaseUrl] = useState<string>(defaultBaseUrl);
  const [session, setSession] = useState<HubSession | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [snapshot, setSnapshot] = useState<GlobalSnapshot | null>(null);
  const [eventCursor, setEventCursor] = useState(0);
  const [lastSyncedAtMs, setLastSyncedAtMs] = useState<number | null>(null);
  /** Latest reported scheduling snapshot per Worker, for review before Apply. */
  const [reportedConfigs, setReportedConfigs] = useState<
    Readonly<Record<string, WorkerSchedulingConfig | null>>
  >({});
  const [settings, setSettings] = useState<SecurityTrustSettings | null>(null);

  const clientRef = useRef<HubClient | null>(null);
  const streamRef = useRef<AbortController | null>(null);
  const refreshTimerRef = useRef<number | null>(null);
  const cursorRef = useRef(0);

  const getClient = useCallback((): HubClient => {
    const client = clientRef.current;
    if (!client) {
      throw new Error("Not connected to a Myrlet Hub.");
    }
    return client;
  }, []);

  /** Drop every non-detail fact tied to the previous Hub session. */
  const clearViewData = useCallback(() => {
    issueOwner.current = null;
    setIssuingCode(false);
    setEnrollmentCode(null);
    cursorRef.current = 0;
    setSnapshot(null);
    setEventCursor(0);
    setLastSyncedAtMs(null);
    setReportedConfigs({});
    setSettings(null);
  }, []);

  /**
   * Clear the session only if `client` is still the live one, so a late 401
   * from a replaced session can never sign out the current one.
   */
  const handleAuthFailure = useCallback(
    (client: HubClient) => {
      if (clientRef.current !== client) {
        return false;
      }
      clientRef.current = null;
      streamRef.current?.abort();
      streamRef.current = null;
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
        refreshTimerRef.current = null;
      }
      clearViewData();
      setSession(null);
      setConnection("disconnected");
      setError("The Hub rejected this Admin Bearer Token. Check the token and connect again.");
      return true;
    },
    [clearViewData],
  );

  const { resetDetail, refreshSelected, ...selection } = useTaskDetail({
    clientRef,
    onAuthFailure: handleAuthFailure,
  });

  /** Session-fatal failure raised outside the detail read: clear both sides. */
  const failSession = useCallback(
    (client: HubClient) => {
      if (handleAuthFailure(client)) {
        resetDetail();
      }
    },
    [handleAuthFailure, resetDetail],
  );

  const resetViewState = useCallback(() => {
    clearViewData();
    resetDetail();
  }, [clearViewData, resetDetail]);

  const applySnapshot = useCallback((next: GlobalSnapshot) => {
    cursorRef.current = next.eventCursor;
    setSnapshot(next);
    setEventCursor(next.eventCursor);
    setLastSyncedAtMs(Date.now());
  }, []);

  const reloadSnapshot = useCallback(
    async (client: HubClient): Promise<boolean> => {
      try {
        const next = await client.getSnapshot();
        if (clientRef.current !== client) {
          return false;
        }
        applySnapshot(next);
        // The policies are session-wide facts; a read failure leaves the last pair visible.
        try {
          const stored = await client.getSecurityTrust();
          if (clientRef.current === client) {
            setSettings(stored);
          }
        } catch {
          // Keep the previous policies; the next refresh retries.
        }
        return true;
      } catch (reloadError) {
        if (clientRef.current !== client) {
          return false;
        }
        if (reloadError instanceof HubApiError && reloadError.status === 401) {
          failSession(client);
        }
        // A transient read failure leaves the last Snapshot in place; the SSE
        // reconnect loop will retry. Snapshot is never fabricated.
        return false;
      }
    },
    [applySnapshot, failSession],
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
    await refreshSelected();
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
            failSession(client);
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
              void refreshSelected();
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
    [failSession, handleEvent, refreshSelected, reloadSnapshot],
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
        try {
          const stored = await client.getSecurityTrust();
          if (clientRef.current === client) {
            setSettings(stored);
          }
        } catch {
          // The connection itself succeeded; policies refresh with the next Snapshot.
        }
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
          failSession(client);
        }
        throw mutationError;
      }
    },
    [failSession, getClient, refreshAll],
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
      if (clientRef.current === client && error instanceof HubApiError && error.status === 401) {
        failSession(client);
      }
      throw error;
    } finally {
      if (issueOwner.current === client) {
        issueOwner.current = null;
        setIssuingCode(false);
      }
    }
  }, [failSession, getClient, enrollmentCode]);

  const rotateWorker = useCallback(
    (workerId: string) => mutate((client) => client.rotateWorker(workerId)),
    [mutate],
  );

  const revokeWorker = useCallback(
    (workerId: string) => mutate((client) => client.revokeWorker(workerId)),
    [mutate],
  );

  const approveWorker = useCallback(
    (workerId: string) => mutate((client) => client.approveWorker(workerId)),
    [mutate],
  );

  /** Apply exactly the revision the Admin reviewed, then refresh both sides. */
  const applyWorkerConfig = useCallback(
    (workerId: string, revision: number) =>
      mutate((client) => client.applyWorkerConfig(workerId, revision)),
    [mutate],
  );

  /** Read one Worker's latest reported snapshot for display before an Apply. */
  const loadReportedConfig = useCallback(
    async (workerId: string): Promise<void> => {
      const client = getClient();
      try {
        const detail = await client.getWorker(workerId);
        if (clientRef.current === client) {
          setReportedConfigs((current) => ({
            ...current,
            [workerId]: detail.reportedConfig,
          }));
        }
      } catch (loadError) {
        if (loadError instanceof HubApiError && loadError.status === 401) {
          failSession(client);
        }
        throw loadError;
      }
    },
    [failSession, getClient],
  );

  /** Merge a Security & Trust patch; the Hub echoes the stored pair. */
  const updateSecurityTrust = useCallback(
    (request: UpdateSecurityTrustRequest) =>
      mutate(async (client) => {
        const stored = await client.updateSecurityTrust(request);
        if (clientRef.current === client) {
          setSettings(stored);
        }
        return stored;
      }),
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

    ...selection,

    enrollmentCode,
    issuingCode,
    createEnrollmentCode,
    rotateWorker,
    revokeWorker,
    approveWorker,
    applyWorkerConfig,
    loadReportedConfig,
    reportedConfigs,
    settings,
    updateSecurityTrust,
  };
}
