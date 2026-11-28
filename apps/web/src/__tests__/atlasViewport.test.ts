/**
 * Pure Board viewport math: framing, zoom anchoring, drag settling, and the
 * minimap coordinate mapping. No React and no DOM here.
 */

import { describe, expect, it } from "vitest";

import {
  MAX_SCALE,
  MIN_SCALE,
  atlasBounds,
  densityFor,
  fitWholeAtlas,
  initialAtlasView,
  minimapScaleFor,
  scaleAroundPoint,
  settleOffset,
  visibleWorldRect,
  worldPointFromMinimap,
} from "../components/atlas/atlasViewport";
import { NODE_HEIGHT, NODE_WIDTH } from "../components/workAtlasLayout";
import type { AtlasFramingBounds, AtlasGroupHeader, AtlasPosition } from "../components/workAtlasLayout";

const BOUNDS: AtlasFramingBounds = { minX: 0, minY: 34, width: 600, height: 300 };

const group = (overrides: Partial<AtlasGroupHeader> = {}): AtlasGroupHeader => ({
  rootId: "root",
  title: "Root",
  x: 0,
  y: 0,
  width: NODE_WIDTH,
  count: 1,
  ...overrides,
});

describe("atlas viewport framing", () => {
  it("fits the whole atlas without magnifying past natural size", () => {
    const huge = fitWholeAtlas(4000, 3000, BOUNDS);
    expect(huge.scale).toBe(1);
    // Centered with the horizontal pad.
    expect(huge.panX).toBeGreaterThanOrEqual(48);

    const small = fitWholeAtlas(320, 240, BOUNDS);
    expect(small.scale).toBeLessThan(1);
    expect(small.scale).toBeGreaterThanOrEqual(MIN_SCALE);
  });

  it("keeps a readable first frame on a wide stage and a single root on a phone", () => {
    const positions = new Map<string, AtlasPosition>([
      ["root", { x: 0, y: 34 }],
      ["child", { x: 296, y: 34 }],
    ]);
    const groups = [group({ rootId: "root", x: 0, y: 0 })];

    const wide = initialAtlasView(1400, 900, BOUNDS, groups, positions);
    expect(wide.scale).toBeLessThanOrEqual(1);
    expect(wide.scale).toBeGreaterThanOrEqual(0.82);

    const phone = initialAtlasView(360, 640, BOUNDS, groups, positions);
    expect(phone.scale).toBe(1);
    // The root card is centered horizontally.
    expect(phone.panX).toBeCloseTo((360 - NODE_WIDTH) / 2, 5);
    expect(phone.panY).toBe(56);
  });

  it("collapses to the comfortable origin when a full fit would be unreadable", () => {
    const wideBounds: AtlasFramingBounds = { minX: -500, minY: -200, width: 6000, height: 4000 };
    const view = initialAtlasView(1000, 700, wideBounds, [group()], new Map());
    expect(view.scale).toBeGreaterThanOrEqual(0.82);
    expect(view.scale).toBeLessThanOrEqual(0.92);
    // Anchored at a 40px inset instead of centering the giant atlas.
    expect(view.panX).toBeCloseTo(40 - wideBounds.minX * view.scale, 5);
  });

  it("derives node bounds from placed cards only", () => {
    expect(atlasBounds(new Map())).toBeNull();
    const bounds = atlasBounds(
      new Map<string, AtlasPosition>([
        ["a", { x: 10, y: 20 }],
        ["b", { x: 60, y: 80 }],
      ]),
    );
    expect(bounds).toEqual({
      minX: 10,
      minY: 20,
      width: 50 + NODE_WIDTH,
      height: 60 + NODE_HEIGHT,
    });
  });
});

describe("atlas viewport transform", () => {
  const view = { panX: 100, panY: 50, scale: 1 };

  it("zooms around a fixed stage point and clamps the scale", () => {
    const zoomed = scaleAroundPoint(view, 200, 100, 1.5);
    expect(zoomed.scale).toBe(1.5);
    // The stage point maps to the same world point before and after.
    expect((200 - zoomed.panX) / zoomed.scale).toBeCloseTo(200 - view.panX, 5);
    expect((100 - zoomed.panY) / zoomed.scale).toBeCloseTo(100 - view.panY, 5);

    expect(scaleAroundPoint(view, 0, 0, 99).scale).toBe(MAX_SCALE);
    expect(scaleAroundPoint(view, 0, 0, 0.01).scale).toBe(MIN_SCALE);
    expect(scaleAroundPoint(view, 0, 0, 1)).toBe(view);
  });

  it("reports the visible world rectangle and density", () => {
    expect(visibleWorldRect({ panX: -100, panY: -50, scale: 2 }, { width: 800, height: 600 })).toEqual({
      x: 50,
      y: 25,
      width: 400,
      height: 300,
    });
    expect(densityFor(0.6)).toBe("regular");
    expect(densityFor(0.5)).toBe("compact");
  });
});

describe("atlas drag settling and minimap mapping", () => {
  it("snaps a dragged card to the shared grid measured from its base", () => {
    expect(settleOffset({ x: 0, y: 34 }, { x: 100, y: 60 })).toEqual({ x: 96, y: 62 });
    // A 1px nudge still lands on the grid around the card's base offset.
    expect(settleOffset({ x: 0, y: 34 }, { x: 4, y: 1 })).toEqual({ x: 0, y: 2 });
  });

  it("maps a minimap click back to a world point", () => {
    const bounds: AtlasFramingBounds = { minX: 100, minY: 50, width: 800, height: 400 };
    const scale = minimapScaleFor(bounds);
    expect(scale).toBeLessThanOrEqual(1);
    expect(worldPointFromMinimap(bounds, scale, 0, 0)).toEqual({ x: 100, y: 50 });
    expect(worldPointFromMinimap(bounds, 0.5, 100, 40)).toEqual({ x: 300, y: 130 });
  });
});
