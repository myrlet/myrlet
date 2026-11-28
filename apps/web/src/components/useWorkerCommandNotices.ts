/**
 * Operator-facing Worker command wrappers for the workspace.
 *
 * The Hub session already owns the mutations; this hook adds only what the workspace
 * adds on top of them — a toast once a command succeeds, and an inline busy/error pair
 * for the Security & Trust settings edit. Keeping it out of `AuthenticatedWorkspace`
 * leaves that component to the view shell.
 */

import { useCallback, useState } from "react";

import { HubApiError } from "../api/client";
import type { UpdateSecurityTrustRequest } from "../api/types";
import type { RotateWorkerResponse } from "../api/types";

/** One Hub failure as short operator-facing text. */
export function describeHubError(error: unknown): string {
  if (error instanceof HubApiError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "The Hub command could not be completed.";
}

interface WorkerCommandSource {
  readonly refresh: () => Promise<void>;
  readonly rotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly revokeWorker: (workerId: string) => Promise<void>;
  readonly approveWorker: (workerId: string) => Promise<void>;
  readonly applyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly updateSecurityTrust: (
    request: UpdateSecurityTrustRequest,
  ) => Promise<unknown>;
}

export function useWorkerCommandNotices(
  hub: WorkerCommandSource,
  notify: (message: string, tone?: "neutral" | "error") => void,
) {
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    void hub.refresh();
  }, [hub.refresh]);

  const rotateWorker = useCallback(
    async (workerId: string) => {
      const response = await hub.rotateWorker(workerId);
      notify("Credential rotated");
      return response;
    },
    [hub.rotateWorker, notify],
  );

  const revokeWorker = useCallback(
    async (workerId: string): Promise<void> => {
      await hub.revokeWorker(workerId);
      notify("Worker revoked");
    },
    [hub.revokeWorker, notify],
  );

  const approveWorker = useCallback(
    async (workerId: string): Promise<void> => {
      await hub.approveWorker(workerId);
      notify("Worker approved");
    },
    [hub.approveWorker, notify],
  );

  const applyWorkerConfig = useCallback(
    async (workerId: string, revision: number): Promise<void> => {
      await hub.applyWorkerConfig(workerId, revision);
      notify(`Applied revision ${revision}`);
    },
    [hub.applyWorkerConfig, notify],
  );

  const changeSecurityTrust = useCallback(
    async (request: UpdateSecurityTrustRequest): Promise<void> => {
      setSettingsBusy(true);
      setSettingsError(null);
      try {
        await hub.updateSecurityTrust(request);
      } catch (trustError) {
        setSettingsError(describeHubError(trustError));
        throw trustError;
      } finally {
        setSettingsBusy(false);
      }
    },
    [hub.updateSecurityTrust],
  );

  return {
    refresh,
    rotateWorker,
    revokeWorker,
    approveWorker,
    applyWorkerConfig,
    changeSecurityTrust,
    settingsBusy,
    settingsError,
  };
}
