/**
 * Work Atlas — interactive spatial canvas for real Task relationships.
 *
 * Cards form a horizontal local tree spine inside each Task Group, and all three
 * recorded relationship kinds render as live SVG curves: Parent, sibling
 * Dependency, and Source. Everything shown is factual — five Task states, the
 * current Delivery, and Harness-reported Progress only. Positions are
 * presentation state: dragging a card never mutates Task data.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { ChevronDown, ChevronUp, Minus, Plus, Waypoints } from "lucide-react";

import type { SnapshotTaskSummary, WorkerRecord } from "../api/types";
import { DELIVERY_STATUS_LABELS, TASK_STATUS_META, formatCount, formatRelativeTime } from "../format";
import { TaskStatusBadge } from "./StatusBadge";
import {
  NODE_HEIGHT,
  NODE_WIDTH,
  buildAtlasLayout,
  comfortableAtlasFrame,
  computeAtlasEdges,
  computeConnectedSubgraph,
  fitAtlasFrame,
} from "./workAtlasLayout";
import type { AtlasFramingOptions, AtlasPosition } from "./workAtlasLayout";

export interface WorkAtlasHandle {
  fit: () => void;
  focusSelected: () => void;
  arrange: () => void;
  restore: () => void;
}

export interface WorkAtlasMeta {
  zoomPercent: number;
  relationCount: number;
  canRestore: boolean;
}

export interface TaskGraphViewProps {
  tasks: readonly SnapshotTaskSummary[];
  workers: readonly WorkerRecord[];
  selectedTaskId: string | null;
  onSelectTask: (taskId: string) => void;
  onClearSelection: () => void;
  onOpenNewTask: () => void;
  onBoardMetaChange?: (meta: WorkAtlasMeta) => void;
  controlsRef?: MutableRefObject<WorkAtlasHandle | null>;
  /** localStorage key for manual card offsets; only written after a real drag. */
  storageKey?: string;
}

const MIN_SCALE = 0.25;
const MAX_SCALE = 1.65;
const ZOOM_FACTOR = 1.15;
const WHEEL_ZOOM_FACTOR = 1.12;
const DRAG_THRESHOLD = 5;
const SNAP_STEP = 12;
const FIT_PAD_X = 48;
const FIT_PAD_TOP = 52;
const FIT_PAD_BOTTOM = 136;
/** First view never shrinks below this scale: cards stay readable with their footer. */
const COMFORTABLE_MIN_SCALE = 0.82;
const COMFORTABLE_MAX_SCALE = 0.92;
const COMFORTABLE_PAD = 48;
const FIT_OPTIONS: AtlasFramingOptions = {
  minScale: MIN_SCALE,
  maxScale: MAX_SCALE,
  padX: FIT_PAD_X,
  padTop: FIT_PAD_TOP,
  padBottom: FIT_PAD_BOTTOM,
};
const COMFORT_OPTIONS = {
  minScale: COMFORTABLE_MIN_SCALE,
  maxScale: COMFORTABLE_MAX_SCALE,
  pad: COMFORTABLE_PAD,
};
const COMPACT_SCALE = 0.52;
const MINIMAP_WIDTH = 176;
const MINIMAP_HEIGHT = 112;
const DEFAULT_STORAGE_KEY = "myrlet.atlas.positions";
const NODE_STYLE = { width: NODE_WIDTH, height: NODE_HEIGHT } as const;

type Views = Record<string, AtlasPosition>;
type Density = "compact" | "regular";

type DragState =
  | { mode: "canvas"; pointerId: number; startX: number; startY: number; startPanX: number; startPanY: number; moved: boolean }
  | { mode: "node"; pointerId: number; taskId: string; startX: number; startY: number; startOffset: AtlasPosition; scale: number; active: boolean };

const clampScale = (scale: number): number => Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
const shortId = (id: string): string => (id.length <= 12 ? id : id.slice(0, 12));
const isWorkerOnline = (worker: WorkerRecord | undefined): boolean =>
  worker !== undefined && worker.online && !worker.revoked;
const snap = (value: number): number => Math.round(value / SNAP_STEP) * SNAP_STEP;

const isFiniteNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);

/** Manual offsets are convenience state; private-mode or quota failures are non-fatal. */
function readStoredOffsets(storageKey: string): Views {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (raw === null) {
      return {};
    }
    const parsed: unknown = JSON.parse(raw);
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
      return {};
    }
    const offsets: Views = {};
    for (const [taskId, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (value === null || typeof value !== "object") continue;
      const { x, y } = value as { x?: unknown; y?: unknown };
      if (isFiniteNumber(x) && isFiniteNumber(y)) {
        offsets[taskId] = { x, y };
      }
    }
    return offsets;
  } catch {
    return {};
  }
}

function writeStoredOffsets(storageKey: string, offsets: Views): void {
  try {
    const entries = Object.entries(offsets).filter(([, offset]) => offset.x !== 0 || offset.y !== 0);
    if (entries.length === 0) {
      window.localStorage.removeItem(storageKey);
      return;
    }
    window.localStorage.setItem(storageKey, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    /* Storage is optional: never break dragging over a persistence failure. */
  }
}

function clearStoredOffsets(storageKey: string): void {
  try {
    window.localStorage.removeItem(storageKey);
  } catch {
    /* Ignore unavailable storage. */
  }
}

interface AtlasTaskNodeProps {
  task: SnapshotTaskSummary;
  x: number;
  y: number;
  selected: boolean;
  hovered: boolean;
  dragging: boolean;
  dimmed: boolean;
  density: Density;
  worker: WorkerRecord | undefined;
  onHover: (taskId: string | null) => void;
  onSelect: (taskId: string) => void;
}

function AtlasTaskNode(props: AtlasTaskNodeProps) {
  const { task, x, y, selected, hovered, dragging, dimmed, density, worker, onHover, onSelect } = props;
  const delivery = task.currentDelivery;
  const online = isWorkerOnline(worker);
  const title = task.title || task.id;
  const statusLabel = TASK_STATUS_META[task.status].label;
  const chip = delivery?.executorId ?? delivery?.slotId ?? null;
  const descendantCount = Math.max(0, task.subtree.total - 1);
  const finishedDescendants = Math.max(
    0,
    task.subtree.finished - (task.status === "finished" ? 1 : 0),
  );
  const meta = delivery ? (
    <div className="atlas-node-meta">
      <span className="atlas-node-worker" title={`Worker ${delivery.workerId} — ${online ? "online" : "offline"}`}>
        <span className="connection-dot" data-state={online ? "online" : "offline"} aria-hidden="true" />
        <span className="mono truncate">{shortId(delivery.workerId)}</span>
      </span>
      {chip === null ? null : (
        <span className="atlas-node-chip truncate" title={delivery.executorId ? `Executor ${delivery.executorId}` : `Slot ${delivery.slotId}`}>
          {shortId(chip)}
        </span>
      )}
    </div>
  ) : descendantCount > 0 ? (
    <div className="atlas-node-meta"><span className="atlas-node-chip">
      {formatCount(descendantCount)} {descendantCount === 1 ? "subtask" : "subtasks"}
      {finishedDescendants > 0 ? ` · ${formatCount(finishedDescendants)} ended` : ""}
    </span></div>
  ) : task.category ? (
    <div className="atlas-node-meta"><span className="atlas-node-chip truncate">{task.category}</span></div>
  ) : null;
  return (
    <div
      role="button" tabIndex={0} className="atlas-node" data-task-id={task.id}
      data-selected={selected} data-hovered={hovered} data-dragging={dragging} data-density={density}
      data-dimmed={dimmed ? "true" : undefined}
      aria-label={`${title} — ${statusLabel}${delivery ? ` — Worker ${delivery.workerId} ${online ? "online" : "offline"}` : ""}`}
      title={`${title} (${task.id})`}
      style={{ ...NODE_STYLE, left: x, top: y, cursor: dragging ? "grabbing" : "grab" }}
      onClick={() => onSelect(task.id)}
      onPointerEnter={() => onHover(task.id)}
      onPointerLeave={() => onHover(null)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(task.id);
        }
      }}
    >
      <div className="atlas-node-head">
        <span className="atlas-node-id mono" title={task.id}>{shortId(task.id)}</span>
        <TaskStatusBadge status={task.status} />
      </div>
      <div className="atlas-node-title">{title}</div>
      {meta}
      {density === "compact" ? null : (
        <div className="atlas-node-footer">
          {delivery?.progress ? (
            <>
              <span className="atlas-node-progress truncate" title={delivery.progress.text}>{delivery.progress.text}</span>
              <span className="atlas-node-age tabular">{formatRelativeTime(delivery.progress.receivedAtMs)}</span>
            </>
          ) : delivery ? (
            <span className="atlas-node-progress">{DELIVERY_STATUS_LABELS[delivery.state]} · No progress reported</span>
          ) : (
            <span className="atlas-node-progress">No Delivery created</span>
          )}
        </div>
      )}
    </div>
  );
}

export function TaskGraphView(props: TaskGraphViewProps) {
  const { tasks, workers, selectedTaskId, onSelectTask, onClearSelection, onOpenNewTask, onBoardMetaChange, controlsRef } = props;
  const storageKey = props.storageKey ?? DEFAULT_STORAGE_KEY;
  const [view, setView] = useState({ panX: 48, panY: 48, scale: 1 });
  const [offsets, setOffsetsState] = useState<Views>(() => readStoredOffsets(storageKey));
  const [restoreSnapshot, setRestoreSnapshot] = useState<Views | null>(null);
  const [hoveredTaskId, setHoveredTaskId] = useState<string | null>(null);
  const [nodeDragId, setNodeDragId] = useState<string | null>(null);
  const [panning, setPanning] = useState(false);
  const [minimapOpen, setMinimapOpen] = useState(true);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const stageRef = useRef<HTMLElement | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const offsetsRef = useRef<Views>(offsets);
  const storageKeyRef = useRef(storageKey);
  const didAutoFitRef = useRef(false);

  const setOffsets = useCallback((next: Views) => {
    offsetsRef.current = next;
    setOffsetsState(next);
  }, []);

  // A different board (Wave 9B passes its own key) starts from that board's offsets.
  useEffect(() => {
    if (storageKeyRef.current === storageKey) return;
    storageKeyRef.current = storageKey;
    const stored = readStoredOffsets(storageKey);
    offsetsRef.current = stored;
    setOffsetsState(stored);
    setRestoreSnapshot(null);
  }, [storageKey]);

  const layout = useMemo(() => buildAtlasLayout(tasks), [tasks]);
  const workerById = useMemo(() => {
    const map = new Map<string, WorkerRecord>();
    for (const worker of workers) map.set(worker.workerId, worker);
    return map;
  }, [workers]);

  const positions = useMemo(() => {
    const map = new Map<string, AtlasPosition>();
    for (const node of layout.nodes) {
      const offset = offsets[node.task.id];
      map.set(node.task.id, { x: node.x + (offset?.x ?? 0), y: node.y + (offset?.y ?? 0) });
    }
    return map;
  }, [layout, offsets]);

  const bounds = useMemo(() => {
    if (positions.size === 0) return null;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const position of positions.values()) {
      minX = Math.min(minX, position.x);
      minY = Math.min(minY, position.y);
      maxX = Math.max(maxX, position.x + NODE_WIDTH);
      maxY = Math.max(maxY, position.y + NODE_HEIGHT);
    }
    return { minX, minY, width: maxX - minX, height: maxY - minY };
  }, [positions]);

  const edges = useMemo(() => computeAtlasEdges(tasks, positions), [tasks, positions]);
  const focusId = hoveredTaskId ?? selectedTaskId;
  const subgraph = useMemo(
    () => (focusId === null ? null : computeConnectedSubgraph(focusId, edges)),
    [edges, focusId],
  );
  const density: Density = view.scale < COMPACT_SCALE ? "compact" : "regular";
  const zoomPercent = Math.round(view.scale * 100);

  const fit = useCallback(() => {
    const element = stageRef.current;
    if (!element || bounds === null) return;
    const width = element.clientWidth, height = element.clientHeight;
    if (width <= 0 || height <= 0) return;
    setView(fitAtlasFrame(width, height, bounds, FIT_OPTIONS).view);
  }, [bounds]);

  const focusSelected = useCallback(() => {
    const element = stageRef.current;
    if (!element || selectedTaskId === null) return;
    const position = positions.get(selectedTaskId);
    if (!position) return;
    const centerX = element.clientWidth / 2, centerY = element.clientHeight / 2;
    setView((previous) => ({
      ...previous,
      panX: centerX - (position.x + NODE_WIDTH / 2) * previous.scale,
      panY: centerY - (position.y + NODE_HEIGHT / 2) * previous.scale,
    }));
  }, [positions, selectedTaskId]);

  /** Zoom around the viewport center; `resolve` receives the current scale. */
  const zoomTo = useCallback((resolve: (scale: number) => number) => {
    const element = stageRef.current;
    if (!element) return;
    const centerX = element.clientWidth / 2, centerY = element.clientHeight / 2;
    setView((previous) => {
      const scale = clampScale(resolve(previous.scale));
      if (scale === previous.scale) return previous;
      const worldX = (centerX - previous.panX) / previous.scale;
      const worldY = (centerY - previous.panY) / previous.scale;
      return { scale, panX: centerX - worldX * scale, panY: centerY - worldY * scale };
    });
  }, []);

  const arrange = useCallback(() => {
    const manual: Views = {};
    let hasManual = false;
    for (const [taskId, offset] of Object.entries(offsetsRef.current)) {
      if (offset.x !== 0 || offset.y !== 0) {
        manual[taskId] = offset;
        hasManual = true;
      }
    }
    setRestoreSnapshot(hasManual ? manual : null);
    setOffsets({});
    clearStoredOffsets(storageKeyRef.current);
  }, [setOffsets]);

  const restore = useCallback(() => {
    if (restoreSnapshot === null) return;
    setOffsets(restoreSnapshot);
    writeStoredOffsets(storageKeyRef.current, restoreSnapshot);
    setRestoreSnapshot(null);
  }, [restoreSnapshot, setOffsets]);

  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const update = (): void => setViewport({ width: element.clientWidth, height: element.clientHeight });
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const onWheel = (event: WheelEvent): void => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const rect = element.getBoundingClientRect();
        const pointerX = event.clientX - rect.left, pointerY = event.clientY - rect.top;
        const factor = event.deltaY < 0 ? WHEEL_ZOOM_FACTOR : 1 / WHEEL_ZOOM_FACTOR;
        setView((previous) => {
          const scale = clampScale(previous.scale * factor);
          if (scale === previous.scale) return previous;
          const worldX = (pointerX - previous.panX) / previous.scale;
          const worldY = (pointerY - previous.panY) / previous.scale;
          return { scale, panX: pointerX - worldX * scale, panY: pointerY - worldY * scale };
        });
        return;
      }
      if (event.shiftKey) {
        const delta = event.deltaY !== 0 ? event.deltaY : event.deltaX;
        setView((previous) => ({ ...previous, panX: previous.panX - delta }));
        return;
      }
      // Trackpads report both axes at once: glide on both so diagonal pans feel native.
      setView((previous) => ({
        ...previous,
        panX: previous.panX - event.deltaX,
        panY: previous.panY - event.deltaY,
      }));
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, []);

  useEffect(() => {
    if (!controlsRef) return;
    controlsRef.current = { fit, focusSelected, arrange, restore };
    return () => {
      controlsRef.current = null;
    };
  }, [arrange, controlsRef, fit, focusSelected, restore]);

  useEffect(() => {
    if (didAutoFitRef.current || bounds === null || viewport.width <= 0 || viewport.height <= 0) return;
    didAutoFitRef.current = true;
    const { view: fitted, zoomToFit } = fitAtlasFrame(
      viewport.width, viewport.height, bounds, FIT_OPTIONS,
    );
    if (zoomToFit >= COMFORTABLE_MIN_SCALE) {
      setView(fitted);
      return;
    }
    // Too much canvas to read at full fit: land on a crisp, readable top-left
    // origin instead of ants, and leave the rest one `Fit`/`F` away.
    setView(comfortableAtlasFrame(bounds, zoomToFit, COMFORT_OPTIONS));
  }, [bounds, viewport.height, viewport.width]);

  useEffect(() => {
    onBoardMetaChange?.({ zoomPercent, relationCount: edges.length, canRestore: restoreSnapshot !== null });
  }, [edges.length, onBoardMetaChange, restoreSnapshot, zoomPercent]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target;
      if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true']")) return;
      if (event.key.toLowerCase() === "f") {
        event.preventDefault();
        if (event.shiftKey) focusSelected();
        else fit();
      } else if (event.key === "0") {
        event.preventDefault();
        zoomTo(() => 1);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [fit, focusSelected, zoomTo]);

  const handlePointerDown = (event: ReactPointerEvent<HTMLElement>): void => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, .atlas-minimap, .atlas-zoom")) return;
    const nodeElement = target.closest<HTMLElement>(".atlas-node");
    const taskId = nodeElement?.dataset.taskId;
    if (taskId) {
      dragRef.current = {
        mode: "node", pointerId: event.pointerId, taskId,
        startX: event.clientX, startY: event.clientY,
        startOffset: offsetsRef.current[taskId] ?? { x: 0, y: 0 },
        scale: view.scale, active: false,
      };
      return;
    }
    dragRef.current = {
      mode: "canvas", pointerId: event.pointerId, startX: event.clientX, startY: event.clientY,
      startPanX: view.panX, startPanY: view.panY, moved: false,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    setPanning(true);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLElement>): void => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const rawX = event.clientX - drag.startX, rawY = event.clientY - drag.startY;
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
      [drag.taskId]: { x: drag.startOffset.x + rawX / drag.scale, y: drag.startOffset.y + rawY / drag.scale },
    });
  };

  const releaseCapture = (pointerId: number): void => {
    if (stageRef.current?.hasPointerCapture(pointerId)) stageRef.current.releasePointerCapture(pointerId);
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
    const base = layout.positions.get(drag.taskId);
    const offset = offsetsRef.current[drag.taskId];
    if (!base || !offset) return;
    const settled = { x: snap(base.x + offset.x) - base.x, y: snap(base.y + offset.y) - base.y };
    const next = { ...offsetsRef.current, [drag.taskId]: settled };
    setOffsets(next);
    // Spatial memory is written once, at drag end — never on mount or pan.
    writeStoredOffsets(storageKeyRef.current, next);
  };

  const minimapScale = bounds === null ? 1 : Math.min(MINIMAP_WIDTH / bounds.width, MINIMAP_HEIGHT / bounds.height, 1);
  const viewWorld = {
    x: -view.panX / view.scale, y: -view.panY / view.scale,
    width: viewport.width / view.scale, height: viewport.height / view.scale,
  };

  const handleMinimapClick = (event: ReactMouseEvent<HTMLButtonElement>): void => {
    if (bounds === null || minimapScale <= 0) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const worldX = bounds.minX + (event.clientX - rect.left) / minimapScale;
    const worldY = bounds.minY + (event.clientY - rect.top) / minimapScale;
    setView((previous) => ({
      ...previous,
      panX: viewport.width / 2 - worldX * previous.scale,
      panY: viewport.height / 2 - worldY * previous.scale,
    }));
  };

  return (
    <section
      ref={stageRef} className="atlas-stage" aria-label="Task graph" data-panning={panning}
      onPointerDown={handlePointerDown} onPointerMove={handlePointerMove}
      onPointerUp={(event) => finishDrag(event, true)} onPointerCancel={(event) => finishDrag(event, false)}
      style={{ touchAction: "none", userSelect: "none", cursor: panning ? "grabbing" : "grab" }}
    >
      {tasks.length === 0 ? (
        <div className="pane-empty">
          <Waypoints size={20} aria-hidden="true" />
          <span className="pane-empty-title">No Tasks to map yet</span>
          <span>Task Groups and their real Parent, Dependency, and Source links appear here once Tasks exist.</span>
          <button type="button" className="button button-sm button-primary" onClick={onOpenNewTask}>
            <Plus size={13} aria-hidden="true" />
            New Task
          </button>
        </div>
      ) : (
        <>
          <div
            className="atlas-world"
            style={{
              position: "absolute", left: 0, top: 0, width: layout.width, height: layout.height,
              transformOrigin: "0 0", transform: `translate(${view.panX}px, ${view.panY}px) scale(${view.scale})`,
            }}
          >
            <svg
              className="atlas-edges" width={Math.max(1, layout.width)} height={Math.max(1, layout.height)}
              aria-hidden="true" style={{ position: "absolute", left: 0, top: 0, overflow: "visible", pointerEvents: "none" }}
            >
              <defs>
                <marker id="atlas-arrow-solid" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
                  <path d="M0 0 L8 4 L0 8 Z" fill="var(--text-muted)" />
                </marker>
                <marker id="atlas-arrow-solid-accent" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
                  <path d="M0 0 L8 4 L0 8 Z" fill="var(--accent)" />
                </marker>
                <marker id="atlas-arrow-open" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
                  <path d="M1 1 L7 4 L1 7" fill="none" stroke="var(--text-muted)" strokeWidth="1.2" />
                </marker>
                <marker id="atlas-arrow-open-accent" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
                  <path d="M1 1 L7 4 L1 7" fill="none" stroke="var(--accent)" strokeWidth="1.2" />
                </marker>
              </defs>
              {edges.map((edge) => {
                const highlighted = subgraph !== null && subgraph.edgeKeys.has(edge.key);
                const dimmed = subgraph !== null && !highlighted;
                const solid = edge.kind === "parent";
                const marker = solid
                  ? highlighted ? "atlas-arrow-solid-accent" : "atlas-arrow-solid"
                  : highlighted ? "atlas-arrow-open-accent" : "atlas-arrow-open";
                const pillText = edge.kind === "dependency" ? "dep" : edge.kind === "source" ? "retry" : null;
                const pillWidth = pillText === null ? 0 : (pillText.length * 5.6 + 12);
                return (
                  <g key={edge.key}>
                    <path
                      className="atlas-edge" data-kind={edge.kind} d={edge.path} fill="none"
                      data-highlighted={highlighted ? "true" : undefined}
                      data-dimmed={dimmed ? "true" : undefined}
                      stroke={highlighted ? "var(--accent)" : "var(--text-muted)"}
                      strokeWidth={solid ? 1.4 : edge.kind === "dependency" ? 1.25 : 1.1}
                      strokeDasharray={edge.kind === "parent" ? undefined : edge.kind === "dependency" ? "6 4" : "2 4"}
                      opacity={dimmed ? 0.25 : edge.kind === "source" ? 0.62 : 0.9}
                      markerEnd={`url(#${marker})`}
                    />
                    {highlighted && pillText !== null ? (
                      <g className="atlas-edge-label" data-highlighted="true">
                        <rect
                          className="atlas-edge-pill" x={edge.labelX - pillWidth / 2} y={edge.labelY - 7}
                          width={pillWidth} height={14} rx={7} ry={7}
                          fill="var(--bg-canvas)" stroke="var(--border-subtle)" strokeWidth={1}
                        />
                        <text x={edge.labelX} y={edge.labelY} textAnchor="middle" dominantBaseline="middle">
                          {pillText}
                        </text>
                      </g>
                    ) : null}
                    {highlighted ? (
                      <g className="atlas-port" aria-hidden="true">
                        <circle className="atlas-port-dot" cx={edge.sourceX} cy={edge.sourceY} r={3.5} fill="var(--accent)" />
                        <circle className="atlas-port-dot" cx={edge.targetX} cy={edge.targetY} r={3.5} fill="var(--accent)" />
                      </g>
                    ) : null}
                  </g>
                );
              })}
            </svg>

            {layout.groups.map((group) => (
              <div
                key={group.rootId} className="atlas-group-header" aria-hidden="true"
                title={`${group.title} — ${formatCount(group.count)} ${group.count === 1 ? "Task" : "Tasks"}`}
                style={{ left: group.x, top: group.y, width: group.width }}
              >
                <span className="atlas-group-kicker">TASK GROUP</span>
                <span className="atlas-group-name truncate">{group.title}</span>
                <span className="atlas-group-rule" />
              </div>
            ))}

            {layout.nodes.map((node) => {
              const position = positions.get(node.task.id) ?? node;
              const delivery = node.task.currentDelivery;
              return (
                <AtlasTaskNode
                  key={node.task.id} task={node.task} x={position.x} y={position.y}
                  selected={node.task.id === selectedTaskId} hovered={node.task.id === hoveredTaskId}
                  dragging={node.task.id === nodeDragId} density={density}
                  dimmed={subgraph !== null && !subgraph.nodeIds.has(node.task.id)}
                  worker={delivery ? workerById.get(delivery.workerId) : undefined}
                  onHover={setHoveredTaskId} onSelect={onSelectTask}
                />
              );
            })}
          </div>

          <div className="atlas-minimap" data-collapsed={!minimapOpen}>
            <div className="atlas-minimap-head">
              <span className="atlas-minimap-title">OVERVIEW</span>
              <button
                type="button" className="atlas-minimap-collapse" aria-expanded={minimapOpen}
                aria-label={minimapOpen ? "Collapse minimap" : "Expand minimap"}
                title={minimapOpen ? "Collapse minimap" : "Expand minimap"}
                onClick={() => setMinimapOpen((open) => !open)}
              >
                {minimapOpen ? <ChevronDown size={12} aria-hidden="true" /> : <ChevronUp size={12} aria-hidden="true" />}
              </button>
            </div>
            {minimapOpen && bounds !== null ? (
              <button
                type="button" className="atlas-minimap-body"
                aria-label="Minimap overview — click to pan the canvas"
                onClick={handleMinimapClick}
                style={{ position: "relative", width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT, padding: 0 }}
              >
                {layout.nodes.map((node) => {
                  const position = positions.get(node.task.id);
                  if (!position) return null;
                  return (
                    <span
                      key={node.task.id} className="atlas-minimap-node" data-tone={node.task.status} aria-hidden="true"
                      style={{
                        position: "absolute",
                        left: (position.x - bounds.minX) * minimapScale,
                        top: (position.y - bounds.minY) * minimapScale,
                        width: Math.max(2, NODE_WIDTH * minimapScale),
                        height: Math.max(2, NODE_HEIGHT * minimapScale),
                      }}
                    />
                  );
                })}
                <span
                  className="atlas-minimap-viewport" aria-hidden="true"
                  style={{
                    position: "absolute",
                    left: (viewWorld.x - bounds.minX) * minimapScale,
                    top: (viewWorld.y - bounds.minY) * minimapScale,
                    width: Math.max(2, viewWorld.width * minimapScale),
                    height: Math.max(2, viewWorld.height * minimapScale),
                  }}
                />
              </button>
            ) : null}
          </div>

          <div className="atlas-zoom">
            <button type="button" className="atlas-zoom-btn" onClick={() => zoomTo((scale) => scale / ZOOM_FACTOR)}
              disabled={view.scale <= MIN_SCALE + 0.001} aria-label="Zoom out" title="Zoom out">
              <Minus size={13} aria-hidden="true" />
            </button>
            <button type="button" className="atlas-zoom-value tabular" onClick={() => zoomTo(() => 1)}
              aria-label={`Zoom ${zoomPercent}% — reset to 100%`} title="Reset zoom to 100%">
              {zoomPercent}%
            </button>
            <button type="button" className="atlas-zoom-btn" onClick={() => zoomTo((scale) => scale * ZOOM_FACTOR)}
              disabled={view.scale >= MAX_SCALE - 0.001} aria-label="Zoom in" title="Zoom in">
              <Plus size={13} aria-hidden="true" />
            </button>
          </div>

          <div className="atlas-hint">Drag empty space or scroll to pan · Drag cards to move · ⌘/Ctrl + scroll to zoom · F fits all</div>
        </>
      )}
    </section>
  );
}
