/**
 * Inspector lifecycle for the Work Atlas workspace.
 *
 * Owns whether the right Inspector is open, its desktop width, and the narrow
 * sheet behavior: a sibling scrim, an inert background, and returning focus to
 * the element that opened the sheet. Pointer and keyboard resizing live here
 * too, including cleanup when a drag is cancelled or the workspace unmounts.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  RefObject,
} from "react";

export const INSPECTOR_MIN_WIDTH = 320;
export const INSPECTOR_MAX_WIDTH = 540;
const DEFAULT_INSPECTOR_WIDTH = 414;

interface UseInspectorPanelOptions {
  /** The workspace body that goes inert behind the narrow sheet. */
  readonly backgroundRef: RefObject<HTMLElement | null>;
}

export function useInspectorPanel({ backgroundRef }: UseInspectorPanelOptions) {
  const [open, setOpen] = useState(false);
  const [width, setWidth] = useState(DEFAULT_INSPECTOR_WIDTH);
  const [narrow, setNarrow] = useState(false);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(null);
  const stopResizeRef = useRef<(() => void) | null>(null);

  const reveal = useCallback(() => setOpen(true), []);
  const close = useCallback(() => setOpen(false), []);
  const toggle = useCallback(() => setOpen((current) => !current), []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(max-width: 1024px)");
    const sync = (): void => setNarrow(media.matches);
    sync();
    media.addEventListener("change", sync);
    return () => media.removeEventListener("change", sync);
  }, []);

  useEffect(() => {
    const background = backgroundRef.current;
    if (!background) {
      return;
    }
    if (narrow && open) {
      // Record the real opener (row, Board card, toolbar button) once, before
      // the background goes inert and focus moves into the sheet.
      if (returnFocusRef.current === null) {
        const opener = document.activeElement;
        if (
          opener instanceof HTMLElement &&
          opener !== document.body &&
          !panelRef.current?.contains(opener)
        ) {
          returnFocusRef.current = opener;
        }
      }
      background.setAttribute("inert", "");
      panelRef.current?.focus();
      return () => background.removeAttribute("inert");
    }
    if (open) {
      // Still open, just wide: keep the opener without stealing focus back.
      return;
    }
    // Closed: the sheet and its inert background are already unmounted, so the
    // real opener can take focus back.
    const target = returnFocusRef.current;
    returnFocusRef.current = null;
    if (target?.isConnected) {
      target.focus();
    }
  }, [backgroundRef, narrow, open]);

  const endResize = useCallback(() => {
    stopResizeRef.current?.();
    stopResizeRef.current = null;
  }, []);

  // A drag never outlives the panel it was resizing.
  useEffect(() => {
    if (!open) {
      endResize();
    }
  }, [endResize, open]);

  useEffect(() => () => stopResizeRef.current?.(), []);

  const beginResize = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      event.preventDefault();
      endResize();
      const startX = event.clientX;
      const startWidth = width;
      const onMove = (moveEvent: PointerEvent): void => {
        const next = startWidth + (startX - moveEvent.clientX);
        setWidth(Math.min(INSPECTOR_MAX_WIDTH, Math.max(INSPECTOR_MIN_WIDTH, next)));
      };
      const stop = (): void => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", stop);
        window.removeEventListener("pointercancel", stop);
        stopResizeRef.current = null;
      };
      stopResizeRef.current = stop;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", stop);
      window.addEventListener("pointercancel", stop);
    },
    [endResize, width],
  );

  const resizeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        setWidth((current) => Math.min(INSPECTOR_MAX_WIDTH, current + 16));
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        setWidth((current) => Math.max(INSPECTOR_MIN_WIDTH, current - 16));
      }
    },
    [],
  );

  return {
    open,
    width,
    panelRef,
    reveal,
    close,
    toggle,
    beginResize,
    resizeKeyDown,
  };
}
