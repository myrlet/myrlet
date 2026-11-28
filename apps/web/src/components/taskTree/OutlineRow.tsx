/**
 * One visible outline row: hierarchy + title + relationship pills, the honest
 * status badge, the latest Progress only, the assigned Worker line, and the
 * desktop-only Copy ID action.
 *
 * Presentation only: the row reads a projected `TreeRow` and the live Worker
 * map, and reports selection, expansion, and copy through callbacks. Fact
 * honesty: five Hub states only (`Handling ended`, never a verdict), worker
 * online/offline indicators read strictly from live Worker records, and empty
 * progress/worker cells render nothing rather than an em-dash placeholder.
 */

import type { KeyboardEvent } from "react";
import { ChevronRight, Copy, GitBranch, RotateCcw } from "lucide-react";

import type { WorkerRecord } from "../../api/types";
import { DELIVERY_STATUS_LABELS, formatCount, formatRelativeTime } from "../../format";
import { shortId } from "../../model/shortId";
import type { TreeRow } from "../../model/taskTree";
import { TaskStatusBadge } from "../StatusBadge";
import { TreeConnectors } from "./TreeConnectors";

function isWorkerOnline(worker: WorkerRecord | undefined): boolean {
  return worker !== undefined && worker.online && !worker.revoked;
}

export interface OutlineRowProps {
  readonly row: TreeRow;
  readonly selected: boolean;
  readonly workerById: ReadonlyMap<string, WorkerRecord>;
  readonly onSelectTask: (taskId: string) => void;
  readonly onToggleExpand: (taskId: string) => void;
  readonly onCopyId: (taskId: string) => void;
  /** Phone layout: the row has no copy action at all. */
  readonly compact: boolean;
}

export function OutlineRow(props: OutlineRowProps) {
  const { row, selected, workerById, onSelectTask, onToggleExpand, onCopyId, compact } = props;

  const delivery = row.task.currentDelivery;
  const progress = delivery?.progress ?? null;
  const workerRecord = delivery ? workerById.get(delivery.workerId) : undefined;
  const online = isWorkerOnline(workerRecord);
  const executorOrSlot = delivery?.executorId ?? delivery?.slotId ?? null;

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.target !== event.currentTarget) {
      return;
    }
    switch (event.key) {
      case "Enter":
      case " ":
        event.preventDefault();
        onSelectTask(row.id);
        break;
      case "ArrowRight":
        if (row.hasChildren && !row.expanded) {
          event.preventDefault();
          onToggleExpand(row.id);
        }
        break;
      case "ArrowLeft":
        if (row.hasChildren && row.expanded) {
          event.preventDefault();
          onToggleExpand(row.id);
        }
        break;
      default:
        break;
    }
  };

  return (
    <div
      role="row"
      className="list-row"
      data-task-id={row.id}
      data-depth={row.depth}
      data-root={row.depth === 0}
      data-selected={selected}
      data-match={row.matchesFilter}
      aria-selected={selected}
      tabIndex={0}
      onClick={() => onSelectTask(row.id)}
      onKeyDown={handleKeyDown}
    >
      {/* 1. Hierarchy + primary title + relationship badges */}
      <div role="cell" className="list-cell list-cell-task">
        <TreeConnectors
          depth={row.depth}
          isLastSibling={row.isLastSibling}
          continuations={row.ancestorContinuations}
        />

        {row.hasChildren ? (
          <button
            type="button"
            className="tree-toggle"
            data-expanded={row.expanded}
            aria-expanded={row.expanded}
            aria-label={`${row.expanded ? "Collapse" : "Expand"} ${row.title} — ${
              row.childCount === 1 ? "1 child Task" : `${formatCount(row.childCount)} child Tasks`
            }`}
            onClick={(event) => {
              event.stopPropagation();
              onToggleExpand(row.id);
            }}
          >
            <ChevronRight size={13} aria-hidden="true" />
          </button>
        ) : (
          <span className="list-leaf-marker" aria-hidden="true" />
        )}

        <span className="list-task-title" title={`${row.title} (${row.id})`}>
          {row.title}
        </span>

        {row.hasChildren && !row.expanded ? (
          <span
            className="list-micro-pill"
            title={`${formatCount(row.childCount)} collapsed child Tasks`}
          >
            +{formatCount(row.childCount)}
          </span>
        ) : null}
        {row.dependencyCount > 0 ? (
          <span
            className="list-micro-pill"
            title={`${formatCount(row.dependencyCount)} sibling ${
              row.dependencyCount === 1 ? "dependency" : "dependencies"
            }`}
          >
            <GitBranch size={10} aria-hidden="true" />
            <span>{row.dependencyCount === 1 ? "dep" : `${row.dependencyCount} deps`}</span>
          </span>
        ) : null}
        {row.hasSourceTask ? (
          <span className="list-micro-pill" title="Created from a source Task (copy / retry)">
            <RotateCcw size={10} aria-hidden="true" />
            <span>retry</span>
          </span>
        ) : null}
      </div>

      {/* 2. Honest V1 status badge */}
      <div role="cell" className="list-cell list-cell-status">
        <TaskStatusBadge status={row.status} />
      </div>

      {/* 3. Latest Progress only; no dash placeholder when absent. On phones a
          row drops this line unless the Harness actually reported Progress. */}
      <div
        role="cell"
        className="list-cell list-cell-progress"
        data-empty={progress === null}
        data-progress={progress ? "reported" : delivery ? "none" : "unassigned"}
      >
        {progress ? (
          <span className="list-progress-line">
            <span className="list-progress-text" title={progress.text}>
              {progress.text}
            </span>
            <span className="list-progress-age mono">
              {formatRelativeTime(progress.receivedAtMs)}
            </span>
          </span>
        ) : delivery ? (
          <span
            className="list-progress-muted"
            title={`${DELIVERY_STATUS_LABELS[delivery.state]} — no Progress reported`}
          >
            No progress
          </span>
        ) : null}
      </div>

      {/* 4. Worker + online indicator + executor/slot on one aligned line. */}
      <div
        role="cell"
        className="list-cell list-cell-worker"
        data-empty={delivery === null}
      >
        {delivery ? (
          <span
            className="list-worker-chip"
            title={`${delivery.workerId} (${online ? "online" : "offline"}) · slot ${delivery.slotId}${
              delivery.executorId ? ` · executor ${delivery.executorId}` : ""
            }`}
          >
            <span
              className="connection-dot"
              data-state={online ? "online" : "offline"}
              aria-hidden="true"
            />
            <span className="list-worker-id mono truncate">{shortId(delivery.workerId)}</span>
            {executorOrSlot ? (
              <span className="list-worker-slot mono truncate">{executorOrSlot}</span>
            ) : null}
          </span>
        ) : null}
      </div>

      {/* 5. Desktop-only copy ID; mobile keeps copying in Task Detail. */}
      {compact ? null : (
        <div role="cell" className="list-cell list-cell-actions">
          <div className="list-row-actions">
            <button
              type="button"
              className="list-action-btn"
              title="Copy Task ID"
              aria-label={`Copy Task ID ${row.id}`}
              onClick={(event) => {
                event.stopPropagation();
                onCopyId(row.id);
              }}
            >
              <Copy size={12} aria-hidden="true" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
