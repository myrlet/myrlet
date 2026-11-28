/**
 * Task Detail headers: the pending header (Snapshot facts only, while the full
 * read loads or after it failed) and the loaded header with the ancestry
 * breadcrumb.
 *
 * Neither header decides anything about the Task; they only present the facts
 * the panel was handed.
 */

import type { ReactNode } from "react";
import { Copy, X } from "lucide-react";

import type { TaskStatus } from "../../api/types";
import { TaskStatusBadge } from "../StatusBadge";
import { middleTruncateId, shortId } from "./formatIds";

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
export function AncestryBreadcrumb({
  ancestry,
  onSelectTask,
}: {
  /** Root-to-current chain; one entry means a root Task. */
  ancestry: readonly { id: string; title: string }[];
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
                {ancestor.title || shortId(ancestor.id)}
              </span>
            ) : (
              <>
                <button
                  type="button"
                  className="detail-breadcrumb-link truncate"
                  onClick={() => onSelectTask(ancestor.id)}
                  title={`Open ${ancestor.title} (${ancestor.id})`}
                >
                  {ancestor.title || shortId(ancestor.id)}
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
  readonly title: string;
  readonly status: TaskStatus;
  readonly taskId: string;
  readonly version: number;
  readonly loading: boolean;
  readonly copyId: () => void;
  readonly breadcrumb: ReactNode;
  readonly onClose: () => void;
}

/** Header for the authoritative Task record. */
export function LoadedDetailHeader({
  title,
  status,
  taskId,
  version,
  loading,
  copyId,
  breadcrumb,
  onClose,
}: LoadedDetailHeaderProps) {
  return (
    <header className="detail-header">
      <div className="detail-heading">
        <nav className="detail-breadcrumb" aria-label="Task ancestry">
          {breadcrumb}
        </nav>
        <h2 className="detail-title">{title}</h2>
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
