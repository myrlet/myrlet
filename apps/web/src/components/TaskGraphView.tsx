/**
 * Work Atlas — interactive spatial canvas for real Task relationships.
 *
 * Composition only: the deterministic layout lives in `workAtlasLayout`, the
 * camera in `atlas/useAtlasViewport`, persisted offsets in
 * `atlas/useAtlasOffsets`, and pointer gestures in `atlas/useAtlasPointer`.
 * Everything shown is factual — five Task states, the current Delivery, and
 * Harness-reported Progress only. Positions are presentation state: dragging a
 * card never mutates Task data.
 */

import { useCallback, useEffect, useRef } from "react";
import type { MutableRefObject } from "react";

import type { SnapshotTaskSummary, WorkerRecord } from "../api/types";
import { DEFAULT_ATLAS_STORAGE_KEY } from "./atlas/atlasOffsets";
import { AtlasCanvas } from "./atlas/AtlasCanvas";
import { AtlasDock, AtlasEmptyBoard, AtlasZoom } from "./atlas/AtlasChrome";
import { AtlasMinimap } from "./atlas/AtlasMinimap";
import {
  MAX_SCALE,
  MIN_SCALE,
  ZOOM_FACTOR,
  centerOnWorldPoint,
  densityFor,
  visibleWorldRect,
} from "./atlas/atlasViewport";
import { useAtlasFocus } from "./atlas/useAtlasFocus";
import { useAtlasModel } from "./atlas/useAtlasModel";
import { useAtlasOffsets } from "./atlas/useAtlasOffsets";
import { useAtlasPointer } from "./atlas/useAtlasPointer";
import { useAtlasViewport } from "./atlas/useAtlasViewport";

export interface TaskGraphControls {
  fit: () => void;
  focusSelected: () => void;
  resetZoom: () => void;
  arrange: () => void;
  restore: () => void;
}

export interface AtlasBoardMeta {
  zoomPercent: number;
  relationCount: number;
  canRestore: boolean;
}

export interface TaskGraphViewProps {
  readonly tasks: readonly SnapshotTaskSummary[];
  readonly workers: readonly WorkerRecord[];
  readonly selectedTaskId: string | null;
  readonly inspectorOpen: boolean;
  readonly activeStatusLabel: string | null;
  readonly onToggleInspector: () => void;
  readonly onClearFilter: () => void;
  readonly onSelectTask: (taskId: string) => void;
  readonly onClearSelection: () => void;
  readonly onBoardMetaChange: (meta: AtlasBoardMeta) => void;
  readonly controlsRef: MutableRefObject<TaskGraphControls | null>;
  readonly storageKey?: string;
}

export function TaskGraphView(props: TaskGraphViewProps) {
  const {
    tasks,
    workers,
    selectedTaskId,
    inspectorOpen,
    activeStatusLabel,
    onToggleInspector,
    onClearFilter,
    onSelectTask,
    onClearSelection,
    onBoardMetaChange,
    controlsRef,
  } = props;
  const storageKey = props.storageKey ?? DEFAULT_ATLAS_STORAGE_KEY;

  const stageRef = useRef<HTMLElement | null>(null);
  const { offsets, offsetsRef, setOffsets, restoreSnapshot, storageKeyRef, arrange, restore } =
    useAtlasOffsets(storageKey);
  const { layout, positions, edges, workerById, bounds } = useAtlasModel(tasks, workers, offsets);
  const { hoveredTaskId, setHoveredTaskId, subgraph } = useAtlasFocus(edges, selectedTaskId);
  const {
    view,
    setView,
    viewport,
    zoomPercent,
    fit,
    focusSelected,
    resetZoom,
    zoomTo,
  } = useAtlasViewport({ stageRef, bounds, groups: layout.groups, positions, selectedTaskId });
  const pointer = useAtlasPointer({
    stageRef,
    view,
    setView,
    basePositions: layout.positions,
    offsetsRef,
    setOffsets,
    storageKeyRef,
    onClearSelection,
  });

  useEffect(() => {
    controlsRef.current = { fit, focusSelected, resetZoom, arrange, restore };
    return () => {
      controlsRef.current = null;
    };
  }, [arrange, controlsRef, fit, focusSelected, resetZoom, restore]);

  useEffect(() => {
    onBoardMetaChange({
      zoomPercent,
      relationCount: edges.length,
      canRestore: restoreSnapshot !== null,
    });
  }, [edges.length, onBoardMetaChange, restoreSnapshot, zoomPercent]);

  const panToWorldPoint = useCallback(
    (point: { x: number; y: number }) => {
      setView((previous) =>
        centerOnWorldPoint(previous, point.x, point.y, viewport.width, viewport.height),
      );
    },
    [setView, viewport.height, viewport.width],
  );

  const viewWorld = visibleWorldRect(view, viewport);
  const density = densityFor(view.scale);

  return (
    <div className="atlas-board">
      <AtlasDock
        activeStatusLabel={activeStatusLabel}
        inspectorOpen={inspectorOpen}
        selectionActive={selectedTaskId !== null}
        canRestore={restoreSnapshot !== null}
        onClearFilter={onClearFilter}
        onFit={fit}
        onFocusSelected={focusSelected}
        onRestore={restore}
        onArrange={arrange}
        onToggleInspector={onToggleInspector}
      />

      <AtlasCanvas
        stageRef={stageRef}
        frame={{ view, panning: pointer.panning }}
        model={{ layout, positions, edges, workerById, density }}
        focus={{
          selectedTaskId,
          hoveredTaskId,
          subgraph,
          onHover: setHoveredTaskId,
          onSelectTask,
        }}
        interaction={{
          draggingTaskId: pointer.nodeDragId,
          onPointerDown: pointer.handlePointerDown,
          onPointerMove: pointer.handlePointerMove,
          onPointerUp: pointer.finishDrag,
        }}
        empty={
          tasks.length === 0 ? (
            <AtlasEmptyBoard activeStatusLabel={activeStatusLabel} onClearFilter={onClearFilter} />
          ) : null
        }
      >
        {tasks.length === 0 ? null : (
          <>
            {bounds === null ? null : (
              <AtlasMinimap
                nodes={layout.nodes}
                positions={positions}
                bounds={bounds}
                viewWorld={viewWorld}
                onPanTo={panToWorldPoint}
              />
            )}
            <AtlasZoom
              scale={view.scale}
              zoomPercent={zoomPercent}
              minScale={MIN_SCALE}
              maxScale={MAX_SCALE}
              zoomFactor={ZOOM_FACTOR}
              onZoomTo={zoomTo}
              onResetZoom={resetZoom}
            />
            <div className="atlas-hint">
              Drag empty space or scroll to pan · Drag cards to move · ⌘/Ctrl + scroll to zoom · F fits all
            </div>
          </>
        )}
      </AtlasCanvas>
    </div>
  );
}
