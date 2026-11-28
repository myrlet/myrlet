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
 * This module owns the table chrome (truncation notice, filter pill, empty
 * states, column headers). Row presentation lives in `taskTree/OutlineRow`, the
 * Task Group buckets in `taskOutlineGroups`, and the pure projection in
 * `model/taskTree`.
 */

import { useMemo } from "react";
import { AlertTriangle, ChevronsUpDown, GitBranch, PanelRight, X } from "lucide-react";

import type { WorkerRecord } from "../api/types";
import type { TreeRow } from "../model/taskTree";
import { groupRowsForOutline } from "./taskOutlineGroups";
import { OutlineRow } from "./taskTree/OutlineRow";

export interface TaskTreeTableProps {
  readonly rows: readonly TreeRow[];
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
