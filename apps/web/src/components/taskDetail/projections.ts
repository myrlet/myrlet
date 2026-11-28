/**
 * React-free projections for the Task Detail panel.
 *
 * These are plain transformations over Hub facts (ancestry, deterministic
 * ordering, and the bounded Context Mini-Tree), kept apart from the views so
 * they can be unit-tested without a DOM. They never invent a relationship and
 * never mutate the records they read.
 */

import type { TaskRecord, TaskTreeResponse } from "../../api/types";
import { indexTasks } from "../../model/taskIndex";

/** Ancestry is display context; stop rather than walk an unbounded chain. */
export const MAX_ANCESTRY_LENGTH = 12;

/**
 * Root-to-current ancestry resolved against the bounded tree read.
 *
 * A parent missing from the read (truncated or out of range) ends the chain, so
 * the breadcrumb states only relationships the Hub actually returned. Parent
 * cycles are cut by the `seen` guard and the length bound.
 */
export function buildAncestry(
  task: TaskRecord,
  tree: TaskTreeResponse | null,
): TaskRecord[] {
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

  while (current.task.parentTaskId !== null && chain.length < MAX_ANCESTRY_LENGTH) {
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

/** Shallowest depth among the projected rows (`0` when there is nothing). */
export function minimumDepth(rows: readonly TaskRecord[]): number {
  let min = rows.length > 0 ? rows[0].depth : 0;
  for (const row of rows) {
    if (row.depth < min) {
      min = row.depth;
    }
  }
  return min;
}

/**
 * Depth-first mini tree over the bounded Task tree read.
 *
 * Tasks whose parent is absent from the read become roots; sibling order is
 * deterministic; `visited` plus the trailing sweep guarantee each Task appears
 * exactly once even if the read contains a parent cycle.
 */
export function miniTreeRows(tree: TaskTreeResponse): TaskRecord[] {
  const { roots, childrenByParent } = indexTasks(tree.tasks);

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
