/**
 * Work Atlas top app bar: brand, origin, live connection, workspace navigation,
 * and the global session actions.
 *
 * It replaces the former left rail entirely. Every number here is a plain Hub
 * fact (Tasks in the Snapshot, online Workers, SSE cursor) — nothing infers a
 * verdict, health score, or progress percentage.
 */

import { Activity, LayoutGrid, Plus, Power, Server, Upload } from "lucide-react";

import { THEMES } from "../state/useHub";
import type { ConnectionStatus, Theme } from "../state/useHub";
import { BrandMark } from "./BrandMark";

/** Canonical workspace views. `board` is the default home. */
export type WorkspaceView = "board" | "list" | "workers" | "events";

export interface TopBarProps {
  readonly activeView: WorkspaceView;
  readonly onSelectView: (view: WorkspaceView) => void;
  readonly baseUrl: string;
  readonly connection: ConnectionStatus;
  readonly taskCount: number;
  readonly onlineWorkerCount: number;
  readonly workerCount: number;
  readonly eventCursor: number;
  readonly theme: Theme;
  readonly onChangeTheme: (theme: Theme) => void;
  readonly onImport: () => void;
  readonly onNewTask: () => void;
  readonly onDisconnect: () => void;
}

const CONNECTION_LABELS: Readonly<Record<ConnectionStatus, string>> = {
  connected: "Hub live",
  connecting: "Connecting…",
  reconnecting: "Reconnecting…",
  disconnected: "Disconnected",
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
    activeView,
    onSelectView,
    baseUrl,
    connection,
    taskCount,
    onlineWorkerCount,
    workerCount,
    eventCursor,
    theme,
    onChangeTheme,
    onImport,
    onNewTask,
    onDisconnect,
  } = props;

  const workspaceActive = activeView === "board" || activeView === "list";

  return (
    <header className="app-bar">
      <div className="app-bar-left">
        <span className="app-bar-brand">
          <BrandMark size={22} />
          <span className="brand-name">Myrlet</span>
        </span>
        <span className="app-bar-origin truncate" title={originLabel(baseUrl)}>
          {originLabel(baseUrl)}
        </span>
        <span className="connection-pill" data-state={connection} title={CONNECTION_LABELS[connection]}>
          <span className="connection-dot" aria-hidden="true" />
          {CONNECTION_LABELS[connection]}
        </span>
      </div>

      <nav className="app-bar-nav" aria-label="Workspace views">
        <button
          type="button"
          className="app-bar-tab"
          data-active={workspaceActive}
          aria-current={workspaceActive ? "page" : undefined}
          title={`Workspace — ${taskCount} Tasks`}
          onClick={() => {
            if (!workspaceActive) {
              onSelectView("board");
            }
          }}
        >
          <LayoutGrid size={13} aria-hidden="true" />
          <span>Workspace</span>
          <span className="app-bar-tab-badge tabular" title={`${taskCount} Tasks on this Hub`}>
            {taskCount}
          </span>
        </button>

        <button
          type="button"
          className="app-bar-tab"
          data-active={activeView === "workers"}
          aria-current={activeView === "workers" ? "page" : undefined}
          title={`Workers — ${onlineWorkerCount} of ${workerCount} online`}
          onClick={() => onSelectView("workers")}
        >
          <Server size={13} aria-hidden="true" />
          <span>Workers</span>
          <span
            className="app-bar-tab-badge tabular"
            title={`${onlineWorkerCount} of ${workerCount} Workers online`}
          >
            {onlineWorkerCount}/{workerCount}
          </span>
        </button>

        <button
          type="button"
          className="app-bar-tab"
          data-active={activeView === "events"}
          aria-current={activeView === "events" ? "page" : undefined}
          title={`Events — cursor #${eventCursor}`}
          onClick={() => onSelectView("events")}
        >
          <Activity size={13} aria-hidden="true" />
          <span>Events</span>
          <span className="app-bar-tab-badge">
            {connection === "connected" ? (
              <>
                <span className="app-bar-live-dot" aria-hidden="true" />
                Live
              </>
            ) : eventCursor > 0 ? (
              `#${eventCursor}`
            ) : (
              "—"
            )}
          </span>
        </button>
      </nav>

      <div className="app-bar-actions">
        <select
          className="select app-bar-theme"
          data-theme-select
          aria-label="Theme"
          title="Theme"
          value={theme}
          onChange={(event) => {
            const next = event.target.value;
            const match = THEMES.find((option) => option.id === next);
            if (match) {
              onChangeTheme(match.id);
            }
          }}
        >
          {THEMES.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
        <button type="button" className="button button-sm" onClick={onImport} title="Import a Task bundle">
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
        <button
          type="button"
          className="icon-button"
          onClick={onDisconnect}
          aria-label="Disconnect from Hub"
          title="Disconnect"
        >
          <Power size={14} aria-hidden="true" />
        </button>
      </div>
    </header>
  );
}
