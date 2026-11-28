/**
 * The scheduling report one Administrator is reviewing for one Worker.
 *
 * Apply exists so an Admin admits exactly the report they inspected, so the report body
 * and the revision submitted to the Hub must come from the same read. This hook keeps them
 * together as one value and refuses a response that belongs to an older read, so a slow
 * reply can never replace newer reviewed content. When the Hub's reported revision moves,
 * the stale pair is dropped so the Admin must load the newer report; the Hub's own
 * revision check still guards every Apply request.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { WorkerSchedulingConfig } from "../api/types";

/** One reviewed report and the exact revision it was reported at. */
export interface ReviewedConfig {
  readonly revision: number;
  readonly config: WorkerSchedulingConfig;
}

/** One Worker's reviewed report plus the lifecycle of the read that fills it. */
export interface WorkerReview {
  readonly reviewed: ReviewedConfig | null;
  readonly loading: boolean;
  readonly error: string | null;
  /**
   * Load one revision's report now. The returned pair is the same value stored in
   * `reviewed`; `null` means the read failed or there was nothing to review.
   */
  readonly load: (revision: number) => Promise<ReviewedConfig | null>;
  /** Discard the reviewed pair (e.g. after a successful Apply). */
  readonly clear: () => void;
}

interface UseWorkerConfigReviewOptions {
  readonly workerId: string;
  /** Latest revision the Snapshot says the Hub accepted, or `null` before one. */
  readonly reportedRevision: number | null;
  /** Read one Worker's detail; its `reportedConfig` is the report to review. */
  readonly fetchReported: (workerId: string) => Promise<WorkerSchedulingConfig | null>;
}

export function useWorkerConfigReview({
  workerId,
  reportedRevision,
  fetchReported,
}: UseWorkerConfigReviewOptions): WorkerReview {
  const [reviewed, setReviewed] = useState<ReviewedConfig | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Bumped on every load; a response whose token is stale is dropped. */
  const tokenRef = useRef(0);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const load = useCallback(
    async (revision: number): Promise<ReviewedConfig | null> => {
      const token = ++tokenRef.current;
      // Clear the previous pair before the read so the view never shows one revision's
      // body under another revision's label while this request is outstanding.
      setReviewed(null);
      setLoading(true);
      setError(null);
      try {
        const config = await fetchReported(workerId);
        if (!mountedRef.current || tokenRef.current !== token) {
          return null; // an older response never replaces newer reviewed content
        }
        if (config === null) {
          setError("The Hub has no report to review for this Worker yet.");
          return null;
        }
        const pair: ReviewedConfig = { revision, config };
        setReviewed(pair);
        return pair;
      } catch (loadError) {
        if (!mountedRef.current || tokenRef.current !== token) {
          return null;
        }
        setError(
          loadError instanceof Error
            ? loadError.message
            : "The report under review could not be loaded.",
        );
        return null;
      } finally {
        if (mountedRef.current && tokenRef.current === token) {
          setLoading(false);
        }
      }
    },
    [fetchReported, workerId],
  );

  // The Snapshot is the authority on which revision is current. When it moves past the
  // reviewed one, the body under review no longer matches what Apply would submit, so drop
  // it and let the card load the newer report.
  useEffect(() => {
    if (tokenRef.current === 0) {
      return;
    }
    setReviewed((current) =>
      current !== null && current.revision !== reportedRevision ? null : current,
    );
  }, [reportedRevision]);

  const clear = useCallback(() => {
    tokenRef.current += 1;
    setReviewed(null);
    setError(null);
    setLoading(false);
  }, []);

  return { reviewed, loading, error, load, clear };
}
