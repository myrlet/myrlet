/**
 * Status badges for Task state and Worker connectivity.
 *
 * Every badge is a borderless geometric glyph paired with an explicit text
 * label, so state never depends on colour alone. `finished` renders the neutral
 * square + "Handling ended" wording from `TASK_STATUS_META` — never a green
 * checkmark or a success verdict. `compact` renders only the glyph, with the
 * honest label exposed to assistive technology.
 */

import type { TaskStatus, WorkerApprovalStatus } from "../api/types";
import { TASK_STATUS_META } from "../format";

export interface TaskStatusBadgeProps {
  status: TaskStatus;
  /** Render the geometric glyph only; the label stays available via `aria-label`. */
  compact?: boolean;
}

export function TaskStatusBadge({ status, compact = false }: TaskStatusBadgeProps) {
  const meta = TASK_STATUS_META[status];

  if (compact) {
    return (
      <span
        className="status-badge"
        data-tone={meta.tone}
        data-shape={meta.shape}
        data-terminal={meta.terminal}
        data-compact="true"
        role="img"
        aria-label={meta.label}
        title={`${meta.label} — ${meta.description}`}
      >
        <span className="status-glyph" aria-hidden="true">
          {meta.glyph}
        </span>
      </span>
    );
  }

  return (
    <span
      className="status-badge"
      data-tone={meta.tone}
      data-shape={meta.shape}
      data-terminal={meta.terminal}
      title={meta.description}
    >
      <span className="status-glyph" aria-hidden="true">
        {meta.glyph}
      </span>
      <span className="status-label">{meta.label}</span>
    </span>
  );
}

export interface WorkerStatusBadgeProps {
  online: boolean;
  revoked: boolean;
  approval: WorkerApprovalStatus;
}

/**
 * Worker connectivity badge: a hairline status dot plus an explicit label.
 * A revoked credential always wins. A pending Worker is connected but not yet
 * admitted to scheduling, so it reads "Awaiting approval", never "Online".
 */
export function WorkerStatusBadge({ online, revoked, approval }: WorkerStatusBadgeProps) {
  const state = revoked
    ? {
        tone: "revoked",
        label: "Revoked",
        description: "Credential revoked; this Worker can no longer connect to the Hub.",
      }
    : approval === "pending"
      ? {
          tone: "pending",
          label: "Awaiting approval",
          description: "Paired and connected, but not yet admitted to scheduling by an Admin.",
        }
      : online
        ? {
            tone: "online",
            label: "Online",
            description: "Connected to the Hub and authenticated.",
          }
        : {
            tone: "offline",
            label: "Offline",
            description: "Not currently connected to the Hub.",
          };

  return (
    <span className="status-badge" data-tone={state.tone} data-shape="dot" title={state.description}>
      <span className="connection-dot" aria-hidden="true" />
      <span className="status-label">{state.label}</span>
    </span>
  );
}
