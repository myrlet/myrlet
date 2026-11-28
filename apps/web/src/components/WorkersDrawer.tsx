/**
 * Workers &amp; Pairing slide-over drawer.
 *
 * It lays over the still-mounted workspace so the Board or List keeps its
 * position, and closes on the backdrop, its own button, or Escape (handled by
 * the workspace keyboard priority). The drawer owns its header and refresh; the
 * workspace supplies the session facts and Worker commands.
 */

import { X } from "lucide-react";

import { WorkersView } from "./WorkersView";
import type { WorkersViewProps } from "./WorkersView";

type WorkersSurfaceProps = Pick<
  WorkersViewProps,
  | "baseUrl"
  | "workers"
  | "issued"
  | "busy"
  | "onCreateEnrollmentCode"
  | "onRotateWorker"
  | "onRevokeWorker"
  | "onCopyText"
>;

export interface WorkersDrawerProps extends WorkersSurfaceProps {
  readonly onlineWorkerCount: number;
  readonly onRefresh: () => void;
  readonly onClose: () => void;
}

export function WorkersDrawer(props: WorkersDrawerProps) {
  const {
    onlineWorkerCount,
    baseUrl,
    workers,
    issued,
    busy,
    onRefresh,
    onClose,
    onCreateEnrollmentCode,
    onRotateWorker,
    onRevokeWorker,
    onCopyText,
  } = props;

  return (
    <div
      className="workers-drawer-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <aside
        className="workers-drawer-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Workers and pairing"
      >
        <header className="workers-drawer-header">
          <div className="workers-drawer-heading">
            <h2 className="workers-drawer-title">Workers &amp; Pairing</h2>
            <span className="view-subtitle">
              {onlineWorkerCount} of {workers.length} online
            </span>
          </div>
          <button type="button" className="button button-sm" onClick={onRefresh}>
            Refresh
          </button>
          <button
            type="button"
            className="icon-button"
            aria-label="Close workers drawer"
            onClick={onClose}
          >
            <X size={15} aria-hidden="true" />
          </button>
        </header>
        <div className="workers-drawer-body">
          <WorkersView
            issued={issued}
            busy={busy}
            baseUrl={baseUrl}
            workers={workers}
            onCreateEnrollmentCode={onCreateEnrollmentCode}
            onRotateWorker={onRotateWorker}
            onRevokeWorker={onRevokeWorker}
            onCopyText={onCopyText}
          />
        </div>
      </aside>
    </div>
  );
}
