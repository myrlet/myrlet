/**
 * Appearance preference for the Myrlet Web UI.
 *
 * Owns the only persisted UI preference (`localStorage["myrlet.theme"]`) and
 * the resolved `<html data-theme>`. Board layout offsets stay with
 * `TaskGraphView` (`localStorage["myrlet.atlas.positions"]`). Mount exactly
 * once at the app root.
 */

import { useCallback, useEffect, useState } from "react";

export type Theme = "system" | "light" | "dark";

type ResolvedTheme = "light" | "dark";

/** Ordered appearance options: System follows the browser, or lock Light / Dark. */
export const THEMES: readonly { readonly id: Theme; readonly label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

const THEME_STORAGE_KEY = "myrlet.theme";

const THEME_IDS: readonly Theme[] = THEMES.map((theme) => theme.id);

function isTheme(value: unknown): value is Theme {
  return typeof value === "string" && (THEME_IDS as readonly string[]).includes(value);
}

/** Read the persisted appearance, defaulting to `system` when nothing is stored. */
function readStoredTheme(): Theme {
  if (typeof window === "undefined") {
    return "system";
  }
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isTheme(stored)) {
      return stored;
    }
  } catch {
    // Storage can be unavailable (privacy mode); follow the browser instead.
  }
  return "system";
}

/** The browser's current `prefers-color-scheme`, safe under jsdom/SSR. */
function readSystemTheme(): ResolvedTheme {
  if (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  ) {
    return "dark";
  }
  return "light";
}

export function useAppearance(): {
  readonly theme: Theme;
  setTheme: (theme: Theme) => void;
} {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);
  const [systemTheme, setSystemTheme] = useState<ResolvedTheme>(readSystemTheme);

  const themeMode: ResolvedTheme = theme === "system" ? systemTheme : theme;

  /* The resolved appearance drives <html data-theme>, so CSS and tests always
     see an explicit light/dark. Only a real choice is persisted. */
  useEffect(() => {
    document.documentElement.dataset.theme = themeMode;
  }, [themeMode]);

  /* Follow OS/browser appearance live while the preference is System. */
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    if (typeof media.addEventListener !== "function") {
      return;
    }
    const onChange = (event: MediaQueryListEvent): void => {
      setSystemTheme(event.matches ? "dark" : "light");
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const setTheme = useCallback((next: Theme) => {
    setThemeState(next);
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Persisting the theme is best-effort.
    }
  }, []);

  return { theme, setTheme };
}
