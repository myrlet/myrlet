/**
 * Read-only Task Detail pane with four segmented tabs: Overview, Tree & Deps,
 * Delivery, and JSON. Every write entry point (Dispatch / Finish / Cancel /
 * Subtask / Copy-Retry) belongs to the CLI, not this UI.
 *
 * Facts come from the Hub and the Harness only. `finished` reads "Handling
 * ended", cancellation states such as `uncertain` stay visible, and a missing
 * Progress or Delivery is stated explicitly instead of being guessed.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { AlertCircle, Copy, Inbox, RotateCcw, X } from "lucide-react";

import type { SnapshotTaskSummary, TaskRecord, TaskTreeResponse } from "../api/types";
import {
  CANCEL_STATE_LABELS,
  DELIVERY_STATUS_LABELS,
  formatRelativeTime,
  formatTimestamp,
} from "../format";
import { ResultPreview } from "./ResultPreview";
import { ResultReader } from "./ResultReader";
import { TaskStatusBadge } from "./StatusBadge";

export interface TaskDetailPanelProps {
  readonly selectedTaskId: string | null;
  readonly summary: SnapshotTaskSummary | null;
  readonly task: TaskRecord | null;
  readonly tree: TaskTreeResponse | null;
  readonly loading: boolean;
  readonly error: string | null;
  readonly onClose: () => void;
  readonly onSelectTask: (taskId: string) => void;
  readonly onCopyText: (label: string, text: string) => void;
  /** Re-read the current selection without dropping what is already shown. */
  readonly onRetry: () => void;
  /** Dismiss the failure notice and keep the panel open. */
  readonly onDismissError: () => void;
}

type DetailTab = "overview" | "tree" | "delivery" | "json";

const DETAIL_TABS: readonly { id: DetailTab; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "tree", label: "Tree & Deps" },
  { id: "delivery", label: "Delivery" },
  { id: "json", label: "JSON" },
];

/* --------------------------------------------------------------------- */
/* Small factual helpers                                                  */
/* --------------------------------------------------------------------- */

/** Absolute timestamp with its relative age; `—` when the fact is absent. */
function TimestampValue({ ms }: { ms: number | null }) {
  if (ms == null) {
    return <span className="detail-value cell-empty">—</span>;
  }
  return (
    <span className="detail-value mono">
      {formatTimestamp(ms)} <span className="muted">({formatRelativeTime(ms)})</span>
    </span>
  );
}

/** Monospace identifier cell with an explicit empty state. */
function IdValue({ id, empty = "—" }: { id: string | null; empty?: string }) {
  if (id == null || id === "") {
    return <span className="detail-value cell-empty">{empty}</span>;
  }
  return <span className="detail-value mono">{id}</span>;
}

function MetaCell({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="meta-cell">
      <span className="meta-label">{label}</span>
      <span className="meta-value">{children}</span>
    </div>
  );
}

function shortId(id: string): string {
  return id.length <= 12 ? id : id.slice(0, 12);
}

/** Keeps both ends of a Task ID readable inside a single non-wrapping row. */
function middleTruncateId(id: string): string {
  return id.length > 18 ? `${id.slice(0, 10)}…${id.slice(-4)}` : id;
}

/** Root-to-current ancestry resolved against the bounded tree read. */
function buildAncestry(task: TaskRecord, tree: TaskTreeResponse | null): TaskRecord[] {
  if (tree === null || tree.tasks.length === 0) {
    return [task];
  }
  const byId = new Map<string, TaskRecord>();
  for (const candidate of tree.tasks) {
    byId.set(candidate.id, candidate);
  }
  const chain: TaskRecord[] = [byId.get(task.id) ?? task];
  const seen = new Set<string>([chain[0].id]);
  let current = chain[0];

  while (current.task.parentTaskId !== null && chain.length < 12) {
    const parent = byId.get(current.task.parentTaskId);
    if (!parent || seen.has(parent.id)) {
      break;
    }
    seen.add(parent.id);
    chain.unshift(parent);
    current = parent;
  }

  return chain;
}

function compareTasks(a: TaskRecord, b: TaskRecord): number {
  if (a.createdAtMs !== b.createdAtMs) {
    return a.createdAtMs - b.createdAtMs;
  }
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** Depth-first mini tree over the bounded Task tree read. */
function miniTreeRows(tree: TaskTreeResponse): TaskRecord[] {
  const byId = new Map<string, TaskRecord>();
  for (const task of tree.tasks) {
    byId.set(task.id, task);
  }
  const roots: TaskRecord[] = [];
  const childrenByParent = new Map<string, TaskRecord[]>();

  for (const task of tree.tasks) {
    const parentId = task.task.parentTaskId;
    if (parentId === null || parentId === task.id || !byId.has(parentId)) {
      roots.push(task);
      continue;
    }
    const siblings = childrenByParent.get(parentId);
    if (siblings) {
      siblings.push(task);
    } else {
      childrenByParent.set(parentId, [task]);
    }
  }

  roots.sort(compareTasks);
  for (const siblings of childrenByParent.values()) {
    siblings.sort(compareTasks);
  }

  const rows: TaskRecord[] = [];
  const visited = new Set<string>();
  const visit = (task: TaskRecord): void => {
    if (visited.has(task.id)) {
      return;
    }
    visited.add(task.id);
    rows.push(task);
    for (const child of childrenByParent.get(task.id) ?? []) {
      visit(child);
    }
  };
  for (const root of roots) {
    visit(root);
  }
  for (const task of tree.tasks) {
    if (!visited.has(task.id)) {
      rows.push(task);
    }
  }
  return rows;
}

/* --------------------------------------------------------------------- */
/* Pure-TS JSON syntax highlighting                                       */
/* --------------------------------------------------------------------- */

type JsonTokenKind = "plain" | "key" | "string" | "number" | "literal" | "punct";

interface JsonToken {
  text: string;
  kind: JsonTokenKind;
}

const JSON_TOKEN_PATTERN =
  /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b|([{}[\],:])/g;

function tokenizeJson(json: string): JsonToken[] {
  const tokens: JsonToken[] = [];
  let cursor = 0;
  JSON_TOKEN_PATTERN.lastIndex = 0;
  let match = JSON_TOKEN_PATTERN.exec(json);

  while (match !== null) {
    const [whole, stringToken, colonToken, numberToken, literalToken, punctToken] =
      match as unknown as [
        string,
        string | undefined,
        string | undefined,
        string | undefined,
        string | undefined,
        string | undefined,
      ];

    if (match.index > cursor) {
      tokens.push({ text: json.slice(cursor, match.index), kind: "plain" });
    }
    if (stringToken !== undefined) {
      tokens.push({ text: stringToken, kind: colonToken !== undefined ? "key" : "string" });
      if (colonToken !== undefined) {
        tokens.push({ text: colonToken, kind: "punct" });
      }
    } else if (numberToken !== undefined) {
      tokens.push({ text: numberToken, kind: "number" });
    } else if (literalToken !== undefined) {
      tokens.push({ text: literalToken, kind: "literal" });
    } else if (punctToken !== undefined) {
      tokens.push({ text: punctToken, kind: "punct" });
    }
    cursor = match.index + whole.length;
    match = JSON_TOKEN_PATTERN.exec(json);
  }

  if (cursor < json.length) {
    tokens.push({ text: json.slice(cursor), kind: "plain" });
  }
  return tokens;
}

function JsonViewer({ value }: { value: unknown }) {
  const json = JSON.stringify(value, null, 2) ?? "null";
  const tokens = tokenizeJson(json);
  return (
    <pre className="json-viewer" aria-label="Task JSON" tabIndex={0}>
      {tokens.map((token, index) =>
        token.kind === "plain" ? (
          <span key={index}>{token.text}</span>
        ) : (
          <span key={index} className={`json-token json-${token.kind}`}>
            {token.text}
          </span>
        ),
      )}
    </pre>
  );
}

/* --------------------------------------------------------------------- */
/* Failure notice                                                         */
/* --------------------------------------------------------------------- */

/**
 * In-panel load failure. The last known Task name and status stay in the
 * header; this only reports the failed read and offers a retry for the current
 * Task plus an explicit dismiss.
 */
function DetailErrorNotice({
  message,
  busy,
  onRetry,
  onDismiss,
}: {
  message: string;
  busy: boolean;
  onRetry: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="detail-error" role="alert" aria-label="Task load failed">
      <AlertCircle size={14} aria-hidden="true" />
      <div className="detail-error-body">
        <span className="detail-error-title">Could not load this Task</span>
        <span className="detail-error-message">{message}</span>
      </div>
      <div className="detail-error-actions">
        <button
          type="button"
          className="button button-sm"
          onClick={onRetry}
          disabled={busy}
          aria-busy={busy}
        >
          <RotateCcw size={12} aria-hidden="true" />
          <span>{busy ? "Retrying…" : "Retry"}</span>
        </button>
        <button
          type="button"
          className="icon-button"
          onClick={onDismiss}
          aria-label="Dismiss load failure"
          title="Dismiss load failure"
        >
          <X size={13} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------------- */
/* Panel                                                                  */
/* --------------------------------------------------------------------- */

export function TaskDetailPanel(props: TaskDetailPanelProps) {
  const {
    selectedTaskId,
    summary,
    task,
    tree,
    loading,
    error,
    onClose,
    onSelectTask,
    onCopyText,
    onRetry,
    onDismissError,
  } = props;

  const [tab, setTab] = useState<DetailTab>("overview");

  // Result reader: modally over the Inspector, without touching the tab or the
  // Overview reading position. Closing restores both focus and scroll.
  const [readerOpen, setReaderOpen] = useState(false);
  const detailBodyRef = useRef<HTMLDivElement | null>(null);
  const readerTriggerRef = useRef<HTMLButtonElement | null>(null);
  const readerScrollRef = useRef(0);
  const readerWasOpenRef = useRef(false);

  const openReader = useCallback(() => {
    readerScrollRef.current = detailBodyRef.current?.scrollTop ?? 0;
    readerWasOpenRef.current = true;
    setReaderOpen(true);
  }, []);

  const closeReader = useCallback(() => {
    setReaderOpen(false);
  }, []);

  useLayoutEffect(() => {
    if (readerOpen || !readerWasOpenRef.current) {
      return;
    }
    readerWasOpenRef.current = false;
    const body = detailBodyRef.current;
    if (body !== null) {
      body.scrollTop = readerScrollRef.current;
    }
    if (readerTriggerRef.current?.isConnected) {
      // Focusing first would scroll the trigger into view and discard the
      // restored reading position in a real browser.
      readerTriggerRef.current.focus({ preventScroll: true });
      if (body !== null) {
        body.scrollTop = readerScrollRef.current;
      }
    }
  }, [readerOpen]);

  const selectionId = selectedTaskId ?? task?.id ?? null;

  // A new Task selection always starts on Overview.
  useEffect(() => {
    setTab("overview");
  }, [selectionId]);

  if (selectionId === null) {
    return (
      <aside className="detail-pane" aria-label="Task detail">
        <div className="pane-empty">
          <Inbox size={20} aria-hidden="true" />
          <span className="pane-empty-title">No Task selected</span>
          <span>Select a Task to inspect its Overview, Tree &amp; Deps, and Delivery.</span>
        </div>
      </aside>
    );
  }

  if (task === null) {
    // Snapshot facts are enough to keep the header honest while the full read
    // loads or after it failed: the Task never appears to be unselected.
    const summaryTitle = summary?.title || summary?.id || selectionId;
    const summaryStatus = summary?.status ?? null;
    return (
      <aside className="detail-pane" aria-label="Task detail">
        <header className="detail-header">
          <div className="detail-heading">
            <nav className="detail-breadcrumb" aria-label="Task ancestry">
              <span className="detail-breadcrumb-current">Task</span>
            </nav>
            <h2 className="detail-title">{summaryTitle}</h2>
            <div className="detail-subtitle" style={{ flexWrap: "nowrap" }}>
              {summaryStatus === null ? null : <TaskStatusBadge status={summaryStatus} />}
              <span className="detail-id-short mono" title={selectionId}>
                {middleTruncateId(selectionId)}
              </span>
              {summary === null ? null : (
                <span
                  className="detail-version-pill mono"
                  title="Task version (optimistic concurrency)"
                >
                  v{summary.version}
                </span>
              )}
              {loading ? (
                <span className="detail-refreshing" role="status">
                  Loading…
                </span>
              ) : null}
            </div>
          </div>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="Close task detail"
            title="Close task detail"
          >
            <X size={15} aria-hidden="true" />
          </button>
        </header>

        <div className="detail-body">
          {error === null ? (
            loading ? (
              <div className="loading-state" role="status">
                Loading Task…
              </div>
            ) : (
              <div className="detail-fallback">
                <p className="detail-empty">Details are not loaded.</p>
                <button type="button" className="button button-sm" onClick={onRetry}>
                  <RotateCcw size={12} aria-hidden="true" />
                  <span>Load Task</span>
                </button>
              </div>
            )
          ) : (
            <DetailErrorNotice
              message={error}
              busy={loading}
              onRetry={onRetry}
              onDismiss={onDismissError}
            />
          )}
        </div>
      </aside>
    );
  }

  const definition = task.task;
  const routing = definition.routing;
  const requiredCapabilities = routing.requiredCapabilities ?? [];
  const sourceTaskId = definition.sourceTaskId;
  const delivery = task.currentDelivery;
  const progress = delivery?.progress ?? null;

  const treeTasks = tree?.tasks ?? [];
  const taskById = new Map(treeTasks.map((candidate) => [candidate.id, candidate]));
  const ancestry = buildAncestry(task, tree);
  const miniRows = tree === null ? [] : miniTreeRows(tree);
  const minMiniDepth = miniRows.reduce(
    (min, candidate) => Math.min(min, candidate.depth),
    miniRows.length > 0 ? miniRows[0].depth : 0,
  );

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

  const breadcrumb = ((): ReactNode => {
    if (ancestry.length <= 1) {
      return <span className="detail-breadcrumb-current">Root Task</span>;
    }
    return ancestry.map((ancestor, index) => {
      const isLast = index === ancestry.length - 1;
      return (
        <span className="detail-breadcrumb-item" key={ancestor.id}>
          {isLast ? (
            <span className="detail-breadcrumb-current truncate" title={ancestor.id}>
              {ancestor.task.title || shortId(ancestor.id)}
            </span>
          ) : (
            <>
              <button
                type="button"
                className="detail-breadcrumb-link truncate"
                onClick={() => onSelectTask(ancestor.id)}
                title={`Open ${ancestor.task.title} (${ancestor.id})`}
              >
                {ancestor.task.title || shortId(ancestor.id)}
              </button>
              <span className="detail-breadcrumb-sep" aria-hidden="true">
                ›
              </span>
            </>
          )}
        </span>
      );
    });
  })();

  const renderOverview = (): ReactNode => (
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
          <IdValue id={delivery?.workerId ?? null} empty="— (no Delivery)" />
        </MetaCell>
      </div>

      {task.resultMarkdown === null ? null : (
        <ResultPreview
          markdown={task.resultMarkdown}
          recordedAtMs={task.resultRecordedAtMs}
          onCopyText={onCopyText}
          onOpenReader={openReader}
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

  const renderTree = (): ReactNode => (
    <>
      <section className="detail-section">
        <h3 className="detail-section-title">Source Task</h3>
        {sourceTaskId == null ? (
          <p className="detail-empty">No source Task; this Task was created directly.</p>
        ) : (
          <button
            type="button"
            className="hierarchy-link mono"
            onClick={() => onSelectTask(sourceTaskId)}
            title={`Open source Task ${sourceTaskId}`}
          >
            {sourceTaskId}
          </button>
        )}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Sibling dependencies</h3>
        {definition.dependencies.length === 0 ? (
          <p className="detail-empty">No sibling dependencies declared.</p>
        ) : (
          <ul className="dep-list">
            {definition.dependencies.map((dependencyId) => {
              const resolved = taskById.get(dependencyId) ?? null;
              return (
                <li key={dependencyId}>
                  <div className="dep-card" data-resolved={resolved !== null}>
                    <button
                      type="button"
                      className="dep-card-main"
                      onClick={() => onSelectTask(dependencyId)}
                      title={`Open dependency ${dependencyId}`}
                    >
                      <span className="dep-card-title truncate">
                        {resolved === null
                          ? "Not in the loaded Task tree"
                          : resolved.task.title || resolved.id}
                      </span>
                      <span className="dep-card-id mono" title={dependencyId}>
                        {shortId(dependencyId)}
                      </span>
                    </button>
                    {resolved === null ? (
                      <span className="cell-empty">—</span>
                    ) : (
                      <TaskStatusBadge status={resolved.status} compact />
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
        {tree === null ? (
          <p className="detail-empty">Related Tasks were not loaded.</p>
        ) : tree.truncated ? (
          <p className="detail-empty">
            The related-Task tree read was truncated; some Tasks may be missing.
          </p>
        ) : null}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Context mini-tree</h3>
        {resourceMiniTree(miniRows, minMiniDepth, task.id, onSelectTask)}
      </section>
    </>
  );

  const renderDelivery = (): ReactNode => (
    <>
      <section className="detail-section">
        <h3 className="detail-section-title">Current Delivery</h3>
        {delivery == null ? (
          <p className="detail-empty">No Delivery assigned yet.</p>
        ) : (
          <div className="detail-grid">
            <span className="detail-label">Delivery</span>
            <span className="detail-value mono">{delivery.id}</span>

            <span className="detail-label">State</span>
            <span className="detail-value">{DELIVERY_STATUS_LABELS[delivery.state]}</span>

            <span className="detail-label">Worker</span>
            <span className="detail-value mono">{delivery.workerId}</span>

            <span className="detail-label">Slot</span>
            <span className="detail-value mono">{delivery.slotId}</span>

            <span className="detail-label">Executor</span>
            <IdValue id={delivery.executorId} />

            <span className="detail-label">Connection epoch</span>
            <span className="detail-value tabular">{delivery.connectionEpoch}</span>

            {delivery.cancelState == null ? null : (
              <>
                <span className="detail-label">Cancel</span>
                <span className="detail-value">{CANCEL_STATE_LABELS[delivery.cancelState]}</span>
              </>
            )}

            {delivery.cancelReason == null || delivery.cancelReason === "" ? null : (
              <>
                <span className="detail-label">Cancel reason</span>
                <span className="detail-value">{delivery.cancelReason}</span>
              </>
            )}
          </div>
        )}
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">Latest Progress</h3>
        {delivery == null ? (
          <p className="detail-empty">No Delivery assigned yet.</p>
        ) : progress == null ? (
          <p className="detail-empty">No Harness progress reported yet.</p>
        ) : (
          <div className="progress-block">
            <p className="progress-block-text">{progress.text}</p>
            <div className="progress-block-meta">
              <span title={formatTimestamp(progress.firstReceivedAtMs)}>
                Execution started: {formatRelativeTime(progress.firstReceivedAtMs)}
              </span>
              <span title={formatTimestamp(progress.receivedAtMs)}>
                Last updated: {formatRelativeTime(progress.receivedAtMs)}
              </span>
            </div>
          </div>
        )}
      </section>
    </>
  );

  const cliSnippet = `myrlet tasks show ${task.id}`;

  const renderJson = (): ReactNode => (
    <>
      <section className="detail-section">
        <div className="prompt-box-head">
          <h3 className="detail-section-title">Task JSON</h3>
          <span className="header-spacer" />
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={() => onCopyText("Task JSON", JSON.stringify(task, null, 2))}
            title="Copy the full Task JSON"
          >
            <Copy size={12} aria-hidden="true" />
            <span>Copy JSON</span>
          </button>
        </div>
        <JsonViewer value={task} />
      </section>

      <section className="detail-section">
        <h3 className="detail-section-title">CLI</h3>
        <div className="copy-row">
          <code className="code-block">{cliSnippet}</code>
          <button
            type="button"
            className="button button-sm button-ghost"
            onClick={() => onCopyText("CLI command", cliSnippet)}
            title="Copy CLI command"
            aria-label="Copy CLI command"
          >
            <Copy size={12} aria-hidden="true" />
          </button>
        </div>
      </section>
    </>
  );

  return (
    <aside className="detail-pane" aria-label="Task detail">
      <header className="detail-header">
        <div className="detail-heading">
          <nav className="detail-breadcrumb" aria-label="Task ancestry">
            {breadcrumb}
          </nav>
          <h2 className="detail-title">{definition.title}</h2>
          <div className="detail-subtitle" style={{ flexWrap: "nowrap" }}>
            <TaskStatusBadge status={task.status} />
            <span className="detail-id-short mono" title={task.id}>
              {middleTruncateId(task.id)}
            </span>
            <button
              type="button"
              className="icon-button"
              onClick={() => onCopyText("Task ID", task.id)}
              aria-label="Copy Task ID"
              title="Copy Task ID"
            >
              <Copy size={12} aria-hidden="true" />
            </button>
            <span className="detail-version-pill mono" title="Task version (optimistic concurrency)">
              v{task.version}
            </span>
            {loading ? <span className="detail-refreshing">Refreshing…</span> : null}
          </div>
        </div>
        <button
          type="button"
          className="icon-button"
          onClick={onClose}
          aria-label="Close task detail"
          title="Close task detail"
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <div className="detail-tabs" role="tablist" aria-label="Task detail sections">
        {DETAIL_TABS.map((entry) => (
          <button
            key={entry.id}
            type="button"
            role="tab"
            id={`detail-tab-${entry.id}`}
            className="detail-tab"
            data-active={tab === entry.id}
            aria-selected={tab === entry.id}
            aria-controls="detail-tabpanel"
            tabIndex={tab === entry.id ? 0 : -1}
            onClick={() => setTab(entry.id)}
          >
            {entry.label}
          </button>
        ))}
      </div>

      <div
        className="detail-body"
        id="detail-tabpanel"
        role="tabpanel"
        aria-labelledby={`detail-tab-${tab}`}
        ref={detailBodyRef}
      >
        {error === null ? null : (
          <DetailErrorNotice
            message={error}
            busy={loading}
            onRetry={onRetry}
            onDismiss={onDismissError}
          />
        )}

        {tab === "overview" ? renderOverview() : null}
        {tab === "tree" ? renderTree() : null}
        {tab === "delivery" ? renderDelivery() : null}
        {tab === "json" ? renderJson() : null}
      </div>

      {readerOpen && task.resultMarkdown !== null ? (
        <ResultReader
          title={definition.title}
          markdown={task.resultMarkdown}
          recordedAtMs={task.resultRecordedAtMs}
          onCopyText={onCopyText}
          onRequestClose={closeReader}
        />
      ) : null}
    </aside>
  );
}

/** Interactive indented context tree over the bounded tree read. */
function resourceMiniTree(
  rows: readonly TaskRecord[],
  minDepth: number,
  currentTaskId: string,
  onSelectTask: (taskId: string) => void,
): ReactNode {
  if (rows.length === 0) {
    return <p className="detail-empty">Related Tasks were not loaded.</p>;
  }
  return (
    <ul className="mini-tree">
      {rows.map((row) => {
        const current = row.id === currentTaskId;
        const indent = Math.max(0, row.depth - minDepth);
        return (
          <li key={row.id}>
            <button
              type="button"
              className="mini-tree-node"
              data-current={current}
              aria-current={current ? "true" : undefined}
              onClick={() => onSelectTask(row.id)}
              title={`${row.task.title} (${row.id})`}
              style={{ paddingLeft: 8 + indent * 14 }}
            >
              <TaskStatusBadge status={row.status} compact />
              <span className="mini-tree-title truncate">{row.task.title || shortId(row.id)}</span>
              <span className="mini-tree-id mono" title={row.id}>
                {shortId(row.id)}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
