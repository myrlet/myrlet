/**
 * Board derived data: layout, per-card positions, relationship edges, and the
 * Worker lookup for current Deliveries.
 *
 * Every value is memoized from `tasks`, `workers`, and the persisted offsets, so
 * pan/zoom and hover changes never rebuild the deterministic layout.
 */

import { useMemo } from "react";

import type { SnapshotTaskSummary, WorkerRecord } from "../../api/types";
import { buildAtlasLayout, computeAtlasEdges } from "../workAtlasLayout";
import type { AtlasEdge, AtlasFramingBounds, AtlasLayout, AtlasPosition } from "../workAtlasLayout";
import { atlasBounds } from "./atlasViewport";
import type { AtlasOffsets } from "./atlasOffsets";

interface AtlasModel {
  readonly layout: AtlasLayout;
  readonly positions: ReadonlyMap<string, AtlasPosition>;
  readonly edges: readonly AtlasEdge[];
  readonly workerById: ReadonlyMap<string, WorkerRecord>;
  readonly bounds: AtlasFramingBounds | null;
}

export function useAtlasModel(
  tasks: readonly SnapshotTaskSummary[],
  workers: readonly WorkerRecord[],
  offsets: AtlasOffsets,
): AtlasModel {
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

  const bounds = useMemo(() => atlasBounds(positions), [positions]);
  const edges = useMemo(() => computeAtlasEdges(tasks, positions), [tasks, positions]);

  return { layout, positions, edges, workerById, bounds };
}
