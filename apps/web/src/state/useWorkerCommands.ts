/**
 * Worker administration commands for the authenticated Hub session.
 *
 * These are ordinary Admin mutations: each one runs on the live session client, asks
 * the session to refresh the Snapshot, and surfaces its error so a dialog can show it
 * inline. The enrollment code is the one piece of Worker state this hook owns; the
 * reviewed scheduling report lives with `useWorkerConfigReview`, so a report body and
 * its revision are never split across hooks.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import { HubApiError } from "../api/client";
import type { HubClient } from "../api/client";
import type {
  CreateEnrollmentCodeRequest,
  CreateEnrollmentCodeResponse,
  RotateWorkerResponse,
  WorkerDetail,
} from "../api/types";

export interface WorkerCommands {
  /** Issue a one-time pairing code; a live code is reused rather than reissued. */
  readonly createEnrollmentCode: (
    request: CreateEnrollmentCodeRequest,
  ) => Promise<void>;
  readonly rotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly revokeWorker: (workerId: string) => Promise<void>;
  readonly approveWorker: (workerId: string) => Promise<void>;
  readonly applyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  /**
   * One Worker's detail. Its `reportedRevision` and `reportedConfig` come from the same
   * response, so a review can never pair an older revision with a newer body.
   */
  readonly fetchWorkerDetail: (workerId: string) => Promise<WorkerDetail>;
  /** The current one-time enrollment code, or `null` once it is spent or expired. */
  readonly enrollmentCode: CreateEnrollmentCodeResponse | null;
  readonly issuingCode: boolean;
}

interface UseWorkerCommandsOptions {
  /** The live session client, or `null` while no Hub session exists. */
  readonly clientRef: { readonly current: HubClient | null };
  /** Bumped whenever session view data is cleared; drops the code and its owner. */
  readonly viewGeneration: number;
  /** Run one mutation and refresh the Snapshot; rethrown to the caller on failure. */
  readonly mutate: <T>(operation: (client: HubClient) => Promise<T>) => Promise<T>;
  /** Clear the session when a command proves its token was rejected. */
  readonly failSession: (client: HubClient) => void;
}

export function useWorkerCommands({
  clientRef,
  viewGeneration,
  mutate,
  failSession,
}: UseWorkerCommandsOptions): WorkerCommands {
  const [enrollmentCode, setEnrollmentCode] = useState<CreateEnrollmentCodeResponse | null>(null);
  const [issuingCode, setIssuingCode] = useState(false);
  const issueOwner = useRef<HubClient | null>(null);

  const getClient = useCallback((): HubClient => {
    const client = clientRef.current;
    if (!client) {
      throw new Error("Not connected to a Myrlet Hub.");
    }
    return client;
  }, [clientRef]);

  useEffect(() => {
    if (!enrollmentCode) return;
    const timer = window.setTimeout(
      () => setEnrollmentCode(null),
      Math.max(0, enrollmentCode.expiresAtMs - Date.now()),
    );
    return () => window.clearTimeout(timer);
  }, [enrollmentCode]);

  // A cleared session starts with no code and no issuance in flight.
  useEffect(() => {
    issueOwner.current = null;
    setIssuingCode(false);
    setEnrollmentCode(null);
  }, [viewGeneration]);

  const createEnrollmentCode = useCallback(
    async (request: CreateEnrollmentCodeRequest) => {
      const client = getClient();
      if (
        issueOwner.current === client ||
        (enrollmentCode && enrollmentCode.expiresAtMs > Date.now())
      ) {
        return;
      }
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
    },
    [clientRef, enrollmentCode, failSession, getClient],
  );

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
    async (workerId: string, revision: number): Promise<void> => {
      await mutate((client) => client.applyWorkerConfig(workerId, revision));
    },
    [mutate],
  );

  /** Read one Worker's detail; the report and its revision are one response. */
  const fetchWorkerDetail = useCallback(
    async (workerId: string): Promise<WorkerDetail> => {
      const client = getClient();
      try {
        return await client.getWorker(workerId);
      } catch (loadError) {
        if (loadError instanceof HubApiError && loadError.status === 401) {
          failSession(client);
        }
        throw loadError;
      }
    },
    [failSession, getClient],
  );

  return {
    createEnrollmentCode,
    rotateWorker,
    revokeWorker,
    approveWorker,
    applyWorkerConfig,
    fetchWorkerDetail,
    enrollmentCode,
    issuingCode,
  };
}
