/**
 * Board pointer gestures: canvas panning and card dragging.
 *
 * A pointer press on empty canvas pans; a press on a card becomes a drag only
 * after the shared threshold, so a click still selects. Card drags are written
 * to storage once, at drag end, never during the gesture.
 */

import { useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent, RefObject } from "react";

import type { AtlasPosition, AtlasView } from "../workAtlasLayout";
import type { AtlasOffsets } from "./atlasOffsets";
import { writeStoredOffsets } from "./atlasOffsets";
import { DRAG_THRESHOLD, settleOffset } from "./atlasViewport";

type DragState =
  | {
      mode: "canvas";
      pointerId: number;
      startX: number;
      startY: number;
      startPanX: number;
      startPanY: number;
      moved: boolean;
    }
  | {
      mode: "node";
      pointerId: number;
      taskId: string;
      startX: number;
      startY: number;
      startOffset: AtlasPosition;
      scale: number;
      active: boolean;
    };

interface UseAtlasPointerArgs {
  readonly stageRef: RefObject<HTMLElement | null>;
  readonly view: AtlasView;
  readonly setView: (update: (previous: AtlasView) => AtlasView) => void;
  /** Deterministic layout positions: drag settling is measured from these. */
  readonly basePositions: ReadonlyMap<string, AtlasPosition>;
  readonly offsetsRef: { current: AtlasOffsets };
  readonly setOffsets: (next: AtlasOffsets) => void;
  readonly storageKeyRef: { current: string };
  readonly onClearSelection: () => void;
}

export interface AtlasPointerState {
  readonly panning: boolean;
  readonly nodeDragId: string | null;
  readonly handlePointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  readonly handlePointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
  readonly finishDrag: (event: ReactPointerEvent<HTMLElement>, clearSelection: boolean) => void;
}

export function useAtlasPointer({
  stageRef,
  view,
  setView,
  basePositions,
  offsetsRef,
  setOffsets,
  storageKeyRef,
  onClearSelection,
}: UseAtlasPointerArgs): AtlasPointerState {
  const [panning, setPanning] = useState(false);
  const [nodeDragId, setNodeDragId] = useState<string | null>(null);
  const dragRef = useRef<DragState | null>(null);

  const handlePointerDown = (event: ReactPointerEvent<HTMLElement>): void => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, .atlas-minimap, .atlas-zoom")) return;
    const nodeElement = target.closest<HTMLElement>(".atlas-node");
    const taskId = nodeElement?.dataset.taskId;
    if (taskId) {
      dragRef.current = {
        mode: "node",
        pointerId: event.pointerId,
        taskId,
        startX: event.clientX,
        startY: event.clientY,
        startOffset: offsetsRef.current[taskId] ?? { x: 0, y: 0 },
        scale: view.scale,
        active: false,
      };
      return;
    }
    dragRef.current = {
      mode: "canvas",
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startPanX: view.panX,
      startPanY: view.panY,
      moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setPanning(true);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLElement>): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const rawX = event.clientX - drag.startX;
    const rawY = event.clientY - drag.startY;
    if (drag.mode === "canvas") {
      if (!drag.moved && Math.hypot(rawX, rawY) <= DRAG_THRESHOLD) return;
      drag.moved = true;
      setView((previous) => ({ ...previous, panX: drag.startPanX + rawX, panY: drag.startPanY + rawY }));
      return;
    }
    if (!drag.active) {
      if (Math.hypot(rawX, rawY) <= DRAG_THRESHOLD) return;
      drag.active = true;
      setNodeDragId(drag.taskId);
      stageRef.current?.setPointerCapture(event.pointerId);
    }
    setOffsets({
      ...offsetsRef.current,
      [drag.taskId]: {
        x: drag.startOffset.x + rawX / drag.scale,
        y: drag.startOffset.y + rawY / drag.scale,
      },
    });
  };

  const releaseCapture = (pointerId: number): void => {
    if (stageRef.current?.hasPointerCapture(pointerId)) {
      stageRef.current.releasePointerCapture(pointerId);
    }
  };

  const finishDrag = (event: ReactPointerEvent<HTMLElement>, clearSelection: boolean): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setPanning(false);
    releaseCapture(event.pointerId);
    if (drag.mode === "canvas") {
      if (clearSelection && !drag.moved) onClearSelection();
      return;
    }
    setNodeDragId(null);
    if (!drag.active) return;
    const base = basePositions.get(drag.taskId);
    const offset = offsetsRef.current[drag.taskId];
    if (!base || !offset) return;
    const next = { ...offsetsRef.current, [drag.taskId]: settleOffset(base, offset) };
    setOffsets(next);
    // Spatial memory is written once, at drag end — never on mount or pan.
    writeStoredOffsets(storageKeyRef.current, next);
  };

  return { panning, nodeDragId, handlePointerDown, handlePointerMove, finishDrag };
}
