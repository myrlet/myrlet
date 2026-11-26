/**
 * Read-only Task Detail pane: Overview, Hierarchy, Delivery & Progress, and the
 * sticky footer that only opens explicit Dispatch / Finish / Cancel dialogs.
 *
 * Facts come from the Hub and the Harness only. `finished` reads "Handling
 * ended", cancellation states such as `uncertain` stay visible, and a missing
 * Progress or Delivery is stated explicitly instead of being guessed.
 */

import { AlertCircle, Inbox, X } from "lucide-react";

import type { TaskRecord, TaskTreeResponse } from "../api/types";
import {
  CANCEL_STATE_LABELS,
  DELIVERY_STATUS_LABELS,
  REJECTION_CODE_LABELS,
  canCancelTask,
  canDispatchTask,
  formatRelativeTime,
  formatTimestamp,
  isCoordinationFinishable,
} from "../format";
import { TaskStatusBadge } from "./StatusBadge";

export interface TaskDetailPanelProps {
  task: TaskRecord | null;
  tree: TaskTreeResponse | null;
  loading: boolean;
  error: string | null;
  onClose: () => void;
  onSelectTask: (taskId: string) => void;
  onDispatch: (task: TaskRecord) => void;
  onCancel: (task: TaskRecord) => void;
  onFinish: (task: TaskRecord) => void;
  onCopyRetry: (task: TaskRecord) => void;
}

/** Absolute timestamp with its relative age; `—` when the fact is absent. */
function TimestampValue({ ms }: { ms: number | null }) {
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
function IdValue({ id, empty = "—" }: { id: string | null; empty?: string }) {
  if (id == null || id === "") {
    return <span className="detail-value cell-empty">{empty}</span>;
  }
  return <span className="detail-value mono">{id}</span>;
}

export function TaskDetailPanel(props: TaskDetailPanelProps) {
  const {
    task,
    tree,
    loading,
    error,
    onClose,
    onSelectTask,
    onDispatch,
    onCancel,
    onFinish,
    onCopyRetry,
  } = props;

  if (task == null) {
    return (
      <aside className="detail-pane" aria-label="Task detail">
        <div className="pane-empty">
          {loading ? (
            <div className="loading-state">Loading Task…</div>
          ) : (
            <>
              <Inbox size={20} aria-hidden="true" />
              <span className="pane-empty-title">No Task selected</span>
              <span>Select a Task to inspect its Overview, Hierarchy, and Delivery.</span>
            </>
          )}
        </div>
        {error ? (
          <div className="detail-body">
            <div className="alert alert-error" role="alert">
              <AlertCircle size={14} aria-hidden="true" />
              <span>{error}</span>
            </div>
          </div>
        ) : null}
      </aside>
    );
  }

  const definition = task.task;
  const routing = definition.routing;
  const requiredCapabilities = routing.requiredCapabilities ?? [];
  const parentTaskId = definition.parentTaskId;
  const sourceTaskId = definition.sourceTaskId;
  const delivery = task.currentDelivery;
  const progress = delivery?.progress ?? null;
  const children = (tree?.tasks ?? []).filter(
    (candidate) => candidate.id !== task.id && candidate.task.parentTaskId === task.id,
  );

  return (
    <aside className="detail-pane" aria-label="Task detail">
      <header className="detail-header">
        <div className="detail-heading">
          <h2 className="detail-title">{definition.title}</h2>
          <div className="detail-subtitle">
            <TaskStatusBadge status={task.status} />
            <span className="mono" title={`Task ID ${task.id}`}>
              {task.id}
            </span>
            <span className="mono" title="Task version (optimistic concurrency)">
              v{task.version}
            </span>
            {loading ? <span>Refreshing…</span> : null}
          </div>
        </div>
        <button
          type="button"
          className="icon-button"
          onClick={onClose}
          aria-label="Close task detail"
          title="Close task detail"
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <div className="detail-body">
        {error ? (
          <div className="alert alert-error" role="alert">
            <AlertCircle size={14} aria-hidden="true" />
            <span>{error}</span>
          </div>
        ) : null}

        <section className="detail-section">
          <h3 className="detail-section-title">Overview</h3>
          <div className="detail-grid">
            <span className="detail-label">Category</span>
            <span className="detail-value">{definition.category === "" ? "—" : definition.category}</span>

            <span className="detail-label">Tags</span>
            <div className="detail-value">
              {definition.tags.length === 0 ? (
                <span className="cell-empty">—</span>
              ) : (
                <div className="detail-subtitle">
                  {definition.tags.map((tag) => (
                    <span className="tag" key={tag}>
                      {tag}
                    </span>
                  ))}
                </div>
              )}
            </div>

            <span className="detail-label">Created</span>
            <TimestampValue ms={task.createdAtMs} />

            <span className="detail-label">Updated</span>
            <TimestampValue ms={task.updatedAtMs} />

            <span className="detail-label">Handling ended</span>
            <TimestampValue ms={task.finishedAtMs} />

            <span className="detail-label">Required capabilities</span>
            <span className="detail-value">
              {requiredCapabilities.length === 0 ? (
                <span className="cell-empty">—</span>
              ) : (
                requiredCapabilities.join(", ")
              )}
            </span>

            <span className="detail-label">Preferred worker</span>
            <IdValue id={routing.preferredWorkerId ?? null} empty="— (any eligible Worker)" />

            <span className="detail-label">Preferred role</span>
            <IdValue id={routing.preferredRole ?? null} empty="— (any role)" />

            <span className="detail-label">Preferred executor</span>
            <IdValue id={routing.preferredExecutorId ?? null} empty="— (any executor)" />
          </div>
        </section>

        <section className="detail-section">
          <h3 className="detail-section-title">Prompt</h3>
          <pre className="detail-text" style={{ margin: 0 }}>
            {definition.prompt}
          </pre>
        </section>

        <section className="detail-section">
          <h3 className="detail-section-title">Hierarchy</h3>
          <div className="hierarchy">
            <div className="hierarchy-item">
              <span className="detail-label">Root</span>
              {task.rootTaskId === task.id ? (
                <>
                  <span className="hierarchy-current mono">{task.rootTaskId}</span>
                  <span className="hierarchy-sep">(this Task)</span>
                </>
              ) : (
                <button
                  type="button"
                  className="hierarchy-link mono"
                  onClick={() => onSelectTask(task.rootTaskId)}
                  title="Open root Task"
                >
                  {task.rootTaskId}
                </button>
              )}
            </div>

            <div className="hierarchy-item">
              <span className="detail-label">Parent</span>
              {parentTaskId == null ? (
                <span className="cell-empty">— (root Task)</span>
              ) : (
                <button
                  type="button"
                  className="hierarchy-link mono"
                  onClick={() => onSelectTask(parentTaskId)}
                  title="Open parent Task"
                >
                  {parentTaskId}
                </button>
              )}
            </div>

            <div className="hierarchy-item">
              <span className="detail-label">Depth</span>
              <span className="tabular">{task.depth}</span>
              {task.depth === 0 ? <span className="hierarchy-sep">(root)</span> : null}
            </div>

            <div className="hierarchy-item">
              <span className="detail-label">Source Task</span>
              {sourceTaskId == null ? (
                <span className="cell-empty">—</span>
              ) : (
                <button
                  type="button"
                  className="hierarchy-link mono"
                  onClick={() => onSelectTask(sourceTaskId)}
                  title="Open the Task this one was copied from"
                >
                  {sourceTaskId}
                </button>
              )}
            </div>

            <div className="hierarchy-item">
              <span className="detail-label">Dependencies</span>
              {definition.dependencies.length === 0 ? (
                <span className="cell-empty">—</span>
              ) : (
                <span className="detail-subtitle">
                  {definition.dependencies.map((dependencyId) => (
                    <button
                      type="button"
                      className="tag"
                      key={dependencyId}
                      onClick={() => onSelectTask(dependencyId)}
                      title={`Open dependency ${dependencyId}`}
                    >
                      {dependencyId}
                    </button>
                  ))}
                </span>
              )}
            </div>

            <div className="hierarchy-item">
              <span className="detail-label">Children</span>
              <span className="cell-muted">
                {children.length === 0
                  ? "No child Tasks loaded"
                  : children.length === 1
                    ? "1 loaded"
                    : `${children.length} loaded`}
              </span>
            </div>

            {children.map((child) => (
              <div className="hierarchy-item" key={child.id}>
                <span className="hierarchy-sep" aria-hidden="true">
                  └
                </span>
                <button
                  type="button"
                  className="hierarchy-link"
                  onClick={() => onSelectTask(child.id)}
                  title={`Open ${child.task.title} (${child.id})`}
                >
                  {child.task.title} <span className="mono">{child.id}</span>
                </button>
                <TaskStatusBadge status={child.status} />
              </div>
            ))}

            {tree == null ? (
              <p className="detail-empty">Related Tasks were not loaded.</p>
            ) : tree.truncated ? (
              <p className="detail-empty">
                The related-Task tree read was truncated; some Tasks may be missing.
              </p>
            ) : null}
          </div>
        </section>

        <section className="detail-section">
          <h3 className="detail-section-title">Delivery &amp; Progress</h3>
          {delivery == null ? (
            <p className="detail-empty">No Delivery assigned.</p>
          ) : (
            <>
              <div className="detail-grid">
                <span className="detail-label">Delivery</span>
                <span className="detail-value mono">{delivery.id}</span>

                <span className="detail-label">State</span>
                <span className="detail-value">{DELIVERY_STATUS_LABELS[delivery.state]}</span>

                <span className="detail-label">Worker</span>
                <span className="detail-value mono">{delivery.workerId}</span>

                <span className="detail-label">Slot</span>
                <span className="detail-value mono">{delivery.slotId}</span>

                <span className="detail-label">Executor</span>
                <IdValue id={delivery.executorId} />

                <span className="detail-label">Connection epoch</span>
                <span className="detail-value tabular">{delivery.connectionEpoch}</span>

                {delivery.rejectionCode == null ? null : (
                  <>
                    <span className="detail-label">Rejection</span>
                    <span className="detail-value">
                      {REJECTION_CODE_LABELS[delivery.rejectionCode]}
                    </span>
                  </>
                )}

                {delivery.cancelState == null ? null : (
                  <>
                    <span className="detail-label">Cancel</span>
                    <span className="detail-value">
                      {CANCEL_STATE_LABELS[delivery.cancelState]}
                    </span>
                  </>
                )}

                {delivery.cancelReason == null || delivery.cancelReason === "" ? null : (
                  <>
                    <span className="detail-label">Cancel reason</span>
                    <span className="detail-value">{delivery.cancelReason}</span>
                  </>
                )}
              </div>

              {progress == null ? (
                <p className="detail-empty">No Harness progress reported yet.</p>
              ) : (
                <div className="progress-block">
                  <p className="progress-block-text">{progress.text}</p>
                  <div className="progress-block-meta">
                    <span title={formatTimestamp(progress.firstReceivedAtMs)}>
                      Execution started {formatRelativeTime(progress.firstReceivedAtMs)}
                    </span>
                    <span title={formatTimestamp(progress.receivedAtMs)}>
                      Last updated {formatRelativeTime(progress.receivedAtMs)}
                    </span>
                  </div>
                </div>
              )}
            </>
          )}
        </section>
      </div>

      <footer className="detail-footer">
        {canDispatchTask(task) ? (
          <button type="button" className="button button-primary" onClick={() => onDispatch(task)}>
            Dispatch
          </button>
        ) : null}
        {isCoordinationFinishable(task) ? (
          <button
            type="button"
            className="button"
            onClick={() => onFinish(task)}
            title="Finish this coordination-only Task (no Worker report is fabricated)"
          >
            Finish
          </button>
        ) : null}
        {canCancelTask(task) ? (
          <button type="button" className="button button-danger" onClick={() => onCancel(task)}>
            Cancel
          </button>
        ) : null}
        <span className="detail-footer-spacer" />
        <button
          type="button"
          className="button"
          onClick={() => onCopyRetry(task)}
          title="Create a new held Task linked to this one via sourceTaskId"
        >
          Copy / Retry
        </button>
      </footer>
    </aside>
  );
}
