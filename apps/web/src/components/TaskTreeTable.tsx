/**
 * Grouped, keyboard-reachable Task tree outline (List mode).
 *
 * Mirrors Work Atlas's Task Group mental model: multi-task root trees each get
 * their own elevated card with an honest subtree summary and rounded SVG tree
 * connectors (`├` / `└` / `│`), while standalone root Tasks group cleanly into
 * a single calm section. The outline is read-only: the only row action is
 * copying the Task ID.
 *
 * Fact honesty: five Hub states only (`Handling ended`, never a verdict),
 * worker online/offline indicators read strictly from live Worker records, and
 * Progress shows the latest Harness text + receipt age without fake metrics.
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
}

const ID_PREVIEW_LENGTH = 12;
const GUIDE_SLOT = 22;

function shortId(id: string): string {
  return id.length <= ID_PREVIEW_LENGTH ? id : id.slice(0, ID_PREVIEW_LENGTH);
}

function isWorkerOnline(worker: WorkerRecord | undefined): boolean {
  return worker !== undefined && worker.online && !worker.revoked;
}

/**
 * Pixel-accurate rounded SVG tree connectors aligned to parent chevron centers.
 * Each ancestor column draws a continuous vertical stem only when that ancestor
 * still has remaining siblings; the final column draws a rounded `├` or `└`.
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
            <span className="list-guide-slot" key={level} style={{ width: GUIDE_SLOT }}>
              {drawStem ? <span className="list-guide-stem" /> : null}
            </span>
          );
        }

        return (
          <span className="list-guide-slot" key={level} style={{ width: GUIDE_SLOT }}>
            <svg
              className="list-guide-svg"
              width={GUIDE_SLOT}
              height="52"
              viewBox={`0 0 ${GUIDE_SLOT} 52`}
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
  readonly title: string;
  readonly summary: string;
  readonly rows: TreeRow[];
}

/** Group flat visible tree rows into multi-node Task Group cards + one Standalone card. */
function groupRowsForOutline(rows: readonly TreeRow[]): TreeGroupBucket[] {
  const trees: TreeGroupBucket[] = [];
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
      const parts = [
        `${formatCount(descendantCount || root.childCount)} ${
          (descendantCount || root.childCount) === 1 ? "subtask" : "subtasks"
        }`,
      ];
      if (finishedDescendants > 0) {
        parts.push(`${formatCount(finishedDescendants)} ended`);
      }
      if (root.task.subtree.queued > 0) {
        parts.push(`${formatCount(root.task.subtree.queued)} queued`);
      }
      if (root.task.subtree.dispatched > 0) {
        parts.push(`${formatCount(root.task.subtree.dispatched)} dispatched`);
      }
      trees.push({
        key: root.id,
        kind: "tree",
        title: root.title || root.id,
        summary: parts.join(" · "),
        rows: slice,
      });
    } else {
      standaloneRows.push(root);
    }
  }

  if (standaloneRows.length > 0) {
    trees.push({
      key: "__standalone__",
      kind: "standalone",
      title: "Standalone Tasks",
      summary: `${formatCount(standaloneRows.length)} ${
        standaloneRows.length === 1 ? "task" : "tasks"
      }`,
      rows: standaloneRows,
    });
  }

  return trees;
}

interface OutlineRowProps {
  row: TreeRow;
  selected: boolean;
  workerById: ReadonlyMap<string, WorkerRecord>;
  onSelectTask: (taskId: string) => void;
  onToggleExpand: (taskId: string) => void;
  onCopyId: (taskId: string) => void;
}

function OutlineRow(props: OutlineRowProps) {
  const {
    row,
    selected,
    workerById,
    onSelectTask,
    onToggleExpand,
    onCopyId,
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
      data-depth={row.depth}
      data-root={row.depth === 0}
      data-selected={selected}
      data-match={row.matchesFilter}
      aria-selected={selected}
      tabIndex={0}
      onClick={() => onSelectTask(row.id)}
      onKeyDown={handleKeyDown}
    >
      {/* 1. Task hierarchy + title + subtle badges */}
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
          <span className="list-leaf-marker" aria-hidden="true">
            <span className="state-dot" data-state={row.status} />
          </span>
        )}

        <div className="list-task-stack">
          <div className="list-task-primary">
            <span className="list-task-title" title={row.title}>
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
          <span className="list-task-id mono" title={`Task ID ${row.id}`}>
            {shortId(row.id)}
          </span>
        </div>
      </div>

      {/* 2. Category pill */}
      <div role="cell" className="list-cell list-cell-category">
        {row.category !== "" ? (
          <span className="list-cat-pill truncate" title={row.category}>
            {row.category}
          </span>
        ) : null}
      </div>

      {/* 3. Honest V1 status badge */}
      <div role="cell" className="list-cell list-cell-status">
        <TaskStatusBadge status={row.status} />
      </div>

      {/* 4. Progress / Delivery summary */}
      <div role="cell" className="list-cell list-cell-progress">
        {progress && delivery ? (
          <div className="list-progress-stack">
            <span className="list-progress-text" title={progress.text}>
              {progress.text}
            </span>
            <span className="list-progress-age mono">
              {formatRelativeTime(progress.receivedAtMs)}
            </span>
          </div>
        ) : delivery ? (
          <div className="list-progress-stack">
            <span className="list-progress-muted">
              {DELIVERY_STATUS_LABELS[delivery.state]} · No progress
            </span>
            <span className="list-progress-age mono">
              {formatRelativeTime(delivery.updatedAtMs)}
            </span>
          </div>
        ) : (
          <span className="list-empty-dot" aria-label="No Delivery">
            —
          </span>
        )}
      </div>

      {/* 5. Worker + online indicator + executor/slot */}
      <div role="cell" className="list-cell list-cell-worker">
        {delivery ? (
          <div
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
            <span className="list-worker-text">
              <span className="list-worker-id mono truncate">{shortId(delivery.workerId)}</span>
              {executorOrSlot ? (
                <span className="list-worker-slot mono truncate">{executorOrSlot}</span>
              ) : null}
            </span>
          </div>
        ) : (
          <span className="list-empty-dot" aria-label="Unassigned">
            —
          </span>
        )}
      </div>

      {/* 6. Read-only actions column (copy ID only; never overlaps title) */}
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
                <span>No Task matches the current search or filters.</span>
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
              aria-label="Tasks grouped by Task Tree with hierarchy, status, progress, and assigned Worker"
            >
              {groups.map((group) => (
                <div key={group.key} className="list-group-card" role="rowgroup">
                  <div className="list-group-head" role="row">
                    <div className="list-group-head-main" role="columnheader">
                      <span className="list-group-kicker">
                        {group.kind === "tree" ? "TASK GROUP" : "STANDALONE"}
                      </span>
                      <span className="list-group-title truncate" title={group.title}>
                        {group.kind === "tree" ? group.title : "Independent Tasks"}
                      </span>
                      <span className="list-group-summary mono">{group.summary}</span>
                    </div>
                    <div className="list-col-label" role="columnheader">
                      Category
                    </div>
                    <div className="list-col-label" role="columnheader">
                      Status
                    </div>
                    <div className="list-col-label" role="columnheader">
                      Progress
                    </div>
                    <div className="list-col-label" role="columnheader">
                      Worker
                    </div>
                    <div className="list-col-label list-col-actions" role="columnheader">
                      <span className="visually-hidden">Actions</span>
                    </div>
                  </div>

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
