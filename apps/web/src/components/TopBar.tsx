/**
 * Work Atlas top app bar: brand, live connection, the centered Board | List
 * workspace switch, and the global session actions.
 *
 * There is no page navigation here. Myrlet exposes one workspace; Workers &
 * Pairing slides over it as a drawer, and theme, shortcuts, and disconnect live
 * in a compact settings popover. Every number is a plain Hub fact (online
 * Workers, SSE cursor) — nothing infers a verdict, health score, or progress.
 */

import { useEffect, useRef, useState } from "react";
import {
  Keyboard,
  ListTree,
  Network,
  Plus,
  Power,
  Settings,
  Upload,
} from "lucide-react";

import { THEMES } from "../state/useHub";
import type { ConnectionStatus, Theme, ThemeDescriptor } from "../state/useHub";
import { BrandMark } from "./BrandMark";

/** The two presentations of the single workspace. `board` is the home. */
export type WorkspaceMode = "board" | "list";

export interface TopBarProps {
  readonly mode: WorkspaceMode;
  readonly onSelectMode: (mode: WorkspaceMode) => void;
  readonly baseUrl: string;
  readonly connection: ConnectionStatus;
  readonly eventCursor: number;
  readonly onlineWorkerCount: number;
  readonly workerCount: number;
  readonly workersOpen: boolean;
  readonly onToggleWorkers: () => void;
  readonly onRefresh: () => void;
  readonly theme: Theme;
  readonly onChangeTheme: (theme: Theme) => void;
  readonly onImport: () => void;
  readonly onNewTask: () => void;
  readonly onDisconnect: () => void;
  readonly onOpenShortcuts: () => void;
}

const CONNECTION_LABELS: Readonly<Record<ConnectionStatus, string>> = {
  connected: "Hub live",
  connecting: "Connecting…",
  reconnecting: "Reconnecting…",
  disconnected: "Disconnected",
};

/** Menu order favours the two light themes first, then the two dark ones. */
const THEME_MENU_ORDER: readonly Theme[] = ["porcelain", "paper", "obsidian", "graphite"];

const THEME_OPTIONS: readonly ThemeDescriptor[] = THEME_MENU_ORDER.map((id) =>
  THEMES.find((option) => option.id === id),
).filter((option): option is ThemeDescriptor => option !== undefined);

/** `Same origin` for the bundled UI, otherwise the Hub host. */
function originLabel(baseUrl: string): string {
  const trimmed = baseUrl.trim();
  if (trimmed === "") {
    return "Same origin";
  }
  try {
    return new URL(trimmed).host;
  } catch {
    return trimmed;
  }
}

export function TopBar(props: TopBarProps) {
  const {
    mode,
    onSelectMode,
    baseUrl,
    connection,
    eventCursor,
    onlineWorkerCount,
    workerCount,
    workersOpen,
    onToggleWorkers,
    onRefresh,
    theme,
    onChangeTheme,
    onImport,
    onNewTask,
    onDisconnect,
    onOpenShortcuts,
  } = props;

  const [menuOpen, setMenuOpen] = useState(false);
  const settingsRef = useRef<HTMLDivElement | null>(null);
  const hostLabel = originLabel(baseUrl);

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const onPointerDown = (event: MouseEvent): void => {
      const target = event.target;
      if (target instanceof Node && settingsRef.current?.contains(target)) {
        return;
      }
      setMenuOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") {
        return;
      }
      // Capture at the window so keydown handlers below do not also clear the
      // Task selection while only the popover is open.
      event.preventDefault();
      event.stopPropagation();
      setMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [menuOpen]);

  const connectionDot = connection === "connected" ? "online" : "offline";
  const workersDot = onlineWorkerCount > 0 ? "online" : "offline";

  return (
    <header className="app-bar">
      <div className="app-bar-left">
        <span className="app-bar-brand">
          <BrandMark size={22} />
          <span className="brand-name">Myrlet</span>
        </span>
        <button
          type="button"
          className="connection-pill"
          data-state={connection}
          title={`${hostLabel} · SSE cursor #${eventCursor} — click to refresh`}
          onClick={onRefresh}
        >
          <span className="connection-dot" data-state={connectionDot} aria-hidden="true" />
          {CONNECTION_LABELS[connection]}
        </button>
      </div>

      <div className="app-bar-center">
        <div className="mode-switch" role="group" aria-label="Workspace mode">
          <button
            type="button"
            className="mode-switch-btn"
            data-active={mode === "board"}
            aria-pressed={mode === "board"}
            onClick={() => onSelectMode("board")}
          >
            <Network size={13} aria-hidden="true" />
            <span>Board</span>
          </button>
          <button
            type="button"
            className="mode-switch-btn"
            data-active={mode === "list"}
            aria-pressed={mode === "list"}
            onClick={() => onSelectMode("list")}
          >
            <ListTree size={13} aria-hidden="true" />
            <span>List</span>
          </button>
        </div>
      </div>

      <div className="app-bar-actions">
        <button
          type="button"
          className="app-bar-workers-btn"
          data-open={workersOpen}
          aria-expanded={workersOpen}
          title={`Workers — ${onlineWorkerCount} of ${workerCount} online. Open Workers & Pairing.`}
          onClick={onToggleWorkers}
        >
          <span className="connection-dot" data-state={workersDot} aria-hidden="true" />
          <span className="app-bar-workers-label">Workers</span>
          <span className="app-bar-workers-count tabular">
            {onlineWorkerCount}/{workerCount}
          </span>
        </button>

        <button
          type="button"
          className="button button-sm"
          onClick={onImport}
          title="Import a Task bundle"
        >
          <Upload size={12} aria-hidden="true" />
          <span>Import</span>
        </button>
        <button
          type="button"
          className="button button-sm button-primary"
          onClick={onNewTask}
          title="New Task (n)"
        >
          <Plus size={12} aria-hidden="true" />
          <span>New Task</span>
        </button>

        <div className="settings-menu-wrap" ref={settingsRef}>
          <button
            type="button"
            className="icon-button"
            aria-label="Settings and theme"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            title="Settings and theme"
            onClick={() => setMenuOpen((current) => !current)}
          >
            <Settings size={15} aria-hidden="true" />
          </button>

          {menuOpen ? (
            <div className="settings-popover" role="menu" aria-label="Settings">
              <div className="settings-section-label">THEME</div>
              <div className="settings-theme-grid" role="group" aria-label="Theme">
                {THEME_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    className="settings-theme-option"
                    role="menuitemradio"
                    aria-checked={theme === option.id}
                    data-active={theme === option.id}
                    onClick={() => onChangeTheme(option.id)}
                  >
                    <span
                      className="settings-theme-swatch"
                      data-swatch={option.id}
                      aria-hidden="true"
                    />
                    <span>{option.label}</span>
                  </button>
                ))}
              </div>

              <div className="settings-divider" role="separator" />

              <button
                type="button"
                className="settings-menu-item"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onOpenShortcuts();
                }}
              >
                <Keyboard size={14} aria-hidden="true" />
                <span className="settings-menu-item-label">Keyboard shortcuts</span>
                <kbd className="kbd mono">?</kbd>
              </button>
              <button
                type="button"
                className="settings-menu-item settings-menu-item-danger"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  onDisconnect();
                }}
              >
                <Power size={14} aria-hidden="true" />
                <span className="settings-menu-item-label">Disconnect</span>
              </button>
            </div>
          ) : null}
        </div>
      </div>
    </header>
  );
}
