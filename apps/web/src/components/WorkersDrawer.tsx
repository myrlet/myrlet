/**
 * Workers &amp; Pairing slide-over drawer.
 *
 * It lays over the still-mounted workspace so the Board or List keeps its
 * position. Escape closes it through the workspace keyboard priority; the
 * backdrop, its own button, and a wrapped Tab cycle all stay inside the panel.
 * On close, focus returns to the surface trigger — the App Bar pill — so a
 * drawer opened from ⌘K never lands on the vanished palette option or `body`.
 * The drawer owns its header and refresh; the workspace supplies the session
 * facts and Worker commands.
 */

import { useRef } from "react";
import type { RefObject } from "react";
import { X } from "lucide-react";

import { useModalFocus } from "../state/useModalFocus";
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
  /** The real surface trigger to restore focus to (App Bar pill, Manage, ⌘K). */
  readonly returnFocusRef: RefObject<HTMLElement | null>;
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
    returnFocusRef,
  } = props;

  const closeButtonRef = useRef<HTMLButtonElement | null>(null);
  const { panelRef, onKeyDown } = useModalFocus({
    open: true,
    fallbackRef: closeButtonRef,
    triggerRef: returnFocusRef,
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
            ref={closeButtonRef}
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
