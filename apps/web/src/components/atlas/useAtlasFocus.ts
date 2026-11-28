/**
 * Board focus: hover, selection, and the highlighted relationship path.
 *
 * Hovering a card previews its connections; otherwise the selected Task drives
 * the highlight. `null` means the whole board renders at full strength.
 */

import { useMemo, useState } from "react";

import type { AtlasEdge, AtlasSubgraph } from "../workAtlasLayout";
import { computeConnectedSubgraph } from "../workAtlasLayout";

interface UseAtlasFocusResult {
  readonly hoveredTaskId: string | null;
  readonly setHoveredTaskId: (taskId: string | null) => void;
  readonly subgraph: AtlasSubgraph | null;
}

export function useAtlasFocus(
  edges: readonly AtlasEdge[],
  selectedTaskId: string | null,
): UseAtlasFocusResult {
  const [hoveredTaskId, setHoveredTaskId] = useState<string | null>(null);
  const focusId = hoveredTaskId ?? selectedTaskId;
  const subgraph = useMemo(
    () => (focusId === null ? null : computeConnectedSubgraph(focusId, edges)),
    [edges, focusId],
  );
  return { hoveredTaskId, setHoveredTaskId, subgraph };
}
