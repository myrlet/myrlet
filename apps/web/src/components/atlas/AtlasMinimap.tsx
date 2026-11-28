/**
 * Board overview minimap.
 *
 * A fold-away map of the deterministic layout with the current viewport
 * rectangle. Clicking anywhere in the body pans the stage to that world point.
 * Phones start folded because the first frame shows a single root card.
 */

import { useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

import { MINIMAP_HEIGHT, MINIMAP_WIDTH, minimapScaleFor, worldPointFromMinimap } from "./atlasViewport";
import { NODE_HEIGHT, NODE_WIDTH } from "../workAtlasLayout";
import type { AtlasFramingBounds, AtlasLayout, AtlasPosition } from "../workAtlasLayout";

/** Phones start folded: their first frame is one root card, not the atlas. */
const startsOpen = (): boolean =>
  typeof window === "undefined" ||
  typeof window.matchMedia !== "function" ||
  !window.matchMedia("(max-width: 720px)").matches;

interface AtlasMinimapProps {
  readonly nodes: AtlasLayout["nodes"];
  readonly positions: ReadonlyMap<string, AtlasPosition>;
  readonly bounds: AtlasFramingBounds | null;
  readonly viewWorld: { x: number; y: number; width: number; height: number };
  readonly onPanTo: (point: AtlasPosition) => void;
}

export function AtlasMinimap({ nodes, positions, bounds, viewWorld, onPanTo }: AtlasMinimapProps) {
  const [open, setOpen] = useState(startsOpen);
  if (bounds === null) return null;
  const minimapScale = minimapScaleFor(bounds);

  const handleClick = (event: ReactMouseEvent<HTMLButtonElement>): void => {
    const rect = event.currentTarget.getBoundingClientRect();
    onPanTo(
      worldPointFromMinimap(
        bounds,
        minimapScale,
        event.clientX - rect.left,
        event.clientY - rect.top,
      ),
    );
  };

  return (
    <div className="atlas-minimap" data-collapsed={!open}>
      <div className="atlas-minimap-head">
        <span className="atlas-minimap-title">OVERVIEW</span>
        <button
          type="button" className="atlas-minimap-collapse" aria-expanded={open}
          aria-label={open ? "Collapse minimap" : "Expand minimap"}
          title={open ? "Collapse minimap" : "Expand minimap"}
          onClick={() => setOpen((current) => !current)}
        >
          {open ? <ChevronDown size={12} aria-hidden="true" /> : <ChevronUp size={12} aria-hidden="true" />}
        </button>
      </div>
      {open ? (
        <button
          type="button" className="atlas-minimap-body"
          aria-label="Minimap overview — click to pan the canvas"
          onClick={handleClick}
          style={{ position: "relative", width: MINIMAP_WIDTH, height: MINIMAP_HEIGHT, padding: 0 }}
        >
          {nodes.map((node) => {
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
  );
}
