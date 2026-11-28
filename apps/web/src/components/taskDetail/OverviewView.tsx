/**
 * Overview tab: the current facts about the Task that do not need the tree
 * read — state, version, category, Worker, the latest Result, the latest
 * Harness Progress, prompt, routing constraints, tags, and timestamps.
 */

import type { Ref } from "react";
import { Copy } from "lucide-react";

import type { TaskRecord } from "../../api/types";
import { formatRelativeTime, formatTimestamp } from "../../format";
import { ResultPreview } from "../ResultPreview";
import { TaskStatusBadge } from "../StatusBadge";
import { IdValue, MetaCell, TimestampValue } from "./fields";

export interface OverviewViewProps {
  readonly task: TaskRecord;
  readonly onCopyText: (label: string, text: string) => void;
  /** Opens the full-result reader. */
  readonly onOpenReader: () => void;
  /** The `Read full result` trigger; focus returns here after closing. */
  readonly readerTriggerRef?: Ref<HTMLButtonElement>;
}

export function OverviewView({
  task,
  onCopyText,
  onOpenReader,
  readerTriggerRef,
}: OverviewViewProps) {
  const definition = task.task;
  const routing = definition.routing;
  const requiredCapabilities = routing.requiredCapabilities ?? [];
  const progress = task.currentDelivery?.progress ?? null;

  const routingChips: readonly { key: string; label: string; value: string }[] = [
    ...requiredCapabilities.map((capability) => ({
      key: `capability:${capability}`,
      label: "capability",
      value: capability,
    })),
    ...(routing.preferredWorkerId
      ? [{ key: "preferredWorkerId", label: "worker", value: routing.preferredWorkerId }]
      : []),
    ...(routing.preferredRole
      ? [{ key: "preferredRole", label: "role", value: routing.preferredRole }]
      : []),
    ...(routing.preferredExecutorId
      ? [{ key: "preferredExecutorId", label: "executor", value: routing.preferredExecutorId }]
      : []),
  ];

  return (
    <>
      <div
        className="meta-grid-2x2"
        style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))" }}
      >
        <MetaCell label="Status">
          <TaskStatusBadge status={task.status} />
        </MetaCell>
        <MetaCell label="Version">
          <span className="mono tabular" title="Task version (optimistic concurrency)">
            v{task.version}
          </span>
        </MetaCell>
        <MetaCell label="Category">
          <span className={definition.category === "" ? "cell-empty" : undefined}>
            {definition.category === "" ? "—" : definition.category}
          </span>
        </MetaCell>
        <MetaCell label="Worker">
          <IdValue id={task.currentDelivery?.workerId ?? null} empty="— (no Delivery)" />
        </MetaCell>
      </div>

      {task.resultMarkdown === null ? null : (
        <ResultPreview
          markdown={task.resultMarkdown}
          recordedAtMs={task.resultRecordedAtMs}
          onCopyText={onCopyText}
          onOpenReader={onOpenReader}
          triggerRef={readerTriggerRef}
        />
      )}

      {progress === null ? null : (
        <section className="overview-progress-card" aria-label="Latest Harness progress">
          <div className="overview-progress-head">
            <span className="overview-progress-kicker">LATEST PROGRESS</span>
            <span className="overview-progress-provider">Harness report</span>
            <span className="header-spacer" />
            <span
              className="overview-progress-age tabular"
              title={`Received ${formatTimestamp(progress.receivedAtMs)}`}
            >
              {formatRelativeTime(progress.receivedAtMs)}
            </span>
          </div>
          <p className="overview-progress-text">{progress.text}</p>
        </section>
      )}

      <section className="detail-section">
        <div className="prompt-box-head">
          <h3 className="detail-section-title">Prompt</h3>
          <span className="header-spacer" />
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={() => onCopyText("Prompt", definition.prompt)}
            title="Copy Prompt"
          >
            <Copy size={12} aria-hidden="true" />
            <span>Copy</span>
          </button>
        </div>
        <div className="prompt-box">
          <pre className="prompt-box-text">{definition.prompt}</pre>
        </div>
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Routing constraints</h3>
        {routingChips.length === 0 ? (
          <p className="detail-empty">No routing constraints; any eligible Worker/Slot matches.</p>
        ) : (
          <ul className="routing-chips">
            {routingChips.map((chip) => (
              <li key={chip.key} className="tag" title={`${chip.label}: ${chip.value}`}>
                <span className="muted">{chip.label}</span>
                <span className="mono">{chip.value}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Tags</h3>
        {definition.tags.length === 0 ? (
          <p className="detail-empty">No tags.</p>
        ) : (
          <ul className="routing-chips">
            {definition.tags.map((tagValue) => (
              <li key={tagValue} className="tag">
                {tagValue}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Timestamps</h3>
        <div className="detail-grid">
          <span className="detail-label">Created</span>
          <TimestampValue ms={task.createdAtMs} />
          <span className="detail-label">Updated</span>
          <TimestampValue ms={task.updatedAtMs} />
          <span className="detail-label">Handling ended</span>
          <TimestampValue ms={task.finishedAtMs} />
        </div>
      </section>
    </>
  );
}
