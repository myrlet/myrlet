import { vi } from "vitest";

interface ColorSchemeController {
  setDark: (dark: boolean) => void;
  setNarrow: (next: boolean) => void;
}

/** A controllable `matchMedia`; color-scheme changes dispatch to real listeners. */
export function stubColorScheme(dark: boolean, narrow = false): ColorSchemeController {
  let colorDark = dark;
  const listeners = new Set<(event: MediaQueryListEvent) => void>();
  const colorMedia = {
    get matches() {
      return colorDark;
    },
    media: "(prefers-color-scheme: dark)",
    onchange: null,
    addEventListener: (type: string, listener: EventListenerOrEventListenerObject | null) => {
      if (type === "change" && typeof listener === "function") {
        listeners.add(listener as (event: MediaQueryListEvent) => void);
      }
    },
    removeEventListener: (type: string, listener: EventListenerOrEventListenerObject | null) => {
      if (type === "change" && typeof listener === "function") {
        listeners.delete(listener as (event: MediaQueryListEvent) => void);
      }
    },
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => true,
  } as MediaQueryList;
  const viewport = new EventTarget();
  const narrowMedia = {
    get matches() { return narrow; },
    media: "(max-width: 1024px)",
    onchange: null,
    addEventListener: viewport.addEventListener.bind(viewport),
    removeEventListener: viewport.removeEventListener.bind(viewport),
    addListener: () => undefined,
    removeListener: () => undefined,
    dispatchEvent: () => true,
  } as MediaQueryList;
  vi.stubGlobal(
    "matchMedia",
    vi.fn((query: string) =>
      query.includes("max-width") ? narrowMedia : colorMedia,
    ),
  );
  return {
    setNarrow(next) {
      narrow = next;
      viewport.dispatchEvent(new Event("change"));
    },
    setDark(next) {
      colorDark = next;
      const event = {
        matches: next,
        media: colorMedia.media,
      } as MediaQueryListEvent;
      for (const listener of listeners) {
        listener(event);
      }
    },
  };
}
