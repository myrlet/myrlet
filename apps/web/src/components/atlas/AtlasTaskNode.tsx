/**
 * One Board card.
 *
 * A card states the Task title, its subtree summary, and one truthful footer
 * line: real Harness Progress wins, otherwise only a Delivery fact the status
 * badge does not already imply (such as a cancellation answer) is shown.
 */

import { shortId } from "../../model/shortId";
import type { DeliveryView, SnapshotTaskSummary, WorkerRecord } from "../../api/types";
import { CANCEL_STATE_LABELS, TASK_STATUS_META, formatCount, formatRelativeTime } from "../../format";
import { NODE_HEIGHT, NODE_WIDTH } from "../workAtlasLayout";
import type { AtlasDensity } from "./atlasViewport";
import { TaskStatusBadge } from "../StatusBadge";

export interface AtlasTaskNodeProps {
  readonly task: SnapshotTaskSummary;
  readonly x: number;
  readonly y: number;
  readonly selected: boolean;
  readonly hovered: boolean;
  readonly dragging: boolean;
  readonly dimmed: boolean;
  readonly density: AtlasDensity;
  readonly worker: WorkerRecord | undefined;
  readonly onHover: (taskId: string | null) => void;
  readonly onSelect: (taskId: string) => void;
}

const NODE_STYLE = { width: NODE_WIDTH, height: NODE_HEIGHT } as const;


const isWorkerOnline = (worker: WorkerRecord | undefined): boolean =>
  worker !== undefined && worker.online && !worker.revoked;

/**
 * Footer line under the card. Real Harness Progress always wins; otherwise only a
 * Delivery fact the Task badge does not already imply earns a line, such as a
 * cancellation answer. `accepted` and `finished` repeat the Task state.
 */
function deliveryFooter(
  delivery: DeliveryView,
): { text: string; live: boolean; ageMs: number | null } | null {
  if (delivery.progress) {
    return { text: delivery.progress.text, live: true, ageMs: delivery.progress.receivedAtMs };
  }
  const text = delivery.cancelState ? CANCEL_STATE_LABELS[delivery.cancelState] : null;
  return text === null ? null : { text, live: false, ageMs: delivery.updatedAtMs };
}

export function AtlasTaskNode(props: AtlasTaskNodeProps) {
  const { task, x, y, selected, hovered, dragging, dimmed, density, worker, onHover, onSelect } =
    props;
  const delivery = task.currentDelivery;
  const online = isWorkerOnline(worker);
  const title = task.title || task.id;
  const statusLabel = TASK_STATUS_META[task.status].label;
  const descendantCount = Math.max(0, task.subtree.total - 1);
  const finishedDescendants = Math.max(
    0,
    task.subtree.finished - (task.status === "finished" ? 1 : 0),
  );
  const footer = delivery ? deliveryFooter(delivery) : null;
  const executor = delivery?.executorId ?? delivery?.slotId ?? null;
  const workerTitle = delivery
    ? `Worker ${delivery.workerId} — ${online ? "online" : "offline"}` +
      (executor === null ? "" : ` · ${delivery.executorId ? "Executor" : "Slot"} ${executor}`)
    : "";
  /* The parent's subtree summary sits under the title so a card states what it
     is before how it stands. Worker/category stay on the bottom status line. */
  const summary =
    descendantCount > 0 ? (
      <span className="atlas-node-summary">
        {formatCount(descendantCount)} {descendantCount === 1 ? "subtask" : "subtasks"}
        {finishedDescendants > 0 ? ` · ${formatCount(finishedDescendants)} ended` : ""}
      </span>
    ) : null;
  const context = delivery ? (
    <span className="atlas-node-worker" title={workerTitle}>
      <span className="connection-dot" data-state={online ? "online" : "offline"} aria-hidden="true" />
      <span className="mono truncate">{shortId(delivery.workerId)}</span>
    </span>
  ) : task.category ? (
    <span className="atlas-node-chip truncate">{task.category}</span>
  ) : null;
  return (
    <div
      role="button" tabIndex={0} className="atlas-node" data-task-id={task.id}
      data-status={task.status}
      data-selected={selected} data-hovered={hovered} data-dragging={dragging} data-density={density}
      data-dimmed={dimmed ? "true" : undefined}
      aria-label={`${title} — ${statusLabel}${footer ? ` — ${footer.text}` : ""}${delivery ? ` — ${workerTitle}` : ""}`}
      title={`${title} (${task.id})`}
      style={{ ...NODE_STYLE, left: x, top: y, cursor: dragging ? "grabbing" : "grab" }}
      onClick={() => onSelect(task.id)}
      onPointerEnter={() => onHover(task.id)}
      onPointerLeave={() => onHover(null)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(task.id);
        }
      }}
    >
      <div className="atlas-node-title">{title}</div>
      {density === "compact" ? null : summary}
      {density === "compact" || footer === null ? null : (
        <div className="atlas-node-footer" data-live={footer.live ? "true" : undefined}>
          <span className="atlas-node-progress truncate" title={footer.text}>{footer.text}</span>
          {footer.ageMs === null ? null : (
            <span className="atlas-node-age tabular">{formatRelativeTime(footer.ageMs)}</span>
          )}
        </div>
      )}
      <div className="atlas-node-status">
        <TaskStatusBadge status={task.status} />
        {context}
      </div>
    </div>
  );
}
