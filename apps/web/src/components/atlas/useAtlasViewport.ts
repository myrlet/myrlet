/**
 * Board viewport state: pan, zoom, fit, focus, and the first frame.
 *
 * Owns the canvas transform and the stage's measured size. Persisted card
 * offsets and pointer gestures live in their own hooks; this one only answers
 * "where is the camera".
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Dispatch, RefObject, SetStateAction } from "react";

import { NODE_HEIGHT, NODE_WIDTH } from "../workAtlasLayout";
import type { AtlasFramingBounds, AtlasGroupHeader, AtlasPosition, AtlasView } from "../workAtlasLayout";
import {
  INITIAL_ATLAS_VIEW,
  WHEEL_ZOOM_FACTOR,
  centerOnWorldPoint,
  fitWholeAtlas,
  initialAtlasView,
  panBy,
  scaleAroundPoint,
} from "./atlasViewport";
import type { AtlasViewportSize } from "./atlasViewport";

interface UseAtlasViewportArgs {
  readonly stageRef: RefObject<HTMLElement | null>;
  readonly bounds: AtlasFramingBounds | null;
  readonly groups: readonly AtlasGroupHeader[];
  readonly positions: ReadonlyMap<string, AtlasPosition>;
  readonly selectedTaskId: string | null;
}

interface AtlasViewport {
  readonly view: AtlasView;
  readonly setView: Dispatch<SetStateAction<AtlasView>>;
  readonly viewport: AtlasViewportSize;
  readonly zoomPercent: number;
  readonly fit: () => void;
  readonly focusSelected: () => void;
  readonly resetZoom: () => void;
  readonly zoomTo: (resolve: (scale: number) => number) => void;
}

export function useAtlasViewport({
  stageRef,
  bounds,
  groups,
  positions,
  selectedTaskId,
}: UseAtlasViewportArgs): AtlasViewport {
  const [view, setView] = useState<AtlasView>(INITIAL_ATLAS_VIEW);
  const [viewport, setViewport] = useState<AtlasViewportSize>({ width: 0, height: 0 });
  const didAutoFitRef = useRef(false);

  const fit = useCallback(() => {
    const element = stageRef.current;
    if (!element || bounds === null) return;
    const width = element.clientWidth;
    const height = element.clientHeight;
    if (width <= 0 || height <= 0) return;
    setView(fitWholeAtlas(width, height, bounds));
  }, [bounds, stageRef]);

  const focusSelected = useCallback(() => {
    const element = stageRef.current;
    if (!element || selectedTaskId === null) return;
    const position = positions.get(selectedTaskId);
    if (!position) return;
    const width = element.clientWidth;
    const height = element.clientHeight;
    setView((previous) =>
      centerOnWorldPoint(
        previous,
        position.x + NODE_WIDTH / 2,
        position.y + NODE_HEIGHT / 2,
        width,
        height,
      ),
    );
  }, [positions, selectedTaskId, stageRef]);

  /** Zoom around the viewport center; `resolve` receives the current scale. */
  const zoomTo = useCallback(
    (resolve: (scale: number) => number) => {
      const element = stageRef.current;
      if (!element) return;
      const centerX = element.clientWidth / 2;
      const centerY = element.clientHeight / 2;
      setView((previous) => scaleAroundPoint(previous, centerX, centerY, resolve(previous.scale)));
    },
    [stageRef],
  );

  const resetZoom = useCallback(() => zoomTo(() => 1), [zoomTo]);

  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const update = (): void => {
      setViewport({ width: element.clientWidth, height: element.clientHeight });
    };
    update();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", update);
      return () => window.removeEventListener("resize", update);
    }
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [stageRef]);

  useEffect(() => {
    const element = stageRef.current;
    if (!element) return;
    const onWheel = (event: WheelEvent): void => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const rect = element.getBoundingClientRect();
        const pointerX = event.clientX - rect.left;
        const pointerY = event.clientY - rect.top;
        const factor = event.deltaY < 0 ? WHEEL_ZOOM_FACTOR : 1 / WHEEL_ZOOM_FACTOR;
        setView((previous) =>
          scaleAroundPoint(previous, pointerX, pointerY, previous.scale * factor),
        );
        return;
      }
      if (event.shiftKey) {
        const delta = event.deltaY !== 0 ? event.deltaY : event.deltaX;
        setView((previous) => panBy(previous, -delta, 0));
        return;
      }
      // Trackpads report both axes at once: glide on both so diagonal pans feel native.
      setView((previous) => panBy(previous, -event.deltaX, -event.deltaY));
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [stageRef]);

  useEffect(() => {
    if (didAutoFitRef.current || bounds === null || viewport.width <= 0 || viewport.height <= 0) {
      return;
    }
    didAutoFitRef.current = true;
    setView(initialAtlasView(viewport.width, viewport.height, bounds, groups, positions));
  }, [bounds, groups, positions, viewport.height, viewport.width]);

  const zoomPercent = Math.round(view.scale * 100);

  return useMemo(
    () => ({ view, setView, viewport, zoomPercent, fit, focusSelected, resetZoom, zoomTo }),
    [fit, focusSelected, resetZoom, view, viewport, zoomPercent, zoomTo],
  );
}
