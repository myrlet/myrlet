/**
 * Myrlet Web UI root.
 *
 * Renders the Auth gate until a Hub session exists, then the header, the Task
 * tree, the Task detail pane, and the explicit action dialogs. All state that
 * is not session-owned lives here; credentials stay inside `useHub`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { TaskRecord } from "./api/types";
import { AuthGate } from "./components/AuthGate";
import { HeaderBar } from "./components/HeaderBar";
import { TaskDetailPanel } from "./components/TaskDetailPanel";
import {
  CancelDialog,
  DispatchDialog,
  FinishDialog,
  ImportBundleDialog,
  NewTaskDialog,
} from "./components/TaskModals";
import { TaskTreeTable } from "./components/TaskTreeTable";
import { WorkersModal } from "./components/WorkersModal";
import { buildTaskTreeRows, taskParentId } from "./format";
import type { TaskFilter } from "./format";
import { useHub } from "./state/useHub";

const EMPTY_FILTER: TaskFilter = {};

function isFilterActive(filter: TaskFilter): boolean {
  const query = (filter.query ?? "").trim();
  const status = filter.status;
  const workerId = filter.workerId;
  const executorId = filter.executorId;
  return (
    query !== "" ||
    (status !== undefined && status !== null && status !== "all") ||
    (workerId !== undefined && workerId !== null && workerId !== "all") ||
    (executorId !== undefined && executorId !== null && executorId !== "all")
  );
}

export default function App() {
  const hub = useHub();
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set<string>());
  const [filter, setFilter] = useState<TaskFilter>(EMPTY_FILTER);
  const [workersOpen, setWorkersOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [newTask, setNewTask] = useState<{ open: boolean; sourceTask: TaskRecord | null }>({
    open: false,
    sourceTask: null,
  });
  const [dispatchTarget, setDispatchTarget] = useState<TaskRecord | null>(null);
  const [cancelTarget, setCancelTarget] = useState<TaskRecord | null>(null);
  const [finishTarget, setFinishTarget] = useState<TaskRecord | null>(null);

  const tasks = hub.tasks;
  const expandInitialized = useRef(false);

  // First load: open every root Task that has children so the tree is readable
  // without clicking. Later snapshots never override the operator's choices.
  useEffect(() => {
    if (expandInitialized.current || tasks.length === 0) {
      return;
    }
    expandInitialized.current = true;
    const parents = new Set<string>();
    for (const task of tasks) {
      const parentId = taskParentId(task);
      if (parentId !== null) {
        parents.add(parentId);
      }
    }
    const next = new Set<string>();
    for (const task of tasks) {
      if (taskParentId(task) === null && parents.has(task.id)) {
        next.add(task.id);
      }
    }
    if (next.size > 0) {
      setExpanded(next);
    }
  }, [tasks]);

  const rows = useMemo(
    () => buildTaskTreeRows(tasks, expanded, filter),
    [tasks, expanded, filter],
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

  const closeAllModals = useCallback(() => {
    setWorkersOpen(false);
    setImportOpen(false);
    setNewTask({ open: false, sourceTask: null });
    setDispatchTarget(null);
    setCancelTarget(null);
    setFinishTarget(null);
  }, []);

  // One global Escape handler closes whichever modal is open. Dialogs also
  // handle Escape themselves; both paths converge on the same closed state.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") {
        return;
      }
      if (
        dispatchTarget !== null ||
        cancelTarget !== null ||
        finishTarget !== null ||
        newTask.open ||
        importOpen ||
        workersOpen
      ) {
        closeAllModals();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [cancelTarget, closeAllModals, dispatchTarget, finishTarget, importOpen, newTask.open, workersOpen]);

  const openDispatch = useCallback(
    (task?: TaskRecord) => {
      setDispatchTarget(task ?? hub.selectedTask);
    },
    [hub.selectedTask],
  );

  const openCancel = useCallback(
    (task?: TaskRecord) => {
      setCancelTarget(task ?? hub.selectedTask);
    },
    [hub.selectedTask],
  );

  const openFinish = useCallback(
    (task?: TaskRecord) => {
      setFinishTarget(task ?? hub.selectedTask);
    },
    [hub.selectedTask],
  );

  const openCopyRetry = useCallback(
    (task?: TaskRecord) => {
      setNewTask({ open: true, sourceTask: task ?? hub.selectedTask });
    },
    [hub.selectedTask],
  );

  const createTask = useCallback(
    async (definition: Parameters<typeof hub.createTask>[0]): Promise<void> => {
      await hub.createTask(definition);
    },
    [hub],
  );

  const importBundle = useCallback(
    async (request: Parameters<typeof hub.importBundle>[0]): Promise<void> => {
      await hub.importBundle(request);
    },
    [hub],
  );

  const dispatchTask = useCallback(
    async (
      taskId: string,
      request: Parameters<typeof hub.dispatchTask>[1],
    ): Promise<void> => {
      await hub.dispatchTask(taskId, request);
    },
    [hub],
  );

  const cancelTask = useCallback(
    async (taskId: string, request: Parameters<typeof hub.cancelTask>[1]): Promise<void> => {
      await hub.cancelTask(taskId, request);
    },
    [hub],
  );

  const finishTask = useCallback(
    async (taskId: string, request: Parameters<typeof hub.finishTask>[1]): Promise<void> => {
      await hub.finishTask(taskId, request);
    },
    [hub],
  );

  const refresh = useCallback(() => {
    void hub.refresh();
  }, [hub]);

  const resetFilter = useCallback(() => {
    setFilter(EMPTY_FILTER);
  }, []);

  if (!hub.session) {
    return (
      <AuthGate
        defaultBaseUrl={hub.baseUrl}
        connecting={hub.connection === "connecting"}
        error={hub.error}
        theme={hub.theme}
        onToggleTheme={hub.toggleTheme}
        onConnect={hub.connect}
      />
    );
  }

  const onlineWorkerCount = hub.workers.filter(
    (worker) => worker.online && !worker.revoked,
  ).length;
  const totalTasks = hub.snapshot?.totalTasks ?? tasks.length;
  const tasksTruncated = hub.snapshot?.tasksTruncated ?? false;

  return (
    <div className="app-shell">
      <HeaderBar
        baseUrl={hub.session.baseUrl}
        connection={hub.connection}
        eventCursor={hub.eventCursor}
        workerCount={hub.workers.length}
        onlineWorkerCount={onlineWorkerCount}
        filter={filter}
        onFilterChange={setFilter}
        workers={hub.workers}
        theme={hub.theme}
        onToggleTheme={hub.toggleTheme}
        onRefresh={refresh}
        onOpenWorkers={() => setWorkersOpen(true)}
        onOpenImport={() => setImportOpen(true)}
        onOpenNewTask={() => setNewTask({ open: true, sourceTask: null })}
        onDisconnect={hub.disconnect}
      />

      <main className="workspace">
        <TaskTreeTable
          rows={rows}
          totalTasks={totalTasks}
          loadedCount={tasks.length}
          tasksTruncated={tasksTruncated}
          selectedTaskId={hub.selectedTaskId}
          onSelectTask={(taskId: string) => {
            void hub.selectTask(taskId);
          }}
          onToggleExpand={toggleExpand}
          onOpenNewTask={() => setNewTask({ open: true, sourceTask: null })}
          hasActiveFilter={isFilterActive(filter)}
          onClearFilters={resetFilter}
        />

        {hub.selectedTaskId !== null ? (
          <>
            <button
              type="button"
              className="detail-scrim"
              aria-label="Close task detail"
              onClick={() => {
                void hub.selectTask(null);
              }}
            />
            <TaskDetailPanel
              task={hub.selectedTask}
              tree={hub.selectedTree}
              loading={hub.detailLoading}
              error={hub.detailError}
              onClose={() => {
                void hub.selectTask(null);
              }}
              onSelectTask={(taskId: string) => {
                void hub.selectTask(taskId);
              }}
              onDispatch={openDispatch}
              onCancel={openCancel}
              onFinish={openFinish}
              onCopyRetry={openCopyRetry}
            />
          </>
        ) : null}
      </main>

      <NewTaskDialog
        open={newTask.open}
        sourceTask={newTask.sourceTask}
        onClose={() => setNewTask({ open: false, sourceTask: null })}
        onSubmit={createTask}
      />

      <ImportBundleDialog
        open={importOpen}
        onClose={() => setImportOpen(false)}
        onSubmit={importBundle}
      />

      <DispatchDialog
        task={dispatchTarget}
        workers={hub.workers}
        onClose={() => setDispatchTarget(null)}
        onSubmit={dispatchTask}
      />

      <CancelDialog
        task={cancelTarget}
        onClose={() => setCancelTarget(null)}
        onSubmit={cancelTask}
      />

      <FinishDialog
        task={finishTarget}
        onClose={() => setFinishTarget(null)}
        onSubmit={finishTask}
      />

      <WorkersModal
        open={workersOpen}
        baseUrl={hub.session.baseUrl}
        workers={hub.workers}
        onClose={() => setWorkersOpen(false)}
        onCreateEnrollmentCode={hub.createEnrollmentCode}
        onRotateWorker={hub.rotateWorker}
        onRevokeWorker={hub.revokeWorker}
      />
    </div>
  );
}
