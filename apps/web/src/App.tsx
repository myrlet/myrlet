/**
 * Myrlet Web UI root — the Work Atlas console.
 *
 * Renders the Auth gate until a Hub session exists, then one 52px top app bar,
 * the relationship Board (default) or the dense List, the collapsible
 * right Inspector, and the compact status bar. Workers & Pairing is a slide-over
 * drawer rather than a separate page. The Task surface is strictly read-only:
 * creation, import, dispatch, cancel, finish, subtask, and retry belong to the
 * CLI, never here. All non-session state lives in this component; the bearer
 * token stays inside `useHub`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import { Check, ChevronDown, Filter, X } from "lucide-react";

import type { TaskStatus } from "./api/types";
import { AuthGate } from "./components/AuthGate";
import { CommandPalette, ShortcutsModal, ToastStack } from "./components/CommandPalette";
import type { ToastItem } from "./components/CommandPalette";
import { TaskDetailPanel } from "./components/TaskDetailPanel";
import { TaskGraphView } from "./components/TaskGraphView";
import type { AtlasBoardMeta, TaskGraphControls } from "./components/TaskGraphView";
import { TaskTreeTable } from "./components/TaskTreeTable";
import { TopBar } from "./components/TopBar";
import type { WorkspaceMode } from "./components/TopBar";
import { WorkersView } from "./components/WorkersView";
import { WorkspaceInspector } from "./components/WorkspaceInspector";
import { TASK_STATUSES, TASK_STATUS_META, buildTaskTreeRows } from "./format";
import type { TaskFilter } from "./format";
import { useHub } from "./state/useHub";

const INSPECTOR_MIN_WIDTH = 320;
const INSPECTOR_MAX_WIDTH = 540;
const DEFAULT_INSPECTOR_WIDTH = 414;
const TOAST_TIMEOUT_MS = 3_800;

const INITIAL_ATLAS_META: AtlasBoardMeta = {
  zoomPercent: 100,
  relationCount: 0,
  canRestore: false,
};

export default function App() {
  const hub = useHub();

  /* The Board opens first; List provides a compact task outline. */
  const [mode, setMode] = useState<WorkspaceMode>("board");
  /* The Inspector only opens on demand, so the workspace starts full-width. */
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [workersOpen, setWorkersOpen] = useState(false);
  const [inspectorWidth, setInspectorWidth] = useState(DEFAULT_INSPECTOR_WIDTH);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set<string>());
  /** The only filter: one Task status, chosen from the status bar. */
  const [statusFilter, setStatusFilter] = useState<TaskStatus | null>(null);
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [atlasMeta, setAtlasMeta] = useState<AtlasBoardMeta>(INITIAL_ATLAS_META);
  const atlasRef = useRef<TaskGraphControls | null>(null);

  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [narrowInspector, setNarrowInspector] = useState(false);
  /* Phone layout: the five status counters collapse into one filter menu. */
  const [compactStatusBar, setCompactStatusBar] = useState(false);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const inspectorRef = useRef<HTMLDivElement | null>(null);
  const inspectorReturnFocusRef = useRef<HTMLElement | null>(null);
  const statusMenuRef = useRef<HTMLDivElement | null>(null);

  const [toasts, setToasts] = useState<readonly ToastItem[]>([]);
  const toastIdRef = useRef(0);
  const toastTimersRef = useRef<number[]>([]);

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

  const filter = useMemo<TaskFilter>(
    () => (statusFilter === null ? {} : { status: statusFilter }),
    [statusFilter],
  );

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

  /** Snapshot facts for the selected Task, used while its detail read is pending. */
  const selectedSummary = useMemo(
    () => tasks.find((task) => task.id === hub.selectedTaskId) ?? null,
    [hub.selectedTaskId, tasks],
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

  const toggleInspector = useCallback(() => {
    setInspectorOpen((current) => !current);
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

  const openWorkers = useCallback(() => {
    setWorkersOpen(true);
  }, []);

  /* ----------------------------- Worker ops --------------------------- */

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

  const modalOpen = paletteOpen || shortcutsOpen || workersOpen || statusMenuOpen;

  useEffect(() => {
    if (typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(max-width: 820px)");
    const sync = (): void => setCompactStatusBar(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(max-width: 1024px)");
    const sync = (): void => setNarrowInspector(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace) {
      return;
    }
    if (narrowInspector && inspectorOpen) {
      // Record the real opener (row, Board card, toolbar button) once, before
      // the background goes inert and focus moves into the sheet.
      if (inspectorReturnFocusRef.current === null) {
        const opener = document.activeElement;
        if (
          opener instanceof HTMLElement &&
          opener !== document.body &&
          !inspectorRef.current?.contains(opener)
        ) {
          inspectorReturnFocusRef.current = opener;
        }
      }
      workspace.setAttribute("inert", "");
      inspectorRef.current?.focus();
      return () => workspace.removeAttribute("inert");
    }
    if (inspectorOpen) {
      // Still open, just wide: keep the opener without stealing focus back.
      return;
    }
    // Closed: the sheet and its inert background are already unmounted, so the
    // real opener can take focus back.
    const target = inspectorReturnFocusRef.current;
    inspectorReturnFocusRef.current = null;
    if (target?.isConnected) {
      target.focus();
    }
  }, [inspectorOpen, narrowInspector]);

  useEffect(() => {
    if (!statusMenuOpen) {
      return;
    }
    const onPointerDown = (event: MouseEvent): void => {
      const target = event.target;
      if (target instanceof Node && statusMenuRef.current?.contains(target)) {
        return;
      }
      setStatusMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [statusMenuOpen]);

  /** Close the phone status menu and put focus back on its trigger. */
  const closeStatusMenu = useCallback(() => {
    setStatusMenuOpen(false);
    statusMenuRef.current
      ?.querySelector<HTMLButtonElement>(".status-filter-trigger")
      ?.focus();
  }, []);

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
        // The Workers drawer sits above the workspace, so it closes before the
        // Task selection is allowed to change.
        if (workersOpen) {
          event.preventDefault();
          setWorkersOpen(false);
          return;
        }
        if (statusMenuOpen) {
          event.preventDefault();
          closeStatusMenu();
          return;
        }
        // Filter is workspace state, below every overlay but above the
        // Inspector/selection. Escape peels it first and leaves the Inspector.
        if (statusFilter !== null) {
          event.preventDefault();
          clearFilter();
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
    clearFilter,
    clearSelection,
    closeStatusMenu,
    hub.selectedTaskId,
    inspectorOpen,
    modalOpen,
    mode,
    moveSelection,
    paletteOpen,
    shortcutsOpen,
    statusFilter,
    statusMenuOpen,
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
  const activeStatusLabel =
    statusFilter === null ? null : TASK_STATUS_META[statusFilter].label;
  const boardMode = mode === "board";

  const filterByStatus = (status: TaskStatus): void => {
    setStatusFilter((current) => (current === status ? null : status));
  };

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
        onDisconnect={hub.disconnect}
        onOpenShortcuts={() => setShortcutsOpen(true)}
      />

      <div className="workspace-split">
        <div className="view-body" data-view={mode} ref={workspaceRef}>
          {boardMode ? (
            <TaskGraphView
              tasks={visibleTasks}
              workers={hub.workers}
              selectedTaskId={hub.selectedTaskId}
              inspectorOpen={inspectorOpen}
              activeStatusLabel={activeStatusLabel}
              onToggleInspector={toggleInspector}
              onClearFilter={clearFilter}
              onSelectTask={selectTaskAndReveal}
              onClearSelection={clearSelection}
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
              allExpanded={allExpanded}
              inspectorOpen={inspectorOpen}
              activeStatusLabel={activeStatusLabel}
              onSelectTask={selectTaskAndReveal}
              onToggleExpand={toggleExpand}
              onToggleExpandAll={toggleExpandAll}
              onToggleInspector={toggleInspector}
              onCopyId={(taskId: string) => copyText("task id", taskId)}
              hasActiveFilter={statusFilter !== null}
              onClearFilters={clearFilter}
              compact={compactStatusBar}
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
            {/* Narrow screens: a sibling scrim behind the panel, never a child
                that could swallow the panel's own close button. Pointer-only;
                keyboard users close with the panel's close button or Escape. */}
            <button
              type="button"
              className="inspector-scrim"
              tabIndex={-1}
              aria-hidden="true"
              onClick={
                hub.selectedTaskId !== null
                  ? clearSelection
                  : () => setInspectorOpen(false)
              }
            />
            <div
              ref={inspectorRef}
              className="inspector-slot"
              style={{ width: `${inspectorWidth}px` }}
              tabIndex={-1}
            >
              {hub.selectedTaskId !== null ? (
                <TaskDetailPanel
                  selectedTaskId={hub.selectedTaskId}
                  summary={selectedSummary}
                  task={hub.selectedTask}
                  tree={hub.selectedTree}
                  loading={hub.detailLoading}
                  error={hub.detailError}
                  onClose={clearSelection}
                  onSelectTask={selectTaskAndReveal}
                  onCopyText={copyText}
                  onRetry={() => {
                    void hub.retrySelected();
                  }}
                  onDismissError={hub.clearDetailError}
                />
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
          {compactStatusBar ? (
            <div className="status-filter" ref={statusMenuRef}>
              <button
                type="button"
                className="status-filter-trigger"
                data-active={statusFilter !== null}
                aria-haspopup="menu"
                aria-expanded={statusMenuOpen}
                aria-label={
                  activeStatusLabel === null
                    ? "Filter by status — all statuses"
                    : `Filter by status — ${activeStatusLabel}`
                }
                onClick={() => setStatusMenuOpen((current) => !current)}
              >
                <Filter size={12} aria-hidden="true" />
                <span className="status-filter-label">
                  {activeStatusLabel ?? "All statuses"}
                </span>
                <span className="status-filter-count tabular">
                  {statusFilter === null ? totalTasks : statusCounts[statusFilter]}
                </span>
                <ChevronDown size={12} aria-hidden="true" />
              </button>
              {statusMenuOpen ? (
                <div className="status-filter-menu" role="menu" aria-label="Filter by status">
                  <button
                    type="button"
                    className="status-filter-option"
                    role="menuitemradio"
                    aria-checked={statusFilter === null}
                    data-active={statusFilter === null}
                    onClick={() => {
                      setStatusFilter(null);
                      closeStatusMenu();
                    }}
                  >
                    <span className="status-filter-check" aria-hidden="true">
                      {statusFilter === null ? <Check size={12} /> : null}
                    </span>
                    <span aria-hidden="true" />
                    <span className="status-filter-option-label">All statuses</span>
                    <span className="tabular muted">{totalTasks}</span>
                  </button>
                  {TASK_STATUSES.map((status) => (
                    <button
                      key={status}
                      type="button"
                      className="status-filter-option"
                      role="menuitemradio"
                      aria-checked={statusFilter === status}
                      data-active={statusFilter === status}
                      onClick={() => {
                        setStatusFilter(status);
                        closeStatusMenu();
                      }}
                    >
                      <span className="status-filter-check" aria-hidden="true">
                        {statusFilter === status ? <Check size={12} /> : null}
                      </span>
                      <span className="state-dot" data-state={status} aria-hidden="true" />
                      <span className="status-filter-option-label">
                        {TASK_STATUS_META[status].label}
                      </span>
                      <span className="tabular muted">{statusCounts[status]}</span>
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          ) : (
            TASK_STATUSES.map((status) => (
              <button
                key={status}
                type="button"
                className="status-counter"
                data-tone={status}
                data-active={statusFilter === status}
                aria-pressed={statusFilter === status}
                onClick={() => filterByStatus(status)}
              >
                <span className="state-dot" data-state={status} aria-hidden="true" />
                <span>{TASK_STATUS_META[status].label}</span>
                <span className="tabular">{statusCounts[status]}</span>
              </button>
            ))
          )}
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
                issued={hub.enrollmentCode}
                busy={hub.issuingCode}
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

      <CommandPalette
        open={paletteOpen}
        tasks={tasks}
        selectedTask={hub.selectedTask}
        theme={hub.theme}
        onClose={() => setPaletteOpen(false)}
        onSelectTask={selectTaskAndReveal}
        onSelectMode={setMode}
        onOpenWorkers={openWorkers}
        onToggleInspector={toggleInspector}
        onFitBoard={() => atlasRef.current?.fit()}
        onArrangeBoard={() => atlasRef.current?.arrange()}
        onChangeTheme={hub.setTheme}
      />

      <ShortcutsModal open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />

      <ToastStack toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
