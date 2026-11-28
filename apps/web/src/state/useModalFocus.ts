/**
 * Focus containment for the two modal surfaces (⌘K palette and the Workers
 * drawer).
 *
 * Opening remembers the element that had focus, moves focus to the panel (or a
 * nominated initial control), keeps Tab inside the panel, and on close returns
 * focus to the remembered opener when it is still mounted. When that opener came
 * from a surface that has since closed (the palette option that opened the
 * Workers drawer, for example), the owner's `fallbackRef` takes the focus
 * instead of dropping it on `body` or a detached node.
 */

import { useCallback, useEffect, useRef } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, RefObject } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

function focusableWithin(panel: HTMLElement): HTMLElement[] {
  return Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (node) => node.tabIndex >= 0 && !node.closest("[inert], [hidden], [aria-hidden=true]"),
  );
}

function restorable(element: Element | null, panel: HTMLElement | null): HTMLElement | null {
  if (!(element instanceof HTMLElement) || element === document.body) {
    return null;
  }
  if (!element.isConnected) {
    return null;
  }
  return panel?.contains(element) ? null : element;
}

export interface UseModalFocusOptions {
  readonly open: boolean;
  /** Focused on open instead of the panel itself (e.g. a search field). */
  readonly initialFocusRef?: RefObject<HTMLElement | null>;
  /** Focused on close when the recorded opener is gone. */
  readonly fallbackRef?: RefObject<HTMLElement | null>;
}

export function useModalFocus<T extends HTMLElement = HTMLElement>({
  open,
  initialFocusRef,
  fallbackRef,
}: UseModalFocusOptions) {
  const panelRef = useRef<T | null>(null);
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    // The control focused at open time is the real opener (App Bar pill,
    // Inspector Manage). When that opener is already gone by close time — the
    // palette option that opened the drawer unmounted with the palette — the
    // owner's fallback takes over so focus never lands on `body` or a detached
    // node.
    openerRef.current = restorable(document.activeElement, panelRef.current);
    const initial = initialFocusRef?.current ?? panelRef.current;
    initial?.focus();
    return () => {
      const recorded = openerRef.current;
      openerRef.current = null;
      const target =
        (recorded?.isConnected ? recorded : null) ??
        (fallbackRef?.current?.isConnected ? fallbackRef.current : null);
      target?.focus();
    };
  }, [fallbackRef, initialFocusRef, open]);

  /** Wrap Tab at both ends so focus never leaves the modal panel. */
  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab" || event.defaultPrevented) {
      return;
    }
    const panel = panelRef.current;
    if (!panel) {
      return;
    }
    const focusable = focusableWithin(panel);
    if (focusable.length === 0) {
      event.preventDefault();
      panel.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    const active = document.activeElement;
    const outside = !(active instanceof Node) || !panel.contains(active);
    if (event.shiftKey) {
      if (active === first || active === panel || outside) {
        event.preventDefault();
        last.focus();
      }
      return;
    }
    if (active === last || active === panel || outside) {
      event.preventDefault();
      first.focus();
    }
  }, []);

  return { panelRef, onKeyDown };
}
