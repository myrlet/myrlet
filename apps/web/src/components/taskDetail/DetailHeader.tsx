/**
 * Task Detail headers: the pending header (Snapshot facts only, while the full
 * read loads or after it failed) and the loaded header with the ancestry
 * breadcrumb.
 *
 * Neither header decides anything about the Task; they only present the facts
 * the panel was handed.
 */

import { useMemo } from "react";
import { Copy, X } from "lucide-react";

import type { TaskRecord, TaskTreeResponse, TaskStatus } from "../../api/types";
import { TaskStatusBadge } from "../StatusBadge";
import { shortId } from "../../model/shortId";
import { buildAncestry } from "./projections";

function middleTruncateId(id: string): string {
  return id.length > 18 ? `${id.slice(0, 10)}…${id.slice(-4)}` : id;
}

function CloseButton({ onClose }: { onClose: () => void }) {
  return (
    <button
      type="button"
      className="icon-button"
      onClick={onClose}
      aria-label="Close task detail"
      title="Close task detail"
    >
      <X size={15} aria-hidden="true" />
    </button>
  );
}

/** Ancestry links, current Task last. Non-link ancestors open their Task. */
function AncestryBreadcrumb({
  ancestry,
  onSelectTask,
}: {
  /** Root-to-current chain; one entry means a root Task. */
  ancestry: readonly TaskRecord[];
  onSelectTask: (taskId: string) => void;
}) {
  if (ancestry.length <= 1) {
    return <span className="detail-breadcrumb-current">Root Task</span>;
  }
  return (
    <>
      {ancestry.map((ancestor, index) => {
        const isLast = index === ancestry.length - 1;
        return (
          <span className="detail-breadcrumb-item" key={ancestor.id}>
            {isLast ? (
              <span className="detail-breadcrumb-current truncate" title={ancestor.id}>
                {ancestor.task.title || shortId(ancestor.id)}
              </span>
            ) : (
              <>
                <button
                  type="button"
                  className="detail-breadcrumb-link truncate"
                  onClick={() => onSelectTask(ancestor.id)}
                  title={`Open ${ancestor.task.title} (${ancestor.id})`}
                >
                  {ancestor.task.title || shortId(ancestor.id)}
                </button>
                <span className="detail-breadcrumb-sep" aria-hidden="true">
                  ›
                </span>
              </>
            )}
          </span>
        );
      })}
    </>
  );
}

export interface PendingDetailHeaderProps {
  /** Snapshot title, or the Task ID when the Snapshot has no row for it. */
  readonly title: string;
  readonly status: TaskStatus | null;
  readonly taskId: string;
  readonly version: number | null;
  /** A first read is in flight. */
  readonly loading: boolean;
  readonly onClose: () => void;
}

/**
 * Header used while no full Task record exists yet. Ancestry is unknown here,
 * so it states the section rather than naming relationships it cannot see.
 */
export function PendingDetailHeader({
  title,
  status,
  taskId,
  version,
  loading,
  onClose,
}: PendingDetailHeaderProps) {
  return (
    <header className="detail-header">
      <div className="detail-heading">
        <nav className="detail-breadcrumb" aria-label="Task ancestry">
          <span className="detail-breadcrumb-current">Task</span>
        </nav>
        <h2 className="detail-title">{title}</h2>
        <div className="detail-subtitle" style={{ flexWrap: "nowrap" }}>
          {status === null ? null : <TaskStatusBadge status={status} />}
          <span className="detail-id-short mono" title={taskId}>
            {middleTruncateId(taskId)}
          </span>
          {version === null ? null : (
            <span
              className="detail-version-pill mono"
              title="Task version (optimistic concurrency)"
            >
              v{version}
            </span>
          )}
          {loading ? (
            <span className="detail-refreshing" role="status">
              Loading…
            </span>
          ) : null}
        </div>
      </div>
      <CloseButton onClose={onClose} />
    </header>
  );
}

export interface LoadedDetailHeaderProps {
  readonly task: TaskRecord;
  readonly tree: TaskTreeResponse | null;
  readonly loading: boolean;
  readonly copyId: () => void;
  readonly onSelectTask: (id: string) => void;
  readonly onClose: () => void;
}

/** Header for the authoritative Task record. */
export function LoadedDetailHeader({
  task, tree, loading, copyId, onSelectTask, onClose,
}: LoadedDetailHeaderProps) {
  const ancestry = useMemo(() => buildAncestry(task, tree), [task, tree]);
  const { id: taskId, status, version } = task;
  return (
    <header className="detail-header">
      <div className="detail-heading">
        <nav className="detail-breadcrumb" aria-label="Task ancestry">
          <AncestryBreadcrumb ancestry={ancestry} onSelectTask={onSelectTask} />
        </nav>
        <h2 className="detail-title">{task.task.title}</h2>
        <div className="detail-subtitle" style={{ flexWrap: "nowrap" }}>
          <TaskStatusBadge status={status} />
          <span className="detail-id-short mono" title={taskId}>
            {middleTruncateId(taskId)}
          </span>
          <button
            type="button"
            className="icon-button"
            onClick={copyId}
            aria-label="Copy Task ID"
            title="Copy Task ID"
          >
            <Copy size={12} aria-hidden="true" />
          </button>
          <span className="detail-version-pill mono" title="Task version (optimistic concurrency)">
            v{version}
          </span>
          {loading ? <span className="detail-refreshing">Refreshing…</span> : null}
        </div>
      </div>
      <CloseButton onClose={onClose} />
    </header>
  );
}
