/**
 * Status pills for Task state and Worker connectivity.
 *
 * Every badge pairs its semantic colour with an explicit text label and a
 * shape glyph, so state never depends on colour alone. `finished` renders the
 * neutral square + "Handling ended" wording from `TASK_STATUS_META` — never a
 * green checkmark or a success verdict.
 */

import type { TaskStatus } from "../api/types";
import { TASK_STATUS_META } from "../format";

export interface TaskStatusBadgeProps {
  status: TaskStatus;
  /** Render the honest one-line meaning next to the pill as well as in `title`. */
  showDescription?: boolean;
}

export function TaskStatusBadge({ status, showDescription = false }: TaskStatusBadgeProps) {
  const meta = TASK_STATUS_META[status];

  const badge = (
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

  if (!showDescription) {
    return badge;
  }

  return (
    <span className="inline">
      {badge}
      <span className="field-hint">{meta.description}</span>
    </span>
  );
}

export interface WorkerStatusBadgeProps {
  online: boolean;
  revoked: boolean;
}

/**
 * Worker connectivity pill. A revoked credential always wins over `online`:
 * a revoked Worker cannot hold an authenticated session.
 */
export function WorkerStatusBadge({ online, revoked }: WorkerStatusBadgeProps) {
  const state = revoked
    ? {
        tone: "revoked",
        label: "Revoked",
        glyph: "⊘",
        description: "Credential revoked; this Worker can no longer connect to the Hub.",
      }
    : online
      ? {
          tone: "online",
          label: "Online",
          glyph: "●",
          description: "Connected to the Hub and authenticated.",
        }
      : {
          tone: "offline",
          label: "Offline",
          glyph: "○",
          description: "Not currently connected to the Hub.",
        };

  return (
    <span className="status-badge" data-tone={state.tone} title={state.description}>
      <span className="status-glyph" aria-hidden="true">
        {state.glyph}
      </span>
      <span className="status-label">{state.label}</span>
    </span>
  );
}
