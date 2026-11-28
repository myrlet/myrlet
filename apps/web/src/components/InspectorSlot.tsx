/**
 * The Inspector slot of the workspace: a resize handle, an optional scrim on narrow
 * screens, and either the selected Task's detail or the workspace overview.
 *
 * Kept out of `AuthenticatedWorkspace` so the shell composes panels instead of holding
 * every panel's markup. The scrim is presented as a pointer-only sibling, never a parent
 * wrapper, so it can never swallow the panel's own close control.
 */

import type {
  SnapshotTaskSummary,
  TaskRecord,
  TaskTreeResponse,
  WorkerRecord,
} from "../api/types";
import {
  INSPECTOR_MAX_WIDTH,
  INSPECTOR_MIN_WIDTH,
  useInspectorPanel,
} from "../state/useInspectorPanel";
import { TaskDetailPanel } from "./TaskDetailPanel";
import { WorkspaceInspector } from "./WorkspaceInspector";

export interface InspectorSlotProps {
  readonly inspector: ReturnType<typeof useInspectorPanel>;
  readonly selectedTaskId: string | null;
  readonly selectedSummary: SnapshotTaskSummary | null;
  readonly selectedTask: TaskRecord | null;
  readonly selectedTree: TaskTreeResponse | null;
  readonly loading: boolean;
  readonly error: string | null;
  readonly tasks: readonly SnapshotTaskSummary[];
  readonly workers: readonly WorkerRecord[];
  readonly onSelectTask: (taskId: string) => void;
  readonly onClearSelection: () => void;
  readonly onCopyText: (label: string, text: string) => void;
  readonly onRetrySelected: () => void;
  readonly onDismissError: () => void;
  readonly onOpenWorkers: () => void;
}

export function InspectorSlot({
  inspector,
  selectedTaskId,
  selectedSummary,
  selectedTask,
  selectedTree,
  loading,
  error,
  tasks,
  workers,
  onSelectTask,
  onClearSelection,
  onCopyText,
  onRetrySelected,
  onDismissError,
  onOpenWorkers,
}: InspectorSlotProps) {
  if (!inspector.open) {
    return null;
  }
  return (
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
      {/* Narrow screens: a sibling scrim behind the panel, never a child that could
          swallow the panel's own close button. Pointer-only; keyboard users close with
          the panel's close button or Escape. */}
      <button
        type="button"
        className="inspector-scrim"
        tabIndex={-1}
        aria-hidden="true"
        onClick={selectedTaskId !== null ? onClearSelection : inspector.close}
      />
      <div
        ref={inspector.panelRef}
        className="inspector-slot"
        style={{ width: `${inspector.width}px` }}
        tabIndex={-1}
      >
        {selectedTaskId !== null ? (
          <TaskDetailPanel
            selectedTaskId={selectedTaskId}
            summary={selectedSummary}
            task={selectedTask}
            tree={selectedTree}
            loading={loading}
            error={error}
            onClose={onClearSelection}
            onSelectTask={onSelectTask}
            onCopyText={onCopyText}
            onRetry={onRetrySelected}
            onDismissError={onDismissError}
          />
        ) : (
          <WorkspaceInspector
            tasks={tasks}
            workers={workers}
            onSelectTask={onSelectTask}
            onOpenWorkers={onOpenWorkers}
            onClose={inspector.close}
          />
        )}
      </div>
    </>
  );
}
