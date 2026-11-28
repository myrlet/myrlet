/**
 * Board relationship rendering: the three real Hub edges plus group labels.
 *
 * Edges come from `computeAtlasEdges`. Selection or hover highlights the
 * connected path and dims everything else; no edge is invented.
 */

import { formatCount } from "../../format";
import type { AtlasEdge, AtlasEdgeKind, AtlasGroupHeader, AtlasSubgraph } from "../workAtlasLayout";

const markerId = (kind: AtlasEdgeKind, highlighted: boolean): string => {
  if (kind === "parent") {
    return highlighted ? "atlas-arrow-solid-accent" : "atlas-arrow-solid";
  }
  return highlighted ? "atlas-arrow-open-accent" : "atlas-arrow-open";
};

const edgePillText = (kind: AtlasEdgeKind): string | null =>
  kind === "dependency" ? "dep" : kind === "source" ? "retry" : null;

/** Markers referenced by `AtlasEdgeLayer`; one set per mounted board. */
function AtlasEdgeMarkers() {
  return (
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
  );
}

interface AtlasEdgeLayerProps {
  readonly width: number;
  readonly height: number;
  readonly edges: readonly AtlasEdge[];
  readonly subgraph: AtlasSubgraph | null;
}

export function AtlasEdgeLayer({ width, height, edges, subgraph }: AtlasEdgeLayerProps) {
  return (
    <svg
      className="atlas-edges" width={Math.max(1, width)} height={Math.max(1, height)}
      aria-hidden="true" style={{ position: "absolute", left: 0, top: 0, overflow: "visible", pointerEvents: "none" }}
    >
      <AtlasEdgeMarkers />
      {edges.map((edge) => {
        const highlighted = subgraph !== null && subgraph.edgeKeys.has(edge.key);
        const dimmed = subgraph !== null && !highlighted;
        const solid = edge.kind === "parent";
        const pillText = edgePillText(edge.kind);
        const pillWidth = pillText === null ? 0 : pillText.length * 5.6 + 12;
        return (
          <g key={edge.key}>
            <path
              className="atlas-edge" data-kind={edge.kind} d={edge.path} fill="none"
              data-highlighted={highlighted ? "true" : undefined}
              data-dimmed={dimmed ? "true" : undefined}
              stroke={highlighted ? "var(--accent)" : "var(--text-muted)"}
              strokeWidth={solid ? 1.4 : edge.kind === "dependency" ? 1.25 : 1.1}
              strokeDasharray={edge.kind === "parent" ? undefined : edge.kind === "dependency" ? "6 4" : "2 4"}
              opacity={dimmed ? 0.3 : edge.kind === "source" ? 0.62 : 0.9}
              markerEnd={`url(#${markerId(edge.kind, highlighted)})`}
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
  );
}

/** Floating Task Group labels; never a drawn container box. */
export function AtlasGroupLabels({ groups }: { readonly groups: readonly AtlasGroupHeader[] }) {
  return (
    <>
      {groups.map((group) => (
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
    </>
  );
}
