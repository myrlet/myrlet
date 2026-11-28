/**
 * Grouped, keyboard-reachable Task tree outline (List mode, the default view).
 *
 * The List is the dense, scannable workspace: one sticky column header, a slim
 * summary divider per Task Group, and aligned columns where the Task title is
 * the primary fact and status, latest Progress, and assigned Worker line up.
 * Group headlines never repeat the root Task title that the first row already
 * carries. The outline is read-only: copying a Task ID is a desktop row action;
 * on narrow screens it moves to Task Detail only.
 *
 * Fact honesty: five Hub states only (`Handling ended`, never a verdict),
 * worker online/offline indicators read strictly from live Worker records, and
 * Progress shows the latest Harness text + receipt age without fake metrics.
 * Empty progress/worker cells render nothing rather than an em-dash placeholder.
 */

import { useMemo } from "react";
import type { KeyboardEvent } from "react";
import {
  AlertTriangle,
  ChevronsUpDown,
  ChevronRight,
  Copy,
  GitBranch,
  PanelRight,
  RotateCcw,
  X,
} from "lucide-react";

import type { WorkerRecord } from "../api/types";
import { DELIVERY_STATUS_LABELS, formatCount, formatRelativeTime } from "../format";
import type { TreeRow } from "../format";
import { TaskStatusBadge } from "./StatusBadge";

/** One visible outline row, as built by `buildTaskTreeRows`. */
export type TaskTreeRow = TreeRow;

export interface TaskTreeTableProps {
  readonly rows: readonly TaskTreeRow[];
  readonly workers: readonly WorkerRecord[];
  readonly totalTasks: number;
  readonly loadedCount: number;
  readonly tasksTruncated: boolean;
  readonly selectedTaskId: string | null;
  readonly allExpanded: boolean;
  readonly inspectorOpen: boolean;
  readonly activeStatusLabel: string | null;
  readonly onSelectTask: (taskId: string) => void;
  readonly onToggleExpand: (taskId: string) => void;
  readonly onToggleExpandAll: () => void;
  readonly onToggleInspector: () => void;
  readonly onCopyId: (taskId: string) => void;
  readonly hasActiveFilter: boolean;
  readonly onClearFilters: () => void;
  /** Phone layout: no per-row copy action; Task Detail keeps copying. */
  readonly compact: boolean;
}

const ID_PREVIEW_LENGTH = 12;

function shortId(id: string): string {
  return id.length <= ID_PREVIEW_LENGTH ? id : id.slice(0, ID_PREVIEW_LENGTH);
}

function isWorkerOnline(worker: WorkerRecord | undefined): boolean {
  return (
    worker !== undefined &&
    worker.online &&
    !worker.revoked &&
    worker.approval === "approved"
  );
}

/**
 * Pixel-accurate rounded SVG tree connectors aligned to parent chevron centers.
 * Each ancestor column draws a continuous vertical stem only when that ancestor
 * still has remaining siblings; the final column draws a rounded `├` or `└`.
 * Slot widths come from CSS so a narrow screen can tighten the indent.
 */
function TreeConnectors({
  depth,
  isLastSibling,
  continuations,
}: {
  depth: number;
  isLastSibling: boolean;
  continuations: readonly boolean[];
}) {
  if (depth <= 0) {
    return null;
  }

  return (
    <span className="list-guides" aria-hidden="true">
      {Array.from({ length: depth }, (_, level) => {
        const isBranch = level === depth - 1;
        if (!isBranch) {
          const drawStem = continuations[level] ?? false;
          return (
            <span className="list-guide-slot" key={level}>
              {drawStem ? <span className="list-guide-stem" /> : null}
            </span>
          );
        }

        return (
          <span className="list-guide-slot" key={level}>
            <svg
              className="list-guide-svg"
              width="22"
              height="52"
              viewBox="0 0 22 52"
              preserveAspectRatio="none"
            >
              {isLastSibling ? (
                <path
                  d="M 10 0 L 10 20 Q 10 26 16 26 L 21 26"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.15"
                  vectorEffect="non-scaling-stroke"
                />
              ) : (
                <>
                  <path
                    d="M 10 0 L 10 52"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.15"
                    vectorEffect="non-scaling-stroke"
                  />
                  <path
                    d="M 10 20 Q 10 26 16 26 L 21 26"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.15"
                    vectorEffect="non-scaling-stroke"
                  />
                </>
              )}
            </svg>
          </span>
        );
      })}
    </span>
  );
}

interface TreeGroupBucket {
  readonly key: string;
  readonly kind: "tree" | "standalone";
  readonly summary: string;
  readonly rows: TreeRow[];
}

/**
 * Group flat visible tree rows into multi-node Task Group sections plus one
 * Standalone section. The summary intentionally omits the root title: the first
 * row already shows it, and repeating it on every divider is noise.
 */
function groupRowsForOutline(rows: readonly TreeRow[]): TreeGroupBucket[] {
  const groups: TreeGroupBucket[] = [];
  const standaloneRows: TreeRow[] = [];

  let index = 0;
  while (index < rows.length) {
    const root = rows[index];
    const slice: TreeRow[] = [root];
    index += 1;
    while (index < rows.length && rows[index].depth > 0) {
      slice.push(rows[index]);
      index += 1;
    }

    const descendantCount = Math.max(0, root.task.subtree.total - 1);
    const isTree = root.hasChildren || descendantCount > 0 || slice.length > 1;
    if (isTree) {
      const finishedDescendants = Math.max(
        0,
        root.task.subtree.finished - (root.status === "finished" ? 1 : 0),
      );
      const childTotal = descendantCount || root.childCount;
      const parts = [
        `${formatCount(childTotal)} ${childTotal === 1 ? "subtask" : "subtasks"}`,
      ];
      if (root.task.subtree.queued > 0) {
        parts.push(`${formatCount(root.task.subtree.queued)} queued`);
      }
      if (root.task.subtree.dispatched > 0) {
        parts.push(`${formatCount(root.task.subtree.dispatched)} dispatched`);
      }
      if (finishedDescendants > 0) {
        parts.push(`${formatCount(finishedDescendants)} ended`);
      }
      groups.push({
        key: root.id,
        kind: "tree",
        summary: parts.join(" · "),
        rows: slice,
      });
    } else {
      standaloneRows.push(root);
    }
  }

  if (standaloneRows.length > 0) {
    groups.push({
      key: "__standalone__",
      kind: "standalone",
      summary: "",
      rows: standaloneRows,
    });
  }

  return groups;
}

interface OutlineRowProps {
  row: TreeRow;
  selected: boolean;
  workerById: ReadonlyMap<string, WorkerRecord>;
  onSelectTask: (taskId: string) => void;
  onToggleExpand: (taskId: string) => void;
  onCopyId: (taskId: string) => void;
  /** Phone layout: the row has no copy action at all. */
  compact: boolean;
}

function OutlineRow(props: OutlineRowProps) {
  const {
    row,
    selected,
    workerById,
    onSelectTask,
    onToggleExpand,
    onCopyId,
    compact,
  } = props;

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

export function TaskTreeTable(props: TaskTreeTableProps) {
  const {
    rows,
    workers,
    totalTasks,
    loadedCount,
    tasksTruncated,
    selectedTaskId,
    allExpanded,
    inspectorOpen,
    activeStatusLabel,
    onSelectTask,
    onToggleExpand,
    onToggleExpandAll,
    onToggleInspector,
    onCopyId,
    hasActiveFilter,
    onClearFilters,
    compact,
  } = props;

  const workerById = useMemo(() => {
    const map = new Map<string, WorkerRecord>();
    for (const worker of workers) {
      map.set(worker.workerId, worker);
    }
    return map;
  }, [workers]);

  const groups = useMemo(() => groupRowsForOutline(rows), [rows]);

  return (
    <section className="task-pane" aria-label="Task tree">
      {tasksTruncated ? (
        <div className="notice-bar">
          <AlertTriangle size={13} aria-hidden="true" />
          <span>
            Showing {loadedCount} of {totalTasks} tasks (snapshot bounded)
          </span>
        </div>
      ) : null}

      <div className="task-table-scroll list-outline-scroll">
        <div className="list-inline-controls">
          {activeStatusLabel === null ? (
            <span />
          ) : (
            <span className="active-filter-pill">
              <span>Filtered: {activeStatusLabel}</span>
              <button
                type="button"
                className="active-filter-clear"
                aria-label="Clear status filter"
                onClick={onClearFilters}
              >
                <X size={12} aria-hidden="true" />
              </button>
            </span>
          )}
          <div className="list-inline-actions">
            <button
              type="button"
              className="button button-sm"
              aria-label={allExpanded ? "Collapse every Task Group" : "Expand every Task Group"}
              title={allExpanded ? "Collapse every Task Group" : "Expand every Task Group"}
              onClick={onToggleExpandAll}
            >
              <ChevronsUpDown size={12} aria-hidden="true" />
              <span>{allExpanded ? "Collapse all" : "Expand all"}</span>
            </button>
            <button
              type="button"
              className="button button-sm"
              data-active={inspectorOpen}
              aria-pressed={inspectorOpen}
              aria-label="Inspector"
              title="Toggle Inspector (I)"
              onClick={onToggleInspector}
            >
              <PanelRight size={12} aria-hidden="true" />
              <span>Inspector</span>
            </button>
          </div>
        </div>

        {rows.length === 0 ? (
          <div className="pane-empty">
            {hasActiveFilter ? (
              <>
                <span className="pane-empty-title">No matching tasks</span>
                <span>No Task matches the current filters.</span>
                <button type="button" className="button button-sm" onClick={onClearFilters}>
                  Clear filters
                </button>
              </>
            ) : (
              <>
                <span className="pane-empty-title">No Tasks in this Hub yet</span>
                <span>
                  Submit Tasks from the Myrlet CLI or an enrolled agent to see them here.
                </span>
              </>
            )}
          </div>
        ) : (
          <div className="list-outline-inner">
            <div
              role="table"
              className="list-outline"
              aria-label="Tasks grouped by Task Tree with hierarchy, status, latest progress, and assigned Worker"
            >
              <div role="row" className="list-columns">
                <div role="columnheader" className="list-col-label">
                  Task
                </div>
                <div role="columnheader" className="list-col-label">
                  Status
                </div>
                <div role="columnheader" className="list-col-label">
                  Progress
                </div>
                <div role="columnheader" className="list-col-label">
                  Worker
                </div>
                <div role="columnheader" className="list-col-label list-col-actions">
                  <span className="visually-hidden">Actions</span>
                </div>
              </div>

              {groups.map((group) => (
                <div key={group.key} className="list-group" role="rowgroup">
                  {/* Tree dividers carry a subtree summary; standalone rows need
                      no divider at all, so they never add a repeated heading. */}
                  {group.kind === "tree" ? (
                    <div className="list-group-head" role="row">
                      <div className="list-group-head-main" role="columnheader">
                        <GitBranch
                          size={11}
                          aria-hidden="true"
                          className="list-group-icon"
                        />
                        <span className="list-group-summary mono">{group.summary}</span>
                      </div>
                    </div>
                  ) : null}

                  <div className="list-group-rows">
                    {group.rows.map((row) => (
                      <OutlineRow
                        key={row.id}
                        row={row}
                        selected={row.id === selectedTaskId}
                        workerById={workerById}
                        onSelectTask={onSelectTask}
                        onToggleExpand={onToggleExpand}
                        onCopyId={onCopyId}
                        compact={compact}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
