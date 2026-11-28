/**
 * Board cards layer: places one `AtlasTaskNode` per laid-out Task.
 *
 * Iteration follows `layout.nodes`, so document order tracks the deterministic
 * layout rather than the Snapshot order.
 */

import type { WorkerRecord } from "../../api/types";
import { AtlasTaskNode } from "./AtlasTaskNode";
import type { AtlasDensity } from "./atlasViewport";
import type { AtlasNode, AtlasPosition } from "../workAtlasLayout";

interface AtlasCardLayerProps {
  readonly nodes: readonly AtlasNode[];
  readonly positions: ReadonlyMap<string, AtlasPosition>;
  readonly selectedTaskId: string | null;
  readonly hoveredTaskId: string | null;
  readonly draggingTaskId: string | null;
  readonly dimmedIds: ReadonlySet<string> | null;
  readonly workerByWorkerId: ReadonlyMap<string, WorkerRecord>;
  readonly density: AtlasDensity;
  readonly onHover: (taskId: string | null) => void;
  readonly onSelectTask: (taskId: string) => void;
}

export function AtlasCardLayer({
  nodes,
  positions,
  selectedTaskId,
  hoveredTaskId,
  draggingTaskId,
  dimmedIds,
  workerByWorkerId,
  density,
  onHover,
  onSelectTask,
}: AtlasCardLayerProps) {
  return (
    <>
      {nodes.map((node) => {
        const { task } = node;
        const position = positions.get(task.id) ?? node;
        const delivery = task.currentDelivery;
        return (
          <AtlasTaskNode
            key={task.id} task={task} x={position.x} y={position.y}
            selected={task.id === selectedTaskId} hovered={task.id === hoveredTaskId}
            dragging={task.id === draggingTaskId} density={density}
            dimmed={dimmedIds !== null && !dimmedIds.has(task.id)}
            worker={delivery ? workerByWorkerId.get(delivery.workerId) : undefined}
            onHover={onHover} onSelect={onSelectTask}
          />
        );
      })}
    </>
  );
}
