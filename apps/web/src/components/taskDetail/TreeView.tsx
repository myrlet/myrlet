/**
 * Tree & Deps tab: source provenance, sibling dependencies resolved against
 * the bounded tree read, and the indented Context Mini-Tree.
 *
 * A dependency or relative the read did not return is stated as absent — never
 * fabricated — and a truncated read says so.
 */

import { useMemo } from "react";

import type { TaskRecord, TaskTreeResponse } from "../../api/types";
import { TaskStatusBadge } from "../StatusBadge";
import { shortId } from "./formatIds";
import { miniTreeRows, minimumDepth } from "./projections";

export interface TreeViewProps {
  readonly task: TaskRecord;
  /** Bounded root-scoped tree read; `null` while it has not loaded. */
  readonly tree: TaskTreeResponse | null;
  readonly onSelectTask: (taskId: string) => void;
}

export function TreeView({ task, tree, onSelectTask }: TreeViewProps) {
  const sourceTaskId = task.task.sourceTaskId;
  const taskById = useMemo(
    () => new Map((tree?.tasks ?? []).map((candidate) => [candidate.id, candidate])),
    [tree],
  );
  const miniRows = useMemo(() => (tree === null ? [] : miniTreeRows(tree)), [tree]);
  const minMiniDepth = useMemo(() => minimumDepth(miniRows), [miniRows]);

  return (
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
        {task.task.dependencies.length === 0 ? (
          <p className="detail-empty">No sibling dependencies declared.</p>
        ) : (
          <ul className="dep-list">
            {task.task.dependencies.map((dependencyId) => {
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
        <MiniTree
          rows={miniRows}
          minDepth={minMiniDepth}
          currentTaskId={task.id}
          onSelectTask={onSelectTask}
        />
      </section>
    </>
  );
}

/** Interactive indented context tree over the bounded tree read. */
function MiniTree({
  rows,
  minDepth,
  currentTaskId,
  onSelectTask,
}: {
  rows: readonly TaskRecord[];
  minDepth: number;
  currentTaskId: string;
  onSelectTask: (taskId: string) => void;
}) {
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
