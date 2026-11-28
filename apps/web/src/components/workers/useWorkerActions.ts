/**
 * Local confirmation + request lifecycle for one enrolled Worker card.
 *
 * Rotate and Revoke are destructive, so each one arms an inline confirmation
 * before any Hub command runs. Apply arms only after the report under review
 * has been loaded, and then pins the exact revision the Admin saw. The hook
 * keeps that pending choice, the reviewed revision, the in-flight busy flag,
 * the honest error, and the once-shown rotated credential; the card owns only
 * presentation. A response that lands after unmount is dropped.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { RotateWorkerResponse } from "../../api/types";
import { describeError } from "./workerErrors";

export type PendingAction = "rotate" | "revoke" | "apply" | null;

interface UseWorkerActionsOptions {
  readonly workerId: string;
  /** Latest report revision, or `null` before the Hub accepted one. */
  readonly reportedRevision: number | null;
  readonly onLoadReported: (workerId: string) => Promise<void>;
  readonly onApproveWorker: (workerId: string) => Promise<void>;
  readonly onApplyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
}

export function useWorkerActions({
  workerId,
  reportedRevision,
  onLoadReported,
  onApproveWorker,
  onApplyWorkerConfig,
  onRotateWorker,
  onRevokeWorker,
}: UseWorkerActionsOptions) {
  const [pending, setPending] = useState<PendingAction>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rotatedCredential, setRotatedCredential] = useState<string | null>(null);
  const [reviewedRevision, setReviewedRevision] = useState<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  /** Run one Hub command under the card's busy/error state. */
  const run = useCallback(
    async (operation: () => Promise<void>): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        await operation();
        return true;
      } catch (actionError) {
        if (mountedRef.current) {
          setError(describeError(actionError));
        }
        return false;
      } finally {
        if (mountedRef.current) {
          setBusy(false);
        }
      }
    },
    [],
  );

  /** Arm one confirmation and clear any previous failure wording. */
  const request = useCallback((action: Exclude<PendingAction, null>) => {
    setError(null);
    setPending(action);
  }, []);

  const cancel = useCallback(() => {
    setPending(null);
    setReviewedRevision(null);
  }, []);

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

  const approve = useCallback(async (): Promise<void> => {
    await run(() => onApproveWorker(workerId));
  }, [onApproveWorker, run, workerId]);

  /** Load the report under review, then arm Apply for exactly that revision. */
  const review = useCallback(async (): Promise<void> => {
    const loaded = await run(() => onLoadReported(workerId));
    if (loaded && mountedRef.current && reportedRevision !== null) {
      setReviewedRevision(reportedRevision);
      setPending("apply");
    }
  }, [onLoadReported, reportedRevision, run, workerId]);

  const apply = useCallback(async (): Promise<void> => {
    if (reviewedRevision === null) {
      return;
    }
    const revision = reviewedRevision;
    const applied = await run(() => onApplyWorkerConfig(workerId, revision));
    if (applied && mountedRef.current) {
      setPending(null);
      setReviewedRevision(null);
    }
  }, [onApplyWorkerConfig, reviewedRevision, run, workerId]);

  return {
    pending,
    busy,
    error,
    rotatedCredential,
    reviewedRevision,
    request,
    cancel,
    rotate,
    revoke,
    approve,
    review,
    apply,
  };
}
