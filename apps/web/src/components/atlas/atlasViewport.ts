/**
 * Work Atlas viewport math.
 *
 * Pure pan/zoom/framing decisions shared by the Board controls, the pointer
 * interaction hooks, and the minimap. No React here: the same viewport size and
 * layout bounds always produce the same transform.
 */

import {
  NODE_HEIGHT,
  NODE_WIDTH,
  comfortableAtlasFrame,
  fitAtlasFrame,
} from "../workAtlasLayout";
import type {
  AtlasFramingBounds,
  AtlasFramingOptions,
  AtlasGroupHeader,
  AtlasPosition,
  AtlasView,
} from "../workAtlasLayout";

export const MIN_SCALE = 0.25;
export const MAX_SCALE = 1.65;
export const ZOOM_FACTOR = 1.15;
export const WHEEL_ZOOM_FACTOR = 1.12;
export const DRAG_THRESHOLD = 5;
const SNAP_STEP = 12;

const FIT_PAD_X = 48;
/* The dock row already clears the canvas, so padding only needs to keep the
   floating Task Group label (34px above the first card) in frame. */
const FIT_PAD_TOP = 44;
const FIT_PAD_BOTTOM = 120;
/** First view never shrinks below this scale: cards stay readable with their footer. */
const COMFORTABLE_MIN_SCALE = 0.82;
const COMFORTABLE_MAX_SCALE = 0.92;
const COMFORTABLE_PAD = 40;
/** Below this stage width the default frame shows one root card instead of the atlas. */
const NARROW_STAGE_WIDTH = 640;
const MOBILE_ROOT_SCALE = 1;
const MOBILE_ROOT_TOP = 56;

const COMPACT_SCALE = 0.52;
export const MINIMAP_WIDTH = 176;
export const MINIMAP_HEIGHT = 112;

export const INITIAL_ATLAS_VIEW: AtlasView = { panX: 48, panY: 48, scale: 1 };

const FIT_OPTIONS: AtlasFramingOptions = {
  minScale: MIN_SCALE,
  /* `Fit` frames the whole atlas but never magnifies past natural size, so a
     small board does not fill the screen with two giant cards. `0` and `+`
     remain the way to zoom in. */
  maxScale: 1,
  padX: FIT_PAD_X,
  padTop: FIT_PAD_TOP,
  padBottom: FIT_PAD_BOTTOM,
};

const COMFORT_OPTIONS = {
  minScale: COMFORTABLE_MIN_SCALE,
  maxScale: COMFORTABLE_MAX_SCALE,
  pad: COMFORTABLE_PAD,
};

export interface AtlasViewportSize {
  width: number;
  height: number;
}

export type AtlasDensity = "compact" | "regular";

const clampScale = (scale: number): number => Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));

export const snapToStep = (value: number): number =>
  Math.round(value / SNAP_STEP) * SNAP_STEP;

/**
 * Narrow default frame: the first Task Group's root card sits alone, fully in
 * frame with its group label, centered horizontally. Everything else stays one
 * `Fit` away.
 */
function mobileRootFrame(
  root: AtlasPosition,
  headerY: number,
  width: number,
  height: number,
): AtlasView {
  const inset = 16;
  const top = Math.min(root.y, headerY);
  const span = Math.max(NODE_HEIGHT, root.y + NODE_HEIGHT - top);
  const scale = Math.min(
    MOBILE_ROOT_SCALE,
    Math.max(MIN_SCALE, (width - inset * 2) / NODE_WIDTH),
    Math.max(MIN_SCALE, (height - MOBILE_ROOT_TOP - 24) / span),
  );
  return {
    scale,
    panX: (width - NODE_WIDTH * scale) / 2 - root.x * scale,
    panY: MOBILE_ROOT_TOP - top * scale,
  };
}

/** Whole-atlas frame used by `Fit`, `F`, and the status bar zoom readout. */
export function fitWholeAtlas(
  width: number,
  height: number,
  bounds: AtlasFramingBounds,
): AtlasView {
  return fitAtlasFrame(width, height, bounds, FIT_OPTIONS).view;
}

/**
 * First frame after the stage has a size. Narrow stages land on one readable
 * root card; wide stages fit the atlas, falling back to a crisp top-left frame
 * when a full fit would shrink cards into ants.
 */
export function initialAtlasView(
  width: number,
  height: number,
  bounds: AtlasFramingBounds,
  groups: readonly AtlasGroupHeader[],
  positions: ReadonlyMap<string, AtlasPosition>,
): AtlasView {
  if (width < NARROW_STAGE_WIDTH) {
    const first = [...groups].sort((a, b) => a.y - b.y || a.x - b.x)[0];
    const root = positions.get(first?.rootId ?? "");
    if (first && root) {
      return mobileRootFrame(root, first.y, width, height);
    }
  }
  const { view: fitted, zoomToFit } = fitAtlasFrame(width, height, bounds, FIT_OPTIONS);
  if (zoomToFit >= COMFORTABLE_MIN_SCALE) {
    return fitted;
  }
  return comfortableAtlasFrame(bounds, zoomToFit, COMFORT_OPTIONS);
}

/** World-space bounds of every placed card, or `null` for an empty board. */
export function atlasBounds(
  positions: ReadonlyMap<string, AtlasPosition>,
): AtlasFramingBounds | null {
  if (positions.size === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const position of positions.values()) {
    minX = Math.min(minX, position.x);
    minY = Math.min(minY, position.y);
    maxX = Math.max(maxX, position.x + NODE_WIDTH);
    maxY = Math.max(maxY, position.y + NODE_HEIGHT);
  }
  return { minX, minY, width: maxX - minX, height: maxY - minY };
}

/** Zoom around an arbitrary stage point, keeping that world point fixed. */
export function scaleAroundPoint(
  view: AtlasView,
  pointX: number,
  pointY: number,
  requestedScale: number,
): AtlasView {
  const scale = clampScale(requestedScale);
  if (scale === view.scale) return view;
  const worldX = (pointX - view.panX) / view.scale;
  const worldY = (pointY - view.panY) / view.scale;
  return { scale, panX: pointX - worldX * scale, panY: pointY - worldY * scale };
}

/** Center the viewport on a world point without changing zoom. */
export function centerOnWorldPoint(
  view: AtlasView,
  worldX: number,
  worldY: number,
  viewportWidth: number,
  viewportHeight: number,
): AtlasView {
  return {
    ...view,
    panX: viewportWidth / 2 - worldX * view.scale,
    panY: viewportHeight / 2 - worldY * view.scale,
  };
}

export const panBy = (view: AtlasView, dx: number, dy: number): AtlasView => ({
  ...view,
  panX: view.panX + dx,
  panY: view.panY + dy,
});

export const densityFor = (scale: number): AtlasDensity =>
  scale < COMPACT_SCALE ? "compact" : "regular";

/** Drags settle on the shared grid, measured from the card's base position. */
export function settleOffset(base: AtlasPosition, offset: AtlasPosition): AtlasPosition {
  return { x: snapToStep(base.x + offset.x) - base.x, y: snapToStep(base.y + offset.y) - base.y };
}

export const minimapScaleFor = (bounds: AtlasFramingBounds): number =>
  Math.min(MINIMAP_WIDTH / bounds.width, MINIMAP_HEIGHT / bounds.height, 1);

/** World rectangle currently visible through the stage viewport. */
export function visibleWorldRect(view: AtlasView, viewport: AtlasViewportSize): {
  x: number;
  y: number;
  width: number;
  height: number;
} {
  return {
    x: -view.panX / view.scale,
    y: -view.panY / view.scale,
    width: viewport.width / view.scale,
    height: viewport.height / view.scale,
  };
}

/** Map a click inside the minimap body to a world point. */
export function worldPointFromMinimap(
  bounds: AtlasFramingBounds,
  minimapScale: number,
  localX: number,
  localY: number,
): AtlasPosition {
  return { x: bounds.minX + localX / minimapScale, y: bounds.minY + localY / minimapScale };
}
