/**
 * Local confirmation + request lifecycle for one enrolled Worker card.
 *
 * Rotate and Revoke are destructive, so each one arms an inline confirmation
 * before any Hub command runs. The hook keeps that pending choice, the in-flight
 * busy flag, the honest error, and the once-shown rotated credential; the card
 * owns only presentation. A response that lands after unmount is dropped.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { RotateWorkerResponse } from "../../api/types";
import { describeError } from "./workerErrors";

export type PendingAction = "rotate" | "revoke" | null;

interface UseWorkerActionsOptions {
  readonly workerId: string;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
}

export function useWorkerActions({
  workerId,
  onRotateWorker,
  onRevokeWorker,
}: UseWorkerActionsOptions) {
  const [pending, setPending] = useState<PendingAction>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rotatedCredential, setRotatedCredential] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  /** Arm one confirmation and clear any previous failure wording. */
  const request = useCallback((action: Exclude<PendingAction, null>) => {
    setError(null);
    setPending(action);
  }, []);

  const cancel = useCallback(() => setPending(null), []);

  const rotate = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const response = await onRotateWorker(workerId);
      if (mountedRef.current) {
        setRotatedCredential(response.credential);
        setPending(null);
      }
    } catch (rotateError) {
      if (mountedRef.current) {
        setError(describeError(rotateError));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [onRotateWorker, workerId]);

  const revoke = useCallback(async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await onRevokeWorker(workerId);
      if (mountedRef.current) {
        setPending(null);
        setRotatedCredential(null);
      }
    } catch (revokeError) {
      if (mountedRef.current) {
        setError(describeError(revokeError));
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [onRevokeWorker, workerId]);

  return { pending, busy, error, rotatedCredential, request, cancel, rotate, revoke };
}
