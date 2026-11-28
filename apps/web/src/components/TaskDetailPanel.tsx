/**
 * Read-only Task Detail pane with four segmented tabs: Overview, Tree & Deps,
 * Delivery, and JSON. Every write entry point (Dispatch / Finish / Cancel /
 * Subtask / Copy-Retry) belongs to the CLI, not this UI.
 *
 * The panel orchestrates: it owns the tab, the Result reader overlay and its
 * focus/scroll restoration, the failure notice, and the header. Each tab is a
 * small view with narrow props (`./taskDetail/`), and the ancestry/mini-tree
 * projections are React-free (`./taskDetail/projections`).
 *
 * Facts come from the Hub and the Harness only. `finished` reads "Handling
 * ended", cancellation states such as `uncertain` stay visible, and a missing
 * Progress or Delivery is stated explicitly instead of being guessed.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Inbox, RotateCcw } from "lucide-react";

import type { SnapshotTaskSummary, TaskRecord, TaskTreeResponse } from "../api/types";
import { ResultReader } from "./ResultReader";
import { DetailErrorNotice } from "./taskDetail/DetailErrorNotice";
import {
  AncestryBreadcrumb,
  LoadedDetailHeader,
  PendingDetailHeader,
} from "./taskDetail/DetailHeader";
import { buildAncestry } from "./taskDetail/projections";
import type { DetailTab } from "./taskDetail/views";
import { DETAIL_TABS, DetailTabBody } from "./taskDetail/views";

export interface TaskDetailPanelProps {
  readonly selectedTaskId: string | null;
  readonly summary: SnapshotTaskSummary | null;
  readonly task: TaskRecord | null;
  readonly tree: TaskTreeResponse | null;
  readonly loading: boolean;
  readonly error: string | null;
  readonly onClose: () => void;
  readonly onSelectTask: (taskId: string) => void;
  readonly onCopyText: (label: string, text: string) => void;
  /** Re-read the current selection without dropping what is already shown. */
  readonly onRetry: () => void;
  /** Dismiss the failure notice and keep the panel open. */
  readonly onDismissError: () => void;
}

export function TaskDetailPanel(props: TaskDetailPanelProps) {
  const {
    selectedTaskId,
    summary,
    task,
    tree,
    loading,
    error,
    onClose,
    onSelectTask,
    onCopyText,
    onRetry,
    onDismissError,
  } = props;

  const [tab, setTab] = useState<DetailTab>("overview");

  // Result reader: modally over the Inspector, without touching the tab or the
  // Overview reading position. Closing restores both focus and scroll.
  const [readerOpen, setReaderOpen] = useState(false);
  const detailBodyRef = useRef<HTMLDivElement | null>(null);
  const readerTriggerRef = useRef<HTMLButtonElement | null>(null);
  const readerScrollRef = useRef(0);
  const readerWasOpenRef = useRef(false);

  const openReader = useCallback(() => {
    readerScrollRef.current = detailBodyRef.current?.scrollTop ?? 0;
    readerWasOpenRef.current = true;
    setReaderOpen(true);
  }, []);

  const closeReader = useCallback(() => {
    setReaderOpen(false);
  }, []);

  useLayoutEffect(() => {
    if (readerOpen || !readerWasOpenRef.current) {
      return;
    }
    readerWasOpenRef.current = false;
    const body = detailBodyRef.current;
    if (body !== null) {
      body.scrollTop = readerScrollRef.current;
    }
    if (readerTriggerRef.current?.isConnected) {
      // Focusing first would scroll the trigger into view and discard the
      // restored reading position in a real browser.
      readerTriggerRef.current.focus({ preventScroll: true });
      if (body !== null) {
        body.scrollTop = readerScrollRef.current;
      }
    }
  }, [readerOpen]);

  const selectionId = selectedTaskId ?? task?.id ?? null;

  // A new Task selection always starts on Overview.
  useEffect(() => {
    setTab("overview");
  }, [selectionId]);

  const ancestry = useMemo(
    () => (task === null ? [] : buildAncestry(task, tree)),
    [task, tree],
  );

  if (selectionId === null) {
    return (
      <aside className="detail-pane" aria-label="Task detail">
        <div className="pane-empty">
          <Inbox size={20} aria-hidden="true" />
          <span className="pane-empty-title">No Task selected</span>
          <span>Select a Task to inspect its Overview, Tree &amp; Deps, and Delivery.</span>
        </div>
      </aside>
    );
  }

  if (task === null) {
    // Snapshot facts are enough to keep the header honest while the full read
    // loads or after it failed: the Task never appears to be unselected.
    return (
      <aside className="detail-pane" aria-label="Task detail">
        <PendingDetailHeader
          title={summary?.title || summary?.id || selectionId}
          status={summary?.status ?? null}
          taskId={selectionId}
          version={summary?.version ?? null}
          loading={loading}
          onClose={onClose}
        />
        <div className="detail-body">
          {error === null ? (
            loading ? (
              <div className="loading-state" role="status">
                Loading Task…
              </div>
            ) : (
              <div className="detail-fallback">
                <p className="detail-empty">Details are not loaded.</p>
                <button type="button" className="button button-sm" onClick={onRetry}>
                  <RotateCcw size={12} aria-hidden="true" />
                  <span>Load Task</span>
                </button>
              </div>
            )
          ) : (
            <DetailErrorNotice
              message={error}
              busy={loading}
              onRetry={onRetry}
              onDismiss={onDismissError}
            />
          )}
        </div>
      </aside>
    );
  }

  return (
    <aside className="detail-pane" aria-label="Task detail">
      <LoadedDetailHeader
        title={task.task.title}
        status={task.status}
        taskId={task.id}
        version={task.version}
        loading={loading}
        copyId={() => onCopyText("Task ID", task.id)}
        breadcrumb={
          <AncestryBreadcrumb
            ancestry={ancestry.map((ancestor) => ({
              id: ancestor.id,
              title: ancestor.task.title,
            }))}
            onSelectTask={onSelectTask}
          />
        }
        onClose={onClose}
      />

      <div className="detail-tabs" role="tablist" aria-label="Task detail sections">
        {DETAIL_TABS.map((entry) => (
          <button
            key={entry.id}
            type="button"
            role="tab"
            id={`detail-tab-${entry.id}`}
            className="detail-tab"
            data-active={tab === entry.id}
            aria-selected={tab === entry.id}
            aria-controls="detail-tabpanel"
            tabIndex={tab === entry.id ? 0 : -1}
            onClick={() => setTab(entry.id)}
          >
            {entry.label}
          </button>
        ))}
      </div>

      <div
        className="detail-body"
        id="detail-tabpanel"
        role="tabpanel"
        aria-labelledby={`detail-tab-${tab}`}
        ref={detailBodyRef}
      >
        {error === null ? null : (
          <DetailErrorNotice
            message={error}
            busy={loading}
            onRetry={onRetry}
            onDismiss={onDismissError}
          />
        )}

        <DetailTabBody
          tab={tab}
          task={task}
          tree={tree}
          onSelectTask={onSelectTask}
          onCopyText={onCopyText}
          onOpenReader={openReader}
          readerTriggerRef={readerTriggerRef}
        />
      </div>

      {readerOpen && task.resultMarkdown !== null ? (
        <ResultReader
          title={task.task.title}
          markdown={task.resultMarkdown}
          recordedAtMs={task.resultRecordedAtMs}
          onCopyText={onCopyText}
          onRequestClose={closeReader}
        />
      ) : null}
    </aside>
  );
}
