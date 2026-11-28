/**
 * Group flat visible tree rows into the List's Task Group sections.
 *
 * This is pure list structure, separate from row presentation: a multi-node
 * Task Group becomes one slim divider plus its rows, and every independent root
 * Task collects into one trailing Standalone section that needs no divider.
 * The divider summary intentionally omits the root title — the first row already
 * shows it, and repeating it on every divider is noise.
 */

import { formatCount } from "../format";
import type { TreeRow } from "../model/taskTree";

export interface TreeGroupBucket {
  readonly key: string;
  readonly kind: "tree" | "standalone";
  readonly summary: string;
  readonly rows: TreeRow[];
}

export function groupRowsForOutline(rows: readonly TreeRow[]): TreeGroupBucket[] {
  const groups: TreeGroupBucket[] = [];
  const standaloneRows: TreeRow[] = [];

  let index = 0;
  while (index < rows.length) {
    const root = rows[index];
    const slice: TreeRow[] = [root];
    index += 1;
    while (index < rows.length && rows[index].depth > 0) {
      slice.push(rows[index]);
      index += 1;
    }

    const descendantCount = Math.max(0, root.task.subtree.total - 1);
    const isTree = root.hasChildren || descendantCount > 0 || slice.length > 1;
    if (isTree) {
      const finishedDescendants = Math.max(
        0,
        root.task.subtree.finished - (root.status === "finished" ? 1 : 0),
      );
      const childTotal = descendantCount || root.childCount;
      const parts = [
        `${formatCount(childTotal)} ${childTotal === 1 ? "subtask" : "subtasks"}`,
      ];
      if (root.task.subtree.queued > 0) {
        parts.push(`${formatCount(root.task.subtree.queued)} queued`);
      }
      if (root.task.subtree.dispatched > 0) {
        parts.push(`${formatCount(root.task.subtree.dispatched)} dispatched`);
      }
      if (finishedDescendants > 0) {
        parts.push(`${formatCount(finishedDescendants)} ended`);
      }
      groups.push({
        key: root.id,
        kind: "tree",
        summary: parts.join(" · "),
        rows: slice,
      });
    } else {
      standaloneRows.push(root);
    }
  }

  if (standaloneRows.length > 0) {
    groups.push({
      key: "__standalone__",
      kind: "standalone",
      summary: "",
      rows: standaloneRows,
    });
  }

  return groups;
}
