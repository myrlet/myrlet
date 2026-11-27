/**
 * Work Atlas — deterministic spatial layout and edge routing.
 *
 * Pure functions: the same Task list always produces the same positions.
 * Groups derive from `rootTaskId`; inside a group the tree spine runs left → right
 * by depth, and every subtree owns a contiguous band of leaf slots so parents
 * center on their children without overlap. Edges are limited to the three
 * relationships Hub records — parent, dependencies, and source.
 */

import type { SnapshotTaskSummary } from "../api/types";

export const NODE_WIDTH = 232;
export const NODE_HEIGHT = 140;
export const COL_GAP = 64;
export const COL_STEP = NODE_WIDTH + COL_GAP;
export const ROW_GAP = 26;
export const ROW_STEP = NODE_HEIGHT + ROW_GAP;
export const GROUP_GAP_X = 96;
export const GROUP_GAP_Y = 76;
/** Distance from a group's topmost card to its floating header label. */
export const GROUP_HEADER_OFFSET = 34;

export type AtlasEdgeKind = "parent" | "dependency" | "source";

export interface AtlasPosition { x: number; y: number; }

export interface AtlasNode { task: SnapshotTaskSummary; x: number; y: number; }

/** Floating uppercase label above a group; never a drawn container box. */
export interface AtlasGroupHeader { rootId: string; title: string; x: number; y: number; width: number; count: number; }

export interface AtlasLayout {
  nodes: AtlasNode[];
  groups: AtlasGroupHeader[];
  /** Deterministic base positions, used for drag snapping. */
  positions: Map<string, AtlasPosition>;
  width: number;
  height: number;
}

export interface AtlasEdge {
  key: string;
  kind: AtlasEdgeKind;
  sourceId: string;
  targetId: string;
  path: string;
  labelX: number;
  labelY: number;
  /** World-space endpoint of the routed curve (card edge, not card center). */
  sourceX: number;
  sourceY: number;
  targetX: number;
  targetY: number;
}

interface GroupDraft {
  rootId: string;
  title: string;
  createdAtMs: number;
  count: number;
  width: number;
  contentHeight: number;
  /** Block-local node positions (content starts at y = 0). */
  nodes: AtlasNode[];
  blockX: number;
  blockY: number;
}

const compareText = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);

const compareTasks = (a: SnapshotTaskSummary, b: SnapshotTaskSummary): number =>
  a.createdAtMs - b.createdAtMs || compareText(a.id, b.id);

/**
 * Lay out one group's local tree. A leaf consumes one vertical slot; a parent
 * centers on the slots its children span. Tasks whose parent is missing become
 * detached sub-roots with a small gap, so no Task is ever dropped.
 */
function layoutGroup(rootId: string, members: readonly SnapshotTaskSummary[]): GroupDraft {
  const ordered = [...members].sort(compareTasks);
  const byId = new Map(ordered.map((task) => [task.id, task]));
  const root = byId.get(rootId) ?? ordered[0];

  const childrenByParent = new Map<string, SnapshotTaskSummary[]>();
  for (const task of ordered) {
    const parentId = task.parentTaskId;
    if (parentId === null || parentId === task.id || !byId.has(parentId)) {
      continue;
    }
    const siblings = childrenByParent.get(parentId);
    if (siblings) siblings.push(task);
    else childrenByParent.set(parentId, [task]);
  }
  for (const siblings of childrenByParent.values()) siblings.sort(compareTasks);

  const nodes: AtlasNode[] = [];
  const placed = new Set<string>();
  let slotCursor = 0;
  let maxDepth = 0;

  const place = (task: SnapshotTaskSummary, depth: number): void => {
    if (placed.has(task.id)) {
      return;
    }
    placed.add(task.id);
    maxDepth = Math.max(maxDepth, depth);
    const kids = (childrenByParent.get(task.id) ?? []).filter((kid) => !placed.has(kid.id));
    const startSlot = slotCursor;
    if (kids.length === 0) {
      nodes.push({ task, x: depth * COL_STEP, y: startSlot * ROW_STEP });
      slotCursor += 1;
      return;
    }
    for (const kid of kids) {
      place(kid, depth + 1);
    }
    const centerSlot = (startSlot + slotCursor - 1) / 2;
    nodes.push({ task, x: depth * COL_STEP, y: centerSlot * ROW_STEP });
  };

  place(root, 0);
  for (const task of ordered) {
    if (!placed.has(task.id)) {
      slotCursor += 1;
      place(task, 0);
    }
  }

  return {
    rootId, title: root.title || root.id, createdAtMs: root.createdAtMs, count: nodes.length,
    width: maxDepth * COL_STEP + NODE_WIDTH,
    contentHeight: Math.max(NODE_HEIGHT, (slotCursor - 1) * ROW_STEP + NODE_HEIGHT),
    nodes, blockX: 0, blockY: 0,
  };
}

/**
 * Multi-node trees stack in the left band by descending size; standalone
 * Top-level Tasks pack into a compact grid beside them so the canvas reads as a
 * curated atlas rather than one tall column.
 */
export function buildAtlasLayout(tasks: readonly SnapshotTaskSummary[]): AtlasLayout {
  const byRoot = new Map<string, SnapshotTaskSummary[]>();
  for (const task of tasks) {
    const members = byRoot.get(task.rootTaskId);
    if (members) members.push(task);
    else byRoot.set(task.rootTaskId, [task]);
  }

  const drafts = [...byRoot.entries()]
    .map(([rootId, members]) => layoutGroup(rootId, members))
    .filter((draft) => draft.count > 0)
    .sort((a, b) => b.count - a.count || a.createdAtMs - b.createdAtMs || compareText(a.rootId, b.rootId));

  const multi = drafts.filter((draft) => draft.count > 1);
  const singles = drafts.filter((draft) => draft.count === 1);

  let leftHeight = 0;
  let leftWidth = 0;
  for (const draft of multi) {
    draft.blockX = 0;
    draft.blockY = leftHeight;
    leftHeight += GROUP_HEADER_OFFSET + draft.contentHeight + GROUP_GAP_Y;
    leftWidth = Math.max(leftWidth, draft.width);
  }
  leftHeight = Math.max(0, leftHeight - GROUP_GAP_Y);

  const cellWidth = NODE_WIDTH + GROUP_GAP_X;
  const cellHeight = GROUP_HEADER_OFFSET + NODE_HEIGHT + GROUP_GAP_Y;
  const rightX = multi.length > 0 ? leftWidth + GROUP_GAP_X : 0;
  let columns = 0;
  let rightWidth = 0;
  let rightHeight = 0;
  if (singles.length > 0) {
    const rowsThatFit = Math.max(1, Math.floor((leftHeight + GROUP_GAP_Y) / cellHeight));
    columns =
      multi.length === 0
        ? Math.max(1, Math.ceil(Math.sqrt(singles.length)))
        : Math.max(1, Math.ceil(singles.length / rowsThatFit));
    columns = Math.min(columns, singles.length);
    singles.forEach((draft, index) => {
      draft.blockX = rightX + (index % columns) * cellWidth;
      draft.blockY = Math.floor(index / columns) * cellHeight;
    });
    const rows = Math.ceil(singles.length / columns);
    rightWidth = rightX + (columns - 1) * cellWidth + NODE_WIDTH;
    rightHeight = (rows - 1) * cellHeight + GROUP_HEADER_OFFSET + NODE_HEIGHT;
  }

  const nodes: AtlasNode[] = [];
  const groups: AtlasGroupHeader[] = [];
  const positions = new Map<string, AtlasPosition>();
  for (const draft of drafts) {
    let top = Number.POSITIVE_INFINITY;
    for (const node of draft.nodes) {
      const x = draft.blockX + node.x;
      const y = draft.blockY + GROUP_HEADER_OFFSET + node.y;
      top = Math.min(top, y);
      nodes.push({ task: node.task, x, y });
      positions.set(node.task.id, { x, y });
    }
    groups.push({
      rootId: draft.rootId, title: draft.title, x: draft.blockX,
      y: top - GROUP_HEADER_OFFSET, width: draft.width, count: draft.count,
    });
  }

  return { nodes, groups, positions, width: Math.max(leftWidth, rightWidth), height: Math.max(leftHeight, rightHeight) };
}

const round = (value: number): number => Math.round(value * 100) / 100;

/* --------------------------------------------------------------------- */
/* Viewport framing                                                       */
/* --------------------------------------------------------------------- */

/** Canvas transform: world point `p` lands at `pan + p * scale` in stage space. */
export interface AtlasView { panX: number; panY: number; scale: number; }

export interface AtlasFramingBounds { minX: number; minY: number; width: number; height: number; }

export interface AtlasFramingOptions {
  minScale: number;
  maxScale: number;
  padX: number;
  padTop: number;
  padBottom: number;
}

export interface AtlasComfortOptions {
  /** Fit scales under this read as unreadable cards, so the first frame stays crisp. */
  minScale: number;
  maxScale: number;
  pad: number;
}

/** Whole-atlas frame; `zoomToFit` is the same scale so callers can compare it. */
export function fitAtlasFrame(
  width: number,
  height: number,
  bounds: AtlasFramingBounds,
  options: AtlasFramingOptions,
): { view: AtlasView; zoomToFit: number } {
  const availW = Math.max(120, width - options.padX * 2);
  const availH = Math.max(120, height - options.padTop - options.padBottom);
  const zoomToFit = Math.min(availW / bounds.width, availH / bounds.height);
  const scale = Math.min(options.maxScale, Math.max(options.minScale, zoomToFit));
  return {
    view: {
      scale,
      panX: options.padX + (availW - bounds.width * scale) / 2 - bounds.minX * scale,
      panY: options.padTop + (availH - bounds.height * scale) / 2 - bounds.minY * scale,
    },
    zoomToFit,
  };
}

/**
 * Readable first frame: anchor the atlas origin — never the atlas center — at a
 * comfortable inset, so cards stay crisp and keep their Delivery/Progress
 * footer. Off-screen groups remain reachable via the overview and `Fit`.
 */
export function comfortableAtlasFrame(
  bounds: AtlasFramingBounds,
  zoomToFit: number,
  options: AtlasComfortOptions,
): AtlasView {
  const scale = Math.min(options.maxScale, Math.max(options.minScale, zoomToFit));
  return { scale, panX: options.pad - bounds.minX * scale, panY: options.pad - bounds.minY * scale };
}

interface RoutedEdge {
  path: string;
  labelX: number;
  labelY: number;
  sourceX: number;
  sourceY: number;
  targetX: number;
  targetY: number;
}

function cubicRoute(
  start: AtlasPosition,
  controlA: AtlasPosition,
  controlB: AtlasPosition,
  end: AtlasPosition,
): RoutedEdge {
  const path =
    `M ${round(start.x)} ${round(start.y)} ` +
    `C ${round(controlA.x)} ${round(controlA.y)}, ` +
    `${round(controlB.x)} ${round(controlB.y)}, ${round(end.x)} ${round(end.y)}`;
  return {
    path,
    labelX: round((start.x + 3 * controlA.x + 3 * controlB.x + end.x) / 8),
    labelY: round((start.y + 3 * controlA.y + 3 * controlB.y + end.y) / 8),
    sourceX: round(start.x),
    sourceY: round(start.y),
    targetX: round(end.x),
    targetY: round(end.y),
  };
}


/**
 * Horizontal neighbours connect right → left; same-column cards connect bottom
 * → top (or bulge right) so a curve never cuts straight through a card face.
 */
function routeEdge(source: AtlasPosition, target: AtlasPosition): RoutedEdge {
  if (Math.abs(source.x - target.x) < 40) {
    if (target.y > source.y) {
      const start = { x: source.x + NODE_WIDTH / 2, y: source.y + NODE_HEIGHT };
      const end = { x: target.x + NODE_WIDTH / 2, y: target.y };
      const lift = Math.max(28, (end.y - start.y) / 2);
      return cubicRoute(start, { x: start.x, y: start.y + lift }, { x: end.x, y: end.y - lift }, end);
    }
    const start = { x: source.x + NODE_WIDTH, y: source.y + NODE_HEIGHT / 2 };
    const end = { x: target.x + NODE_WIDTH, y: target.y + NODE_HEIGHT / 2 };
    const bulge = Math.max(56, Math.abs(end.y - start.y) * 0.35 + 36);
    return cubicRoute(start, { x: start.x + bulge, y: start.y }, { x: end.x + bulge, y: end.y }, end);
  }

  const forward = target.x > source.x;
  const start = { x: forward ? source.x + NODE_WIDTH : source.x, y: source.y + NODE_HEIGHT / 2 };
  const end = { x: forward ? target.x : target.x + NODE_WIDTH, y: target.y + NODE_HEIGHT / 2 };
  const reach = Math.max(48, Math.abs(end.x - start.x) * 0.45);
  return cubicRoute(
    start,
    { x: start.x + (forward ? reach : -reach), y: start.y },
    { x: end.x - (forward ? reach : -reach), y: end.y },
    end,
  );
}

const EDGE_ORDER: Readonly<Record<AtlasEdgeKind, number>> = { source: 0, dependency: 1, parent: 2 };

/** Real relationship edges only: both endpoints visible and the relation recorded. */
export function computeAtlasEdges(
  tasks: readonly SnapshotTaskSummary[],
  positions: ReadonlyMap<string, AtlasPosition>,
): AtlasEdge[] {
  const edges: AtlasEdge[] = [];
  const seen = new Set<string>();

  const add = (kind: AtlasEdgeKind, sourceId: string, targetId: string): void => {
    if (sourceId === targetId) {
      return;
    }
    const key = `${kind}:${sourceId}->${targetId}`;
    const source = positions.get(sourceId);
    const target = positions.get(targetId);
    if (seen.has(key) || !source || !target) {
      return;
    }
    seen.add(key);
    edges.push({ key, kind, sourceId, targetId, ...routeEdge(source, target) });
  };

  for (const task of tasks) {
    if (!positions.has(task.id)) {
      continue;
    }
    if (task.parentTaskId !== null) {
      add("parent", task.parentTaskId, task.id);
    }
    if (task.sourceTaskId !== null) {
      add("source", task.sourceTaskId, task.id);
    }
    for (const dependencyId of new Set(task.dependencies)) {
      add("dependency", dependencyId, task.id);
    }
  }

  edges.sort((a, b) => EDGE_ORDER[a.kind] - EDGE_ORDER[b.kind]);
  return edges;
}

/** Everything a focused Task shows on the canvas. */
export interface AtlasSubgraph {
  nodeIds: Set<string>;
  edgeKeys: Set<string>;
}

/**
 * Full relationship path through `focusId`: every upstream ancestor, every
 * downstream descendant, plus the direct sibling Dependency and Source edges
 * incident to the focused Task. Transitive Dependency/Source chains stay out of
 * scope — Hub records them as direct sibling facts, not a graph walk.
 */
export function computeConnectedSubgraph(
  focusId: string,
  edges: readonly AtlasEdge[],
): AtlasSubgraph {
  const nodeIds = new Set<string>([focusId]);
  const edgeKeys = new Set<string>();

  const parentEdges = edges.filter((edge) => edge.kind === "parent");
  const parentOf = new Map<string, AtlasEdge>();
  const childrenOf = new Map<string, AtlasEdge[]>();
  for (const edge of parentEdges) {
    if (!parentOf.has(edge.targetId)) parentOf.set(edge.targetId, edge);
    const children = childrenOf.get(edge.sourceId);
    if (children) children.push(edge);
    else childrenOf.set(edge.sourceId, [edge]);
  }

  // Upward: the ancestry spine, one Parent edge per hop.
  let ancestorId = focusId;
  const visitedAncestors = new Set<string>([focusId]);
  for (;;) {
    const edge = parentOf.get(ancestorId);
    if (!edge || visitedAncestors.has(edge.sourceId)) break;
    visitedAncestors.add(edge.sourceId);
    nodeIds.add(edge.sourceId);
    edgeKeys.add(edge.key);
    ancestorId = edge.sourceId;
  }

  // Downward: every descendant subtree, breadth-first over Parent edges.
  const queue: string[] = [focusId];
  const visitedDescendants = new Set<string>([focusId]);
  while (queue.length > 0) {
    const current = queue.shift() as string;
    for (const edge of childrenOf.get(current) ?? []) {
      if (visitedDescendants.has(edge.targetId)) continue;
      visitedDescendants.add(edge.targetId);
      nodeIds.add(edge.targetId);
      edgeKeys.add(edge.key);
      queue.push(edge.targetId);
    }
  }

  // Direct sibling relations only; these never chain further.
  for (const edge of edges) {
    if (edge.kind === "parent") continue;
    if (edge.sourceId !== focusId && edge.targetId !== focusId) continue;
    edgeKeys.add(edge.key);
    nodeIds.add(edge.sourceId);
    nodeIds.add(edge.targetId);
  }

  return { nodeIds, edgeKeys };
}
