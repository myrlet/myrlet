/**
 * Keyboard handling for the authenticated Work Atlas workspace.
 *
 * Mounted only while a Hub session exists, so these shortcuts are inert on the
 * Auth gate. Escape priority is explicit — palette, shortcuts sheet, Workers
 * drawer, status menu, status filter, then selection — and a nested surface
 * that already consumed the key wins through `defaultPrevented`. Editable
 * targets and modifier chords never trigger workspace shortcuts.
 */

import { useEffect } from "react";

import type { WorkspaceMode } from "../components/TopBar";

export interface WorkspaceShortcutOptions {
  readonly mode: WorkspaceMode;
  readonly paletteOpen: boolean;
  readonly shortcutsOpen: boolean;
  readonly workersOpen: boolean;
  readonly statusMenuOpen: boolean;
  readonly filterApplied: boolean;
  /** A selected Task or an open Inspector — both retract on Escape. */
  readonly selectionActive: boolean;
  /** A selected Task specifically; `Shift+F` only centers a real selection. */
  readonly hasSelectedTask: boolean;
  readonly togglePalette: () => void;
  readonly closePalette: () => void;
  readonly openShortcuts: () => void;
  readonly closeShortcuts: () => void;
  readonly closeWorkers: () => void;
  readonly closeStatusMenu: () => void;
  readonly clearFilter: () => void;
  readonly clearWorkspace: () => void;
  readonly toggleInspector: () => void;
  readonly resetZoom: () => void;
  readonly fitBoard: () => void;
  readonly focusSelected: () => void;
  readonly moveSelection: (direction: 1 | -1) => void;
}

export function useWorkspaceShortcuts(options: WorkspaceShortcutOptions): void {
  const {
    mode,
    paletteOpen,
    shortcutsOpen,
    workersOpen,
    statusMenuOpen,
    filterApplied,
    selectionActive,
    hasSelectedTask,
    togglePalette,
    closePalette,
    openShortcuts,
    closeShortcuts,
    closeWorkers,
    closeStatusMenu,
    clearFilter,
    clearWorkspace,
    toggleInspector,
    resetZoom,
    fitBoard,
    focusSelected,
    moveSelection,
  } = options;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if ((event.metaKey || event.ctrlKey) && (event.key === "k" || event.key === "K")) {
        event.preventDefault();
        togglePalette();
        return;
      }

      if (event.key === "Escape") {
        // A nested surface (e.g. the Settings popover) already consumed this
        // Escape; do not also close the drawer or clear the selection.
        if (event.defaultPrevented) {
          return;
        }
        if (paletteOpen) {
          event.preventDefault();
          closePalette();
          return;
        }
        if (shortcutsOpen) {
          event.preventDefault();
          closeShortcuts();
          return;
        }
        // The Workers drawer sits above the workspace, so it closes before the
        // Task selection is allowed to change.
        if (workersOpen) {
          event.preventDefault();
          closeWorkers();
          return;
        }
        if (statusMenuOpen) {
          event.preventDefault();
          closeStatusMenu();
          return;
        }
        // Filter is workspace state, below every overlay but above the
        // Inspector/selection. Escape peels it first and leaves the Inspector.
        if (filterApplied) {
          event.preventDefault();
          clearFilter();
          return;
        }
        if (selectionActive) {
          event.preventDefault();
          clearWorkspace();
        }
        return;
      }

      const target = event.target;
      const inField =
        target instanceof HTMLElement &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);
      if (inField) {
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      if (paletteOpen || shortcutsOpen || workersOpen || statusMenuOpen) {
        return;
      }

      switch (event.key) {
        case "?":
          event.preventDefault();
          openShortcuts();
          break;
        case "i":
          event.preventDefault();
          toggleInspector();
          break;
        case "0":
          if (mode === "board") {
            event.preventDefault();
            resetZoom();
          }
          break;
        case "f":
          if (mode === "board") {
            event.preventDefault();
            fitBoard();
          }
          break;
        case "F":
          if (mode === "board" && hasSelectedTask) {
            event.preventDefault();
            focusSelected();
          }
          break;
        case "j":
        case "ArrowDown":
          if (mode === "list") {
            event.preventDefault();
            moveSelection(1);
          }
          break;
        case "k":
        case "ArrowUp":
          if (mode === "list") {
            event.preventDefault();
            moveSelection(-1);
          }
          break;
        default:
          break;
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [
    clearFilter,
    clearWorkspace,
    closePalette,
    closeShortcuts,
    closeStatusMenu,
    closeWorkers,
    filterApplied,
    resetZoom,
    fitBoard,
    focusSelected,
    hasSelectedTask,
    mode,
    moveSelection,
    openShortcuts,
    paletteOpen,
    selectionActive,
    shortcutsOpen,
    statusMenuOpen,
    toggleInspector,
    togglePalette,
    workersOpen,
  ]);
}
