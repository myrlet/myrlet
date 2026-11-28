/**
 * In-panel load failure. The last known Task name and status stay in the
 * header; this only reports the failed read and offers a retry for the current
 * Task plus an explicit dismiss.
 */

import { AlertCircle, RotateCcw, X } from "lucide-react";

export interface DetailErrorNoticeProps {
  readonly message: string;
  readonly busy: boolean;
  readonly onRetry: () => void;
  readonly onDismiss: () => void;
}

export function DetailErrorNotice({
  message,
  busy,
  onRetry,
  onDismiss,
}: DetailErrorNoticeProps) {
  return (
    <div className="detail-error" role="alert" aria-label="Task load failed">
      <AlertCircle size={14} aria-hidden="true" />
      <div className="detail-error-body">
        <span className="detail-error-title">Could not load this Task</span>
        <span className="detail-error-message">{message}</span>
      </div>
      <div className="detail-error-actions">
        <button
          type="button"
          className="button button-sm"
          onClick={onRetry}
          disabled={busy}
          aria-busy={busy}
        >
          <RotateCcw size={12} aria-hidden="true" />
          <span>{busy ? "Retrying…" : "Retry"}</span>
        </button>
        <button
          type="button"
          className="icon-button"
          onClick={onDismiss}
          aria-label="Dismiss load failure"
          title="Dismiss load failure"
        >
          <X size={13} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}
