/**
 * Small factual value cells shared by the Overview and Delivery views.
 *
 * Each one states a Hub fact or an explicit empty value; none of them guesses,
 * rounds, or derives a verdict.
 */

import type { ReactNode } from "react";

import { formatRelativeTime, formatTimestamp } from "../../format";

/** Absolute timestamp with its relative age; `—` when the fact is absent. */
export function TimestampValue({ ms }: { ms: number | null }) {
  if (ms == null) {
    return <span className="detail-value cell-empty">—</span>;
  }
  return (
    <span className="detail-value mono">
      {formatTimestamp(ms)} <span className="muted">({formatRelativeTime(ms)})</span>
    </span>
  );
}

/** Monospace identifier cell with an explicit empty state. */
export function IdValue({ id, empty = "—" }: { id: string | null; empty?: string }) {
  if (id == null || id === "") {
    return <span className="detail-value cell-empty">{empty}</span>;
  }
  return <span className="detail-value mono">{id}</span>;
}

export function MetaCell({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="meta-cell">
      <span className="meta-label">{label}</span>
      <span className="meta-value">{children}</span>
    </div>
  );
}
