/**
 * Live SSE Audit Stream — the Hub event feed, presented honestly.
 *
 * SSE frames are invalidation hints, never authoritative state: every frame is
 * shown with its raw payload and a footer says so explicitly. The inspector
 * offers `Open Task ↗` when the payload names a Task and a copyable JSON view.
 */

import { useEffect, useMemo, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import type { HubEventEnvelope } from "../api/types";
import { formatRelativeTime, formatTimestamp } from "../format";
import type { ConnectionStatus } from "../state/useHub";

type KindFilter = "all" | "task" | "delivery" | "worker";

const KIND_FILTERS: readonly { readonly value: KindFilter; readonly label: string }[] = [
  { value: "all", label: "All" },
  { value: "task", label: "task.*" },
  { value: "delivery", label: "delivery.*" },
  { value: "worker", label: "worker.*" },
];

/* --------------------------------------------------------------------- */
/* Payload facts                                                          */
/* --------------------------------------------------------------------- */

interface EventFacts {
  readonly taskId: string | null;
  readonly workerId: string | null;
  readonly deliveryId: string | null;
  readonly summary: string | null;
}

const EMPTY_FACTS: EventFacts = {
  taskId: null,
  workerId: null,
  deliveryId: null,
  summary: null,
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
}

function readString(record: Record<string, unknown>, keys: readonly string[]): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim() !== "") {
      return value;
    }
  }
  return null;
}

function readFacts(payload: unknown): EventFacts {
  const record = asRecord(payload);
  if (!record) {
    return EMPTY_FACTS;
  }
  return {
    taskId: readString(record, ["taskId", "task_id"]),
    workerId: readString(record, ["workerId", "worker_id"]),
    deliveryId: readString(record, ["deliveryId", "delivery_id"]),
    summary: readString(record, ["summary", "message", "text", "status", "reason"]),
  };
}

function subjectOf(facts: EventFacts): string | null {
  return facts.taskId ?? facts.workerId ?? facts.deliveryId;
}

function stringifyPayload(payload: unknown): string {
  try {
    const text = JSON.stringify(payload, null, 2);
    return text === undefined ? "null" : text;
  } catch {
    return "null";
  }
}

/* --------------------------------------------------------------------- */
/* Lightweight JSON syntax tinting                                        */
/* --------------------------------------------------------------------- */

const JSON_TOKEN =
  /("(?:\\.|[^"\\])*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b/g;

function tintJson(json: string): ReactNode[] {
  JSON_TOKEN.lastIndex = 0;
  const nodes: ReactNode[] = [];
  let cursor = 0;
  let tokenIndex = 0;
  let match = JSON_TOKEN.exec(json);
  while (match !== null) {
    if (match.index > cursor) {
      nodes.push(json.slice(cursor, match.index));
    }
    if (match[1] !== undefined) {
      const isKey = match[2] !== undefined;
      nodes.push(
        <span
          className={isKey ? "json-key" : "json-string"}
          key={`json-${tokenIndex}`}
        >
          {match[1]}
        </span>,
      );
      tokenIndex += 1;
      if (isKey) {
        nodes.push(match[2]);
      }
    } else if (match[3] !== undefined) {
      nodes.push(
        <span className="json-number" key={`json-${tokenIndex}`}>
          {match[3]}
        </span>,
      );
      tokenIndex += 1;
    } else if (match[4] !== undefined) {
      nodes.push(
        <span
          className={match[4] === "null" ? "json-null" : "json-boolean"}
          key={`json-${tokenIndex}`}
        >
          {match[4]}
        </span>,
      );
      tokenIndex += 1;
    }
    cursor = JSON_TOKEN.lastIndex;
    match = JSON_TOKEN.exec(json);
  }
  if (cursor < json.length) {
    nodes.push(json.slice(cursor));
  }
  return nodes;
}

/* --------------------------------------------------------------------- */
/* View                                                                   */
/* --------------------------------------------------------------------- */

export interface EventsViewProps {
  readonly events: readonly HubEventEnvelope[];
  readonly eventCursor: number;
  readonly connection: ConnectionStatus;
  readonly paused: boolean;
  readonly onTogglePaused: () => void;
  readonly onClear: () => void;
  readonly onOpenTask: (taskId: string) => void;
  readonly onCopyText: (label: string, text: string) => void;
}

export function EventsView(props: EventsViewProps) {
  const { events, eventCursor, connection, paused, onTogglePaused, onClear, onOpenTask, onCopyText } =
    props;

  const [kind, setKind] = useState<KindFilter>("all");
  const [query, setQuery] = useState("");
  const [selectedSeq, setSelectedSeq] = useState<number | null>(null);

  const trimmedQuery = query.trim().toLowerCase();

  const filtered = useMemo(
    () =>
      events.filter((event) => {
        if (kind !== "all" && !event.kind.startsWith(`${kind}.`)) {
          return false;
        }
        if (trimmedQuery === "") {
          return true;
        }
        const facts = readFacts(event.payload);
        const haystack = [
          String(event.seq),
          event.kind,
          facts.taskId ?? "",
          facts.workerId ?? "",
          facts.deliveryId ?? "",
          facts.summary ?? "",
        ]
          .join(" ")
          .toLowerCase();
        return haystack.includes(trimmedQuery);
      }),
    [events, kind, trimmedQuery],
  );

  // Select the newest frame once, so the inspector is useful without a click.
  useEffect(() => {
    if (selectedSeq === null && events.length > 0) {
      setSelectedSeq(events[events.length - 1]?.seq ?? null);
    }
  }, [events, selectedSeq]);

  const selected = useMemo(
    () => events.find((event) => event.seq === selectedSeq) ?? null,
    [events, selectedSeq],
  );

  const selectedFacts = selected ? readFacts(selected.payload) : EMPTY_FACTS;
  const selectedJson = selected ? stringifyPayload(selected.payload) : "";

  const connectionLabel =
    connection === "connected"
      ? "Connected"
      : connection === "connecting"
        ? "Connecting"
        : connection === "reconnecting"
          ? "Reconnecting"
          : "Disconnected";

  return (
    <section className="events-view" aria-label="Live event stream">
      <div className="events-toolbar">
        <div className="filter-pills" role="group" aria-label="Filter events by kind">
          {KIND_FILTERS.map((option) => (
            <button
              key={option.value}
              type="button"
              className="filter-pill"
              data-active={kind === option.value}
              aria-pressed={kind === option.value}
              onClick={() => setKind(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
        <input
          className="input view-search"
          type="search"
          data-view-search
          aria-label="Search events"
          placeholder="Search kind, subject, or summary…"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <span className="connection-pill" data-state={paused ? "offline" : connection}>
          <span className="connection-dot" aria-hidden="true" />
          {paused ? "Paused" : connectionLabel}
        </span>
        <span className="mono view-subtitle">#{eventCursor}</span>
        <button type="button" className="button button-sm" onClick={onTogglePaused}>
          {paused ? "Resume" : "Pause"}
        </button>
        <button type="button" className="button button-sm" onClick={onClear}>
          Clear
        </button>
      </div>

      <div className="events-created">
        <span className="muted">
          {filtered.length === events.length
            ? `${events.length} ${events.length === 1 ? "event" : "events"}`
            : `${filtered.length} of ${events.length} events`}
        </span>
      </div>

      <div className="events-split">
        <div className="events-table-wrap">
          {events.length === 0 ? (
            <div className="pane-empty">
              <span className="pane-empty-title">No live events yet</span>
              <span>
                Live Hub events appear here as state transitions occur. SSE frames are invalidation
                hints; authoritative state comes from Hub Snapshot.
              </span>
            </div>
          ) : filtered.length === 0 ? (
            <div className="pane-empty">
              <span className="pane-empty-title">No matching events</span>
              <span>No frame matches the current kind filter or search.</span>
            </div>
          ) : (
            <table className="event-table">
              <caption className="visually-hidden">
                Live Hub event stream: sequence, time, kind, subject, and summary
              </caption>
              <thead>
                <tr>
                  <th scope="col">Seq</th>
                  <th scope="col">Time</th>
                  <th scope="col">Kind</th>
                  <th scope="col">Subject</th>
                  <th scope="col">Summary</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((event) => {
                  const facts = readFacts(event.payload);
                  const subject = subjectOf(facts);
                  const isSelected = event.seq === selectedSeq;
                  const onRowKeyDown = (keyEvent: KeyboardEvent<HTMLTableRowElement>): void => {
                    if (keyEvent.key === "Enter" || keyEvent.key === " ") {
                      keyEvent.preventDefault();
                      setSelectedSeq(event.seq);
                    }
                  };
                  return (
                    <tr
                      key={event.seq}
                      className="event-row"
                      data-selected={isSelected}
                      aria-selected={isSelected}
                      tabIndex={0}
                      onClick={() => setSelectedSeq(event.seq)}
                      onKeyDown={onRowKeyDown}
                    >
                      <td className="tabular">{event.seq}</td>
                      <td className="mono" title={formatTimestamp(event.createdAtMs)}>
                        {formatRelativeTime(event.createdAtMs)}
                      </td>
                      <td className="mono">{event.kind}</td>
                      <td className="mono">{subject ?? <span className="cell-empty">—</span>}</td>
                      <td className="cell-muted">
                        {facts.summary ?? <span className="cell-empty">—</span>}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>

        <aside className="event-inspector" aria-label="Event payload inspector">
          {selected === null ? (
            <div className="pane-empty">
              <span className="pane-empty-title">No event selected</span>
              <span>Select a frame to inspect its payload.</span>
            </div>
          ) : (
            <>
              <header className="event-inspector-header">
                <h2 className="event-inspector-title">Event #{selected.seq}</h2>
                {selectedFacts.taskId ? (
                  <button
                    type="button"
                    className="button button-sm"
                    onClick={() => onOpenTask(selectedFacts.taskId ?? "")}
                  >
                    Open Task ↗
                  </button>
                ) : null}
              </header>

              <div className="detail-grid">
                <span className="detail-label">Seq</span>
                <span className="detail-value tabular">{selected.seq}</span>

                <span className="detail-label">Kind</span>
                <span className="detail-value mono">{selected.kind}</span>

                <span className="detail-label">Time</span>
                <span className="detail-value mono">{formatTimestamp(selected.createdAtMs)}</span>

                <span className="detail-label">Task</span>
                <span className="detail-value mono">
                  {selectedFacts.taskId ?? <span className="cell-empty">—</span>}
                </span>

                <span className="detail-label">Worker</span>
                <span className="detail-value mono">
                  {selectedFacts.workerId ?? <span className="cell-empty">—</span>}
                </span>

                <span className="detail-label">Delivery</span>
                <span className="detail-value mono">
                  {selectedFacts.deliveryId ?? <span className="cell-empty">—</span>}
                </span>
              </div>

              <div className="event-json-header">
                <span className="field-label">Payload</span>
                <button
                  type="button"
                  className="button button-sm"
                  onClick={() => onCopyText("event JSON", selectedJson)}
                >
                  Copy JSON
                </button>
              </div>
              <pre className="json-view code-block">{tintJson(selectedJson)}</pre>

              <p className="detail-empty event-footer-note">
                SSE frames are invalidation hints; authoritative state comes from Hub Snapshot.
              </p>
            </>
          )}
        </aside>
      </div>
    </section>
  );
}
