/**
 * Bottom status bar for the Work Atlas.
 *
 * Desktop: five honest status counters plus Snapshot totals, Board zoom, and
 * the shortcut hint. Phones: the counters collapse into one accessible filter
 * menu. The menu closes on an outside press; selecting a state or pressing
 * Escape returns focus to its trigger. Escape priority stays with the workspace
 * keyboard handler, which calls back through `menuOpen`.
 */

import { useEffect, useRef } from "react";
import type { RefObject } from "react";
import { Check, ChevronDown, Filter } from "lucide-react";

import type { TaskStatus } from "../api/types";
import { TASK_STATUSES, TASK_STATUS_META } from "../format";

export interface StatusBarProps {
  readonly compact: boolean;
  readonly statusFilter: TaskStatus | null;
  readonly statusCounts: Readonly<Record<TaskStatus, number>>;
  readonly totalTasks: number;
  readonly relationCount: number;
  readonly zoomPercent: number | null;
  readonly menuOpen: boolean;
  readonly triggerRef: RefObject<HTMLButtonElement | null>;
  readonly onMenuOpenChange: (open: boolean) => void;
  readonly onSelectStatus: (status: TaskStatus | null) => void;
}

export function StatusBar(props: StatusBarProps) {
  const {
    compact,
    statusFilter,
    statusCounts,
    totalTasks,
    relationCount,
    zoomPercent,
    menuOpen,
    triggerRef,
    onMenuOpenChange,
    onSelectStatus,
  } = props;

  const menuRef = useRef<HTMLDivElement | null>(null);
  const activeStatusLabel =
    statusFilter === null ? null : TASK_STATUS_META[statusFilter].label;

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const onPointerDown = (event: MouseEvent): void => {
      const target = event.target;
      if (target instanceof Node && menuRef.current?.contains(target)) {
        return;
      }
      onMenuOpenChange(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [menuOpen, onMenuOpenChange]);

  /** Close after a real choice and put focus back on the trigger. */
  const chooseStatus = (status: TaskStatus | null): void => {
    onSelectStatus(status);
    onMenuOpenChange(false);
    triggerRef.current?.focus();
  };

  return (
    <footer className="status-bar">
      <div className="status-bar-left">
        {compact ? (
          <div className="status-filter" ref={menuRef}>
            <button
              ref={triggerRef}
              type="button"
              className="status-filter-trigger"
              data-active={statusFilter !== null}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              aria-label={
                activeStatusLabel === null
                  ? "Filter by status — all statuses"
                  : `Filter by status — ${activeStatusLabel}`
              }
              onClick={() => onMenuOpenChange(!menuOpen)}
            >
              <Filter size={12} aria-hidden="true" />
              <span className="status-filter-label">
                {activeStatusLabel ?? "All statuses"}
              </span>
              <span className="status-filter-count tabular">
                {statusFilter === null ? totalTasks : statusCounts[statusFilter]}
              </span>
              <ChevronDown size={12} aria-hidden="true" />
            </button>
            {menuOpen ? (
              <div className="status-filter-menu" role="menu" aria-label="Filter by status">
                <button
                  type="button"
                  className="status-filter-option"
                  role="menuitemradio"
                  aria-checked={statusFilter === null}
                  data-active={statusFilter === null}
                  onClick={() => chooseStatus(null)}
                >
                  <span className="status-filter-check" aria-hidden="true">
                    {statusFilter === null ? <Check size={12} /> : null}
                  </span>
                  <span aria-hidden="true" />
                  <span className="status-filter-option-label">All statuses</span>
                  <span className="tabular muted">{totalTasks}</span>
                </button>
                {TASK_STATUSES.map((status) => (
                  <button
                    key={status}
                    type="button"
                    className="status-filter-option"
                    role="menuitemradio"
                    aria-checked={statusFilter === status}
                    data-active={statusFilter === status}
                    onClick={() => chooseStatus(status)}
                  >
                    <span className="status-filter-check" aria-hidden="true">
                      {statusFilter === status ? <Check size={12} /> : null}
                    </span>
                    <span className="state-dot" data-state={status} aria-hidden="true" />
                    <span className="status-filter-option-label">
                      {TASK_STATUS_META[status].label}
                    </span>
                    <span className="tabular muted">{statusCounts[status]}</span>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        ) : (
          TASK_STATUSES.map((status) => (
            <button
              key={status}
              type="button"
              className="status-counter"
              data-tone={status}
              data-active={statusFilter === status}
              aria-pressed={statusFilter === status}
              onClick={() =>
                onSelectStatus(statusFilter === status ? null : status)
              }
            >
              <span className="state-dot" data-state={status} aria-hidden="true" />
              <span>{TASK_STATUS_META[status].label}</span>
              <span className="tabular">{statusCounts[status]}</span>
            </button>
          ))
        )}
      </div>
      <div className="status-bar-right">
        <span className="mono" title="Tasks and relationships in the current Snapshot">
          {totalTasks} tasks · {relationCount} relations
        </span>
        {zoomPercent !== null ? (
          <span className="mono" title="Board zoom">
            {zoomPercent}%
          </span>
        ) : null}
        <span className="mono status-hints" aria-hidden="true">
          ⌘K · I · ?
        </span>
      </div>
    </footer>
  );
}
