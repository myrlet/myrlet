/**
 * Local confirmation + request lifecycle for one enrolled Worker card.
 *
 * Rotate and Revoke are destructive, so each one arms an inline confirmation before any
 * Hub command runs. Apply is armed only after the report under review has been loaded, and
 * it submits exactly the revision that body was reported at — the pair comes from
 * `useWorkerConfigReview`, never from a separately-read revision. This hook keeps the
 * pending choice, the in-flight busy flag, the honest error, and the once-shown rotated
 * credential; the card owns only presentation. A response that lands after unmount is
 * dropped.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { RotateWorkerResponse } from "../../api/types";
import type { WorkerReview } from "../../state/useWorkerConfigReview";
import { describeError } from "./workerErrors";

export type PendingAction = "rotate" | "revoke" | "apply" | null;

interface UseWorkerActionsOptions {
  readonly workerId: string;
  /** Latest revision the Hub accepted, used to reload the report under review. */
  readonly reportedRevision: number | null;
  /** The report under review, bound to the revision it was reported at. */
  readonly review: WorkerReview;
  readonly onApproveWorker: (workerId: string) => Promise<void>;
  readonly onApplyWorkerConfig: (workerId: string, revision: number) => Promise<void>;
  readonly onRotateWorker: (workerId: string) => Promise<RotateWorkerResponse>;
  readonly onRevokeWorker: (workerId: string) => Promise<void>;
}

export function useWorkerActions({
  workerId,
  reportedRevision,
  review,
  onApproveWorker,
  onApplyWorkerConfig,
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

  /**
   * Reload the latest report, then arm Apply for exactly the revision it belongs to.
   * A reload that fails leaves Apply unavailable; the Admin retries when the Hub is back.
   */
  const reviewNow = useCallback(async (): Promise<void> => {
    if (reportedRevision === null) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const loaded = await review.load(reportedRevision);
      if (mountedRef.current && loaded !== null) {
        setPending("apply");
      }
    } finally {
      if (mountedRef.current) {
        setBusy(false);
      }
    }
  }, [reportedRevision, review]);

  const apply = useCallback(async (): Promise<void> => {
    const pair = review.reviewed;
    if (pair === null) {
      return;
    }
    const applied = await run(() => onApplyWorkerConfig(workerId, pair.revision));
    if (applied && mountedRef.current) {
      setPending(null);
      review.clear();
    }
  }, [onApplyWorkerConfig, review, run, workerId]);

  return {
    pending,
    busy,
    error,
    rotatedCredential,
    request,
    cancel,
    rotate,
    revoke,
    approve,
    review: reviewNow,
    apply,
  };
}
