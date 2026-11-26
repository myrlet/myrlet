/**
 * Sticky 48px operational header: brand, live SSE status, filters, and the
 * explicit workspace actions (Workers, Import, New Task, Disconnect).
 *
 * The connection pill reports the real stream state and the last event cursor;
 * it never claims "live" while the stream is reconnecting.
 */

import { LogOut, Moon, Plus, RefreshCw, Search, Sun, Upload, Users } from "lucide-react";

import type { TaskStatus, WorkerRecord } from "../api/types";
import { formatCount, TASK_STATUS_META, TASK_STATUSES } from "../format";
import type { TaskFilter } from "../format";
import type { ConnectionStatus, Theme } from "../state/useHub";

export interface HeaderBarProps {
  baseUrl: string;
  connection: ConnectionStatus;
  eventCursor: number;
  workerCount: number;
  onlineWorkerCount: number;
  filter: TaskFilter;
  onFilterChange: (next: TaskFilter) => void;
  workers: readonly WorkerRecord[];
  theme: Theme;
  onToggleTheme: () => void;
  onRefresh: () => void;
  onOpenWorkers: () => void;
  onOpenImport: () => void;
  onOpenNewTask: () => void;
  onDisconnect: () => void;
}

const CONNECTION_LABELS: Readonly<Record<ConnectionStatus, string>> = {
  connected: "Connected",
  connecting: "Connecting",
  reconnecting: "Reconnecting",
  disconnected: "Disconnected",
};

function parseStatusFilter(raw: string): TaskStatus | "all" {
  return TASK_STATUSES.includes(raw as TaskStatus) ? (raw as TaskStatus) : "all";
}

function BrandMark() {
  return (
    <span className="brand-mark" aria-hidden="true">
      <svg
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <rect x="2.5" y="2.5" width="15" height="15" rx="4" />
        <path d="M6.6 13V7.1l3.4 3.1 3.4-3.1V13" />
      </svg>
    </span>
  );
}

export function HeaderBar(props: HeaderBarProps) {
  const {
    baseUrl,
    connection,
    eventCursor,
    workerCount,
    onlineWorkerCount,
    filter,
    onFilterChange,
    workers,
    theme,
    onToggleTheme,
    onRefresh,
    onOpenWorkers,
    onOpenImport,
    onOpenNewTask,
    onDisconnect,
  } = props;

  const nextTheme: Theme = theme === "porcelain" ? "graphite" : "porcelain";
  const nextThemeLabel = nextTheme === "graphite" ? "Graphite" : "Porcelain";
  const ThemeIcon = nextTheme === "graphite" ? Moon : Sun;

  const origin = baseUrl.trim() === "" ? "same-origin" : baseUrl.trim();
  const connectionLabel = CONNECTION_LABELS[connection];

  const selectedWorkerId =
    typeof filter.workerId === "string" && filter.workerId !== "" ? filter.workerId : "all";
  // Keep a vanished Worker selectable instead of silently resetting the filter.
  const missingWorker =
    selectedWorkerId !== "all" && !workers.some((worker) => worker.workerId === selectedWorkerId);

  const workersLabel = `Workers (${formatCount(onlineWorkerCount)}/${formatCount(workerCount)})`;

  return (
    <header className="app-header">
      <div className="header-brand">
        <BrandMark />
        <span className="brand-name hide-below-720">Myrlet</span>
      </div>

      <span className="header-divider" aria-hidden="true" />

      <div className="header-section hide-below-960">
        <span className="mono cell-muted" title={`Hub origin: ${origin}`}>
          {origin}
        </span>
      </div>

      <span className="connection-pill" data-state={connection} title={`Hub ${origin} — ${connectionLabel}`}>
        <span className="connection-dot" aria-hidden="true" />
        <span role="status" aria-live="polite">
          {connectionLabel}
        </span>
        {eventCursor > 0 ? (
          <span className="connection-cursor" title="Last received event sequence">
            · #{eventCursor}
          </span>
        ) : null}
      </span>

      <span className="header-divider" aria-hidden="true" />

      <div className="header-section">
        <div className="search">
          <span className="search-icon" aria-hidden="true">
            <Search size={13} />
          </span>
          <input
            className="input"
            type="search"
            aria-label="Search tasks"
            placeholder="Search tasks"
            value={filter.query ?? ""}
            onChange={(event) => onFilterChange({ ...filter, query: event.target.value })}
            spellCheck={false}
            autoComplete="off"
          />
        </div>

        <select
          className="select filter-select"
          aria-label="Filter by status"
          value={filter.status ?? "all"}
          onChange={(event) =>
            onFilterChange({ ...filter, status: parseStatusFilter(event.target.value) })
          }
        >
          <option value="all">All statuses</option>
          {TASK_STATUSES.map((status) => (
            <option key={status} value={status}>
              {TASK_STATUS_META[status].label}
            </option>
          ))}
        </select>

        <select
          className="select filter-select"
          aria-label="Filter by worker"
          value={selectedWorkerId}
          onChange={(event) => onFilterChange({ ...filter, workerId: event.target.value })}
        >
          <option value="all">All workers</option>
          {missingWorker ? <option value={selectedWorkerId}>{selectedWorkerId}</option> : null}
          {workers.map((worker) => (
            <option key={worker.workerId} value={worker.workerId}>
              {`${worker.workerId}${worker.revoked ? " (revoked)" : worker.online ? "" : " (offline)"}`}
            </option>
          ))}
        </select>
      </div>

      <div className="header-spacer" />

      <div className="header-actions">
        <button
          type="button"
          className="icon-button"
          onClick={onRefresh}
          aria-label="Refresh snapshot"
          title="Refresh snapshot"
        >
          <RefreshCw size={15} aria-hidden="true" />
        </button>

        <button
          type="button"
          className="button button-sm"
          onClick={onToggleTheme}
          aria-label={`Switch to ${nextThemeLabel} theme`}
          title={`Switch to ${nextThemeLabel} theme`}
        >
          <ThemeIcon size={14} aria-hidden="true" />
          <span className="button-label">{nextThemeLabel}</span>
        </button>

        <span className="header-divider" aria-hidden="true" />

        <button
          type="button"
          className="button button-sm"
          onClick={onOpenWorkers}
          aria-label={`Workers: ${formatCount(onlineWorkerCount)} of ${formatCount(workerCount)} online`}
          title="Workers and pairing"
        >
          <Users size={14} aria-hidden="true" />
          <span className="button-label">{workersLabel}</span>
        </button>

        <button
          type="button"
          className="button button-sm"
          onClick={onOpenImport}
          aria-label="Import Task bundle"
          title="Import a Task bundle"
        >
          <Upload size={14} aria-hidden="true" />
          <span className="button-label hide-below-960">Import</span>
        </button>

        <button
          type="button"
          className="button button-sm button-primary"
          onClick={onOpenNewTask}
          aria-label="New Task"
          title="New Task"
        >
          <Plus size={14} aria-hidden="true" />
          <span className="button-label">New Task</span>
        </button>

        <button
          type="button"
          className="button button-sm button-ghost"
          onClick={onDisconnect}
          aria-label="Disconnect from Hub"
          title="Disconnect from Hub"
        >
          <LogOut size={14} aria-hidden="true" />
          <span className="button-label hide-below-960">Disconnect</span>
        </button>
      </div>
    </header>
  );
}
