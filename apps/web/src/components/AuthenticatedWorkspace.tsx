/**
 * The authenticated Work Atlas workspace.
 *
 * Mounted only while a Hub session exists, so every listener, overlay, and
 * piece of view state here is released on disconnect or auth loss. It owns the
 * Board/List presentation switch, the one status filter, tree expansion, the
 * Inspector, and the overlay stack; the Hub session, Snapshot, SSE loop,
 * credentials, and Task selection stay with `useHub` and `useTaskDetail`.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { TASK_STATUS_META } from "../format";
import type { Theme } from "../state/useAppearance";
import {
  INSPECTOR_MAX_WIDTH,
  INSPECTOR_MIN_WIDTH,
  useInspectorPanel,
} from "../state/useInspectorPanel";
import type { useHub } from "../state/useHub";
import { useWorkspaceOutline } from "../state/useWorkspaceOutline";
import { useWorkspaceShortcuts } from "../state/useWorkspaceShortcuts";
import { CommandPalette, ShortcutsModal } from "./CommandPalette";
import { StatusBar } from "./StatusBar";
import { TaskDetailPanel } from "./TaskDetailPanel";
import { TaskGraphView } from "./TaskGraphView";
import type { AtlasBoardMeta, TaskGraphControls } from "./TaskGraphView";
import { TaskTreeTable } from "./TaskTreeTable";
import { TopBar } from "./TopBar";
import type { WorkspaceMode } from "./TopBar";
import { WorkersDrawer } from "./WorkersDrawer";
import { WorkspaceInspector } from "./WorkspaceInspector";

const INITIAL_ATLAS_META: AtlasBoardMeta = {
  zoomPercent: 100,
  relationCount: 0,
  canRestore: false,
};

type WorkspaceHub = Pick<ReturnType<typeof useHub>,
  | "connection" | "eventCursor" | "snapshot" | "tasks" | "workers"
  | "selectedTaskId" | "selectedTask" | "selectedTree" | "detailLoading" | "detailError"
  | "selectTask" | "retrySelected" | "clearDetailError" | "refresh" | "disconnect"
  | "enrollmentCode" | "issuingCode" | "createEnrollmentCode" | "rotateWorker" | "revokeWorker"
>;

interface AuthenticatedWorkspaceProps {
  readonly hub: WorkspaceHub;
  readonly baseUrl: string;
  readonly theme: Theme;
  readonly onChangeTheme: (theme: Theme) => void;
  readonly notify: (message: string, tone?: "neutral" | "error") => void;
}

export function AuthenticatedWorkspace({ hub, baseUrl, theme, onChangeTheme, notify }: AuthenticatedWorkspaceProps) {
  const { tasks, workers } = hub;
  const totalTasks = hub.snapshot?.totalTasks ?? tasks.length;
  const tasksTruncated = hub.snapshot?.tasksTruncated ?? false;

  /* The Board opens first; List provides a compact task outline. */
  const [mode, setMode] = useState<WorkspaceMode>("board");
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [workersOpen, setWorkersOpen] = useState(false);
  const [atlasMeta, setAtlasMeta] = useState<AtlasBoardMeta>(INITIAL_ATLAS_META);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  /* Phone layout: the five status counters collapse into one filter menu. */
  const [compactStatusBar, setCompactStatusBar] = useState(false);

  const workspaceRef = useRef<HTMLDivElement>(null);
  const atlasRef = useRef<TaskGraphControls | null>(null);
  const statusTriggerRef = useRef<HTMLButtonElement | null>(null);
  const inspector = useInspectorPanel({ backgroundRef: workspaceRef });

  const {
    statusFilter,
    setStatusFilter,
    clearFilter,
    rows,
    allExpanded,
    statusCounts,
    visibleTasks,
    toggleExpand,
    toggleExpandAll,
  } = useWorkspaceOutline(tasks);

  /** Snapshot facts for the selected Task, used while its detail read is pending. */
  const selectedSummary = useMemo(
    () => tasks.find((task) => task.id === hub.selectedTaskId) ?? null,
    [hub.selectedTaskId, tasks],
  );

  /* ------------------------------ Selection ---------------------------- */

  /** Selecting a Task always reveals its detail in the Inspector. */
  const selectTaskAndReveal = useCallback(
    (taskId: string) => {
      inspector.reveal();
      void hub.selectTask(taskId);
    },
    [inspector.reveal, hub.selectTask],
  );

  /** Clearing the selection retracts the Inspector, restoring full-bleed canvas. */
  const clearSelection = useCallback(() => {
    inspector.close();
    void hub.selectTask(null);
  }, [inspector.close, hub.selectTask]);

  /* ------------------------------ Notices ------------------------------ */

  const copyText = useCallback(
    (label: string, text: string) => {
      const clipboard: Clipboard | undefined =
        typeof navigator === "undefined" ? undefined : navigator.clipboard;
      if (!clipboard || typeof clipboard.writeText !== "function") {
        notify(`Could not copy ${label}`, "error");
        return;
      }
      void clipboard.writeText(text).then(
        () => notify(`Copied ${label}`),
        () => notify(`Could not copy ${label}`, "error"),
      );
    },
    [notify],
  );

  const openWorkers = useCallback(() => {
    setWorkersOpen(true);
  }, []);

  /* ----------------------------- Worker ops --------------------------- */

  const refresh = useCallback(() => {
    void hub.refresh();
  }, [hub.refresh]);

  const rotateWorker = useCallback(
    async (workerId: string) => {
      const response = await hub.rotateWorker(workerId);
      notify("Credential rotated");
      return response;
    },
    [hub.rotateWorker, notify],
  );

  const revokeWorker = useCallback(
    async (workerId: string): Promise<void> => {
      await hub.revokeWorker(workerId);
      notify("Worker revoked");
    },
    [hub.revokeWorker, notify],
  );

  /* ---------------------------- Responsiveness ------------------------- */

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

  /* ------------------------------ Keyboard ----------------------------- */

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

  /** Close the phone status menu and put focus back on its trigger. */
  const closeStatusMenu = useCallback(() => {
    setStatusMenuOpen(false);
    statusTriggerRef.current?.focus();
  }, []);

  const togglePalette = useCallback(() => setPaletteOpen((current) => !current), []);
  const closePalette = useCallback(() => setPaletteOpen(false), []);
  const openShortcuts = useCallback(() => setShortcutsOpen(true), []);
  const closeShortcuts = useCallback(() => setShortcutsOpen(false), []);
  const closeWorkers = useCallback(() => setWorkersOpen(false), []);
  const toggleWorkers = useCallback(() => setWorkersOpen((current) => !current), []);
  const fitBoard = useCallback(() => atlasRef.current?.fit(), []);
  const focusSelected = useCallback(() => atlasRef.current?.focusSelected(), []);

  useWorkspaceShortcuts({
    mode,
    paletteOpen,
    shortcutsOpen,
    workersOpen,
    statusMenuOpen,
    filterApplied: statusFilter !== null,
    selectionActive: hub.selectedTaskId !== null || inspector.open,
    hasSelectedTask: hub.selectedTaskId !== null,
    togglePalette,
    closePalette,
    openShortcuts,
    closeShortcuts,
    closeWorkers,
    closeStatusMenu,
    clearFilter,
    clearWorkspace: clearSelection,
    toggleInspector: inspector.toggle,
    fitBoard,
    focusSelected,
    moveSelection,
  });

  /* ------------------------------ Render ------------------------------- */

  const onlineWorkerCount = workers.filter(
    (worker) => worker.online && !worker.revoked,
  ).length;
  const boardMode = mode === "board";
  const activeStatusLabel =
    statusFilter === null ? null : TASK_STATUS_META[statusFilter].label;

  return (
    <>
      <TopBar
        mode={mode}
        onSelectMode={setMode}
        baseUrl={baseUrl}
        connection={hub.connection}
        eventCursor={hub.eventCursor}
        onlineWorkerCount={onlineWorkerCount}
        workerCount={workers.length}
        workersOpen={workersOpen}
        onToggleWorkers={toggleWorkers}
        onRefresh={refresh}
        theme={theme}
        onChangeTheme={onChangeTheme}
        onDisconnect={hub.disconnect}
        onOpenShortcuts={openShortcuts}
      />

      <div className="workspace-split">
        <div className="view-body" data-view={mode} ref={workspaceRef}>
          {boardMode ? (
            <TaskGraphView
              tasks={visibleTasks}
              workers={workers}
              selectedTaskId={hub.selectedTaskId}
              inspectorOpen={inspector.open}
              activeStatusLabel={activeStatusLabel}
              onToggleInspector={inspector.toggle}
              onClearFilter={clearFilter}
              onSelectTask={selectTaskAndReveal}
              onClearSelection={clearSelection}
              onBoardMetaChange={setAtlasMeta}
              controlsRef={atlasRef}
            />
          ) : (
            <TaskTreeTable
              rows={rows}
              workers={workers}
              totalTasks={totalTasks}
              loadedCount={tasks.length}
              tasksTruncated={tasksTruncated}
              selectedTaskId={hub.selectedTaskId}
              allExpanded={allExpanded}
              inspectorOpen={inspector.open}
              activeStatusLabel={activeStatusLabel}
              onSelectTask={selectTaskAndReveal}
              onToggleExpand={toggleExpand}
              onToggleExpandAll={toggleExpandAll}
              onToggleInspector={inspector.toggle}
              onCopyId={(taskId: string) => copyText("task id", taskId)}
              hasActiveFilter={statusFilter !== null}
              onClearFilters={clearFilter}
              compact={compactStatusBar}
            />
          )}
        </div>

        {inspector.open ? (
          <>
            <div
              className="resize-handle"
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize inspector"
              aria-valuemin={INSPECTOR_MIN_WIDTH}
              aria-valuemax={INSPECTOR_MAX_WIDTH}
              aria-valuenow={Math.round(inspector.width)}
              tabIndex={0}
              onPointerDown={inspector.beginResize}
              onKeyDown={inspector.resizeKeyDown}
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
                hub.selectedTaskId !== null ? clearSelection : inspector.close
              }
            />
            <div
              ref={inspector.panelRef}
              className="inspector-slot"
              style={{ width: `${inspector.width}px` }}
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
                  workers={workers}
                  onSelectTask={selectTaskAndReveal}
                  onOpenWorkers={openWorkers}
                  onClose={inspector.close}
                />
              )}
            </div>
          </>
        ) : null}
      </div>

      <StatusBar
        compact={compactStatusBar}
        statusFilter={statusFilter}
        statusCounts={statusCounts}
        totalTasks={totalTasks}
        relationCount={atlasMeta.relationCount}
        zoomPercent={boardMode ? atlasMeta.zoomPercent : null}
        menuOpen={statusMenuOpen}
        triggerRef={statusTriggerRef}
        onMenuOpenChange={setStatusMenuOpen}
        onSelectStatus={setStatusFilter}
      />

      {workersOpen ? (
        <WorkersDrawer
          onlineWorkerCount={onlineWorkerCount}
          baseUrl={baseUrl}
          workers={workers}
          issued={hub.enrollmentCode}
          busy={hub.issuingCode}
          onRefresh={refresh}
          onClose={() => setWorkersOpen(false)}
          onCreateEnrollmentCode={hub.createEnrollmentCode}
          onRotateWorker={rotateWorker}
          onRevokeWorker={revokeWorker}
          onCopyText={copyText}
        />
      ) : null}

      <CommandPalette
        open={paletteOpen}
        tasks={tasks}
        selectedTask={hub.selectedTask}
        theme={theme}
        onClose={() => setPaletteOpen(false)}
        onSelectTask={selectTaskAndReveal}
        onSelectMode={setMode}
        onOpenWorkers={openWorkers}
        onToggleInspector={inspector.toggle}
        onFitBoard={() => atlasRef.current?.fit()}
        onArrangeBoard={() => atlasRef.current?.arrange()}
        onChangeTheme={onChangeTheme}
      />

      <ShortcutsModal open={shortcutsOpen} onClose={() => setShortcutsOpen(false)} />
    </>
  );
}
