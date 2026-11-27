/**
 * Generous, keyboard-reachable Task tree table.
 *
 * Each Task row shows the honest Hub hierarchy: orthogonal connector guides for
 * nested Tasks, a disclosure chevron, a short monospace ID (full ID on hover),
 * a bold title, and a non-wrapping child-count badge. Inline micro-actions are
 * hover/focus affordances only; they stop propagation so they act on the row's
 * Task directly instead of also re-selecting it.
 *
 * Progress/Execution is a factual summary only: the latest Harness-authored
 * `progress.text` plus its receipt age, otherwise the Delivery state label,
 * otherwise an explicit `—`. Nothing here invents percentages or verdicts.
 */

import type { KeyboardEvent } from "react";
import { AlertTriangle, ArrowRight, ChevronRight, Copy, Plus } from "lucide-react";

import { DELIVERY_STATUS_LABELS, formatCount, formatRelativeTime } from "../format";
import type { TreeRow } from "../format";
import { TaskStatusBadge } from "./StatusBadge";

export interface TaskTreeTableProps {
  rows: readonly TreeRow[];
  totalTasks: number;
  loadedCount: number;
  tasksTruncated: boolean;
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
  onToggleExpand: (taskId: string) => void;
  onQuickDispatch: (taskId: string) => void;
  onQuickAddSubtask: (parentTaskId: string) => void;
  onCopyId: (taskId: string) => void;
  onOpenNewTask: () => void;
  hasActiveFilter: boolean;
  onClearFilters: () => void;
}

const ID_PREVIEW_LENGTH = 12;
const GUIDE_WIDTH = 14;

/** Short display ID; long IDs keep the full value in `title`. */
function shortId(id: string): string {
  return id.length <= ID_PREVIEW_LENGTH ? id : id.slice(0, ID_PREVIEW_LENGTH);
}

function dispatchable(status: TreeRow["status"]): boolean {
  return status === "held" || status === "queued";
}

/**
 * Orthogonal tree connector guides: one hairline vertical per ancestor level,
 * with an elbow on the final level that reaches toward the row content.
 */
function TreeGuides({ depth }: { depth: number }) {
  if (depth <= 0) {
    return null;
  }
  return (
    <span
      className="tree-guides-inline"
      aria-hidden="true"
      style={{ display: "inline-flex", alignSelf: "stretch", flex: "none" }}
    >
      {Array.from({ length: depth }, (_, level) => {
        const elbow = level === depth - 1;
        return (
          <span
            key={level}
            style={{
              position: "relative",
              width: GUIDE_WIDTH,
              alignSelf: "stretch",
              borderLeft: "1px solid var(--border-default)",
            }}
          >
            {elbow ? (
              <span
                style={{
                  position: "absolute",
                  left: 0,
                  top: "50%",
                  width: GUIDE_WIDTH / 2 + 2,
                  height: 1,
                  background: "var(--border-default)",
                }}
              />
            ) : null}
          </span>
        );
      })}
    </span>
  );
}

interface TreeTableRowProps {
  row: TreeRow;
  selected: boolean;
  onSelectTask: (taskId: string) => void;
  onToggleExpand: (taskId: string) => void;
  onQuickDispatch: (taskId: string) => void;
  onQuickAddSubtask: (parentTaskId: string) => void;
  onCopyId: (taskId: string) => void;
}

function TreeTableRow(props: TreeTableRowProps) {
  const {
    row,
    selected,
    onSelectTask,
    onToggleExpand,
    onQuickDispatch,
    onQuickAddSubtask,
    onCopyId,
  } = props;

  const delivery = row.task.currentDelivery;
  const progress = delivery?.progress ?? null;

  const handleKeyDown = (event: KeyboardEvent<HTMLTableRowElement>): void => {
    // Nested controls (chevron, micro-actions) handle their own keys.
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
    <tr
      className="tree-row"
      data-selected={selected}
      data-match={row.matchesFilter}
      aria-selected={selected}
      tabIndex={0}
      onClick={() => onSelectTask(row.id)}
      onKeyDown={handleKeyDown}
    >
      <td>
        <div className="cell-task">
          <TreeGuides depth={row.depth} />
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
              <ChevronRight size={12} aria-hidden="true" />
            </button>
          ) : (
            <span className="tree-spacer" aria-hidden="true" />
          )}

          <span className="task-title" title={row.title}>
            {row.title}
          </span>
          <span className="task-id mono" title={`Task ID ${row.id}`}>
            {shortId(row.id)}
          </span>
          {row.childCount > 0 ? (
            <span className="subtask-badge" title={`${formatCount(row.childCount)} child Tasks loaded`}>
              {formatCount(row.childCount)} {row.childCount === 1 ? "subtask" : "subtasks"}
            </span>
          ) : null}

          <span className="row-actions">
            {dispatchable(row.status) ? (
              <button
                type="button"
                className="button button-sm button-ghost"
                title="Dispatch this Task"
                onClick={(event) => {
                  event.stopPropagation();
                  onQuickDispatch(row.id);
                }}
              >
                <ArrowRight size={12} aria-hidden="true" />
                <span>Dispatch</span>
              </button>
            ) : null}
            <button
              type="button"
              className="button button-sm button-ghost"
              title="Add a subtask under this Task"
              onClick={(event) => {
                event.stopPropagation();
                onQuickAddSubtask(row.id);
              }}
            >
              <Plus size={12} aria-hidden="true" />
              <span>Subtask</span>
            </button>
            <button
              type="button"
              className="button button-sm button-ghost"
              title="Copy this Task ID"
              aria-label={`Copy Task ID ${row.id}`}
              onClick={(event) => {
                event.stopPropagation();
                onCopyId(row.id);
              }}
            >
              <Copy size={12} aria-hidden="true" />
            </button>
          </span>
        </div>
      </td>

      <td className={row.category === "" ? "cell-empty" : "cell-muted"}>
        {row.category === "" ? "—" : row.category}
      </td>

      <td>
        <TaskStatusBadge status={row.status} />
      </td>

      <td>
        {progress && delivery ? (
          <div className="progress-cell">
            <span className="progress-text" title={progress.text}>
              {progress.text}
            </span>
            <span className="progress-meta">
              {DELIVERY_STATUS_LABELS[delivery.state]} ·{" "}
              {formatRelativeTime(progress.receivedAtMs)}
            </span>
          </div>
        ) : delivery ? (
          <div className="progress-cell">
            <span className="progress-text">{DELIVERY_STATUS_LABELS[delivery.state]}</span>
            <span className="progress-meta">
              No Progress reported · {formatRelativeTime(delivery.updatedAtMs)}
            </span>
          </div>
        ) : (
          <span className="cell-empty">—</span>
        )}
      </td>

      <td>
        {delivery ? (
          <div className="truncate" title={`${delivery.workerId} · slot ${delivery.slotId}`}>
            <span className="mono">{delivery.workerId}</span>
            <span className="cell-empty"> · {delivery.slotId}</span>
          </div>
        ) : (
          <span className="cell-empty">—</span>
        )}
      </td>
    </tr>
  );
}

export function TaskTreeTable(props: TaskTreeTableProps) {
  const {
    rows,
    totalTasks,
    loadedCount,
    tasksTruncated,
    selectedTaskId,
    onSelectTask,
    onToggleExpand,
    onQuickDispatch,
    onQuickAddSubtask,
    onCopyId,
    onOpenNewTask,
    hasActiveFilter,
    onClearFilters,
  } = props;

  const countLabel = tasksTruncated
    ? `${formatCount(loadedCount)} of ${formatCount(totalTasks)} loaded`
    : `${formatCount(totalTasks)} ${totalTasks === 1 ? "task" : "tasks"}`;

  return (
    <section className="task-pane" aria-label="Task tree">
      {hasActiveFilter ? (
        <div className="task-pane-header">
          <span className="task-pane-count">{countLabel}</span>
          <span className="muted">· {formatCount(rows.length)} shown</span>
          <span className="header-spacer" />
          <button type="button" className="button button-sm button-ghost" onClick={onClearFilters}>
            Clear filters
          </button>
        </div>
      ) : null}

      {tasksTruncated ? (
        <div className="notice-bar">
          <AlertTriangle size={13} aria-hidden="true" />
          <span>
            Showing {loadedCount} of {totalTasks} tasks (snapshot bounded)
          </span>
        </div>
      ) : null}

      <div className="task-table-scroll">
        {rows.length === 0 ? (
          <div className="pane-empty">
            {hasActiveFilter ? (
              <>
                <span className="pane-empty-title">No matching tasks</span>
                <span>No Task matches the current search or filters.</span>
                <button type="button" className="button button-sm" onClick={onClearFilters}>
                  Clear filters
                </button>
              </>
            ) : (
              <>
                <span className="pane-empty-title">No tasks yet</span>
                <span>Tasks created on this Hub will appear here.</span>
                <button
                  type="button"
                  className="button button-sm button-primary"
                  onClick={onOpenNewTask}
                >
                  <Plus size={13} aria-hidden="true" />
                  New Task
                </button>
              </>
            )}
          </div>
        ) : (
          <table className="tree-table">
            <caption className="visually-hidden">
              Tasks with parent/child hierarchy, status, progress/execution summary, and assigned
              Worker
            </caption>
            <colgroup>
              <col style={{ width: "39%" }} />
              <col style={{ width: "10%" }} />
              <col style={{ width: "18%" }} />
              <col style={{ width: "19%" }} />
              <col style={{ width: "14%" }} />
            </colgroup>
            <thead>
              <tr>
                <th scope="col">Task</th>
                <th scope="col">Category</th>
                <th scope="col">Status</th>
                <th scope="col">Progress</th>
                <th scope="col">Worker</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <TreeTableRow
                  key={row.id}
                  row={row}
                  selected={row.id === selectedTaskId}
                  onSelectTask={onSelectTask}
                  onToggleExpand={onToggleExpand}
                  onQuickDispatch={onQuickDispatch}
                  onQuickAddSubtask={onQuickAddSubtask}
                  onCopyId={onCopyId}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}
