/**
 * Workspace Inspector: the right-hand panel shown when no Task is selected.
 *
 * It is a factual overview only — queued/dispatched Tasks that can be opened,
 * Registered/Online/Offline Worker counts plus active Delivery counts, and a
 * workspace summary of Task trees, Tasks, and Categories. No render claims a
 * success verdict, health score, or progress percentage.
 */

import { ArrowUpRight, Server, X } from "lucide-react";

import type { SnapshotTaskSummary, WorkerRecord } from "../api/types";
import { formatCount, formatRelativeTime } from "../format";
import type { TaskStatus } from "../api/types";
import { TaskStatusBadge } from "./StatusBadge";

export interface WorkspaceInspectorProps {
  tasks: readonly SnapshotTaskSummary[];
  workers: readonly WorkerRecord[];
  onSelectTask: (taskId: string) => void;
  onOpenWorkers: () => void;
  onClose: () => void;
}

const ATTENTION_STATUSES: readonly TaskStatus[] = ["dispatched", "queued"];

function shortId(id: string): string {
  return id.length <= 12 ? id : id.slice(0, 12);
}

function workerHostLabel(worker: WorkerRecord): string {
  if (worker.inventory === null) {
    return "Inventory not reported";
  }
  return `${worker.inventory.host.os} · ${worker.inventory.host.arch}`;
}

function isOnline(worker: WorkerRecord): boolean {
  return worker.online && !worker.revoked;
}

interface StatCellProps {
  label: string;
  value: string;
  hint?: string;
}

function StatCell({ label, value, hint }: StatCellProps) {
  return (
    <div className="stat-cell" title={hint ?? `${label}: ${value}`}>
      <span className="stat-cell-value tabular">{value}</span>
      <span className="stat-cell-label">{label}</span>
    </div>
  );
}

export function WorkspaceInspector(props: WorkspaceInspectorProps) {
  const { tasks, workers, onSelectTask, onOpenWorkers, onClose } = props;

  const attentionTasks = tasks.filter((task) => ATTENTION_STATUSES.includes(task.status));

  const registeredCount = workers.length;
  const onlineCount = workers.filter(isOnline).length;
  const offlineCount = registeredCount - onlineCount;
  const activeDeliveryCount = workers.reduce(
    (total, worker) => total + worker.activeDeliveries,
    0,
  );

  const treeCount = new Set(tasks.map((task) => task.rootTaskId)).size;
  const categoryCount = new Set(
    tasks.map((task) => task.category).filter((category) => category !== ""),
  ).size;

  return (
    <aside
      className="inspector-pane"
      aria-label="Workspace inspector"
      style={{ display: "flex", flexDirection: "column", minHeight: 0, height: "100%" }}
    >
      <header className="inspector-header">
        <h2 className="inspector-title">Workspace Inspector</h2>
        <span className="header-spacer" />
        <button
          type="button"
          className="icon-button"
          onClick={onClose}
          aria-label="Close workspace inspector"
          title="Close workspace inspector"
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <div className="inspector-body" style={{ flex: "1 1 auto", minHeight: 0, overflow: "auto" }}>
        <section className="inspector-section" aria-label="Queued and dispatched Tasks">
          <h3 className="inspector-section-title">Queued &amp; Dispatched</h3>
          {attentionTasks.length === 0 ? (
            <p className="detail-empty">No queued or dispatched tasks.</p>
          ) : (
            <ul className="attention-list">
              {attentionTasks.map((task) => (
                <li key={task.id}>
                  <button
                    type="button"
                    className="attention-row"
                    onClick={() => onSelectTask(task.id)}
                    title={`Open Task ${task.id}`}
                    aria-label={`Open Task ${task.title || task.id}`}
                  >
                    <TaskStatusBadge status={task.status} compact />
                    <span className="attention-body">
                      <span className="attention-title truncate">{task.title || task.id}</span>
                      <span className="attention-meta">
                        <span className="mono" title={task.id}>
                          {shortId(task.id)}
                        </span>
                        <span className="muted">
                          {" · "}
                          {formatRelativeTime(task.updatedAtMs)}
                        </span>
                      </span>
                    </span>
                    <ArrowUpRight size={13} aria-hidden="true" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="inspector-section" aria-label="Workers">
          <div className="inspector-section-head">
            <h3 className="inspector-section-title">Workers</h3>
            <button
              type="button"
              className="button button-sm button-ghost"
              onClick={onOpenWorkers}
              aria-label="Open Workers"
              title="Open Workers"
            >
              <Server size={12} aria-hidden="true" />
              <span>Open</span>
            </button>
          </div>

          <div
            className="stat-grid meta-grid-2x2"
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(4, minmax(0, 1fr))",
              gap: "1px",
            }}
          >
            <StatCell
              label="Registered"
              value={formatCount(registeredCount)}
              hint={`${registeredCount} Workers registered with this Hub`}
            />
            <StatCell
              label="Online"
              value={formatCount(onlineCount)}
              hint={`${onlineCount} Workers connected and not revoked`}
            />
            <StatCell
              label="Offline"
              value={formatCount(offlineCount)}
              hint={`${offlineCount} Workers not currently connected`}
            />
            <StatCell
              label="Active"
              value={formatCount(activeDeliveryCount)}
              hint={`${activeDeliveryCount} active deliveries reported by Workers`}
            />
          </div>

          {workers.length === 0 ? (
            <p className="detail-empty">No Workers registered yet.</p>
          ) : (
            <ul className="inspector-worker-list">
              {workers.map((worker) => {
                const online = isOnline(worker);
                return (
                  <li key={worker.workerId}>
                    <button
                      type="button"
                      className="inspector-worker"
                      onClick={onOpenWorkers}
                      title={`Open Workers — ${worker.workerId}`}
                      aria-label={`Worker ${worker.workerId}, ${
                        worker.revoked ? "revoked" : online ? "online" : "offline"
                      }, ${worker.activeDeliveries} active deliveries`}
                    >
                      <span
                        className="connection-dot"
                        data-state={online ? "online" : "offline"}
                        aria-hidden="true"
                      />
                      <span className="inspector-worker-body">
                        <span className="inspector-worker-id mono truncate">
                          {worker.workerId}
                        </span>
                        <span className="inspector-worker-meta truncate">
                          {workerHostLabel(worker)}
                        </span>
                      </span>
                      <span
                        className="inspector-worker-count tabular"
                        title="Active deliveries"
                      >
                        {formatCount(worker.activeDeliveries)}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
      </div>

      <footer className="inspector-summary" aria-label="Workspace summary">
        <div
          className="stat-grid"
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
            gap: "1px",
          }}
        >
          <StatCell
            label="Task trees"
            value={formatCount(treeCount)}
            hint={`${treeCount} Task trees in the loaded snapshot`}
          />
          <StatCell
            label="Tasks"
            value={formatCount(tasks.length)}
            hint={`${tasks.length} Tasks in the loaded snapshot`}
          />
          <StatCell
            label="Categories"
            value={formatCount(categoryCount)}
            hint={`${categoryCount} distinct non-empty Categories`}
          />
        </div>
      </footer>
    </aside>
  );
}