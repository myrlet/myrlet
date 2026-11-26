/**
 * Dense, keyboard-reachable Task Tree table.
 *
 * Progress/Execution is a factual summary only: the latest Harness-authored
 * `progress.text` plus its receipt age, otherwise the Delivery state label,
 * otherwise an explicit `—`. Nothing here invents percentages or verdicts.
 */

import type { KeyboardEvent } from "react";
import { AlertTriangle, ChevronRight, Plus } from "lucide-react";

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
  onOpenNewTask: () => void;
  hasActiveFilter: boolean;
  onClearFilters: () => void;
}

interface TreeTableRowProps {
  row: TreeRow;
  selected: boolean;
  onSelectTask: (taskId: string) => void;
  onToggleExpand: (taskId: string) => void;
}

/**
 * Row indentation is `depth * 18px` (Root = 0), so trees deeper than the
 * five-level scale in `app.css` stay readable. `data-depth` is still emitted to
 * keep the shared `.cell-task[data-depth]` hooks meaningful.
 */
function depthIndent(depth: number): { paddingLeft: string } {
  return { paddingLeft: `${depth * 18}px` };
}

function TreeTableRow({ row, selected, onSelectTask, onToggleExpand }: TreeTableRowProps) {
  const delivery = row.task.currentDelivery;
  const progress = delivery?.progress ?? null;

  const handleKeyDown = (event: KeyboardEvent<HTMLTableRowElement>): void => {
    // Nested controls (the disclosure chevron) handle their own keys.
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
      className="task-row"
      data-selected={selected}
      data-match={row.matchesFilter}
      aria-selected={selected}
      tabIndex={0}
      onClick={() => onSelectTask(row.id)}
      onKeyDown={handleKeyDown}
    >
      <td>
        <div
          className="cell-task"
          data-depth={Math.min(row.depth, 4)}
          style={depthIndent(row.depth)}
        >
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
          <span className="task-id" title={`Task ID ${row.id}`}>
            {row.id}
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
    onOpenNewTask,
    hasActiveFilter,
    onClearFilters,
  } = props;

  const countLabel = tasksTruncated
    ? `${formatCount(loadedCount)} of ${formatCount(totalTasks)} loaded`
    : `${formatCount(totalTasks)} ${totalTasks === 1 ? "task" : "tasks"}`;

  return (
    <section className="task-pane" aria-label="Task tree">
      <div className="task-pane-header">
        <span className="task-pane-title">Tasks</span>
        <span className="task-pane-count">{countLabel}</span>
        <span className="header-spacer" />
        {hasActiveFilter ? (
          <>
            <span className="muted">{formatCount(rows.length)} shown</span>
            <button type="button" className="button button-sm button-ghost" onClick={onClearFilters}>
              Clear filters
            </button>
          </>
        ) : null}
      </div>

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
          <table className="task-table">
            <caption className="visually-hidden">
              Tasks with parent/child hierarchy, status, progress/execution summary, and assigned
              Worker
            </caption>
            <thead>
              <tr>
                <th scope="col">Task</th>
                <th scope="col">Category</th>
                <th scope="col">Status</th>
                <th scope="col">Progress / Execution</th>
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
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}
