/**
 * Board stage: the transformed world layer, its relationship edges, group
 * labels, and Task cards, plus whatever overlays the board places on top.
 *
 * Pure rendering around one pan/zoom transform plus the pointer handlers that
 * turn presses into canvas pan and card drags. All data arrives derived, grouped
 * by the concern it belongs to.
 */

import type { PointerEvent as ReactPointerEvent, ReactNode, RefObject } from "react";

import { AtlasCardLayer } from "./AtlasCardLayer";
import { AtlasEdgeLayer, AtlasGroupLabels } from "./AtlasEdges";
import type { AtlasDensity } from "./atlasViewport";
import type { AtlasEdge, AtlasLayout, AtlasPosition, AtlasSubgraph, AtlasView } from "../workAtlasLayout";
import type { WorkerRecord } from "../../api/types";

/** What the camera currently shows. */
export interface AtlasFrame {
  readonly view: AtlasView;
  readonly panning: boolean;
}

/** Everything computed from Tasks, Workers, and persisted offsets. */
export interface AtlasModelView {
  readonly layout: AtlasLayout;
  readonly positions: ReadonlyMap<string, AtlasPosition>;
  readonly edges: readonly AtlasEdge[];
  readonly workerById: ReadonlyMap<string, WorkerRecord>;
  readonly density: AtlasDensity;
}

/** Hover, selection, and the highlighted relationship path. */
export interface AtlasFocusView {
  readonly selectedTaskId: string | null;
  readonly hoveredTaskId: string | null;
  readonly subgraph: AtlasSubgraph | null;
  readonly onHover: (taskId: string | null) => void;
  readonly onSelectTask: (taskId: string) => void;
}

/** Pointer gestures that move the camera or a card. */
export interface AtlasInteraction {
  readonly draggingTaskId: string | null;
  readonly onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  readonly onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
  readonly onPointerUp: (event: ReactPointerEvent<HTMLElement>, clearSelection: boolean) => void;
}

interface AtlasCanvasProps {
  readonly stageRef: RefObject<HTMLElement | null>;
  readonly frame: AtlasFrame;
  readonly model: AtlasModelView;
  readonly focus: AtlasFocusView;
  readonly interaction: AtlasInteraction;
  readonly empty: ReactNode;
  readonly children?: ReactNode;
}

export function AtlasCanvas({
  stageRef,
  frame,
  model,
  focus,
  interaction,
  empty,
  children,
}: AtlasCanvasProps) {
  const { view, panning } = frame;
  const { layout, positions, edges, workerById, density } = model;
  return (
    <section
      ref={stageRef} className="atlas-stage" aria-label="Task graph canvas" data-panning={panning}
      onPointerDown={interaction.onPointerDown} onPointerMove={interaction.onPointerMove}
      onPointerUp={(event) => interaction.onPointerUp(event, true)}
      onPointerCancel={(event) => interaction.onPointerUp(event, false)}
      style={{ touchAction: "none", userSelect: "none", cursor: panning ? "grabbing" : "grab" }}
    >
      {empty}
      {layout.nodes.length === 0 ? null : (
        <div
          className="atlas-world"
          style={{
            position: "absolute", left: 0, top: 0, width: layout.width, height: layout.height,
            transformOrigin: "0 0", transform: `translate(${view.panX}px, ${view.panY}px) scale(${view.scale})`,
          }}
        >
          <AtlasEdgeLayer width={layout.width} height={layout.height} edges={edges} subgraph={focus.subgraph} />
          <AtlasGroupLabels groups={layout.groups} />
          <AtlasCardLayer
            nodes={layout.nodes}
            positions={positions}
            selectedTaskId={focus.selectedTaskId}
            hoveredTaskId={focus.hoveredTaskId}
            draggingTaskId={interaction.draggingTaskId}
            dimmedIds={focus.subgraph?.nodeIds ?? null}
            workerByWorkerId={workerById}
            density={density}
            onHover={focus.onHover}
            onSelectTask={focus.onSelectTask}
          />
        </div>
      )}
      {children}
    </section>
  );
}
