/**
 * The scheduling report one Administrator is reviewing for one Worker.
 *
 * Apply exists so an Admin admits exactly the report they inspected. `reportedRevision`
 * and `reportedConfig` must therefore come from the *same* response — a revision read from
 * an older Snapshot paired with a body from a newer Detail response would let the Admin
 * apply a revision different from what they saw. This hook loads both from one
 * `GET /v1/workers/{id}` read and stores them as a single pair, refuses a response that a
 * newer read superseded, and drops the pair when the Snapshot revision moves past it. The
 * Hub's own revision-mismatch rejection still guards every Apply.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { WorkerDetail, WorkerSchedulingConfig } from "../api/types";

/** One reviewed report and the exact revision it was reported at, from one read. */
export interface ReviewedConfig {
  readonly revision: number;
  readonly config: WorkerSchedulingConfig;
}

/** One Worker's reviewed report plus the lifecycle of the read that fills it. */
export interface WorkerReview {
  readonly reviewed: ReviewedConfig | null;
  readonly loading: boolean;
  readonly error: string | null;
  /** Load the report now, binding the body to the revision of that same response. */
  readonly load: () => Promise<ReviewedConfig | null>;
  /** Discard the reviewed pair (e.g. after a successful Apply). */
  readonly clear: () => void;
}

interface UseWorkerConfigReviewOptions {
  readonly workerId: string;
  /** Latest revision the Snapshot says the Hub accepted, or `null` before one. */
  readonly reportedRevision: number | null;
  /** Read one Worker's detail; both fields below come from that single response. */
  readonly fetchDetail: (workerId: string) => Promise<WorkerDetail>;
}

export function useWorkerConfigReview({
  workerId,
  reportedRevision,
  fetchDetail,
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

  const load = useCallback(async (): Promise<ReviewedConfig | null> => {
    const token = ++tokenRef.current;
    // Clear the previous pair before the read so the view never shows one revision's
    // body under another revision's label while this request is outstanding.
    setReviewed(null);
    setLoading(true);
    setError(null);
    try {
      const detail = await fetchDetail(workerId);
      if (!mountedRef.current || tokenRef.current !== token) {
        return null; // an older response never replaces newer reviewed content
      }
      // Revision and body are read from the same response; neither is taken from the
      // Snapshot or stored separately, so they can never describe different reports.
      const revision = detail.worker.reportedRevision;
      const config = detail.reportedConfig;
      if (revision === null || config === null) {
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
  }, [fetchDetail, workerId]);

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
