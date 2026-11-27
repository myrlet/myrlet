/**
 * Myrlet Web UI root — the Work Atlas console.
 *
 * Renders the Auth gate until a Hub session exists, then the 52px top app bar,
 * the 42px workspace toolbar, the full-bleed Board (default) or List, the
 * collapsible right Inspector, and the 26px status bar. Workers & Pairing is a
 * slide-over drawer rather than a separate page, and every other overlay is
 * rendered from here. All non-session state lives in this component; the bearer
 * token stays inside `useHub`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import {
  ChevronsUpDown,
  Crosshair,
  LayoutGrid,
  Maximize2,
  PanelRight,
  RotateCcw,
  Search,
  X,
} from "lucide-react";

import type { TaskRecord, TaskStatus } from "./api/types";
import { AuthGate } from "./components/AuthGate";
import { CommandPalette, ShortcutsModal, ToastStack } from "./components/CommandPalette";
import type { ToastItem } from "./components/CommandPalette";
import { TaskDetailPanel } from "./components/TaskDetailPanel";
import { TaskGraphView } from "./components/TaskGraphView";
import type { WorkAtlasHandle, WorkAtlasMeta } from "./components/TaskGraphView";
import {
  CancelDialog,
  DispatchDialog,
  FinishDialog,
  ImportBundleDialog,
  NewTaskDialog,
} from "./components/TaskModals";
import { TaskTreeTable } from "./components/TaskTreeTable";
import { TopBar } from "./components/TopBar";
import type { WorkspaceMode } from "./components/TopBar";
import { WorkersView } from "./components/WorkersView";
import { WorkspaceInspector } from "./components/WorkspaceInspector";
import {
  TASK_STATUSES,
  TASK_STATUS_META,
  buildTaskTreeRows,
  canDispatchTask,
} from "./format";
import type { TaskFilter } from "./format";
import { useHub } from "./state/useHub";

const EMPTY_FILTER: TaskFilter = {};
const INSPECTOR_MIN_WIDTH = 320;
const INSPECTOR_MAX_WIDTH = 540;
const DEFAULT_INSPECTOR_WIDTH = 414;
const TOAST_TIMEOUT_MS = 3_800;

const INITIAL_ATLAS_META: WorkAtlasMeta = {
  zoomPercent: 100,
  relationCount: 0,
  canRestore: false,
};

function isFilterActive(filter: TaskFilter): boolean {
  const query = (filter.query ?? "").trim();
  const status = filter.status;
  const workerId = filter.workerId;
  return (
    query !== "" ||
    (status !== undefined && status !== null && status !== "all") ||
    (workerId !== undefined && workerId !== null && workerId !== "all")
  );
}

interface NewTaskRequest {
  readonly open: boolean;
  readonly sourceTask: TaskRecord | null;
  readonly parentTaskId: string | null;
}

const CLOSED_NEW_TASK: NewTaskRequest = { open: false, sourceTask: null, parentTaskId: null };

export default function App() {
  const hub = useHub();

  const [mode, setMode] = useState<WorkspaceMode>("board");
  /* Choice A — full-bleed canvas first: the Inspector only opens on demand. */
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [workersOpen, setWorkersOpen] = useState(false);
  const [inspectorWidth, setInspectorWidth] = useState(DEFAULT_INSPECTOR_WIDTH);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set<string>());
  const [filter, setFilter] = useState<TaskFilter>(EMPTY_FILTER);
  const [atlasMeta, setAtlasMeta] = useState<WorkAtlasMeta>(INITIAL_ATLAS_META);
  const atlasRef = useRef<WorkAtlasHandle | null>(null);

  const [newTask, setNewTask] = useState<NewTaskRequest>(CLOSED_NEW_TASK);
  const [importOpen, setImportOpen] = useState(false);
  const [dispatchTarget, setDispatchTarget] = useState<TaskRecord | null>(null);
  const [cancelTarget, setCancelTarget] = useState<TaskRecord | null>(null);
  const [finishTarget, setFinishTarget] = useState<TaskRecord | null>(null);

  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  const [toasts, setToasts] = useState<readonly ToastItem[]>([]);
  const toastIdRef = useRef(0);
  const toastTimersRef = useRef<number[]>([]);
  const pendingDispatchRef = useRef<string | null>(null);

  const tasks = hub.tasks;
  const expandInitialized = useRef(false);

  // First load: open every root Task that has children so the List is readable
  // without clicking. Later snapshots never override the operator's choices.
  useEffect(() => {
    if (expandInitialized.current || tasks.length === 0) {
      return;
    }
    expandInitialized.current = true;
    const parents = new Set<string>();
    for (const task of tasks) {
      if (task.parentTaskId !== null) {
        parents.add(task.parentTaskId);
      }
    }
    const next = new Set<string>();
    for (const task of tasks) {
      if (task.parentTaskId === null && parents.has(task.id)) {
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

  const parentIds = useMemo(() => {
    const parents = new Set<string>();
    for (const task of tasks) {
      if (task.parentTaskId !== null) {
        parents.add(task.parentTaskId);
      }
    }
    return parents;
  }, [tasks]);

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

  // The Board takes Tasks directly, so shared status/worker filters are applied
  // here to keep Board and List honest about what is being shown.
  const visibleTasks = useMemo(() => {
    const query = (filter.query ?? "").trim().toLowerCase();
    const status =
      filter.status !== undefined && filter.status !== null && filter.status !== "all"
        ? filter.status
        : null;
    const workerId =
      filter.workerId !== undefined && filter.workerId !== null && filter.workerId !== "all"
        ? filter.workerId
        : null;
    if (query === "" && status === null && workerId === null) {
      return tasks;
    }
    return tasks.filter((task) => {
      if (status !== null && task.status !== status) {
        return false;
      }
      if (workerId !== null && task.currentDelivery?.workerId !== workerId) {
        return false;
      }
      if (query !== "") {
        const haystack =
          `${task.id} ${task.title} ${task.category} ${task.currentDelivery?.workerId ?? ""}`
            .toLowerCase();
        if (!haystack.includes(query)) {
          return false;
        }
      }
      return true;
    });
  }, [tasks, filter]);

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

  const resetFilter = useCallback(() => {
    setFilter(EMPTY_FILTER);
  }, []);

  /* ------------------------------ Selection ---------------------------- */

  /** Selecting a Task always reveals its detail in the Inspector. */
  const selectTaskAndReveal = useCallback(
    (taskId: string) => {
      setInspectorOpen(true);
      void hub.selectTask(taskId);
    },
    [hub.selectTask],
  );

  /** Clearing the selection retracts the Inspector, restoring full-bleed canvas. */
  const clearSelection = useCallback(() => {
    setInspectorOpen(false);
    void hub.selectTask(null);
  }, [hub.selectTask]);

  /* ------------------------------ Toasts ------------------------------ */

  const dismissToast = useCallback((id: string) => {
    setToasts((current) => current.filter((toast) => toast.id !== id));
  }, []);

  const pushToast = useCallback((message: string, tone: "neutral" | "error" = "neutral") => {
    toastIdRef.current += 1;
    const id = `toast-${toastIdRef.current}`;
    setToasts((current) => [...current, { id, message, tone }]);
    const timer = window.setTimeout(() => {
      setToasts((current) => current.filter((toast) => toast.id !== id));
      toastTimersRef.current = toastTimersRef.current.filter((handle) => handle !== timer);
    }, TOAST_TIMEOUT_MS);
    toastTimersRef.current.push(timer);
  }, []);

  useEffect(
    () => () => {
      for (const timer of toastTimersRef.current) {
        window.clearTimeout(timer);
      }
      toastTimersRef.current = [];
    },
    [],
  );

  const copyText = useCallback(
    (label: string, text: string) => {
      const clipboard: Clipboard | undefined =
        typeof navigator === "undefined" ? undefined : navigator.clipboard;
      if (!clipboard || typeof clipboard.writeText !== "function") {
        pushToast(`Could not copy ${label}`, "error");
        return;
      }
      void clipboard.writeText(text).then(
        () => pushToast(`Copied ${label}`),
        () => pushToast(`Could not copy ${label}`, "error"),
      );
    },
    [pushToast],
  );

  /* ------------------------------ Dialogs ----------------------------- */

  const closeAllModals = useCallback(() => {
    setImportOpen(false);
    setNewTask(CLOSED_NEW_TASK);
    setDispatchTarget(null);
    setCancelTarget(null);
    setFinishTarget(null);
    setShortcutsOpen(false);
    setPaletteOpen(false);
  }, []);

  const openNewTask = useCallback(() => {
    setNewTask({ open: true, sourceTask: null, parentTaskId: null });
  }, []);

  const openSubtask = useCallback((taskId: string) => {
    setNewTask({ open: true, sourceTask: null, parentTaskId: taskId });
  }, []);

  const openCopyRetry = useCallback((task: TaskRecord) => {
    setNewTask({ open: true, sourceTask: task, parentTaskId: null });
  }, []);

  const openWorkers = useCallback(() => {
    setWorkersOpen(true);
  }, []);

  /**
   * Row-level `Dispatch` must open the dialog with a full `TaskRecord`. Ask the
   * Hub for the detail, then let the effect below open the dialog the moment the
   * matching record lands — no guessed Task, no race.
   */
  const requestQuickDispatch = useCallback(
    (taskId: string) => {
      if (hub.selectedTask !== null && hub.selectedTask.id === taskId) {
        setDispatchTarget(hub.selectedTask);
        return;
      }
      pendingDispatchRef.current = taskId;
      void hub.selectTask(taskId);
    },
    [hub.selectedTask, hub.selectTask],
  );

  useEffect(() => {
    const pendingId = pendingDispatchRef.current;
    if (pendingId === null) {
      return;
    }
    if (hub.selectedTask !== null && hub.selectedTask.id === pendingId) {
      pendingDispatchRef.current = null;
      setDispatchTarget(hub.selectedTask);
      return;
    }
    if (hub.selectedTaskId !== pendingId) {
      pendingDispatchRef.current = null;
    }
  }, [hub.selectedTask, hub.selectedTaskId]);

  const dispatchSelected = useCallback(() => {
    const task = hub.selectedTask;
    if (task !== null && canDispatchTask(task)) {
      setDispatchTarget(task);
      return;
    }
    pushToast("Select a dispatchable Task first", "error");
  }, [hub.selectedTask, pushToast]);

  /* ----------------------------- Commands ----------------------------- */

  const createTask = useCallback(
    async (definition: Parameters<typeof hub.createTask>[0]): Promise<void> => {
      await hub.createTask(definition);
      pushToast("Task created");
    },
    [hub.createTask, pushToast],
  );

  const importBundle = useCallback(
    async (request: Parameters<typeof hub.importBundle>[0]): Promise<void> => {
      await hub.importBundle(request);
      pushToast("Bundle imported");
    },
    [hub.importBundle, pushToast],
  );

  const dispatchTask = useCallback(
    async (
      taskId: string,
      request: Parameters<typeof hub.dispatchTask>[1],
    ): Promise<void> => {
      await hub.dispatchTask(taskId, request);
      pushToast("Dispatched");
    },
    [hub.dispatchTask, pushToast],
  );

  const cancelTask = useCallback(
    async (taskId: string, request: Parameters<typeof hub.cancelTask>[1]): Promise<void> => {
      await hub.cancelTask(taskId, request);
      pushToast("Cancelled");
    },
    [hub.cancelTask, pushToast],
  );

  const finishTask = useCallback(
    async (taskId: string, request: Parameters<typeof hub.finishTask>[1]): Promise<void> => {
      await hub.finishTask(taskId, request);
      pushToast("Parent finished");
    },
    [hub.finishTask, pushToast],
  );

  const rotateWorker = useCallback(
    async (workerId: string) => {
      const response = await hub.rotateWorker(workerId);
      pushToast("Credential rotated");
      return response;
    },
    [hub.rotateWorker, pushToast],
  );

  const revokeWorker = useCallback(
    async (workerId: string): Promise<void> => {
      await hub.revokeWorker(workerId);
      pushToast("Worker revoked");
    },
    [hub.revokeWorker, pushToast],
  );

  const refresh = useCallback(() => {
    void hub.refresh();
  }, [hub.refresh]);

  /* ----------------------------- Keyboard ----------------------------- */

  const moveSelection = useCallback(
    (direction: 1 | -1) => {
      if (rows.length === 0) {
        return;
      }
      const currentIndex = rows.findIndex((row) => row.id === hub.selectedTaskId);
      const nextIndex =
        currentIndex === -1
          ? direction === 1
            ? 0
            : rows.length - 1
          : Math.min(rows.length - 1, Math.max(0, currentIndex + direction));
      const row = rows[nextIndex];
      if (row !== undefined && row.id !== hub.selectedTaskId) {
        selectTaskAndReveal(row.id);
      }
    },
    [rows, hub.selectedTaskId, selectTaskAndReveal],
  );

  const modalOpen =
    paletteOpen ||
    shortcutsOpen ||
    workersOpen ||
    importOpen ||
    newTask.open ||
    dispatchTarget !== null ||
    cancelTarget !== null ||
    finishTarget !== null;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if ((event.metaKey || event.ctrlKey) && (event.key === "k" || event.key === "K")) {
        event.preventDefault();
        setPaletteOpen((current) => !current);
        return;
      }

      if (event.key === "Escape") {
        // A nested surface (e.g. the Settings popover) already consumed this
        // Escape; do not also close the drawer or clear the selection.
        if (event.defaultPrevented) {
          return;
        }
        if (paletteOpen) {
          event.preventDefault();
          setPaletteOpen(false);
          return;
        }
        if (shortcutsOpen) {
          event.preventDefault();
          setShortcutsOpen(false);
          return;
        }
        if (
          importOpen ||
          newTask.open ||
          dispatchTarget !== null ||
          cancelTarget !== null ||
          finishTarget !== null
        ) {
          event.preventDefault();
          closeAllModals();
          return;
        }
        // The Workers drawer sits above the workspace, so it closes before the
        // Task selection is allowed to change.
        if (workersOpen) {
          event.preventDefault();
          setWorkersOpen(false);
          return;
        }
        if (hub.selectedTaskId !== null || inspectorOpen) {
          event.preventDefault();
          clearSelection();
        }
        return;
      }

      const target = event.target;
      const inField =
        target instanceof HTMLElement &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);
      if (inField) {
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (modalOpen) {
        return;
      }

      switch (event.key) {
        case "?":
          event.preventDefault();
          setShortcutsOpen(true);
          break;
        case "n":
          event.preventDefault();
          openNewTask();
          break;
        case "d":
          event.preventDefault();
          dispatchSelected();
          break;
        case "i":
          event.preventDefault();
          setInspectorOpen((current) => !current);
          break;
        case "f":
          if (mode === "board") {
            event.preventDefault();
            atlasRef.current?.fit();
          }
          break;
        case "F":
          if (mode === "board" && hub.selectedTaskId !== null) {
            event.preventDefault();
            atlasRef.current?.focusSelected();
          }
          break;
        case "/": {
          const search = document.querySelector<HTMLInputElement>("[data-view-search]");
          if (search) {
            event.preventDefault();
            search.focus();
          }
          break;
        }
        case "j":
        case "ArrowDown":
          if (mode === "list") {
            event.preventDefault();
            moveSelection(1);
          }
          break;
        case "k":
        case "ArrowUp":
          if (mode === "list") {
            event.preventDefault();
            moveSelection(-1);
          }
          break;
        default:
          break;
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [
    cancelTarget,
    clearSelection,
    closeAllModals,
    dispatchSelected,
    dispatchTarget,
    finishTarget,
    hub.selectedTaskId,
    importOpen,
    inspectorOpen,
    modalOpen,
    mode,
    moveSelection,
    newTask.open,
    openNewTask,
    paletteOpen,
    shortcutsOpen,
    workersOpen,
  ]);

  /* ----------------------------- Inspector ---------------------------- */

  const beginResize = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      const startX = event.clientX;
      const startWidth = inspectorWidth;
      const onMove = (moveEvent: PointerEvent): void => {
        const next = startWidth + (startX - moveEvent.clientX);
        setInspectorWidth(
          Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, next)),
        );
      };
      const onUp = (): void => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
      };
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
    },
    [inspectorWidth],
  );

  const resizeKeyDown = useCallback((event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      setInspectorWidth((current) => Math.min(INSPECTOR_MAX_WIDTH, current + 16));
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      setInspectorWidth((current) => Math.max(INSPECTOR_MIN_WIDTH, current - 16));
    }
  }, []);

  if (!hub.session) {
    return (
      <AuthGate
        defaultBaseUrl={hub.baseUrl}
        connecting={hub.connection === "connecting"}
        error={hub.error}
        theme={hub.theme}
        onChangeTheme={hub.setTheme}
        onConnect={hub.connect}
      />
    );
  }

  const onlineWorkerCount = hub.workers.filter(
    (worker) => worker.online && !worker.revoked,
  ).length;
  const totalTasks = hub.snapshot?.totalTasks ?? tasks.length;
  const tasksTruncated = hub.snapshot?.tasksTruncated ?? false;
  const activeStatus: TaskStatus | null =
    filter.status !== undefined && filter.status !== null && filter.status !== "all"
      ? filter.status
      : null;
  const activeWorkerId =
    filter.workerId !== undefined && filter.workerId !== null && filter.workerId !== "all"
      ? filter.workerId
      : "all";
  const boardMode = mode === "board";
  const filterActive = isFilterActive(filter);

  const filterByStatus = (status: TaskStatus): void => {
    setFilter((current) => ({ ...current, status: activeStatus === status ? "all" : status }));
  };

  const openPalette = (): void => setPaletteOpen(true);

  return (
    <div className="app-frame">
      <TopBar
        mode={mode}
        onSelectMode={setMode}
        baseUrl={hub.session.baseUrl}
        connection={hub.connection}
        eventCursor={hub.eventCursor}
        onlineWorkerCount={onlineWorkerCount}
        workerCount={hub.workers.length}
        workersOpen={workersOpen}
        onToggleWorkers={() => setWorkersOpen((current) => !current)}
        onRefresh={refresh}
        theme={hub.theme}
        onChangeTheme={hub.setTheme}
        onImport={() => setImportOpen(true)}
        onNewTask={openNewTask}
        onDisconnect={hub.disconnect}
        onOpenShortcuts={() => setShortcutsOpen(true)}
      />

      <div className="board-toolbar" role="toolbar" aria-label="Board toolbar">
        <div className="toolbar-left">
          <div className="toolbar-search-wrap">
            <Search size={13} aria-hidden="true" />
            <input
              className="input toolbar-search"
              type="search"
              data-view-search
              aria-label="Find a task, ID, or worker"
              placeholder="Find a task, ID, or worker…"
              value={filter.query ?? ""}
              onChange={(event) => {
                const value = event.target.value;
                setFilter((current) => ({ ...current, query: value }));
              }}
            />
            <kbd
              className="toolbar-kbd"
              role="button"
              tabIndex={0}
              title="Open the Command Palette"
              onClick={openPalette}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  openPalette();
                }
              }}
            >
              ⌘K
            </kbd>
          </div>

          <select
            className="select toolbar-select"
            aria-label="Filter by status"
            value={activeStatus ?? "all"}
            onChange={(event) => {
              const value = event.target.value;
              setFilter((current) => ({
                ...current,
                status: value === "all" ? "all" : (value as TaskStatus),
              }));
            }}
          >
            <option value="all">All statuses ({tasks.length})</option>
            {TASK_STATUSES.map((status) => (
              <option key={status} value={status}>
                {TASK_STATUS_META[status].label} ({statusCounts[status]})
              </option>
            ))}
          </select>

          <select
            className="select toolbar-select"
            aria-label="Filter by worker"
            value={activeWorkerId}
            onChange={(event) => {
              const value = event.target.value;
              setFilter((current) => ({
                ...current,
                workerId: value === "all" ? "all" : value,
              }));
            }}
          >
            <option value="all">All workers</option>
            {hub.workers.map((worker) => (
              <option key={worker.workerId} value={worker.workerId}>
                {worker.workerId}
                {worker.revoked ? " (revoked)" : worker.online ? "" : " (offline)"}
              </option>
            ))}
          </select>

          {filterActive ? (
            <button type="button" className="button button-sm button-ghost" onClick={resetFilter}>
              Clear
            </button>
          ) : null}
        </div>

        <div className="toolbar-tools">
          {boardMode ? (
            <>
              <button
                type="button"
                className="button button-sm"
                title="Fit all groups (F)"
                onClick={() => atlasRef.current?.fit()}
              >
                <Maximize2 size={12} aria-hidden="true" />
                <span>Fit</span>
              </button>
              {hub.selectedTaskId === null ? null : (
                <button
                  type="button"
                  className="button button-sm"
                  title="Center selected Task (Shift+F)"
                  onClick={() => atlasRef.current?.focusSelected()}
                >
                  <Crosshair size={12} aria-hidden="true" />
                  <span>Focus</span>
                </button>
              )}
              {atlasMeta.canRestore ? (
                <button
                  type="button"
                  className="button button-sm"
                  title="Restore the previous layout"
                  onClick={() => atlasRef.current?.restore()}
                >
                  <RotateCcw size={12} aria-hidden="true" />
                  <span>Restore</span>
                </button>
              ) : null}
              <button
                type="button"
                className="button button-sm button-accent"
                title="Arrange the Board layout"
                onClick={() => atlasRef.current?.arrange()}
              >
                <LayoutGrid size={12} aria-hidden="true" />
                <span>Arrange</span>
              </button>
            </>
          ) : (
            <button type="button" className="button button-sm" onClick={toggleExpandAll}>
              <ChevronsUpDown size={12} aria-hidden="true" />
              <span>{allExpanded ? "Collapse all" : "Expand all"}</span>
            </button>
          )}
          <span className="toolbar-divider" aria-hidden="true" />
          <button
            type="button"
            className="button button-sm"
            data-active={inspectorOpen}
            aria-pressed={inspectorOpen}
            title="Toggle Inspector (i)"
            onClick={() => setInspectorOpen((current) => !current)}
          >
            <PanelRight size={12} aria-hidden="true" />
            <span>Inspector</span>
          </button>
        </div>
      </div>

      <div className="workspace-split">
        <div className="view-body" data-view={mode}>
          {boardMode ? (
            <TaskGraphView
              tasks={visibleTasks}
              workers={hub.workers}
              selectedTaskId={hub.selectedTaskId}
              onSelectTask={selectTaskAndReveal}
              onClearSelection={clearSelection}
              onOpenNewTask={openNewTask}
              onBoardMetaChange={setAtlasMeta}
              controlsRef={atlasRef}
            />
          ) : (
            <TaskTreeTable
              rows={rows}
              workers={hub.workers}
              totalTasks={totalTasks}
              loadedCount={tasks.length}
              tasksTruncated={tasksTruncated}
              selectedTaskId={hub.selectedTaskId}
              onSelectTask={selectTaskAndReveal}
              onToggleExpand={toggleExpand}
              onQuickDispatch={requestQuickDispatch}
              onQuickAddSubtask={openSubtask}
              onCopyId={(taskId: string) => copyText("task id", taskId)}
              onOpenNewTask={openNewTask}
              hasActiveFilter={filterActive}
              onClearFilters={resetFilter}
            />
          )}
        </div>

        {inspectorOpen ? (
          <>
            <div
              className="resize-handle"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize inspector"
              aria-valuemin={INSPECTOR_MIN_WIDTH}
              aria-valuemax={INSPECTOR_MAX_WIDTH}
              aria-valuenow={Math.round(inspectorWidth)}
              tabIndex={0}
              onPointerDown={beginResize}
              onKeyDown={resizeKeyDown}
            />
            <div className="inspector-slot" style={{ width: `${inspectorWidth}px` }}>
              {hub.selectedTaskId !== null ? (
                <>
                  <button
                    type="button"
                    className="detail-scrim"
                    aria-label="Close task detail"
                    onClick={clearSelection}
                  />
                  <TaskDetailPanel
                    task={hub.selectedTask}
                    tree={hub.selectedTree}
                    loading={hub.detailLoading}
                    error={hub.detailError}
                    onClose={clearSelection}
                    onSelectTask={selectTaskAndReveal}
                    onDispatch={(task: TaskRecord) => setDispatchTarget(task)}
                    onAddSubtask={openSubtask}
                    onCancel={(task: TaskRecord) => setCancelTarget(task)}
                    onFinish={(task: TaskRecord) => setFinishTarget(task)}
                    onCopyRetry={openCopyRetry}
                    onCopyText={copyText}
                  />
                </>
              ) : (
                <WorkspaceInspector
                  tasks={tasks}
                  workers={hub.workers}
                  onSelectTask={selectTaskAndReveal}
                  onOpenWorkers={openWorkers}
                  onClose={() => setInspectorOpen(false)}
                />
              )}
            </div>
          </>
        ) : null}
      </div>

      <footer className="status-bar">
        <div className="status-bar-left">
          {TASK_STATUSES.map((status) => (
            <button
              key={status}
              type="button"
              className="status-counter"
              data-tone={status}
              data-active={activeStatus === status}
              aria-pressed={activeStatus === status}
              onClick={() => filterByStatus(status)}
            >
              <span className="state-dot" data-state={status} aria-hidden="true" />
              <span>{TASK_STATUS_META[status].label}</span>
              <span className="tabular">{statusCounts[status]}</span>
            </button>
          ))}
        </div>
        <div className="status-bar-right">
          <span className="mono" title="Tasks and relationships in the current Snapshot">
            {totalTasks} tasks · {atlasMeta.relationCount} relations
          </span>
          {boardMode ? (
            <span className="mono" title="Board zoom">
              {atlasMeta.zoomPercent}%
            </span>
          ) : null}
          <span className="mono status-hints" aria-hidden="true">
            ⌘K · I · ?
          </span>
        </div>
      </footer>

      {workersOpen ? (
        <div
          className="workers-drawer-backdrop"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) {
              setWorkersOpen(false);
            }
          }}
        >
          <aside
            className="workers-drawer-panel"
            role="dialog"
            aria-modal="true"
            aria-label="Workers and pairing"
          >
            <header className="workers-drawer-header">
              <div className="workers-drawer-heading">
                <h2 className="workers-drawer-title">Workers &amp; Pairing</h2>
                <span className="view-subtitle">
                  {onlineWorkerCount} of {hub.workers.length} online
                </span>
              </div>
              <button type="button" className="button button-sm" onClick={refresh}>
                Refresh
              </button>
              <button
                type="button"
                className="icon-button"
                aria-label="Close workers drawer"
                onClick={() => setWorkersOpen(false)}
              >
                <X size={15} aria-hidden="true" />
              </button>
            </header>
            <div className="workers-drawer-body">
              <WorkersView
                baseUrl={hub.session.baseUrl}
                workers={hub.workers}
                onCreateEnrollmentCode={hub.createEnrollmentCode}
                onRotateWorker={rotateWorker}
                onRevokeWorker={revokeWorker}
                onCopyText={copyText}
              />
            </div>
          </aside>
        </div>
      ) : null}

      <NewTaskDialog
        open={newTask.open}
        sourceTask={newTask.sourceTask}
        defaultParentTaskId={newTask.parentTaskId}
        onClose={() => setNewTask(CLOSED_NEW_TASK)}
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

      <CommandPalette
        open={paletteOpen}
        tasks={tasks}
        selectedTask={hub.selectedTask}
        theme={hub.theme}
        onClose={() => setPaletteOpen(false)}
        onSelectTask={selectTaskAndReveal}
        onSelectMode={setMode}
        onOpenWorkers={openWorkers}
        onNewTask={openNewTask}
        onImportBundle={() => setImportOpen(true)}
        onDispatchSelected={dispatchSelected}
        onFitBoard={() => atlasRef.current?.fit()}
        onArrangeBoard={() => atlasRef.current?.arrange()}
        onChangeTheme={hub.setTheme}
      />

      <ShortcutsModal open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />

      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
