/**
 * Workers &amp; Pairing slide-over drawer.
 *
 * It lays over the still-mounted workspace so the Board or List keeps its
 * position. Escape closes it through the workspace keyboard priority; the
 * backdrop, its own button, and a wrapped Tab cycle all stay inside the panel.
 * Focus returns to the opener, with the App Bar pill as the fallback when
 * a Command Palette option has already unmounted.
 * The drawer owns its header and refresh; the workspace supplies the session
 * facts and Worker commands.
 */

import type { RefObject } from "react";
import { X } from "lucide-react";

import { useModalFocus } from "../state/useModalFocus";
import { WorkersView } from "./WorkersView";
import type { WorkersViewProps } from "./WorkersView";

type WorkersSurfaceProps = Pick<
  WorkersViewProps,
  | "baseUrl"
  | "workers"
  | "reported"
  | "issued"
  | "busy"
  | "onCreateEnrollmentCode"
  | "onLoadReported"
  | "onApproveWorker"
  | "onApplyWorkerConfig"
  | "onRotateWorker"
  | "onRevokeWorker"
  | "onOpenPairing"
  | "onCopyText"
>;

export interface WorkersDrawerProps extends WorkersSurfaceProps {
  readonly onlineWorkerCount: number;
  readonly onRefresh: () => void;
  readonly onClose: () => void;
  /** Fallback for close focus: the App Bar pill, used when the real opener is gone. */
  readonly returnFocusRef: RefObject<HTMLElement | null>;
}

export function WorkersDrawer(props: WorkersDrawerProps) {
  const {
    onlineWorkerCount,
    baseUrl,
    workers,
    reported,
    issued,
    busy,
    onRefresh,
    onClose,
    onCreateEnrollmentCode,
    onLoadReported,
    onApproveWorker,
    onApplyWorkerConfig,
    onRotateWorker,
    onRevokeWorker,
    onOpenPairing,
    onCopyText,
    returnFocusRef,
  } = props;

  const { panelRef, onKeyDown } = useModalFocus({
    open: true,
    fallbackRef: returnFocusRef,
  });

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
        ref={panelRef}
        className="workers-drawer-panel"
        role="dialog"
        aria-modal="true"
        aria-label="Workers and pairing"
        tabIndex={-1}
        onKeyDown={onKeyDown}
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
            reported={reported}
            onCreateEnrollmentCode={onCreateEnrollmentCode}
            onLoadReported={onLoadReported}
            onApproveWorker={onApproveWorker}
            onApplyWorkerConfig={onApplyWorkerConfig}
            onRotateWorker={onRotateWorker}
            onRevokeWorker={onRevokeWorker}
            onOpenPairing={onOpenPairing}
            onCopyText={onCopyText}
          />
        </div>
      </aside>
    </div>
  );
}
