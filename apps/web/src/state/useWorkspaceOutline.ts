/**
 * Board/List outline state for the authenticated workspace.
 *
 * Owns tree expansion (initialized once from the first Snapshot, then left to
 * the operator), the shared status filter projection, and the derived rows,
 * parent set, and per-status counts both views render. It mounts with the
 * workspace, so a new session starts from a fresh expansion and filter.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { SnapshotTaskSummary, TaskStatus } from "../api/types";
import { buildTaskTreeRows } from "../format";
import type { TaskFilter } from "../format";

export function useWorkspaceOutline(tasks: readonly SnapshotTaskSummary[]) {
  const [statusFilter, setStatusFilter] = useState<TaskStatus | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set<string>());
  const expandInitialized = useRef(false);

  const parentIds = useMemo(() => {
    const parents = new Set<string>();
    for (const task of tasks) {
      if (task.parentTaskId !== null) {
        parents.add(task.parentTaskId);
      }
    }
    return parents;
  }, [tasks]);

  // First load: open every root Task that has children so the List is readable
  // without clicking. Later snapshots never override the operator's choices.
  useEffect(() => {
    if (expandInitialized.current || tasks.length === 0) {
      return;
    }
    expandInitialized.current = true;
    const next = new Set<string>();
    for (const task of tasks) {
      if (task.parentTaskId === null && parentIds.has(task.id)) {
        next.add(task.id);
      }
    }
    if (next.size > 0) {
      setExpanded(next);
    }
  }, [parentIds, tasks]);

  const filter = useMemo<TaskFilter>(
    () => (statusFilter === null ? {} : { status: statusFilter }),
    [statusFilter],
  );

  const rows = useMemo(
    () => buildTaskTreeRows(tasks, expanded, filter),
    [tasks, expanded, filter],
  );

  const allExpanded =
    parentIds.size > 0 && Array.from(parentIds).every((taskId) => expanded.has(taskId));

  const statusCounts = useMemo<Record<TaskStatus, number>>(() => {
    const counts: Record<TaskStatus, number> = {
      held: 0,
      queued: 0,
      dispatched: 0,
      finished: 0,
      cancelled: 0,
    };
    for (const task of tasks) {
      counts[task.status] += 1;
    }
    return counts;
  }, [tasks]);

  /* The Board takes Tasks directly; the shared status filter applies here too. */
  const visibleTasks = useMemo(
    () =>
      statusFilter === null ? tasks : tasks.filter((task) => task.status === statusFilter),
    [tasks, statusFilter],
  );

  const toggleExpand = useCallback((taskId: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(taskId)) {
        next.delete(taskId);
      } else {
        next.add(taskId);
      }
      return next;
    });
  }, []);

  const toggleExpandAll = useCallback(() => {
    setExpanded((current) => {
      const everyOpen =
        parentIds.size > 0 && Array.from(parentIds).every((taskId) => current.has(taskId));
      return everyOpen ? new Set<string>() : new Set(parentIds);
    });
  }, [parentIds]);

  const clearFilter = useCallback(() => {
    setStatusFilter(null);
  }, []);

  return {
    statusFilter,
    setStatusFilter,
    clearFilter,
    rows,
    allExpanded,
    statusCounts,
    visibleTasks,
    toggleExpand,
    toggleExpandAll,
  };
}
