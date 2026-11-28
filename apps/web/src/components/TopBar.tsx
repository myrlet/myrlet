/**
 * Work Atlas top app bar: brand, live connection, the centered Board | List
 * workspace switch, and the global session actions — one 52px row, nothing else.
 *
 * Myrlet exposes one read-only workspace. Workers & Pairing slides over it as a
 * drawer, and appearance, shortcuts, and disconnect live in a compact settings
 * popover. Every number is a plain Hub fact (online Workers, SSE cursor) —
 * nothing infers a verdict, health score, or progress. On a narrow screen the
 * brand wordmark and the connection wording drop away (each control keeps an
 * explicit accessible name), so the mode switch never collides with status.
 */

import { useEffect, useRef, useState } from "react";
import type { Ref } from "react";
import {
  Keyboard,
  ListTree,
  Monitor,
  Moon,
  Network,
  Power,
  Settings,
  Sun,
} from "lucide-react";

import { THEMES } from "../state/useAppearance";
import type { Theme } from "../state/useAppearance";
import type { ConnectionStatus } from "../state/useHub";
import { BrandMark } from "./BrandMark";

/** The two presentations of the single workspace. `list` is the home. */
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
  /** The real Workers pill, so the drawer can restore focus to it on close. */
  readonly workersTriggerRef?: Ref<HTMLButtonElement>;
  readonly onRefresh: () => void;
  readonly theme: Theme;
  readonly onChangeTheme: (theme: Theme) => void;
  readonly onDisconnect: () => void;
  readonly onOpenShortcuts: () => void;
}

const CONNECTION_LABELS: Readonly<Record<ConnectionStatus, string>> = {
  connected: "Hub live",
  connecting: "Connecting…",
  reconnecting: "Reconnecting…",
  disconnected: "Disconnected",
};

const THEME_ICONS: Readonly<Record<Theme, typeof Monitor>> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

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
    workersTriggerRef,
    onRefresh,
    theme,
    onChangeTheme,
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
          aria-label={`${CONNECTION_LABELS[connection]} — ${hostLabel}. Refresh the Snapshot.`}
          onClick={onRefresh}
        >
          <span className="connection-dot" data-state={connectionDot} aria-hidden="true" />
          <span className="connection-text">{CONNECTION_LABELS[connection]}</span>
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
          ref={workersTriggerRef}
          type="button"
          className="app-bar-workers-btn"
          data-open={workersOpen}
          aria-expanded={workersOpen}
          aria-label={`Workers ${onlineWorkerCount}/${workerCount}`}
          title={`Workers — ${onlineWorkerCount} of ${workerCount} online. Open Workers & Pairing.`}
          onClick={onToggleWorkers}
        >
          <span className="connection-dot" data-state={workersDot} aria-hidden="true" />
          <span className="app-bar-workers-label">Workers</span>
          <span className="app-bar-workers-count tabular">
            {onlineWorkerCount}/{workerCount}
          </span>
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
              <div className="settings-section-label">APPEARANCE</div>
              <div className="theme-segment" role="group" aria-label="Appearance">
                {THEMES.map((option) => {
                  const Icon = THEME_ICONS[option.id];
                  const active = theme === option.id;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      className="theme-segment-btn"
                      role="menuitemradio"
                      aria-checked={active}
                      data-active={active}
                      onClick={() => onChangeTheme(option.id)}
                    >
                      <Icon size={13} aria-hidden="true" />
                      <span>{option.label}</span>
                    </button>
                  );
                })}
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
